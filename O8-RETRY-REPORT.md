# O8-RETRY 施工报告

工作树：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/o8-retry`（分支 `wt/o8-retry`）  
范围：审计文档 `docs/session-workbench/O8-TRANSIENT-RETRY-AUDIT.zh-CN.md` **§5.1 最小 B**（A0 + B-lite）。  
未改 `D:/code/revisiting/work/repo_audit/repos/codeg`。未 push。

拍板落地：

| 项 | 做了？ |
|---|---|
| A0：瞬时 `session/prompt` RPC 失败不再杀连接 | 做 |
| B-lite：`turn_failed_transient` + 红条 Retry | 做 |
| A1：自动重试 | **不做**（agent 可能已持久化用户消息） |
| B-full：新 `ConversationStatus` | **不做** |

---

## 1. 改了什么

### 1.1 `session/prompt` RPC 失败不再经 `?` 杀连接

`src-tauri/src/acp/connection.rs` 的 prompt 臂由 `let response = prompt_result?` 改为 `match`：

- 用 §4.2 保守谓词 `is_transient_prompt_rpc_error`（零内容 + 无未 resolve AIR warning + `ErrorCode` 不是 Auth/Method/ResourceNotFound + 文本命中瞬时模式且不命中不可重试模式）。
- 命中：打非终端 `AcpEvent::Error { code: Some("turn_failed_transient"), terminal: false, details: 原文 }` + `TurnComplete { stop_reason: "transient" }`，`break` 回空闲循环（既有 `StatusChanged { Connected }`）。
- 未命中 / 无法判定：`return Err(e)`，今天的终端杀连接行为不变。
- **没有** A1 有限重试、没有退避循环。

`turn_failure_error_event` 增加 `"transient"` 臂，code `turn_failed_transient`，`terminal: false`。RPC 原文叠进 `details`（调用点），方便告警展开看到 `API Error: 503 …`。

### 1.2 lifecycle：`"transient"` 显式映射到 `Cancelled`

`src-tauri/src/acp/lifecycle.rs` 的 `TurnComplete` 匹配把 `"transient"` 和 `refusal|max_tokens|…|empty` 放在同一臂 → `ConversationStatus::Cancelled`。**不会**掉进 `_ => None`（行卡 `InProgress` = O43 复发）。可 Retry 与用户取消仍靠 `last_error.code` 区分（用户取消走 `"cancelled"`，lifecycle 不改行）。

### 1.3 前端：本地化 + 红条 Retry

- `acp-connections-context.tsx`：`case "turn_failed_transient"` → `backendErrors.turnFailedTransient`；`ERROR` / hydrate 写入 `errorCode`。
- `conversation-shell.tsx`：红条旁在 `onTransientRetry` 存在时画 Retry（复用 AIR 条的 `Folder.chat.sessionFailure.action.retry`）。
- `conversation-detail-panel.tsx`：仅 owner + `errorCode === "turn_failed_transient"` 时把 Retry 接到既有 `handleSessionFailureAction("retry")` → `lastUserPromptText` + `mqEnqueue`。
- 快照 `last_error.code` 经 `snapshot-denormalize.ts` 的 `lastErrorCode` 灌回，刷新后仍能出 Retry。

### 1.4 i18n

`backendErrors.turnFailedTransient` 写入 `src/i18n/messages/` 下 10 个文件（en / zh-CN / zh-TW / ja / ko / es / de / fr / pt / ar）。

### 1.5 明确没做

未改 `ConversationStatus`、无 migration、未动 SQLite CHECK、parser、automation/work_task 四态解释、AIR 协议。未做自动重试。未用 `PendingReview` / `turn_failed_unknown` 顶替。

---

## 2. 验证（每句对应真实命令输出）

工作目录均为本 worktree。Rust 命令在 `src-tauri/`，且 `$env:CARGO_TARGET_DIR = "target"`。

### 2.1 `cargo fmt --check`

我验证了：`cd src-tauri; cargo fmt` 之后再 `cargo fmt --check` 以 exit 0 结束，没有 rustfmt 差集。

### 2.2 `cargo check --features test-utils`

第一次：`resource path '..\out' doesn't exist`（worktree 没有 Next 静态导出目录，tauri-build 失败）。这不是产品代码错误。创建了 gitignored 的空 `out/` 后重跑。

我验证了：第二次 `cargo check --features test-utils` **Finished `dev` profile … in 1m 42s，exit 0**。

### 2.3 `cargo clippy --all-targets --features test-utils -- -D warnings`

我验证了：clipy **Finished `dev` profile … in 1m 56s，exit 0**（`-D warnings` 未报本改动的 lint）。

### 2.4 `cargo test --features test-utils <过滤词>`

过滤词：`transient_prompt_rpc_error`、`turn_failure_error_event`、`turn_complete_maps_normal_and_failure`。

测试 **编译成功**（`Finished test profile in 6m 04s`），随后三个 exe 均在载入时以 **exit code 0xc0000139 / STATUS_ENTRYPOINT_NOT_FOUND** 死掉。这是任务书写明的 worktree 桌面测试 exe 环境问题，不是本改动的断言失败。

我验证了：`cargo test --features test-utils --no-run` **Finished `test` profile … in 4m 14s，exit 0**，产物包括 `codeg_lib` / `codeg` / `codeg_mcp` / `codeg_server` 及 integration test exe。权威测试由领导在主仓门禁跑。

新增/改动的 Rust 测试（编译已纳入 clippy `--all-targets` 与 `--no-run`）：

- `turn_failure_error_event_preserves_existing_reasons` 表加 `("transient", "turn_failed_transient")`
- `turn_failure_error_event_maps_transient_as_non_terminal`
- `transient_prompt_rpc_error_matches_conservative_503`（`InternalError` + `API Error: 503 No available accounts`）
- `transient_prompt_rpc_error_rejects_non_retryable_and_unknown`（裸 InternalError、Auth/Method/ResourceNotFound、unauthorized、AIR warning、已有 agent 输出、dropped update）
- `turn_complete_maps_normal_and_failure_statuses` 表加 `("transient", Cancelled)`

### 2.5 前端

我验证了：

```
pnpm vitest run src/contexts/acp-connections-context.test.tsx src/lib/snapshot-denormalize.test.ts src/components/conversations/conversation-detail-panel-layout.test.ts
```

**Test Files 3 passed / Tests 117 passed**（含新的 `turn_failed_transient` 本地化、`errorCode` 存储、快照 `lastErrorCode`、红条 Retry 源码接线）。

我验证了：对触到的前端文件 `pnpm eslint <paths>` 在修完 layout 测试 prettier 引号后 **exit 0**。

我验证了：`pnpm eslint .` **0 errors, 3 warnings**，告警都在未改文件（`message-input.tsx` unused `sourceConversationId`、`session-bulk-action-bar.tsx` hook deps、`message-list-view.tsx` unused `outbound`），**没有因十语 i18n 新增告警**。

### 2.6 浏览器

本次改动的红条 Retry 依赖活 ACP 连接与 `session/prompt` RPC 失败，本 worktree 没有可驱动的桌面会话。我没有在浏览器里点过 Retry。行为由上面的前端测试 + 源码接线测试覆盖。

---

## 3. 风险（审计文档已写、未扩做）

- 部分 adapter 可能把 `session/prompt` RPC error 当成 session 已死。A0 之后下一次用户 Retry 仍可能失败，那时若谓词不再命中会走今天的杀连接路径。
- 未做 A1：不会自动重发，避免 agent 已入队 user 造成双份 prompt。
- 行状态仍是「已取消」；可恢复性在连接 + `last_error.code`。idle-sweep 掉连接后 Retry 入口消失——这是不加 DB 列的上限。
