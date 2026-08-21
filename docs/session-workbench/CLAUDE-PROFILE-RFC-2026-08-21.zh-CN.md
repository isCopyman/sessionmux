# Claude 启动配置档 RFC（订阅 / API / 多 settings，2026-08-21）

- 缘起：用户提出"我在 `.claude` 里有多个 settings.json，甚至项目下也有，分别对应
  订阅、API、不同 API URL —— codeg 启动 claude 时能不能选其中某个？或者 codeg 自带
  内置 settings 启动时用？这个好做吗，会不会破坏多 harness 结构？"
- 结论先行：**好做，而且比原方案（合成 env）更干净**；不破坏多 harness 抽象，
  但会暴露若干"假设只有一个全局 Claude 配置目录"的代码点，需一并改造。
- 前置调研：`O39-PROVIDER-SWITCH-RECON.zh-CN.md`（paseo / monet 两家做法）。
- **本 RFC 未施工，等用户拍板**（涉及凭据、schema 小改、以及停掉一处现有写盘行为）。

## 1. 机制核实（本人逐个读码/读包核实）

### 1.1 用户的心智模型是对的

Claude 的配置是分层合并：用户全局 → 项目 → local，后者覆盖前者；显式
`--settings <file>` 再加一层；进程 env 再压一层。

### 1.2 codeg 跑的不是 `claude` CLI，是 ACP 适配器

codeg 启动的是 `@zed-industries/claude-agent-acp`（0.21.0），它内部调
`@anthropic-ai/claude-agent-sdk`（0.2.71）。所以能不能"选 settings"取决于
**适配器**，不是 CLI 有没有 `--settings`。核实结果（本机已装包）：

| 事实 | 位置 |
| --- | --- |
| 适配器把整个配置目录解析成 `CLAUDE_CONFIG_DIR ?? ~/.claude` | `@zed-industries/claude-agent-acp/dist/acp-agent.js:11` |
| 适配器从该目录读 `settings.json` | `dist/settings.js:74` |
| 适配器**显式**要求 SDK 加载三层设置 `settingSources: ["user","project","local"]` | `dist/acp-agent.js:862` |
| SDK 同样以 `CLAUDE_CONFIG_DIR ?? ~/.claude` 解析配置目录 | `claude-agent-sdk/sdk.mjs`（`T6()`） |
| SDK 另支持 `settings`（对象或路径）与 `--setting-sources` | `sdk.mjs` / `cli.js` |

**关键推论**：`CLAUDE_CONFIG_DIR` 是一个**进程环境变量**，而 codeg 本来就为每次
spawn 单独构造 env（`build_session_runtime_env`，`commands/acp.rs:9328`）。
所以——

> **每个会话用哪份 Claude 配置 = 启动时给它不同的 `CLAUDE_CONFIG_DIR`。
> 适配器零改动、SDK 零改动、用户的 `~/.claude` 一个字节都不动。**

一份配置目录里带的不只是 `settings.json`，还有**登录凭据**。所以这一个开关
天然覆盖用户要的全部三态：

| 想要的效果 | 档怎么配 |
| --- | --- |
| 官方订阅（就是我平时登录的那个） | 不设 `CLAUDE_CONFIG_DIR`（跟随 `~/.claude`），或指向该目录 |
| 另一个订阅账号 | 指向另一个目录（如 `~/.claude-work`，用户在那儿登录过一次） |
| API / 中转 | 指向 codeg 生成的档目录，里面 `settings.json` 写好 `env.ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` |
| 项目级差异 | 天然生效——适配器已加载 project 层（`settingSources` 含 `project`/`local`） |

### 1.3 为什么这比"只注入 env"更好

- 与 Claude 自己的配置模型同构，用户已有的多份 settings 直接可用，不用在 codeg 里
  重新填一遍。
- 不需要把凭据塞进进程 env（env 会被子进程、`ps`、崩溃报告看到）。
- 天然带 OAuth 凭据，"订阅 vs API"不再需要"清空 token 逼它回落 OAuth"这类
  防御式技巧（monet 的 `official-direct` 就在干这个）。

## 1.4 认证优先级（从发行包里挖出的权威表，非记忆）

用户问："settings.json 里没配 url 就是默认用登录凭证？还是 env token？还是把 token
写进 settings.json？"—— 三个都成立，但有严格顺序。SDK 发行包里的解析函数原文
（`@anthropic-ai/claude-agent-sdk/cli.js`，`function Du()`）：

```js
if (process.env.ANTHROPIC_AUTH_TOKEN)      return {source:"ANTHROPIC_AUTH_TOKEN"}
if (process.env.CLAUDE_CODE_OAUTH_TOKEN)   return {source:"CLAUDE_CODE_OAUTH_TOKEN"}
if (<oauth token via file descriptor>)     return {source:"CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR"}
if (<apiKeyHelper 脚本>)                    return {source:"apiKeyHelper"}
if (<存储的 oauth 凭据>.accessToken)         return {source:"claude.ai"}     // ← 订阅
return {source:"none"}
```

另有 `ANTHROPIC_API_KEY` 走独立分支（`f$()`，受 Bedrock/Vertex 等开关门控）。

三条结论：

1. **没有 url / token → 回落 `claude.ai` 存储凭据 = 订阅**（用户猜对了）。凭据文件是
   `<CLAUDE_CONFIG_DIR>/.credentials.json`（`QN1()`：`storagePath = join(configDir,
   ".credentials.json")`）——**这正是"换配置目录=换登录账号"成立的原因**。
2. **`ANTHROPIC_AUTH_TOKEN` 优先级最高，会压过订阅**。所以"我要用订阅"不能只靠
   "不填 token"，还必须确保没有从别处继承来一个（CLI 自己都为此做了告警：
   `claude-ai-external-token` —— 已登录 claude.ai 但被外部 token 覆盖时提示）。
3. **写进 settings.json 的 `env` 块和设进程环境变量是等价的，而且前者会覆盖后者。**
   实证（`function pxq()`）：`let A = T1().env||{}; for (...) process.env[Y] = z`，
   逐层设置文件的 `env` **无条件写进 `process.env`**。

### 1.5 由此得到的关键陷阱（推翻"只注入 env 就够了"）

既然设置文件的 `env` 块是**后写入**且无条件覆盖，那么：

> **codeg 今天靠 spawn env 注入的 `ANTHROPIC_*`，会被用户自己的
> `~/.claude/settings.json`（或项目级 settings）里的同名键静默覆盖。**

这解释了 O39 调研留的悬念"要不要上 `--settings` 文件"——答案是：光靠 env 不可靠。
而 `CLAUDE_CONFIG_DIR` 方案天然免疫：它换的是**加载哪一份 settings**，不存在被另一
份覆盖的问题。这条独立地把方案选择从"合成 env"推向"换配置目录"。

（另注：`apiKeyHelper` 是 settings.json 的合法字段，指向一个输出 key 的脚本，优先级
在订阅之上。托管档若用它可避免 key 明文落 settings，但要多管一个脚本文件，第一期不用。）

## 2. 今天的做法错在哪（并非空谈，上游已有 issue）

codeg 现在的"绑定模型供应商"会 **cascade 写进用户的全局原生配置文件**：
`cascade_update_agent_config`（`commands/acp.rs:9007-9040`）把
`ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` 写进 `~/.claude/settings.json`
（Codex 侧写 `~/.codex/config.toml`）。

- 后果一：同机"会话 A 走订阅、会话 B 走中转"根本不可能——它们共享同一个文件。
- 后果二：污染用户的原生工具。**上游 issue #520（OPEN，未修）**：codeg 0.26.2 在
  用户根本没配供应商时把 `model_provider = "codeg"` + 空 `base_url` 写进
  `~/.codex/config.toml`，导致原生 Codex App 一起坏掉，且"在 codeg 里删供应商也修
  不好"。issue 下有第二位用户在 Windows 独立复现。相关：#406。
- 结论：**做 per-session 档的前提，是先停掉往用户原生文件写这件事。** 这同时顺手
  解掉 #520 这一类上游 bug（我们分支自己受益）。

## 3. 会不会破坏多 harness 结构？——不会，但要正名

codeg 的抽象是「agent_type → 命令 + args + env + 原生配置文件路径」，本来就有
**逐 agent 分支**的先例（`agent_env_keys`、`persist_agent_local_config_json` 里
Claude / Codex / Cline / KimiCode 各走各的路）。所以"Claude 用配置目录、Codex 用
隔离的 config.toml、其它 agent 只用 env"不是破坏抽象，而是**给已经存在的分支起个
统一的名字**：

> **启动档（launch profile）** = 「这次 spawn 让 agent 看见什么配置」。
> 各 agent 实现形状不同：Claude=配置目录 / Codex=隔离 provider 块 / 其余=env 覆盖。
> 上层（会话绑定、UI 选择器、切换即重启）对所有 agent 一致。

真正需要改造的是那些**假设"只有一个全局 Claude 配置目录"**的点（已核实）：

| 代码点 | 现状 | 档化后要怎样 |
| --- | --- | --- |
| `parsers/claude.rs:990-995` | **已经认识** `CLAUDE_CONFIG_DIR`（有 env 参数，回落 `~/.claude`） | 好消息：读转录这条线天生兼容，只需按会话传对目录 |
| `commands/acp.rs:7582` | 硬编码 `~/.claude/settings.json`（agent 原生配置面板） | 要按当前档解析 |
| `commands/mcp.rs:686` | 硬编码 `~/.claude/settings.json`（MCP 开关） | 要按档解析；**注意**：换档=换 MCP 清单，codeg 注入的 codeg-mcp 必须跟着进新档目录，否则协作工具在该会话里消失 |
| `commands/acp.rs:7714` | 硬编码 `~/.claude/skills` | 同上（换档会换 skills 可见性） |

**这是本方案唯一的实质风险**：配置目录是"一整套人格"（settings + 凭据 + MCP +
skills + agents）。换档不只换钱包，也换工具箱。设计上必须让 codeg 自己的注入
（codeg-mcp）在每个档里都在场，否则用户会遇到"换了个档，群聊工具没了"。

## 4. 建议方案（第一期只做 Claude Code）

1. **档目录三种来源**：
   - `跟随默认`（不设 env，= 今天的行为，兼容老会话）
   - `指向已有目录`（用户填 `~/.claude-work` 这类自己维护的目录 —— 直接满足
     "我已经有多个 settings"）
   - `codeg 托管档`（codeg 在自己数据目录下建 `profiles/<id>/`，写 `settings.json`，
     凭据只落这里，`0600`）
2. **绑定**：会话级可选绑定，空 = 跟随 agent 默认（不迁移任何现有会话）。
   *需要一个可空列*（`conversation.provider_profile_id` 或等价）——**属 schema 改动，
   按铁律等你签字**；不想动 schema 的话第一期可以只做"新建会话时选、存在会话不可改"，
   把绑定放进已有的会话偏好 JSON —— 但那会污染 ACP 偏好的语义，我不推荐。
3. **注入**：`build_session_runtime_env` 解析顺序 = 会话档 > agent 默认 > 今天的行为；
   Claude 分支产出 `CLAUDE_CONFIG_DIR`（+ 必要时 `settings` 路径）。
4. **停写用户原生文件**：Claude 路径的 cascade 改为默认关（开关保留，便于回退）。
   已经写进用户文件的内容**不自动清理**（另需单独拍板）。
5. **UI**：新建会话选择器旁一个"接入方式"下拉；已建会话在头部显示当前档，改选 →
   标 pending → 下一回合/手动重连时重启该进程（复用现有 `SessionConfigStaleBanner`，
   turn 进行中禁止）。**不加新的大按钮**（依你今天的原则）。
6. **codeg-mcp 必须跟档走**：每个托管档目录生成时一并写入 codeg 的 MCP 注入；
   指向用户自有目录的档，给出"该目录缺少 codeg 协作工具"的显式提示与一键补写。

## 4.5 前后端各干什么（用户追问："UI 在哪改，前后端怎么处理"）

**后端（Rust）**
1. `build_session_runtime_env`（`commands/acp.rs:9328`）里新增一步"解析本会话的档"：
   会话绑定 > agent 默认 > 今日行为。Claude 分支的产物就是一个
   `CLAUDE_CONFIG_DIR=<目录>`（跟随默认时**不设**该键，保持今天的语义）。
2. 托管档目录的生成/维护（`<数据目录>/claude-profiles/<id>/settings.json`，0600），
   含把 codeg-mcp 注入写进该目录。
3. 关掉 Claude 路径的 `cascade_update_agent_config` 写盘（默认关，开关可回退）。
4. 档的 CRUD 命令 + `_core` 函数（桌面/服务器双模式共用，按仓库惯例）。
5. 连通性自检（可选）：用该档跑一次最小请求，回报 401/404/超时。

**前端（三处，全部复用已有壳子，不新造页面）**
1. **设置 → 智能体 → Claude Code**（`src/components/settings/acp-agent-settings.tsx:169`
   现有三态 `official_subscription / custom / model_provider`）：把这里升级成"档列表"
   ——跟随默认 / 指向已有目录（填路径，带目录选择器）/ 托管档（填 URL+key）。
   现有 `/settings/model-providers` 页作为"端点目录"保留，档引用它，不再各填一份。
2. **新建会话**：`AgentSelector` 旁一个"接入方式"下拉，默认=该 agent 的默认档；
   用户不选就是 NULL（跟随全局，老会话零影响）。
3. **会话头部**：显示当前档名（小 chip，**不加大按钮**）；改选 → 标 pending →
   复用现有 `SessionConfigStaleBanner` 提示重连，turn 进行中禁止。

**数据面**：档表（或复用 `model_provider` 加类型列）+ 会话上一个可空外键。
后者是唯一的 schema 改动，等签字。

## 5. 待拍板

| # | 问题 | 我的建议 |
| --- | --- | --- |
| A | 是否停掉往 `~/.claude/settings.json` / `~/.codex/config.toml` 的 cascade 写入 | **停**（默认关，可开关回退）。顺带解掉上游 #520 一类问题 |
| B | 会话级绑定要不要加那个可空列（schema 小改） | **加**。这是唯一干净解；改动是加一个 nullable 列，不动既有数据 |
| C | 凭据存哪 | 托管档 = 目录内 `settings.json`（0600）；**另修一个现存问题**：`model_provider` 的 API key 今天在 list 接口里明文回传，应改掩码 |
| D | 第一期范围 | 只做 Claude Code；Codex 的隔离 provider 块单列（它更重） |
| E | 命名 | 对外叫「接入方式」，档的集合沿用已有「模型供应商」页，不再造第四个词 |
| F | 换档是否允许中途切 | 允许，但必须重启该会话进程，且 turn 进行中拒绝 |

## 6. 未能确定（施工期要实测）

- 适配器把 `settingSources` 写死为三层，意味着**项目级 settings 一定会被加载**。
  若某个档需要"忽略项目设置"，现有适配器做不到（除非改用 `--settings` 或给适配器
  提 PR）。第一期接受这个边界。
- 换 `CLAUDE_CONFIG_DIR` 后，历史转录的落盘位置随之改变（`projects/` 在档目录内），
  对"影子会话扫描"的影响需实测（见 `DEV-RELEASE-INSTANCE-BOUNDARY.zh-CN.md`）。
- 托管档目录首次使用时是登录态为空的：API 档无所谓；"另一个订阅账号"档需要用户
  自己先在那个目录里登录一次，codeg 只能给引导，不能代登录。
