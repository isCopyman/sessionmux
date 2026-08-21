# JSON 持久化面盘点（切面 ⑤ 前置）

- 日期：2026-08-21
- 工作树：`wt/serde-inventory`
- 范围：只读审计。产品代码未改。
- 方法：grep `serde_json::to_string|to_value|to_writer`、`rename_all`、`column_type = "Text"`、实体 `*_json` 列，再顺藤摸瓜到写入点与结构体。未通读大文件。
- 风险：红 = 已落盘，改 casing 必须迁移或双读；黄 = 落盘但短命/可重建；绿 = 不落盘，同版本内存/线契约。

---

## 1. SQLite 列里存 JSON 的

SeaORM 实体目录 `src-tauri/src/db/entities/` 没有 `ColumnType::Json`。JSON 一律走 `String` / `Text`。协作 / 队列 / Timer 没有实体文件，走 raw SQL 表。

`folder_link.name/target_path`、`quick_message.content`、`remote_workspace_connection.token`、`work_task_template.name/title` 也标了 `Text`，内容是路径或明文，**不是** serde JSON，下列不收录。`opened_tab` 全是关系列，布局不在库里（见 §2 localStorage）。

### 1.1 有 SeaORM 实体的 JSON 列

| 表.列 | file:line | 写入点 | 结构体 / 形状 | 当前 casing | 风险 |
|---|---|---|---|---|---|
| `automation.config` | `db/entities/automation.rs:52-55` | `automation_service.rs:290,327` `to_string(&draft.config)`；引擎 `automation/engine.rs:405` `to_value(&task_cfg)` | 列存 `serde_json::Value` 原样；运行时 `from_value` → `AutomationConfig`（`models/automation.rs:92`）。内嵌 `prompt_blocks: Vec<Value>` 再 `from_value` → `PromptInputBlock` | **结构体字段默认 snake_case**（无 `rename_all`）。测试/引擎样本：`prompt_blocks`、`display_text`、`target_conversation_id`。`AutomationAction` 枚举值 `rename_all = "snake_case"`（`automation.rs:74`）：`launch_session` / `enqueue_task` / `queue_prompt` | **红** |
| `work_task.config` | `db/entities/work_task.rs:55-58` | `work_task_service.rs:441,519` `to_string(&draft.config)` | 同上，`WorkTaskConfig`（`models/work_task.rs:122`）。引擎 `work_task/engine.rs:3365-3369` 把 `prompt_blocks` 解成 `PromptInputBlock` | snake_case：`prompt_blocks`、`display_text`、`agent_type`、`mode_id`、`config_values`、`label_snapshot`。遗留行靠 `#[serde(default)]` | **红** |
| `work_task.merge_state` | `work_task.rs:78-82` | `work_task_service.rs:1441` `to_string(state)` | `WorkTaskMergeState`（`work_task.rs:298`） | 默认 snake_case：`pre_merge_head`、`delete_worktree`、`auto_message` | **红** |
| `work_task.pending_merge` | `work_task.rs:83-89` | `work_task_service.rs:1551` `to_string(intent)` | `WorkTaskQueuedMerge`（`work_task.rs:320`） | snake_case：`delete_worktree`、`queued_at` | **红**（排队中的合并意图，进程重启后仍要消费） |
| `work_task.preflight` | `work_task.rs:99-102` | `work_task_service.rs:1817` `to_string(preflight)` | `WorkTaskPreflight`（`work_task.rs:334`） | snake_case：`exit_code`、`output_tail` | **红**（review 灯；可重跑 preflight，但现存行会解失败） |
| `work_task_settings.config` | `work_task_settings.rs:4-16` | `work_task_service.rs:2178` `to_string(settings)` | `WorkTaskFolderSettings`（`work_task.rs:140`） | snake_case。单元测试 `work_task.rs:362+` 用遗留 JSON `"default_agent_type"` 证明 | **红** |
| `work_task_template.config` | `work_task_template.rs:4-17` | `work_task_service.rs:2248` `to_string(&draft.config)` | 同 `WorkTaskConfig` | snake_case | **红** |
| `work_task_event.payload` | `work_task_event.rs:19-21` | `record_event(..., Some(serde_json::json!({...})))` 如 `work_task_service.rs:561` `{ "action": "delete" }`；引擎读 `payload.get("action"|"intent"|"feedback"|"kind"|"id")`（`work_task/engine.rs:3785-3804`） | **无具名结构体**，手写 `Value` | 现存键是短 snake/单字：`action`、`intent`、`feedback`、`from`/`to`、`kind`、`id`。`status_changed` 注释例 `{from, to}`（`work_task_event.rs:19`） | **红**（append-only 时间线；改键名旧事件静默丢字段） |
| `custom_agent.spec_json` | `custom_agent.rs:19-20` | `custom_agent_service.rs:100` `to_string(&def.spec)`；读 `from_str::<CustomAgentSpec>`（同文件:25） | `CustomAgentSpec`（`acp/custom_registry.rs:136`）+ `NpxSpec`/`UvxSpec`/`BinaryPlatformSpec` | **默认 snake_case**，且注释写明「ACP registry `distribution` 对象 verbatim」。`CustomDistributionKind`/`CustomAgentSource` 枚举 `rename_all = "snake_case"`（`custom_registry.rs:48,151`） | **红**。额外约束：形状对齐上游 ACP registry，不是 codeg 私有 DTO |
| `agent_setting.env_json` | `agent_setting.rs:13` | `commands/acp.rs:5142,8805+` `to_string(&BTreeMap)`；`serialize_env_map` | `BTreeMap<String,String>`，键是环境变量名（`OPENAI_MODEL`、`CODEG_HOST_TOOLS`…） | **不是 Rust 字段 casing**。改 serde rename_all 碰不到这些键 | **绿**（对切面 ⑤ 字段改名）；内容本身仍是用户配置，勿当可丢缓存 |
| `agent_setting.agent_type` | `agent_setting.rs:8` | `agent_setting_service.rs:55,123,…` `to_string(&AgentType)` | `AgentType`（`models/agent.rs:23`）。测试钉死 JSON 是 `"claude_code"` 这种带引号的 **wire 字符串**（`agent.rs:242-246`），不是结构体字段 | 枚举 wire：`claude_code` / `open_code` / `custom:goose`。**改 `as_wire` 会孤立行** | **红**（值空间，不是 camelCase/snake_case 字段问题）。切面 ⑤ 不要动 |
| `model_provider.agent_types_json` | `model_provider.rs:11` | `model_provider_service.rs:20,61` `to_string(&vec![AgentType])` | `Vec<AgentType>` → JSON 字符串数组 `["codex"]` | 同 AgentType wire | **黄/红之间偏黄**：列已有平行 `agent_type` 单值列（迁移 `m20260518` 从 `$[0]` backfill）；读侧 `models/model_provider.rs:37` 当 `Vec<String>`。改 casing 仍会解失败 |
| `conversation.preferred_config_values` | `conversation.rs:94-98` | `conversation_service.rs:1266` `to_string(&BTreeMap<configId,valueId>)` | 不透明 map，键是 selector id（含 `__codeg_host_model__` 等） | 键不是 serde 字段名 | **绿**（对 rename_all）；map 内容仍持久 |
| `chat_channel.config_json` | `chat_channel.rs:11` | `commands/chat_channel.rs:310,382` `to_string`；读 `manager.rs:433` `from_str::<Value>` 再解 `TelegramConfig`/`LarkConfig`/`WeixinConfig`（`chat_channel/types.rs:14-28`） | `TelegramConfig`：`chat_id`、`topic_mode`；`LarkConfig`：`app_id`、`chat_id`；`WeixinConfig`：`base_url`。均无 `rename_all` | snake_case | **红** |
| `chat_channel.event_filter_json` | `chat_channel.rs:12` | 通道 CRUD 与全局 `app_metadata` 的 `chat_event_filter` 是两条线。通道列由实体读写；全局键见 §1.3 | 过滤列表 JSON（字符串数组一类）。未能逐条钉死通道列的具名结构体，读侧 `event_subscriber.rs:353` 当 `Value` | 未能确定通道列是否与全局 filter 同形 | 通道列：**红**（用户配过就会留下） |
| `chat_channel_thread_binding.provider_payload_json` | `chat_channel_thread_binding.rs:18` | `manager.rs:242` 读出塞进 `ChannelMessageTarget.provider_payload` | 供应商不透明 JSON | 第三方形状，冻结 | **红**（冻结，不要当 codeg DTO 改名） |
| `fork_relation.anchor` | `fork_relation.rs:43-45` | 实体注释：provider-native message anchor JSON | 无 codeg 结构体 | 第三方形状 | **红**（冻结） |
| `app_metadata.value` | `app_metadata.rs:10` | 通用 KV。JSON 子集见 §1.3 | 混标量与 JSON | 按 key | 见下 |

### 1.2 raw SQL 表（无 entities/*.rs）

协作表 `collaboration_event` **没有 metadata JSON 列**（`m20260816_000002_collaboration.rs:18-33`）。快照是独立 TEXT 列。RFC 里「collaboration_event 的 metadata」实际是 **投递进队列时写进 `PromptQueueDraft.blocks[].text` 的信封**。

| 表.列 | file:line | 写入点 | 结构体 / 形状 | casing | 风险 |
|---|---|---|---|---|---|
| `conversation_prompt_queue_item.draft_json` | 迁移 `m20260816_000001_prompt_queue.rs:31`；读写 `prompt_queue_service.rs:85,366,535` | `to_string(&PromptQueueDraft)` / `from_str::<PromptQueueDraft>` | `PromptQueueDraft`（`models/prompt_queue.rs:7-12`）：`blocks` + `display_text`；`blocks` 是 `Vec<PromptInputBlock>`（`acp/types.rs:5`，`tag = "type", rename_all = "snake_case"`） | **草稿外层 camelCase**（`displayText`）。**块内 snake_case**（`type: "text"\|"image"\|"resource_link"`，`mime_type`） | **红**。队列项可跨重启；协作投递还会把信封 JSON 嵌进 `text` |
| 信封 JSON（嵌在 draft 文本里，不是列） | `collaboration_service.rs:64-65,590-613` | `json!({ "eventId", "deliveryId", "sourceConversationId", …})` 再 `to_string`，夹在 `<<<CODEG_SESSION_MESSAGE_V1:…>>>` | 手写 Value，无 serde 结构体。`ENVELOPE_VERSION = 1` | **camelCase 键**：`eventId`、`sourceConversationId`、`expectsReply`、`replyToEventId`、`bodyTruncated`、`parentSourceConversationId`… | **红**。已入队的信件/Room 提及正文里带着这份 JSON；改键名旧信封解析会裂（若有解析器）或 LLM 提示词契约漂移 |
| `conversation_timer` | `m20260816_000006_session_timer.rs` | 列存标量（`idle_secs`、`prompt_text`…），**无 JSON 列** | 线 DTO `SessionTimer` 是 camelCase（`models/session_timer.rs:15`），但那是 API，不是落库 JSON | — | **绿**（对切面 ⑤）。改 Timer 线字段不影响 SQLite |
| `collaboration_delivery.embedded_turn_ref` | 迁移 `m20260816_000002:49` | 字符串 turn id，不是 JSON | — | — | 排除 |

`PromptQueueItem` / `PromptQueueSnapshot`（camelCase）是读出后的线 DTO，不整包落库。落库的是 `draft_json` + 标量列（`state`/`source` 为 snake_case 枚举字符串）。

### 1.3 `app_metadata` 里值为 JSON 的 key

写入统一走 `app_metadata_service::upsert_value`。

| key | 写入点 | 结构体 | casing | 风险 |
|---|---|---|---|---|
| `pet.config` | `commands/pet.rs:32,112,126-128` | `PetWindowConfig`（`models/pet.rs:205-222`）`rename_all = "camelCase"` | camelCase：`activePetId`、`alwaysOnTop` | **红** |
| `logging.level` | `logging/mod.rs:27`；`commands/logging.rs:86-90` | `LogSettings`（`logging/mod.rs:100`） | 默认 snake_case：`level` + `targets: [{target, level}]`。`LogLevel` 是 `lowercase` 枚举 | **红** |
| `system_proxy_settings` | `commands/system_settings.rs:24,282` | `SystemProxySettings`（`models/system.rs:3`） | snake_case：`proxy_url` | **红** |
| `system_language_settings` | `:25,330` | `SystemLanguageSettings`（`system.rs:35`） | 字段默认；`AppLocale`/`LanguageMode` `rename_all = "snake_case"`（`zh_cn`、`zh_tw`） | **红** |
| `system_terminal_settings` | `:26,246` | `SystemTerminalSettings`（`system.rs:42`） | snake_case：`default_shell` | **红** |
| `git_settings` | `commands/version_control.rs:14,159` | `GitSettings`（`system.rs:100`） | snake_case：`custom_path` | **红** |
| `github_accounts` | `version_control.rs:16,206` | `GitHubAccountsSettings` + `GitHubAccount`（`system.rs:105-119`） | snake_case：`server_url`、`avatar_url`、`is_default`、`created_at` | **红** |
| `chat_event_webhooks` | `commands/chat_channel.rs:342-384` | `Vec<WebhookConfig>`（`chat_channel/webhook.rs:22`）：`url`、`enabled` | 无 rename（单音节） | **红**（键几乎不受 casing 影响） |
| `chat_event_filter` | `chat_channel.rs:290-324` | JSON 数组（代码当 `Value`/字符串列表） | 未能钉死具名结构体 | **红** |
| `opened_tabs_version` | `tab_service.rs:31,222` | 整数字符串，非 JSON 对象 | — | 排除 |
| 布尔/标量：`web_service_*`、`feedback`/`question`/`session_info`/`session_collaboration` 开关、chat authoring 两键、zoom/appearance、command prefix、message language | 各 commands | `"true"` / 端口号 / 语言码 | — | 排除（非对象 JSON） |

---

## 2. 磁盘文件（codeg 自己写的 JSON/JSONL）

不含第三方 agent 自己的转录（Claude/Codex 等 session 文件我们只读）。codeg **写入** agent 配置目录的 MCP/config 片段算在内。

| 路径 / 角色 | file:line | 结构体 | casing | 风险 |
|---|---|---|---|---|
| `~/.codeg/preferences.json` | `preferences.rs:14-21,50-52` | `AppPreferences`：`disable_hardware_acceleration` | snake_case | **红**（启动热路径、WebView2 开关） |
| 宠物目录 `pet.json` | `models/pet.rs:32,67-86`；写 `pets/mod.rs:227-239`、`marketplace.rs:645`、`codex_import.rs:194` | `PetManifest` `rename_all = "camelCase"`。注释：**字段名对齐 Codex `pet.json`**（`displayName`、`spritesheetPath`） | camelCase + `flatten extra` | **红，冻结**。改名会与 Codex 导入/市场包不兼容 |
| `~/.codeg/skills/` 下 experts / science manifest | `commands/experts.rs:157-174,383`；`commands/science.rs:136-153,367` | 私有 `Manifest`：`codeg_version`、`installed_at`、`pending_user_review` | snake_case | **红** |
| server 模式 `tokens.json` | `keyring_store.rs:43-82` | `HashMap<String,String>`（keyring 替代） | map 键是 `github-token:…` / `chat-channel:…` | **黄**（密钥文件；无结构体字段可改名） |
| 备份 ZIP 内 `manifest.json` | `commands/backup/manifest.rs:42-65`；`archive.rs:148` `to_vec_pretty` | `BackupManifest` `rename_all = "camelCase"`：`formatVersion`、`createdAt`、`appVersion`、`latestMigration`、`includesExternalTranscripts`… | camelCase | **红**。旧 `.codegbak` 必须仍能预览/恢复 |
| codeg ACP 转录 JSONL | `acp_transcript.rs:79-156,225-226` | `TranscriptHeader` 默认字段：`v`、`kind`、`agent`、`session_id`、`cwd`、`started_at_ms`、`continues_from`。`TranscriptEntry`：`t`、`k`、`p`。`EntryKind` snake_case。TurnEnd 的 `p` 注释写 **camelCase**：`stopReason`、`durationMs` | 头/条目外层偏 snake；payload 内混用 | **红**。这是 custom/ACP-native 会话的权威历史 |
| 回合计时 JSONL `<codeg_turn_timings_root>/<agent>/<session-id>.jsonl` | `turn_timings.rs:24-30,264-266` | 文档形状 `{"v","ord","conn","prompt_sha","started_at_ms","ended_at_ms"}`（短键 + snake） | 专用短名 | **黄**。可重建（缺了只是 footer 没时钟），但改名会让旧 journal 整文件被 order-gate 丢掉 |
| `persist_agent_local_config_json` | `commands/acp.rs:7639-7707` | 把 UI 的 `config_json` patch **merge 进 agent 自己的 JSON 配置文件**（OpenCode 整文件覆盖；其它 merge）。Codex/Cline/Kimi 走专用 TOML/JSON 路径 | **第三方 schema** | **红，冻结**。不是 codeg 模型 |
| MCP 设置 UI 写 `mcpServers` | `commands/mcp.rs:743,1563+,2405` 等，各 agent 的 `~/.claude.json`、`~/.gemini/settings.json`、`~/.cursor/mcp.json`… | 第三方 `mcpServers` 对象 | 第三方 | **红，冻结** |
| `codeg-mcp` 注入 | `acp/connection.rs:3526,3862` `inject_codeg_mcp` | **不写盘**。`session/new` 的 ACP `mcpServers` 数组里塞 stdio 伴生进程 | 线契约 | **绿**（进程内）。agent CLI 自己的 mcp 文件由上一行 UI 写，伴生注入不落那些文件 |
| OpenCode plugin 文档改写 | `acp/opencode_plugins.rs:302-305` | 读改写 OpenCode JSON 配置 | 第三方 | **红，冻结** |
| Codex model catalog JSON | `acp/codex_model_catalog.rs:531-535`；`codex_catalog_source.rs:71` | catalog 快照 | 生成物，可重扫 | **黄** |
| experts/science 之外的宠物 marketplace 包 | 同 `pet.json` | 同上 | camelCase | **红，冻结** |

### 2.1 前端 localStorage（布局/偏好，不在 SQLite）

RFC 点名「tab 持久化、配置块」。`opened_tab` 只存会话/房间页签集合；**分组布局在浏览器**：

| key | file:line | 形状 | 风险 |
|---|---|---|---|
| `workspace:tab-groups:v1` / `workspace:workbench:{id}:tab-groups:v1` | `stores/tab-store.ts:354,399-404,1370-1384` | `{ layout, assignments, selection, tileByGroup, drafts, activeDraft, sessionViewState }` —— **TS 字段名，无 serde** | **黄**。设备本地；坏了只丢分组/滚动位置，页签集合仍在 SQLite。改 TS 字段名会让旧 blob `isLayoutNode` 校验失败被丢弃 |
| `workspace:active-workbench-id:v1` | `tab-store.ts:355` | 纯 id 字符串 | 绿/黄 |
| `workspace:tile-mode` | `:351` | `"true"` | 排除 |
| appearance / theme / custom CSS / workspace bg | `lib/appearance-script.ts` 等 | 混标量与一份 custom theme JSON | **黄**（可重置主题） |
| agent 排序、workspace mode、office auto-preview、i18n、`codeg_token` | 各 context | 短命偏好 | **黄** |

server 模式多客户端：localStorage 不跨浏览器，改名只伤该浏览器。仍建议分组 blob 加版本键而不是改现有字段。

---

## 3. 跨进程 / 跨版本契约（不落盘，但 server 有升级窗口）

桌面 Tauri：前后端同版本，绿。`codeg-server`：旧静态前端 + 新二进制会共存一个升级窗口。

线协议两层：

1. **HTTP handler 请求壳**几乎全是 `rename_all = "camelCase"`（`web/handlers/**`，grep 约 **170+** 处）。嵌套的 `draft: AutomationDraft` / `WorkTaskDraft` **自己没有 rename_all**，所以 POST body 是 `{ "draft": { "folder_id": …, "config": { "prompt_blocks": … } } }` —— 外壳 camel，内核 snake。
2. **响应 / WS payload** 直接 `Json<Model>` / `emit_event(model)`，跟随该 model 自己的 serde。

### 3.1 有版本偏差风险的事件 / 帧（server 黄，桌面绿）

| 通道 / 帧 | 结构体 file:line | casing | 偏差风险 |
|---|---|---|---|
| `conversation://changed` | `ConversationChange` `event_bridge.rs:196` `tag=kind, snake_case`；内嵌 `DbConversationSummary` **默认 snake_case**（`models/conversation.rs:35`） | `kind: "upsert"` + `folder_id` / `agent_type` | **黄（server）**。前端 `types.ts` `ConversationSummary`/`Db` 族是 snake_case。改成 camelCase 旧前端会把 upsert 当未知 `kind` 丢进 status 分支（文件头注释 `event_bridge.rs:275-277` 已警告未知 kind 的危害） |
| `folder://changed` | `FolderChange` `:239` snake tag；`FolderDetail` 默认 snake | 同左 | **黄（server）** |
| `tabs://changed` | `TabsChanged` `:331` **无 rename_all** → `workbench_id`；`OpenedTab` snake（`folder.rs:85`）。`types.ts:557` 镜像 snake | snake | **黄（server）**。`OpenedTabsSnapshot`/`SaveTabsOutcome` 虽标了 camelCase（`folder.rs:103,114`），字段 `items`/`version`/`accepted`/`tabs` 单音节，实际无差异 |
| `prompt-queue://changed` | `PromptQueueSnapshot` camelCase（`prompt_queue.rs:106`） | camel | **黄（server）** |
| `collaboration://changed` / `room://changed` | `CollaborationChanged`、`RoomChanged` 等 camelCase（`collaboration.rs:216+`，RoomChanged `:657`） | camel | **黄（server）**。这是 types.ts 里 camel 的那一族 |
| `automation://changed` / `task://changed` | id-only 枚举，snake tag（`event_bridge.rs:353,378`） | 几乎无字段 | 低 |
| `session-timer://changed` | `SessionTimerChanged` `:315` 无 rename → `conversation_ids` | snake | **黄（server）**。Timer **详情** API 是 camelCase（`session_timer.rs:15`），事件却是 snake。已经是双约定 |
| `acp://event` / WS attach | `EventEnvelope` `acp/types.rs:52` 默认 `connection_id`；`AcpEvent` `tag=type, snake_case`（`:102`） | snake | **黄（server）**。流量最大的实时面 |
| WS 控制面 `ClientMsg`/`ServerMsg` | `web/ws_attach.rs:39,57` snake tags：`attach`/`snapshot`/`connection_id`/`event_seq` | snake | **黄（server）**。改 tag 会直接连不上 |
| HTTP 响应 `Automation`/`WorkTask`/`Conversation*` | 无 rename_all 的 Info 结构体 | snake，与 `types.ts` 注释一致（`types.ts:1841`：「Wire form is snake_case like Automations」） | **黄（server）** |
| `pet://sessions` | `PetSessionsPayload` camelCase（`pet.rs:328`） | camel | 桌面为主；server 也有 HTTP 快照。黄（server） |
| `workspace://transfer-progress`、backup progress | camelCase（`workspace_transfer.rs:19+`；`backup/manifest.rs:92`） | camel | 短命操作，黄偏低 |
| `workbench://place-session` | `types.ts:582` 已是 camelCase（`requestId`、`workbenchId`） | camel | 未能在本次把 Rust 发射结构体逐行钉死到同一文件；handlers/host-control 侧输入是另套 | 未能完全确定 Rust 发射体与 TS 是否逐字段同构 |

`codeg-mcp` JSON-RPC（`bin/codeg_mcp.rs:133` `to_string(resp)`）是伴生进程 stdio，跟前端版本无关；工具入参是 MCP schema。切面 ⑤ 若改 host-control 工具字段名，伤的是 **已在跑的 agent 会话**，不是旧前端。标 **黄（进程内工具契约）**，不是 DB 迁移。

---

## 4. serde 属性现状统计

### 4.1 `src-tauri/src/models/`

| 约定 | 数量级 | 代表 |
|---|---|---|
| 显式 `rename_all = "camelCase"` | **55** | 几乎全集中在 `collaboration.rs`（29）、`pet.rs`（16）、`prompt_queue.rs`（4）、`session_timer.rs`（3）、`folder.rs` 的 Snapshot/Outcome（2）、`conversation.rs` 的 fork/import key（3）、`token_usage.rs`（1）、`background.rs`（1） |
| 显式 `rename_all = "snake_case"` | **23** | **绝大多数是枚举**：协作策略/状态、`PromptQueueSource/ItemState`、`AutomationAction`、`FollowUpIntent`、`MessageRole`/`ContentBlock` tag、`AppLocale`、`ScanSessionStatus`、`PetState`… |
| 结构体无 `rename_all`（字段默认 snake_case） | 其余 Info/Draft/Config | `ConversationSummary`/`DbConversationSummary`、`FolderInfo`/`FolderDetail`/`OpenedTab`、`AutomationInfo`/`AutomationConfig`/`AutomationDraft`、`WorkTask*` 配置与 Info、`System*Settings`、`GitHubAccount`、`ChatChannelInfo`、`MessageTurn` 字段 |

`types.ts` 镜像这个分裂，而不是单独发明一套：

- snake 族：`ConversationSummary`（`agent_type`）、`FolderInfo`、`OpenedTab`（`folder_id`、`is_active`）、`Automation*`、`WorkTask*`（文件内注释写明 snake）
- camel 族：`PromptQueueItem`（`conversationId`、`displayText`）、`CollaborationDelivery`（`eventId`、`invocationPolicy`）、`SessionTimer`（`idleGraceSecs`）

### 4.2 `web/handlers`

请求/响应壳 **几乎纯 camelCase**（≥170 处 `rename_all = "camelCase"`；未见 handlers 里成规模的 snake `rename_all`）。这只改外壳字段（`folderId`、`automationId`），**不**改嵌套 model。

结果：server HTTP 出现「外壳 camel + 内核 snake」的三明治，例如 `CreateParams { draft: WorkTaskDraft }`（`web/handlers/work_task.rs:40-43`）。

### 4.3 其它值得单独记的

- `AcpEvent` / `PromptInputBlock` / `ContentBlock`：`tag = "type", rename_all = "snake_case"`（ACP/转录/队列块的公共形状）
- `WebEvent` 手工序列化字段名 `channel`/`payload`（`event_bridge.rs:19-24`）
- 实体里的 SeaORM 枚举（`ConversationStatus`、`WorkTaskStatus`、`TriggerKind`…）`rename_all = "snake_case"`，但这些进的是 **SQL 枚举字符串列**，不是 JSON 对象字段

---

## 5. 风险总表（按施工优先级）

### 红 — 改 casing = 数据迁移

改这些结构体的 `rename_all` 或字段名，已安装用户的 SQLite / 文件会解不出或静默 `Default`。

1. `WorkTaskConfig` / `WorkTaskFolderSettings` / `WorkTaskMergeState` / `WorkTaskQueuedMerge` / `WorkTaskPreflight` / 模板 config  
2. `AutomationConfig`（含 `action` 枚举值）  
3. `PromptQueueDraft` 外层 camelCase + 内层 `PromptInputBlock` snake_case（**同一 JSON 里两套**）  
4. 协作信封 camelCase 键（嵌在 draft 文本）  
5. `work_task_event.payload` 手写键  
6. `CustomAgentSpec`（+ 上游 registry 形状）  
7. `chat_channel.config_json` 的 Telegram/Lark/WeixinConfig  
8. `app_metadata`：`PetWindowConfig`（camel）、`LogSettings`/`System*`/`Git*`/`GitHubAccount`（snake）  
9. `~/.codeg/preferences.json`  
10. experts/science manifest  
11. ACP 转录 JSONL（`TranscriptHeader`/`TranscriptEntry`）  
12. 备份 `manifest.json`（camelCase）  
13. `pet.json`（**冻结，Codex 兼容**）  
14. 写入 agent 家目录的 MCP/config JSON（**冻结，第三方**）  
15. `AgentType` JSON 字符串（值空间冻结）  
16. `provider_payload_json` / `fork_relation.anchor`（冻结，第三方）

`PromptInputBlock` 特别危险：它同时出现在（3）队列草稿、（1）任务/自动化 `prompt_blocks`、以及 live `session/prompt`。改 `mime_type` → `mimeType` 会让旧任务永远 `bad prompt blocks`（`work_task/engine.rs:3368`）。

### 黄 — 落盘但可重建 / 短命

- 回合计时 JSONL（丢了只少时钟）  
- Codex catalog 生成文件  
- localStorage 分组布局 / 主题  
- server `tokens.json` 的键名（不是字段）  
- server 模式下一切 HTTP/WS 线结构（同版本桌面可忽略）

### 绿 — 不落盘

- 多数 `models/collaboration.rs` 线 DTO（库表是列，不是这些结构体的 JSON）  
- `SessionTimer` 线 DTO、`PetSessionEntry` 等  
- handler 外壳 camelCase  
- `inject_codeg_mcp` ACP 线注入  
- `env_json` / `preferred_config_values` 的 **map 键**（不是 serde 字段）  
- `opened_tab` 行（无 JSON 列）

协作 **线** DTO 是绿，但 **信封 JSON** 是红——不要因为 `CollaborationDelivery` 不落盘就改信封键。

---

## 6. 结论：切面 ⑤ 施工建议

### 6.1 不要做一次全局改名

现状不是「全仓库一半 camel 一半 snake」，而是 **三层叠在一起**：

| 层 | 主导约定 | 证据 |
|---|---|---|
| 持久化的 codeg 私有对象 | **snake_case 字段** | 任务/自动化/系统设置/GitHub 账号/LogSettings/CustomAgentSpec/preferences/experts manifest/转录 header |
| 持久化的少数新对象 | **camelCase** | `PromptQueueDraft.displayText`、`PetWindowConfig`、备份 manifest、`pet.json`、协作信封键 |
| 线（HTTP 壳、协作/队列/宠物 DTO） | **camelCase 在扩张** | handlers 170+ camel；`collaboration.rs` 29 处；`types.ts` 协作/队列族 |
| 线（会话/文件夹/任务/自动化 Info、ACP 事件） | **snake_case** | `types.ts` 明确写 WorkTask/Automation wire 是 snake；`AcpEvent` snake tag |

`types.ts` 是镜像，不是第三套。统一「以 TS 为准」会得出互相打架的答案，必须按 **落盘 vs 只走线** 切开。

### 6.2 目标约定（建议）

**不把已落盘的 snake 对象改成 camel。** 那是迁移工程，不是重命名。

建议写成三条硬规则，写进切面 ⑤ 的施工 RFC：

1. **已落盘 JSON（§5 红项）冻结当前 casing。** 新字段用 `#[serde(default)]` 加在现有约定上（任务/自动化已经这么做了）。需要双读的只有「同一结构体以后若被误加 `rename_all`」——用 CI/测试钉死样本 JSON，而不是现在就双读。
2. **新的纯线 DTO**（HTTP/WS、不进 SQLite/文件）：跟 handlers 走 **camelCase**，与协作/队列族对齐。禁止再新增无 `rename_all` 的 Info 结构体却被前端当 camel 用。
3. **第三方形状冻结**：`pet.json`、agent `mcpServers`/local config、`CustomAgentSpec`、`provider_payload`、`fork_relation.anchor`、`AgentType` wire 字符串。切面 ⑤ 名单里直接划掉。

**不要**把会话/任务/自动化 Info 改成 camelCase 当作「统一」。那会：

- 逼 `types.ts` 最大的那一族整表改名；  
- 打开 server 升级窗口（§3）；  
- **更糟**：`WorkTaskDraft.config` / `AutomationDraft.config` 是 `Value` **原样入库**（`to_string(&draft.config)`），线字段名 **就是** 库里的字段名。改线 = 改盘。

### 6.3 红项迁移策略

| 项 | 策略 |
|---|---|
| 任务/自动化/settings/template config、merge/preflight | **冻结不动**。新字段只加 snake + `default`。禁止给这些结构体加 `rename_all = "camelCase"` |
| `PromptQueueDraft` | **冻结混用**（外 camel 内 snake）。若将来外层也改 snake，必须：`alias = "displayText"` 双读 + 写回时选一种 + 一次性 `UPDATE` 改 `draft_json`。不值得为切面 ⑤ 做 |
| 协作信封键 | **冻结**。要改就 bump `ENVELOPE_VERSION` 并双读 V1。与 serde rename_all 无关 |
| `PetWindowConfig` / 备份 manifest | 已是 camel。保持。备份再加字段用 `default` + format_version |
| `pet.json` / MCP / agent config / CustomAgentSpec | **冻结不动** |
| ACP 转录 JSONL | **冻结**。已有 `v` 字段；不兼容才 bump，并让阅读器跳过未知 version（代码已如此设计，`acp_transcript.rs:72-73`） |
| `app_metadata` 系统/日志/GitHub | 冻结 snake。新 key 新对象，不要改旧对象字段名 |
| `work_task_event.payload` | 冻结键名。新 kind 用新键，旧 kind 别改 |

**一次性迁移脚本**：目前 **没有** 值得写的。红项要么该冻，要么是第三方。双读只在有人已经把两种 casing 写进同一列时才需要——本次 **没有扫到混写实证**（任务测试明确用 snake 样本；队列测试路径走 `PromptQueueDraft` serde，应只有 camel 外层）。

### 6.4 建议施工顺序

1. **把本清单里的红项标成「禁止 `rename_all` / 禁止改字段名」**（CI：对 `WorkTaskConfig`、`AutomationConfig`、`PromptQueueDraft`、`TranscriptHeader`、`PetManifest`、`BackupManifest`、`PetWindowConfig`、`System*Settings`、`CustomAgentSpec` 做 round-trip 样本测试——任务 settings 已有先例）。这是切面 ⑤ 真正的前置闭合。  
2. **新代码规范**：纯线 DTO camelCase；落盘对象沿用该文件现有约定；`serde_json::Value` 入库的 draft.config **视为落盘**。  
3. **不要**在切面 ⑤ 把 `conversation.rs`/`work_task.rs` Info 改成 camel 去「对齐」协作。那是 API 破坏，且会误伤 SQLite。  
4. **可选、独立、低优先级**：给 server 模式的线 DTO 加 `deny_unknown_fields` 的反面——即文档化「server 升级窗口内禁止改已发出字段名」。桌面同版本可忽略。  
5. 切面 ① 的 Room 时间线若将来落 JSON，**先选定约定再加列**，不要再开第三种。

### 6.5 未能确定

- `chat_channel.event_filter_json`（通道列）与 `app_metadata.chat_event_filter` 是否同一形状。  
- `work_task_event.payload` 全 kind 的完整键集合（只抽了 `user_action`/`status_changed`/引擎读取点）。  
- `workbench://place-session` Rust 发射体与 `types.ts` 是否逐字段同构。  
- TurnEnd 转录 `p` 里 `stopReason` 是否由 serde 结构体生成还是手写 Value（注释是 camelCase；Header 外层是 snake）。若切面 ⑤ 动转录，需要先读 `TurnEnd` 的序列化点。

以上四条都不阻碍「冻结红项 + 新 DTO 用 camel」这条施工建议。
