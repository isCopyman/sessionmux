# 宪章① 阶段一 派工规格（包 I / 包 J）

> 上位简报：`docs/session-workbench/REFACTOR-1-TRANSCRIPT-SCAFFOLD-BRIEF.zh-CN.md`
> **本文件与简报冲突时，以本文件为准**（本文件已按主线实际代码核过一遍，见 §1 勘误）。
>
> 基线：`codex/session-message-v1` 最新（**`4d1bf83a`**，已含 fork/rewind 第三轮
> `ca922f4b` 与模型体检 RFC；第三轮九项门禁已在主线全绿）。协调分支 `wt/charter1`。
> **永不 push，永不动 main，不跑 `pnpm tauri build`。**
>
> **动手前另读**：`docs/session-workbench/MODEL-AUDIT-RFC-2026-08-21.zh-CN.md`
> 的 **①** 与 **⑤** 两节（见本文 §1.5，已摘出与本批相关的部分）。

## 0. 共同硬规矩（前三轮的真实事故换来的，逐条都是）

1. **`git worktree add` 之后立刻 `cd` 过去**；写任何文件前先 `pwd` 确认。
2. **绝不在协调者工作树内运行任何写命令。** 协调者的树是 `codeg-wt/charter1`。
   你的会话 cwd **初始可能就是它**——机制限制，你负责走开。
3. **禁止 `git add -A` / `git add .`**，只用显式路径。（前几轮三次误写全靠这条挡住。）
4. **前端门禁前先 `pnpm install --frozen-lockfile`**（新 worktree 没自己的 node_modules，
   否则直接 `Cannot find module '@vitejs/plugin-react'`）。
5. **交付帖设 `expects_reply=false`。**
6. **门禁四条跑全**：`pnpm eslint .`、`pnpm tsc --noEmit`、`pnpm vitest run`、`pnpm build`。
   少跑一条就可能漏——上一轮就有人两个 clippy 面全绿、错只在第三面。
   本阶段是**纯前端**，不动 Rust；万一动了，补 cargo 三连并用独立 `CARGO_TARGET_DIR`。
7. **磁盘**：并行 worktree 会撑爆盘（上一轮真出过 `os error 112`）。纯前端包影响小，
   但跑完若发现盘紧，在 Room 说一声。
8. 同一步骤失败两次停手，Room 报事实（命令 + 报错原文）等指令。判断题上交，**不要猜**。
9. **发现规格/简报写错，直接在 Room 说**——前三轮工人挑出过协调者三处硬错，都省掉了
   下游连锁错误。这是明确鼓励的行为。
10. 报告要带证据：`file:line`、命令原文输出、测试计数。没有工具输出的"我验证了"等于没验证。

## 1. 勘误与协调者已核事实（**动手前必读**，简报有两处要修正）

| 简报写的 | 实际（协调者用 `git ls-tree` / `grep` 核过） |
|---|---|
| `src/components/**conversations**/message-list-view.tsx` | **`src/components/message/message-list-view.tsx`**（简报路径错） |
| （未提规模） | 该文件 **1739 行**；配套测试 `message-list-view.test.tsx` **583 行** |
| "既有测试约 200+ 行" | 实际 **583 + 242 = 825 行**（另含 `virtualized-message-thread.test.tsx`） |

行号锚点**基本准确**，已复核：
`mergeConsecutiveAssistantTurns` **:562**、`applyCollaborationTimelineProjection` **:258**、
`loadOlderTurns` **:1000-1003**、`AutoScrollOnSend` **:936**、
`useStickToBottomContext` 导入 **:78** / 使用 **:941**、find 状态机 **:1303 起**
（`findEntries` :1307、`findOpen/findQuery` :1325-1326、`ConversationFindEntry` 来自
`@/lib/conversation-find`）、`ConversationMessageNav` 使用 **:1719**。

**协调者发现的一处简报未提的耦合（重要）**：find 与分页**不独立**。`message-list-view.tsx:1303-1306`
的注释原文：

> Search semantic message text across the loaded transcript, **then page older
> history while a non-empty query is active** so the final count covers the
> complete native Session rather than only the initial tail window.

即 **find 激活时会驱动分页继续加载更早历史**。所以骨架件 #4（infinite scroll）与 #5（find）
**必须由同一个人做**，而且重接后要保证这条"查找时自动翻页直到覆盖全会话"的行为不丢。
**这是包 I 不再往下拆的根本原因。**

其它已核事实：

- `VirtualizedMessageThread` 在 `src/components/message/virtualized-message-thread.tsx`
  （**481 行**，配 242 行测试）。这是 #1 抽 generic 的源头。
- `src/lib/conversation-find-highlight.ts`（209 行）的行选择器是**模块级常量**
  `FIND_ROW_SELECTOR = "[data-virtual-item-index], [data-find-row-index]"`（`:107`）。
  简报说它"已泛化"——准确说是**按并集泛化，不是按参数泛化**。阶段二要的
  `[data-room-post-content]` 参数化**目前不存在**。**阶段一不要动它**，但请在汇报里
  确认这个缺口（阶段二会用到）。
- `src/components/rooms/use-room-find.ts`（183 行）阶段一**不改**，但你的
  `useTranscriptFind` API 必须能覆盖它的用例；覆盖不了的写进汇报。

## 1.5 与模型体检 RFC 的交叉切面（2026-08-21 新增，**动手前读**）

主线新落 `MODEL-AUDIT-RFC-2026-08-21.zh-CN.md`（六切面手术清单 + 手术顺序）。
与本批相关的两条，**它们是约束不是建议**：

- **切面 ①（Room/Session 双时间线底座）裁决原文：「最后做，先冻结范围。UI 面统一
  由转录面骨架先行（进行中）」**——"进行中"指的就是你们这一批。含义很硬：
  **骨架只统一 UI 面，数据面一律不碰**。不要造跨 Room/Session 的共享数据模型，
  不要动 `models/message.rs` / `models/collaboration.rs` / `types.ts` 里的时间线类型，
  不要给 Room 侧编造 phase/usage/model 字段。RFC 明写数据面统一要等 ②③④⑥ 之后立项，
  你现在"顺手统一"会把后面的决定重新打包一遍。
- **切面 ⑤（序列化双约定 camelCase/snake_case）裁决：赶在新增字段前统一，但前置是
  盘点 JSON 持久化模型**。对你们的含义：**阶段一不要改任何序列化字段名**
  （骨架件是纯 UI 组件/hook，本来也不该碰；这条是防"顺手规整"）。

其余四切面（② human 身份、③ 回复义务、④ 根 folder 推导、⑥ 工作台归属）与本批无
写入面交叉；⑥ 正由交互原语批（包 K/L）并行执行，边界见 §0 与
`INTERACTION-PRIMITIVES-DISPATCH.zh-CN.md`。

## 2. 分包与理由

| 包 | 内容 | 为什么这么分 |
|---|---|---|
| **I** | 骨架件 #1 #2 #4 #5 + message-list-view 重接 | 四件**全部**触碰 `message-list-view.tsx` / `virtualized-message-thread.tsx`；#4 与 #5 还有 §1 那条耦合。拆开必然合并地狱。 |
| **J** | 骨架件 #3（DateSeparator + `withDateSeparators`） | 纯新文件 + 单测，**零重接**（简报明写阶段一不要求 message-list-view 使用）。与 I 无共享写入面，可完全并行。 |

---

## 包 I：转录面骨架主体（架构级）

分支 `wt/charter1-scaffold`，目录 `codeg-wt/charter1-scaffold`。

### 交付四件（全部放 `src/components/transcript/`，各配新单测）

1. **`VirtualizedTranscript`** —— 泛型容器。props：`items: readonly T[]`、
   `getItemKey(item, index)`、`renderItem(item, index)`、`emptyState?`、
   `scrollApiRef?`（暴露 `scrollToIndex`）。每行包裹元素带
   `data-virtual-item-index={index}`。**类型上不得出现 `MessageTurn`。**
   首选路线：从 `virtualized-message-thread.tsx` 抽 generic，让它变成骨架之上的
   会话特化壳（而不是复制一份）。
2. **`TranscriptStickToBottom`** —— 封装 `useStickToBottomContext`（`use-stick-to-bottom`），
   暴露 `isAtBottom` / `scrollToBottom` / `scrollApiRef` 接线；把 `AutoScrollOnSend`
   （`:936`）重接到它。
3. **`useTranscriptInfiniteScroll`** —— `{hasOlder, isLoading, onLoadOlder}`；滚近顶部
   自动加载更早，**prepend 后视口保持稳定**；替换现有分页触发（`:1000-1003` 的
   `loadOlderTurns` 一带）并保持行为一致。
4. **`useTranscriptFind`** —— 从 `:1303-1494` 抽出（含 Ctrl+F 监听）。
   入参 `{entries: {key, index, text}[], isActive, rootRef, viewportElement?}`，
   返回 `{query, current, total, open, onNext, onPrevious, onClose, onQueryChange, focusToken}`。
   **高亮复用 `src/lib/conversation-find-highlight.ts`，不要另写。**
   **必须保住 §1 那条 find↔分页耦合行为。**

### 硬约束（违反即打回）

- **message-list-view 对外行为与 DOM 契约零变化**：`data-virtual-item-index`、
  `data-conversation-search-content`、`data-letter-event-id` 全保留；虚拟化 key
  `${phase}-${role}-${msg.id}` **不变**（分页 prepend 前后必须稳定）。
- **明确不进骨架、本阶段不动**：`mergeConsecutiveAssistantTurns`(:562)、
  `ConversationMessageNav`、`applyCollaborationTimelineProjection`(:258)、compaction 卡、
  用户消息复制。留在 message-list-view。
- **`rooms-page.tsx` / `use-room-find.ts` 一律不改**（阶段二的事）。
- **既有 825 行测试不得删、不得放松断言**，只允许导入路径机械调整。
- **不新增 npm 依赖**；骨架件之间**不得循环依赖**。

### 必补测试（简报点名的最大风险）

**off-screen 命中能滚动定位 + 高亮** 的集成测试。`findEntries[i].index` 必须严格等于
虚拟化显示顺序索引；分页后要重算。这条没有测试就是没做完。

---

## 包 J：日期分隔（纯件，与 I 并行）

分支 `wt/charter1-dateseparator`，目录 `codeg-wt/charter1-dateseparator`。

### 交付

放 `src/components/transcript/`：

1. **`TranscriptDateSeparator`** 组件（展示件）。
2. **`withDateSeparators(items, getTimeMs, formatLabel)`** 纯函数：按日期在条目流中插入
   separator 虚拟项，返回 **discriminated union**：`{kind:"item", item, index}` /
   `{kind:"separator", label, key}`（字段名你定，但必须是 discriminated union，
   且**必须在汇报里给出最终签名**——阶段二要照着接）。
3. 两者各配单测。

### 关键要求

- **纯函数必须可单独测试**，不依赖 React/DOM。
- 边界必须覆盖：空数组、单条、同日多条、跨日、**跨年**、乱序输入（明确你的契约是
  "假定已排序"还是"内部排序"——**在汇报里写清**）、时间戳缺失/非法。
- 时区口径要明确（用本地时区还是 UTC）——**这是判断题，你先给出你的选择和理由，
  在汇报里标出来让协调者拍板**，不要默默定。
- **阶段一不要求 message-list-view 使用它**，不要去改 message-list-view。
- 日期文案走 i18n（仓库已有 next-intl，10 语种）；若你认为阶段一不该定文案，
  在汇报里说明并给出最小可用方案。

### 边界

- 只新增文件，**不改任何既有文件**（i18n messages 除外，若你决定加文案键）。
- 不碰包 I 的地盘（虚拟化 / stick-to-bottom / find / 分页）。

---

## 3. 汇报格式（两个包相同，简报 §工序 要求）

Room 回帖，`expects_reply=false`，包含：

1. 改动文件清单
2. **骨架件最终 API 签名**（阶段二要照着接，必须准确）
3. 重接点清单（包 J 无）
4. **没做的事 + 拿不准的点**（这一栏空着基本说明没认真想）
5. 分支名 + commit 列表
6. **门禁四条的输出摘要**（数字要真实，协调者会在合并树上重跑复核）
