# 规格 A 产出：13 家 parser 的消息锚点清点

> 只读事实。不改 parser。行号以本分支 `wt/fork-anchor-inventory`（基 `b8512285`）为准。
> 路径相对 `src-tauri/src/`，除非另标。
>
> **规格写「13 家」，权威注册表实际是 14 家。**
> `db/service/import_service.rs:27-42` 的 `ALL_PARSER_AGENTS: [AgentType; 14]`
> 含 ClaudeCode … Qoder。本表 14 行，按该数组顺序。Custom 走
> `AcpNativeParser`（同文件 `60-64`），不在数组里，只在文末脚注。

投影结构体：`models/message.rs:178-179` `UnifiedMessage.id: String`；
`models/message.rs:208-209` `MessageTurn.id: String`。两者都已存在，问题只在各
parser 往里塞什么。`UnifiedMessage` **不进 DB、不进详情 API**（`db/` 零命中）。
详情页只读 `ConversationDetail.turns: Vec<MessageTurn>`
（`models/conversation.rs:127-129`）。

**补约束（314 复核包 B，非返工）**：Claude Agent SDK 的 `resumeSessionAt` 要的是
**被保留那一轮的最后一条 chain entry**，不是「任意一条 assistant uuid」。
主表「截断后仍可定位」原口径仍是「该 id 在文件截断后能否找回同一条记录」。
Claude / Qoder 在那一列加了不够当 `resumeSessionAt` 锚的标注，细则见
[§补：轮末 chain entry](#补轮末-chain-entryresumesessionat-约束)。

---

## 主表

| parser | 原生文件里的逐条 id 字段 | 消息级投影 `UnifiedMessage.id` 塞的是什么 | turn 级投影 `MessageTurn.id` 塞的是什么 | 跨会话稳定？ | 截断后仍可定位？ |
|---|---|---|---|---|---|
| ClaudeCode | JSONL 行顶层 **`uuid`**（`parsers/claude.rs:1332-1336, 1392-1396, 1509-1513`）。另有嵌套 `message.id`（API call 分组，只给 usage 去重，`1529-1535`），不当消息锚。无 typed struct。 | 主路径 **拷贝 `uuid`**，缺则 `""`（`1339, 1491, 1541`）。旁路合成：`goal-{len}`（`498`，原生 uuid 只进 `tool_use_id`）、`synth-assistant-{len}`（`1633`）、`synth-result-{len}`（`1753`）。 | **丢掉 uuid**，三角色一律 `turn-{n}`（assistant `2551` / system `2585` / user `2597`）。 | uuid 再 import 同文件不变。空串会撞。`goal-{n}` / `synth-*-{n}` / `turn-{n}` 是位置。 | **单行 uuid：该行还在就能定位。不够当 `resumeSessionAt` 锚**（拿得到 assistant uuid，拿不到轮末 chain entry：attachment 丢、tool_result uuid 在分组时被吞）。见 §补。`turn-{n}` 左移。 |
| Codex | **用户/助手事件无逐条 id**。夹具 `event_msg.user_message` 只有 `message` 文本（`parsers/codex.rs:4335`；`tests/parsers_snapshot.rs` 同形）。有、但不进消息 id：`payload.call_id`/`tool_call_id`/`id` → `tool_use_id`（`2801-2806, 1935-1939`）；`turn_context.payload.turn_id` **不读**；`response_item.reasoning.id` **不读**。无 `item_id`/`uuid` 命中。 | **全位置合成**，17 处 `format!(... messages.len())`：`user-{n}`（`2400, 3385`）、`assistant-{n}`（`2429`）、`tool-{n}`（`2489, 2892, 2950, 3027, 3210`）、`tool-result-{n}`（`3142, 3162, 3226, 3344`）、`thinking-{n}`（`2783, 3931`）、`assistant-imagegen-{n}`（`2584, 3431`）。唯一常量：`"codex-goal-user"`（`3552`）。 | 一律 `turn-{n}`（`4185, 4197, 4237`）。不读 `msg.id`。 | 否。确定性再解析同一未改文件会得到相同 `user-0`，但那是位置，不是 provider id。 | **否。** 删更早的 user/agent/tool 事件，后续 `messages.len()` 变小，全部漂。 |
| OpenCode | SQLite `message.id` **TEXT PRIMARY KEY**（DDL：`tests/parsers_snapshot.rs:388-391`）。`SELECT id, time_created, data FROM message`（`parsers/opencode.rs:247-265`）。`data` JSON 不读 `$.id`。`part.id` / `callID` 不是消息锚。 | **拷贝表列** `id: msg_id`（`332-333`）。唯一构造点。 | `turn-{n}`（`1135, 1147, 1186`）。丢掉 TEXT PK。 | 消息级：TEXT PK，非下标；跨不同 `opencode.db` 是否全局唯一 **未能证实**（仓内夹具是 `m-user` 这类短串）。turn 级：否。 | 消息级：行还在就能按 PK 找。turn 级：否。parser 无 rewind/compact 语义。 |
| Gemini | 消息对象 **`id`**。JSON `session-*.json` 的 `messages[]`（`parsers/gemini.rs:627-631`）与 JSONL `session-*.jsonl` 同字段；JSONL 按 `id` merge（`141-150`）。无 typed struct。 | 有 `id` 则拷贝，否则 `msg-{n}`（`627-631` 算一次，user/assistant/system 三处 `647, 663, 681` 共用）。 | `turn-{n}`（`803, 818, 861`）。丢掉 `msg_id`。 | 有原生 `id`：同文件再 import 稳定。夹具是 `u1`/`a1`，**不校验全局唯一**。无 `id`：位置。 | 原生 `id`：对象还在就能对上。`msg-{n}` / `turn-{n}` 漂。 |
| OpenClaw | JSONL 顶层 **`id`** + `parentId`（`parsers/openclaw.rs:150-158`）。空 `id` 整行丢弃（`170-172`）。`JRecord.id`（`95-102`）。无 typed message struct。 | **拷贝** `msg_id = rec.id.clone()`（`592`），user/assistant/toolResult 三处（`611, 634, 647`）。 | `turn-{n}`（`1110, 1122, 1156`）。 | 消息级：文件内以 `id` 为键（`191`）。夹具 `u1`/`a1`，跨文件短 id 可能撞。turn 级：否。 | 消息级：记录还在就能按 `id` 找。turn 级：否。`.jsonl.reset.*` 是整文件归档，不是逐条 rewind。 |
| Cline | **无。** `ApiMessage` 只有 `role/content/ts/modelInfo/metrics`（`parsers/cline.rs:56-66`）。无 `deny_unknown_fields`，原生若多写了 id 会被 serde 丢掉。`ts` 只当时间（`255-260`）。 | **不使用 `UnifiedMessage`**（本文件零构造）。 | `{conversation_id}-{turn_counter}`，1-based，本轮发出的 turn 序号（`285, 305, 320`）。`conversation_id` = task 目录名。 | 会话 task `id` 稳定。turn 后缀是位置。不在 `turn-<digits>` 命名空间。 | 无原生消息 id 可定位。只砍尾巴时前缀 `{taskId}-k` 碰巧不变。 |
| Hermes | SQLite `messages.id` **INTEGER PRIMARY KEY AUTOINCREMENT**（`parsers/hermes.rs:232-249`；DDL `tests/parsers_snapshot.rs:904-907`）。`WHERE active = 1 ORDER BY id ASC`。 | **拷贝** `id: msg_id.to_string()`（`329-330`）。空 content 行 skip（`323-327`）。 | `turn-{n}`（`719, 731, 768`）。 | 消息级：单库 AUTOINCREMENT，跨 session 不复用（SQLite 语义）。不是 UUID。turn 级：否。 | **rewind 留行 + `active=0`**（测试 `tests/parsers_snapshot.rs:879,1016-1017`）：仍 active 的 INTEGER id 指向同一行。turn 级中间 deactivate 会左移。 |
| CodeBuddy | **消息行无逐条 id**（夹具 `parsers/codebuddy.rs:1065-1072` 只有 type/role/timestamp/sessionId/content）。工具：`callId` 否则 `id` → `tool_use_id`（`532-537`），不是消息锚。无 typed struct。 | **JSONL 行号**：`cb-user-{idx}`（`211`）、`cb-assistant-{idx}`（`225`）、`cb-reasoning-{idx}`（`240`）、`cb-toolcall-{idx}`（`260`）、`cb-toolresult-{idx}`（`289`）。`idx` = `lines().enumerate()`（`174`），空行/坏行/标题行也占号。 | `turn-{n}`（`944, 956, 994`）。丢掉 `cb-*`。 | 否。`cb-user-0` / `turn-0` 每会话重复。 | 否（位置）。只砍尾巴时前缀碰巧不变；头插空行/title 后面全漂。 |
| KimiCode | loop event 夹具有 **`event.uuid` / `turnId` / `parentUuid` / `toolCallId`**（`parsers/kimi_code.rs:1094-1098`）。parser **只读 `toolCallId`** → `tool_use_id`（`434-437, 467-470`）。`uuid`/`turnId`/`parentUuid` 零读取。`turn.prompt` 夹具 **无** 逐条 id（`1090`）。 | **JSONL 行号**：`kc-user-{idx}`（`382`）、`kc-text-{idx}`（`406`）、`kc-think-{idx}`（`421`）、`kc-toolcall-{idx}`（`446`）、`kc-toolresult-{idx}`（`488`）。`idx` = `lines().enumerate()`（`343`）。 | `turn-{n}`（`929, 941, 979`）。 | 投影 id：否。原生 `event.uuid` 未投影；夹具 `"p0"`/`"s1"` 像会话内局部。 | 投影：否。原生 loop uuid 若将来当锚：记录还在就能找；**用户 `turn.prompt` 仍可能无锚**（真实文件是否另有字段：未能证实）。 |
| Pi | **每行都有顶层 `id` + `parentId`**（模块注释 `parsers/pi.rs:70-73`；夹具 `828-847`：`m1`/`m2`/`m3`/`m4`）。`parse_message_record` **不读** 这两字段（`338` 起）。只读：session 头 `id`（会话，`288-294`）、`bashExecution.id` → `tool_use_id`（`436-439`）、内嵌 `toolCall.id`（`523`）。 | **行号覆盖原生 id**：`pi-user-{idx}`（`356`）、`pi-assistant-{idx}`（`382`）、`pi-toolresult-{idx}`（`403`）、`pi-bashcall-{idx}`（`443`）、`pi-bashresult-{idx}`（`457`）。`idx` = `lines().enumerate()`（`272`）。 | `turn-{n}`（`695, 707, 745`）。 | 投影：否。原生行 `id` 夹具是 `m1` 短串，生产是否 UUID **未能证实**。 | 投影：否。原生行 `id`（未投影）：记录还在理论上能找——parser 今天没用。 |
| Grok | **无 `UnifiedMessage`。** ACP `updates.jsonl`。已读未投影：`update._meta.promptIndex`（`parsers/grok.rs:730, 767`，只用来合并同 prompt 的 chunk）。真捕获有 `turn_completed.prompt_id`，**零读取**。`toolCallId` → 块级。无逐条消息 uuid。 | **absent**（不 import / 不构造 `UnifiedMessage`）。 | 先 `String::new()`（`769, 1643`），最后 `grok-turn-{i}`（`1013-1015`）。注释写「append-only 所以位置 id 再解析稳定」——那是再 parse，不是截断。 | 否。`grok-turn-0` 每会话重复。`promptIndex` 是 0-based 用户轮次，跨会话必撞。 | `grok-turn-{i}`：否。`auto_compact_completed` 会多插一轮 assistant（`884-916`），后续 i 右移。 |
| Cursor | **无 `UnifiedMessage`。** SQLite blob DAG：`DecodedState.turn_blob_ids`（`parsers/cursor.rs:752-753`），从 protobuf field 8 抽出（`788-792`）。内容寻址的 turn blob id。`build_turns` 按这些 id 读 blob（`925`），然后丢掉。 | **absent**。 | 先 `String::new()`（`985, 1001, 1033, 1050`），最后 `cursor-turn-{i}`（`1079-1080`）。 | `cursor-turn-{i}`：否。原生 blob id 是内容哈希，同内容跨会话会撞、改内容则变；**未投影**。 | 投影：否。原生 blob id：删前面的 turn 不改后面 blob id（未投影）。 |
| DeepSeek | **无 `UnifiedMessage`。** 事件 `data.id` / `data.message.id` 夹具明确存在（`parsers/deepseek.rs:918, 969, 995, 1010`：`u-1`/`a-1`/`t-1`/`a-2`）。parser **不读** 这些字段。只读 tool-call 块的 `block.id` → `tool_use_id`（`627`）。另有事件 `seq`，不当消息 id。 | **absent**。 | 直接 `turn-{n}`：user（`524`）、assistant 开张（`722`）。 | `turn-{n}`：否。原生 `data.id` 未投影；夹具短串。 | 投影：否。原生 `data.id`（未投影）：记录还在理论上能找。 |
| Qoder | 与 Claude 同形 JSONL：**行 `uuid`** + `parentUuid`（`parsers/qoder.rs:272-284, 546`）。`message.id` 只给 usage 合并（`608-640`），不当 `UnifiedMessage.id`。 | 有 uuid 则拷贝，否则 `q-user-{n}` / `q-assistant-{n}`（`581, 654`）。 | **调用 Claude 的** `group_into_turns`（import `13-16`，调用 `828`）→ 一律 `turn-{n}`（`parsers/claude.rs:2551, 2585, 2597`）。 | uuid 同 Claude。fallback `q-*-{n}` 是位置。 | **user/assistant uuid：该行还在就能定位。不够当轮末锚**（附件不投影，tool_result uuid 同样被 Claude 分组吞掉）。见 §补。 |

---

## 1. 分档

口径：**原生会话文件里有没有一个非位置字段能指回同一条消息（或同一轮）**，不管 codeg 今天有没有把它投影出去。投影活到哪一层见主表和 §2。

### 档 1 — 已有稳定原生锚点

文件里就有、再 import 同记录仍是同一字符串（或同一 SQLite 行）：

| parser | 一句话 |
|---|---|
| **ClaudeCode** | 行顶层 `uuid` 已进 `UnifiedMessage.id`。 |
| **Qoder** | 行顶层 `uuid` 已进 `UnifiedMessage.id`（缺则位置 fallback）。 |
| **OpenCode** | `message.id` TEXT PK 已进 `UnifiedMessage.id`。 |
| **OpenClaw** | JSONL 记录 `id` 已进 `UnifiedMessage.id`。 |
| **Hermes** | `messages.id` INTEGER PK 已进 `UnifiedMessage.id`；rewind 用 `active=0` 保行。单库唯一，不是 UUID。 |
| **Gemini** | `messages[].id` 有则进 `UnifiedMessage.id`；JSONL 按 id merge。缺 id 才退化为 `msg-{n}`。 |
| **Pi** | 每条 JSONL 都有顶层 `id`（注释 + 夹具），**parser 不投影**。 |
| **DeepSeek** | `data.id` / `message.id` 夹具存在，**parser 不投影**。 |
| **Cursor** | store.db 的 turn blob id 是原生 **turn 粒度** 锚点，**parser 不投影**。 |

### 档 2 — 有 id 但不稳定、不唯一、或不覆盖所有消息

| parser | 一句话 |
|---|---|
| **KimiCode** | loop event 有 `uuid`/`turnId`，parser 不读；用户 `turn.prompt` 夹具无逐条 id。投影是行号。 |
| **Grok** | 真捕获有 `prompt_id`（未读）；已读的 `promptIndex` 是 0-based 轮次序号。投影 `grok-turn-{i}`。 |
| **Codex** | 用户/助手 `event_msg` 无逐条 id；`call_id` / `turn_id` / `reasoning.id` 在文件里但不当消息锚。投影全位置合成。 |
| **CodeBuddy** | 消息行无逐条 id；工具 `callId` 只服务配对。投影是 JSONL 行号。 |

### 档 3 — 完全没有逐条原生消息 id

| parser | 一句话 |
|---|---|
| **Cline** | `ApiMessage` 无 id 字段；直接合成 `{taskId}-{n}`。 |

Codex / CodeBuddy 若把「工具 call_id」也算消息锚，它们定位的是 tool 块，不是 user/assistant 消息。本表按「从这里分叉」的消息粒度，把它们放在档 2。

---

## 补：轮末 chain entry（`resumeSessionAt` 约束）

> 314 复核包 B 追加的约束，**不是返工主表**。SDK 原文在
> `@anthropic-ai/claude-agent-sdk@0.3.232` `sdk.d.ts:1836-1894`
> （嵌在全局 `claude-agent-acp@0.69.0` 里，不是空的全局 `@anthropic-ai` 目录）。

`resumeSessionAt` 接受 **任意 chain-entry UUID**，但必须是 **被保留那一轮的最后一条**。
取早了（典型：只拿到 assistant uuid），校验器会给
`Resume rejected by --resume-drops-turn:` 且确定性失败。

`sdk.d.ts:1886-1892`：

> General rule subsuming all of the above: fork at the KEPT turn's last chain
> entry, whatever it is — `resumeSessionAt` accepts any chain UUID.

三种不能停在 assistant uuid 的情况（同文件）：

1. **end-turn 工具会话**（`1860-1871`）：轮末是 `structured_output` **attachment**，
   否则是 tool_result carrier。「Fork at the LAST entry … not the last assistant UUID」。
2. **`shouldQuery: false` 裸 user append**（`1873-1876`）：以普通 user 行落盘，
   fork 点若把它留在丢弃区间会拒绝。
3. **中断回合**（`1888-1892`）：已完成的非错误 tool_result 在尾巴上，fork 在
   assistant uuid 会被故意拒绝；其后的 interrupt marker / cancel-batch **可跳过**。

下面只钉 codeg parser **今天能不能把这条轮末 uuid 投影出来**。

### 单独标出：拿得到 assistant uuid，拿不到轮末 entry

**ClaudeCode — 对外锚点不够用。**

| 轮末实际是什么 | parser 今天怎么处理 | 投影里有没有这条 uuid |
|---|---|---|
| 最后一条 assistant 行 | 拷贝 `uuid` → `UnifiedMessage.id`（`parsers/claude.rs:1509-1541`） | 消息级有；turn 级变成 `turn-{n}`（`2551`） |
| 最后一条 tool_result（user 行） | 拷贝 `uuid` → `UnifiedMessage.id`（`1491`）；`group_into_turns` 把它 **吸收进前一条 assistant turn 并丢掉该 uuid**（`2565-2574`，判定 `is_tool_result_only` `2525-2531`） | 消息级短暂有；**对外 turn 层没有** |
| `structured_output` attachment | `attachment` 臂只认 `goal_status`（`1551-1590`，`goal_status_transition` 过滤 `409-410`）。其它附件 **整行丢弃**，uuid 不进任何投影 | **无** |
| interrupt 标记 `[Request interrupted by user]` | `is_interrupt_marker` 整行丢弃（`1294-1296`, `606-609`）。SDK 说这些 marker 可跳过 | 标记本身不需要；前面的 tool_result uuid 仍在 turn 层被吞 |
| `isMeta` 行 | `is_meta_message` 整行丢弃（`581-586, 1294`） | **无** |
| `shouldQuery: false` 裸 user append | parser **零读取** `shouldQuery`（`src-tauri/` 无命中）。若它是普通非空 user 行会走 `1491`；若是 isMeta / 剥标签后空（`1386-1388`）被丢。真实落盘形态 **未能证实** | 未能证实 |

Goal 附件的原生 uuid 只进 `tool_use_id`（`claude-goal-{uuid}`，`493-498`），
`UnifiedMessage.id` 仍是位置合成 `goal-{n}`。

**Qoder — 同样不够用。** 同形 JSONL，缺口对齐：

- 内容记录只认 `user|assistant`（`is_content_record`，`parsers/qoder.rs:196-200`）
- `attachment` / `system`「indexed but not returned」（`261-263`）：walk 能跨过，
  **投影没有附件 uuid**
- `is_meta_message` / interrupt 同样丢掉（`535`）
- user/assistant uuid 进 `UnifiedMessage`（`546, 581, 654`），再走 Claude 的
  `group_into_turns`（`828`）→ tool_result uuid 同样被吞

### 其余家和这条约束的关系

这条是 **Claude Agent SDK 的 chain UUID 语义**。其它 parser 的原生 id 不是
`resumeSessionAt` 的输入。它们主表「截断后」列仍按「能否找回同一条原生记录」，
**不能**拿那些 id 去填 `resumeSessionAt`。

| parser | 和这条约束 |
|---|---|
| Codex / CodeBuddy / Cline / Grok / Cursor / DeepSeek / KimiCode / Pi | 投影层没有 Claude chain uuid |
| OpenCode / OpenClaw / Gemini / Hermes | 消息级有自己的稳定 id，**不是** Claude JSONL `uuid` |

---

## 2. turn 层塌陷

### 点名

消息级已经拿到（或本可以拿到）原生锚、turn 级丢掉的：

| parser | 消息级有什么 | turn 级变成什么 | 证据 |
|---|---|---|---|
| ClaudeCode | `uuid` | `turn-{n}` | `1491/1541` → `2551/2585/2597` |
| Qoder | `uuid` | `turn-{n}`（复用 Claude 分组） | `581/654` → `qoder.rs:828` → `claude.rs:2551` |
| OpenCode | TEXT PK | `turn-{n}` | `333` → `1135/1147/1186` |
| OpenClaw | JSONL `id` | `turn-{n}` | `611/634/647` → `1110/1122/1156` |
| Gemini | `messages[].id` | `turn-{n}` | `647/663/681` → `803/818/861` |
| Hermes | INTEGER PK | `turn-{n}` | `330` → `719/731/768` |
| CodeBuddy | `cb-*-{idx}`（已是合成） | `turn-{n}` | `211…289` → `944/956/994` |
| KimiCode | `kc-*-{idx}`（已是合成；原生 uuid 未进消息层） | `turn-{n}` | `382…488` → `929/941/979` |
| Pi | `pi-*-{idx}`（原生行 `id` 未进消息层） | `turn-{n}` | `356…457` → `695/707/745` |

根本不经过 `UnifiedMessage`、直接出位置 turn id 的：

- **Cline** `{taskId}-{n}`（`285/305/320`）
- **Grok** `grok-turn-{i}`（`1013-1015`）
- **Cursor** `cursor-turn-{i}`（`1079-1080`）；原生 `turn_blob_ids` 读了就扔（`925`）
- **DeepSeek** `turn-{n}`（`524, 722`）；原生 `data.id` 没读

`group_into_turns` 的共同动作：拷 `content` / `timestamp` / `usage` / `model` / `completed_at`，**唯独不拷 `UnifiedMessage.id`**。assistant 还经常吞掉紧随的 tool 行，多条消息 id 塌成一个 turn。

### 前端详情页读的是哪一层（调用链）

1. UI：`src/stores/conversation-runtime-store.ts:3137-3140`
   `getFolderConversation(fetchId, { tailTurns | fromIndex })`。
2. 客户端：`src/lib/api.ts:2423-2431` → transport `get_folder_conversation`。
3. 命令：`commands/conversations.rs:1831-1848`
   `get_folder_conversation` → `get_folder_conversation_with_live_core`（`1747`）。
4. 核心：`get_folder_conversation_core`（`1378`）按 `summary.agent_type` 构造 parser
   （`1409-1425`，与 `import_service.rs:44-64` 同一张表），调用
   `parser.get_conversation(&eid)`（`1426`），取出 **`d.turns`**（`1428`）。
5. trait 合同：`parsers/mod.rs:215-217`
   `get_conversation → ConversationDetail`；`models/conversation.rs:127-129`
   该结构体字段是 `turns: Vec<MessageTurn>`，没有 `messages: Vec<UnifiedMessage>`。
6. 前端类型：`src/lib/types.ts:267-281` `MessageTurn`；
   `src/lib/types.ts:708-710` `DbConversationDetail.turns: MessageTurn[]`。
7. 分页：`get_folder_conversation_turns_core`（`1806-1819`）仍是同一份
   `detail.turns` 再切片。
8. **活着的二次改写**：`apply_in_flight_message_id`（`1648-1693`）可能把
   **最后一条匹配的 user `MessageTurn.id` 覆盖成 ACP 客户端 `message_id`**
   （`1692`），并从 `get_folder_conversation_with_live_core` 调用（`1793-1794`）。
   这是详情页实际看到的 id，不是 parser 原样。

结论：详情页、窗口切片、token 看板的 `turn_key`（`db/entities/token_usage_turn.rs:19-20`，
「parser 的 `MessageTurn::id`，provenance for debugging, not a key」）读的都是
**turn 层**。消息级即使带了原生 uuid，前端也看不见。

旁路：`get_conversation`（`commands/conversations.rs:469-496`）同样返回
`parser.get_conversation` 的 `ConversationDetail`（仍是 turns），不经 DB。
文件夹详情不走这条。

---

## 3. 和 `turn-<digits>` 保留命名空间的冲突面

定义与用法：

```
src-tauri/src/acp/manager.rs:66-68
fn is_reserved_turn_id(id: &str) -> bool {
    matches!(id.strip_prefix("turn-"), Some(rest)
        if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}
```

调用点：发 prompt 时挑 `viewer_message_id`（`acp/manager.rs:1433-1437`）。客户端
给的 `message_id` 若命中该形状，丢掉，改用 `user-{conn_id}-{event_seq}`。
测试钉死：`turn-0`/`turn-42` 命中；`optimistic-…`、`user-conn-7`、`turn-`、
`turn-1a`、`turnabout-1`、`""` 不命中（`3617-3629`）。

注释自称「every parser assigns via `format!("turn-{}", n)`」（`61-62`）。**不准确。**

### 今天谁已经占用这个形状

`MessageTurn.id` **就是** `turn-<digits>` 的（会和保留检查同形）：

- Claude / Qoder（经 Claude 分组）/ Codex / OpenCode / Gemini / OpenClaw /
  Hermes / CodeBuddy / KimiCode / Pi / DeepSeek
- 证据：各 parser `format!("turn-{}", turns.len())`，见主表第三列。

**不是** 这个形状的：

- Cline `{taskId}-{n}`（task 目录名，通常毫秒时间戳）
- Grok `grok-turn-{i}`（`strip_prefix("turn-")` 失败）
- Cursor `cursor-turn-{i}`（同上）
- ACP native（不在 14 家数组里）`acp-{seq}`（`parsers/acp_native.rs:381, 438, 537`）

### 如果把原生锚点塞进现有 `id` 字段，会不会撞

只陈述现状，不选型。

| 若塞进… | 和 `is_reserved_turn_id` |
|---|---|
| Claude / Qoder **uuid** | 不撞。UUID 不是 `turn-<digits>`。 |
| OpenCode TEXT PK | 夹具 `m-user` 不撞；线上若真写成 `turn-12` 会撞——**未能证实** 线上形态。 |
| OpenClaw JSONL `id` | 夹具 `u1`/`a1` 不撞；线上形态未能证实。 |
| Gemini `messages[].id` | 夹具 `u1` 不撞；fallback `msg-{n}` 不撞。 |
| Hermes `"1"` / `"42"` | **不撞**（没有 `turn-` 前缀）。但和别的会话的 `"1"` 会撞，若当全局 key。 |
| Pi 原生 `m1` | 不撞（未投影）。 |
| DeepSeek `u-1` | 不撞（未投影）。 |
| Cursor blob hex | 不撞（未投影）。 |
| Codex `user-{n}` / `assistant-{n}` | 不撞。`user-conn-7` 测试明确放行。 |
| Grok `grok-turn-{i}` / Cursor `cursor-turn-{i}` | 不撞。 |
| **维持现状 `turn-{n}`** | **已经占用** 保留空间。这是 `is_reserved_turn_id` 存在的原因：挡住客户端伪造 parser turn id（`61-65, 1679-1680`）。 |

另外两处会看见 `MessageTurn.id`：

- `apply_in_flight_message_id`（`1679-1692`）把 live `message_id` 盖到匹配的
  user turn 上；若 live id 与另一条 turn 的 id 相同则拒绝盖写。把原生 uuid
  放进 turn id 之后，客户端若碰巧用同一字符串当 live id，会走碰撞分支。
- `token_usage_turn.turn_key` 存的是 `MessageTurn.id`，**不是主键**
  （`db/entities/token_usage_turn.rs:19-20`）。改 id 形状只影响这份 provenance。

`commands/token_usage.rs:182` 的 `format!("turn-{idx}")` 和
`commands/turn_window.rs:164` 的 `format!("turn-{secs}")` 是命令层自己的合成，
不是 parser 输出。

---

## 观察（建议，不是结论）

1. 规格写 13 家，`ALL_PARSER_AGENTS` 是 `[AgentType; 14]`。漏的是 **Qoder**
   （数组最后一项，`import_service.rs:41`）。Custom/ACP 仍在数组外。
2. SURVEY「OpenCode 自增 id」与代码矛盾：测试 DDL 是 **TEXT PRIMARY KEY**
   （`tests/parsers_snapshot.rs:388-391`），不是 INTEGER。
3. `is_reserved_turn_id` 注释「every parser assigns `turn-{n}`」不成立：
   Cline / Grok / Cursor 不是这个形状。
4. 协调者起点行号复核：**仍准**。补遗：Claude 还有 `498/1633/1753` 合成点；
   Claude turn 合成不止 `:2585`（那是 System；assistant 在 `2551`，user 在 `2597`）；
   Codex 构造点共 17 处，另有常量 `"codex-goal-user"`（`3552`）。
5. 最反直觉的事实见下节。
6. 314 复核包 B 之后：Claude / Qoder 的「截断后仍可定位」从「单行 uuid 还在就能找」
   收紧为「不够当 `resumeSessionAt` 锚」——见 §补。不改其它家主表口径。

---

## 最反直觉的一条

**不是「大家都没有锚点」，而是「有的家读了再扔，有的家文件里就有、眼皮底下不读」。**

Claude / OpenCode / OpenClaw / Gemini / Hermes / Qoder 已经把原生 id 写进
`UnifiedMessage.id`，然后 `group_into_turns` 无例外改成 `turn-{n}`——详情页只
看得见后者。

更刺的是另一侧：Pi 的每条 JSONL **都有** `id`/`parentId`（parser 自己的模块注释
写了，夹具 `m1`–`m4` 也有），`parse_message_record` 却用 `pi-user-{行号}` 盖掉。
DeepSeek 夹具里 user/assistant 带着 `data.id` = `u-1`/`a-1`，parser 一行都不读，
直接 `turn-{n}`。Cursor 从 DAG 里把原生 turn blob id 读进 `turn_blob_ids`，
投影完写成 `cursor-turn-{i}`。

所以「codeg 现在没持久化任何 provider anchor」（SURVEY §5）这句话对 **对外
turn 层** 成立，对 **parser 内部消息层** 不成立，对 **原生文件** 更不成立。

---

## 脚注：Custom / `AcpNativeParser`

不在 `ALL_PARSER_AGENTS` 里。`build_parser` 的 `Custom(_)` 臂（`import_service.rs:60-64`）
走 `parsers/acp_native.rs`。turn id 为 `acp-{seq}`（`381, 438, 537`），不在
`turn-<digits>` 保留空间。未纳入主表。
