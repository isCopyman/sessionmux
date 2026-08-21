# O66-A 施工报告 — 接入配置档真正拥有连接配置（后端）

日期：2026-08-21
工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/profile-conn`（分支 `wt/profile-conn`）
范围：**只做 Rust 后端**（`src-tauri/`）。未改 `src/`、未改 i18n、未改 schema / 迁移 / DB 列、未 push、未动主工作树。

## 1. 问题（已核实）

`agent_setting.env_json` 里的 `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_BASE_URL` / 模型别名 / `CLAUDE_AUTH_MODE=custom` 对**所有** Claude 会话无差别注入。SDK 凭据顺序（`Du()`）是 env token 高于配置目录 OAuth。用户建「走订阅」的档时，全局 token 仍然生效 → **以为在用订阅，其实在烧 API**。

本任务按 O39 §2 Monet channel 语义补齐：档自带 connection env；非 `follow-default` 时对 agent 全局连接键做防御清扫；新增虚拟档 `official-direct`。**不写用户的 `~/.claude/settings.json`。**

## 2. 做了什么

### 2.1 档自带 `env`

`ClaudeProfileRecord.env: BTreeMap<String,String>`，`#[serde(default)]`，旧文件缺字段能反序列化（测试 `old_profile_json_without_env_deserializes`）。

**优先级（冲突时专用字段为准，`env` 先写入再被覆盖）：**

1. `record.env` 里的非空键
2. 专用字段 `baseUrl` → `ANTHROPIC_BASE_URL`
3. 专用字段 `authToken` → `ANTHROPIC_AUTH_TOKEN`
4. 专用字段 `model` → `ANTHROPIC_MODEL`

同一规则用于：

- `kind=managed` 物化 `settings.json` 的 `env` 块（仍是 REPLACE、只写非空、不写 mcp）
- spawn 注入（managed 与 configDir 都会把档的 env 打进 `runtime_env`，这样进程 env 盖得住继承值；否则只写 `settings.json` 仍会被进程里残留的 `ANTHROPIC_AUTH_TOKEN` 压过，正是这次要修的失败模式）

`kind=configDir`：`env` **只在 spawn 注入**，不改用户目录里的任何文件。

list 返回的 `env`：键名（大小写不敏感）含 `TOKEN` / `KEY` / `SECRET` 的值走既有 `mask_api_key`。`authToken` 仍然只回掩码。

upsert `env`：**整表替换**。省略 = 不动；`{}` = 清空。

### 2.2 虚拟档 `official-direct`

- id `official-direct`，kind `officialDirect`，不落文件，与 `follow-default` 并列。
- `claude_profile_list` 顺序：`follow-default` → `official-direct` → 用户档。
- 语义（Monet）：`ANTHROPIC_BASE_URL=https://api.anthropic.com`；`ANTHROPIC_AUTH_TOKEN=""`（空串，spawn 层 `env_remove`，同 `apply_claude_env_policy`）。顺带清空 `ANTHROPIC_API_KEY`（同一套 CLI 凭据键，policy 本来就会清；只清 token 仍可能被 API key 认证）。
- 不设 `CLAUDE_CONFIG_DIR`。
- 用户不得创建同名档；kind `officialDirect` 不可 persist。不可 upsert / delete。`conversation_set_claude_profile` 可以绑这个 id（不必有文件）。

### 2.3 防御清扫（单一事实源）

`PROFILE_OWNED_ENV_KEYS` / `PROFILE_OWNED_ENV_PREFIXES` + `is_profile_owned_env_key` / `strip_profile_owned_agent_env`。

精确键：`ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_API_KEY`、`ANTHROPIC_BASE_URL`、`ANTHROPIC_MODEL`、`CLAUDE_AUTH_MODE`、`CLAUDE_CODE_USE_BEDROCK` / `_VERTEX` / `_FOUNDRY`。

前缀：`ANTHROPIC_CUSTOM_MODEL_OPTION*`；`ANTHROPIC_DEFAULT_*_MODEL`（`ANTHROPIC_DEFAULT_` 前缀 **且** `_MODEL` 后缀，避免误伤无关键）。

**未列入**（规格没写，不发明）：`ANTHROPIC_REASONING_MODEL`。

行为：

- 解析出的档 **不是** `follow-default` 时，在 `apply_claude_profile_env`（`build_session_runtime_env` 组装最后一步）先 `retain` 掉上述键，再叠加档自己的 env / official-direct 哨兵 / `CLAUDE_CONFIG_DIR`。
- `follow-default` **完全不清扫**（今天的行为）。
- **只对 `AgentType::ClaudeCode` 生效。** 不删 SQLite 里的 `env_json`，只是注入时跳过。

非连接类键（`CLAUDE_CODE_GIT_BASH_PATH`、`DISABLE_TELEMETRY`、`ENABLE_TOOL_SEARCH`、`CLAUDE_CODE_SCROLL_SPEED`、以及测试用的 `CODEG_O66_MISC`）照常注入。

### 2.4 指纹

`is_volatile_fingerprint_key` 对 Claude 的 profile-owned 键与 `CLAUDE_CONFIG_DIR` 同等排除。否则 agent 级 `refresh_config_staleness`（`conversation_id=None`）会把每个非 follow-default 会话在任意设置保存时标 stale。换档仍走既有 `conversation_set_claude_profile` → `SessionConfigStale`。

副作用（与 O59 排除 `CLAUDE_CONFIG_DIR` 同类）：只改 `env_json` 里的 ANTHROPIC_* 连接键、不改其它键时，follow-default 会话不会仅因指纹变化而被标 stale。换档 / 改非连接类 env 仍会。

## 3. 给前端的 API 契约增量

桌面：`invoke` 命令名不变。服务器：`POST /<命令名>`。纯线 DTO **camelCase**。Token 只回掩码。

相对 O59-A §7，**只增加**下面这些。未列出的字段/命令行为不变。

### 3.1 `ClaudeProfileKind`

```
"followDefault" | "officialDirect" | "configDir" | "managed"
```

新增 `"officialDirect"`。前端 TS 联合类型需要加上（本任务未改 `src/`）。

### 3.2 `ClaudeProfileInfo`（list / upsert 返回）

新增：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `env` | `Record<string, string>` | 掩码后的 map。虚拟档为 `{}`。键名含 `TOKEN`/`KEY`/`SECRET`（不区分大小写）的值已掩码 |
| `isVirtual` | `boolean` | `follow-default` 与 `official-direct` 为 `true`，用户档为 `false` |

虚拟档第二条：

```json
{
  "id": "official-direct",
  "label": "Official direct",
  "kind": "officialDirect",
  "authTokenMasked": "",
  "env": {},
  "isVirtual": true,
  "createdAt": "1970-01-01T00:00:00Z",
  "updatedAt": "1970-01-01T00:00:00Z"
}
```

`claude_profile_list` 顺序：`follow-default`，然后 `official-direct`，然后磁盘上的用户档。

### 3.3 `ClaudeProfileUpsert`

新增可选 `env?: Record<string, string>`。

- **省略**：不改已存 `env`
- **`{}`**：清空
- **有键的对象**：整表替换（不是 merge）

`id` 不得为 `follow-default` 或 `official-direct`。`kind` 不得为 `followDefault` 或 `officialDirect`。

`authToken` 语义不变：省略=保留，`""`=清空。

### 3.4 `claude_profile_delete` / `conversation_set_claude_profile`

- 不能删 `follow-default` / `official-direct`。
- `conversation_set_claude_profile` 可以把会话绑到 `official-direct`（与绑 `follow-default` 一样，不要求磁盘文件）。`null` 解绑语义不变。

### 3.5 Agent 默认档

仍写 `env_json.CODEG_CLAUDE_PROFILE`。值可以是 `official-direct`。空/缺 = `follow-default`。

### 3.6 前端要注意的行为

- 选非 `follow-default` 档时，agent 设置里的 `ANTHROPIC_*` / `CLAUDE_AUTH_MODE` / Bedrock/Vertex/Foundry 开关 **不会**进这次 spawn。连接以档为准。
- 选 `official-direct` = 强制官方端点 + 清 token，走配置目录 OAuth。这才是「订阅」档。`follow-default` 仍是「跟随 CLI / 跟随全局 env_json」。
- list 的 `env` 里看不到明文密钥。upsert 仍可写入明文（只写）。

## 4. 测试（真实输出）

工作树 `src-tauri/`，`CARGO_TARGET_DIR=target`，L5：`--no-default-features --features test-utils --lib`。

| filter | `test result:` |
| --- | --- |
| `claude_profile` | `ok. 26 passed; 0 failed; 0 ignored; 0 measured; 2682 filtered out` |
| `fingerprint_config_is_deterministic` `list_profiles` `codeg_internal_pins` `claude_cascade` `claude_native_cascade` `mark_conversation_config_stale` `work_task_profile` `unknown_automation_profile` | `ok. 13 passed; 0 failed; 0 ignored; 0 measured; 2695 filtered out` |

合计 **39 passed / 0 failed**（两批有重叠命名空间但命令分开跑；`claude_profile` 26 条是本任务主集）。覆盖：

- follow-default 注入与今天一致（owned 键仍在，含 ANTHROPIC_*）
- 非 follow-default：owned 键不出现，非连接类键仍在
- official-direct：`ANTHROPIC_BASE_URL=https://api.anthropic.com`，token / API key 空串
- 档自带 env 生效；与 baseUrl/token/model 冲突时专用字段赢
- 旧档 JSON 无 `env` 能反序列化
- list 对疑似密钥键掩码；虚拟档 `isVirtual=true`；不可 upsert/delete
- Codex 完全不受影响

## 5. 验证命令（L1：不靠管道判定退出码）

无 `out/` 时桌面 clippy 会因 `frontendDist` 失败。建了 gitignored 占位 `out/index.html`。

| 步骤 | 退出码 |
| --- | --- |
| `cargo fmt --check` | `FMT-EXIT:0` |
| `cargo clippy --all-targets --features test-utils -- -D warnings` | `CLIPPY-EXIT:0` |
| `cargo test --no-default-features --features test-utils --lib <filters>` | 上表全部 0 |
| `cargo check --no-default-features --bin codeg-server` | `SERVER-EXIT:0` |
| `cargo check --no-default-features --bin codeg-mcp` | `MCP-EXIT:0` |

未在 worktree 跑桌面 `cargo test --features test-utils`（L5：exe 可能 0xc0000139）。权威测试由主仓门禁跑。

## 6. 未做（按任务书）

- schema / 迁移 / 新 DB 列
- `src/` 与 i18n
- `cascade_update_agent_config`（O59-A 已关 Claude 分支）
- Codex / Gemini / 其它 agent
- 删除用户 `agent_setting.env_json` 里的键
- 不清理已经写进 `~/.claude/settings.json` 的旧 `ANTHROPIC_*`
