# O69-A（改版）施工报告 — 一个接入配置档就是一份 codeg 自己拥有的 settings.json

日期：2026-08-21  
lane：`profile-own`  
提交范围：Rust 后端 `src-tauri/` + 本报告。未改 `src/`、未改 i18n、未改 schema / migration / DB 列、未 push、未动 main 或主工作树。

## 1. 结果

Claude 接入现在只有两种产品语义：

- `follow-default`：不设 `CLAUDE_CONFIG_DIR`，不写文件，codeg 零档注入；Claude CLI 读取自己的默认与项目配置。
- codeg 档：`managed` 档拥有一份 `<data_dir>/claude-profiles/<id>/settings.json`，会话通过 `CLAUDE_CONFIG_DIR` 选中它；`configDir` 仍只指向用户自己维护的目录，codeg 不写那个目录。

`ClaudeProfileRecord` 新增向后兼容的 `settingsJson?: string`。旧档 JSON 没有该字段仍可反序列化，测试 `old_profile_json_without_env_or_settings_json_deserializes` 已覆盖。

O66-A 已存在的结构化 `env` 字段保留线兼容：它仍是档内的显式运行时覆盖，并在物化时叠到 `settingsJson.env` 上。它不是本次要删除的 agent 全局第三层。本次一次性迁移不会向这个字段写连接键；迁移的非专用连接键全部进入原始 `settingsJson.env`。

## 2. managed settings.json 的物化与 spawn

物化顺序：

1. 解析 `settingsJson` JSON 对象，作为整份文件基底；没有基底时使用 `{}`。
2. 叠加既有 O66 `env` 兼容 map 中的非空值。
3. 三个专用字段最后覆盖同名键：
   - `baseUrl` → `env.ANTHROPIC_BASE_URL`
   - `authToken` → `env.ANTHROPIC_AUTH_TOKEN`
   - `model` → `env.ANTHROPIC_MODEL`

必须逐字钉死给前端的规则：**空/`None` 的专用字段不写这三个键，也不删基底里同名的键——不然用户在 JSON 里手写的会被抹掉。**

物化仍为 REPLACE + 幂等：目标文件从档记录重建，不和上次物化文件做 read-modify-write；基底中的其它顶层键、`env` 中其它键都会保留。`codeg-mcp` 仍通过 ACP wire 注入，不写进 settings.json。

spawn 行为：

- 三个非空专用字段继续作为进程环境覆盖注入。
- 既有结构化 `env` 兼容 map 继续按 O66-A 注入。
- **`settingsJson` 里的其它键不注入进程环境；Claude CLI 自己读取该文件。**
- 任何非 `follow-default` 档都会给 `ANTHROPIC_BASE_URL`、`ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_API_KEY` 放入空哨兵，再由档的专用字段 / 兼容 `env` 覆盖。vendored spawn 把空哨兵翻译成 `env_remove`，所以它们不是传给子进程的空变量；作用是阻止 codeg 父进程或容器继承值压过选中的 settings.json。
- `follow-default` 完全不变：不清扫、不加空哨兵、不设 `CLAUDE_CONFIG_DIR`。

## 3. Secret 规则

疑似 secret 的判定只看 `env` 块的直接键名：大小写不敏感地包含 `TOKEN`、`KEY` 或 `SECRET`。

掩码统一调用既有 `mask_api_key`：

- 字符数 `<= 8`：返回等长的 `•`。
- 字符数 `> 8`：保留前 4 个 Unicode 字符与后 4 个字符，中间为 `•`；掩码总长度最多 20 字符，所以很长的 secret 也只返回 20 字符。
- 空字符串仍为空字符串。

适用面：

- `authToken` 明文永不出现在 `ClaudeProfileInfo`；只返回 `authTokenMasked`。
- `ClaudeProfileInfo.env` 的疑似 secret 值掩码。
- `ClaudeProfileInfo.settingsJson` 中 `env` 直属块的疑似 secret 值掩码。
- `claude_settings_read` 对合法 JSON 对象执行相同掩码。

masked round-trip：更新已有档时，如果收到的 `settingsJson.env.<key>` 值与当前存储值经 `mask_api_key` 得到的掩码完全相等，就恢复当前存储明文，等价于“该项没改”。如果没有同键存储值可恢复（新档或新增键），而值具有 `mask_api_key` 的掩码形状，该键会被删除，绝不会把 `•••` 写成真实 token。测试分别覆盖了 list 掩码、已有档保留与新档丢弃。

## 4. `claude_settings_read`

新增桌面 Tauri 命令和服务器 handler / route；只读，不创建、不修改、不删除任何被读文件。

默认路径复用 `parsers::claude::resolve_claude_config_dir()` 这一既有事实源，再拼 `settings.json`；没有硬编码 home。它与仓库的 Claude parser / agent root 解析一致，并尊重进程级 `CLAUDE_CONFIG_DIR`。

- `path = null` / 省略：读 CLI 默认 settings.json。
- `path = string`：读指定文件。
- 不存在：成功返回 `exists=false`、`text=""`。
- 合法顶层对象：掩码 `env` 直属 secret；有掩码时返回 pretty JSON。
- 非法 JSON或合法但顶层不是对象：原文逐字返回，便于前端显示损坏位置；由于无法可靠定位 `env`，此时不做掩码，`droppedSecretKeys=[]`。

## 5. 一次性迁移

迁移挂在桌面和 `codeg-server` 共用的 `db::init_database` 启动收口点：schema migration 完成后、任何会话可 spawn 前执行。没有新表、新列或迁移文件。

触发：Claude Code 的现有 `agent_setting.env_json` 含 `is_profile_owned_env_key` 判定的任意连接键，且 `CODEG_CLAUDE_PROFILE_MIGRATED != "1"`。

动作：

1. 先写并物化 managed 档；文件成功后才更新 DB 行，避免文件写失败时删除用户配置。
2. id 从 `imported` 开始；记录文件或 managed 目录占用时依次使用 `imported-2`、`imported-3`……。
3. label 取 `ANTHROPIC_BASE_URL` 可解析 URL 的 host，失败则 `Imported`。
4. `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_MODEL` 分别进入三个专用字段。
5. 其余所有 profile-owned 键原样进入 `settingsJson.env`；复用 O66-A 的 `PROFILE_OWNED_ENV_KEYS` / `PROFILE_OWNED_ENV_PREFIXES` / `is_profile_owned_env_key`，没有复制第二份清单。`ANTHROPIC_DEFAULT_FABLE_MODEL` 被 `ANTHROPIC_DEFAULT_` + `_MODEL` 规则覆盖。
6. `CLAUDE_AUTH_MODE` 不写进档。
7. DB `env_json.CODEG_CLAUDE_PROFILE=<新 id>`，并删除全部 profile-owned 连接键。
8. 加 `CODEG_CLAUDE_PROFILE_MIGRATED=1`。
9. 非连接类键逐键保留；只对 `AgentType::ClaudeCode` 执行。

为什么不带 `CLAUDE_AUTH_MODE` 不改变行为：它是 codeg 的启动策略选择键，不是 settings.json 内容。迁移后“已选非 follow-default 档”本身成为认证边界；三个凭据/端点空哨兵保持 `official_subscription` 原本清除宿主继承凭据的效果，managed 档的专用值再覆盖哨兵。`custom` / `model_provider` 原本不触发该清除策略，其逐键连接值现由专用字段与 settings.json 恢复。测试 `migrated_official_auth_mode_is_expressed_by_profile_credential_scrub` 钉死前一种行为，真实形状迁移测试钉死后一种逐键等价。

幂等/no-op：

- 标记为 `1` 后再次运行不再建档。
- `env_json={}`：不建档、不打标记、不写 profiles 目录。
- 只有非连接键：不建档、不打标记、DB map 字节语义不变。

全程不读写迁移用户的 `~/.claude/settings.json`；新读命令也只读。

## 6. `official-direct` 退休

- `claude_profile_list` 不再返回 `official-direct`。
- `ClaudeProfileKind::OfficialDirect`、`OFFICIAL_DIRECT_PROFILE_ID`、解析与 spawn 语义保留。
- `conversation_set_claude_profile` 仍接受 `official-direct`，已有绑定会继续解析为官方 URL + 清 token/API key，不会炸。
- 因为它不再可发现，要求“已知 profile”的新 work-task / automation 不再把它列为可选 id；已有会话绑定不受影响。

## 7. 完整前端 API 契约

所有 DTO 为 camelCase。桌面使用 Tauri `invoke`；服务器使用 `POST /<command>`。

### 7.1 `ClaudeProfileKind`

```ts
type ClaudeProfileKind =
  | "followDefault"
  | "officialDirect" // 只为旧绑定/显式 conversation bind 保留
  | "configDir"
  | "managed"
```

### 7.2 `ClaudeProfileInfo` 全字段

| 字段 | 线类型 | 返回规则 |
| --- | --- | --- |
| `id` | `string` | profile id |
| `label` | `string` | 展示名 |
| `kind` | `ClaudeProfileKind` | list 不再产生 `officialDirect`，但类型保留 |
| `configDir` | `string`，可省略 | 仅 `configDir` 有值 |
| `baseUrl` | `string`，可省略 | managed 专用字段 |
| `authTokenMasked` | `string` | 总是存在；无 token 为 `""`；有值使用上述 `mask_api_key` 形状 |
| `model` | `string`，可省略 | managed 专用字段 |
| `settingsJson` | `string`，可省略 | managed 原始对象文本的掩码版本；`env` secret 不回明文 |
| `env` | `Record<string,string>` | 总是存在；既有 O66 兼容 map，secret-looking 值已掩码；它不等于 `settingsJson.env` 的投影 |
| `isVirtual` | `boolean` | `follow-default=true`；磁盘档=false |
| `createdAt` | RFC 3339 `string` | `follow-default` 固定 `1970-01-01T00:00:00Z` |
| `updatedAt` | RFC 3339 `string` | 同上；磁盘档为更新时间 |

线 DTO 没有明文 `authToken` 字段。`settingsJson` 和 `env` 的 secret 判定、掩码形状见第 3 节。

### 7.3 `claude_profile_list`

- 桌面：`invoke("claude_profile_list")`
- 服务器：`POST /claude_profile_list`，body `{}`
- 返回：`ClaudeProfileInfo[]`

顺序严格为：

1. `follow-default`
2. `<data_dir>/claude-profiles/*.json` 按文件路径/文件名升序，即 id 的字典序磁盘档

没有 `official-direct`。迁移后若只有新迁移档，就是 `[follow-default, imported]`；若已有用户档，`imported[-N]` 按 id 与它们一起排序。

### 7.4 `ClaudeProfileUpsert` 全字段与语义

| 字段 | 入参类型 | 语义 |
| --- | --- | --- |
| `id` | `string`，必填 | `[a-z0-9-_]{1,64}`；不能是两个虚拟 id |
| `label` | `string`，必填 | trim 后非空，最多 128 字符 |
| `kind` | `"configDir" \| "managed"` | `followDefault` / `officialDirect` 会报错 |
| `configDir` | `string \| null`，可省略 | `configDir` 必须提供绝对路径；其它 kind 忽略为无值 |
| `baseUrl` | `string \| null`，可省略 | managed：trim 后空/省略/null 会清空专用字段 |
| `authToken` | `string \| null`，可省略 | write-only；省略/null=保留；`""`/全空白=清空；非空=替换 |
| `model` | `string \| null`，可省略 | managed：trim 后空/省略/null 会清空专用字段 |
| `settingsJson` | `string \| null`，可省略 | 见下方四态契约 |
| `env` | `Record<string,string>`，可省略 | O66 兼容：省略=保留，`{}`=清空，有键 map=整表替换；不是 merge |

`settingsJson` 四态：

- 省略：更新时保留已有值；新档为无基底。
- `null`：与省略相同，保留已有值（serde `Option` 契约）。
- `""` 或全空白字符串：清空基底，managed 物化从 `{}` 开始。
- 合法 JSON 对象字符串：替换基底；后端会规范化为 pretty JSON。非法 JSON 或顶层非对象直接返回可展示的 `InvalidInput` 错误，例如 `settingsJson must be valid JSON: ...` / `settingsJson must have a JSON object at the top level`。

`kind=configDir` 传非 null 的 `settingsJson`（包括 `""`）直接报错：`settingsJson is only valid for kind=managed; configDir profiles use the user's own directory`。`null` 按上面的省略语义处理，不构成一份 settingsJson。

masked save-back：已有档同键值若完全等于当前存储值的掩码，就保留明文。新档/无同键存储值时，符合掩码形状的 secret 键会丢弃。

- 桌面：`invoke("claude_profile_upsert", { payload })`
- 服务器：`POST /claude_profile_upsert`，body 就是 `ClaudeProfileUpsert`
- 返回：`ClaudeProfileInfo`

### 7.5 `claude_settings_read`

入参：

```ts
interface ClaudeSettingsReadParams {
  path?: string | null
}
```

返回：

```ts
interface ClaudeSettingsReadResult {
  path: string
  text: string
  exists: boolean
  droppedSecretKeys: string[]
}
```

`droppedSecretKeys` 是 `env` 直属、非空、疑似 secret 且已在 `text` 中掩码的**裸键名**数组，例如 `["ANTHROPIC_API_KEY", "MY_TOKEN"]`；按键名字典序。它在文件不存在、非法 JSON、顶层非对象或没有可掩码 secret 时为空。前端从该 preview 新建档时应提示“这些密钥没有带过来，请重新填写”；新档 upsert 会丢弃这些 mask-shaped 值。

- 桌面：`invoke("claude_settings_read", { path })`
- 服务器：`POST /claude_settings_read`，body `{ "path": string | null }`

### 7.6 其余 profile API

`claude_profile_delete`：

- 入参 `{ id: string }`，返回 `null`。
- 只删 codeg 的 `<id>.json` 与 `<id>/` managed 目录；不碰用户 `configDir`。
- 两个虚拟 id 都不能删。

`conversation_set_claude_profile`：

- 入参 `{ conversationId: number, profileId: string | null }`。
- 返回 `{ conversationId, profileId?, affectedRunningSessions }`。
- `null` 解绑、回到 agent 默认解析；显式 `follow-default` 强制该会话跟随 CLI。
- 仍接受 `official-direct`；普通磁盘 id 必须存在。

### 7.7 迁移后前端可观测变化

- Claude agent `env_json` 中原连接键消失，非连接键原样保留。
- `CODEG_CLAUDE_PROFILE` 变为 `imported`（占用时为 `imported-N`）。
- 增加 `CODEG_CLAUDE_PROFILE_MIGRATED="1"`。
- `claude_profile_list` 出现该 managed 档，label 通常是旧 base URL 的 host。
- 该档 `settingsJson` 的 secret 已掩码；专用 token 只通过 `authTokenMasked` 展示。
- `official-direct` 从 list 消失，但旧会话仍能使用。

## 8. 已发现但按任务书未改：model_provider 会把连接键写回 env_json

代码现实中有两条现存回写路径，本批停手、不扩大修改范围：

1. `commands/acp.rs::cascade_update_model_provider`：provider 编辑后把 URL/key 与 `parse_provider_model` 结果 merge 回 dependent agent 的 `env_json`，再 `agent_setting_service::update`。
2. `commands/acp.rs::acp_update_agent_env` 的 provider bind 分支：`parse_provider_model` merge 到 `merged_env` 后序列化写回 `env_json`。

因此一次性迁移完成后，用户后续编辑/绑定 Claude model provider 仍可能重新出现 profile-owned 连接键。非 `follow-default` spawn 的档清扫仍会防止这些键压过档，但 DB “长期不再承载连接键”的彻底收口需要下一批统一改这两条写路径。本提交没有自行修改它们。

## 9. 验证（真实最终输出）

工作目录：本 worktree 的 `src-tauri/`；`CARGO_TARGET_DIR=target`。无 `out/` 时创建了 gitignored `out/index.html` 占位，仅用于桌面 build.rs，不在提交中。

| 门禁 | 结果 |
| --- | --- |
| `cargo fmt --check` | `FMT-EXIT:0` |
| `cargo clippy --all-targets --features test-utils -- -D warnings` | `CLIPPY-EXIT:0` |
| `cargo test --no-default-features --features test-utils --lib claude_profile` | `TEST-EXIT:0`；`36 passed; 0 failed; 2694 filtered out` |
| `cargo check --no-default-features --bin codeg-server` | `SERVER-EXIT:0` |
| `cargo check --no-default-features --bin codeg-mcp` | `MCP-EXIT:0` |
| `git diff --check` | `DIFF-CHECK-EXIT:0` |

测试承重覆盖：旧档兼容；四态 upsert；对象校验；基底保留与专用字段胜出；list/read 掩码；masked save-back；新档丢 secret；read 缺失/损坏；真实用户键形状迁移逐键等价（含 FABLE）；非连接键不动；迁移幂等/no-op；official auth-mode 凭据清除；`official-direct` 不在 list 但旧绑定仍解析；非 Claude agent 不受影响。
