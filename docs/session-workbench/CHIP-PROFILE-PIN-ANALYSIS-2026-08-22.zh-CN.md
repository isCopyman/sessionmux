# Chip 显示与 conversation pin 缺口 —— 分析（2026-08-22）

> 只读分析产出，作为实施的事实源。每条现状断言都带 file:line 锚点。
> 由 codex 在主树只读模式生成，主会话核对后落库。

结论：这是两个独立问题，但 (b) 的故障描述要收窄。缺陷 (a) 是新标签页显示值与后端启动解析不一致；缺陷 (b) 不是“本次 respawn 立即丢掉 mode/effort/config”，这些值会经 `localStorage` 重放，而是“用户在建行前做的显式选择没有写成该 conversation 的 pin”，以后换客户端或 agent 级默认变化时会漂移。最小且诚实的修法是：由 `ConversationTabView` 把 Claude agent 默认 profile 逐层传给 chip，并在 render 中派生显示值；首次创建 conversation 时把本标签页显式改过的 profile、mode 和 config values 一起写入新行，然后保留现有 respawn。不要改写连接生命周期。

### 1. 两个问题是否独立

独立，互不包含。

- (a) 当前 state 初值是 `follow-default`，无 conversation 时又不查询实际值，因此新页签必然先显示“跟随默认”：`src/components/chat/claude-profile-selector.tsx:59`、`:90`。实际启动解析顺序却是 conversation pin → agent `CODEG_CLAUDE_PROFILE` → `follow-default`：`src-tauri/src/commands/claude_profile.rs:685`、`:695`、`:708`、`:712`。
- profile 的首次发送路径已经把选择写入 conversation，再断开并重连：`src/components/chat/apply-pending-claude-profile.ts:23`、`:24`、`:28`、`:29`。修好这条路径不会纠正首次显示；反过来，修正显示也不会持久化 mode/config。
- `list[0]` 确实参与问题：后端列表固定把 `follow-default` 放在第一项：`src-tauri/src/commands/claude_profile.rs:834`、`:837`。当前“找不到就选 `list[0]`”因此总是退回“跟随默认”：`src/components/chat/claude-profile-selector.tsx:68`。应删除这次 state snap，改成 render 时校验候选 id 是否存在并派生 fallback。

### 2. (a) 的值应从哪里来

所有生产挂载路径的共同 owner 应是 `ConversationTabView`。它已经订阅 `useAcpAgents`：`src/components/conversations/conversation-detail-panel.tsx:601`；agent DTO 已包含 `env`：`src/lib/types.ts:2885`、`:2917`，而后端正是从 `agent_setting.env_json` 解析并返回该 map：`src-tauri/src/commands/acp.rs:10302`、`:10414`、`:10432`。

建议 prop 路径：

- `ConversationTabView`
- → `ConversationShell` → `ChatInput` → `MessageInput`
- → `InlineClaudeProfileSelector`

以及欢迎页的直达路径：

- `ConversationTabView`
- → `ChatInput` → `MessageInput`
- → `InlineClaudeProfileSelector`

两处生产挂载分别在 `src/components/conversations/conversation-detail-panel.tsx:2123`、`:2301`；唯一 chip 挂载点在 `src/components/chat/message-input.tsx:1441`。

传入 `agentDefaultProfileId`，chip 按以下优先级在 render 中派生：用户 pending choice → agent default → `follow-default`，并用已加载的 profile catalog 把不存在的 id 归一为 `follow-default`。这样不需要 effect 内同步 `setState`，也不需要 chip 导入 `useAcpAgents`。

确有暂时“无人知道”的窗口：agent store 初始是空数组且 `fresh=false`，首次加载失败还会保留该状态：`src/hooks/use-acp-agents.ts:46`、`:47`、`:48`、`:74`。此时应显示禁用的 loading/未知态，不能提前声称“跟随默认”。

### 3. (b) 实际发生了什么，应怎样修

你的“立即 respawn 会丢”假设应当否定，但存在持久化缺口：

- mode：选择时写 React state 和 per-agent `localStorage`：`src/components/conversations/conversation-detail-panel.tsx:1669`、`:1671`、`:1676`。重连会读取该偏好并传给 `acp_connect`：`src/contexts/acp-connections-context.tsx:5110`、`:5122`、`:5127`；后端在 selector ready 前应用：`src-tauri/src/acp/connection.rs:6203`、`:6210`。所以本次 respawn 保留 mode。
- effort 与任意原生 configId/valueId：选择时先更新 UI、保存 `localStorage`，再调用后端：`src/contexts/acp-connections-context.tsx:5674`、`:5678`、`:5684`、`:5688`。重连会逐项应用普通 config：`src-tauri/src/acp/connection.rs:6222`、`:6256`；Grok effort 也有专门重放：`src-tauri/src/acp/connection.rs:2750`、`:2811`、`:2824`。
- 但建行前的后端持久化确实直接返回：mode 在 `src-tauri/src/commands/acp.rs:9661`、`:9665`，config 在 `:9688`、`:9692`。新行本身又把两个字段初始化为 `None`：`src-tauri/src/db/service/conversation_service.rs:116`、`:117`。重连应用偏好不会反写 DB，因此 conversation 没有获得 pin。

正确的小修是“创建时持久化”，不是取消 respawn：

- `ConversationTabView` 只记录本标签页用户显式触碰过的 pending mode/config；不要把整份 per-agent `localStorage` 模板全部钉进 conversation，否则用户没碰过的默认也会被冻结。当前合并契约本来就是 conversation 逐键覆盖模板：`src-tauri/src/commands/acp.rs:9621`、`:9637`、`:9640`。
- 扩展普通与 chat 两个创建请求，让共享 Rust create core 在插入行时直接写 `preferred_mode_id` 和 `preferred_config_values`，其中 profile 使用现有 `__codeg_profile__` 键。现有两种前端创建请求都没有这些字段：`src/lib/api.ts:3363`、`:3381`。
- profile 是启动期配置，后端只在构建新进程环境时解析和应用：`src-tauri/src/commands/claude_profile.rs:746`、`:760`、`:767`。不 respawn 就要把 pending profile 提前接入初次 `acp_connect`，或延迟自动连接，改动更大；简单复用旧进程则是不诚实的。

这项修改必须同时覆盖 desktop 的 Tauri command 与 server 的 Axum DTO/handler；两边目前分别走 `src-tauri/src/commands/conversations.rs:2125`、`:2378` 和 `src-tauri/src/web/handlers/conversations.rs:302`、`:326`，共享 core 在 `src-tauri/src/commands/conversations.rs:2076`、`:2330`。无需新依赖。

### 4. 是否还有第三种情况

有，最危险的是“选中的 profile 在首次发送前被删除”。

- chip 只在挂载时取列表，并只在 `conversationId` 改变时重读绑定：`src/components/chat/claude-profile-selector.tsx:62`、`:82`、`:90`、`:102`；删除操作则直接删记录和目录：`src-tauri/src/commands/claude_profile.rs:1337`、`:1343`、`:1345`。已挂载 chip 会继续显示旧 profile。
- 首次发送目前先创建行，再写 profile：`src/components/conversations/conversation-detail-panel.tsx:1317`、`:1322`、`:1323`。已删除 profile 会在 setter 校验时失败：`src-tauri/src/commands/claude_profile.rs:1374`、`:1380`。
- catch 恢复草稿但没有清除已写入的 `dbConvIdRef`：`src/components/conversations/conversation-detail-panel.tsx:1360`、`:1384`。下次发送会命中“已有 id”分支并直接发送：`:1215`、`:1220`，从而可能绕过 pending profile，重新形成“chip 与进程不一致”。

让创建 core 在插入前验证 pending profile，并在同一次创建中写入全部 pins，可以同时关闭这条路径。已存在但绑定 profile 缺失的 conversation 在重新挂载时反而处理正确：后端明确回退 `follow-default`：`src-tauri/src/commands/claude_profile.rs:668`、`:673`，getter 又复用同一解析器：`:1293`、`:1312`。

### 5. 最小测试集

1. `src/components/chat/claude-profile-selector.test.tsx`：新页签、无 pending、agent default=`api` 时显示“中转”；再覆盖 default prop 延迟到达，证明值来自 render 派生而非 effect 同步 state。当前测试仍明确期待新页签显示 Follow default：`:148`、`:156`。

2. 同文件：agent default 指向 catalog 中不存在的 id 时显示 `follow-default`；防止保留 `list[0]` state snap 或直接显示裸 id。

3. `src/contexts/acp-connections-context.test.tsx`：让 `getSavedPrefsForConnect` 返回非空 mode、effort 和一个普通 config，断言 reconnect 的 `acpConnect` 收到全部值。现有 respawn 测试只覆盖 `undefined`/空 map：`:739`、`:743`、`:744`。

4. Rust conversation 创建测试：普通与 chat 创建都应在新行中一次写入 pending mode/config/profile；不存在的 profile 必须在插入前失败且不留下 conversation。放在现有 `src-tauri/src/commands/conversations.rs` 的 create 测试组附近；DB 断言复用 `selector_prefs`：`src-tauri/src/db/service/conversation_service.rs:1205`、`:1218`。

5. 扩展 `apply-pending-claude-profile.test.ts`：确认创建完成后仍保持“已持久化 pins → disconnect → connect → send”的顺序。现有测试只钉住 profile write → disconnect → connect：`src/components/chat/apply-pending-claude-profile.test.ts:39`、`:65`。
