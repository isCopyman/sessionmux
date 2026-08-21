# O64 施工报告 — 让 agent 能读任务看板（codeg-mcp 只读工具）

日期：2026-08-21  
工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/task-read`（分支 `wt/task-read`）  
范围：**只做 Rust 后端**（`src-tauri/`）。未改 `src/`、未改 i18n、未改 schema / 迁移 / DB 列、未 push、未动主工作树。

## 1. 先核实，不猜

- 任务强制挂项目根 folder：`work_task_service.rs` `create` 拒绝 `parent_id.is_some()`。
- worktree cwd hop 已在 `commands/chat_authoring.rs::resolve_folder`（conversation → 最深 path match → 一次 parent hop）。`list_tasks` 复用它；`folder_path="all"` 在 hop 之前短路，避免把 `"all"` 当路径去匹配。
- 前端列映射事实源：`src/components/tasks/board-columns.ts` `STATUSES_BY_COLUMN`（任务书里的 `src/lib/...board-columns` 实际路径是这个文件）。
- 注册/分发照抄 O59-C `list_profiles`：`tool_schema.json` → `CompanionFeatures::allows_tool` → companion 解析 → `BrokerMessage` 往返 → listener → `ChatAuthoringAccess`。
- 查询只加 `work_task_service` 只读函数；MCP 层不写 SQL。

## 2. 做了什么

### 2.1 `list_tasks`（只读）

参数全可选：

| 参数 | 缺省 | 行为 |
| --- | --- | --- |
| `folder_path` | 调用者项目（走既有 hop） | `"all"` = 跨项目，按 `updated_at` 倒序 |
| `status` | 不过滤 | 列名 `todo` / `in_progress` / `attention` / `done`，或原始状态 |
| `limit` | 30 | 硬上限 100，超了夹住不报错 |

返回紧凑行：`id` / `title` / `status` / `agent_type` / `updated_at` / `has_worktree` / `conversation_id` / `failure_reason`。外层 `total` + `truncated`。

**不返回** prompt 正文、`display_text`、时间线、`last_error`、`config`。`agent_type` 取任务自己的 override（没有就省略），没有把 command 层的 inherit 盖戳拷进 MCP。

列名 → 状态集合是前端表的规格副本，注释钉在 `src/components/tasks/board-columns.ts`。`todo` / `done` 既是列名也是原始状态：**列名优先**（`status=todo` 含 `queued`）。原始状态里不与列名撞车的（`queued`、`awaiting_input` 等）仍单选。

### 2.2 `get_task`（只读）

`task_id` 必填。返回单卡：`title` / `status` / `agent_type` / `model`（`config_values["model"]`）/ `profile`（`config_values["__codeg_profile__"]`，只回 id）/ `prompt_excerpt`（`display_text` 截到 1000 字符，复用 `truncate_chars`）/ `base_branch` / `work_branch` / `has_worktree` / `conversation_id` / 最近 10 条事件（`kind` + `at` + `summary` 截 200）/ `last_error`（截 500）。找不到是 soft miss（`found: false` + note），不是工具错误。

### 2.3 注册与开关

- 开关：`taskboard`（与 `create_work_task` 同组）。能建任务的 companion 才能读。`automations`  alone 看不到这两个工具。
- 英文描述写明 **read-only**，以及默认只看调用者自己的项目。
- 只读：两个工具不写库、不改状态。非法 `status` / 解析不出项目是 soft note。

## 3. 明确不做

- 写工具（改状态、认领、改别人的卡）
- schema / 迁移 / DB 列
- `src/`、i18n、前端
- 跨项目排序发明（`all` = `updated_at` desc）
- 分页游标
- 把 command 层 `annotate_agent_type` 的 inherit 盖戳搬进 MCP（规格只写返回 `agent_type`，任务 override 没有就省略）

## 4. 取舍

1. **`todo`/`done` 当列名。** 与前端四列一致；要单选 `queued` 请传原始状态。测试钉了 `attention` 集合，也钉了「每个 `WorkTaskStatus` 恰好落一列」。
2. **host 再夹一次 `limit`。** companion 解析已经 `clamp(1, 100)`；`ChatAuthoringAccess` 再夹一次，避免旧 companion 把 150 打进来。
3. **事件 summary** 不是把前端 `timelineDetail` 整份拷过来。`status_changed` 用 `from → to`，其余优先 `message` / `summary` / `error`，再退回 payload JSON，然后 200 字符截断。
4. **call-time 不关 feature flag。** 与 `list_profiles` 相同：injection 时 `allows_tool` 已经挡住；只读工具不在 host 再读一遍 `work_tasks_enabled`。

## 5. 测试（真实输出）

工作树 `src-tauri/`，L1：每步自己的退出码；L5：`--no-default-features --features test-utils --lib`。

| 过滤器 | 结果 |
| --- | --- |
| `list_tasks` | `ok. 6 passed; 0 failed; 0 ignored; 0 measured; 2703 filtered out` `EXIT=0` |
| `get_task` | `ok. 2 passed; 0 failed; … 2707 filtered out` `EXIT=0` |
| `board_status_filter` | `ok. 1 passed; … 2708 filtered out` `EXIT=0` |
| `list_matching` | `ok. 1 passed; … 2708 filtered out` `EXIT=0` |
| `recent_events_are_newest` | `ok. 1 passed; … 2708 filtered out` `EXIT=0` |
| `tools_list` | `ok. 3 passed; … 2706 filtered out` `EXIT=0` |
| `render_task` | `ok. 1 passed; … 2708 filtered out` `EXIT=0` |
| `parse_list_tasks` | `ok. 1 passed; … 2708 filtered out` `EXIT=0` |
| `parse_get_task` | `ok. 1 passed; … 2708 filtered out` `EXIT=0` |
| `list_profiles_is_gated` | `ok. 1 passed; … 2708 filtered out` `EXIT=0` |
| `list_tasks_schema` | `ok. 1 passed; … 2708 filtered out` `EXIT=0` |

本任务新增 / 回归钉住的行为：

| 测试 | 钉住的行为 |
| --- | --- |
| `list_tasks_defaults_to_the_caller_project_and_hops_off_a_worktree` | 缺省 = 调用者项目；worktree cwd hop 到项目根；行里无 prompt / display_text / last_error / config |
| `list_tasks_folder_path_all_returns_every_project_newest_first` | `folder_path="all"` 跨项目，`updated_at` 倒序 |
| `list_tasks_attention_filter_matches_frontend_column` | `attention` = `awaiting_input`/`review`/`merging`/`failed`（`board-columns.ts`）；原始 `awaiting_input` 单选 |
| `board_status_filter_matches_frontend_column_table` | 四列集合 + 每个状态恰好一列 |
| `list_tasks_clamps_limit_and_sets_truncated` | `limit=2` 时 `total=5 truncated`；`150` 夹到 100；101 条返回 100 + truncated |
| `list_matching_filters_and_caps_without_writing` | service 只读；列过滤；limit 截页不截 total |
| `get_task_truncates_and_omits_profile_secrets` | excerpt 1000 / last_error 500 / summary 200 / 事件 10 条；无 token / authToken / baseUrl / prompt_blocks |
| `parse_list_tasks_clamps_limit_to_hard_cap` | companion 150 → 100，缺省 30 |
| `list_tasks_schema_says_read_only_and_defaults_to_caller_project` | 工具描述含 Read-only + 默认本项目 |
| `tools_list_contains_only_shared_host_bridge_tools` | 清单含 `list_tasks` / `get_task` |
| `list_profiles_is_gated_by_either_authoring_feature` | 读工具只跟 `taskboard`，不跟 `automations` |

没有 0 passed 的过滤器被算作跑过。

## 6. 验证命令（L1：不靠管道判定退出码）

在 `src-tauri/` 下，`CARGO_TARGET_DIR=target`。Windows 没有 `/tmp`，日志写 worktree `verify-logs/`（gitignored / 未提交）。每步 `*> log; echo STAGE-EXIT:$LASTEXITCODE`。

| 步骤 | 退出码 | 日志要点 |
| --- | --- | --- |
| `cargo fmt --check` | `FMT-EXIT:0` | — |
| `cargo clippy --all-targets --features test-utils -- -D warnings` | `CLIPPY-EXIT:0` | `Finished dev profile ... in 2m 34s`。`out/` 占位已按 O59-A/C 建 gitignored `out/index.html` |
| `cargo test --no-default-features --features test-utils --lib <filter>` | 上表全部 `EXIT=0` | — |
| `cargo check --no-default-features --bin codeg-mcp` | `MCP-EXIT:0` | `Finished ... in 1m 01s` |
| `cargo check --no-default-features --bin codeg-server` | `SERVER-EXIT:0` | `Finished ... in 1.06s` |

未在 worktree 跑桌面 `cargo test --features test-utils`（L5：exe 可能 0xc0000139）。权威门禁由领导在主仓跑。

## 7. 改动文件

- `src-tauri/src/acp/chat_authoring.rs` — `ListTasksQuery` / 紧凑 row / detail outcome、trait 只读方法、截断常量
- `src-tauri/src/acp/delegation/{companion.rs,listener.rs,transport.rs,tool_schema.json}`
- `src-tauri/src/commands/chat_authoring.rs` — hop 复用、列过滤、截断、不回 secret
- `src-tauri/src/db/service/work_task_service.rs` — `board_status_filter`、`list_matching`、`recent_events`（只读）
- `src-tauri/tests/host_bridge_e2e.rs` — stub `list_tasks` / `get_task`
- `TASK-READ-REPORT.md` — 本报告
