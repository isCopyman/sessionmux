# O59-A 后端施工报告 — Claude 启动配置档

日期：2026-08-21  
工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/claude-profile`（分支 `wt/claude-profile`）  
范围：**只做后端**。未改 `src/`、未改 i18n、未改 schema、未 push。

## 1. MCP 注入（任务书第 2 步：先核实，不猜）

现有 `codeg-mcp` **不是**通过 `settings.json` 注入的。

- 注入点：`src-tauri/src/acp/connection.rs` 的 `inject_codeg_mcp`，在 `session/new` 的 `mcpServers` 上把伴生进程挂上 ACP 线缆。
- 与 `CLAUDE_CONFIG_DIR` 无关：换档不会让 `codeg-mcp` 消失。
- 用户 MCP 清单：Claude 走 `~/.claude.json` 的 `mcpServers` + `~/.claude/settings.json` 的 `enabledPlugins`（`commands/mcp.rs`），codeg 再经 `load_mcp_servers_for_agent` 转发给 ACP。这是另一条路径，本任务不改。

因此 **managed 档的 `settings.json` 不写 `mcpServers` / 不写 codeg-mcp**。写进去会和线缆注入双挂。测试 `managed_materialize_writes_only_non_empty_env_keys_and_is_idempotent` 断言物化文件里没有 `mcpServers`。

## 2. 档的磁盘布局

数据目录用调用方传入的 `data_dir`（桌面：`paths::resolve_effective_data_dir`；服务器：`AppState.data_dir`），不自己拼 home。

```
<data_dir>/claude-profiles/<id>.json     # 档元数据（含 managed 的明文 token）
<data_dir>/claude-profiles/<id>/         # 仅 kind=managed：Claude 配置目录
    settings.json                        # CLAUDE_CONFIG_DIR 指向这个目录
```

`<id>.json` 与 `<id>/` 文件名不同，不冲突。

虚拟档 `follow-default` 不落文件。用户不得创建同名档。

Unix 写盘 0600（新文件 `OpenOptions.mode(0o600)`；已存在且 world-readable 则收紧）。Windows 无 POSIX 0600，普通 `fs::write`，ACLs 跟随父目录（与 `write_hermes_secret_file` 相同策略）。

### 物化 `settings.json`（kind=managed）

REPLACE 语义，幂等。只写有值的 env 键：

```json
{ "env": { "ANTHROPIC_BASE_URL": "...", "ANTHROPIC_AUTH_TOKEN": "...", "ANTHROPIC_MODEL": "..." } }
```

upsert 时立刻物化；spawn 解析时若是 managed 再物化一次（幂等）。不写空键，不写 mcp。

## 3. 解析顺序（单一事实源）

`commands::claude_profile::resolve_claude_profile(db, data_dir, conversation_id, claude_agent_env_json)`：

1. 会话绑定：`preferred_config_values["__codeg_profile__"]`（常量 `PREFERRED_PROFILE_CONFIG_KEY`，与 `__codeg_host_*__` 放在 `acp/connection.rs`）
2. 否则 agent 默认：`agent_setting.env_json["CODEG_CLAUDE_PROFILE"]`（不加 DB 列）
3. 否则 `follow-default`

绑定的档文件缺失：warn 后回落到 `follow-default`（不阻断 spawn）。

`null` 解绑 = 删掉该键（走 agent 默认）。显式写成 `"follow-default"` = 会话强制跟随默认，即使 agent 默认是别的档。

## 4. 注入

`build_session_runtime_env` 增加 `conversation_id: Option<i32>`。仅 `AgentType::ClaudeCode` 调用 `apply_claude_profile_env`，且在 env 组装**最后**：

- `follow-default`：**不设** `CLAUDE_CONFIG_DIR`
- `configDir`：设为用户给的绝对路径
- `managed`：设为物化目录 `<data_dir>/claude-profiles/<id>/`
- 若 `env_json` **已经**有 `CLAUDE_CONFIG_DIR`：保留用户值并 `tracing::warn!`

`CLAUDE_CONFIG_DIR` 从 `fingerprint_config` 排除（与 `OPENCLAW_RESET_SESSION` 同类），避免 agent 级 staleness 刷新把所有非默认档会话误标过期。换档走 `conversation_set_claude_profile` → `ConnectionManager::mark_conversation_config_stale` → 已有 `SessionConfigStale` 事件。

## 5. 跳过 ACP 下发循环

`is_codeg_internal_pin(key)`：`key` 以 `__codeg_` 开头且 **不是** Host semantic pin（`__codeg_host_model__` / `__codeg_host_thought_level__`）。

`ordered_preferred_entries` 过滤掉内部键；循环开头再 skip 一层。`__codeg_profile__` 不会 `set_config`、不会 `tracing::error!`。Host pin 仍解析后下发。

## 6. 停止污染 `~/.claude/settings.json`

`cascade_update_agent_config` 的 **Claude 分支**默认不写用户原生文件。Gemini / Codex / 其它分支未动。不自动清理已写入的旧内容。

回退开关：Claude 的 `agent_setting.env_json` 键 `CODEG_CASCADE_CLAUDE_SETTINGS`，值为 `1` / `true` / `yes` / `on`（大小写不敏感）时恢复旧写入。默认关。

## 7. 给前端的 API 契约

新的纯线 DTO 一律 **camelCase**。Token **只回掩码**，线契约里没有明文 `authToken` 字段（upsert 入参除外，那是只写）。

桌面：Tauri `invoke` 命令名如下。  
服务器：`POST /<命令名>`，JSON body 与参数同形。

### 7.1 `claude_profile_list`

- 参数：无（桌面从 AppHandle 解析 data_dir；服务器用 `AppState.data_dir`）
- 返回：`ClaudeProfileInfo[]`  
  第一项永远是虚拟档 `follow-default`。

### 7.2 `claude_profile_upsert`

- 参数（单对象 `payload` / JSON body）：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | string | `[a-z0-9-_]{1,64}`，不能是 `follow-default` |
| `label` | string | 非空，≤128 |
| `kind` | `"followDefault"` \| `"configDir"` \| `"managed"` | 不能 upsert `followDefault` |
| `configDir` | string? | `kind=configDir` 时必填，必须是绝对路径 |
| `baseUrl` | string? | 仅 `managed` |
| `authToken` | string? | 仅 `managed`；**省略=保留已存值**；`""`=清空 |
| `model` | string? | 仅 `managed` |

- 返回：`ClaudeProfileInfo`（见下）

### 7.3 `claude_profile_delete`

- 参数：`{ "id": string }`
- 返回：`()` / `null`
- 删除 `<id>.json`；若存在 managed 目录一并删。不能删 `follow-default`。不碰用户自己的 `configDir` 路径。

### 7.4 `conversation_set_claude_profile`

- 参数：`{ "conversationId": number, "profileId": string \| null }`
- `profileId = null`：解绑，下次解析走 agent 默认
- `profileId = "follow-default"`：会话显式跟随默认
- 其它 id：必须已存在于档列表
- 返回：

```json
{
  "conversationId": 12,
  "profileId": "api",
  "affectedRunningSessions": 0
}
```

`affectedRunningSessions` 为 1 表示该会话有活的 ACP 连接且已被标配置过期（前端弹已有 `SessionConfigStaleBanner`）。回合进行中禁止切档由前端用现有 turn 闸处理；后端只标记 stale，不杀进程。

### 7.5 `ClaudeProfileInfo`（list / upsert 返回）

```json
{
  "id": "api",
  "label": "中转",
  "kind": "managed",
  "configDir": null,
  "baseUrl": "https://example.test/v1",
  "authTokenMasked": "sk-t••••••••7890",
  "model": "claude-sonnet-4",
  "createdAt": "2026-08-21T00:00:00+00:00",
  "updatedAt": "2026-08-21T00:00:00+00:00"
}
```

- 没有 `authToken` 字段。
- `follow-default`：`kind=followDefault`，`authTokenMasked=""`，`createdAt`/`updatedAt` 为 `1970-01-01T00:00:00Z`。
- `configDir` 仅 `kind=configDir` 出现。

### 7.6 Agent 默认档（无新命令）

写 Claude 的已有 `acp_update_agent_env` / env 面板即可：`env_json.CODEG_CLAUDE_PROFILE = "<id>"`。空/缺 = 跟随 `follow-default`。

## 8. 测试（真实输出）

过滤词在 `src-tauri` 下跑：

```
CARGO_TARGET_DIR=target cargo test --no-default-features --features test-utils --lib <filter>
```

| filter | `test result:` |
| --- | --- |
| `claude_profile` | `ok. 10 passed; 0 failed; 0 ignored; 0 measured; 2669 filtered out` |
| `codeg_internal_pins` | `ok. 1 passed` |
| `claude_cascade` | `ok. 1 passed`（Windows；Unix 另有 HOME 写盘测试 `#[cfg(unix)]`） |
| `claude_native_cascade` | `ok. 1 passed` |
| `mark_conversation_config_stale` | `ok. 1 passed` |
| `fingerprint_config_is_deterministic` | `ok. 1 passed` |

合计 **15 passed / 0 failed**。覆盖：解析三档、`follow-default` 不产出 `CLAUDE_CONFIG_DIR`、configDir/managed 路径、非 Claude 不受影响、内部键不下发、managed 物化键与幂等、id 校验、cascade 默认不写、token 掩码。

## 9. 验证命令（L1：不靠管道判定退出码）

工作树无 `out/` 时桌面 `cargo check --features test-utils` 会因 `tauri.conf.json` 的 `frontendDist: "../out"` 失败。本机建了 gitignored 的占位 `out/index.html` 后：

| 步骤 | 退出码（命令自己的 `$LASTEXITCODE`） |
| --- | --- |
| `cargo fmt --check` | `FMT-EXIT:0` |
| `cargo check --features test-utils` | `CHECK-EXIT:0` |
| `cargo clippy --all-targets --features test-utils -- -D warnings` | `CLIPPY-EXIT:0` |
| `cargo test --no-default-features --features test-utils --lib <filters>` | 上表全部 0 |
| `cargo check --no-default-features --bin codeg-server` | `SERVER-EXIT:0` |

未在 worktree 跑桌面 `cargo test --features test-utils`（L5：exe 可能 0xc0000139）。权威测试由主仓门禁跑。

## 10. 未做（按任务书）

- schema / 迁移 / 新 DB 列
- 前端与 i18n
- Codex / Gemini 的 cascade 与注入
- 连通性探活、apiKeyHelper、transcript 档标记
- 自动扫描 `~/.claude` 生成档
- 不清理已经写进用户 `~/.claude/settings.json` 的旧 `ANTHROPIC_*`
