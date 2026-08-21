# O67 看板可读性三件套（纯前端）施工报告

工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/board-ux`（分支 `wt/board-ux`）。
未改主仓 `codeg`。未 push。未动 `src-tauri/`、`src/components/settings/`、`src/components/chat/`。

本批 = 看板设计 §2 三件：缺口② 分组维度 + 缺口③ 甲案（attention 列内分层，本 worktree 已有实现，补 DOM 测试）+ 缺口④ 卡片活动点。

## 做了什么

### ① 分组维度（缺口②）

- 工具栏加「分组」选择器：无（默认）/ 按项目 / 按 agent。Collection 与 Room 不进选项。
- 实现为**列内二级分段**：四列地基不动，每列内部按组分段；段头 = 小标题 + 该段计数。
- 列表视图加同样的分组头。`grouping=none` 走原来的卡片/行列表，不插入段头。
- 选择记进 `localStorage` 键 `workspace:tasks-board-grouping`，写法照抄同文件里 `viewMode`。
- 「按项目」且 `folderFilter` 已选定单一项目时，退化成一段且**不显示段头**。
- 未改 `columnForStatus`、未改拖拽语义（无 folder 时 To-do 仍不可持久化排序）、未改 10 态状态机。

新文件：`src/components/tasks/board-grouping.ts`、`task-group-header.tsx`。
存储：`src/lib/tasks-board-filter-storage.ts`。

### ② attention 列分层（缺口③甲案）

本 worktree 里排序 + 视觉分层已在（P1 落地）：

- 排序：`board-columns.ts` `failed → awaiting_input → review → merging`，级内 freshest。
- 视觉：`attentionSurfaceClass` + `StatusChip` / `statusAccent`（failed destructive；awaiting_input amber + 仅它有轻脉冲；review 中性；merging `opacity-70`）。
- `merging` 仍归 attention（`board-columns.ts` 注释：点合并瞬间不能跳列）。

本批**没有重写**这套实现，补了看板 DOM 渲染顺序测试，以及卡片表面类名测试。

### ③ 卡片活动点（缺口④）

- `running` + 会话连接 `prompting` → 绿色（`bg-emerald-500`）脉冲点。
- `running` 但不在 Prompting → 同色静止点。
- 无 `conversation_id` → 不渲染点。
- 数据源复用 `acp-connections-context` 连接快照；查找键照抄工作台树 O34（`connection_id` 优先，否则 `conv-{folder}-{agent}-{conversation}`）。纯前端推导，无后端字段。
- 不做「最后输出 X 秒前」。

新文件：`src/components/tasks/task-activity.ts`。

### i18n

`Tasks` 命名空间新键（十语齐）：`groupBy` / `groupByNone` / `groupByFolder` / `groupByAgent` / `groupUngrouped`。
en / zh-CN / zh-TW / ja / ko / es / de / fr / pt / ar。

## 明确没做

- 未动 `src-tauri/`、schema、10 态状态机、CAS、四列地基。
- 未拆第五列。
- 未做 Room / Collection 分组。
- 未改拖拽语义。
- 未动 settings / chat 目录。

## 测试

新增/加宽：

| 文件 | 条数 | 覆盖 |
| --- | --- | --- |
| `board-grouping.test.ts` | 8 | 无分组 / 按项目 / 按 agent / folderFilter 退化 / 未分组桶 |
| `task-activity.test.ts` | 7 | 连接键、pulse / static / 无 conversation_id |
| `tasks-page.test.tsx` | 10 | 分组 DOM、localStorage 读写恢复、attention 渲染顺序、活动点接线 |
| `task-card.test.tsx` | 18（原 14 + 4） | 脉冲/静止/无点；attention 表面类 |
| `tasks-board-filter-storage.test.ts` | 6（原 4 + 2） | grouping 往返与垃圾值回退 |
| `src/i18n/messages.test.ts` | 9 | 十语键集合对等 |

`tasks-page.test.tsx` 具体断言：

- grouping=none：无 `task-group-header`，四列标题仍在。
- grouping=folder / agent：段头出现、段内计数正确、四列仍在。
- 分组选择写入 `workspace:tasks-board-grouping` 并在重挂后恢复。
- folderFilter 选定单一项目后按项目分组不再画段头。
- attention 列标题顺序：Broke（failed）→ Ask me（awaiting_input）→ Review me → Merging one。
- running+Prompting 有 `animate-pulse`；running 非 Prompting 无脉冲；无 `conversation_id` 不渲染点。

## 验证

命令经 Git bash 脚本执行，每步 `> /tmp/*.log; echo STAGE-EXIT:$?`（L1：不用管道判定退出码）。

我验证了 `pnpm install --frozen-lockfile` **INSTALL-EXIT:0**。日志尾：`Already up to date` / `Done in 302ms using pnpm v11.9.0`。

我验证了 `pnpm tsc --noEmit` **TSC-EXIT:0**。`/tmp/a.log` 为空。

我验证了 `pnpm eslint src` **ESLINT-EXIT:0**。0 error；1 条既有 warning（`src/components/message/message-list-view.tsx:261` `'outbound' is assigned a value but never used`），本批未改该文件。

我验证了 `pnpm vitest run src/components/tasks src/lib src/i18n` **VITEST-EXIT:0**。日志尾：

```
Test Files  144 passed (144)
     Tests  2006 passed (2006)
Duration    26.72s
```

其中本批相关：`tasks-page.test.tsx` 10 passed、`board-grouping.test.ts` 8 passed、`task-activity.test.ts` 7 passed、`task-card.test.tsx` 18 passed、`tasks-board-filter-storage.test.ts` 6 passed、`messages.test.ts` 9 passed。

同一步未失败两次。prettier 初检 11 处格式，`--fix` 后 eslint 再跑为 0 error。

未在真实 Codeg 桌面窗口里点看板：本环境没有可驱动的应用会话。分组/分层/活动点靠 DOM 测试与既有 token 类名，没有浏览器实机走查。
