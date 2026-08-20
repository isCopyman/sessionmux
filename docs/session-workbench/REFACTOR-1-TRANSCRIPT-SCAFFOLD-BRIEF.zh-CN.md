# 重构宪章① 转录面骨架 — 编队派工简报

- 日期：2026-08-21
- 派工：人类用户（群聊）→ Room 协调者拆工调度
- 性质：重构优先批第一项（旗舰）。用户已拍板"先重构和抽象构建，防各种小 bug"。
- 上游侦察：已完成（能力清单/差距/边界/风险见下），工人不必重新全仓摸底，但行号以实际代码为准。

## 目标

把会话转录面（`src/components/conversations/message-list-view.tsx`）里的通用能力沉淀成可复用的**转录面骨架**，然后把 Room 时间线迁上骨架。最终一并解决：

- **O21**：Room 时间线全量渲染无虚拟化（渐进渲染缺失）
- **O28**：Room 面板每次打开滚动条在最上方，无滚动锚定/跟随最新
- **O29**：Room 时间线无日期分隔

## 阶段一：骨架抽取（先行，行为零变化）

新目录 `src/components/transcript/`，五个独立导出件，每件配新单测：

1. **VirtualizedTranscript**（泛型容器）：props = `items: readonly T[]`、`getItemKey(item, index)`、`renderItem(item, index)`、`emptyState?`、`scrollApiRef?`（暴露 scrollToIndex）。每行包裹元素带 `data-virtual-item-index={index}`。类型上不得引用 MessageTurn。首选路线：从现有 `VirtualizedMessageThread` 抽 generic，让它变成骨架之上的会话特化壳。
2. **TranscriptStickToBottom**：封装现有 stick-to-bottom 上下文（`useStickToBottomContext`），暴露 isAtBottom / scrollToBottom / scrollApiRef 接线；message-list-view 的 `AutoScrollOnSend` 重接到它。
3. **TranscriptDateSeparator** 组件 + `withDateSeparators(items, getTimeMs, formatLabel)` 纯函数：条目流按日期插 separator 虚拟项，返回 discriminated union（`{kind:"item"}` / `{kind:"separator"}`）。阶段一只建件+单测，不要求 message-list-view 使用（供阶段二 Room 用）。
4. **useTranscriptInfiniteScroll**：`{hasOlder, isLoading, onLoadOlder}`，滚近顶部自动加载更早，prepend 后视口保持稳定；替换 message-list-view 现有分页触发逻辑（约 994-1003、1353-1372，`loadOlderTurns`）并保持行为一致。
5. **useTranscriptFind**：从 message-list-view 查找状态机（约 1303-1494，含 Ctrl+F 监听 1410-1432）抽出。入参 `{entries: {key, index, text}[], isActive, rootRef, viewportElement?}`，返回 `{query, current, total, open, onNext, onPrevious, onClose, onQueryChange, focusToken}`；高亮复用 `src/lib/conversation-find-highlight.ts`（已泛化支持 `[data-virtual-item-index], [data-find-row-index]` 行选择器），不要另写高亮。message-list-view 重接到它。

**明确不进骨架、阶段一不动**：`mergeConsecutiveAssistantTurns`（562-717）、`ConversationMessageNav`（1520-1581）、协作投影 `applyCollaborationTimelineProjection`（258-427）、compaction 卡（537-551）、用户消息复制（719-759）。这些留在 message-list-view。

**阶段一硬约束**：

- message-list-view 对外行为与 DOM 契约零变化：`data-virtual-item-index`、`data-conversation-search-content`、`data-letter-event-id` 全保留；虚拟化 key `${phase}-${role}-${msg.id}` 不变（分页 prepend 前后稳定）。
- `rooms-page.tsx`、`use-room-find.ts` 阶段一一律不改；但骨架 API 必须能覆盖 use-room-find 的用例，发现缺口写进汇报。
- 既有测试不得删除、不得放松断言；只允许导入路径机械调整。
- 不新增 npm 依赖；骨架件间不得循环依赖。

## 阶段二：Room 时间线迁移（依赖阶段一合并后开工）

`rooms-page.tsx` 时间线（现为 `events.map()` 全量渲染）迁上骨架：

- VirtualizedTranscript 接管渲染（key 用 `event.id`，分页 prepend 前后稳定，绝不能用数组索引）→ O21
- TranscriptStickToBottom：打开房间落底、跟随最新 → O28
- withDateSeparators 按 `createdAt` 插日期栏 → O29
- useTranscriptInfiniteScroll 替换手动"加载更早"按钮逻辑（保留按钮作退路）
- `use-room-find.ts` 迁到 useTranscriptFind；高亮行选择器参数化为 `[data-room-post-content]`
- Room 无 phase/流式概念：骨架侧默认 persisted，不引入假 phase

**阶段二保留现状**：同说话人 5 分钟窗口聚合（`sameSpeaker`）是 Room 自己的展示逻辑，留在 rooms-page，不搬进骨架；消息合并（assistant run merge）Room 不需要，不迁。

## 已知风险（侦察产出，动工前读一遍）

1. **find 定位精确性是最大风险**：`findEntries[i].index` 必须严格等于虚拟化显示顺序索引，分页/插日期栏后要重算。**必补集成测试：off-screen 命中能滚动定位+高亮**。
2. 日期 separator 是虚拟项，虚拟化容器必须支持非消息行。
3. message-list-view 相关既有测试约 200+ 行（虚拟化、merge 缓存、find、协作投影），重接后全部要绿。
4. 侦察行号可能有轻微漂移，以实际代码为准；若结构严重不符，先在群聊上报再动手。

## 工序与门禁（每阶段同样要求）

- 基于 `codex/session-message-v1` 最新拉 `wt/` worktree 分支开发；**永不 push origin，永不动 main**。
- commit message 带 `charter-1` 字样，阶段二另带 `O21/O28/O29`。
- 完工自跑门禁并把输出摘要贴进群聊汇报：`pnpm eslint .`、`pnpm tsc --noEmit`、`pnpm vitest run`、`pnpm build`（纯前端活不动 Rust；若动了 Rust 补 cargo clippy/test 三连，用独立 CARGO_TARGET_DIR）。
- 汇报格式：改动文件清单 / 骨架件最终 API 签名 / 重接点清单 / 没做的事与拿不准的点 / 分支名+commit 列表 / 门禁输出摘要。@人类 验收合并。

## 人员配置（用户指令 2026-08-21）

- Claude Code 工人：**opus5（claude-opus-5）**，思考强度按任务复杂度取高档（骨架抽取属架构级）。
- Grok 工人：**grok4.6，思考强度拉满**。
- 协调者：拆工、排期（fork/rewind 在飞轮次自行权衡先后）、把关汇报质量；不必亲自写码。
