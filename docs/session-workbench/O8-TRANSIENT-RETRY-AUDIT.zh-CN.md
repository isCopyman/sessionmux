# O8 设计审计：5xx / 临时错误打断回合后的自动重试与可恢复态

日期：2026-08-21  
工作树：`wt/o8-audit`（只读；本文件是唯一写入）  
范围：`session/prompt` 失败 → turn 落库 → 前端红条；零内容窗口；重发入口 vs O43 五层归属；瞬时错误判别；可恢复态；A/B 两步提案。

**铁律遵守**：未改任何产品代码。结论均来自本工作树当前源码的 `file:line`。读不到的标「未能确定」。

---

## 0. 先给结论（读代码后的判断，不是提案口号）

实测「API Error: 503 No available accounts」**大概率不走** `turn_failed_empty*` / `turn_failed_refusal` / `turn_failed_max_tokens` / `turn_failed_unknown` 家族。

那一族只覆盖 **ACP `StopReason`（加 codeg 合成的 `"empty"`）**。`session/prompt` **JSON-RPC 失败**走另一条通道：`prompt_result?` 把 `sacp::Error` 直接顶出 `run_conversation_loop`，连接任务把它变成 **`terminal: true` 的 `AcpEvent::Error` + `Disconnected`**，会话行 CAS 成 `Cancelled`。这与「回合直接终止、红条、系统不重试、闲置十分钟才被人发现」高度吻合。

仓库里**已经有**适配器内部重试的三套表面，codeg **没有**对 `session/prompt` RPC 失败做任何重试：

| 表面 | 谁在重试 | 失败后连接 |
|---|---|---|
| Claude `_claude/sdkMessage` `system/api_retry` | Claude CLI 自己 | 回合还活着，琥珀色/红色重试条 |
| JetBrains AIR `sessionFailure`（claude-agent-acp ≥0.67 / codex-acp ≥1.2） | 适配器自己（警告=重试中，error=放弃） | 连接活着；error 条带 `retry` 按钮 |
| Codex `_meta.codex.error` `willRetry==true` | Codex 自己 | 回合还活着，`TurnRetrying` |
| **`session/prompt` JSON-RPC Err** | **无人** | **连接死，terminal Error** |

O8 真正缺的不是「再做一套 AIR」，而是：**RPC 失败这条通道把一次 503 升级成了连接死亡**。自动重试（A）和可恢复态（B）都应该先把这条通道从「杀连接」改成「回合失败、连接仍在」。

**不该在未拍板时做的事**：给 `ConversationStatus` 加新枚举（要 migration + 所有读状态的引擎）；在 `send_prompt_inner` 层重放同一条用户消息（会撞 O43、会再广播一条 `UserMessage`）。

---

## 1. 错误在哪落地

### 1.1 正常回合怎么进 `session/prompt`

UI / host / 队列 → `ConnectionManager::send_prompt_linked_with_message_id`（`manager.rs:1359`）→ `send_prompt_inner`（`manager.rs:1101-1174`）把 `ConnectionCommand::Prompt` 送进连接循环。

连接循环 `run_conversation_loop` 收到 `Prompt`（`connection.rs:7708`）：

1. `prompt_ledger.record_prompt_blocks`（`7717`）
2. `AcpEvent::StatusChanged { Prompting }`（`7773-7779`）——同时清 `SessionState.last_error`（`session_state.rs:627-631`）
3. `AcpEvent::UserMessage`（`7791-7793`）——写入 `pending_user_message`
4. `record_prompt`（`7819`，实现 `710-724`）——codeg 自己的 transcript，**在 RPC 发出之前**就落盘
5. `PromptRequest::new` + `cx.send_request_to(Agent, prompt_request).block_task()`（`7821-7835`）
6. `TurnOutputProbe::new(stderr_mark)`（`7849`）
7. `select!`：流式 `session/update` **和** prompt 响应 **并行**（`7889-8173`）

`send_prompt_inner` 在入队前把 `turn_in_flight = true`（`manager.rs:1157`）。这个旗只在 `TurnComplete` 时清掉（`session_state.rs:937-941`）。注释写明：连接死亡会连状态一起丢掉，所以旗不会残留（`session_state.rs:937-940`）。

### 1.2 通道 A（最像这次事故）：`session/prompt` JSON-RPC 失败 → 杀连接

`select!` 的 prompt 臂：

```
prompt_result = &mut prompt_response => {
    let response = prompt_result?;   // connection.rs:8075-8076
```

`?` 的含义：`Err(sacp::Error)` **立刻返回** `run_conversation_loop`，**不**发 `TurnComplete`，**不**跑 `turn_failure_error_event`，**不** `record_turn_end`。

之后：

| 跳 | 函数 | 行 | 做什么 |
|---|---|---|---|
| 1 | `run_conversation_loop` | `8076` | `prompt_result?` 把 RPC 错误顶出去 |
| 2 | `handle_fork_or_exit` | `6945-6948` | `Err(e) => return Err(e)`，原样上抛 |
| 3 | `run_connection` 尾部 | `5253-5270` | `sacp::Error` → `AcpError::protocol(raw)`（`Protocol` **没有**稳定 `code()`，见 `error.rs:151`） |
| 4 | `spawn_agent_connection` 线程 | `1576-1616` | `AcpEvent::Error { message: e.to_string(), code: e.code() /* None */, terminal: true }` → `StatusChanged { Error }` → `StatusChanged { Disconnected }` |
| 5 | `SessionState::apply_event` | `1047-1062` | 写入 `last_error`（仅内存快照，不入 conversation 表） |
| 6 | `emit_with_state` | `event_bridge.rs:443-484` | `apply_event` + `event_seq++` + 连接广播 + 进程内 bus |
| 7 | lifecycle worker | `lifecycle.rs:42-48, 199-225, 266-274` | `terminal: true` → `update_status_if(InProgress → Cancelled)` + `ConversationStatusChanged` |
| 8 | 前端 `case "error"` | `acp-connections-context.tsx:3956-4085` | `code` 为 `None` → `default` 显示 **原始英文** `e.message`（`4031-4032`） |
| 9 | 前端 ERROR reducer | `2393-2402` | `conn.error = message` |
| 10 | 作曲家红条 | `conversation-shell.tsx:370-374` | `{error && <div className="… text-destructive …">{error}</div>}` |
| 11 | 状态栏 alert | 同 `4056-4062` `pushAlert("error", …)` | 另一条红条 |
| 12 | 连接线程结束 | `connection.rs:1528-1531, 1617-1619` | `ConnectionCleanupGuard` 从 manager map 删条目 |

**503 落进哪一类？** 不进 `turn_failed_*`。`AcpError::Protocol` 的 `code()` 是 `None`（`error.rs:132-152`）。前端因此把 agent 原文（例如 `API Error: 503 No available accounts`）直接画成红条。

同一条路径上，**回合状态机的落库**是：

- 发送时已经 `InProgress`（`manager.rs:1556-1566`）
- RPC 失败**没有** `TurnComplete`，所以 lifecycle 的 `TurnComplete` 臂（`lifecycle.rs:141-170`）**不会**跑
- 真正改库的是 terminal Error 的 CAS：`InProgress → Cancelled`（`lifecycle.rs:207-212`）

`ConversationStatus` 只有四态：`InProgress / PendingReview / Completed / Cancelled`（`conversation.rs:17-26`）。没有「被临时错误打断、可恢复」。

### 1.3 通道 B：ACP `StopReason` → `turn_failed_*`（连接活着）

两条对称出口，都先走纯函数 `finish_turn_reason`（`7540-7551`）再 `turn_failure_error_event`（`7563-7615`）：

- 流式 `SessionMessage::StopReason`（`7990-8070`）——**故意不** `record_turn_end`（`7537-7538, 8005-8007`）
- prompt **成功响应**里的 `response.stop_reason`（`8075-8172`）——**会** `record_turn_end`（`8141-8147`）

`turn_failure_error_event` 映射（`7568-7600`）：

| `StopReason` / 合成 | `code` | `terminal` |
|---|---|---|
| `refusal` | `turn_failed_refusal` | `false` |
| `max_tokens` | `turn_failed_max_tokens` | `false` |
| `max_turn_requests` | `turn_failed_max_turn_requests` | `false` |
| `unknown` | `turn_failed_unknown` | `false` |
| `empty`（`EndTurn` 且 `!saw_agent_output`） | `turn_failed_empty` / `_protocol` / `_metadata` | `false` |
| `end_turn` / `cancelled` / 其它 | **不发 Error** | — |

注释（`7557-7559`）写明：OpenCode 一类会把网关错误映射成 `Refusal`。所以 **如果** 某 adapter 把 503 做成 `StopReason::Refusal`，会进 `turn_failed_refusal`，前端显示本地化「拒绝继续」而不是原文 503（`acp-connections-context.tsx:3993-3996`）。

这两条出口随后：

1. `AcpEvent::Error { terminal: false }`（`7602-7614`）——注释：连接继续活、broker 靠 `complete_call` 收尾
2. `AcpEvent::TurnComplete { stop_reason }`（`8033-8042` / `8152-8161`）
3. `SessionState` 清 `live_message`、`turn_in_flight=false`、`status=Connected`（`session_state.rs:845-953`）
4. 循环末尾再发一次 `StatusChanged { Connected }`（`connection.rs:8466-8473`）
5. lifecycle：`refusal|max_tokens|max_turn_requests|unknown|empty` → 行状态 `Cancelled`（`lifecycle.rs:141-148`）
6. 前端 `turn_complete` 把 status 设回 `connected`（`acp-connections-context.tsx:3883-3908`）；`error` 事件仍把 `conn.error` 写成红条

**503 会不会进这一族？** 只有 adapter 把它做成 `StopReason` 才会。Claude Code 的「API Error: 503 No available accounts」在本仓库**没有**被映射到这些 `code` 的证据。更像通道 A 或通道 C。

### 1.4 通道 C：AIR 终端失败（连接活着，伪装 `end_turn`）

codeg 向 Claude/Codex 广告 `clientCapabilities._meta.jetbrains.air`（`connection.rs:3244-3288`）。

适配器约定（`9434-9449`）：

- 重试中的警告走 `session_info_update._meta`（`11183-11186` → `AcpEvent::SessionFailure`）
- **终端失败**骑在 **prompt 响应 `_meta`** 上（`response_session_failure`，`9450-9461`）
- claude-agent-acp 的 `failActiveWithSessionFailure` **用伪装的 `end_turn` 结束回合**，避免被合成 `"empty"`

codeg 在 `8077-8124` 显式跳过 empty 诊断，若 `_meta` 里有 `severity == "error"` 的 AIR 记录。然后：

1. `AcpEvent::SessionFailure`（`8088-8093`）——**在** `TurnComplete` 之前
2. `TurnComplete { stop_reason: "end_turn" }`（因为伪装）
3. lifecycle 把行写成 **`PendingReview`**（`lifecycle.rs:143`）——这是成功语义，不是失败
4. 前端 `SessionFailureBanner` 画破坏性红条，带 `retry / login / new_session`（`session-failure-banner.tsx:7-12, 186-190`）
5. `retry` 把**上一条用户文本**重新丢进消息队列（`conversation-detail-panel.tsx:1955-1988`，`lastUserPromptText` 在 `src/lib/session-failures.ts:258-275`）

字段报告 2026-08-15（`9445-9449`）：claude 0.68.0 中途断网先发「Reconnecting to Claude, attempt N of 5」，终端 `transport_lost` **只**出现在 prompt 响应 `_meta`。说明 **Claude 适配器自己已经会重试**；它放弃之后 codeg 不会再重试。

**未能确定**：这次「No available accounts」是否被 claude-agent-acp 包进 AIR。若包了，用户看到的应是 AIR 条（带 Retry 按钮），不是 `conn.error` 原文红条。事故描述更像通道 A。

### 1.5 通道 D：Claude `api_retry` 扩展通知（回合还活着）

`_claude/sdkMessage` + `type=system, subtype=api_retry`（`connection.rs:10001-10028`）→ `AcpEvent::ClaudeSdkMessage` → 前端 `CLAUDE_API_RETRY`（`acp-connections-context.tsx:3863-3881`）→ `conversation-shell.tsx:359-368` 底部旋转红条。

这是 **Claude CLI 自己的重试**。它停了之后，若 CLI 再把失败做成 RPC error，就会掉进通道 A。

### 1.6 通道 E：流式 assistant 文本里的「API Error: 503…」

`AgentMessageChunk` 算 agent 输出（`is_agent_output_update`，`7227-7234`）。若 CLI 把 503 写成一条助手消息然后 `EndTurn`：

- `saw_agent_output = true` → **不**合成 empty
- `turn_failure_error_event("end_turn")` 返回 `None`（`14198`）
- 用户在 transcript 里看到错误文本，**没有** `conn.error` 红条
- 行状态变成 `PendingReview`

事故是「红条」不是「助手说了一句 503」，所以不太像这条。但自动重试的零内容窗口必须把它排除：已经有内容块了。

### 1.7 旁路：Grok compact 503（不是这次）

Grok `_x.ai/session_notification` `auto_compact_failed` + `"API error (status 503)"` 映射成 **非终端** `AcpEvent::Error`（`13321-13341`）。与协调者回合的 `session/prompt` 无关。

### 1.8 `turn_failed_*` 家族对 503 的位置

`turn_failed_empty*` 只在「adapter 声称 `EndTurn` 且 codeg 没看到 agent 输出」时合成（`7540-7551, 7412-7467`）。503 作为 RPC 错误或 AIR 终端失败都**不会**进这一族。只有 503 被压成 `StopReason::Unknown`/`Refusal`，或空回合 + stderr 里碰巧有 503，才会间接沾边。stderr 只进 empty 的 `details`（`7482-7514`），**不**改 `code`。

---

## 2. 「零内容窗口」可判定吗

**可以，而且已经有专用探针。** 它是 turn-scoped、不落库、不进快照。

### 2.1 首选：`TurnOutputProbe`（当前回合、还在循环里）

定义 `connection.rs:7354-7377`：

| 字段 | 含义 |
|---|---|
| `saw_agent_output: bool` | 见过回复文本 / thinking / tool call |
| `saw_metadata_update: bool` | 只见过 plan / mode / usage / user echo 等 |
| `dropped_decode` / `dropped_dispatch` | 解析失败的 update |
| `stderr_mark` | 本回合 stderr 水位 |

`is_agent_output_update`（`7227-7234`）把这些算成「有内容」：

- `AgentMessageChunk`
- `AgentThoughtChunk`
- `ToolCall` / `ToolCallUpdate`

**不算内容**：`UserMessageChunk`、`Plan`、`*ModeUpdate`、`ConfigOptionUpdate`、`SessionInfoUpdate`、`AvailableCommandsUpdate`、`UsageUpdate`。单测钉死 Plan 不是 agent 输出（`14022-14031`）。

Grok 扩展通知若被当成回合输出，会直接 `probe.saw_agent_output = true`（`7923-7925`）。

`diagnose_empty_turn`（`7460-7467`）：

- 有 drop → `ProtocolMismatch`（**不要**自动重试：可能已经有输出，只是 codeg 读不了）
- 否则有 metadata → `MetadataOnly`（注释 `7416-7417`：**不是**无害证明）
- 否则 → `NoOutput`

**安全自动重试的谓词（循环内、`TurnComplete` 之前）**：

```
!probe.saw_agent_output && probe.dropped_total() == 0
```

`MetadataOnly` 是否允许重试需要拍板：一次 usage_update 之后的 503 仍可能是「模型还没吐字」。偏保守就要求 `!saw_metadata_update`（即严格 `EmptyTurnCause::NoOutput`）。

### 2.2 回合结束后：探针已经没了

`TurnComplete` 清空 `live_message`（`session_state.rs:929`），从终局文本填 `last_assistant_text`（`894-928`）。

| 信号 | 位置 | 能否当零内容窗口 |
|---|---|---|
| `SessionState.live_message` | 回合中累积；完成后 `None` | 循环内可以；完成后不行 |
| `last_assistant_text` | 完成后的主线程文本 | 空 ≠ 没内容（可能只打了 tool） |
| `last_turn_ended_abnormally` | `session_state.rs:467-479, 863` | 只说 stop_reason ≠ `end_turn`，不说有没有块 |
| `last_error` | `293-302, 1047-1062` | 有错误，不计量 |
| `pending_user_message` | 回合中的用户条 | 与助手内容无关 |
| AIR `session_failures` | `416-424` | 适配器视角，不是块计数 |

**结论**：零内容窗口**只在** `run_conversation_loop` 的 turn `select!` 里、`TurnComplete` 之前可靠。A 必须做在 `prompt_result` 臂，不能做在 `send_prompt_inner` 之后的「再发一条」。

通道 A 今天用 `?` 直接返回，探针还在作用域里但被丢掉了。改 `?` 为 `match` 就可以读 `probe.saw_agent_output`。

---

## 3. 重发入口 vs O43 五层归属

O43 结案原文（`docs/session-workbench/DOGFOODING-LOG-2026-08-20.zh-CN.md:626-631`；行号已漂移，下面是 **本树当前** 对应点）：

| 层 | 结案说法 | 当前代码 |
|---|---|---|
| ① | fork 后绑定切换前排干 in-flight turn | `fork_session` 在 `turn_in_flight` 时 **直接拒** `TurnInProgress`（`manager.rs:2054-2068`），不是 sleep-wait。fork 成功后 `handle_fork_or_exit` 在激活屏障上等 C2 落库（`6962-6998`），再 `ConversationForked` 改绑 `conversation_id`（`session_state.rs:1009-1017`），**然后**才 `SessionStarted` |
| ② | 连接声称的会话 ≠ DB 绑定则拒写 | `send_prompt_linked_with_message_id`：`requested != linked` 拒（`manager.rs:1430-1436`）。`persist_fork_outcome`：`external_id != original_session_id` 拒（`2289-2293`） |
| ③ | 事件写入以 DB 绑定为准 | `emit_with_state` 用 `state.connection_id`（`event_bridge.rs:477-480`）。lifecycle `TurnComplete` 用 `state.conversation_id`（`lifecycle.rs:157`），不用事件里 agent 自报的 session 去改行 |
| ④ | fork 交接期消息进队列不进旧会话 | 过期 C1 视图发到已绑 C2 的连接会被 ② 挡住。fork 全程握着 `prompt_lock`（`2019-2020, 2087-2089`），并发 `send_prompt_linked` 会排在后面。C2 **不继承** live Turn/queue/mailbox（`2203-2205`）。前端 `TurnBusyError` → 消息队列 |
| ⑤ | 回归测试钉交接窗口 | `send_prompt_linked_rejects_a_stale_c1_view_after_fork_handoff`（`4981-5020`）；`fork_session_rejects_when_turn_in_flight`（`4676`） |

### 3.1 最干净的无 UI 重发：不要走 `send_prompt_inner`

`send_prompt_inner`（`manager.rs:1101`）是「新回合」入口：设 `turn_in_flight`、可选再广播 `UserMessage`、再 `record_prompt`、再一次 `session/prompt`。AIR 条上的 Retry 走的就是这条（经 `mqEnqueue`）。

**同一 prompt、同一回合**的干净注入点是 `connection.rs:8075` 的 `prompt_result` 臂：不要 `?`，在 `!probe.saw_agent_output` 且判别为瞬时错误时，backoff，再 `cx.send_request_to(Agent, PromptRequest::new(sid.clone(), prompt_blocks.clone()))`。

这样：

- 不再发 `UserMessage`（已经在 `7791` 发过）
- 不再 `record_prompt`（已经在 `7819` 写过）
- 不碰 `send_prompt_linked` 的身份核对 / InProgress 翻转 / 队列准入
- `turn_in_flight` 保持 true → ① 继续拒 fork
- 不进 `prompt_lock`（那把锁在 enqueue 后就放了）

`prompt_blocks` 在 `7748` 已经物化，内层循环里还活着。

### 3.2 若走 `send_prompt_inner` 会碰到哪几层

那是 **新回合**，五层都会再跑一遍：

- ①：要等当前回合 `TurnComplete` 清 `turn_in_flight`，否则 `TurnInProgress`。若与 fork 交错：fork 握 `prompt_lock` 期间 send 会阻塞；锁放下后 `conversation_id` 可能已经是 C2 → 原 prompt 打到 **分叉会话**
- ②：带着旧 `conversation_id` 会在 `1430-1436` 被拒（这是防护，不是安全重试）
- ③：新事件打在 **当前** `state.conversation_id` 上
- ④：交接期应进队列；自动重试若绕过队列就会插队
- ⑤：现有测试覆盖的是拒写，不是「自动重试 + fork」

**fork 交接期遇到重试会怎样**

| 重试落点 | 行为 |
|---|---|
| 循环内、`turn_in_flight` 仍 true | `fork_session` 返回 `TurnInProgress`（`2062-2068`）。中途 `Fork` 命令不在 in-turn handler 里（in-turn 只处理 permission/mode/config/steer/cancel/disconnect；未知命令 `_ => {}`，`8453`）。fork 进不了循环 |
| `TurnComplete` 之后、`send_prompt_inner` | 普通新回合。若 fork 已把 state 绑到 C2，旧会话 id 被 ② 拒绝；不带 id 则会打到 C2 |
| 通道 A 今天（杀连接） | fork 目标连接已经没了 |

激活屏障（`6962-6998`）只在 **已经发出的** `session/fork` 协议成功之后。重试不会走进这条路径，除非有人在 `turn_in_flight` 仍为 true 时把 `Fork` 塞进 channel——`fork_session` 的门就是拦这个的。

### 3.3 循环内重发的剩余风险（O43 以外）

1. **Agent 侧历史可能已经记下这条 user 消息。** codeg 的 `record_prompt` 在 RPC 前就写了（`7815-7819`）。Claude SDK 也常先入队 user 再打模型。同一 `session/prompt` 再发一次，agent 会话里可能出现两条相同 user。零内容窗口只能保证 **codeg 没渲染过助手块**，不能保证 **agent 没持久化 prompt**。这是 A 必须拍板的核心风险。
2. AIR/Claude 可能 **已经** 在这一次 `session/prompt` 里重试过 5 次（`9445-9447`）。codeg 再叠 2 次是双重重试。循环内重试必须看到 AIR warning 或 `claudeApiRetry` 就停。
3. `DispatchUncertain`（`error.rs:18-22`）的语义是「可能已经在传输上了，调用方不得默默重放」。那是 `send_prompt_inner` 的 dispatch ack，不是 prompt RPC。循环内重试的是 **已经完成的失败 RPC**，不是 uncertain dispatch。
4. StopReason / 成功响应路径的注释声称非 `end_turn` 要 cascade-cancel 子委托（`8044-8069, 8163-8171`），但这两处 `break` 前 **没有** 看到 `cancel_by_parent` 调用。真正调用在用户 `Cancel`（`8393-8413`）和连接 teardown（`1563-1573`）。**未能确定** 非 cancel 的 turn 失败是否另有间接 drain。循环内重试若底下还挂着子 agent，可能重复委托。

---

## 4. 瞬时错误怎么判别

### 4.1 结构化字段在哪

`sacp::Error`（本仓库当 JSON-RPC 失败用）：

| 字段 | 本仓库用法 | 行 |
|---|---|---|
| `.code: ErrorCode` | `classify_session_load_failure(e.code, &err_str)` | `connection.rs:4995, 7136-7151` |
| `.to_string()` | 用户可见原文 | `4994, 1577-1582` |
| `.data` JSON | Grok `MODEL_SWITCH_INCOMPATIBLE_AGENT` | `2310-2315` |

已知 `ErrorCode` 变体（来自 load-failure 测试，`12266-12337`）：`ResourceNotFound`、`InternalError`（-32603）、`MethodNotFound`、`AuthRequired`。

**没有**一等 HTTP 状态字段。HTTP 只出现在：

- Codex `willRetry` 的 `codexErrorInfo.*.httpStatusCode`（`9515-9532`）→ `TurnRetrying.error_status`
- AIR `category` 字符串：`connection|access|limit|request|service|unknown`（`types.rs:81, registry.rs:420-422`）
- 错误 **文本**（「API Error: 503 …」）

通道 A 的 `AcpError::protocol` 还 `sanitize_protocol_message`（`error.rs:177-195`），去掉 spawn 时间和本地路径，**保留** 503 原文。然后 `code()` 返回 `None`，HTTP 状态到不了前端，除非留在 `message` 里。

AIR 记录（`9389-9431`）：`category`、`severity`、`title`、`details`、`actions`。`title` 是 adapter 转述的用户可见句。**未能确定** claude-agent-acp 是否把「No available accounts」放进 `title` 以及用哪个 `category`。

stderr：`StderrTail` + `probe.stderr_mark`（`7482-7514`）。只有 empty 诊断读它。通道 A 的 `?` 根本不看 stderr。

### 4.2 保守可重试集（建议）

**最佳判定位置**：`connection.rs:8075` 刚拿到 `Err(sacp::Error)` 之后、决定要不要 `?` 上抛之前。这里同时有 `e.code`、`e.to_string()`、`e.data`、`probe`。不要在前端、不要在 `send_prompt_inner`。

保守谓词（全部为真才重试）：

1. `!probe.saw_agent_output && probe.dropped_total() == 0`
2. 没有未 resolve 的 AIR `severity=="warning"`（适配器已经在重试）
3. `e.code` **不是** `AuthRequired` / `MethodNotFound` / `ResourceNotFound`
4. 文本（小写）命中 **任一**：
   - `\b5\d\d\b` 或 `status 503` / `status 502` / `status 529`
   - `overloaded` / `overloaded_error`
   - `rate limit` / `rate_limit` / `too many requests` / `\b429\b`
   - `no available accounts`
   - `temporarily unavailable` / `try again` / `unavailable`
   - `connection reset` / `connection refused` / `timed out` / `timeout` / `transport_lost`
5. 文本 **不**命中：`authentication`、`unauthorized`、`invalid api key`、`permission denied`、`prompt too long`、`context overflow`、`resume rejected`

`InternalError`（-32603）**单独不够**：load-failure 路径里它既表示进程退出，也表示各种内部失败（`7124-7127, 12286-12308`）。必须叠加文本。

AIR 终端记录（通道 C）：适配器已经放弃。`actions` 含 `retry` 只表示 **用户** 可以再发，不表示 codeg 该自动再发一次 `session/prompt`。A **不要**对 AIR `severity=="error"` 自动重试。

Codex `willRetry==false` 已被 `codex_retry_indicator` 丢掉（`9519-9520`），不会画重试条。那是终端错误，同样不要自动重试。

### 4.3 现成分类器，但不能复用当 5xx 探针

`classify_session_load_failure`（`7135-7151`）是 `session/load` 专用（`resource_not_found` / `session_unavailable`）。模式「`ErrorCode` + 文本」可以抄，**映射表不行**。

---

## 5. 可恢复态（B 面）——最小改动集

今天失败回合的「状态」是几层拼出来的，没有一等「被临时错误打断」：

| 层 | 今天 | 文件 |
|---|---|---|
| 连接 | 通道 A：`Error` 然后 `Disconnected` 并删 map 条目。通道 B/C：`Connected` + `last_error` / AIR 表 | `connection.rs:1576-1616`；`session_state.rs:953` |
| 行状态 | A：`Cancelled`（terminal CAS）。B：`Cancelled`。C（伪装 end_turn）：`PendingReview`（看起来像成功） | `lifecycle.rs:141-148, 207-212` |
| 快照 | `last_error: {message, code, details}` 仅内存；新 `Prompting` 清掉（`627-631`） | `session_state.rs:168-181, 1452` |
| 前端 | `conn.error` 红条；AIR 条带按钮；没有「点一下恢复这个回合」除非 AIR `retry` | `conversation-shell.tsx:370-374`；`session-failure-banner.tsx` |

`ConnectionStatus` 已有 `Error`（`types.rs:733-739`），但是 **连接死** 的意思，不是「回合可恢复」。

### 5.1 最小 B（推荐，不改行枚举）

目标：503 不再杀连接；人能看见「临时失败」并点 Retry；行不要假装成功。

1. **`connection.rs:8075`**：`prompt_result` 改为 `match`。瞬时 + 零内容：可选 A 的有限重试；耗尽或不可重试：发 **非终端** `AcpEvent::Error { code: Some("turn_failed_transient"), terminal: false, message, details }`，再 `TurnComplete { stop_reason: "transient" }`，回到空闲循环（已有 `8466-8473` 的 `Connected`）。
2. **`lifecycle.rs:141-148`**：给 `"transient"` 一个明确映射。**不要**掉进 `_ => None`（行会卡在 `InProgress` = O43「回复中」）。没有新枚举的话映射到 `Cancelled`，靠 `last_error.code` 区分「可点 Retry 的取消」和「用户取消」。
3. **`turn_failure_error_event` 或旁路**：新 `code` `turn_failed_transient`，`terminal: false`。
4. **前端** `acp-connections-context.tsx:3966-4033`：本地化该 code；红条旁复用 AIR Retry（`conversation-detail-panel.tsx:1967-1988` 的 `mqEnqueue`）。
5. **i18n**：`backendErrors.turnFailedTransient`（10 种语言，任务完成后按仓库惯例扫）。
6. **测试**：`turn_failure_error_event` 表（`14197-14215`）；lifecycle 对 `"transient"`；前端 code 路由。

**不动**：`ConversationStatus` 枚举、SQLite CHECK、parser、automation/work_task 对四态的解释、AIR 协议。

代价：侧栏仍显示「已取消」，只是红条可点 Retry。可恢复性在 **连接 + last_error**，不在行枚举。刷新后若连接还在，快照会带上 `last_error`（`1452`，hydrate `acp-connections-context.tsx:1495`）。连接被 idle-sweep 掉以后，可恢复性就没了——这是不加 DB 列的上限。

### 5.2 完整 B（要拍板：新行状态）

若产品要的是「闲置十分钟后打开侧栏仍能看出来、点一下继续」：

- `ConversationStatus::Interrupted`（或 `AwaitingRetry`）+ migration CHECK
- lifecycle `"transient"` → 该态
- 前端列表/会话卡标签 + 可点恢复
- `prompt_queue` / `session_dispatcher` / automation / work_task 把该态当「可再发」而不是终态
- 可能要在 conversation 行存 `last_error_code` / `last_error_message`（今天 `last_error` 只在连接快照里）

这是跨状态机改动。**深夜不该做。** 用户已经说了先审计。

### 5.3 B 不该做的

- 用 `PendingReview` 表示 503（通道 C 的伪装 `end_turn` 已经在这么做，会把失败画成待审）
- 让行留在 `InProgress`（O43）
- 为 B 重用 `turn_failed_unknown`（语义是无法识别的 stop reason，不是 HTTP 临时失败）

---

## 6. 两步实施计划

### 步骤 A — 零内容窗口内自动重试 ≤2 次 + 退避 + UI 事件

**建议：有条件做，但不要作为第一刀单独上。** 先把通道 A 从杀连接改成回合失败（A0），再考虑自动重试（A1）。A1 有 agent 历史重复的风险，必须拍板。

#### A0（我认为应该做，仍要拍板「不杀连接」）

把 `prompt_result?` 改成可恢复的回合失败。没有 A0，A1 的重试耗尽仍会拆掉连接，十分钟无人值守的问题还在。

改动文件：

- `src-tauri/src/acp/connection.rs` — `8075` `match`；耗尽/不可重试时发非终端 Error + `TurnComplete`；**不要**把瞬时 RPC 错误返回 `run_conversation_loop`
- `src-tauri/src/acp/lifecycle.rs` — `"transient"` 停止原因
- `src-tauri/src/acp/error.rs` — 不必新 `AcpError` 变体；事件 `code` 就够
- 前端 code 路由 + i18n
- 单测：模拟 `sacp::Error` InternalError + 「API Error: 503 No available accounts」→ 连接仍在、`turn_in_flight` 清掉、行不是卡住的 `InProgress`

风险：

- **未能确定** 所有 adapter 在 `session/prompt` RPC 失败后是否还接受下一次 prompt。Claude 的 AIR 走伪装 `end_turn` 而不是 RPC error，暗示他们不想用 RPC error 表示可恢复失败。A0 可能对「RPC error ⇒ session 已死」的 adapter 有害——下一次 prompt 会再失败，那时再拆连接。需要按 agent 门控，或「再失败一次才拆」。
- 委托 cascade-cancel 注释与代码不一致（§3.3.4）。A0 让连接活着，子 agent 可能比今天（teardown 会 `cancel_by_parent`）挂得更久。

#### A1（自动重试 ≤2，指数退避，UI 事件）

**需要拍板。** 我倾向 **先不要做**，直到有证据表明 503 的 `session/prompt` RPC 失败 **没有** 把 user 消息写入 agent 会话。若 CLI 已经入队 user，静默重发就是双份 prompt。

若拍板做：

改动文件：

- `src-tauri/src/acp/connection.rs` — 在 `select!` 里、同一 `sid` / `prompt_blocks` 上重发 `PromptRequest`；退避（例如 500ms、2000ms）用 `tokio::time::sleep`，**继续**读 `session.read_update()` 以免堵死 permission；次数记在 turn-local 变量，不要进 `SessionState`
- 重试中：`AcpEvent::TurnRetrying { message, error_status }`（`types.rs:324-338`）或 AIR 风格 warning `SessionFailure`。前端已有 Claude 重试条（`conversation-shell.tsx:359-368`）
- **不要**经过 `send_prompt_inner` / UI
- 谓词：§4.2 + 无进行中 AIR warning
- 测试：`connection.rs` 里 `TurnOutputProbe` / `diagnose_empty_turn` 已有（`14020+`）；新增：503 + 零内容 → 恰好 2 次重发然后成功；有 `AgentMessageChunk` 之后的 503 → 0 次重发；`AuthRequired` → 0 次；AIR warning 在飞 → 0 次；fork 在重试中仍收 `TurnInProgress`

风险清单（A1）：

| 风险 | 严重度 | 备注 |
|---|---|---|
| Agent 会话里重复 user 消息 | 高 | 零内容窗口覆盖不了 |
| 与 adapter 内部重试叠加 | 中 | 必须让 AIR/api_retry |
| 拉长 `turn_in_flight`，fork 被拒更久 | 低 | ① 仍成立 |
| 重试中 permission/tool 卡死 | 中 | sleep 时要继续 `read_update` |
| 重试成功但第一次 RPC 其实已经在 agent 侧跑过 | 高 | 双份 tool |
| ProtocolMismatch（读不了输出）被当成空窗口 | 高 | 用 `dropped_total()==0` 排除 |
| 协调者回合重试打到错误会话 | 低 | 循环内不换 `conversation_id` |

#### A 明确不要做

- 不要从 UI 或 `send_prompt_linked` 自动重放（O43 ②④，重复 `UserMessage`）
- 不要对 AIR `severity=="error"` 自动 `session/prompt`（适配器已经放弃；用户有按钮）
- 不要对 `turn_failed_refusal` 自动重试（可能是真正的安全拒绝；OpenCode 把网关错误塞进 Refusal 是 adapter 的事，不应在 codeg 静默绕过）
- 不要动 `ConversationStatus` 枚举

### 步骤 B — 显式可恢复态

**分两级。B-lite 可以跟 A0 一起做。带新枚举的 B-full 不要在这次做，除非用户要「重启应用后仍能看见可恢复」。**

#### B-lite（与 A0 相同的表面，无新枚举）

见 §5.1。文件：

- `connection.rs`（非终端 Error + `TurnComplete transient`）
- `lifecycle.rs`（`transient` → `Cancelled`，或拍板后的新态）
- `acp-connections-context.tsx` + `conversation-shell.tsx` / `session-failure-banner.tsx`（Retry）
- `i18n/messages/*.json`
- 测试：lifecycle、前端 code、`send_prompt` 在 `transient` 之后、连接仍活时成功

#### B-full（要拍板）

见 §5.2。额外：

- `src-tauri/src/db/entities/conversation.rs` + 新 migration
- `src/lib/types.ts` 及所有 `ConversationStatus` 穷尽匹配
- `prompt_queue.rs`、`session_dispatcher.rs`、`automation/engine.rs`、`work_task/engine.rs`
- 可能的 `conversation.last_error_*` 列

风险：漏改一个状态匹配就会把 Interrupted 当终态或当进行中。这就是「深夜不许动回合状态机」。

### 哪一步不该做 / 要拍板

| 项 | 建议 |
|---|---|
| A0：RPC 失败不再杀连接 | **建议做**。这才是「闲置十分钟」的根因。仍要拍板：失败后 session 是否仍可 prompt（建议按 agent 门控，或二次失败再拆） |
| A1：自动重试 ≤2 | **默认不做**，除非拍板接受「可能双份 user 消息」。优先让 adapter 内部重试（已经存在） |
| B-lite：`turn_failed_transient` + Retry 按钮 + 连接活着 | **建议做**（与 A0 同一刀）。最小可见可恢复，不动枚举 |
| B-full：新 `ConversationStatus` | **不做**，除非要跨重启的可恢复。那是产品决策 |
| 在 `send_prompt_inner` 层自动重放 | **不要做**（O43、重复 UserMessage、fork 错绑） |
| 把 503 塞进现有 `turn_failed_unknown` / `empty` | **不要做**（错误的诊断，错误的 i18n） |

---

## 7. 事故路径对照（把已知现象套回代码）

报告的现象：协调者回合中途「API Error: 503 No available accounts」，回合终止，红条，不自动重试，约 10 分钟无人发现。

与代码最吻合的一条：

1. Claude CLI / 网关返回 503 文本
2. 要么 CLI 自己的重试用尽，要么从未把这次失败标成 `api_retry` / AIR warning
3. `session/prompt` 以 `sacp::Error` 返回（很可能 `InternalError`，原文在 message 里）
4. `connection.rs:8076` `?` 拆掉整个连接循环
5. 前端 `default` 分支显示原文（红条）
6. 行 → `Cancelled`；连接 map 条目删除
7. 没有 Retry（AIR 条不会出现——那是通道 C；`conn.error` 条没有按钮）
8. 协调者队列不再有活连接可泵，直到有人发现

**未能确定**（本树没有那次事故的日志）：

- 当时的 claude-agent-acp 版本是否 ≥0.67（AIR）
- message 是 RPC error 原文还是 AIR `title`
- 503 之前是否已经有 assistant/tool 块（若有，A1 也不该重试）

---

## 8. 现成资产（实现时抄，不要再造）

- 零内容：`TurnOutputProbe` / `diagnose_empty_turn` / `is_agent_output_update`
- 非终端 turn 失败事件：`turn_failure_error_event`（照加一个 `transient` 臂）
- 错误 `code` + 前端 switch：`AcpError::code` 模式 vs 事件 `code`；前端 `case "error"`
- 用户可见重试：`TurnRetrying` + Claude 重试条；AIR `SessionFailure` + `lastUserPromptText` + `mqEnqueue`
- `ErrorCode` + 文本分类：`classify_session_load_failure` 的形状，不要它的映射
- 发送门闩：`turn_in_flight` + `TurnInProgress` + 前端消息队列
- 快照 `last_error` 已经能在刷新后把失败填回 `conn.error`

缺的只是：**在 `prompt_result` 臂抓住 RPC 错误，而不是把它升级成连接死亡。**
