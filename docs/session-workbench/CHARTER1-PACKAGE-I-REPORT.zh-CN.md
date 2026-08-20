# 包 I 汇报：转录面骨架主体

分支：`wt/transcript-scaffold`（基于 `c99f1ed6`）。不发 Room，本文件即规格 §3 的六项汇报。

## 0. 锚点核对（动手前）

规格 §1 行号在基线 `c99f1ed6` 上动手前 grep 结果（`message-list-view.tsx` 仍 1787 行，与协调者声明一致）：

| 锚点 | 现场 |
|---|---|
| 文件总行数 | 1787 |
| `applyCollaborationTimelineProjection` | :266 |
| `mergeConsecutiveAssistantTurns` | :570 |
| `UserMessageForkButton` | :769 |
| `AutoScrollOnSend` / `useStickToBottomContext` | :981 / :986 |
| `loadOlderTurns` | :1046-1049 |
| find 状态机起点 / `findEntries` | :1351-1355 |
| `findOpen` / `findQuery` | :1373 / :1374 |
| `searchingOlderHistory` + find→分页 effect | :1398-1420 |
| `ConversationMessageNav` | :1767 |

抽取后 `message-list-view.tsx` 现为 **1614 行**（find 状态机与 stick-to-bottom 直调迁出）。既有测试文件未改：`message-list-view.test.tsx` **583** + `virtualized-message-thread.test.tsx` **242** = **825**。

`FIND_ROW_SELECTOR`（`src/lib/conversation-find-highlight.ts:107`）仍是模块级并集常量 `"[data-virtual-item-index], [data-find-row-index]"`，**没有**按参数泛化到 `[data-room-post-content]`。阶段一未动该文件（规格要求）。

---

## 1. 改动文件清单

新增（`src/components/transcript/`，包 J 的 date-separator / with-date-separators 未改）：

- `virtualized-transcript.tsx`
- `virtualized-transcript.test.tsx`
- `virtualized-transcript-find.test.tsx`（off-screen 命中滚动定位 + 高亮）
- `transcript-stick-to-bottom.tsx`
- `transcript-stick-to-bottom.test.tsx`
- `use-transcript-infinite-scroll.ts`
- `use-transcript-infinite-scroll.test.ts`
- `use-transcript-find.ts`
- `use-transcript-find.test.tsx`

修改：

- `src/components/transcript/index.ts`（追加四件导出；既有包 J 导出原样保留）
- `src/components/message/virtualized-message-thread.tsx`（薄壳 re-export）
- `src/components/message/message-list-view.tsx`（重接）
- `PACKAGE-I-REPORT.md`（本文件）

未改：`rooms-page.tsx`、`use-room-find.ts`、`conversation-find-highlight.ts`、既有 825 行测试、Rust、序列化字段、数据模型。

---

## 2. 骨架件最终 API 签名

### `VirtualizedTranscript<T>`

规格最低集：`items: readonly T[]`、`getItemKey(item, index)`、`renderItem(item, index)`、`emptyState?`、`scrollApiRef?`（`scrollToIndex`）。每行包裹 `data-virtual-item-index={index}`。类型文件中无 `MessageTurn`。

为保持 Session 零行为变化，实现还保留了从 `VirtualizedMessageThread` 抽出来的兼容 props（**是否留在泛型件上：待拍板**）：

```ts
export interface VirtualizedThreadViewState {
  scrollOffset: number
  atBottom: boolean
  virtualItemCount: number
  virtualizerCache: CacheSnapshot | null
}

export type TranscriptScrollApi = {
  scrollToIndex: (index: number, opts?: ScrollToIndexOpts) => void
}

export interface VirtualizedTranscriptProps<T> {
  items: readonly T[]
  getItemKey: (item: T, index: number) => string
  renderItem: (item: T, index: number) => ReactNode
  emptyState?: ReactNode
  scrollApiRef?: RefObject<TranscriptScrollApi | null>
  // --- 兼容 extras（抽自 VirtualizedMessageThread，阶段二 Room 可能只要最低集）---
  itemSize?: number
  bufferSize?: number          // default 800
  gap?: number                 // default 16
  padding?: number             // default 16
  className?: string
  contentClassName?: string
  contentProps?: Omit<MessageThreadContentProps, "children" | "className">
  hasOlder?: boolean
  isLoadingOlder?: boolean
  onLoadOlder?: () => void
  loadOlderLabel?: string
  loadingOlderLabel?: string
  prependEpoch?: number
  prependScopeKey?: string | number
  initialViewState?: VirtualizedThreadViewState | null
  onViewStateChange?: (state: VirtualizedThreadViewState) => void
}

export function shouldShiftForPrepend(
  prev: { scope: string | number | undefined; epoch: number },
  next: { scope: string | number | undefined; epoch: number }
): boolean

export const VirtualizedTranscript: <T>(
  props: VirtualizedTranscriptProps<T>
) => JSX.Element
```

近顶翻页触发（`LOAD_OLDER_THRESHOLD_PX = 240` 的向下穿越）和 prepend `shift` 留在本容器内（绑 virtua）。`VirtualizedMessageThread` 是其上的会话特化壳：同路径 re-export，既有测试不用改导入。

### `TranscriptStickToBottom`

```ts
export interface TranscriptStickToBottomApi {
  isAtBottom: boolean
  scrollToBottom: StickToBottomContext["scrollToBottom"]
  scrollRef: StickToBottomContext["scrollRef"]
  stopScroll: StickToBottomContext["stopScroll"]
}

export function useTranscriptStickToBottom(): TranscriptStickToBottomApi

export const TranscriptStickToBottom: (props: {
  autoScrollSignal?: number
}) => null
```

`useTranscriptStickToBottom` 暴露 `isAtBottom` / `scrollToBottom` / `scrollRef` / `stopScroll`。组件把 `AutoScrollOnSend` 重接为「`autoScrollSignal` 上升则 `scrollToBottom`」。virtua 的 `scrollApiRef` 仍在 `VirtualizedTranscript` 上（规格把这个名字写在 StickToBottom 条下，见 §4 待拍板）。

### `useTranscriptInfiniteScroll`

```ts
export interface UseTranscriptInfiniteScrollArgs {
  hasOlder: boolean
  isLoading: boolean
  onLoadOlder: () => void
  isActive?: boolean            // default true
  findOpen?: boolean            // find↔分页接线
  findQuery?: string
}

export interface UseTranscriptInfiniteScrollResult {
  hasOlder: boolean
  isLoading: boolean
  onLoadOlder: () => void
  searchingOlderHistory: boolean
}

export function useTranscriptInfiniteScroll(
  args: UseTranscriptInfiniteScrollArgs
): UseTranscriptInfiniteScrollResult
```

`searchingOlderHistory = findOpen && findQuery.length > 0 && (hasOlder || isLoading)`。find 激活且仍有更早页时反复调用 `onLoadOlder`（与抽出前 `:1398-1420` 同条件）。

### `useTranscriptFind`

```ts
export interface TranscriptFindEntry {
  key: string
  index: number   // 必须等于虚拟化显示顺序索引，不是 entries 数组下标
  text: string
}

export interface UseTranscriptFindArgs {
  entries: readonly TranscriptFindEntry[]
  isActive: boolean
  rootRef: RefObject<HTMLElement | null>
  viewportElement?: HTMLElement | null     // 规格形状
  viewportRef?: RefObject<HTMLElement | null>  // Room OverlayScrollbars 后填；待拍板
  scrollToIndex?: (                       // 规格未列；虚拟化 off-screen 必需；待拍板
    index: number,
    opts?: { align?: "start" | "center" | "end" | "nearest" }
  ) => void
}

export interface TranscriptFindState {
  query: string
  current: number   // 1-based；无命中为 0
  total: number
  open: boolean
  onNext: () => void
  onPrevious: () => void
  onClose: () => void
  onQueryChange: (query: string) => void
  focusToken: number
}

export function useTranscriptFind(args: UseTranscriptFindArgs): TranscriptFindState
```

高亮复用 `conversation-find-highlight.ts`，未另写。内部把 `{key,index,text}` 映射为 `ConversationFindEntry`。

---

## 3. 重接点清单

1. **`AutoScrollOnSend`**（`message-list-view.tsx:973`）改为渲染 `<TranscriptStickToBottom autoScrollSignal={signal} />`，不再直调 `useStickToBottomContext`。
2. **Find 状态机**（原 `:1351-1542`）换成 `useTranscriptFind`。`findEntries` 现为 `{ key: item.key, index: threadIndex, text }`；`index` 是 `threadItems` 显示顺序（跳过 typing/compaction 等无文本行，不改写成 entries 下标）。
3. **Ctrl+F**：`isActive: isActive && showMessageNav`（保住原 `showMessageNav` 门控）。
4. **off-screen 定位**：`scrollToIndex` → `scrollApiRef.current?.scrollToIndex`。
5. **find↔分页**：`useTranscriptInfiniteScroll({ hasOlder, isLoading, onLoadOlder: handleLoadOlder, isActive, findOpen: find.open, findQuery: find.query })`。`searchingOlderHistory` 交给 `ConversationFindBar.searching`。
6. **`VirtualizedMessageThread`** 仍由 message-list-view 使用，props `hasOlder` / `isLoadingOlder` / `onLoadOlder` 走 infinite-scroll 回传（值与原来相同）。
7. **DOM 契约未动**：`data-virtual-item-index`（容器）、`data-conversation-search-content` / `data-letter-event-id`（`HistoricalMessageGroup`）、虚拟化 key `` `${phase}-${role}-${msg.id}` ``（`:1118`）。

骨架件之间无循环依赖：`VirtualizedTranscript` → `useTranscriptStickToBottom`；find / infinite-scroll 互不 import，由宿主显式接线。

---

## 4. 没做的事 + 拿不准的点

### 没做（规格禁止或明确阶段二）

- 未改 `rooms-page.tsx` / `use-room-find.ts`。
- 未参数化 `FIND_ROW_SELECTOR`（阶段二要用 `[data-room-post-content]` 的缺口仍在）。
- 未动 `mergeConsecutiveAssistantTurns`、`ConversationMessageNav`、`applyCollaborationTimelineProjection`、compaction 卡、用户消息复制。
- 未动数据面（RFC 切面 ①）与任何序列化字段名（切面 ⑤）。
- 未改包 J 文件；未新增 npm 依赖；未跑 cargo。

### `useTranscriptFind` 对 `use-room-find` 的覆盖

能覆盖：Ctrl/Cmd+F（含 monaco/iframe/`[data-native-find-scope]` 豁免）、literal match list、CSS Custom Highlight、`viewportRef` / `viewportElement` 交给 `revealConversationFindRange`、返回值形状与 Room 一致。

缺口（阶段二接线时要补，不在本包发明）：

1. Room 的 `ConversationFindEntry` 字段是 `{itemKey, threadIndex}`，骨架是 `{key, index}`，调用方要做一层映射。
2. Room 的 `findPagedFromRef`（同一 `oldestId` 且后端仍报 truncated 则停步，防空转）**没有**进 `useTranscriptInfiniteScroll`。Session 原实现也没有这道闸。
3. 高亮行选择器仍是并集常量，不是参数。Room 现阶段靠 `data-find-row-index` + `data-conversation-search-content`，不靠 `[data-room-post-content]`。

### 待拍板

1. **`scrollToIndex?` 是否进入 `useTranscriptFind` 正式入参。** 规格只写了 `{entries, isActive, rootRef, viewportElement?}`。不传就保不住 Session 的 off-screen 先 `scrollToIndex` 再高亮。已做成可选并写出。
2. **`viewportRef?` 是否与 `viewportElement?` 并存。** 规格是值；Room 的 OverlayScrollbars viewport 是回调后填的 ref。只留值则阶段二 Room 接不上。
3. **`VirtualizedTranscript` 是否继续内置 `MessageThreadContent` / `max-w-3xl` / `MessageScrollProvider`。** 抽 generic 且 Session 测试零改导入，所以整段壳跟着走了。Room 阶段二可能要无 Session chrome 的容器。
4. **兼容 extras**（`hasOlder` / `prependEpoch` / `initialViewState` 等）留在泛型件还是收回会话壳。
5. **`scrollApiRef` 挂在 StickToBottom 还是 VirtualizedTranscript。** 实现挂后者（virtua `scrollToIndex`）；前者暴露的是 stick-to-bottom 的 `scrollToBottom` / `scrollRef`。
6. **Ctrl+F `event.target instanceof Element` 守卫。** 抽出前是 `as HTMLElement` 后 `.closest`。测试里打到 `window`/`document` 会炸。改成 Element 才调用 `closest`，行为对真实 DOM 目标不变。

---

## 5. 分支名 + commit 列表

- 分支：`wt/transcript-scaffold`
- 基线：`c99f1ed6`
- 本包提交：`refactor(transcript): package I — VirtualizedTranscript scaffold + message-list-view rewire`（见 git log；未 push）

---

## 6. 门禁输出摘要

工作树执行，无 cargo。

```
pnpm install --frozen-lockfile
```

exit 0。`Packages: +1255`，`Done in 15s using pnpm v11.9.0`。

```
pnpm eslint .
```

exit 0。`✖ 3 problems (0 errors, 3 warnings)`，三处均在既有文件、非本包引入：

- `src/components/chat/message-input.tsx:304` `_sourceConversationId` unused
- `src/components/conversations/session-bulk-action-bar.tsx:471` exhaustive-deps
- `src/components/message/message-list-view.tsx:261` `_outbound` unused（抽取前已存在）

```
npx tsc --noEmit
```

exit 0。无诊断。

```
pnpm test
```

exit 0。`Test Files  379 passed (379)` / `Tests  4831 passed (4831)` / `Duration  85.49s`。

本包新增测试（含必补集成）：

- `virtualized-transcript.test.tsx` 5
- `transcript-stick-to-bottom.test.tsx` 3
- `use-transcript-infinite-scroll.test.ts` 7
- `use-transcript-find.test.tsx` 6
- `virtualized-transcript-find.test.tsx` 3（off-screen 命中滚动定位+高亮；prepend 后 `index` 仍等于显示顺序；`searchingOlderHistory` 直到翻完）

既有 825 行测试文件未删、未改断言、未改导入路径。针对性复跑：`message-list-view.test.tsx` 21、`virtualized-message-thread.test.tsx` 8、`virtualized-message-thread.test.ts` 3，全绿。

```
pnpm build
```

exit 0。`Compiled successfully in 16.3s`；`Generating static pages using 31 workers (32/32)`。未跑 `pnpm tauri build` / cargo。
