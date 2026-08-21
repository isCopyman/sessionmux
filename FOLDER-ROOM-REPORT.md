# O54 施工报告：分类树文件夹菜单「新建群聊」（带文件夹作用域）

工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/folder-room`（分支 `wt/folder-room`）。未改主仓 `D:/code/revisiting/work/repo_audit/repos/codeg`。未 push。

开工先读了 `docs/session-workbench/LESSONS.zh-CN.md`（L1 退出码纪律、L8 i18n 十语）。

## 做了什么

### 1. 带作用域的打开方式

`src/contexts/create-room-dialog-context.tsx`：

- 保留 `open` / `setOpen`（侧栏、欢迎页全局入口不变）。
- 新增 `folderScopeId: number | null` 与 `openForFolder(folderId)`。
- `openForFolder` 设作用域再打开；`setOpen(false)` / `setOpen(true)` 都会清掉作用域，避免下次全局打开粘住上次的文件夹。

`src/components/layout/workspace-chrome-controller.tsx` 把 `folderScopeId` 传给已有的 `CreateRoomDialog`。弹窗仍只在打开时挂载。

### 2. 建群弹窗按文件夹过滤

`src/components/rooms/create-room-dialog.tsx`（复用 O56 的 `SessionPicker` + `createRoomWith`，**未改** `session-picker.tsx` / `room-membership.ts`）：

- 有 `folderScopeId` 时，传给 `SessionPicker` 的 candidates 只留 `folder_id === scope` 的 live 会话（`roomMemberCandidates` 已排除归档）。
- 无作用域时行为与原来相同。
- 标题旁小 chip：能从 `useAppWorkspaceStore.folders` 解析到则 `alias ?? name`，文案 `Room.createFolderScope`（「仅 {name}」）；解析不到用 `Room.createFolderScopeGeneric`。
- 发起人预选：当前激活会话不在作用域内（或不存在 / 已归档）则不预选。
- 作用域内无候选：复用 `createNoCandidates`。

### 3. 分类树菜单

`src/components/collections/collection-tree.tsx`：

- 图标用该文件已有的 `Users`（房间行同一套 lucide）。
- 菜单文案复用侧栏键 `Folder.sidebar.newRoom`（en「New room」/ zh-CN「新建群聊」），未新开菜单键。

**Path 文件夹行**（`renderPath`，`data-collection-path`）：原先没有右键菜单，只有悬停「新建会话 / 新建分类」小按钮。本期给该行加了右键菜单：「新建会话」（已有 `onNewSession` 时）之后是「新建群聊」，`onSelect` → `openForFolder(root.id)`。没有加新的大按钮。

**Collection 行**（任务书点名的含 `newChild` 那组菜单）：在「新建会话」之后同样加「新建群聊」，右键与 ⋯ 悬停菜单对齐。`onSelect` → `openForFolder(item.root_folder_id)`。没有 `root_folder_id` 的遗留 Collection 不出现该项（与「新建会话」同一门槛）。

## 取舍：Collection 级作用域本期不做

Collection 行也有「新建会话」，但候选人**不按 Collection 成员过滤**，只按该 Collection 的 `root_folder_id`（canonical Path）过滤。

原因：一个 Collection 里的会话可以来自不同文件夹（含 worktree），「这个分类里的人」语义要另定。任务书明确本期不做 Collection 级作用域。

所以右键 Research（`root_folder_id = 7`）和右键 Path `project`（`id = 7`）打开的是同一作用域。嵌套 Collection（Sources）同样落到祖先 Path，不落到 Collection id。

未做工作台树（workbench-tree）入口。

## i18n（十语）

新键（en / zh-CN / zh-TW / ja / ko / es / de / fr / pt / ar）：

| 键 | en |
| --- | --- |
| `Room.createFolderScope` | Only {name} |
| `Room.createFolderScopeGeneric` | This folder only |

复用：`Folder.sidebar.newRoom`、`Room.createNoCandidates`、`Room.createTitle`。

## 测试

- `collection-tree.test.tsx`：Collection 右键 / ⋯ 菜单出现「New room」且在「New Conversation」之后，点击调用 `openForFolder(7)`；Path 行右键同样 `openForFolder(7)`；无 Path 的遗留 Collection 不出现该项。
- `create-room-dialog.test.tsx`：无作用域仍列出全部 live 会话（回归）；有作用域只列该文件夹；chip 用 name / alias / 通用文案；激活会话不在作用域则不预选；空态复用 `createNoCandidates`。
- `create-room-dialog-context.test.tsx`：`openForFolder` 写入作用域，关闭或 `setOpen(true)` 后复位。
- 现有 `session-picker.test.tsx` 继续绿。

## 明确没动

- `src/components/rooms/session-picker.tsx`
- `src/lib/room-membership.ts`
- 后端、schema
- workbench-tree
- 没有新的大按钮

## 验证

每条「我验证了」对应下面真实命令。PowerShell 里退出码用 `$LASTEXITCODE`，日志在 `%TEMP%\o54-verify\`，判定读日志正文（L1：不用管道当退出码）。

### `pnpm install --frozen-lockfile`

我验证了退出码 0：`Done in 12.5s using pnpm v11.9.0`（worktree 原先没有 `node_modules`）。

### `pnpm vitest run src/components/collections src/components/rooms src/contexts`

我验证了 `VITEST-EXIT:0`（第二次全量；第一次 Path 行测试用 `getByTitle` 匹配带换行的 `title` 失败，改成 `findByText("project")` 后复跑）。日志摘要：

```
Test Files  18 passed (18)
     Tests  495 passed (495)
```

其中本任务相关文件：

- `collection-tree.test.tsx`：52 passed
- `create-room-dialog.test.tsx`：9 passed
- `session-picker.test.tsx`：5 passed
- `create-room-dialog-context.test.tsx`：2 passed

### `pnpm eslint src/components/collections src/components/rooms src/contexts`

我验证了 `ESLINT-EXIT:0`。`t2.log` 为空（无诊断）。

### `pnpm tsc --noEmit`

我验证了 `TSC-EXIT:0`。`t3.log` 为空（无诊断）。命令耗时 33.59s。

### 额外：`pnpm vitest run src/i18n/messages.test.ts`

新键走十语，我额外跑了键集合对等测试：`I18N-EXIT:0`，`Test Files 1 passed` / `Tests 9 passed`。

## 未在浏览器里点

任务书要求 vitest / eslint / tsc，没有要求起桌面应用。分类树右键和建群弹窗过滤只在组件测试里走通。没有用浏览器点真实侧栏。
