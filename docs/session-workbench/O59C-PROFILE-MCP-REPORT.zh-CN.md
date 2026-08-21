# O59-C 施工报告 — 把启动配置档暴露给 agent（codeg-mcp 工具面）

日期：2026-08-21  
工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/profile-mcp`（分支 `wt/profile-mcp`）  
范围：**只做 Rust 后端**（`src-tauri/`）。未改 `src/`、未改 i18n、未改 schema / 迁移 / DB 列、未 push。

## 1. `model` 键核实（先核实，不猜）

任务书说 `engine.rs:944` 读 `config_values.get("model")`，要先确认写入方与消费方。

**消费方（同一条 preferred-config 通路）：**

- `src-tauri/src/work_task/engine.rs:944` — `config_effective` 审计事件读 `config_values.get("model")`。
- 同文件 `:1000` — `effective_agent_config` 得到的 `config_values` 原样传给 `spawn_agent(..., preferred_config_values)`。
- `src-tauri/src/acp/manager.rs:570` — `spawn_agent` 参数就是 `preferred_config_values: BTreeMap<String, String>`。
- `src-tauri/src/acp/connection.rs` — `"model"` 是普通 ACP config option（`category: "model"`），会从 `preferred_config_values` 下发。`is_codeg_internal_pin("model")` 为 false（与 `__codeg_profile__` 相对）。

**写入方（既有约定，不是新键）：** 前端 / 任务编辑器把选中的模型写进 `WorkTaskConfig.config_values["model"]`。Host Control 的语义钉是另一把键 `__codeg_host_model__`，本任务不碰。

核实结果与任务书一致。MCP 层写入的就是 `"model"`。常量 `LAUNCH_MODEL_CONFIG_KEY` 钉在 `commands/claude_profile.rs`，避免 MCP 层再发明一个键。

## 2. Automation 通路核实

`create_automation` 的 `action=launch_session` **与 work task 共用同一条 `config_values` 通路**，没有第二条：

- 落库：`AutomationConfig.config_values`（`models/automation.rs:103`）。
- 开火：`automation/engine.rs:597` `cfg.config_values.clone()` → `spawn_agent` 的 `preferred_config_values`。
- `enqueue_task`：`automation/engine.rs:399` 把同一份 `config_values` 拷进 `WorkTaskConfig`，再走 work-task 引擎。

因此两个可选参数写进 `NewAutomationSpec` → `AutomationConfig.config_values` 即可。未发明第二条通路。

## 3. 做了什么

### 3.1 `list_profiles`（只读）

- 注册/分发照抄 `list_rooms` / `list_sessions`：`tool_schema.json` → `CompanionFeatures::allows_tool` → companion 解析 → `BrokerMessage::ListProfiles` 往返 → listener → `ChatAuthoringAccess::list_profiles`。
- 列举只调 `claude_profile_list_core`（单一事实源）。每条返回 `id` / `label` / `kind` / `destination`。
  - `configDir` → 目录路径
  - `managed` → `baseUrl`
  - `follow-default` → 说明它是默认（不设 `CLAUDE_CONFIG_DIR`）
- **不返回明文 token，也不返回掩码字段。**
- 工具英文描述含任务书指定句："A profile decides which Claude configuration (subscription login vs API endpoint) a session launches with"。
- 可选 `agent_type`，默认 `claude_code`；其它 agent 返回空列表 + 说明档目前只支持 Claude Code。
- 功能开关：`automations || taskboard`（没有新 `--features` token）。能建任务/自动化的 companion 才能看到档列表。

### 3.2 `create_work_task` / `create_automation` 可选 `profile` + `model`

- 参数解析用既有 `optional_string`。两者都省略 → `config_values` 仍是空 `BTreeMap`（与今天 JSON `{}` 逐字节一致）。
- `profile`：必须存在于 `claude_profile_list_core` 的列表；非法值 **soft reject**（`created: false` + 错误信息列出合法 id），**不写任何行 / 任何 config_values**。禁止静默回落默认档。
- `model`：写入既有 `"model"` 键。
- 档只支持 Claude Code：
  - automation：创建时已解析出具体 agent（文件夹默认或调用者），非 `claude_code` 直接报错。
  - work task：显式 `agent_type` 且不是 Claude Code → 报错。

### 3.3 work-task inherit 与静默回落（取舍，见第 5 节）

`effective_agent_config`（`work_task/engine.rs:3331`）在任务 **没有** `agent_type` 时，整表改用文件夹 `settings.config_values`，任务自己的 `config_values` 不会进 `spawn_agent`。

若只把 `__codeg_profile__` 写进任务、却把 `agent_type` 留空，档会存在库里、开火时被丢掉——这就是任务书禁止的「用户以为走 API 其实烧订阅」。

因此：`profile` 有值且 `agent_type` 省略时，落库把 `agent_type` 钉成 `claude_code`，让既有「任务覆盖整表生效」的分层把档真正带上。`profile` 与 `model` 都省略时仍是今天的 inherit，回归测试钉住空 `config_values`。

## 4. 明确不做

- schema / 迁移 / 新 DB 列
- `src/` 与 i18n
- 前端
- Codex / 其它 agent 的档
- 档的存储格式与既有 CRUD 命令

## 5. 取舍（写进报告，没有另开通路）

1. **非法 profile 是 authoring 的 soft reject，不是 companion `-32602`。** companion 进程没有 `data_dir`，存在性校验必须在 host 调 `claude_profile_list_core`。这与未知 `agent_type` / 坏 cron 的既有写法一致：LLM 读 `note`，库里什么都不写。
2. **work-task 最终 agent：** 创建时要得到与开火相同的 agent，需要 `settings_get_effective` + 把 `effective_agent_config` 公开。未把引擎的私有分层拷一份到 MCP。退一步：显式非 Claude 报错；`profile` 省略 agent 时钉 `claude_code`（上一节）。
3. **只传 `model`、不传 `profile`、且 inherit agent：** 仍可能被文件夹 `config_values` 整表盖掉。未为「仅模型」去钉 agent（模型不是 Claude 专属）。要模型在 inherit 任务上生效，调用方应同时传 `agent_type`。

## 6. 测试（真实输出）

工作树 `src-tauri/`，L1：每步自己的退出码；L5：`--no-default-features --features test-utils --lib`。

`profile` 过滤器（一次跑里含本任务新增 + O59-A 档测试）：

```
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 2668 filtered out
TEST-EXIT-profile:0
```

本任务新增 / 回归，均在上述或后续过滤器里 `ok`：

| 测试 | 钉住的行为 |
| --- | --- |
| `omitted_profile_and_model_yield_empty_config_values` | 不传 → 空 map，JSON `{}` |
| `create_work_task_resolves_a_worktree_to_its_project` | 回归：不传时 `config_values` 为 `{}` |
| `create_automation_persists_a_fireable_row` | 回归：不传时 `config_values` 为 `{}` |
| `valid_profile_writes_preferred_profile_key` | `__codeg_profile__=<id>` |
| `work_task_profile_and_model_land_in_config_values` | 档键 + `"model"` 键落库 |
| `automation_launch_session_profile_lands_in_config_values` | automation 同一对键 |
| `unknown_profile_errors_with_valid_ids_and_writes_nothing` | 错误含合法 id |
| `unknown_work_task_profile_is_rejected_and_writes_nothing` | 不写任务行 |
| `unknown_automation_profile_is_rejected_and_writes_nothing` | 不写 automation 行 |
| `model_writes_the_existing_model_key` | `"model"` |
| `mcp_profile_list_omits_plain_and_masked_tokens` | 无 token / 无掩码字段 |
| `list_profiles_returns_ids_without_tokens` | 同上（host 列举） |
| `render_profile_list_omits_tokens` | companion 渲染 |
| `work_task_profile_without_agent_type_pins_claude_code` | inherit + profile → 钉 claude_code |
| `work_task_profile_rejected_for_explicit_non_claude_agent` | 显式 codex + profile → 报错 |
| `parse_work_task_omits_profile_and_model_by_default` | 解析回归 |
| `parse_work_task_reads_optional_profile_and_model` | 解析 |
| `parse_automation_reads_optional_profile_and_model` | 解析 |
| `list_profiles_schema_describes_subscription_vs_api` | 工具描述含指定句 |
| `tools_list_contains_only_shared_host_bridge_tools` | 清单含 `list_profiles` |

其它过滤器：`create_work_task` 2 passed；`create_automation` 2 passed；`parse_work_task` 2 passed；`parse_automation` 1 passed；`tools_list` 3 passed；`list_profiles` 3 passed。全部 `TEST-EXIT-*:0`。

过滤器 `launch_config` 是 `0 passed; 2697 filtered out`（名字没对上函数 `launch_config_values`）。**不把它算作跑过。** 对应行为由 `omitted_profile_and_model_yield_empty_config_values` 覆盖。

## 7. 验证命令（L1：不靠管道判定退出码）

在 `src-tauri/` 下，`CARGO_TARGET_DIR=target`：

| 步骤 | 退出码 |
| --- | --- |
| `cargo fmt --check` | `FMT-EXIT:0` |
| `cargo clippy --all-targets --features test-utils -- -D warnings` | 第一次因缺 `out/`（`tauri.conf.json` `frontendDist: "../out"`）build.rs 失败 `CLIPPY-EXIT:101`。按 O59-A 同样建了 gitignored 占位 `out/index.html` 后重跑 **`CLIPPY-EXIT:0`**，日志 `Finished dev profile ... in 1m 46s` |
| `cargo test --no-default-features --features test-utils --lib <filter>` | 上表全部 0（`launch_config` 除外，0 passed 已标明） |
| `cargo check --no-default-features --bin codeg-mcp` | `MCP-EXIT:0`（`Finished ... in 1m 07s`） |
| `cargo check --no-default-features --bin codeg-server` | `SERVER-EXIT:0`（`Finished ... in 1.05s`） |

未在 worktree 跑桌面 `cargo test --features test-utils`（L5：exe 可能 0xc0000139）。权威门禁由领导在主仓跑。

## 8. 改动文件

- `src-tauri/src/acp/chat_authoring.rs` — spec 字段、`ProfileListOutcome`、trait
- `src-tauri/src/acp/delegation/{companion.rs,listener.rs,transport.rs,tool_schema.json}`
- `src-tauri/src/commands/{claude_profile.rs,chat_authoring.rs}`
- `src-tauri/src/lib.rs`、`src-tauri/src/bin/codeg_server.rs` — `DbChatAuthoring` 传入 `data_dir`
- `src-tauri/tests/host_bridge_e2e.rs` — stub `list_profiles`
