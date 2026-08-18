# Fork / Rewind / 按消息位置 Fork：能力矩阵与可行路径调研

> 状态：2026-08-19 凌晨完成的只读调研报告（G10，explore-k3 执行，编排会话验收入库）。
> 本文是**调研事实与建议**，不是拍板稿；实施前需按 §6 建议逐条决策，并先完成
> [SESSION-HISTORY-CAPABILITIES-RFC](./SESSION-HISTORY-CAPABILITIES-RFC.zh-CN.md) 的对账修订（见 §1 脱节清单）。

结论先行：codeg 现在只有一条活体 fork 管线——ACP `session/fork` 的"整会话 head fork"，且 **RFC（2026-08-15）与代码已在 5 处脱节**（fork 行布局在 RFC 写完次日被 0bfb86a0 反转）。Rewind/checkpoint 代码里**完全不存在**（grep rewind 只有两处无关注释）。按消息 fork 最现实的路径是：claude 走文件手术或给 claude-agent-acp 加 `_meta` 扩展，codex 走 app-server `thread/fork`+`thread/rollback`（codex-acp 1.4.0 内部已在用 thread/fork，等于官方示范），其余家降级为重放。

## 1. RFC 已设计什么、哪里脱节

RFC 已设计（仍有效）：三种能力拆分 forkHead/forkAtMessage+editAndFork/restoreFiles（§2 表）；能力层 + 多 provider 架构（§6，ClaudeNative/CodexAppServer/Acp/StableCli/VersionedJsonl 五个 provider）；HistoryAnchor 类型；谱系独立表不复用 parent_id（§9）；JSONL 兼容器 8 条军规（§8，只读原件、克隆新 id、resume 验证、experimental 标记）；UI 诚实呈现能力差异（§10）；4 阶段实施（§11）。

脱节点（按严重度）：

1. **fork 行布局反转**。RFC §3（`docs/session-workbench/SESSION-HISTORY-CAPABILITIES-RFC.zh-CN.md:58`）写"当前会话行切换到新原生 Session，另建 sibling 行保留原历史"。0bfb86a0（2026-08-16，RFC 写完次日）已反转：现在 C1/S1 **整行不动**，新 INSERT C2 绑 S2，活跃连接迁到 C2（`src-tauri/src/acp/manager.rs:1959-1962` 注释 "leave C1/S1 untouched and INSERT C2"，2077-2106）。旧行为可从 `git show 0bfb86a0^:src-tauri/src/acp/manager.rs` 的 persist_fork_outcome 看到（UPDATE 当前行→S2、INSERT sibling 留 S1）。"sibling" 一词在 RFC 里的指向已反。
2. **acpFork 签名已扩展**。RFC §3 只说前端调用；现 `src/lib/api.ts:348-372` 带 `conversationId/folderId`（为"fork 早于首条 prompt 的未链接会话"补的 adopt 路径），后端 `commands/acp.rs:9816-9826`、`web/handlers/acp.rs:331-336` 对应。注意 `src/lib/tauri.ts:160` 仍只传 connectionId——桌面端 Option 缺省 None 能跑，但吃不到 adopt 修复，**两端封装不一致**。
3. **`[Fork]` 前缀从前端移到后端**：现在由 persist_fork_outcome 打在新行 C2 上并 `title_locked=true`（`manager.rs:2051-2059, 2080-2081`），前端正则已删。
4. **版本快照过时**：RFC 基于 0.25.0/b6e1d904、claude-acp 0.67.0/codex-acp 1.2.0；当前 codeg 0.26.1，钉 claude-acp **0.69.0**、codex-acp **1.4.0**（`registry.rs:455-457, 589-591`）。§4 的 capability 快照需重测。
5. **新增机制 RFC 未记**：67974819（08-18）让 fork 继承源会话 pin 的 model/thinking effort/mode（`manager.rs:2100-2104`，`preferred_mode_id`/`preferred_config_values` 继承）；`background_watch.rs:1256-1285` 记录了 claude fork 的**文件层布局**（fork = 复制父 transcript 到新 session 文件、记录保留原始时间戳、fork 时刻在文件头写 queue-operation 元数据）——这个事实是文件手术路的重要正面证据，RFC 不知道。

仍属实未脱节：`conversation.parent_id` 仍专用 delegation，fork 显式 `parent_id: Set(None)`（`manager.rs:2088`，`entities/conversation.rs:58`）；无通用谱系字段。

## 2. 现有 fork 管线（代码证实）

链路：`message-input.tsx:1633`（composer 下拉"分叉发送"，唯一入口）→ `conversation-detail-panel.tsx:1369 handleForkSend`（门控 `conn.supportsFork`，2080）→ `api.ts:348 acpFork` → `commands/acp.rs:9816` / `web/handlers/acp.rs:459` → `manager.rs:1742 fork_session`（prompt_lock 串行化、turn_in_flight 闸门、取消屏蔽 tokio::spawn）→ `connection.rs:8225` ConnectionCommand::Fork → `acp/fork.rs:19` 发 `session/fork`（UntypedMessage，sacp 11.0.0 无 typed 封装）→ `connection.rs:6591 handle_fork_or_exit`：用 ForkSessionResponse **直接 attach S2，不走 session/load 往返**，可递归嵌套 fork → `manager.rs:1993 persist_fork_outcome` 事务内写 C2（继承 folder/kind/model/git_branch/origin_cwd/pin 值 + collection 成员）→ `ConversationForked` 事件 → `session_state.rs:1009-1016` 连接重绑 C2。

fork 的对象：**当前完整会话（head fork）**，新原生身份 = agent 返回的新 sessionId 写 `external_id`（`manager.rs:2087`）。谱系只有标题前缀 `[Fork]`，无 DB 记录。

支持面：纯运行时探测 `session_capabilities.fork.is_some()`（`connection.rs:4190-4194`），事件 `ForkSupported` 推前端（`session_state.rs:284`）。代码注释能证实的：grok 1.0.4 的 initialize 只答 `{list, resume, close}`，**无 fork**（`registry.rs:819-821`）；claude-acp 声明 fork 是 RFC 基于 0.67 的快照，0.69 registry 注释未重申；codex-acp 1.4.0 是否声明 fork **注释未记录，需实测**；其余家无记录。

Rewind/checkpoint/truncate：**absent**。全仓无 rewind 功能代码；gemini parser 无 checkpoint 字样；无 claude `--fork-session`/`--resume` CLI 调用（codeg 全部走 ACP wire）。

## 3. 逐家原生能力盘点

通用底座：连接链 **resume → load → new+continues_from**（`connection.rs:4381-4531`；load 失败降级 new 并用 transcript header 的 `continues_from` 链接历史）。关键事实：**用户看到的历史来自磁盘 parser，不是 ACP wire**（`connection.rs:4383-4386`）；`session/load` 的 replay 被 drain 丢弃，只有 custom agent 无录制历史时才 hydrate（`4556-4563`）；codeg 自录 transcript（acp_transcript.rs）**只覆盖 Custom agent**（`connection.rs:559-563` transcript_dir_for 仅 custom 返回 Some）。13 个内置 parser 全齐（`import_service.rs:26-58`）。

| 家 | a) resume 接入 | b) 原生 fork 被用？ | c) rewind/checkpoint wire 可见？ | d) load 重放 |
|---|---|---|---|---|
| claude | ACP resume→load 链（具体 advertise 档位注释未记） | ACP session/fork 有；CLI `--fork-session` 未调用 | absent（/rewind、checkpoint 均不可见） | drain 丢弃 |
| codex | 同链；1.0.1+ resume 时从 config.toml 解 model_provider（`registry.rs:472-474`） | ACP fork 声明未证实；**但 app-server 原生有 thread/fork**——codex-acp 1.4.0 用它做 file-change audit（`registry.rs:579-588`、`connection.rs:3121-3129`） | absent；`thread/rollback` 存在 app-server 但 wire 不可见 | drain 丢弃 |
| gemini | external_id 不稳定（session/new fallback 会盖掉原 id），detail 加载靠 cwd+±300s 模糊匹配回写（`conversations.rs:1323-1366`） | 无 | absent（parser 无 checkpoint；/chat save 不可见） | 未记录 |
| opencode | 同链（能力未记录） | 无 | absent | 未记录 |
| cline | ACP session UUID ≠ 文件 id，走同上的 cwd 模糊 fallback | 无 | absent | 未记录 |
| grok | `session/resume` 第一档，1.0.0 起声明（`registry.rs:808-816`） | **无**（{list,resume,close}） | absent | resume 无重放（注释明确说不走 load 的 replay） |
| deepseek | loadSession + list/resume 都声明（`registry.rs:917-919`） | 无记录 | absent | 有 loadSession |
| openclaw/hermes/codebuddy/kimi/pi/cursor | 同链，细节未记录 | 无记录 | absent | 未记录 |
| custom | codeg 自录 transcript + hydrate | 无 | absent | replay 即唯一历史源 |

文件格式现状（parser 证实）：claude = `~/.claude/projects/<dir>/<sid>.jsonl`，行带 sessionId/cwd/gitBranch/uuid（`claude.rs:876-916`），subagent 是 `<sid>/subagents/agent-<id>.jsonl`；codex = `~/.codex/sessions/**/rollout-*.jsonl`，首行 `session_meta`，标题索引在 `session_index.jsonl`（`codex.rs:33,101,274-281`），**0.147 起原生 fork 的文件格式 = `session_meta.parent_thread_id` + `fork_turns` 复制父历史，codeg parser 已认识**（`codex.rs:1817-1838`）；gemini = `~/.gemini/tmp/<hash>/chats/session-*.json|jsonl`（`gemini.rs:36-66`）；cline = `taskHistory.json` 索引 + `tasks/<id>/api_conversation_history.json`（`cline.rs:21,55,137-154`）；opencode = **SQLite `opencode.db`**（只读打开，`opencode.rs:40-65`）；grok = `~/.grok/sessions/`（`grok.rs:138-198`）；deepseek = `~/.dsh/sessions`（`registry.rs:915-916`）。

## 4. 按消息位置 fork 的三条路径

**A. ACP 协议路——当前无影子，短期不可行。** schema 0.11.7 的 `ForkSessionRequest` 只有 session_id/cwd/additional_directories/mcp_servers/_meta（`agent-client-protocol-schema-0.11.7/src/agent.rs:1253-1281`），`LoadSessionRequest` 同样无截断参数（同文件 1084-1109）。唯一扩展点是 `_meta`：可以给 claude-agent-acp / codex-acp 私下约定 `_meta.messageUuid` / `_meta.upToTurn`（两家都是 TS 包，diff tarball 可行），但这不再是"协议路"而是逐家私约。工作量：若无私约，阻塞在 ACP RFD 演进；若有私约，每家中等。

**B. 原生文件手术路——claude/codex 可行，其余逐家降级。** 机制：复制原生会话文件 → 截到第 N 条 → 改会话 id → 让 harness resume 副本 → codeg 按 (agent_type, external_id) 精确匹配导入（`import_service.rs:402-407`）。逐家：

- **claude：风险中低**。jsonl 逐行可截；手术 = 复制 + 截断 + 全文替换 sessionId + 新文件名。正面证据：claude 原生 fork 在文件层就是"复制 transcript"（background_watch.rs:1256 已证实 codeg 懂这个布局）。风险点：parentUuid 链在截断点后的悬空引用、tool_use/tool_result 配对、isSidechain 子代理段、file-history 记录——parser 不读 parentUuid，但 Claude Code resume 时读，需实测验证（RFC §8 军规第 7 条的"原生 CLI 只读 resume 验证"正好兜这个）。
- **codex：风险中**。rollout 复制截断 + 改 session_meta.id + 写 parent_thread_id（格式已有原生先例）+ 同步 session_index.jsonl。但 codex resume 走 app-server 线程注册，文件手术产物能否被 app-server 发现需实测。
- **gemini：中**。单文件 json/jsonl 好截，但 id 本就不稳定（fallback 的存在即证据），原生 resume 弱。
- **cline：中**。两处文件（索引 + 历史）都要改。
- **opencode：高**。SQLite 多表（session/message/part + 自增 id 外键），手术=SQL 复制，不建议第一版碰。
- **grok/deepseek 等：未知**，原生 resume 入口与 id 语义未审计。
- 通用风险：手术产物会被全机扫描当"新会话"导入，要处理好 `harness_internal`/`codeg_owned` 标记（`import_service.rs:409-420`），否则可能被自动隐藏或重复。

**C. 重放路——通用兜底，不是 fork。** 新开会话 + 截断历史组装成首条 prompt。成本低、全 harness 通用；失真明确：工具调用链/思考过程丢失、token 成本随历史长度线性、长历史撞上下文窗口、agent 不认这段"自己的"历史。RFC §7.5 的 handoff 语义就是这个的正式化。适合给不支持 A/B 的家做"带上下文另开新会话"，UI 必须诚实叫 handoff 而非 fork。

## 5. 约束核对

- `parent_id` 禁复用有三重背书：`README.zh-CN.md:117`（§4 禁止复用清单）、`SESSION-WORKBENCH-RFC.zh-CN.md:744`、代码侧 fork 显式置 None。谱系需新表（RFC §9 的 source/target/relation_kind/anchor 字段设计可直接用）。
- 文件检查点与 git worktree：RFC 把 restoreFiles 设计成**独立能力**（§2 表、§10 单独入口+diff 预览+cwd 校验），没有把它绑到 git；Zed 的 git checkpoint 只是 §5.1 的参考经验。现有 `work_task` 的 worktree 是编码流水线（README.zh-CN.md:124 明确"不是 Topic"），与会话检查点无耦合设计，RFC 未回答"文件检查点存哪"——这是 RFC 的空白而非脱节。
- 消息不落库：db entities 无 message 表（`src-tauri/src/db/entities/` 清单确认），历史唯一事实源是原生文件 + custom agent 的自录 transcript。这意味着"消息锚点"只能从 parser 输出反推（claude 行 uuid、codex turn 序号），codeg 现在**没有持久化任何 provider anchor**——forkAtMessage 的第一步工程其实是锚点采集。

## 6. 建议（不定案，待用户/编排裁决）

最小可用切片排序：

1. **先补 RFC 对账 + 谱系表 + head fork 加固**（小）：把 §3 的 5 处脱节改对；新建 fork_relation 表记 fork_head 关系（不动 parent_id）；给现有 ACP fork 补回归测试。这是后续一切的底座，且无协议风险。
2. **claude 按消息 fork 走 `_meta` 扩展优先、文件手术兜底**（中）：claude-agent-acp 是 TS 包，给它加 `_meta.messageUuid` 让 SDK 原生 forkSession 定位历史 UUID；若上游不接，退回文件手术——claude jsonl 是所有家里格式知识最全、fork 布局已被 background_watch 证实的。无论哪条，先落"消息锚点采集"（parser 输出带 claude uuid / codex turn id）。
3. **codex 走 app-server thread/fork + thread/rollback**（中-大）：codex-acp 1.4.0 内部已在用 thread/fork，等于官方示范；代价是 codeg 要开第二条 app-server 通道或给 codex-acp 提能力。
4. **其余家降级重放路**（小-中）：UI 诚实叫"带上下文新开会话"，不冒充 fork；gemini/cline 等有稳定文件格式的后续再逐个评估手术。

不建议：opencode sqlite 手术进第一版；任何家走终端按键自动化（RFC §7.4 已正确否决）；在 ACP 加标准化截断参数前对 forkAtMessage 做协议层承诺。
