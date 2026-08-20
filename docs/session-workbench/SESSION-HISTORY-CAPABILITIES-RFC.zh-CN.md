# Codeg Session 历史能力子 RFC

> 状态：Draft  
> 调研日期：2026-08-15  
> 对账修订：2026-08-20 —— §3、§4 已按当前代码重写（原文基于 0.25.0，与 0bfb86a0
> 之后的实现有 5 处脱节；脱节清单见
> [能力矩阵与可行路径调研](./SESSION-FORK-REWIND-SURVEY-2026-08-19.zh-CN.md) §1）。
> 第二轮更正：2026-08-20 —— 把切片 2 第一轮三份 FINDINGS
> （[`FINDINGS-A`](./fork-rewind-slices/FINDINGS-A-anchor-inventory.zh-CN.md) /
> [`FINDINGS-B`](./fork-rewind-slices/FINDINGS-B-claude-fork-audit.zh-CN.md) /
> [`FINDINGS-C`](./fork-rewind-slices/FINDINGS-C-codex-fork-audit.zh-CN.md)）
> 回灌进本文件。触及：§2 粒度、§3.6/§3.7 谱系已落地、§4 实测、§7.1/§7.2、
> 新增 §7.6 锚点形状、§9、§11 阶段 0。SURVEY 本身不改（历史快照）。
> 范围：Fork、历史消息分叉、编辑后重发、对话回退、文件检查点与跨 Harness 降级  
> 上位产品需求：[产品需求与使用场景](./PRODUCT-SPEC.zh-CN.md#73-fork编辑旧消息和文件恢复)

## 1. 结论

Codeg 不应把 Fork、Rewind 和文件恢复实现成一个模糊的通用按钮。它们是三种不同能力：

1. **Head Fork**：复制 Session 当前完整状态，得到一个新的原生 Session；
2. **Historical Fork**：从某条历史消息创建新 Session，原 Session 不变；
3. **File Restore**：把工作目录恢复到历史检查点，可能覆盖当前文件状态。

当前 Codeg 0.25.0 已经实现第一种：对支持 ACP `session/fork` 的连接执行“分叉发送”。它尚未
实现统一的历史消息锚点，也没有跨 Harness 的文件恢复能力。

推荐把这些能力放进独立的 **Session History Capability Layer**。统一的是产品语义、能力探测、
结果和错误模型，不是强迫所有 Harness 走同一条协议。实现优先级为：

```text
原生结构化 SDK / 服务接口
→ ACP 已明确暴露的能力
→ 稳定的机器可调用 CLI 接口
→ 版本化 JSONL 兼容器（最后手段）
```

默认行为必须是非破坏性的：分叉产生新 Session；恢复文件必须单独选择并确认；不得直接截断
原生会话或原地修改其 JSONL。

## 2. 术语与用户语义

| 能力 | 输入 | 输出 | 是否改变原 Session | 是否改变文件 |
|---|---|---|---|---|
| `forkHead` | 当前 Session | 新原生 Session | 否 | 否 |
| `forkAtMessage` | Session + 历史消息锚点 | 新原生 Session | 否 | 否 |
| `editAndFork` | Session + 历史消息 + 新文本 | 新原生 Session 并发送新文本 | 否 | 否 |
| `truncateInPlace` | Session + 历史消息锚点 | 原 Session 被截断 | 是 | 否 |
| `restoreFiles` | Session + 检查点 | 文件恢复结果 | 否 | 是 |
| `forkWithFiles` | 历史消息 + 检查点 | 新 Session + 文件恢复结果 | 否 | 是 |
| `handoff` | 来源 Session + 摘要/材料 | 另一 Harness 的新 Session | 否 | 否 |

第一版不向普通用户提供 `truncateInPlace`。界面中的“编辑旧消息”必须解释为 `editAndFork`，
不能静默重写事实历史。

消息锚点也不能假定是 Codeg 当前消息表的自增 ID，更不能假定是 assistant UUID。
`forkAtMessage` 的锚点粒度因 Harness 而异（§10 要求诚实呈现能力差异，这里落到表上）：

- **Claude**（claude-agent-acp 0.69.0）：产品语义是从这条消息分叉。原生锚点是被保留那一轮的最后一条 chain entry 的 uuid（`sdk.d.ts:1886-1892`），不是 assistant uuid。粒度：**消息级**。
- **Codex**（codex-acp 1.4.0）：产品语义是从这一轮分叉。原生锚点是 app-server `thread/fork.lastTurnId`（FINDINGS-C §2.3）。粒度：**轮级**。
- **通用 ACP**：当前协议无历史锚点，不承诺 `forkAtMessage`（§7.3）。

适配器需要保存可验证的原生锚点。详情见 §7.6。

## 3. 当前 Codeg 实现事实

本节按 Codeg 0.26.1 的代码重写（原文写于 0.25.0/`b6e1d904`，次日被 0bfb86a0 反转，
之后又被 67974819 扩充）。下面每条都可在给出的 `file:line` 上核对。

### 3.1 调用链

- `src/lib/api.ts:348` 的 `acpFork` 提供前端调用；
- `conversation-detail-panel.tsx:1373` 的 `handleForkSend` 先 Fork，再把草稿发送到新
  Session；门控在同文件 `:2110`（`conn.supportsFork`）；
- `message-input.tsx:1227 / :1711` 只在连接报告支持 Fork 时显示“分叉发送”下拉项；
- 后端入口双模式：`src-tauri/src/commands/acp.rs:9871`（Tauri）与
  `src-tauri/src/web/handlers/acp.rs:459`（HTTP，参数结构在同文件 `:331`）；
- `src-tauri/src/acp/manager.rs:1882` 的 `fork_session` 负责并发闸门（prompt 锁 +
  `turn_in_flight`）、取消屏蔽与数据库落盘；
- `src-tauri/src/acp/fork.rs:19` 用 `UntypedMessage` 发 `session/fork`（sacp 11.0.0
  尚无 typed 封装），`connection.rs:6711` 的 `handle_fork_or_exit` 拿
  `ForkSessionResponse` 直接 attach 新 Session，不再走一次 `session/load`。

### 3.2 行布局：C1/S1 不动，INSERT C2（原文写反了）

原文写的是“当前会话行切换到新原生 Session，另建 sibling 行保留原 Session 历史”。
0bfb86a0（2026-08-16）把它反转了，现在的事实是：

- 原会话行 C1 及其原生 Session S1 **整行不动**，新 Fork 是一条新 INSERT 的行 C2，绑
  Agent 返回的新 sessionId S2（`manager.rs:2099` 的函数文档 “leave C1/S1 untouched and
  INSERT C2”，实现在 `manager.rs:2133` 起的 `persist_fork_outcome`）；
- C2 在同一事务内继承 C1 的 `folder_id` / `kind` / `model` / `git_branch` /
  `origin_cwd` / `created_by`（`manager.rs:2217-2249`），以及 C1 的 Collection 成员关系
  （`manager.rs:2252-2266`）；不继承任何活体状态（Turn、队列、mailbox）；
- 活跃连接随后从 C1 迁到 C2，`ConversationForked` 事件推前端。

因此 RFC 其它章节里出现的“sibling”一词，指向的是 **新建的 C2**，不是保留历史的旧行。

### 3.3 `[Fork]` 前缀已在后端

前缀不再由前端加，前端正则已删除。现在由 `persist_fork_outcome` 在 C2 上打标：容错剥
掉源标题已有的 `[Fork]` 前缀（`manager.rs:2195`），再写 `[Fork] {title}` 并置
`title_locked = true`（`manager.rs:2220-2221`），让原生标题回填不能抹掉这个区分。源标题
为空时不加前缀，`title_locked` 保持 false。

### 3.4 `acpFork` 签名已扩展 —— 且两端封装不一致（已知问题）

`api.ts:348` 的 `acpFork` 现在是 `(connectionId, conversationId?, folderId?)`。多出的两
个参数用于“fork 早于首条 prompt 的未链接会话”：从历史打开的会话，其连接是 resume 出来
的，行要到首条 prompt 才与连接绑定，而分叉发送恰好发生在那之前——传这两个 id 让后端
adopt 该行，fork 才不会以“未链接”被拒。后端两侧都已接住
（`commands/acp.rs:9871-9874`、`web/handlers/acp.rs:331-341`）。

**已知问题**：`src/lib/tauri.ts:160` 的同名封装仍然只传 `connectionId`。Rust 侧两个参数
是 `Option`，缺省为 `None`，所以桌面端不会报错，但吃不到上面的 adopt 修复。两端封装因此
不一致，任何后续 fork 相关的签名改动都要同时改这两处。修掉它不在本切片范围内。

### 3.5 Fork 继承 pin 的 model / thinking effort（原文未记）

67974819（2026-08-18）起，C2 继承 C1 的 Session 级选择器 pin：`preferred_mode_id` 与
`preferred_config_values`（`manager.rs:2247-2248`）。语义是“Fork 是同一件工作的继续”，
用户在 Fork 里重新选择才分岔。这条约束了后续 provider：任何新的 fork 路径都必须保持
pin 继承，否则 Fork 出来的会话会静默换模型。

### 3.6 谱系与能力探测

- `conversation.parent_id` 仍专用于 delegation。Fork 显式写
  `parent_id: Set(None)`（`manager.rs:2228`，字段语义见
  `db/entities/conversation.rs:68`）。谱系已落在独立表 `fork_relation`
  （migration `m20260820_000002`）：`persist_fork_outcome` 在同一事务里
  `record_fork_head`（`manager.rs:2274`，`db/service/fork_lineage_service.rs:25-45`）。
  `anchor` 字段对 head fork 为 NULL，留给 `fork_at_message`（§9）。
- 支持面是**纯运行时探测**，不是按 `agent_type` 写死：`connection.rs:4264-4268` 读
  `initialize` 回复里的 `sessionCapabilities.fork`，经 `ForkSupported` 事件落到
  `session_state.rs:664`，再推前端。这与 §6“能力矩阵必须来自运行时探测”的要求一致。

### 3.7 仍然成立的结论

这套实现可以继续作为 `forkHead` 的 ACP provider，不应推倒重写。需要补的是：

- 明确记录来源与目标的 Fork 关系 — **已落地**（`fork_relation`，见 §9）；
- 历史消息级锚点（切片 2：先做锚点管道，见 §7.6）；
- 原生 Claude/Codex provider（Claude 走 §7.1 旁路，不改上游；Codex 要上游，见 §7.2）；
- 文件恢复的独立能力与安全确认；
- UI 对能力差异的诚实呈现（粒度已落在 §2）。

## 4. ACP 的真实边界

ACP 的 Session Fork RFD 当前仍标为 Draft，部分 SDK、Agent 和 Client 已按该草案实现
`session/fork`。Agent 通过 Session capability 宣告 Fork 支持，Client 提交当前
`sessionId`、cwd 和 MCP servers 等上下文，Agent 返回新 Session ID。

但当前协议只表示“复制当前完整 Session”。它没有历史 message/turn 参数，也没有标准化：

- 从任意消息分叉；
- 编辑旧消息后分叉；
- 原地截断对话；
- 恢复文件检查点；
- 会话分支谱系的持久查询。

ACP 的 Session Fork RFD 已把“未来增加可选 message ID”列为扩展方向。这说明 Historical
Fork 目前不能通过通用 ACP 稳定实现。协议讨论中也仍在探索历史分页、重放游标等能力。

2026-08-20 的版本快照（原文基于 0.25.0 / claude-acp 0.67.0 / codex-acp 1.2.0，已过时）。
Codeg 现在是 0.26.1（`package.json:4`、`src-tauri/tauri.conf.json:4`），钉的适配器版本：

- `@agentclientprotocol/claude-agent-acp@0.69.0`（`acp/registry.rs:459-460`）；
- `@agentclientprotocol/codex-acp@1.4.0`（`acp/registry.rs:593-594`）；
- `@xai-official/grok@1.0.5`（`acp/registry.rs:835-836`）：对 1.0.5 二进制重新实测，
  `initialize` 仍只答 `sessionCapabilities: {list, resume, close}`——**无 fork**
  （`registry.rs:829-833`）。

原文那张 “谁声明 Fork” 的清单不应再当事实用，理由有两条：

1. 版本已经全部走过，0.67.0 那次快照的结论不能顺延到 0.69.0；
2. 更要紧的是 Codeg 根本不读静态矩阵——支持面来自每条连接的 `initialize` 回复
   （§3.6，`connection.rs:4264-4268`）。静态清单只在“该不该给某家排期”时有参考价值。

第一轮实测已经闭合这两处空白（[`FINDINGS-B`](./fork-rewind-slices/FINDINGS-B-claude-fork-audit.zh-CN.md) §1；
[`FINDINGS-C`](./fork-rewind-slices/FINDINGS-C-codex-fork-audit.zh-CN.md) §1）：

- **claude-agent-acp 0.69.0 声明 fork**：`dist/acp-agent.js:715`，形状是空对象 `{}`
  （非布尔），挂在 `agentCapabilities.sessionCapabilities` 下。codeg 探测
  `.fork.is_some()`（`connection.rs:4264-4268`）会把它当成支持。
- **codex-acp 1.4.0 不声明 fork**：`dist/index.js:29697-29703` 的
  `sessionCapabilities` 只有 `{resume, list, close, delete, additionalDirectories}`，
  也没有 `session/fork` handler。内部 `thread/fork` 只服务 AIR
  `agentFileChangeReport`（FINDINGS-C §2），**不是** ACP fork 声明。
  不要把这条内部通道当成它声明了 ACP fork。

因此 Codeg 当前基于能力探测显示按钮是正确的，但 ACP provider 只能承诺 `forkHead`。
Claude 的 `forkAtMessage` 走 §7.1 的 `_meta` 旁路，不经过协议历史锚点；Codex 的
`forkAtMessage` 目前够不着，要等上游把 `thread/fork` 接到 ACP（§7.2）。

## 5. Zed 与 Paseo 提供的架构经验

### 5.1 Zed

Zed 的第一方 Agent 支持编辑历史消息、回退线程和恢复 Git checkpoint。源码中，这依赖 Zed
内部 connection 的 `truncate` 能力；通用 External Agent connection 默认不提供该能力。UI
只有在 connection 明确支持时才允许 rewind/edit。

可借鉴的是：

- 能力由 provider 显式声明；
- 对话回退与文件 checkpoint 是不同操作；
- UI 不为不支持的 External Agent 伪造一致性。

不能据此推断“Zed 已经通过 ACP 为所有 Agent 实现了历史分叉”。

### 5.2 Paseo

Paseo 没有把关键能力全部压到 ACP：

- Claude provider 使用 Anthropic Agent SDK 的 `forkSession`，定位原生历史消息 UUID 后创建新
  Session；
- Codex provider 使用 app-server 的 `thread/fork` 和 `thread/rollback`；
- 通用 ACP provider 对没有原生支持的 Rewind 能力保持关闭。

Paseo 的“Rewind conversation”实质上是非破坏性 Historical Fork：原 Session 保留，结果是
一个新的原生 Session。文件恢复则单独走 checkpoint/rewind 能力。

这正是 Codeg 应采用的模式：统一能力层，下面允许多个 provider。

## 6. 拟议能力层

下面的类型只表达接口语义，不是要求直接照搬为数据库字段：

```ts
type SessionHistoryCapabilities = {
  forkHead: "stable" | "experimental" | "unsupported";
  forkAtMessage: "stable" | "experimental" | "unsupported";
  editAndFork: "stable" | "experimental" | "unsupported";
  restoreFiles: "stable" | "experimental" | "unsupported";
  restoreConversationAndFiles: "stable" | "experimental" | "unsupported";
};

type HistoryAnchor = {
  conversationId: string;
  codegMessageId?: string;
  providerAnchor?: string;
  checkpointId?: string;
};

type HistoryOperationResult = {
  sourceConversationId: string;
  targetConversationId?: string;
  nativeSessionId?: string;
  relationKind?: "fork_head" | "fork_at_message" | "handoff";
  fileChanges?: FileRestorePreview;
  warnings: string[];
};
```

后端提供一个稳定的应用服务，按当前 Harness、版本、Session 来源和运行状态选择 provider：

```text
SessionHistoryService
├── ClaudeNativeHistoryProvider
├── CodexAppServerHistoryProvider
├── AcpHeadForkProvider
├── StableCliHistoryProvider
└── VersionedJsonlHistoryProvider (experimental)
```

能力矩阵必须来自运行时探测与版本适配，不能只按 `agent_type` 写死。相同 Harness 的不同
adapter 或版本可能具有不同能力。

## 7. Provider 策略

### 7.1 Claude

**不需要上游改动。** 0.69.0 的 `session/fork` 已经是 head fork
（`unstable_forkSession` → `query({ resume, forkSession: true })` → CLI
`--resume` + `--fork-session`，FINDINGS-B §2）。消息级截断走同一条 ACP 方法，额外带：

```json
{ "_meta": { "claudeCode": { "options": { "resumeSessionAt": "<chain-entry-uuid>" } } } }
```

静态贯通（ROUND2-SPECS §1；活体探针的静态部分）：

- `_meta` 经 zod 收下并整包交给 `createSession`（FINDINGS-B §3，`acp-agent.js:761`）；
- `...userProvidedOptions` spread 在 `acp-agent.js:4821`，`resumeSessionAt` **不在**
  ACP 强制覆盖清单里；
- SDK `sdk.mjs:118` 把它编成 `--resume-session-at=` argv；
- SDK 用 ProcessTransport + `stream-json` 起 CLI，即 `sdk.d.ts:1878-1884` 所称的
  print/headless **武装 lane**。print lane 那颗雷已静态排除。

adapter **没有**读 `_meta.messageUuid` / `upToMessage`，也 **没有**调用 SDK
`forkSession({ upToMessageId })`。SURVEY 猜的那两个键和那条 API 在 0.69.0 都不接。
真正能用的是 `claudeCode.options` 的 spread 副产品。

**三颗雷（必须进错误模型，不能当普通网络失败）：**

1. **lane 限制（已静态确认 codeg 走的是武装 lane，但仍是非文档化旁路）。**
   `resumeSessionAt` + `resumeDropsTurn` 是 PRINT/HEADLESS LANE ONLY
   （`sdk.d.ts:1878-1884`）。交互式 `claude --resume` 与后台 job 会忽略这对参数——
   加载完整历史，不截断、不报错。走错 lane = 看起来 fork 成功，实际没截断。
   codeg 经 ACP adapter 的 `query()` / ProcessTransport 起 CLI，静态上正是武装 lane；
   是否真截断尚未活体证实（探针 turn 没跑完，见台账「活体探针结果」）。
2. **`resumeDropsTurn` 拒绝确定性、不可重试。** 错误消息以
   `Resume rejected by --resume-drops-turn:` 开头（`sdk.d.ts:1845-1858`）。
   必须映射到 rewind-recovery（清掉 pending fork target，保留证据，改走普通 resume），
   **禁止退避重试**——同一请求会永远失败。
3. **spread 副产品不是契约。** 上游只要在 `createSession` 的覆盖清单里加一条
   `resumeSessionAt`，旁路静默失效。没有 capability 广告、没有测试钉死。切片 2 接线时
   必须把这条通道的探测/失败面写进错误模型，不能假装它是稳定 API。

历史分叉仍应创建新原生 Session，原会话只读保留。只有 provider 能证明锚点仍有效时
才显示「从这里分叉」。锚点的真实形状见 §7.6——**不是** assistant uuid。

### 7.2 Codex

原生粒度是 **turn 级（`lastTurnId`），不是 message 级**。SURVEY 的
`thread/fork{threadId, messageId}` 说法与 1.4.0 / Codex 0.147 的 generated 类型
对不上（FINDINGS-C §2.3：`ThreadForkParams.lastTurnId`，类型里没有 `messageId`）。
产品语义因此是「从这一轮分叉」，不能承诺「从这条消息分叉」。

当前够不着这条能力：

- 1.4.0 **不声明** ACP `sessionCapabilities.fork`（`dist/index.js:29697-29703`）；
- 没有 `session/fork` handler；
- 内部 `thread/fork` 的唯一调用点是 AIR `agentFileChangeReport`
  （`dist/index.js:27569`），codeg 不广告 AIR，所以这条不会跑；
- app-server 通道在 adapter 子进程里，ACP 客户端碰不到（FINDINGS-C §4）。

裁决（FINDINGS-C §6，协调者维持）：给 codex-acp 提能力，把已有的 `threadFork` 接到
ACP `session/fork`；按 turn 截断走 `_meta`。再开第二条 app-server 是上游不接 PR 的退路，
不是第一入口。本切片不写这条编码。

`thread/rollback` 已标 DEPRECATED（will be removed soon），且只丢 turn、**不回文件**，
适配器零调用点。**rewind 不押它。** Codex 侧 rewind 搁置。

### 7.3 ACP

ACP provider 只实现 Agent 实际声明的能力。当前 `session/fork` 映射为 `forkHead`；在协议没有
历史锚点前，不把它冒充成 `forkAtMessage`。

### 7.4 Grok 与其他 CLI

如果 Harness 只有交互式 `/rewind` 或 `/undo`，但没有稳定的结构化接口，就保持 unsupported
或 experimental。终端按键自动化不能作为默认可靠实现，因为它难以确认目标 Session、分叉
结果和失败后的文件状态。

### 7.5 跨 Harness

跨 Harness 不叫原生 Fork。它创建目标 Harness 的新 Session，并附带可检查、可编辑的 handoff：

- 来源 Session 链接；
- 用户选择的消息范围或摘要；
- 明确附加的文件和结论；
- 生成时间与来源 Harness。

### 7.6 锚点的真实形状（切片 2 的核心约束）

Claude Agent SDK 的 fork 点不是「任意一条 assistant uuid」。`sdk.d.ts:1886-1892`：

> General rule subsuming all of the above: fork at the KEPT turn's last chain
> entry, whatever it is — `resumeSessionAt` accepts any chain UUID.

三种不能停在 assistant uuid 上的情况（同文件 1860-1892）：

1. end-turn 工具会话：轮末是 `structured_output` **attachment**（否则是 tool_result carrier）；
2. `shouldQuery: false` 裸 user append：fork 点若把它留在丢弃区间会拒绝；
3. 中断回合：已完成的非错误 tool_result 在尾巴上，fork 在 assistant uuid 会被故意拒绝。

codeg 今天恰恰把那类记录扔了（FINDINGS-A §补）：

- `parsers/claude.rs:1551-1591` 的 `attachment` 分支只保留 `goal_status`，其余整条丢弃
  （注释点名 agent listings / skill listings / task reminders 是「给模型的上下文，不是对话」）；
- `group_into_turns`（`:2565-2571`）吸收 tool_result **正文**但丢弃其 uuid，turn 的 `id`
  一律 `turn-{n}`（`:2551 / 2585 / 2597`）。Qoder 走同一份分组（`qoder.rs:828`），
  附件 indexed but not returned（`qoder.rs:261-263`）。

实测 transcript 佐证（台账「活体探针结果」）：一条 prompt 之后链尾是四条带 uuid 的
attachment——`deferred_tools_delta` / `agent_listing_delta` / `skill_listing` /
`total_tokens_reminder`——正是 `claude.rs:1552-1554` 注释点名并被整条丢弃的那几种。

因此切片 2 的真实前置不是「把 `resumeSessionAt` 塞进 `_meta`」，而是先做**锚点管道**：
让该轮 parentUuid 链上最后一条 chain entry 的 uuid 活到 `MessageTurn` 的可选字段
（不改现有 `id` 的 `turn-<digits>` 语义）。没有这一步，forkAtMessage 在一整类会话上会
确定性失败，且失败不可重试。

## 8. JSONL 兼容器的边界

先补一条原文没有的正面证据：**Claude 的原生 fork 在文件层就是“复制 transcript”**。
`acp/background_watch.rs:1256-1266` 记录了这个布局——fork 把父 transcript 逐条复制进新的
session 文件，每条记录保留其**原始的、fork 之前的时间戳**，然后在**文件头**写入 fork 时
刻的新元数据记录（`queue-operation`、`mode` 等）。Codeg 的 watcher 必须懂这个布局才能算
对增量基线：头部元数据按字节偏移在前、按时间戳在后，所以基线只认 `user`/`assistant` 记
录，否则会把整段复制来的历史误判成新内容重复渲染。

这条事实对 §7.1 的意义是：claude 的“按消息 fork 走文件手术”并不是要发明一种新格式，而是
在复刻 Claude 自己已经在做的事（复制 + 换 sessionId + 新文件名），只是多一步截断。它降低
了该路线的格式风险，但**不豁免**下面任何一条军规——尤其第 4、7 条：复制来的记录仍带
`parentUuid` 链、工具调用配对和 `isSidechain` 段，截断点之后的悬空引用只有用原生 CLI 做
一次只读 Resume 才能证伪。

直接编辑 Claude、Codex 或其他 Harness 的原生 JSONL 风险很高。文件中可能含有父子 UUID、
sidechain、工具调用配对、compaction、checkpoint、重复事件和 provider 私有版本字段。仅删除
几行文本可能得到“看起来能读、实际上不能可靠 Resume”的伪 Session。

如果某个 Harness 没有 SDK、服务接口或稳定 CLI，而 JSONL 兼容器具有足够价值，必须遵守：

1. 原文件只读，绝不原地截断或改写；
2. 只支持经过版本识别的格式，未知版本立即拒绝；
3. 克隆到新的原生 Session ID 和新文件；
4. 校验事件顺序、父子引用、工具调用配对和末端可恢复性；
5. 使用临时文件、完整校验和原子替换；
6. 保留可恢复备份和操作审计；
7. 用对应原生 CLI 执行一次只读 Resume 验证后才导入 Codeg；
8. UI 标记为 experimental，并展示格式版本与失败回退方式。

JSONL 兼容器不能成为统一领域模型，也不能让 Codeg 复制一套聊天正文充当第二事实源。

## 9. 谱系与现有字段兼容

不得复用 `conversation.parent_id`。它当前表示 delegation 子会话，改变语义会破坏 Codeg
现有查询与测试。Fork 路径显式写 `parent_id: Set(None)`（`manager.rs:2228`）。

**已实现（切片 1）**：独立表 `fork_relation`，migration `m20260820_000002`。

落地字段（`db/entities/fork_relation.rs`，`db/migration/m20260820_000002_fork_relation.rs`）：

```text
id
source_conversation_id   -- FK to conversation, ON DELETE CASCADE
target_conversation_id   -- FK to conversation, ON DELETE CASCADE
relation_kind            -- CHECK: fork_head / fork_at_message / handoff; default fork_head
anchor                   -- nullable JSON. head fork is NULL; reserved for forkAtMessage
created_at
```

`persist_fork_outcome` 在 INSERT C2 的同一事务里 `record_fork_head`
（`manager.rs:2274`，`fork_lineage_service.rs:25-45`），`anchor` 写 NULL。
查询端点 `conversation_fork_lineage` 已双模式暴露（`web/router.rs:65-66`）。

`relation_kind` 的另外两个取值是预授权：历史分叉与跨 Harness handoff 不需要再改 schema。
`anchor` **就是**留给 `forkAtMessage` 的槽——切片 2 的锚点管道产出应写进这里，而不是
复用 `MessageTurn.id`。

主界面默认只显示「分叉自……」徽标和跳转；完整关系树放在详情或命令中。UI 徽标本身尚未做
（切片 1 只把端点就位）。

## 10. UI 与安全规则

- 当前末端 Fork 保留现有“分叉发送”入口；
- 历史用户消息的菜单按能力显示“从这里分叉”“编辑后分叉”；
- 分叉成功后打开新 Session，并允许立即重命名或加入 Collection；
- 原 Session 不自动归档，不丢失当前 Workbench 位置；
- 文件恢复使用单独入口，先显示变更预览、未提交文件风险和目标 cwd；
- “仅对话”“仅文件”“对话和文件”只有在能力真实存在时才出现；
- 运行中 turn、消息锚点失效或 Session 未完整导入时阻止操作并说明原因；
- experimental provider 不得默认执行，并且失败后不能留下半个会话或部分恢复的目录。

## 11. 实施阶段

### 阶段 0：能力事实与现有 Fork 加固

- 建立按 provider/version/session 的能力矩阵；
- 为 Codeg 当前 ACP `forkHead` 增加回归测试和可理解的错误信息；
- 明确 UI 文案，不再把 Head Fork 与 Historical Fork 混称；
- 记录轻量 Fork 关系，但不复用 `conversation.parent_id`。**已落地**（§9）。

### 阶段 1：Claude/Codex 历史分叉

- 增加 Claude native provider；
- 增加 Codex app-server provider；
- 保存和验证消息锚点；
- 支持“从这里分叉”和“编辑后分叉”；
- 验证新 Session 可由原生 CLI/Desktop Resume。

### 阶段 2：文件检查点

- 分离对话操作和文件操作；
- 增加 diff 预览、cwd 校验、未提交改动保护和恢复结果；
- 只在 provider 有真实 checkpoint 时启用组合恢复。

### 阶段 3：其他 Harness 与协议演进

- 跟踪 ACP 是否加入历史消息锚点或 truncate/checkpoint 能力；
- 对 Grok、Gemini 等逐个评估结构化接口；
- 只有自动化接口稳定、可验证时才从 experimental 升级。

## 12. 验收标准

1. 不支持 Fork 的 Harness 不显示可点击的 Fork；
2. Head Fork 与 Historical Fork 在能力和文案中可以区分；
3. 历史分叉产生可被原生 Harness Resume 的新 Session；
4. 原 Session 的 ID、消息和原生文件保持不变；
5. 编辑后分叉不会改写原消息；
6. 文件恢复前显示目标 cwd 和变更预览；
7. 文件恢复失败不会留下未说明的部分状态；
8. Fork 谱系不占用 `conversation.parent_id`；
9. 外部客户端创建或继续的分支能被 Codeg 同步发现；
10. provider 升级或能力变化后，UI 能重新探测而不是继续展示陈旧按钮。

## 13. 调研来源

- [ACP Session Fork RFD](https://agentclientprotocol.com/rfds/session-fork)
- [ACP Registry capability matrix](https://github.com/agentclientprotocol/registry/blob/main/.protocol-matrix/latest.md)
- [ACP Protocol Discussions（Session history、分页与 replay 提案）](https://github.com/agentclientprotocol/agent-client-protocol/discussions)
- [Zed Agent Panel：Editing Messages 与 Checkpoints](https://zed.dev/docs/ai/agent-panel)
- [Zed Parallel Agents](https://zed.dev/docs/ai/parallel-agents)
- 切片 2 第一轮实测：
  [FINDINGS-A 锚点清点](./fork-rewind-slices/FINDINGS-A-anchor-inventory.zh-CN.md)、
  [FINDINGS-B claude fork](./fork-rewind-slices/FINDINGS-B-claude-fork-audit.zh-CN.md)、
  [FINDINGS-C codex fork](./fork-rewind-slices/FINDINGS-C-codex-fork-audit.zh-CN.md)
- 第二轮规格与交叉结论：[ROUND2-SPECS](./fork-rewind-slices/ROUND2-SPECS.zh-CN.md)、
  [调度台账](./fork-rewind-slices/README.zh-CN.md)

本 RFC 的 Codeg、Paseo 与 Zed 代码结论来自 2026-08-15 的本地源码审计；切片 2 的协议与
parser 事实来自 2026-08-20 的 FINDINGS A/B/C。协议与 provider 能力会变化，实施前必须
重新核对版本和 capability，不把本次快照当成永久事实。
