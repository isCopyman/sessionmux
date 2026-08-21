# O69-C 施工报告 — 档改用 `--settings` 叠加生效，不再换配置目录

日期：2026-08-21
lane：`settings-overlay`
提交范围：Rust 后端 `src-tauri/` + 本报告。未改 `src/`、未改 i18n、未改 schema / migration / DB 列、未 push、未动 main 或主工作树。O69-A 的 `settingsJson` 内容模型、掩码、`claude_settings_read`、一次性迁移均未改。

## 1. 结果

Claude `managed` 档不再把 `CLAUDE_CONFIG_DIR` 指到档目录。物化路径仍是 O69-A 的 `<data_dir>/claude-profiles/<id>/settings.json`，生效方式改为 ACP `session/new`（以及同链的 `session/load` / `session/resume`）上的：

```
_meta.claudeCode.options.extraArgs.settings = "<settings.json 绝对路径>"
```

适配器把 `extraArgs` 渲成 argv，等价于 `claude --settings <绝对路径>`。家目录仍是默认 `~/.claude`（或进程里已有的 `CLAUDE_CONFIG_DIR`），所以 OAuth 凭据 / projects / todos / skills 还在。

| 档 | `CLAUDE_CONFIG_DIR` | `--settings` overlay | 连接 env 再注入 |
| --- | --- | --- | --- |
| `follow-default` | 不设 | 不加 `_meta.extraArgs` | 不扫、不加空哨兵（与今天逐键一致） |
| `managed` | 不设 | 物化文件绝对路径，逐字、无引号 | 三个连接键空哨兵 / `env_remove`；**不再**把 baseUrl / token / model / `record.env` 写进进程 env |
| `configDir` | 仍设 | 不设 `extraArgs.settings` | 保持现状（`record.env` 仍注入） |
| `official-direct` | 不设 | 不加 overlay | 保持现状（官方 URL + 空 token/API key） |

`codeg-mcp` 仍走 `session/new.mcpServers`（`inject_codeg_mcp`）。它和 `extraArgs.settings` 在同一份 `session/new` 请求上共存：前者是 ACP 顶层字段，后者在 `_meta.claudeCode`。测试钉死两样都在，且 `emitRawSDKMessages` 不被覆盖。

## 2. `--settings` 是否压过 user settings（停手条件）

**压过。没有停手。** 同键时 command-line / flag settings 高于 user settings。证据三条：

1. `claude --help`（本机 Claude Code 2.1.236）：`--settings <file-or-json>  Path to a settings JSON file or a JSON string to load additional settings from`。措辞是 **additional**，不是换家。
2. 官方文档 [Settings — How scopes interact](https://code.claude.com/docs/en/settings)：同键优先级为 **Managed > Command line arguments > Local > Project > User（最低）**。`--settings` 属于 command line arguments。
3. `@anthropic-ai/claude-agent-sdk` `sdk.d.ts`（本机 npx 缓存 `claude-agent-acp` 依赖）：`settings`「loaded into the **"flag settings" layer, which has the highest priority among user-controlled settings**. Equivalent to the `--settings` CLI flag。」

适配器 `@zed-industries/claude-agent-acp` `dist/acp-agent.js` 把 `params._meta.claudeCode.options` 展开进 SDK，并保留 `extraArgs`：

```js
settingSources: ["user", "project", "local"],
...userProvidedOptions,
extraArgs: { ...userProvidedOptions?.extraArgs, "replay-user-messages": "" },
```

所以 `_meta.claudeCode.options.extraArgs.settings` 会变成 `--settings <path>`，同时 `settingSources` 仍加载 user/project/local。家还是 `CLAUDE_CONFIG_DIR ?? ~/.claude`。

### 2.1 `env` 块 / `pxq()` 风险

Claude CLI 启动时会把已合并 settings 的 `env` 写进进程环境。flag layer 对 user-controlled 同键最高，因此档 `settings.json` 里**写了的** `env.ANTHROPIC_*` 会压过 `~/.claude/settings.json` 同键。

**没写的键不会被清掉。** `--settings` 是叠加，不是整份替换。用户 `~/.claude/settings.json` 若已有 `env.ANTHROPIC_AUTH_TOKEN`，一个没写 token 的 managed 档**不会**把它抹掉。进程级空哨兵只挡住 codeg 父进程 / 容器继承，挡不住 user settings 文件里的 `env` 块。

这正是「档不再是一张白纸」的一部分，必须写进前端文案。若产品要「纯订阅档不受用户 settings 里 API token 影响」，需要另批决定是否在物化文件里写空哨兵键——本任务规格禁止自作主张回退 `CLAUDE_CONFIG_DIR`，也禁止发明未写的清键语义。

非 `follow-default` 仍 `env_remove` `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY`，并保留 `PROFILE_OWNED_ENV_KEYS` 对 `agent_setting.env_json` 的防御清扫。

## 3. 给前端的行为后果（文案要照着写）

档是叠在用户 CLI 配置上面的一层，不是隔离的第二份家。

- 用户自己的 `~/.claude/settings.json`（hooks / statusline / permissions / 未在档里覆盖的 `env` 键）**仍然生效**。切档不会丢掉这些。
- 档里写了的键（含 `effortLevel` / `permissions` / `hooks` / `env`）按 §2 压过 user 同键。
- 项目级 `.claude/settings.json` 和 local 仍然加载（适配器 `settingSources` 写死三层）。
- **不写 token 的档**：家目录 OAuth 还在，所以可以是订阅档；但如果用户 settings 的 `env` 里已经有 API token，那个 token 仍会生效（见 §2.1）。
- `configDir` 语义不变：换一个完整的家。UI 已不再提供新建入口。
- `follow-default`：零注入、零 overlay，跟随 CLI。

## 4. 实现要点

- `apply_claude_profile_env`：`managed` 把物化 `settings.json` 的绝对路径放进内部键 `CODEG_CLAUDE_SETTINGS_OVERLAY`；**不**设 `CLAUDE_CONFIG_DIR`；**不**再注入 baseUrl / token / model / `record.env`。
- `spawn_agent_connection` 在 `build_agent` 和指纹之前 `take_claude_settings_overlay` 删掉该键，所以它不是子进程环境变量。指纹的 volatile 集合也排除它，防止 agent 级 refresh 把所有 managed 会话标 stale。
- `claude_raw_sdk_session_meta` 把 overlay 写进已有的 `_meta.claudeCode`（与 `emitRawSDKMessages` 共存）。路径 `to_string_lossy()` 逐字放入 JSON 字符串，不加引号。
- `configDir` / `official-direct` / `follow-default` / 非 ClaudeCode 的路径按上表，未发明新语义。

## 5. 明确没做

- schema / migration / DB 列
- `src/`、i18n
- O69-A `settingsJson` 内容模型、掩码、`claude_settings_read`、一次性迁移
- 恢复 `official-direct` 进 list
- `model_provider` 两条回写路径
- Codex / Gemini / 其它 agent
- 未把 `mcpServers` 搬进 `_meta`（代码现实里它就是 ACP 顶层字段；任务书那句「`inject_codeg_mcp` 往 `_meta` 写 mcpServers」与实现不符，按现实共存，未发明第二份 MCP 注入）

## 6. 验证（真实最终输出）

工作目录：本 worktree 的 `src-tauri/`；`CARGO_TARGET_DIR=target`。无 `out/` 时创建了 gitignored `out/index.html` 占位，仅用于桌面 build.rs，不在提交中。L1：每步单独重定向再打 `*-EXIT`，不用管道判定退出码。L5：`--no-default-features --features test-utils --lib`。

| 门禁 | 结果 |
| --- | --- |
| `cargo fmt --check` | `FMT-EXIT:0` |
| `cargo clippy --all-targets --features test-utils -- -D warnings` | `CLIPPY-EXIT:0` |
| `cargo test --no-default-features --features test-utils --lib claude_profile` | `TEST-EXIT:0`；`38 passed; 0 failed; 2697 filtered out` |
| `cargo test … --lib overlay` | `TEST-EXIT:0`；`4 passed`（路径逐字 / mcp 共存 / 空格+非 ASCII / overlay 键被 remove） |
| `cargo test … --lib extra_args` | `TEST-EXIT:0`；`1 passed`（无 overlay 不加 extraArgs；Codex 不受影响） |
| `cargo test … --lib fingerprint_config_is_deterministic` | `TEST-EXIT:0`；`1 passed` |
| `cargo test … --lib build_new_session_request_sets_claude_raw_meta` | `TEST-EXIT:0`；`1 passed` |
| `cargo check --no-default-features --bin codeg-server` | `SERVER-EXIT:0` |
| `cargo check --no-default-features --bin codeg-mcp` | `MCP-EXIT:0` |
| `git diff --check` | `DIFF-CHECK-EXIT:0` |

测试承重覆盖：managed → overlay 绝对路径且无 `CLAUDE_CONFIG_DIR`；路径含空格/非 ASCII 逐字、无引号；档 + codeg-mcp 同请求共存；configDir 仍设 `CLAUDE_CONFIG_DIR`、不设 overlay；follow-default 连接键与杂项键逐键保留；非 follow-default 三个连接键仍被 `env_remove` 且 managed 不再回填；非 Claude agent 不受影响；迁移后有效连接配置在 settings.json `env` 里，进程 env 只留空哨兵。
