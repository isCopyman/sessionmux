# 切面 ⑤ 收尾 — 红项结构体 persisted JSON round-trip 钉死

- 日期：2026-08-21
- 工作树：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/serde-pins`（分支 `wt/serde-pins`）
- 范围：**只加测试**。生产代码 0 行改动（`git diff --stat` 12 个 `.rs` 文件合计 `710 insertions, 0 deletions`）。
- 依据：`docs/session-workbench/SERDE-PERSISTENCE-INVENTORY.zh-CN.md` §5 红项 + §6.4 第 1 条。

## 结论

红项结构体的**当前落盘 casing 已用样本测试钉在 CI 里**。手写的「盘点记录形状」JSON 全部能解；`to_value` 断言关键字段名存在、错误 casing 不存在。

**没有 CRITICAL。** 没有出现「自己写的落盘 JSON 解不出」——样本 casing 与生产 serde 一致。

## 新增测试（24 条）

命名后缀：`*_persisted_json_shape_is_pinned`（双方向）/ `*_legacy_sample_still_parses`（仅能反序列化的通道配置）。

| 结构体 | 测试名 | 钉死的 casing | 查重 |
|---|---|---|---|
| `WorkTaskConfig` | `work_task_config_persisted_json_shape_is_pinned` | snake：`prompt_blocks` / `display_text` / `agent_type` / `mode_id` / `config_values` / `label_snapshot` | 新 |
| `WorkTaskFolderSettings` | `work_task_folder_settings_persisted_json_shape_is_pinned` | snake：`default_agent_type`、`delete_worktree_default`、`stage_prompts` 等 | 既有 `legacy_settings_json_decodes_without_stage_prompts` 只覆盖缺字段解码；本条补序列化负向断言 |
| `WorkTaskMergeState` | `work_task_merge_state_persisted_json_shape_is_pinned` | snake：`pre_merge_head` / `delete_worktree` / `auto_message` | 新 |
| `WorkTaskQueuedMerge` | `work_task_queued_merge_persisted_json_shape_is_pinned` | snake：`delete_worktree` / `queued_at` | 新 |
| `WorkTaskPreflight` | `work_task_preflight_persisted_json_shape_is_pinned` | snake：`exit_code` / `output_tail` | 新 |
| `AutomationConfig` + `AutomationAction` | `automation_config_persisted_json_shape_is_pinned` | 字段 snake；枚举值 `launch_session` / `enqueue_task` / `queue_prompt` | 新 |
| `PromptQueueDraft` | `prompt_queue_draft_persisted_json_shape_is_pinned` | **三明治**：外层 `displayText` + 内层 `type`/`mime_type` | 新；同一条测试里钉两套 |
| `PromptInputBlock` | `prompt_input_block_persisted_json_shape_is_pinned` | tag `type` = `text` / `image` / `resource_link`；`mime_type` | 新 |
| `CustomAgentSpec` | `custom_agent_spec_persisted_json_shape_is_pinned` | snake：`node_required` / `uv_required` / `archive`；枚举 `npx`/`uvx`/`binary`、`registry`/`manual` | 既有 `spec_json_is_paste_compatible_with_the_acp_registry` 只证明能解；本条补序列化 + 枚举值 |
| `TelegramConfig` | `telegram_config_legacy_sample_still_parses` | snake：`chat_id` / `topic_mode` | 见下方「Deserialize-only」 |
| `LarkConfig` | `lark_config_legacy_sample_still_parses` | snake：`app_id` / `chat_id` | 同上 |
| `WeixinConfig` | `weixin_config_legacy_sample_still_parses` | snake：`base_url` | 同上 |
| `PetWindowConfig` | `pet_window_config_persisted_json_shape_is_pinned` | camel：`activePetId` / `alwaysOnTop` | 新 |
| `PetManifest` | `pet_manifest_persisted_json_shape_is_pinned` | camel：`displayName` / `spritesheetPath` | 既有 `manifest_round_trips_codex_layout` 无负向断言；本条补 `display_name`/`spritesheet_path` 必须缺席 |
| `BackupManifest` | `backup_manifest_persisted_json_shape_is_pinned` | camel：`formatVersion` / `createdAt` / `appVersion` / `latestMigration` / `includesExternalTranscripts` / `includesSecrets` | 新 |
| `LogSettings` | `log_settings_persisted_json_shape_is_pinned` | snake 字段 + `LogLevel` lowercase（`debug`）；裸 `{"level":"info"}` 仍能解 | 新 |
| `SystemProxySettings` | `system_proxy_settings_persisted_json_shape_is_pinned` | snake：`proxy_url` | 新 |
| `SystemLanguageSettings` | `system_language_settings_persisted_json_shape_is_pinned` | 字段默认；`AppLocale`/`LanguageMode` snake 值 `zh_cn` / `zh_tw` / `manual` / `system` | 新 |
| `SystemTerminalSettings` | `system_terminal_settings_persisted_json_shape_is_pinned` | snake：`default_shell` | 新 |
| `GitSettings` | `git_settings_persisted_json_shape_is_pinned` | snake：`custom_path` | 新 |
| `GitHubAccount` | `github_account_persisted_json_shape_is_pinned` | snake：`server_url` / `avatar_url` / `is_default` / `created_at` | 新 |
| `AppPreferences` | `app_preferences_persisted_json_shape_is_pinned` | snake：`disable_hardware_acceleration` | 新；模块 `#[cfg(feature = "tauri-runtime")]`，见验证 |
| `TranscriptHeader` | `transcript_header_persisted_json_shape_is_pinned` | `v` / `kind` / `session_id` / `started_at_ms` | 新 |
| `TranscriptEntry` | `transcript_entry_persisted_json_shape_is_pinned` | `t` / `k` / `p`；`EntryKind` snake 值 `prompt` / `update` / `turn_end` | 新 |

### 查重后故意不加

- **`AgentType` wire 字符串**：`models/agent.rs` 已有 `builtin_wire_names_are_unchanged`（14 个内建值逐个 `to_string`/`from_str` 钉 `"claude_code"` / `"open_code"` / …）和 `custom_agents_round_trip_through_serde`（`"custom:goose"`）。规格：「缺的枚举值补上即可，别重复」——没有缺的，不加。

## 停手点（非 CRITICAL，不改生产代码）

### 1. `TelegramConfig` / `LarkConfig` / `WeixinConfig` 只有 `Deserialize`

`chat_channel/types.rs:13-28`：三个配置体 `#[derive(Deserialize)]`，**没有 `Serialize`**。落盘写入走的是 `serde_json::Value`/`to_string`，不是这些结构体的 `Serialize`。

规格第 2 向是「构造实例 `to_value`」。在不给生产代码加 `Serialize` 的前提下编不过。替代钉法（仍只改测试）：

1. 当前 snake 样本 `from_str` 成功且字段值正确。
2. camel 样本必须解失败（防止有人加 `rename_all = "camelCase"`）。

若以后要钉序列化形状，需要生产代码 `#[derive(Serialize)]`——本次按铁律没动。

### 2. `AppPreferences` 测试在本机桌面 `cargo test` 未能执行

`preferences.rs` 整模块 `#[cfg(feature = "tauri-runtime")]`。测试已写进该文件，桌面 feature 编译通过，但本机 Windows 测试 exe 以 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` 起不来（仓库已记录的 O50 / Common-Controls v6 问题）。`--no-default-features --lib` 不含该模块，所以 **23/24 条执行过，`app_preferences_persisted_json_shape_is_pinned` 只验证了编译**。

## 验证

工作目录：`src-tauri`。`CARGO_TARGET_DIR=target`。

本机无 Next.js `out/`，桌面 feature 的 `tauri_build` 会报 `resource path '..\out' doesn't exist`。验证时在 worktree 根建了 **gitignore 的** `out/index.html` 占位（不提交）。

### 过滤：`persisted_json_shape_is_pinned`

命令（先试规格里的桌面 feature，exe 起不来；再跑能执行的 lib）：

```
cargo test --features test-utils persisted_json_shape_is_pinned
```

- 补了 `out/` 之后：**编译成功**（`Finished test profile ... in 3m 33s`）
- 随后：`exit code: 0xc0000139, STATUS_ENTRYPOINT_NOT_FOUND`（测试 exe 未进入 harness）

```
cargo test --no-default-features --features test-utils --lib persisted_json_shape_is_pinned
```

```
running 20 tests
test models::system::tests::git_settings_persisted_json_shape_is_pinned ... ok
test acp_transcript::tests::transcript_header_persisted_json_shape_is_pinned ... ok
test models::work_task::tests::work_task_preflight_persisted_json_shape_is_pinned ... ok
test models::pet::tests::pet_manifest_persisted_json_shape_is_pinned ... ok
test models::pet::tests::pet_window_config_persisted_json_shape_is_pinned ... ok
test models::system::tests::system_proxy_settings_persisted_json_shape_is_pinned ... ok
test models::system::tests::system_terminal_settings_persisted_json_shape_is_pinned ... ok
test models::system::tests::github_account_persisted_json_shape_is_pinned ... ok
test models::prompt_queue::tests::prompt_queue_draft_persisted_json_shape_is_pinned ... ok
test models::system::tests::system_language_settings_persisted_json_shape_is_pinned ... ok
test acp_transcript::tests::transcript_entry_persisted_json_shape_is_pinned ... ok
test acp::types::envelope_tests::prompt_input_block_persisted_json_shape_is_pinned ... ok
test models::work_task::tests::work_task_merge_state_persisted_json_shape_is_pinned ... ok
test logging::tests::log_settings_persisted_json_shape_is_pinned ... ok
test models::automation::tests::automation_config_persisted_json_shape_is_pinned ... ok
test models::work_task::tests::work_task_config_persisted_json_shape_is_pinned ... ok
test acp::custom_registry::tests::custom_agent_spec_persisted_json_shape_is_pinned ... ok
test commands::backup::manifest::tests::backup_manifest_persisted_json_shape_is_pinned ... ok
test models::work_task::tests::work_task_folder_settings_persisted_json_shape_is_pinned ... ok
test models::work_task::tests::work_task_queued_merge_persisted_json_shape_is_pinned ... ok

test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 2636 filtered out; finished in 0.00s
```

### 过滤：`legacy_sample_still_parses`

```
cargo test --no-default-features --features test-utils --lib legacy_sample_still_parses
```

```
running 3 tests
test chat_channel::types::tests::lark_config_legacy_sample_still_parses ... ok
test chat_channel::types::tests::weixin_config_legacy_sample_still_parses ... ok
test chat_channel::types::tests::telegram_config_legacy_sample_still_parses ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 2653 filtered out; finished in 0.00s
```

新增可执行钉死测试合计 **23 passed**（另 1 条 AppPreferences 仅桌面编译）。

### 全量 `--lib`（server 面，能真正跑 harness）

```
cargo test --no-default-features --features test-utils --lib
```

```
test result: ok. 2655 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 60.74s
```

（过滤跑时是 2636 filtered + 20 = 2656 个测试名；全量 2655 passed + 1 ignored = 2656，一致。）

规格要求的桌面全量 `cargo test --features test-utils`：**编译过、exe 0xc0000139，未声称通过。**

### fmt / clippy

```
cargo fmt --check
```

exit 0，无输出。

```
cargo clippy --all-targets --features test-utils -- -D warnings
```

exit 0（`Finished dev profile ... in 2m 42s`）。桌面 clippy 只 typecheck，不跑测试 exe，所以不受 0xc0000139 影响。

## 改动文件

只新增 `#[cfg(test)]` 模块或往已有 `mod tests` 里加函数：

- `src-tauri/src/models/work_task.rs`
- `src-tauri/src/models/automation.rs`
- `src-tauri/src/models/prompt_queue.rs`
- `src-tauri/src/models/system.rs`
- `src-tauri/src/models/pet.rs`
- `src-tauri/src/acp/types.rs`（加进已有 `envelope_tests`）
- `src-tauri/src/acp/custom_registry.rs`
- `src-tauri/src/chat_channel/types.rs`
- `src-tauri/src/commands/backup/manifest.rs`
- `src-tauri/src/logging/mod.rs`
- `src-tauri/src/preferences.rs`
- `src-tauri/src/acp_transcript.rs`

未 push。未改 `repos/codeg` 主树。
