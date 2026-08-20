# 宪章① 阶段二汇报 — Room 时间线迁上转录面骨架（O21 / O28 / O29）

- 分支：`wt/charter1-phase2`
- 基线：`39ac02ce`（已含骨架包 I+J）
- 性质：纯前端 UI 面。未动 Rust、未动序列化、未给 Room 编造 phase/usage。

## 1. 改动清单

### 交付面（规格要求）

| 文件 | 做什么 |
|---|---|
| `src/components/rooms/rooms-page.tsx` | 时间线从 `events.map()` 换成 `VirtualizedTranscript`；`withDateSeparators` 插日期栏；`MessageThread` + `TranscriptStickToBottom` 打开落底/跟随最新；`useTranscriptInfiniteScroll` 接 find↔分页；保留骨架自带的「加载更早」按钮作退路；`findPagedFromRef` 防空转闸留在本文件 |
| `src/components/rooms/use-room-find.ts` | 改为薄封装 `useTranscriptFind`。`{itemKey, threadIndex}` → `{key, index}`；高亮选择器 `article[data-room-post-content]` |
| `src/components/rooms/room-post-body.tsx` | 帖子内容节点加 `data-room-post-content`（搜索正文仍走既有 `data-conversation-search-content`） |
| `src/lib/conversation-find-highlight.ts` | `applyConversationFindHighlights` 第四参可选 `rowSelector`，默认=`FIND_ROW_SELECTOR`（Session 调用零变化）；常量改为 export |

### 骨架透传（见 §3）

| 文件 | 做什么 |
|---|---|
| `src/components/transcript/use-transcript-find.ts` | 可选 `rowSelector?` 透传到 highlighter。不传 = 现行为 |

### 测试

| 文件 | 做什么 |
|---|---|
| `src/components/rooms/rooms-page.test.tsx` | virtua jsdom mock（默认全量渲染，既有断言不放松）；新增 5 条集成 |
| `src/lib/conversation-find-highlight.test.ts` | 可选 selector 命中 `article[data-room-post-content]`、不误伤 virtua 包装行 |

未改：`message-list-view.tsx`、`virtualized-message-thread.tsx`、`virtualized-transcript.tsx`、包 J 日期件、i18n 文案键、Rust、任何序列化字段。

## 2. 重接点

1. **O21 虚拟化**：`datedEntries`（`withDateSeparators` 的 discriminated union）交给 `VirtualizedTranscript`。`getItemKey` = separator 用 union 的 `key`（`date-yyyy-mm-dd`），item 用 `event.id`。**不用数组下标。** Room 无 phase/流式，未引入假 phase。
2. **O28 落底 / 跟随**：时间线外包 `MessageThread`（`StickToBottom`，`initial="instant"`，`key={roomId}`）。`RoomTranscriptStick` 把 `scrollRef` 填进 find 的 `viewportRef`，并渲染 `TranscriptStickToBottom`；发帖成功后 `stickSignal++`。新帖到达若视口在底部，由 stick-to-bottom 库跟随。
3. **O29 日期分隔**：`withDateSeparators(events, createdAt, formatTranscriptDayLabel, { timeZone: "local" })`。分隔项是虚拟化容器里的非消息行；`getItemKey` 走 separator key。
4. **向上翻页**：近顶穿越仍在 `VirtualizedTranscript` 内（virtua `onScroll`）。手动按钮是同一条 loader 行（`hasOlder` / `loadOlderLabel=t("loadOlder")`）。`prependEpoch` 在真正 prepend 到新帖后 +1，`prependScopeKey=roomId`，prepend 后 `shift=true`。
5. **find**：`findEntries[i].threadIndex` = **含日期分隔后的显示顺序索引**。分隔项不进 entries 但占显示位（例如 `[sep, a, sep, b]` → a=1, b=3）。`useRoomFind` 映射到骨架 `{key, index}`，并把 `scrollToIndex` 接到 virtua。
6. **`findPagedFromRef` 防空转闸**（Room 特有，骨架没有）：find 驱动的 `onLoadOlder` 在 `rooms-page` 包一层——同一 `oldestId` 且后端仍 `truncated` 则停步。手动按钮 / 近顶翻页走未加闸的 `loadOlder`。
7. **高亮**：Room 传 `article[data-room-post-content]`，当前命中标在带 `id="room-event-…"` 的 `<article>` 上（Ctrl+F 命中计数 / 上下跳转 / `data-conversation-find-current` 仍在）。Session 调用仍三参，默认并集选择器。
8. **保留现状**：同说话人 5 分钟 `sameSpeaker` 仍按**事件数组**的前一条算，不按显示行；`RoomPostBody` markdown（O18）不动；`RoomReplyProgressBadge` 不动。

未使用 `bare` opt-out：Room 用已有 `gap={0}` / `padding={8}` 适配，不拆 Session chrome。Session 825 行测试未改导入、未改断言。

## 3. 没做的 + 拿不准的

### 没做

- 未给 `VirtualizedTranscript` 加 `bare`（现有 extras 够用）。
- 未把 `findPagedFromRef` 塞进 `useTranscriptInfiniteScroll`（规格：留在 rooms 侧）。
- 未给 Today/Yesterday 加 10 语 i18n 键；日期文案走包 J 的 `formatTranscriptDayLabel`（Today/Yesterday 英文，其它日期跟 `useLocale()`）。
- 未改 Session 面任何调用；未动数据面 / 序列化 / Rust。
- 未新增 npm 依赖。

### 已做但超出「唯一允许动骨架的口子」的一处（请拍板）

规格只允许给 `VirtualizedTranscript` 加 `bare`。但「`use-room-find` 基于 `useTranscriptFind`」+「Room 侧传含 `[data-room-post-content]` 的选择器」+「Session 调用一律不改」三者同时成立，必须让 `useTranscriptFind` 把可选 `rowSelector` 透传给 `applyConversationFindHighlights`。已做成可选，默认 undefined = 现常量路径；`use-transcript-find.test.tsx` / `virtualized-transcript-find.test.tsx` / `message-list-view.test.tsx` 未改。

若不允许这处透传：只能在 rooms 侧复制一份高亮 effect，或停手改规格。

### 待拍板

1. **日期时区**：阶段二接包 J 默认 `"local"`。Room 是否应改 UTC？
2. **Today/Yesterday 文案**：现为英文 helper。是否补 `Room.dateToday` / `Room.dateYesterday` 十语键？
3. **`useTranscriptFind.rowSelector`** 是否进入骨架正式入参（见上）。
4. **打开落底**：依赖 `MessageThread` 的 `initial="instant"` + 首次挂载时 events 已在（`hydrated` 门）。没有单独测 stick-to-bottom 库的像素滚动（jsdom 无 layout）；集成测试断言时间线在 `role="log"` 的 StickToBottom 树里。

## 4. 门禁输出摘要（带计数）

工作树执行，无 cargo。`pnpm install --frozen-lockfile` 先行。

```
pnpm install --frozen-lockfile
```

exit 0。`Packages: +1255`，`Done in 12.5s using pnpm v11.9.0`。

```
pnpm eslint .
```

exit 0。`✖ 3 problems (0 errors, 3 warnings)`，三处均在既有文件、非本包引入：

- `src/components/chat/message-input.tsx:304` `_sourceConversationId` unused
- `src/components/conversations/session-bulk-action-bar.tsx:471` exhaustive-deps
- `src/components/message/message-list-view.tsx:261` `_outbound` unused

```
npx tsc --noEmit
```

exit 0。无诊断。

```
pnpm test
```

exit 0。`Test Files  379 passed (379)` / `Tests  4842 passed (4842)` / `Duration  63.22s`。

本包新增 / 保留：

- `rooms-page.test.tsx` **42**（既有 37 未删未放松 + 5 条新集成：日期分隔后的 find 索引、打开落底 scroller、off-screen 命中滚动定位+高亮、prepend 后 `shift`+日期分隔、findPagedFromRef 同 oldestId 停步）
- `conversation-find-highlight.test.ts` **9**（+1 可选 selector）
- 既有 Session：`message-list-view.test.tsx` 21、`virtualized-message-thread.test.tsx` 8，全绿

```
pnpm build
```

exit 0。`Compiled successfully in 8.4s`；`Generating static pages using 31 workers (32/32)`。未跑 `pnpm tauri build` / cargo。
