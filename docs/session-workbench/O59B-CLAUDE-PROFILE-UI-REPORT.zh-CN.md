# O59-B 前端施工报告 — Claude 启动配置档 UI

日期：2026-08-21  
工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/claude-profile-ui`（分支 `wt/claude-profile-ui`）  
范围：**只做前端**（`src/` 与 `src/i18n/`）。未改 `src-tauri/`、未改 schema、未 push、未动 main。

## 1. 做了什么

### 传输层
- `src/lib/types.ts`：`ClaudeProfileInfo` / `ClaudeProfileUpsert` / `ConversationClaudeProfileResult`（camelCase），常量 `FOLLOW_DEFAULT_CLAUDE_PROFILE_ID`、`CODEG_CLAUDE_PROFILE_ENV_KEY`，以及 `isValidClaudeProfileId`。
- `src/lib/api.ts`：`claudeProfileList` / `claudeProfileUpsert` / `claudeProfileDelete` / `conversationSetClaudeProfile`，写法与既有 `getTransport().call(...)` 相同。

### Chip（主戏）
- 新组件 `src/components/chat/claude-profile-selector.tsx`，视觉照抄 `InlineSessionConfigSelector`（`Button variant="ghost" size="xs"`、`ChevronDown`、`max-w-[10rem] truncate`）。
- 宿主：`message-input.tsx` 的 `inlineSelectorItems`。**仅 `agentType === "claude_code"` 时渲染**。
- Chip **只显示档名**（跟随默认走 i18n，其它走 `label`）。`aria-label` 仍是 `控件名: 档名`（与刚落地的 session-config chip 一致）；可见文本不含 `名字: `。
- 下拉：第一行 `DropdownMenuLabel` =「接入配置」→ separator → `RadioGroup`（不重排，`follow-default` 保持后端给的第一项）→ separator →「管理接入配置…」走既有 `openSettingsWindow("agents", { agentType: "claude_code" })`。
- 选档：`conversationSetClaudeProfile`；成功/失败 toast。**不**再画「下一回合生效」横幅（后端 stale + 既有 `SessionConfigStaleBanner`）。
- 回合进行中：`disabled={isPrompting || sourceConversationId == null}`。`isPrompting` 是 message-input 已有的“正在跑”判定（`disabled` 在 prompting 时反而是 false，因为编辑器要保持可输入）。

### 设置页
- 新组件 `src/components/settings/claude-profile-catalog.tsx`，挂在 `acp-agent-settings.tsx` 的 Claude Code 配置区（现有三态认证之上）。
- 列表：`label`、`kind` 徽标、`configDir` 或 `baseUrl`、`authTokenMasked`。无明文 token。
- Dialog 新建/编辑；`follow-default` 不可编辑、不可删除。删除走既有 `AlertDialog` 二次确认。
- `authToken` 留空 = 不发送该字段（契约：省略=保留）。placeholder 说明这一点。
- 「设为该 agent 默认」：`patchEnvText` + 既有 `persistEnv` → `acp_update_agent_env`，写入 `CODEG_CLAUDE_PROFILE`。

### i18n
全部新键放在 **`AcpAgentSettings.claudeProfile`**（chip 与设置页共用这一处命名空间）。十语对等：en / zh-CN / zh-TW / ja / ko / es / de / fr / pt / ar。

`idInvalid` 里的 `{1,64}` 用 ICU 转义 `'{'1,64'}'`，否则 next-intl 会当成占位符（施工时测试里炸过 `INVALID_ARGUMENT_TYPE`）。

## 2. 契约 vs 代码（以代码为准）

对照 `CLAUDE-PROFILE-BACKEND-REPORT.md` §7、`models/claude_profile.rs`、`commands/claude_profile.rs`：

| 点 | 报告 §7 | 代码 | 前端怎么做 |
| --- | --- | --- | --- |
| DTO 字段 | camelCase；只有 `authTokenMasked` | 一致（`rename_all = "camelCase"`；Info 无 `authToken`） | 按代码 |
| `claude_profile_list` | 无参数；第一项永远 `follow-default` | 一致 | 不排序 |
| `claude_profile_upsert` | 「单对象 `payload` / JSON body」 | **桌面** Tauri 参数名是 `payload`；**服务器** handler 把 JSON body 直接反序列成 `ClaudeProfileUpsert`（不包一层 `payload`） | 按契约/Tauri 发 `{ payload }`。服务器模式这条会对不上。见下 |
| `claude_profile_delete` | `{ id }` | Tauri `id: String`；web `{ id }` | `{ id }` |
| `conversation_set_claude_profile` | `{ conversationId, profileId }` | Tauri 参数 + web params 均为 camelCase | 照抄 |
| 当前绑定 | 无 GET | 绑定在 `preferred_config_values["__codeg_profile__"]`，会话 summary **不下发**该字段 | chip 初次显示 `follow-default`，选档后乐观更新。刷新后 chip 会回到跟随默认，**真正 spawn 仍读后端绑定**。四条 API 里没有读当前档的命令，未发明第五条 |
| `follow-default` 的 `label` | 未规定前端文案 | 后端写死英文 `"Follow default"` | 用 i18n `followDefault` 覆盖显示 |

**upsert 双模式形状不一致**是后端既成事实，本任务不能改 `src-tauri/`。前端按任务书「命令名与参数形状照抄契约」走 `{ payload }`（与 Tauri `claude_profile_upsert(..., payload)` 对齐）。Web `POST /claude_profile_upsert` 目前吃的是未包装的 upsert 对象；服务器部署下这条会 400，除非后续后端把 web handler 改成 `{ payload }` 或前端再按 `isDesktop()` 分叉（本次未分叉）。

## 3. 明确没做（按任务书）

- 不动 `src-tauri/`、schema
- 连通性探活
- Codex / 其它 agent 的档 UI
- MCP 工具面（O59-C）
- 不扫描 `~/.claude`
- 不显示明文 token
- 不自己实现 stale 横幅
- 未把档编进窄屏齿轮面板（`collapsedSettings`）。chip 在 `inlineSelectorItems` 里，随其它 chip 在 `<30rem` 时折进齿轮；齿轮内容仍只有 mode/config。Claude 会话连上后通常已有 model 等 config chip，欢迎页 composer 较宽。若需要窄屏也可切档，应另开任务把档加进 `collapsedSettings`。

## 4. 验证（L1：不靠管道判定退出码）

工作树：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/claude-profile-ui`。PowerShell 用 `> log 2>&1; Write-Output "…-EXIT:$LASTEXITCODE"`，每步看命令自己的 `$LASTEXITCODE` 和日志正文。

| 步骤 | 退出码 | 日志正文 |
| --- | --- | --- |
| `pnpm tsc --noEmit` | `TSC-EXIT:0` | 空日志（无诊断） |
| `pnpm eslint src` | `ESLINT-EXIT:0` | 1 warning，**既有**、非本任务：`src/components/message/message-list-view.tsx:261` `'outbound' is assigned a value but never used` |
| `pnpm vitest run src/components/chat src/components/settings src/i18n` | `VITEST-EXIT:0` | `Test Files 67 passed (67)` / `Tests 882 passed (882)` |

本任务新增/覆盖的测试（均含在上面 882 里）：

| 要求 | 测试 |
| --- | --- |
| chip 只对 Claude 渲染 | `MessageInput Claude launch profile chip > renders the chip only for Claude Code` |
| chip 文本是裸档名（不含 `: `） | `InlineClaudeProfileSelector > keeps the chip bare…`（`textContent` 不含 `": "`） |
| 下拉第一行是控件名 | 同上，menu 内 `Launch profile` |
| 选档调用 `conversationSetClaudeProfile` | `InlineClaudeProfileSelector > calls conversationSetClaudeProfile when a profile is chosen`（`(12, "api")`） |
| 回合进行中 chip 禁用 | `MessageInput … > disables the chip while the turn is running`（`isPrompting: true`）以及 selector 的 `disabled: true` |
| 设置页 CRUD 乐观刷新 | `ClaudeProfileCatalog > optimistically inserts…`（upsert 后出现「中转」且 `list` 仍只调一次）；`removes a deleted profile…` |
| id 校验拒绝非法值与保留字 | `isValidClaudeProfileId` 单测 + catalog 表单拒绝 `Bad Id` / `follow-default` |

**没有**在浏览器里点过真实桌面/服务器 UI：本 worktree 任务用 vitest + tsc + eslint 核验。未声称“实机走查通过”。

`pnpm install --frozen-lockfile`：本机 `node_modules` 当时不存在，安装步骤 `INSTALL-EXIT:0`（该步 stdout 进了 `.tmp-install.log`，判定用的是该命令自己的 `$LASTEXITCODE`，不是管道下游）。

## 5. 未发明的规格缝

- 无 GET 当前会话档：chip 冷启动显示跟随默认。若产品要刷新后仍显示已绑定档名，需要后端给读接口或把 `__codeg_profile__` 放进会话 summary。
- upsert web/Tauri 形状不一致：见 §2。未猜着修后端。
