# ACP 生态与适配器调研：社区桥、协议演进、同行做法

> 状态：2026-08-19 联网调研（只读代码 + WebSearch/WebFetch/npm/crates/GitHub API 实测）。
> 姐妹篇：[SESSION-FORK-REWIND-SURVEY-2026-08-19](./SESSION-FORK-REWIND-SURVEY-2026-08-19.zh-CN.md)（仓内机制事实，本文不重述）。
> 本文 ACP 一律指 Zed 系 **Agent Client Protocol**，与 IBM/Linux Foundation 的同名协议无关。

结论先行：**fork/rewind 痛点已有两个现成实现在上游排队**——claude-agent-acp PR #872（`session/fork` 加 `_meta.claudeCode.rewindTo`，按消息 fork，正是 G10"路径A私约"的现成实现）和 PR #966（`_session/rewind` 包原生 /rewind，文件+对话双模式），均 open 且无 maintainer 回应。协议侧：schema 已从 codeg 钉的 0.11.7 走到 **1.6.0**（2026-07-21），v2 alpha 已发，**Session Rewind RFD（PR #1321）在酝酿**但无 maintainer 评审。维护负担侧：官方 ACP Registry 已收 41 个 agent 且版本每小时自动更新，是最接近"可直接依赖的适配器集合"的东西；同行里只有 Vibe Kanban 走"自维护 executor + 原生 headless CLI"路并且原生 fork 覆盖反而更好。

## 1. 协议本身的演进

- **版本线**（crates.io `agent-client-protocol-schema` API 实测）：0.11.7（codeg 钉，`Cargo.toml:113` 带 `unstable_session_fork` 等 feature）→ 0.14.0（06-18）→ **1.0.0（06-24）→ 1.6.0（07-21，最新稳定）**；TS 侧 Schema v1.20.0（07-21），**v2.0.0-alpha.2 已发**。release notes（GitHub releases atom 实测）在 0.12–1.6 区间**无 fork/rewind/checkpoint 条目**；亮点：1.3.0 起 v2 线统一 `session/load`+`session/resume`（#1584）、boolean config 稳定化（#1604）、elicitation 增强（#1397）、1.6.0 加 tool call name（#1752）。
- **sacp 11.0.0（2026-03-16）已是该 crate 最新版**（crates.io 实测）；它是 Symposium 的 ACP 扩展/proxy SDK（symposium-dev/symposium-acp，MIT/Apache-2.0），不是官方 org 的 rust-sdk。sacp 与 schema 1.x 的兼容性未核对。
- **session-fork RFD**（docs/rfds/session-fork.mdx，josevalim 起草）：现 spec 只有 head fork；**fork-at-message 明示留作未来扩展**（"optional message ID… fork happens at a specific message"，capability 里预留 messageId-for-checkpoints）。即协议路仍无影子，G10 判断成立。
- **v2 session-resume-replay RFD**（draft，2026-07-02）：`session/load` 并入 `session/resume`，加 `replayFrom` cursor（现仅 `{type:"start"}`），文末点名未来 cursor 含 "a specific message, checkpoint"——**replay 游标是截断语义进入协议的最可能入口**。对 codeg 的"load replay 被丢弃"痛点无直接影响（replay 仍在 wire 上，用不用是 client 选择）。
- **Session Rewind RFD：PR #1321**（agentclientprotocol/agent-client-protocol，htahaozlu，2026-05-31 开，**open、零 maintainer 评审**）：提 `session/rewind{sessionId,toMessageId}` + `session/edit_prompt` + `history_truncated` 通知 + `session.rewind` capability；明确 supersede #1214、与 #1261（session/inject）互补。第三方 harness（wolfharness #109）已把它列进 v2 跟踪，标 P3。

## 2. 社区桥盘点（逐家，版本为 npm/GitHub API 2026-08-19 实测）

| 家 | codeg 钉 | 最新 | 桥/维护方 | 活跃度与备注 |
|---|---|---|---|---|
| claude | 0.69.0 | **0.70.0**（08-18） | @agentclientprotocol/claude-agent-acp，ACP org（Zed 系）官方 | 极活跃（8 月已 8 个 release）；0.70.0 = loaded session 换 provider（#1002）；**fork-at-message/rewind 见 §3** |
| codex | 1.4.0 | **1.5.0**（GitHub 08-18；**npm latest 仍 1.4.0**，发布滞后） | @agentclientprotocol/codex-acp，ACP org | 极活跃；1.5.0 同为主题"loaded session 换 provider"（#404）；无 fork/rewind 新东西 |
| gemini | 0.55.1 | 0.55.1 | @google/gemini-cli 原生 `--acp`，Google 官方 | 已是最新 |
| opencode | 1.18.18 | 1.18.18（08-13） | anomalyco/opencode 原生 `acp` 子命令 | 已是最新 |
| cline | 3.0.55 | 3.0.55 | cline 原生 `--acp` | 已是最新 |
| kimi | 0.36.1 | **0.37.2** | @moonshot-ai/kimi-code 原生 `acp`，Moonshot 官方 | 活跃；0.37.x 增量内容未查到 |
| grok | 1.0.4 | **1.0.5** | @xai-official/grok 原生 `agent stdio`，xAI 官方 | patch 级 |
| codebuddy | 2.137.0 | 2.137.1 | @tencent-ai/codebuddy-code 原生 `--acp`，腾讯官方 | patch 级 |
| openclaw | 2026.7.1 | 2026.7.1-2 | openclaw 原生 `acp` | patch 级 |
| cursor | 2026.08.11 | 未查到（无公开版本 API） | cursor-agent 原生 `acp` | — |
| deepseek | 0.3.0 | **0.5.0**（08-18） | **deepseek-acp = xintaofei/deepseek-acp 个人社区桥**（非 DeepSeek 官方；官方 @deepseek-ai/dsh-acp 是 automation-only） | **极年轻**：0.1.0 发布于 08-15，4 天 6 个版本；0.4.0/0.5.0 变更内容未查到，升级前需重新审 diff |
| pi | 0.0.33 | 0.0.33 | pi-acp 社区桥（包 `pi-acp`） | 已是最新 |
| hermes | 0.20.1 | **0.20.4** | hermes-agent = wyrtensi/hermes-agent-npm 社区桥（包官方仓源码） | 活跃；沿用既有"exact pin + 审 wrapper diff"纪律即可 |

官方 **ACP Registry**（agentclientprotocol/registry，Apache-2.0，CI 校验 handshake）：现收 **41 个 agent**，索引 `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`，**版本每小时 cron 自动更新**。codeg 未接入的新面孔：github-copilot-cli（ACP public preview 2026-01-28，GitHub 官方 changelog）、goose、qwen-code、kilo、amp-acp、auggie、factory-droid、mistral-vibe、glm-acp-agent、junie 等。**deepseek 不在 registry**（codeg 这条线纯社区）。各桥许可证未逐项核实（registry 本身 Apache-2.0；sacp/schema crate Apache-2.0 / MIT）。

## 3. 能直接解决 fork/rewind 痛点的现成货（显著标出）

- **claude-agent-acp PR #872「fork 时 rewind 到指定消息」**（nitayart，2026-07-13 开，open、无评审）：`session/fork` 加 `_meta.claudeCode.rewindTo`（ACP messageId），内部翻成 SDK uuid 后走 `resumeSessionAt` + `forkSession:true`——**就是按消息 fork**，新 session id，走 `_meta` 私约不改协议，capability 广告 `_meta.claudeCode.rewind`。带 5 个测试、对 Claude Code 2.1.207 实测过。这正是 G10"路径A：给桥加 `_meta` 扩展"设想的完整实现。
- **claude-agent-acp PR #966「expose file and conversation rewind over ACP」**（Kanishk2207，2026-08-06 开，open、作者 08-18 仍在催评审）：`_session/rewind_points`（列可回退点）+ `_session/rewind{mode: files|conversation|both, dryRun}`，包 Claude 原生 /rewind（`rewindFiles` + `resumeSessionAt`，并证实后者"truncates rather than branches"）；默认开 `enableFileCheckpointing`；36+ 测试。动机来自 Emacs agent-shell 客户端。
- 两 PR 共享 `resolveMessageUuid` helper、有 6 处机械冲突；都**没有 maintainer 回应**——风险是烂尾，机会是 codeg 去评审/联署能加速，或照方案给钉版桥打 patch（G10 已评估 tarball diff 可行）。
- 佐证（佐证等级：PR 描述+测试，未自行跑码）：claude Agent SDK 原生就有 `forkSession`/`resumeSessionAt`/`rewindFiles` 三件套；codex app-server 原生 `thread/fork`+`thread/rollback`（仓内 registry.rs:579-588 已证实 codex-acp 1.4.0 在用）。**两家主力 harness 的原生底座都齐了，缺的只是 wire 暴露**。

## 4. 同行怎么解决 N 家适配负担

- **Zed / JetBrains**：ACP 发起方，把桥建设搬到 ACP org 众包（claude-agent-acp/codex-acp 即此产物），registry 与 JetBrains 联合运营（2026-01-28 上线）——**把适配面做成公共品**。
- **AionUi**（iOfficeAI/AionUi，免费开源桌面"24/7 Cowork"，最像 codeg 的同行）：宣传接 20+ CLI agent，**"via ACP or compatibility adapters"**（readme），自研补 ACP 够不到的家；数据本地 SQLite。其 issue #155 显示兼容适配层会绕开原生 CLI 的 MCP 配置——自研层的典型坑。
- **Vibe Kanban**（BloopAI）：**不用 ACP**，自维护 `StandardCodingAgentExecutor` trait 包各家 headless CLI；其 executor 能力表里 **ClaudeCode/Codex/Gemini/Opencode/QwenCode 的 Session Fork 全 ✓**（deepwiki 文档）——走原生 CLI 的 fork/resume 机制，fork 覆盖反而比 ACP 路好，代价是全自研适配。
- **Conductor**（Melty Labs，Mac 闭源商业）与 **Crystal**（Stravu，MIT，2026-02 弃坑→继任 Nimbalyst）：都只深耕 Claude Code + Codex（Conductor 加 Cursor），**靠不收编长尾来规避适配负担**。
- **现成 ACP client 层**：acpx（openclaw org，headless ACP CLI client，npm `acpx`）及其 Rust 端口 motosan-dev/acp-cli；Vercel ai-sdk 有社区 ACP provider。都是 client 侧，不含 adapter 集合。
- **没有找到可直接依赖的"适配器集合"包**；最接近的是 ACP Registry 的 registry.json（元数据+版本，不含实现）。

## 5. 横向替代（扫描级）

- **各家原生 headless/SDK**：claude Agent SDK（forkSession/resumeSessionAt/rewindFiles，§3）、codex app-server（thread/fork、thread/rollback）、gemini `-p`、opencode server 模式、社区的 headless-cli/agent-cli-to-api 包装——能力普遍比 ACP wire 全，但每家一套，等于回到 Vibe Kanban 路。
- **MCP 上做会话管理**：未查到任何把 fork/rewind 做成 MCP 标准的方案；MCP 定位是工具供给不是会话控制。
- 判断：ACP 生态在升温（registry 41 家、Copilot 入场、v2 在途），**换协议的收益看不到，留在 ACP + 盯 v2 rewind/replay-cursor 是对的**。

## 6. 落到 codeg 的建议（不定案）

1. **claude：盯并考虑参与 PR #872/#966 评审**；若上游持续无回应，照 #872 方案给钉版桥打 `_meta.rewindTo` patch（与 G10 建议 2 完全一致，现在有现成 diff 可抄）。这是 forkAtMessage 全仓最短路径。
2. **codex：升 1.5.0**（等 npm 落地）继续钉官方桥；按消息 fork 仍走 G10 的 app-server 路，社区无更好货。
3. **deepseek-acp：跟升到 0.5.0 前先审 0.3.0→0.5.0 diff**（个人社区桥、4 天 6 版，沿用 hermes 的审计纪律）。
4. **常规跟进**：kimi 0.37.2、hermes 0.20.4、grok/codebuddy/openclaw patch；gemini/cline/pi/opencode 已是最新。
5. **协议升级独立评估**：schema 0.11.7→1.6.0（核对 sacp 11.0.0 兼容性与 unstable feature 改名；fork 在 1.x 的状态未核对——0.11.7 里它还在 `unstable_session_fork` 后面）。升级本身不解 fork/rewind，但躺在 0.11.x 会越落越远。
6. **盯上游两个提案**：Session Rewind RFD（PR #1321）与 v2 replayFrom cursor——任一落地都直接解 G10 痛点①③；可在 #1321 留 codeg 的 use case 增加存在感。
7. **减负**：接 ACP Registry 的 registry.json 做"新版本监控 + 新 agent 发现"（copilot-cli/goose/qwen-code 是潜在新接入对象），比手工巡 13 家 npm 省力；不建议为此迁到 Vibe Kanban 式全自研 executor。
8. **不建议**：现在自研任何家的 rewind（claude 等 #966、codex 有 app-server 原生）；把 registry.json 当运行时依赖（只做监控源，pin 权留在 registry.rs）。
