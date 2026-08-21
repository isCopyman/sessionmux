# 催办遇锁死队列空转 + overdue_unread 口径

工作树：`D:/code/revisiting/work/repo_audit/repos/codeg/.cli-delegate/worktrees/collab`  
分支：`cli-delegate-collab`  
未改 `D:/code/revisiting/work/repo_audit/repos/codeg`。未 push。未动 `main`。未改 DB schema / migration / 表列。

## 1. 做了什么

### 甲、会话级队列已暂停时催办不再入队

`enqueue_mailbox_attention` 在既有 `has_pending_mailbox_attention`（只认 `queued`/`claimed`）之后，**另加一道独立判断**：读 `conversation_prompt_queue_state.paused_reason`，非空则：

- 不插入队列项
- 返回 `Ok(false)`（与现有短路同语义，`sweep_once` 因此不调用 `record_successful_reminder`）
- `tracing::info!` 打会话 id 和暂停原因（不是 warn）

新增只读 helper `prompt_queue_service::queue_paused_reason`。未改 `pause_queue` / `resume_queue` / `user_send_clears_pause` / 入队后的暂停继承。

### 乙、`overdue_unread` 排除 `failed`

`list_overdue_reminder_targets` 的 `overdue_unread` SUM 补上 `AND d.state <> 'failed'`，与同查询的 `overdue_reply`、以及 `reset_idle_reminder_cursors` 对齐。

代码注释：投递失败的信 Agent 根本读不到，重投是队列的职责，不是催办的职责。

## 2. 明确没做

- 未改 `prompt_queue_service` 的暂停/解除语义，未动 `user_send_clears_pause` 白名单，未让暂停自动恢复。
- 未改 `invocation_policy = 'invoke_when_idle'` 前提。
- 未改前端 / i18n。
- 未改 `mark_origin_embedded` / `agent_received_at`。
- 未加新表、新列。

## 3. 测试

| # | 断言 | 位置 |
|---|---|---|
| 1 | `paused_reason` 非空 → `enqueue_mailbox_attention` 不插入，sweep 不记成功催办 | `collaboration_reminder_runtime.rs` |
| 2 | `paused_reason` 为空 → 仍入队（回归） | 同上 |
| 3 | `state='failed'` 逾期未读不计入 `overdue_unread` | `collaboration_service.rs` |
| 4 | `state='queued'` 逾期未读仍计入（回归） | 同上 |

测试 1 先删掉首投留下的 collaboration origin（否则会撞上旧的 `queued`/`claimed` 短路，测不到新判断），再 `pause_queue`，再直接入队 + `sweep_once`。

## 4. 提问（未自行决定）

`newest_due_at` 的未读分支仍未排除 `failed`（任务书只点了 SUM）。failed-only 会话现在会被 HAVING 滤掉，这条不影响甲乙的主路径。若会话同时有 failed 未读 + 有效欠账，`newest_due_at` 仍可能被 failed 那封的到期时刻拉动。要不要对齐，等拍板。

## 5. 验证

工作目录：本 worktree 的 `src-tauri/`。`$env:CARGO_TARGET_DIR = "target-gate"`。每条单独跑，判定只认命令自己的输出文本和 `*-EXIT:` 标记。

本段在长命令跑完后回填。提交时验证尚未跑，避免超时丢工作。
