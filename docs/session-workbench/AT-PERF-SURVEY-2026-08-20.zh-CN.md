# @ 补全性能与缓存调研报告（2026-08-20 夜，r-atperf 只读调研）

> 回答 HANDOFF-2026-08-20-release8 §P1。调查方式：源码阅读 + 对 Claude Code 发行
> 二进制做字符串取证，**无运行时实测**。同日更早的
> `AT-COMPLETION-RECON-2026-08-20.zh-CN.md` 中"单 root 假设"结论**已过期**
> （多 root 支持已合入），引用时注意。

## 一、结论先行

**用户的直觉"每敲一个字符就重新触发一次搜索"，观感对，归因错。** 敲键时
**没有任何后端往返**——文件清单、会话列表、git 提交在首次打开 @ 面板后全在前端
内存里，且已有 150ms 防抖。让人觉得"又在重搜"的是三件事：

1. **每敲一字，面板把已有结果整个清空、显示"搜索中…"**，最少闪 150ms。数据一直
   在内存里，纯渲染层自找。这是感知问题的**主要来源**。
2. **前缀细化不复用**：`r`→`re`→`rea` 每次都从头扫最多 5 万条文件+全部会话，
   明明新结果必是旧结果子集。Claude Code 和 Codex 都利用了这个性质，我们没有。
3. **会话组每键两趟全量遍历+全量对象分配**，而后端会话列表**无 LIMIT**
   （`conversation_service.rs:1172` 直接 `query.all`）。

顺带发现两个同类问题：**待回复面板无事件合并**（房间刷屏时连打 N 次全表扫描级
查询）、**会话中心后端查询无分页**。

两家 CLI：**都不是每键重扫磁盘**，都是"建一次清单+内存模糊过滤"。我们的缓存
**驻留范围最小**（每个 composer 实例各一份，卸载即失效）且**文件清单从不失效**。

## 二、我方三条链路现状

### 链路 1：@ 提及面板

| 环节 | 位置 |
|---|---|
| 触发检测 | `mention-suggestion.ts:67-84`（IME/代码块不触发 :74-77） |
| query→取数 | `suggestion-popup.tsx:206-227`，150ms 防抖（`FETCH_DEBOUNCE_MS` :28）+ AbortController |
| 数据合成 | `use-reference-search.ts:324-395` |
| 文件 | 从 `filesRef.current` 读（:381），来自 `useFileTree` |
| 会话 | `listAllConversations()` promise 缓存 `sessionsRef`（:331-343） |
| 提交 | `gitLog(path, 100)` 缓存 `commitsRef`（:346-365） |
| 缓存失效 | 窗口 focus 清会话/提交（:315-322）——**不碰文件缓存** |
| 分组过滤 | `buildReferenceGroups`（:101-197），每组上限 50（:35） |
| 文件清单 | `use-file-tree.ts:89-131`，按 root 组合键缓存 |
| 后端 | `folders.rs:4715-4735` → `walk_workspace_files_multi`（:4751-4784），5 万条上限（:4387）+ 10s deadline（:4398） |

**敲键后端往返 = 0**（逐环节确认）。每键实际代价：

- (a) **结果被清空**：`stale = result.query !== state.query`（suggestion-popup.tsx:202），
  stale 时 `activeGroup` 置 null（:243）、`flat` 置空（:249）、渲染走 loadingLabel
  （:547-550）、tab 计数归零（:509）。**观感的直接来源。**
- (b) **buildReferenceGroups 全量重扫**：文件组命中 50 break（:124-127）但不命中
  扫完 5 万条，匹配纯 `.includes()`（:121）无评分；会话组先给每条算
  `sessionMentionTitle` 建重名表（:141-145）再全量 `sessionToSuggestion` 最后才
  过滤（:146-153），两趟全量+无 break，后端无 LIMIT。
- (c) **零前缀复用**。
- (d) Room 层：`room-mention-search.ts:156-175` 每次新建 3s race（从不取消底层，
  :135-136 注释自认）；成员重排规模有界，可忽略。

**缓存驻留**：`useReferenceSearch` 挂在 4 处（message-input.tsx:434、
rooms-page.tsx:460、task-message-composer.tsx:151、automation-editor.tsx:164），
各带独立缓存；搜索对话框另挂一份 `useFileTree`（search-command-dialog.tsx:89）。
**同 folder N 个标签 = N 次对同一 root 的完整扫描**，互不共享；文件清单从不失效
（会话期间新建的文件在 composer 重挂载前永不出现）。

### 链路 2：会话中心

`conversation-manage-dialog.tsx` 约 :848-947：**已有 300ms 防抖 + cancelled 防
乱序（做得好，不用动）**；但任一筛选器变化=完整重查、前端零缓存；后端 `list_all`
（`conversation_service.rs:1098-1176`）**无 LIMIT/分页**（:1172），Title 搜索
`LIKE '%s%'`（:1157）走不了索引。问题不是重复查，是**单次查询无上限**。

### 链路 3：待回复面板

`use-collaboration-unread-overview.ts`：事件驱动不轮询（:67-79，先订阅再取首屏
:64-66，方向对）；但**无合并/防抖**——:70 只要事件带 conversationIds 就立刻
reload。`requestSequenceRef` 只丢迟到响应，每个事件仍真实打一次
`unread_overview`（`collaboration_service.rs:2166-2250`：无 LIMIT、每行 4 个相关
子查询 + 第 5 条聚合，O(会话数×4)）。**风险场景：房间刷屏 → N 次重查背靠背。**

## 三、两家 CLI 机制

### Claude Code CLI（v2.1.236）

复原源码（repo_audit/repos/claude-code，非官方）+ 真实二进制字符串取证交叉验证：
`skipped index rebuild`、`cache refresh completed`、`getPathsForSuggestions`、
`FileIndex`、`tengu_file_suggestions_*` 等均在 330MB 发行产物中出现 → **机制可信，
数值常量（15/64/5000ms）只能当参考**（压缩 mangle 掉了）。

- **进程级单例索引**，惰性构造；每键 `startBackgroundCacheRefresh()`
  （fire-and-forget）+ `fileIndex.search(query, 15)` **纯内存**。
- 刷新双重节流：`.git/index` mtime 变了立刻刷，否则 5s 一次
  （注释原话 "This prevents every keystroke from spawning git ls-files"）；
  路径列表采样指纹没变就**跳过重建**。
- 发现：git 仓 `git ls-files --recurse-submodules`（5s 超时）+ 未跟踪文件后台
  merge（10s）；非 git 回退 `rg --files`。
- 渐进可用：每 4ms 让出事件循环，`readyCount` 支持查询半建索引，建完发信号重跑
  上次查询。
- 匹配：26 位字母位图 O(1) 预筛 + fzf 风格评分 + top-k 堆。
- **无按键防抖**——被节流的是刷新不是查询。

### Codex CLI（开源实证，HEAD 868ac158）

- TUI 为每个搜索根维持**长期存活 session**（`tui/src/file_search.rs:16-26`）；
  每键只 `update_query`（=往 channel 发信号，"cheap relative to re-walking"）；
  **整个 session 只 walk 一次**（walker/matcher 双线程 + nucleo Injector）。
- **前缀增量匹配**（关键，`file-search/src/lib.rs:506-515`）：
  `query.starts_with(&last_query)` 时 `reparse(..., append=true)`，只在存活候选里
  继续筛。
- matcher 内部 10ms tick 合并通知；流式快照（`walk_complete`）；三层取消；
  无墙钟超时；limit 20。

### 对比

| 维度 | codeg | Claude Code | Codex |
|---|---|---|---|
| 每键重扫磁盘 | 否 | 否 | 否 |
| 缓存驻留 | **每 composer 实例一份** | 进程级单例 | per-root session |
| 失效策略 | **文件清单从不失效** | git mtime + 5s 节流 + 指纹跳过 | session 内静态 |
| 按键防抖 | 150ms UI | 无（节流刷新） | 10ms tick |
| 前缀复用 | **无** | 位图预筛 | **append 增量** |
| 匹配算法 | 裸 `.includes()` 无排序 | 位图+评分 top-k | nucleo |
| 渐进结果 | 无 | 有 | 有 |
| 后端硬保护 | **5 万条+10s（比两家都严）** | 分段超时 | 靠取消 |

## 四、方案建议（按感知收益/成本排序）

- **P0-1 停止每键清空面板**（改动最小收益最大）：stale 时继续渲染上次结果，
  保留"不可选中"安全语义（:176-180 注释的正确性约束必须保留），渲染与可选中
  拆开（渲染用 `activeGroup.items`，可选判定用 `!stale`）。改点 :243/:249/
  :547-555/:509，不动取数。
- **P0-2 前缀复用**：`lastResultRef {query, groups}`，新 query 前缀于旧 query 时
  在旧 items 上过滤。**正确性边界：仅当该组 `truncated === false` 才复用**，按组
  判断。
- **P0-3 修会话组**：title 预算一次、先过滤再适配、够 50 break。
- **P1-1 文件清单缓存提到模块级共享**（Map<rootsKey,{files,loadedAt,promise}>，
  hook 退化为订阅者；含 in-flight 去重）。
- **P1-2 文件清单便宜失效**：TTL 60s + focus 刷新，stale 不阻塞渲染（先旧后新）。
  **不上 fs watcher**（成本远高于"最多滞后一分钟"的严重度；三家没一家做 push 监听）。
- **P1-3 待回复面板事件合并**：:70 加 200-300ms trailing debounce。几行改动，
  挡掉三条链路里最重的单次查询。**优先级提到与 P0 同级。**
- **P2 会话中心分页**（可晚）：list_all 加 limit/offset（默认 500），UI 沿用
  truncated 惯例。LIKE 索引问题到几千会话前不值得动。300ms 防抖不要动。
- **明确不做**：fs watcher、后端进程内清单缓存、frecency、自研位图/nucleo、
  后端取消令牌链路、动 Room 3s race。
- **顺带发现单独立项**：`src/lib/file-search-match.ts` 的 `rankFileMatches`
  分级评分器已存在且被搜索对话框用上，@ 面板文件组还在跑裸 `.includes()`——
  给 @ 面板接上=零新依赖的相关性改进，但要等 P0-2 落地才划算（它按设计扫完
  全部候选）。

## 五、未验证部分

- 无运行时计时/profiling；"5 万条子串扫描毫秒级"是 JS 常识估计。
- Claude Code 数值常量未经证实（机制有二进制实证）。
- Claude Code UI 层是否另有防抖未通读（只确认 useTypeahead 订阅 indexBuildComplete）。
- 两家 CLI 部分与我方现状部分同一证据水准（同一人读码）。

## 领导决策（2026-08-20 夜）

实施 **P0-1 / P0-2 / P0-3 / P1-1 / P1-2 / P1-3**（低风险、边界清晰、全前端），
等 w-ime（碰 suggestion-popup.tsx）和 w-roomcount（碰 use-collaboration-unread-
overview.ts）合并后派一个工人做，避免冲突。**P2 分页与 rankFileMatches 接入
留给用户拍板**（前者改默认行为，后者是相关性口味问题）。
