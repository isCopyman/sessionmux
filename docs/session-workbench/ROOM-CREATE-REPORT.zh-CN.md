# O51 施工报告：先建群、进群拉人

工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/room-create`（分支 `wt/room-create`）。未改主仓。

## 做了什么

### A. 后端最少成员 2 → 1

- `unique_member_ids`：空列表拒绝，1 人通过。文案改为 `A Room requires at least one Session member`。
- 任务书写的是 `len() < 1`。`cargo clippy -- -D warnings` 命中 `clippy::len_zero`，改成语义相同的 `unique.is_empty()`。
- `remove_member`：`remaining <= 2` 改为 `<= 1`。读完实现后：**没有**单独的「发起人不可移除」规则；房主与其他成员一样，只要房间里还剩别人就可以离开。下限只禁止拆到 0 人。
- 发起人不在 `member_conversation_ids` 里时仍会自动补进列表（原 290–296 逻辑未动）。
- 测试：空成员建群失败（min-1）；新增 `create_room_accepts_a_single_live_member`；移除成员的下限断言从 2 改为 1。

### B. 建群弹窗：标题 + 发起人

`create-room-dialog.tsx`：

- 多选 checkbox 改为 radio 单选发起人（沿用搜索列表与行样式）。
- 预选当前激活页签对应的 live Session（`tab-store` 的 `rawTabs` + `activeTabId`）；页签不是会话或解析不到则不预选。
- 选了发起人即可点建群；成功后仍 toast + `openRoom`。
- `defaultRoomTitle` 单人走 `createTitleSolo`（如 `{name}'s room` / `{name} 的群聊`）；两人以上仍是 `A / B`（`…`），bulk 路径不调用这个新参数。

**未改** `session-bulk-action-bar.tsx`。

### C. 房内拉人

`rooms-page.tsx`：

- 头部「⋯」左侧加 `outline` + `sm` 的带文字按钮（`UserPlus` + `t("addMember")`），`onClick=setAddOpen(true)`。菜单里原项保留。侧栏 `h-6 w-6` 的「+」保留。
- `detail.members.length <= 1` 且时间线为空时，emptyState 换成邀请引导（说明文案 + 默认 `Button`）。两人以上仍用原来的 `timelineEmpty`。

未改 add-member 弹窗逻辑。

### D. i18n

十语（en / zh-CN / zh-TW / ja / ko / es / de / fr / pt / ar）同步：

- 改：`Room.createHint`、`Room.createNeedTwo`（键名未改，文案改为「选一个发起人」）。
- 增：`Room.createTitleSolo`、`Room.emptyInviteHint`、`Room.emptyInviteAction`。

## 全库 Room 成员数假设（grep 核实）

| 位置 | 结论 |
| --- | --- |
| `collaboration_room_service.rs` `unique_member_ids` / `remove_member` / 对应测试 | **已改** |
| `create-room-dialog.tsx` `members.length < 2` / 按钮 disabled | **已改** 为发起人一人即可 |
| `session-bulk-action-bar.tsx` `sessionCount < 2` + `toastRoomNeedTwo` | **不改**（任务明确不动多选会话建群） |
| `Room.emptyHint`「侧栏至少选两个 Session」 | **不改**：这是 `!detail` 时的房间页空态，说的是 bulk 入口，不是本弹窗 |
| `host_control_room.rs` 「Pass at least one other Session id」+ `member_session_ids.minItems: 1` | **不改**：MCP 网关文案/schema。调用方自己会被补进成员。若数组里只有自己，后端现在会建成 1 人群。改 MCP 文案超出本任务 |
| `src-tauri/experts/skills/codeg-room/SKILL.md`、`codeg-multi-agent`、`SESSION-COMMUNICATION-USAGE.zh-CN.md` | **不改**：文档/技能，不是运行时校验 |
| `MAINTAINABILITY-PROGRAM-2026-08-19.zh-CN.md` | 历史条目，不改 |
| `session_search.rs` 「at least two characters」 | 内容搜索，无关 |
| 其余 `len() < 2` / `>= 2` | 解析器、路径、UI 计数等，与 Room 成员下限无关 |

没有 schema / migration。`created_by_conversation_id` 未动。没有做零成员空群。

## 验证

每条「我验证了」对应下面真实命令。PowerShell 里用 `$env:CARGO_TARGET_DIR = "target"`，cwd 为 `src-tauri`（除非另写）。

### 前端依赖

我验证了 `pnpm install --frozen-lockfile` 退出码 0（worktree 原先没有 `node_modules`）。

### `pnpm vitest run src/components/rooms`

我验证了退出码 0：4 files / 68 tests passed（含新建的 `create-room-dialog.test.tsx` 3 条，以及 rooms-page 头部拉人 / 空群引导 / 多人仍用原 empty 文案）。

另跑过 `src/lib/room-create.test.ts`（7 passed），solo 标题用例在内。

打开 add-member 弹窗时 jsdom 打出既有警告：`<button>` 套 `<button>`（拉人列表里的 Checkbox）。任务要求不动 add-member 弹窗，未修。

### `pnpm eslint src/components/rooms src/lib/room-create.ts src/lib/room-create.test.ts`

我验证了退出码 0（prettier 先修过 `rooms-page.test.tsx` 一行折行）。

### Rust `cargo check --features test-utils`

第一次失败：`resource path '..\out' doesn't exist`（Tauri `frontendDist`）。这是 worktree 没有 Next 导出，不是这次代码。本地建了不入库的 `out/index.html` 占位后再跑。

我验证了第二次 `cargo check --features test-utils` 退出码 0（`Finished dev profile ... in 1m 29s`，以及 `is_empty` 之后增量 check 13.51s 退出码 0）。

编译期有 sidecar 占位警告（`codeg-mcp-x86_64-pc-windows-msvc.exe` 0 字节），与本任务无关。

### `cargo test --features test-utils collaboration_room`

我验证了测试 **编译成功**（`Finished test profile ... in 5m 22s`），随后进程 `codeg_lib-….exe` 以 **0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND** 立刻退出。这是任务书里写的 worktree 桌面测试 exe 载入即死，不是断言失败。

我验证了 `cargo test --features test-utils collaboration_room --no-run` 退出码 0。权威运行要靠主仓门禁。

### `cargo clippy --all-targets --features test-utils -- -D warnings`

第一次失败：`unique.len() < 1` → `clippy::len_zero`。改成 `is_empty()` 后，我验证了 clippy 退出码 0。

### `cargo fmt --check`

我验证了退出码 0。

### 未做的 UI 实机验证

这是 Tauri 桌面应用，本会话没有跑桌面窗口或浏览器点选。行为由 vitest（弹窗单选/预选/建群参数、房内拉人按钮、空群引导）覆盖。未在真实窗口里点过新建群聊。

## 未做（按任务）

- 零成员空群、`created_by` 结构、migration
- add-member 弹窗逻辑
- bulk-action-bar 建群路径
- push
