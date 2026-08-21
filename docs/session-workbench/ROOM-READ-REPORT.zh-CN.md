# O48 施工报告：群帖逐步披露读取接口

工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/room-read`（分支 `wt/room-read`）。未改主仓 `codeg`。未 push。

## 做了什么

群消息不做硬截断。`read_room` 各模式单帖内联上限 2000 个 Unicode scalar value（Rust `char`，char 边界切分）；超限帖给 head + `body_truncated` / `body_total_chars`，并在该事件 body 尾部给出 `read_room_post` 续读指引。新 MCP 工具 `read_room_post` 按 offset/max_chars 取窗，不推进已读游标。投递侧 8000 阈值未改，截断 note 改为指向 `read_room_post`。

## 实现对照规格

1. **`read_room` 内联 2000 字符**
   - ≤2000：原 `String` 原样返回（逐字节不变）。
   - 超限：`chars().take(2000)` 前缀 + 换行 + `Body truncated (N of M chars). Call read_room_post with event_id=<id> offset=N for the rest.`
   - 新字段：`SessionRoomEvent.body_truncated`（false 时 `skip_serializing_if`）、`body_total_chars`（`Option`，未截断省略）。
   - `consume_room_window` 调用处注释写明：内联截断不改变游标语义，截断帖仍算已消费。

2. **`read_room_post`**
   - 入参：`event_id` 必填；`offset` 默认 0；`max_chars` 默认 8000，宿主与 companion 均 clamp 到 1..=40000。
   - 出参：`SessionRoomPostReadOutcome`（from/title/created_at 等与 `SessionRoomEvent` 同族字段 + body 窗口 + `body_total_chars` + 未读完时的续读 note，含下一个 offset）。
   - 鉴权：listener 用 token 解析 caller session，再 `get_room_event` + `require_member`，与 `read_room` 同一成员校验；非成员 / 不存在 / 协作关闭均 `available: false` + note。
   - **不**调用 `consume_room_window`（注释写明：单帖细读，不是窗口消费）。

3. **MCP 注册**
   - 架构与任务书一致：`tool_schema.json` → companion `allows_tool` / `tools/call` → `BrokerMessage::ReadRoomPost` → listener → `SessionCollaborationAccess::read_room_post`。未停手。
   - `read_room` 工具描述已改为「超长帖给 head + `read_room_post` 续读」。
   - 同族工具列表（`list_rooms` / `read_room` / `read_room_post` / `post_room`）同步改了 companion feature 门、tools/list 断言、e2e stub、技能与 tools manual。

4. **投递侧**
   - 找到现成 note：`collaboration_service.rs` 房间信封在 `MAX_FIRST_DELIVERY_BODY_CHARS`（8000）截断时提示去 `read_room` 取余。
   - 已改为：`Call read_room_post with event_id=… offset=8000 for the rest. Call read_room with room_id=… for surrounding posts.`
   - 8000 阈值本身未改。落库 1MB 上限未改。

5. **明确不做（遵守）**
   - 未改 body 落库上限、未改投递 8000 阈值、未动 `read_room` 三种模式 / 默认 50 / 上限 200 / 游标机制、未做正文搜索或摘要。

## 验证（真实命令）

工作目录均为 `src-tauri`，`CARGO_TARGET_DIR=target`。首次桌面 `cargo check --features test-utils` 因缺少 `../out` 被 `tauri_build` 拒绝（`resource path ../out doesn't exist`）。本地建了空目录 `out/`（已在 `.gitignore`，不提交）后再跑。

| 命令 | 结果 |
| --- | --- |
| `cargo fmt` 后 `cargo fmt --check` | exit 0 |
| `cargo check --features test-utils` | exit 0（建空 `out/` 之后；有 sidecar placeholder warning） |
| `cargo check --no-default-features --bin codeg-mcp` | exit 0 |
| `cargo clippy --all-targets --features test-utils -- -D warnings` | exit 0 |
| `cargo test --features test-utils --lib room_post_bodies_are_progressively_disclosed` | 编译成功；运行时 **0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND**（任务书已知怪癖） |
| `cargo test --features test-utils --lib --no-run` | exit 0 |
| `cargo test --no-default-features --features test-utils --lib <过滤词>` | 下列测试均 ok |

我验证了桌面 feature 下 lib 测试 exe 载入即死 0xc0000139，与任务书描述一致。权威测试以主仓门禁为准。我改跑 `--no-run`（编译通过）以及 `--no-default-features --features test-utils --lib`（实际执行通过）：

- `commands::collaboration::tests::room_post_bodies_are_progressively_disclosed` — ok  
  覆盖：CJK 2000 逐字节不变、emoji 边界截断字段与续读指引、`read_room_post` 不推进游标、截断帖仍被 `unread=true` 消费、offset 尾窗 / 中窗 / 越过末尾、`max_chars=50000` 被硬上限 40000 截住、非成员拒绝、event_id 不存在。
- `room_body_window_is_char_boundary_safe_and_identity_under_inline_limit` — ok
- `room_body_window_offset_tail_and_empty_past_end` — ok
- `session_room_event_omits_truncation_fields_when_full` — ok
- `first_delivery_truncates_oversized_room_mention_and_points_at_read_room_post` — ok
- `tools_list_contains_only_shared_host_bridge_tools` — ok
- `tools_list_keeps_mailbox_and_room_on_separate_servers` — ok
- `collaboration_calls_validate_before_spawning` — ok（缺 `event_id` 返回 -32602）

过滤词：`room_post_bodies_are_progressively_disclosed`、`room_body_window`、`session_room_event_omits`、`first_delivery_truncates_oversized_room`、`tools_list_contains_only_shared`、`tools_list_keeps_mailbox`、`collaboration_calls_validate_before_spawning`。

未在浏览器验证（本任务无 UI 改动）。未跑桌面测试 exe 的实际断言（0xc0000139）。
