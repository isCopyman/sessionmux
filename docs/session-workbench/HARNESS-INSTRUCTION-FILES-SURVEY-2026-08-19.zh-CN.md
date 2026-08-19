# 跨 harness 指令文件调研：13 家约定、可移植性分层、同步方案

> 状态：2026-08-19 联网调研（WebFetch/WebSearch 官方文档实测 + 只读代码 + git 历史实测）。
> 缘起：`docs/session-workbench/HANDOFF-2026-08-19-leadership-relay.zh-CN.md:87-93` 记录的未派工需求"跨 harness 的用户级配置文件管理"。
> 姐妹篇：[ACP-ECOSYSTEM-ADAPTER-SURVEY-2026-08-19](./ACP-ECOSYSTEM-ADAPTER-SURVEY-2026-08-19.zh-CN.md)（该篇全文未涉及指令文件，不重叠）。
> 本文"指令文件"指 agent 启动时自动读入上下文的 markdown 约定文件（CLAUDE.md / AGENTS.md / GEMINI.md 等），不含 MCP/权限等配置文件。

结论先行：**AGENTS.md 已经赢了，13 家里 12 家原生读或可配置读，唯一硬拒的是 Claude Code**——Anthropic 官方文档原话就是 "Claude Code reads `CLAUDE.md`, not `AGENTS.md`"，官方给的互操作解法是在 CLAUDE.md 里写一行 `@AGENTS.md` 导入，或 `ln -s AGENTS.md CLAUDE.md`。**Windows 上 symlink 那条路官方自己劝退**（"requires Administrator privileges or Developer Mode, so use the `@AGENTS.md` import instead"），加上 Git for Windows 默认 `core.symlinks=false`，symlink 大法对本项目的 Windows 主力环境基本不可用——**这直接把方案空间收敛到"import 一行 + 生成式同步"两条**。**"一份大而全的共享文件"是被三方独立证据反对的**：Anthropic 官方给出 200 行硬指标并明说 `@import` 不省 token（"imported files still load and enter the context window at launch"），ETH Zurich 实证论文（arXiv 2602.11988）测出 context 文件通常不提升成功率、反而平均 +20% 推理成本，且摘要明写"仓库架构概览无用"——**恰好就是最不需要跨家共享的那类内容**，这个巧合是本文建议的核心依据。**codeg 仓内现状**：代码里零指令文件读写逻辑，但已有一套跨 13 家、Windows 用 junction 免管理员、失败回落复制的文件摆渡基础设施（`experts.rs`），复用成本极低；而 **codeg 自己的 AGENTS.md 与 CLAUDE.md 就是活标本**——同一天创建、内容仅差标题两行，但 CLAUDE.md 15 次提交 vs AGENTS.md 6 次，**9 次提交只改了 CLAUDE.md**，漂移真实发生过（详见 §5）。建议排序见 §6：**推荐 b'（轻量，但只做 lint/doctor 不做生成）> a（playbook）> c（现状）**。

## 1. 逐家指令文件盘点（13 家，2026-08-19 官方文档实测）

| 家 | 项目级 | 用户级/全局 | 原生 AGENTS.md | 多文件叠加 | 佐证 |
|---|---|---|---|---|---|
| claude code | `./CLAUDE.md` 或 `./.claude/CLAUDE.md`；`./CLAUDE.local.md`；`.claude/rules/*.md`（可带 `paths:` frontmatter 按路径懒加载） | `~/.claude/CLAUDE.md`、`~/.claude/rules/`；企业级 managed policy（Win: `C:\Program Files\ClaudeCode\CLAUDE.md`） | ❌ **明确拒绝** | 全部拼接，根→cwd，越近越靠后；子目录按需加载 | code.claude.com/docs/en/memory（官方，逐字核） |
| codex | 从 git 根到 cwd 每层 `AGENTS.override.md` > `AGENTS.md`（每层最多一个） | `~/.codex/AGENTS.override.md` > `~/.codex/AGENTS.md` | ✅ 标准发起方 | 从根往下拼接、空行分隔，"files closer to your current directory override earlier guidance" | learn.chatgpt.com/docs/agent-configuration/agents-md（官方） |
| gemini-cli | `GEMINI.md`（cwd + 各级父目录 + 子目录扫描） | `~/.gemini/GEMINI.md` | ⚠️ **可配置**：`settings.json` 的 `context.fileName` 收数组，官方示例就是 `["AGENTS.md","CONTEXT.md","GEMINI.md"]` | 全部拼接，每次 prompt 都发 | google-gemini.github.io/gemini-cli/docs/cli/gemini-md.html（官方） |
| opencode | `AGENTS.md`（向上遍历），无则回退 `CLAUDE.md` | `~/.config/opencode/AGENTS.md`，无则回退 `~/.claude/CLAUDE.md`（可用环境变量关掉） | ✅ 首选 | 三类各取第一个匹配；另有 `opencode.json` 的 `instructions` 字段收 glob 与远程 URL | opencode.ai/docs/rules（官方） |
| cline | `.clinerules/` **目录**（所有 `.md`/`.txt` 合并）；另原生检测 `.cursorrules`/`.windsurfrules`/`AGENTS.md` | `Documents\Cline\Rules`（Win）；**`~/.agents/AGENTS.md`** | ✅ | workspace + global 合并，冲突时 workspace 优先；Rules 面板可逐条开关 | docs.cline.bot/features/cline-rules（官方） |
| grok（现名 **Grok Build**，xAI） | `AGENTS.md`/`Agents.md`/`AGENT.md`/`CLAUDE.md`/`Claude.md`/`CLAUDE.local.md` + `.grok/rules/*.md` | `~/.grok/config.toml`（配置，非指令文件） | ✅ 且原生兼容 Claude 生态（skills/plugins/hooks） | 逐级收集，"deeper files taking precedence on conflicts" | docs.x.ai/build/features/project-rules（官方） |
| kimi（现名 **Kimi Code CLI**） | `AGENTS.md` 或 `.kimi-code/AGENTS.md` | `~/.kimi-code/AGENTS.md`；**`~/.agents/AGENTS.md`** | ✅ | 三处相对优先级官方**未明说**；旧版 kimi-cli 为逐级 merge | moonshotai.github.io/kimi-code/en/customization/agents.html（官方） |
| deepseek | — | — | 未查到 | — | **官方无同名编码 CLI**；codeg 钉的 `deepseek-acp` 是个人社区桥（姐妹篇 §2） |
| codebuddy（腾讯 CodeBuddy Code） | `CODEBUDDY.md`，**不存在时自动回退 `AGENTS.md`**；`.codebuddy/CODEBUDDY.md`、`.local.md`、`.codebuddy/rules/*.md` | `~/.codebuddy/CODEBUDDY.md`、`~/.codebuddy/rules/*.md` | ✅ 回退式 | 五类**全部拼接累加**（用户级→项目主文件→规则→子目录→local） | codebuddy.ai/docs/cli/memory（官方） |
| openclaw | execution folder 的 `AGENTS.md` 作为 project context 追加 | workspace `~/.openclaw/workspace/`：`AGENTS.md`+`SOUL.md`+`IDENTITY.md`+`USER.md`+`MEMORY.md` | ✅ | 追加拼接；execution folder **只取 AGENTS.md**，其余 5 个不从那里加载；有 20k/60k 字符上限 | docs.openclaw.ai/concepts/system-prompt（官方） |
| cursor | `AGENTS.md`、`CLAUDE.md`、`.cursor/rules/*.mdc`（mdc 支持 `globs`/`alwaysApply` frontmatter） | 未查到 | ✅ 原生，无需配置 | 子目录优先于父目录 | cursor.com/docs/rules（官方） |
| pi（Pi Coding Agent，earendil-works） | `AGENTS.md`/`CLAUDE.md`；某层放 `AGENTS.override.md` 则**只替换该层** | `~/.pi/agent/AGENTS.md`；`~/.pi/agent/SYSTEM.md` | ✅ | "All matching files are concatenated"，全局→父目录→cwd 全拼 | github.com/badlogic/pi-mono coding-agent README（官方） |
| hermes（Nous Research Hermes Agent） | **5 选 1，first match wins**：`.hermes.md`/`HERMES.md` → `AGENTS.override.md` → `AGENTS.md` → `CLAUDE.md` → `.cursorrules` | `~/.hermes/SOUL.md`（只从 HERMES_HOME 加载）；`memories/MEMORY.md`、`USER.md` | ✅（优先级第 3） | **互斥不拼接**——唯一一家如此 | 官方文档 + **issue #5200 open：文档宣称的层级合并与 `prompt_builder.py` 实际不符**，字面表述不可尽信 |

**import/include 语法**只有 4 家有，且都不省 token（见 §2）：claude code `@path`（最深 4 跳，反引号可转义）、codebuddy `@path`（最深 5 跳，机制与 claude 几乎逐字一致，大概率照抄）、gemini-cli `@file.md`（相对/绝对路径）、opencode 的 `instructions` 字段（glob + 远程 URL，5 秒超时）。codex/grok/kimi/pi/openclaw/hermes 的**指令文件层面均未查到** import 语法（pi 的 `APPEND_SYSTEM.md`、kimi 的 Jinja2 `{% include %}` 都是 system-prompt/agent 定义层的机制，不是指令文件的，勿混）。

**两个跨家硬事实**：其一，`~/.agents/AGENTS.md` 正在成为**用户级**跨工具约定，已确认 cline、kimi 两家原生读——这是"用户级指令一份到位"目前唯一有多家背书的落点。其二，agents.md 官网列的 23 家支持清单里**本文 13 家一个都没有**（含明确原生支持的 grok/kimi/pi/openclaw/hermes），说明**该清单严重滞后，不能拿"没上榜"当"不支持"**。

## 2. token 开销与"大而全"是否明智

- **Claude Code 是全量注入、且 import 不省 token**（官方原文：CLAUDE.md "loaded in full regardless of length"；imported files "still load and enter the context window at launch"）。真正懒加载的只有 **skills**（用到才载）、**`.claude/rules/` 带 `paths:` 的路径规则**、**auto memory 的 topic 文件**。官方硬指标："target under 200 lines per CLAUDE.md file. Longer files consume more context and reduce adherence."
- gemini-cli 同为全量（"sends them to the model with every prompt"）；openclaw 是唯一给了硬字符上限的（20k/文件、60k/总）。其余家的注入时机未逐一查证，**未查到**按需加载的反例。
- 独立实证：**arXiv 2602.11988**（ETH Zurich，Gloaguen/Mündler/Müller/Raychev/Vechev）测出提供 context 文件通常不提升任务成功率、平均 **+20% 推理成本**，跨模型跨 agent 跨（人写/AI 写）均成立；摘要逐字点名 "repository overviews, although popular and recommended by model providers, are not helpful"——**架构概览无效是一手实证**；"操作指令类（构建/测试命令）有效"这半句只在正文细分结论里，本次只核到摘要，标**二手待核**。**arXiv 2606.15828**（SCAM 2026）在 100 个热门仓库里量化出 Lint Leakage 62%、Context Bloat 42%、Skill Leakage 35%。
- **推论**：这条证据链恰好和 §3 的分层重合——**能共享的那层（命令、约定）正是有用的那层；不能共享的那层（harness 专属机制）本来就该分家**。所以"少而准 + 分家"不是妥协，是两个维度同时最优。

## 3. 内容的可移植性分层

- **L1 完全可移植（该共享）**：项目事实（技术栈、目录职责）、构建/检查/测试命令、代码风格与 lint 约定、关键约束（静态导出、路径别名、条件编译约定）。codeg 现有 CLAUDE.md/AGENTS.md **121 行里绝大部分是这层**。按 ETH 论文，命令类是唯一被证实有效的内容。
- **L2 语义可移植、写法分家**：子目录/模块级规则。各家载体不同（claude `.claude/rules/` + `paths:`、cursor `.mdc` + `globs`、cline `.clinerules/` 目录、codebuddy `.codebuddy/rules/`），**内容能抄，frontmatter 抄不了**。
- **L3 不可移植（必须分家）**：claude 的 subagent 模型钉法/hooks/skill 引用/`@import` 语法本身、cursor 的 `alwaysApply`、hermes 的 `SOUL.md` 人格、openclaw 的 workspace 五件套。**用户直觉正确——这层强求一致就是错的**。值得注意的是 grok 原生兼容 claude 的 skills/plugins/hooks，所以 claude↔grok 之间 L3 的边界比其他家窄。
- **落地写法**：L1 放共享源；L2 各家自己长；L3 只进各家专属文件。claude 侧官方推荐的形状正是 `@AGENTS.md` 一行 + 下面接 `## Claude Code` 专属段。

## 4. 社区现有方案与坑

| 工具 | 模型 | 目标数 | 覆写策略 | 活跃度（2026-08-19 GitHub/npm API 实测） |
|---|---|---|---|---|
| **ruler**（intellectronica） | `.ruler/` 单源 → 生成 | 30+ | 默认 **merge**；写前留 `.bak`；自动维护 `.gitignore` 托管块；`--nested` 支持 monorepo | ★2876，今天有 push；npm 0.3.44（06-30，落后于 GitHub） |
| **rulesync**（dyoshikawa） | `.rulesync/` 单源 → 生成，另有 `import` 反向吸收 | 40+ | generate 覆写 | ★1326，昨天 push；npm 16.14.0 同步发版 |
| **skillshare**（runkids） | 单源 → symlink/copy 分发 | 50+ | **Windows 用 NTFS junction，明说 no admin required** | ★2560，昨天 push；v0.20.0，Go 单二进制 |
| ai-rulez / ai-rules-sync | 单源生成，含 local-override 合并 | 19+ / 8 | 生成 | ★137 / ★118，均近两周有更新 |
| vibe-rules | 在目标文件里维护 XML 标签托管区块（不整体覆写） | 若干 | 区块内更新 | ★529 但**整整一年无提交**，勿依赖 |
| airul | 文档聚合 → 分发 | 6 | 覆写 | ★34，近一年无提交，可判废弃 |

**⚠️ 重名坑**：`rulesync.dev` / npm `rulesync-cli` 是**另一个商业 SaaS**（网页 dashboard + `npx rulesync-cli pull`，beta 后收费），与开源 `dyoshikawa/rulesync` 无关，装错包会很困惑。另 **skillshare 主打 Skills/Agents 分发而非项目级指令文件**，与本议题相邻但不同域——但它的 Windows junction 解法与 codeg `experts.rs` 的做法**完全一致**，是个独立佐证。

**symlink 的坑（对本项目是决定性的）**：① Anthropic 官方自己劝退 Windows symlink，改推 `@AGENTS.md`；② Git for Windows **默认 `core.symlinks=false`**，仓库里提交的 symlink 克隆到 Windows 会变成"内容是一串路径的普通文本文件"，除非 `git clone -c core.symlinks=true` 且系统允许；③ Git Bash 里 `ln -s` 默认造的是副本不是链接（需 `MSYS=winsymlinks:nativestrict`）；④ **原子写（temp + rename）会把 symlink 本身替换成普通文件**——claude-code issue #40857 已确认这个通病发生在 `.claude/settings.local.json` 上（原文："The rename replaces the symlink itself rather than writing through it"）。至于"`/init` 会不会写坏 CLAUDE.md 的 symlink"：**未查到直接案例**，且官方现行文档承诺 `/init` 对已存在的 CLAUDE.md 只建议不覆写；但同款机制在同一工具内已被证实，不能排除。硬链接同样中招且断裂更隐蔽。

**各家官方自带的导入能力**：claude code 的 `/init` **默认只读** `.cursor/rules/`、`.cursorrules`、`.github/copilot-instructions.md`；要读 `AGENTS.md`、`.devin/rules/`、`.windsurf/rules/`、`.clinerules` **必须设 `CLAUDE_CODE_NEW_INIT=1`**。另有 `/import [codex|gemini]`（v2.1.213+），一次性把 Codex/Gemini CLI 的指令文件、MCP servers、commands、subagents、skills 搬进来——**只支持这两家，且是一次性拷贝不是持续同步**。其余家未查到反向导入能力。

## 5. codeg 仓内现状（只读核实，2026-08-19）

- **代码中不存在任何指令文件读写逻辑**（可靠否定结论）。全仓 grep `CLAUDE.md`/`AGENTS.md`/`GEMINI.md`/`clinerules`/`cursorrules`/`copilot-instructions`/`append-system-prompt` 的命中只有两类：文档自述，以及**解析别家产物**——`src-tauri/src/parsers/codex.rs:3935-3958` 的 `strip_agents_instructions_block` 专门剥离 Codex 写进 transcript 的 `# AGENTS.md instructions for <path>` 样板块，免得污染自动标题。方向是"读展示"，不是"写注入"。
- **experts 注入 = 文件摆渡，不碰 prompt**：19 个 skill 经 `include_dir!` 编译期打包（`src-tauri/src/commands/experts.rs:35`）→ 解压到 `~/.codeg/skills/<id>/`（`experts.rs:189-193`，`lib.rs:423` / `bin/codeg_server.rs:439` 调用）→ 启用时链接进**各家自己就会扫描的 skills 目录**（`link_one_locked`，`experts.rs:811-876`）。各家目录是硬编码表 `skill_storage_spec`（`src-tauri/src/commands/acp.rs:7710-7894`，如 ClaudeCode→`~/.claude/skills`、Codex→`~/.codex/skills`+`~/.agents/skills`、Cline→`.clinerules/skills`）。**关键**：`create_link_raw`（`experts.rs:392-413`）在 Windows 走 `junction::create`，失败才递归复制并置 `copy_mode=true` 让 UI 警告升级不会自动传播——**这正是 §4 里 skillshare 采用的同一解法，codeg 已经拥有一套 Windows 安全的跨 13 家摆渡设施**。
- **codeg-mcp 走 wire 不落盘**：作为 `session/new` 的 `mcpServers` 字段随 ACP 请求注入（`src-tauri/src/acp/connection.rs:3384-3643`），随连接生灭。但**改别人家配置文件已有先例**：对 Hermes/Kimi/Grok/Cursor 四家，codeg 直接读写其原生全局 MCP 配置（`src-tauri/src/commands/mcp.rs`，如 Kimi `~/.kimi-code/mcp.json:2692-2854`、Grok `~/.grok/config.toml:2878`）。
- **自家仓库就是漂移标本**：根目录 `AGENTS.md`（121 行）与 `CLAUDE.md`（121 行）**同在初始提交 54d1097b(2026-03-06) 创建**，当前内容**只差第 1、3 两行**（标题与"guidance to Code Agent / Claude Code"一句）。但 git 历史显示 **CLAUDE.md 15 次提交 vs AGENTS.md 6 次，9 次提交只改了 CLAUDE.md、0 次只改 AGENTS.md**。漂移真实发生过并且是这样收场的：`27626265`(04-26) 给 CLAUDE.md 单独 +2 行 → 一路单边累积 → `93ba9fdf`(05-05) 把 CLAUDE.md 独有的 14 行**删掉**（不是补给 AGENTS.md）→ `3ffe3d06`(05-31) 又单边改 1 行 → 同日 `20b74236` 才两边一起改补齐。**结论：人肉同步能做到，但靠的是删内容和当天想起来，没有任何机制保障。**

## 6. 落到 codeg 的建议（不定案）

1. **先做 b'（推荐）：codeg 只做"检查"不做"生成"**。在设置页/命令里加一个跨 harness 指令文件 **lint/doctor**：扫工作区与用户目录，列出发现的所有指令文件、各家实际会读哪几个（按 §1 的表）、以及**共享层内容的漂移点**（如 codeg 自己那 9 次单边提交）。理由：生成器要覆写用户文件，风险和维护面都大；而 §5 证明**真正的痛点是"忘了同步"而非"同步很麻烦"**——两个 121 行文件人工对齐不难，难的是没人提醒。诊断工具收益/风险比最好，且不与 ruler/rulesync 抢生态位（用户想用照样能用）。
2. **b'' 作为可选加项：一键写 `@AGENTS.md` 桥接**。让 AGENTS.md 当唯一共享源，codeg 只负责在 CLAUDE.md 顶部落一行 `@AGENTS.md` 并保留下方 `## Claude Code` 专属段（官方推荐形状）；gemini 侧则写 `settings.json` 的 `context.fileName: ["AGENTS.md","GEMINI.md"]`。这两笔都是**幂等的一行级写入**，不是全文生成，且 codeg 对 gemini 之外四家已有改配置文件的先例。**明确不做 symlink**（§4 四条坑，Windows 全中）。
3. **a（playbook）无论如何都要做，且是 1/2 的前置**。写进 `docs/`：L1 共享层放 AGENTS.md（含 `~/.agents/AGENTS.md` 作用户级落点，cline/kimi 已原生读）；L2 各家 rules 目录自己长；L3 专属机制**只进各家私有文件、不要求一致**。并把 200 行上限、"架构概览类无用"（arXiv 2602.11988 摘要一手）写成硬约定——**codeg 现有 121 行文件里的"项目概述/架构"两节正是论文点名无效的那类，是最该先瘦身的地方**。
4. **顺手补一条自检**：给 codeg 仓库自己加个 CI 检查，比对 AGENTS.md 与 CLAUDE.md 除首 3 行外是否一致。成本 5 行脚本，直接堵住 §5 那 9 次单边提交的复发。
5. **不建议**：① 自研全量生成器（ruler ★2876 / rulesync ★1326 都在日更，重造无收益，真要用直接接现成的）；② 任何形式的 symlink/hardlink 方案；③ 追求 13 家完全一致（L3 层强求一致会把 hermes 的 SOUL.md、openclaw 的 workspace 五件套这类人格/运行时语义污染进通用文件）；④ 把 agents.md 官网的支持清单当权威（§1 末：本文 13 家一个都没上榜）；⑤ 把 hermes 官方文档的层级合并描述当准（issue #5200 明指与代码不符）。
6. **待用户拍板的取舍**：b' 的诊断范围是**只管工作区**还是**连 `~/.claude`、`~/.codex`、`~/.agents` 等用户级目录一起扫**？后者信息更全但要读用户主目录，涉及权限与隐私预期，需要显式同意。
