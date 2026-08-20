# Codeg Session 历史能力子 RFC

> 状态：Draft  
> 调研日期：2026-08-15  
> 对账修订：2026-08-20 —— §3、§4 已按当前代码重写（原文基于 0.25.0，与 0bfb86a0
> 之后的实现有 5 处脱节；脱节清单见
> [能力矩阵与可行路径调研](./SESSION-FORK-REWIND-SURVEY-2026-08-19.zh-CN.md) §1）。
> §1、§2、§5–§13 的设计结论未受影响。  
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

消息锚点也不能假定是 Codeg 当前消息表的自增 ID。不同 Harness 可能使用 assistant UUID、
user message ID、turn ID 或 checkpoint ID；适配器需要保存可验证的原生锚点。

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
  `db/entities/conversation.rs:68`），当前 Fork 没有通用谱系字段——这正是 §9 要新增独立
  关系表的原因，也是它不能复用 `parent_id` 的原因。
- 支持面是**纯运行时探测**，不是按 `agent_type` 写死：`connection.rs:4264-4268` 读
  `initialize` 回复里的 `sessionCapabilities.fork`，经 `ForkSupported` 事件落到
  `session_state.rs:664`，再推前端。这与 §6“能力矩阵必须来自运行时探测”的要求一致。

### 3.7 仍然成立的结论

这套实现可以继续作为 `forkHead` 的 ACP provider，不应推倒重写。需要补的是：

- 明确记录来源与目标的 Fork 关系；
- 历史消息级锚点；
- 原生 Claude/Codex provider；
- 文件恢复的独立能力与安全确认；
- UI 对能力差异的诚实呈现。

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

需要实测才能填的空（本切片未做）：claude-agent-acp 0.69.0 是否仍声明 fork（registry 注释
未重申）；codex-acp 1.4.0 是否声明 ACP `session/fork`（注释未记录）。**注意不要把
codex-acp 1.4.0 内部用的 app-server `thread/fork` 当成它声明了 ACP fork**——1.4.0 用
`thread/fork` 是为了实现 AIR `agentFileChangeReport`（`registry.rs:580-592`），那是另一
条通道；但它同时也是 §7.2 那条路线可行的正面证据：官方适配器自己就在用 app-server 的
thread fork。

因此 Codeg 当前基于能力探测显示按钮是正确的，但 ACP provider 只能承诺 `forkHead`。

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

优先使用 Anthropic Agent SDK 的 Session Fork/Resume 语义，保存 Codeg 消息与原生 UUID 的映射。
历史分叉应创建新原生 Session，原会话只读保留。只有 provider 能证明消息锚点仍有效时才显示
“从这里分叉”。

### 7.2 Codex

优先使用 Codex app-server 的结构化 thread fork/rollback 能力，不通过模拟 TUI 按键实现。
Codeg 需要先验证目标 Codex 版本、事件序列和分叉后的 Session 可被原生客户端继续发现，再把
能力标为 stable。

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
现有查询与测试。

需要谱系时新增独立关系记录，概念上至少包含：

```text
source_conversation_id
target_conversation_id
relation_kind
source_message_id / provider_anchor（可选）
provider
created_at
metadata
```

实现字段必须在 schema RFC 中结合现有迁移重新命名和审核。主界面默认只显示“分叉自……”徽标
和跳转；完整关系树放在详情或命令中，避免 Session 越多侧栏越难用。

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
- 记录轻量 Fork 关系，但不复用 `conversation.parent_id`。

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

本 RFC 的 Codeg、Paseo 与 Zed 代码结论来自 2026-08-15 的本地源码审计。协议与 provider
能力会变化，实施前必须重新核对版本和 capability，不把本次快照当成永久事实。
