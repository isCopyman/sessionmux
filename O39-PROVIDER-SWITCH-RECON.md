# O39 调研：paseo / monet 如何做「订阅 vs API」provider 配置切换

日期：2026-08-21
性质：只读设计输入。本文件是唯一写入物。产品代码未改。
场景：同一个 agent CLI（如 Claude Code）既可用订阅登录跑、也可用 API key / 中转跑；用户要在 codeg 里**按会话**选哪一份配置。

---

## 1. Paseo

参考仓：`D:/code/revisiting/work/repo_audit/repos/paseo`

### 1.1 配置模型

Paseo 把「配置档」做成**独立 provider 条目**，不是「同一个 Claude 上的认证模式开关」。

- 存哪：daemon 配置文件 `$PASEO_HOME/config.json`（默认 `~/.paseo/config.json`）。明文 JSON，未见 keychain / DB。见 `docs/data-model.md:187-228`、`docs/architecture.md:384-398`。
- 结构：`agents.providers` 是 `Record<providerId, ProviderOverride>`。schema 在 `packages/protocol/src/provider-config.ts:46-58`：

```ts
export const ProviderOverrideSchema = z.object({
  extends: z.string().optional(),
  label: z.string().optional(),
  description: z.string().optional(),
  command: z.array(z.string().min(1)).min(1).optional(),
  env: z.record(z.string(), z.string()).optional(),
  params: z.record(z.string(), z.unknown()).optional(),
  models: z.array(ProviderProfileModelSchema).optional(),
  additionalModels: z.array(ProviderProfileModelSchema).optional(),
  disallowedTools: z.array(z.string()).optional(),
  enabled: z.boolean().optional(),
  order: z.number().optional(),
});
```

- ID 规则：`/^[a-z][a-z0-9-]*$/`（同文件 `61`）。自定义条目必须声明 `extends`（内建之一或 `"acp"`）和 `label`（`69-93`）。
- 内建 ID：`claude | codex | copilot | opencode | pi | omp`（`60`）。
- 「订阅 vs API」的做法：给同一套 CLI 开多份别名。文档原话是 *「Multiple profiles for the same provider」*（`docs/custom-providers.md:258-291`）。例：

  - `claude` 内建：不写 `env` → 走 Claude Code 自己的登录态（OAuth 订阅 / `~/.claude`）。
  - `claude-work` / `claude-personal`：`extends: "claude"` + 各自的 `ANTHROPIC_API_KEY`。
  - `zai` / `qwen`：`extends: "claude"` + `ANTHROPIC_AUTH_TOKEN` + `ANTHROPIC_BASE_URL` 指向中转（`docs/custom-providers.md:81-176`）。
  - Codex 同理：`OPENAI_BASE_URL` + `OPENAI_API_KEY`；Paseo 还会把它们映射进 Codex 的 `model_providers` TOML（`docs/custom-providers.md:202-247`）。

- 另一层「档」：`daemon.agentProfiles`（`packages/protocol/src/messages.ts:146-164`）是命名启动包：`provider + model + modeId + thinkingOptionId + featureValues`。**不含凭据**。凭据在 provider 条目的 `env` 里。glossary 明确这两种 profile 不是一回事（`docs/custom-providers.md:262`）。

- 凭据形态：API key / token 直接写在 `config.json` 的 `env` 字段。调研范围内**未见** OS keychain。`providers.openai.apiKey` 只服务 Paseo 自己的语音 STT/TTS，不服务 Codex（`docs/data-model.md:211-323`）。

### 1.2 切换粒度

| 粒度 | 有没有 | 证据 |
|---|---|---|
| 全局默认 | 有：内建 `claude` / `codex` 本身就是默认条目；上次选择记在 create-agent preferences | `packages/app/src/agent-profiles/internal/use-agent-profile-picker.ts:108-120` |
| 每项目 | 无独立「项目凭据」。catalog 按 cwd 缓存，但 `env` 仍是 daemon 全局 | `docs/providers.md:107-118` |
| 每会话 | **有，但只能在创建时选**。agent 记录钉死 `provider` | `packages/server/src/server/agent/agent-storage.ts:45-48` |
| 会话中途切 provider | **不能** | 见下 |

活 agent 不能换进程：

> *「a live agent is one provider's process and cannot switch」*
> `packages/app/src/agent-profiles/internal/use-agent-profile-picker.ts:47-51`

对已运行 agent 应用 profile 时，payload **故意不含 provider**：

> *「Provider is absent because a running agent cannot change the process it is.」*
> `packages/app/src/agent-profiles/internal/materialize-profile.ts:45-58`

能热改的只有 model / mode / thinking / features（`applyAgentConfig`，`packages/client/src/daemon-client.ts:3191-3227`）。UI 有「重新加载 Agent」用于 skills / MCP / 登录状态，不是换 provider（`packages/app/src/i18n/resources/zh-CN.ts:595-596`）。

### 1.3 注入机制

启动时把选中 provider 的 `env` overlay 进子进程，**不改用户的 `~/.claude/settings.json`**。

1. `ProviderOverride.env` → `ProviderRuntimeSettings.env`（`packages/server/src/server/agent/provider-registry.ts:255-269`）。
2. `extends` 派生 provider 时 merge env：override 覆盖 base（同文件 `272-294`、`838-841`）。
3. spawn 走 `createProviderEnv` / `createProviderEnvSpec`（`packages/server/src/server/agent/provider-launch-config.ts:213-248`）：`runtimeSettings.env` 盖过 `process.env`。Claude 适配器在 `packages/server/src/server/agent/providers/claude/agent.ts:1575-1590`。
4. 额外清掉父会话泄漏变量：`CLAUDECODE` / `CLAUDE_CODE_ENTRYPOINT` 等（`provider-launch-config.ts:203-238`），避免「在 Claude Code 里再开 Claude Code」。
5. Codex 特例：除 env 外，还写入 Codex `model_provider` / `model_providers` 配置（`docs/custom-providers.md:229-247`）。
6. 第三方 Anthropic 兼容端点建议配 `disallowedTools: ["WebSearch"]`（`docs/custom-providers.md:717-737`）。

没有包装器二进制；`command` 字段可以整段替换启动 argv（`docs/custom-providers.md:316-364`）。

### 1.4 UI 形态

- **新建 agent 时选 provider**：composer / new-workspace 的 provider 下拉。自定义 `extends: claude` 的条目以独立 provider 出现，label 自定（`docs/custom-providers.md:291`：「You can select which one to use when launching an agent」）。数据来自 `use-agent-form-state.ts` + `provider-selection.ts:37-41`。
- **设置页**：Agent Profiles 编辑器选 provider/model/mode（`packages/app/src/agent-profiles/settings/agent-profile-edit-modal.tsx:252-306`）。自定义 provider 的 `env` **调研范围内未见图形编辑器**——用户手改 `config.json`（`docs/custom-providers.md:1-16`）。
- **已建会话**：agent 记录带 `provider`（`agent-storage.ts:47`）。活会话上的 profile picker 只会改 model/mode，不会改 provider。
- 已建会话是否显示「当前用的哪份配置」：显示的是 provider label（因为它就是选中的那条 provider），不是「订阅 / API」二元标签。

### 1.5 进行中会话

切 provider = 新开一个 agent。旧进程不动。Reload 只刷新 skills/MCP/登录，不换凭据档。未能确定：reload 会不会重读 `config.json` 里该 provider 已改的 `env`——代码路径是「重新加载同一 provider 的进程」，按常理会，但本调研没有跟到 reload 实现。

### 1.6 多 provider 命名与去重

- 用户自选 ID + `label`。ID 全局唯一，校验失败直接拒绝（`provider-config.ts:69-76`）。
- 同一 CLI 多档 = 多条 `extends` 同一内建（`docs/custom-providers.md:258-261`）。
- 派生条目在 registry 里 `derivedFromProviderId` 指向基类（`provider-registry.ts:75-79`、`853`），UI 当独立 provider 列。
- 没有「同一凭据去重」；两份相同 key 的条目会并存。
- 覆盖内建 ID（如直接改 `agents.providers.claude.env`）是改默认 Claude，不是新增档。

---

## 2. Monet

参考仓：`D:/code/revisiting/work/repo_audit/repos/monet`

Monet 的对应物叫 **channel（渠道）**，明确服务「官方订阅 / 官方直连 / 第三方中转」三态，而且是**同一引擎进程上的运行时绑定**，不是新开一种 agent。

### 2.1 配置模型

文件布局写在 `src-tauri/src/channels.rs:1-10`：

```
~/.monet/
  settings.json          应用设置：默认会话/Agent 渠道 + 渠道展示元数据
  channels/<id>.json     渠道配置；`_ccSpace.connection` 存共享连接
  runtime/<sid>-<ns>.json  per-spawn 合成产物（进程结束即删）
```

红线（同文件 `9-10`）：`authToken` 不回传前端（list 只给掩码）、不进 argv；经 `--settings` 文件路径 + spawn env 注入。读取用时重读，不做进程级缓存。

内建保留 ID（`22-38`）：

| ID | 语义 |
|---|---|
| `official` | 「跟随 CLI / 零注入」。不对应 `channels/` 下的文件。CLI `~/.claude/settings.json` 什么样就什么样。 |
| `official-direct` | 「官方直连」。无渠道文件。合成压制 settings：强制 `ANTHROPIC_BASE_URL=https://api.anthropic.com`，空 token → CLI 视为未设、回落 OAuth 订阅。挤掉用户 CLI 配置里的第三方认证/路由键。 |
| `apple-fm` | Apple Foundation Models 虚拟渠道。 |

用户渠道：`channels/<id>.json`，ID 字母数字 `-_`，1–64 字符（`120-134`）。结构（`138-171`）：

- `_ccSpace.connection`：`baseUrl` + `authMode`（`bearer` / `none`）+ `authToken`
- 可选 `claude` / `codex` 引擎覆盖（空字段继承共享 connection）
- 顶层 `env`：模型角色映射（21 个 `ANTHROPIC_DEFAULT_*` / `ANTHROPIC_CUSTOM_MODEL_OPTION*` / `ANTHROPIC_MODEL`，`49-81`）
- `engineSupport`：同一渠道可绑 Claude + Codex

凭据落盘：`write_json_0600`（`611-619`），Unix 上 `0o600`。前端 `ChannelInfo.authTokenMasked`（`src/composables/useChannels.ts:5-28`）。未见 keychain。

`settings.json` 按引擎分槽记默认渠道/模型/effort（`channels.rs:379-409`）：`defaultSessionChannels` / `defaultAgentChannels` 各一份 `claude-code` / `codex`。

### 2.2 切换粒度

| 粒度 | 有没有 | 证据 |
|---|---|---|
| 全局默认 | 有。设置页按引擎选默认会话渠道 | `src/views/SettingsView.vue:1059-1096`；`channels.rs:1869-1874` |
| 每项目 | 未见独立项目凭据 | — |
| 每会话 | **有**。`SessionSettings.channelId` 存在 `localStorage` `monet:session-settings:<sid>` | `src/composables/useSessionSettings.ts:4-6, 32-41` |
| 引擎会话草稿 | 另一份 `monet:engine-run-config:<sid>`，含 `channelId` | `src/engines/runConfig.ts:1-8, 46-51` |
| 会话中途切 | **允许**。下一回合 spawn 时若 channel 与活进程不同则重启进程 | `src-tauri/src/streaming.rs:1165-1201` |

空线程在首条消息前切渠道：新建线程并原位替换草稿（`src/engines/draftChannel.ts:61-68`），不污染已有 transcript。

`official` 与 `null` 对 Codex 视为同一渠道（不注入 Provider，`draftChannel.ts:23-26`）。

### 2.3 注入机制

核心是 `prepare_injection`（`channels.rs:2203-2291`）+ spawn 消费（`streaming.rs:915-977`）。

对 Claude Code：

1. `channel == official`（或空）：`prepare_injection` 可返回 `None`（无 ultracode/advisor/fastMode 时，`2210-2212`）→ **零注入**，CLI 自己的 settings 生效。
2. `channel == official-direct`：合成 `{ env: { ANTHROPIC_BASE_URL: "https://api.anthropic.com" } }`（`2217`），再 fortify：空 `ANTHROPIC_AUTH_TOKEN` + 防御键清空。
3. 用户渠道：读 `channels/<id>.json`，把 connection 写成 `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN`（`2237-2241`）。
4. 防御清扫 `DEFENSE_ENV_KEYS`（`41-47`）：`ANTHROPIC_API_KEY`、`CLAUDE_CODE_USE_BEDROCK`、`CLAUDE_CODE_USE_VERTEX`、`CLAUDE_CODE_USE_FOUNDRY`。无 token 时再清 `ANTHROPIC_AUTH_TOKEN`（`2251-2258`）。空串写入 settings + `env_remove`（`streaming.rs:971-973`）。
5. 合成 JSON 写到 `~/.monet/runtime/<session>-<nanos>.json`，spawn 加 `--settings <path>`（`streaming.rs:924-926`），同时把 env 对注入进程（`974-976`）。**不改 `~/.claude/settings.json`。**
6. 进程退出删 runtime 文件；启动兜底清空（`channels.rs:6-7, 2293-2302`）。

`official` 渠道在设置页会显示 CLI 实际指向：只读 `~/.claude/settings.json` 的 `env.ANTHROPIC_BASE_URL`，有 host 则标 `third-party`（`get_cli_env_target`，`channels.rs:1913-1931`；UI `SettingsView.vue:1205`）。这是提示，不是注入。

Codex：渠道可 `mode: managed` 带 `providerId`，另有 `apply_channel_options`（`engines/codex/runtime.rs`，本调研未逐行展开；Claude 路径已足够回答「订阅 vs API」）。

Agent 服务（非会话 CLI）对 `official-direct` 走固定 `runtime/official-direct.json`（`channels.rs:1897-1908`；`agent.rs:104-125`）。

### 2.4 UI 形态

- **设置页「渠道」**：默认会话渠道（按引擎）、智能增强默认、连接列表（增删改、探活、enable 开关）、`ChannelForm`（`SettingsView.vue:1050-1227`）。
- **会话内切换器**：顶栏 `RunConfigCapsule`（`src/components/topbar/RunConfigCapsule.vue:148-207`）。选项含 `official`（跟随引擎 / 跟随 CLI）、`official-direct`（官方直连）、用户渠道。
- **新建任务**：`NewTaskEnginePicker` 用引擎默认渠道创建（`src/components/workbench/NewTaskEnginePicker.vue:102-119`）。
- **已建会话显示当前配置**：
  - capsule 显示当前 channel；若已选但尚未附着到活进程，标 `channelPending`（`EngineSessionDetail.vue:358`；`RunConfigCapsule.vue:423`）。
  - `official` 且 CLI 实际是第三方时，用 `observedChannelLabel` 提示真实去向（`RunConfigCapsule.vue:424-425`）。
  - transcript 里渠道切换画横线 `channelMarks`（`useSessionSettings.ts:17-27, 257-270`）。**不进 jsonl**；清本地数据后无从还原。新会话不继承 marks。

### 2.5 进行中会话

`send_message` 发现活进程后比较 `sp.channel` 与本次 `channel`（以及 effort / fastMode / advisor / chrome / extra_args 等启动参数）。不等则 `close_session` 再按新配置 spawn（`streaming.rs:1165-1201`）。注释：*「启动配置变更，重启进程」*。

因此：**下一回合生效，通过重启进程**；不是热替换 env。模型本身可用 CLI `set_model`，但「切回默认」也要重启（`1179-1181`）。

空草稿切渠道走 `rebindDraftChannel`：新 session 替换旧草稿（`draftChannel.ts:65-68`）。

### 2.6 多 provider 命名与去重

- 用户渠道 ID 自选，与三个保留 ID 冲突则拒绝（`channels.rs:124-126`）。
- 展示名 `ChannelMeta.name`，与 ID 分离。
- 同一 base_url + token 可以建两条，没有内容去重。
- `official` 不是「官方 Anthropic」的保证——它是「不注入」。真正强制订阅是 `official-direct`。这是 Monet 最值得抄的语义拆分。

---

## 3. codeg 现状（本 worktree 核实）

工作目录：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/o39-research`（分支 `wt/o39-research`）。

### 3.1 已经有的：agent-type 级「订阅 / 自定义端点 / 模型供应商」

Claude Code 设置页已有三态（`src/components/settings/acp-agent-settings.tsx:169-174, 11160-11193`）：

- `official_subscription`
- `custom`（手填 `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN`）
- `model_provider`（绑一条 `model_provider` 表记录）

`CLAUDE_AUTH_MODE` 写进该 agent 的 `env_json`（`821-860`）。`inferClaudeAuthMode` 对旧行兼容：显式 knob > `model_provider_id` > 有自定义凭据则 `custom`，否则 `official_subscription`（`833-844`）。

`model_provider` 表（`src-tauri/src/db/entities/model_provider.rs:4-16`）：`name, api_url, api_key, agent_type, model`。设置页 `/settings/model-providers`（`src/app/settings/model-providers/page.tsx`）。绑定字段在 `agent_setting.model_provider_id`（`src-tauri/src/db/entities/agent_setting.rs:4-17`）——**每个 agent_type 一行，不是每个会话**。

`CustomAgentSpec`（`src-tauri/src/acp/custom_registry.rs:135-144`）是 ACP 发行物（npx / uvx / 平台 binary），**不是**订阅/API 凭据档。binary 上的 `env` 是启动发行物用的，与「这个 Claude 会话走哪家网关」无关。

### 3.2 注入：`build_session_runtime_env`

单源（`src-tauri/src/commands/acp.rs:9328-9389`）。调用方：`acp_connect`、delegation spawn、probe、`session_dispatcher.rs:187`、automation / work_task / host_control / chat_channel。

拼装顺序：

1. `agent_setting.env_json`
2. 原生配置文件（Claude = `~/.claude/settings.json` 的 `env` / `api_base_url` / `api_key`，`8817-8856` + `7580-7582`）
3. 若 `model_provider_id` 有值，`apply_model_provider_env` 用供应商的 url/key **覆盖**（`8858-8879`）
4. git credential helper env
5. OpenClaw 无 session 时的 reset flag

Claude 键名（`8742-8748`）：`ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_MODEL`。另有模型角色映射到 `ANTHROPIC_DEFAULT_*` / `ANTHROPIC_CUSTOM_MODEL_OPTION*`（`8882-8897`）。

官方订阅策略 `apply_claude_env_policy`（`src-tauri/src/acp/connection.rs:146-179`）：当 `CLAUDE_AUTH_MODE=official_subscription` 时，把子进程将要继承的 `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY` 写成空 → spawn 层 `env_remove`。这已经是 Monet `official-direct` 防御清扫的近亲，但只在 **agent_type 全局** 生效。

### 3.3 危险点：绑定供应商会改用户全局 CLI 文件

`cascade_update_agent_config`（`acp.rs:9001-9040`）在 model provider 变更时把 url/key/model **写进 `~/.claude/settings.json`**（经 `persist_agent_local_config_json`）。Codex 还会改 `~/.codex/auth.json` 和 `config.toml`（`9054+`）。

后果：同一台机器上「会话 A 用订阅、会话 B 用中转」会互相踩全局文件。Paseo 和 Monet 都避免这么干：凭据只 overlay 到**这一次 spawn**。

### 3.4 切换粒度（现状）

- **全局 / per-agent-type**：有。改设置后 `refresh_config_staleness` + `fingerprint_config`（`acp.rs:9409-9483`）把已跑会话标 stale。
- **每会话**：`conversation` 表无 provider / auth 字段（`src-tauri/src/db/entities/conversation.rs:51-106`）。已有 `preferred_mode_id`、`preferred_config_values`（ACP 选择器：model / thinking），可作会话级扩展挂点，但今天不带凭据档。
- **新建会话 UI**：`AgentSelector` 只选 agent_type；会话一旦入库即 disable（`conversation-detail-panel.tsx:2207-2223`）。没有「这份 Claude 用订阅还是 API」的下拉。
- **进行中**：`SessionConfigStaleBanner`（`src/components/chat/session-config-stale-banner.tsx:17-31`）提示 reconnect。Reconnect 用**当前全局** settings 重拉进程，不是会话自己的档。

`build_session_runtime_env` 的 `session_id` 参数目前只影响 OpenClaw reset flag（`9385-9387`），**不按会话选凭据**。

### 3.5 凭据存储（现状）

- `model_provider.api_key`：SQLite 明文。list API 另给 `api_key_masked`，但 `ModelProviderInfo.api_key` 仍回传明文（`src-tauri/src/models/model_provider.rs:3-14, 44-49`）。
- `agent_setting.env_json`：SQLite。
- `keyring_store.rs` 只服务 GitHub token 和 chat-channel token，**不服务模型供应商 key**。
- 再加一份镜像在 `~/.claude/settings.json`（cascade）。

### 3.6 codeg 要补的缺口

| 层 | 缺口 | 说明 |
|---|---|---|
| 数据 | 没有「可复用的凭据档」与会话的 **N:1 绑定** | `model_provider` 是供应商目录，但只能绑在 `agent_setting` 上（一 agent_type 一份）。会话行没有 `provider_profile_id`。 |
| 数据 | 没有「官方直连」与「跟随 CLI」的拆分 | 只有 `official_subscription`，语义接近 Monet `official-direct`（清继承 env）。没有 Monet `official`（零注入、承认 CLI 可能已经是中转）。 |
| 注入 | spawn env 已具备，但会被全局 settings.json 污染 | 要做 per-session，必须停止（或门控）`cascade_update_agent_config` 对 Claude 原生文件的写入，改为只 overlay 这一次 spawn。 |
| 注入 | `build_session_runtime_env(session_id)` 未按会话解析档 | 需要：会话绑定 > agent_type 默认 > 官方策略。 |
| UI | 设置页已能配，新建会话 / 会话头没有选 | 用户拍板的场景是「每个会话用哪种」，不是「整个 Claude Code 用哪种」。 |
| UI | 已建会话不显示当前档 | 无 capsule、无 pending、无 transcript 横线。 |
| 生命周期 | stale/reconnect 只跟全局指纹 | 需要「这份会话的档变了 / 用户在会话里改了档」→ 重启该进程。Monet 的 `sp.channel != channel` 就是这个。 |
| 兼容 | 见结论 | 默认不迁移现有会话。 |

`CustomAgentSpec` 这条线本次不用扩：它解决「怎么把一个自定义 ACP 二进制拉起来」，不解决「Claude Code 走订阅还是 API」。

---

## 4. 结论

### 4.1 两家对比

| 维度 | Paseo | Monet | codeg 现状 |
|---|---|---|---|
| 配置档是什么 | 派生 provider（`extends: claude` + `env`） | channel（含两个虚拟官方 ID） | agent_type 上的 auth mode + 可选 `model_provider` |
| 存哪 | `~/.paseo/config.json` 明文 | `~/.monet/channels/*.json` 0600 + `settings.json` | SQLite `model_provider` / `agent_setting.env_json` + **写入** `~/.claude/settings.json` |
| 切粒度 | 创建 agent 时选；活进程不能换 provider | 每会话；下一回合重启进程 | 全局 per-agent-type；会话无绑定 |
| 注入 | spawn env overlay；不改 `~/.claude` | `--settings` 合成文件 + env；防御 `env_remove`；不改 `~/.claude` | spawn env overlay **且** cascade 改原生文件 |
| 「订阅」怎么表达 | 内建 `claude` 不设 API key，走 CLI 登录 | `official-direct` 强制官方 URL + 空 token → OAuth；`official` 是跟随 CLI | `CLAUDE_AUTH_MODE=official_subscription` + `apply_claude_env_policy` 清继承 env |
| 「API / 中转」 | 新 provider 条目 + `ANTHROPIC_*` | 用户渠道文件 | `custom` 或 `model_provider` |
| UI | 新建时 provider 下拉；设置页改 profile 不含凭据表单 | 设置页渠道库 + 会话顶栏胶囊 + transcript 横线 | 仅设置页；新建会话只选 agent |
| 中途切换 | 不能；新开 agent | 能；重启同一会话进程 | 改设置后 banner 要求 reconnect（用新的**全局**配置） |
| 去重 | ID 唯一，内容可重复 | ID 唯一（保留字冲突拒绝），内容可重复 | `model_provider.name` 无强唯一约束（未能从本调研确认 DB unique） |

### 4.2 各自可抄

**从 Monet 抄（更贴用户原话「同一 CLI 上切订阅/API」）：**

1. 虚拟渠道拆成「跟随 CLI」和「强制官方订阅」。否则用户 `~/.claude/settings.json` 里若已有中转，所谓「官方」其实还在走代理。
2. per-spawn 合成 settings + `env_remove` 防御键，**不要改** `~/.claude/settings.json`。
3. 会话级 `channelId`，活进程比较后重启。
4. 顶栏胶囊 + `channelPending`（已选 ≠ 已附着）。
5. 设置页是渠道**库**，默认值按引擎分槽；会话可以覆盖。

**从 Paseo 抄：**

1. 配置档是一等公民：有稳定 ID、label、env、可选模型列表、`disallowedTools`。中转要禁 WebSearch。
2. 创建时必须显式选档；活进程不能悄悄换「哪家网关」（避免 transcript 半截换计费主体却还以为是同一会话）。
3. 派生而不是改内建：保留一条干净的官方 Claude，中转是旁边的条目。
4. Agent Profile（启动包）和凭据档分离——模型/思考档不要和 API key 捆死。codeg 已有 `preferred_config_values`，应对齐这个分离。

**不要抄：**

- Paseo 把凭据明文塞进 daemon `config.json`、且没有 GUI 编辑渠道（对 codeg 桌面用户不友好）。
- Monet 把会话渠道只存在 **localStorage**（换机器 / 清站点数据 / 服务器模式会丢）。codeg 有 SQLite，应落库。
- codeg 自己的 cascade 写全局 CLI 文件——这是 per-session 的对立面。

### 4.3 给 codeg 的方案雏形

#### A. 数据模型

建议引入「凭据档 / 渠道」（名字待拍板），与现有 `model_provider` 的关系二选一：

- **方案 A1（推荐，改动小）**：把 `model_provider` 升级为渠道库。增加 `kind`：`cli_passthrough | official_subscription | api_endpoint`。`cli_passthrough` / `official_subscription` 是内建行，不可删。`api_endpoint` 复用现有 `api_url` / `api_key` / `model` JSON。`agent_setting.model_provider_id` 继续当 **agent_type 默认档**。
- **方案 A2**：新表 `provider_profile`，`model_provider` 只保留「API 端点」子集。迁移成本高，除非要彻底改名。

会话绑定：

```
conversation.provider_profile_id  NULLABLE
```

- `NULL` = 跟随该 `agent_type` 的 `agent_setting`（**现有会话全部保持 NULL → 行为不变**）。
- 非 NULL = 本会话钉死这一档，与全局设置脱钩。

不要把 API key 再写进 `conversation` 行。档在库里，会话只存外键。

`preferred_config_values` 继续只管 ACP 选择器（模型、thinking）。凭据档可以**建议**默认模型，但会话模型选择仍走现有选择器。

#### B. 注入

唯一注入点继续是 `build_session_runtime_env`，但解析顺序改为：

1. 若 `session_id` 能解析到 conversation 且 `provider_profile_id` 非空 → 用该档。
2. 否则用 `agent_setting` 的 auth mode / `model_provider_id`（今日行为）。
3. 档 = `official_subscription` → 现有 `apply_claude_env_policy`（可再对齐 Monet：强制 `ANTHROPIC_BASE_URL=https://api.anthropic.com`）。
4. 档 = `cli_passthrough` → **不要**清 env、**不要**写 url/key（Monet `official`）。
5. 档 = `api_endpoint` → 写入 `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN`（及模型映射），并 `env_remove` `ANTHROPIC_API_KEY` / Bedrock / Vertex / Foundry，避免 CLI 用户配置漏进来。

**停止**（或加开关默认关）Claude 路径上的 `cascade_update_agent_config` 写 `~/.claude/settings.json`。全局默认只存在 SQLite；进程看到的是 spawn env。否则 per-session 无法成立。

Claude 不必上 Monet 的 `--settings` 文件，除非发现「只设 env 盖不住 CLI user settings」。Monet 同时做了 `--settings` 和 env，是因为 Claude 会合并多层 settings。若 codeg 验证「空串 env_remove + 显式 ANTHROPIC_*」已够，可少一条 runtime 文件。**这点需要实现期实测，调研未能替产品下结论。**

Codex：Paseo/Monet 都承认 Codex 读 `config.toml` 的 `model_providers`，单靠 `OPENAI_BASE_URL` 不够。codeg 已有 cascade 写 toml 的路径；per-session 时应对齐 Paseo：给这一次 spawn 一份隔离 provider 块，而不是改用户全局 `~/.codex/config.toml`。本调研未把 Codex 路径挖到可实施粒度，列为后续。

#### C. UI

1. **设置页（已有，增强）**：渠道库。保留现有 Model Providers 页或与 Claude 设置里的 auth 下拉合并。必须能建多条 API 档（工作中转 / 个人中转 / 官方订阅）而不互删。
2. **新建会话流**：`AgentSelector` 旁加「配置档」下拉，默认 = 该 agent 的 `agent_setting` 绑定。写入 `conversation.provider_profile_id`（用户显式选了才非 NULL；直接回车 → NULL → 跟随全局，兼容）。
3. **会话头部**：已建会话显示当前档名。允许改选 → 标 pending → 下一回合或点「Reconnect」重启该进程（复用 `SessionConfigStaleBanner` / `reapplyConfig`）。**不要**在 turn in-flight 时杀进程（banner 已有这个禁用条件，`session-config-stale-banner.tsx:28-29`）。
4. agent_type 本身在会话创建后仍不可换（现有 `disabled={dbConversationId != null}`），与 Paseo「活进程不能换 provider 种类」一致。换的是同一 Claude 上的凭据档，不是换到 Codex。

Transcript 横线（Monet `channelMarks`）是锦上添花，第一期可不做。若做，落 SQLite 或会话侧车，不要只写 localStorage（服务器模式没有这份存储）。

#### D. 兼容原则（默认不迁移现有会话）

- 现有 `conversation` 行 `provider_profile_id = NULL` → 继续读 `agent_setting`，观感与今天相同。
- 不改写用户已经填好的 `CLAUDE_AUTH_MODE` / `model_provider_id`。
- 不自动扫描 `~/.claude/settings.json` 生成渠道（Monet 只做只读提示 `get_cli_env_target`）。若 CLI 已经指向中转，用户在设置里看到的「官方订阅」可能和真实流量不一致——这正是要不要引入 `cli_passthrough` 的拍板点。
- 实现期若关掉 cascade 写 `settings.json`，已写进去的 url/key **不要自动删**；官方档依赖 `apply_claude_env_policy` 在 spawn 时盖过它们即可。主动清理是另一次迁移，需单独拍板。

### 4.4 需要用户拍板

1. **凭据存哪**  
   Monet：0600 文件，前端只见掩码。Paseo：明文 JSON。codeg 现状：SQLite 明文 + list API 仍带明文 key。  
   选项：继续 SQLite / 改走 `keyring_store`（桌面）+ server 文件 / 0600 侧车。服务器模式下没有 OS keychain。

2. **「官方」到底是哪种**  
   - 只保留今天的 `official_subscription`（强制清代理 env，≈ Monet `official-direct`）  
   - 再加 `cli_passthrough`（≈ Monet `official`），承认有人把中转写在 `~/.claude/settings.json` 里、希望 codeg 别动  
   用户原话是「订阅或者 api 之间切换」。若订阅 = Anthropic 账号登录，应对齐 `official-direct`，否则「选了订阅」仍可能打到 settings.json 里的中转。

3. **中途切换是否允许**  
   Monet：允许，重启进程。Paseo：不允许换 provider。建议允许（同一 Claude、同一 transcript），但必须重启进程且 turn in-flight 时拒绝。

4. **关掉对 `~/.claude/settings.json` 的 cascade 写入？**  
   per-session 几乎是前提。副作用：终端里直接跑 `claude` 不再自动吃 codeg 里绑的供应商。Monet/Paseo 都接受这个边界（终端要复用渠道时显式 `--settings`）。

5. **命名**  
   对外叫「渠道 / 配置档 / 供应商 / 连接」？Monet=渠道，Paseo=provider，codeg 已有「模型供应商」。避免再造第四个词，除非要跟现有「模型供应商」页合并。

6. **第一期范围**  
   只做 Claude Code，还是 Claude + Codex 一起？Codex 注入比 Claude 重（要碰 `model_providers`）。Paseo/Monet 都先把 Claude 的订阅/API 问题做干净。

---

## 5. 未能确定

- Paseo reload-agent 是否重读已修改的 provider `env`（reload 实现未跟到）。
- Monet Codex managed channel 的完整注入（只确认有 `codex.providerId` / `apply_channel_options`）。
- Claude Code 是否在「进程 env 已设 ANTHROPIC_*」之后仍合并 user `settings.json` 里的同名键——决定 codeg 要不要上 `--settings` 合成文件。
- `model_provider.name` 是否有 DB 唯一约束。
- Paseo 是否在任何 GUI 里编辑 `agents.providers.*.env`（设置页全文检索未找到；文档只教手改 `config.json`）。
