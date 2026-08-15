# Codeg Session 历史能力子 RFC

> 状态：Draft  
> 调研日期：2026-08-15  
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

截至 Codeg `b6e1d904`（0.25.0）：

- `src/lib/api.ts` 的 `acpFork` 提供前端调用；
- `conversation-detail-panel.tsx` 的 `handleForkSend` 先 Fork，再把草稿发送到新 Session；
- `message-input.tsx` 只在连接报告支持 Fork 时显示“分叉发送”；
- `src-tauri/src/acp/manager.rs` 的 `fork_session` 负责协议调用、并发闸门与数据库落盘；
- 当前会话行切换到新原生 Session，另建 sibling 行保留原 Session 历史；
- `conversation.parent_id` 仍专用于 delegation，当前 Fork 没有通用谱系字段。

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

2026-08-15 的 ACP registry 能力矩阵显示：

- `claude-acp 0.67.0` 声明 Session Fork；
- `codex-acp 1.2.0` 未声明 Session Fork；
- `grok-build 1.0.4` 未声明 Session Fork；
- 另有部分 Agent 已实现当前末端 Fork。

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
