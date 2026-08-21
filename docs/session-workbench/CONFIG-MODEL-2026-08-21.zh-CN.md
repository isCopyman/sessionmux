# codeg 配置模型（定稿，2026-08-21）

用户在实机里看不懂配置面板，连问了五轮。这份文档记下**最终只保留哪些概念**，
以及每条被删掉的层为什么该删。后续任何"再加一层"的提案，先读这里。

用户原话：

> codeg 里就是要么就跟随 cli，要么就是 codeg 里独立的配置。
> 不就是切换配置吗，怎么这么复杂。
> 实际上 codeg 里的这些设置不也是迁移的各种 settings.json？

## 0. 一句话

**一个接入配置档 = 一份 codeg 自己拥有的 settings.json。** 除此之外只有「跟随 CLI」。
没有第三种。

## 1. 用户能看见的两种东西

| | 跟随 CLI | codeg 档 |
| --- | --- | --- |
| 是什么 | 零注入、零写入 | codeg 目录里的一份 `settings.json` |
| 怎么生效 | 什么都不传 | `--settings <档的 settings.json>` 叠加（见 §1.1） |
| 谁在管 | 你自己的 `~/.claude` 和项目级配置 | codeg |
| 面板里 | 页签下是你自己的 settings.json 编辑器 | 页签下是这份档的编辑器 |
| 切换粒度 | —— | **每个会话**（输入框上方的 chip） |

### 1.1 生效机制（2026-08-21 下午核实，推翻了两个早先的错误结论）

codeg 启动 Claude 走 ACP 适配器 `@zed-industries/claude-agent-acp`，不是 `claude` CLI 本体。
但适配器把 `session/new` 的 `_meta.claudeCode.options` **透传进 SDK**：

```js
// dist/acp-agent.js
const userProvidedOptions = params._meta?.claudeCode?.options;
const options = { settingSources: ["user","project","local"], ...userProvidedOptions,
                  extraArgs: { ...userProvidedOptions?.extraArgs, "replay-user-messages": "" }, ... }
```

而 SDK（`@anthropic-ai/claude-agent-sdk/sdk.mjs`）把 `extraArgs` 逐条渲染成 argv：
`... else h.push(\`--${key}\`, value)`。

**所以 `_meta.claudeCode.options.extraArgs = { settings: "<绝对路径>" }` ≡ `claude --settings <path>`**，
也就是 Monet 的机制（O39 §2，`streaming.rs:924-926`）。

**实测（2026-08-21，不是源码推理）**：直接驱动 ACP 适配器
`@agentclientprotocol/claude-agent-acp@0.69.0`（codeg 实际装的那个版本），
`initialize` → `session/new` 带 `_meta.claudeCode.options.extraArgs.settings=<file>` → `session/prompt`，
只换那份文件的内容：

| settings 内容 | 结果 |
| --- | --- |
| `{"env":{"CODEG_SETTINGS_PROBE":"x"}}` | 几秒回 `pong`（走 `~/.claude` 的 OAuth 订阅） |
| `{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:1"}}` | 挂 90 秒零输出 |
| `{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-ant-probe-invalid-0000"}}` | **`401 Invalid bearer token`** |

401 是决定性证据：那个假 token 被真的拿去认证了。第一行同时证明**不写 token 的档就是订阅档**。

被推翻的两条（都是我说错的，记在这里防止再犯）：

| 说过的 | 实际 |
| --- | --- |
| 「Claude 只能指目录，不能指单个 settings 文件」 | 错。`claude --settings <file-or-json>` 存在，`--help` 有 |
| 「codeg 档用不了订阅」 | 只对 `CLAUDE_CONFIG_DIR` 方案成立。`--settings` 是**叠加**，家还是 `~/.claude`，OAuth / projects / todos / skills 都在，**不写 token 的档就是订阅档** |

顺带一条用户实测 + 源码核对的结论：「settings 里有 URL 就静默掉官方登录」——现象对，**触发条件是 token 不是 URL**。
`isAnthropicAuthEnabled()` 看的是 `ANTHROPIC_AUTH_TOKEN` / `apiKeyHelper` / `ANTHROPIC_API_KEY` /
Bedrock·Vertex·Foundry。只是中转 URL 和 token 总是一起写，看着像 URL 触发。

**后果（前端文案要照顾）**：档不是白纸——用户自己 `~/.claude/settings.json` 里的 hooks / statusLine /
permissions 仍然生效，档叠在上面。多半是好事（切档不丢 hooks），但要说清楚。

档的编辑器 = **名称 + Base URL + API Key + 模型**（Key 要掩码所以必须是独立字段）
\+ 折叠的 **settings.json 全文编辑器**。

`Haiku/Sonnet/Opus 别名`、`effortLevel`、`自定义模型`、`hooks`、`permissions`、`statusLine`
**一律不做独立字段**——它们本来就是 settings.json 里的键，JSON 编辑器天然覆盖。

## 2. 被删掉的层，以及为什么

### ① agent 全局连接配置（`agent_setting.env_json` 里的 `ANTHROPIC_*`）

**已删（O69-A 迁移）。** 这一层最坏：

- 它既不是用户的配置文件，也不属于任何一份档，是 DB 行 + spawn 时注入。
- `ANTHROPIC_AUTH_TOKEN` 的优先级**高于**配置目录里的 OAuth 登录态
  （SDK `Du()` 顺序）。用户建一个"走订阅"的档，实际还在烧中转的钱。
- 「官方直连」这个虚拟档**只是为了绕开它**才存在的。

迁移：这些键搬进一个叫 `imported` 的档并设为默认，`env_json` 里删掉，
非连接类键（`CLAUDE_CODE_GIT_BASH_PATH`、`DISABLE_TELEMETRY` 等）一个不动。

### ② 虚拟档 `official-direct`

**已删（不再出现在 list）。** ①搬走之后，「跟随 CLI」本身就等于官方直连。
kind 与解析语义保留，已绑定的会话不会炸。

### ③ 档的 kind 选择器（`configDir` / `managed`）

**UI 已删。** `configDir`（指向别人维护的目录）是"链接"语义、双向；
用户明确选了单向导入。后端仍解析 configDir 档，面板只是不再新建它。
`configDir` 也是唯一还用 `CLAUDE_CONFIG_DIR` 的 kind——那正是它的语义（换一个完整的家）。

### ④ 档自己的 `env` 自由表

**不暴露。** env 本来就是 settings.json 的一个块。档里同时有 `env` 和 `settingsJson.env`，
等于把刚拆掉的"两个入口打架"原样搬进档里。后端字段保留为实现细节，前端不发。

### ⑤ Claude 的 codeg env 覆盖层（面板上那个 textarea）

**折叠。** 对 Claude 冗余（它自己的配置文件就有 env 块，而且**文件赢**）。
不直接删是因为可能有只存在于 DB、没镜像进文件的键。其它 agent 保留展开——
它们没有能写 env 的配置文件，这是唯一通路。

## 3. 覆盖顺序（现状事实，代码位置带上）

`src-tauri/src/commands/acp.rs:8817 build_runtime_env_from_setting`：

```
1. agent_setting.env_json            ← codeg 的 env 覆盖层
2. 配置文件的 env 块覆盖上去          ← 非空才覆盖，空串跳过（清不掉）
3. 配置文件的 apiBaseUrl / apiKey 再覆盖
4. 档的连接配置（O66-A apply_claude_profile_env）
```

**文件赢过 codeg 的 env 覆盖层。** 面板上 `envVarsScope` 那句文案原本写反了
（说 overlay 赢），已在本批改正——这条错误文案本身就是用户困惑的来源之一。

另有同向的第二重：Claude CLI 启动时会把 settings.json 的 `env` 写进进程环境（`pxq()`），
所以就算 codeg 不注入，文件里的也会赢。

②搬走之后这条链只剩「档 or 你自己的文件」，不再有打架。

## 4. 导入，不是链接

用户拍板：**单向导入，不做双向同步。**

`claude_settings_read(path?)` 读一份现成的 `settings.json`（默认读 `~/.claude/settings.json`），
原文回给前端，直接变成一个档。密钥不明文回传；掩码形状的键在建新档时被丢弃，
并通过 `droppedSecretKeys` 告诉 UI 哪几个要重填。

「链接一个目录、双向生效」= `configDir` 档，见 §2③，UI 不再提供。

## 5. 明确不做

- 不再往用户的 `~/.claude/settings.json` 写任何东西（O59-A 已把 Claude 的 cascade 默认关掉）。
- 不给档加第二张 env 表。
- 不给 Haiku/Sonnet/Opus/effort/自定义模型各造一个独立字段。
- 不做双向同步。
- 不改 DB schema。

## 6. 适配器原生的 `providers/*`（2026-08-21 傍晚发现，未采用，记着）

用户给了 [claude-agent-acp PR #1002](https://github.com/agentclientprotocol/claude-agent-acp/pull/1002)
（"switch providers for loaded Claude sessions"，2026-08-17 合并）。适配器有一套原生的
客户端托管路由：`providers/list` / `providers/set` / `providers/disable`
（实现里叫 `unstable_listProviders` / `unstable_setProvider` / `unstable_disableProvider`，
带 `unstable_` 前缀）。

**我们装的 0.69.0 有这三个方法，但没有那个 PR 的改进。** 实测（grep dist/acp-agent.js）：
`clearAuth` / `restoreAuth` / `apiKeyHelper` / `forceLoginMethod` 一个符号都没有，
所以"停用竞争认证再恢复"和"重建已加载会话"都还没进这一版。

**和 `--settings` 叠加是互补关系，不是替代：**

| | `--settings` 叠加（O69-C，已上） | `providers/set`（未采用） |
| --- | --- | --- |
| 覆盖面 | 整份 settings.json：模型别名、effortLevel、permissions、hooks、env | 只有 `apiType` / `baseUrl` / `headers` |
| 作用域 | 每次 spawn（`session/new` 的 `_meta`） | 进程级（codeg 一会话一进程，≈会话级） |
| 时机 | 启动时 | 可在会话中途调用（新版才重建在跑的会话） |
| 能否清掉用户 settings.json 里的 token | **不能**（见 O69-C §2.1） | 新版能（本版没有） |

**采纳条件**：升级适配器到含 #1002 的版本之后，用 `providers/set` 补 §2.1 那个洞
（纯订阅档不受用户 settings 里 API token 影响），`--settings` 继续承担其余部分。
`unstable_` 前缀意味着形状可能变，接的时候要做能力探测而不是硬调。

顺带记一条：同一份 `initialize` 结果里还宣告了
`sessionCapabilities: { close, delete, fork, list, resume }` —— **`fork` 是适配器原生的**，
跟 P7 的 rewind/fork 课题直接相关，动工前先读这里。

## 7. 完整层级（2026-08-21 晚补，用户发现面板在这点上撒谎）

用户问：「跟随默认」显示的是全局 settings.json，会不会被项目设置覆盖？**会。**

适配器 `dist/acp-agent.js` 里 `settingSources: ["user", "project", "local"]` 是**写死的**
（实测 grep，全文件仅此一处），三层全加载。配合 Claude 文档的同键优先级，
一个 codeg Claude 会话的实际层级是：

```
1. Managed（企业策略）                        最高
2. --settings              ← codeg 档在这一层
3. .claude/settings.local.json（项目本地）
4. .claude/settings.json（项目，进仓库那份）
5. ~/.claude/settings.json（用户全局）         ← 「跟随默认」页签显示/编辑的就是这层，最低
```

**两个后果：**

- **「跟随默认」显示的是最底层。** 同一份全局设置在不同 cwd 下效果可能不同——
  仓库里 committed 了 `.claude/settings.json` 的项目会盖掉它。面板原文案
  「对所有『跟随默认』的会话生效」把人误导成"这就是会生效的东西"，已改（十语）。
- **codeg 档压过项目设置。** 这是档的一个好性质：仓库里 committed 的 settings
  盖不掉用户选的档。原来也没说，一并写进文案。

**还没做**：面板只显示单层，不显示合并后的实际生效值。要做「有效配置」视图的话，
需要后端按会话 cwd 去读项目层再合并——记为待办，不在本批。

## 8. 提案：「应用项目自带的 .claude 配置」开关（未实现）

用户 2026-08-21 晚：「如果想用仓库里的 settings.json 怎么办？不同 settings 位于不同仓库下……
加一个开关？」

**先纠正前提**：「想用项目下的配置」**今天已经是默认行为**——`.claude/settings.json`
本来就加载，而且压过用户全局（§7）。用户不需要做任何事。缺的是**关掉它**的能力。

### 这不是信任闸门（我先走错过一次）

我一度把它改写成安全问题，并提议复用 pi 那套 project-trust
（`pi-project-trust-banner.tsx` + `acp.rs:5605 pi_project_trust_launch_block`）。
**用户否决，理由成立**：pi 那套是默认拒绝、拦住启动、逼用户答复的**闸门**，
因为 `.pi/extensions` 是启动即执行的代码；我们这个是**偏好**，默认开，
今天本来就是这个行为。做成闸门会改掉默认行为、多一个挡路横幅，比需求大得多。

保留这段是因为 pi 那套确实存在且是按文件夹记忆的——**将来**若真要做仓库信任，
它是现成样板，但那是另一件事，别和本条混为一谈。

### 形状

- 一个普通开关：**「应用项目自带的 .claude 配置」，默认开**。
- **放会话上**（和档 chip 同排），设置面板里只放"新会话默认值"。
  理由：它跟着 **cwd** 走，不跟着档走。同一个中转档在自己仓库和 clone 来的仓库
  该有不同答案；放档上会逼用户为同一个端点建两个档，分类维度是错的。
- 不拦启动、不弹询问横幅。

| 开关 | `settingSources` |
| --- | --- |
| 开（默认） | `["user","project","local"]`（不传，用适配器默认） |
| 关 | `["user"]` |

### 时序（用户问到的点）

`settingSources` 和 `--settings` 一样只在 `session/new` 的 `_meta` 里传一次，
**连接建立后改不了**。所以这个开关必须复用已有的
`AcpEvent::SessionConfigStale { stale, kind }` → `SessionConfigStaleBanner`
（`acp/manager.rs:1041`、`src/components/chat/session-config-stale-banner.tsx`），
和换档走同一条路：标 stale，横幅提供重启。不要另编一套。

### 机制（2026-08-21 晚**实测通过**）

`dist/acp-agent.js`：`...userProvidedOptions` 在写死的
`settingSources: ["user","project","local"]` **之后**展开，所以客户端传的会盖掉它，
**不需要改适配器**。

实测：临时项目目录 `.claude/settings.json` 写一份带无效 token 的 `env`，
直接驱动适配器（入口是 **`dist/index.js`**，不是 `dist/acp-agent.js`——踩过一次坑），
同一目录跑两次 `session/new` + `session/prompt`：

| 传的内容 | 结果 |
| --- | --- |
| 不传 `settingSources` | **`401 Invalid bearer token`** —— 项目层被加载 |
| `settingSources: ["user"]` | **`pong`** —— 项目层被跳过 |

正向信号两侧都有，结论确定。探针留在
`C:/Users/63036/AppData/Local/Temp/proj-probe/`。

### 无关但顺带记

档记录是 `claude-profiles/<id>/` 下的 **JSON 文件**（`claude_profile.rs:433 read_record`），
不是数据库表——给档加字段只是 serde 加 `#[serde(default)]`，**零迁移、不需要用户签字**。


## 9. 优先级倒挂：实测 + 用户拍板的目标序（2026-08-21 深夜）

### 9.1 实测（两侧正向信号，不是推断）

临时项目目录下放 `.claude/settings.json`，`env.ANTHROPIC_BASE_URL` 指向
`127.0.0.1:4711`；另写一份 overlay 指向 `127.0.0.1:4712`。两个端口各起一个只记日志、
一律回 401 的监听器，看哪个被打到。

| 跑法 | 命中 |
| --- | --- |
| `claude -p "say pong"`（不带 overlay） | **PROJECT** `4711 POST /v1/messages` |
| `claude --settings overlay.json -p "say pong"` | **OVERLAY** `4712 POST /v1/messages`，4711 一次没碰 |

对照组是必须的：只跑第二行的话，4711 没命中也可能是"目录没被信任、项目 settings 压根
没加载"。第一行证明它会加载。

**结论：`--settings` 压过项目层。所以 codeg 当前的实际优先级是**

    ~/.claude(用户全局)  <  项目 .claude  <  codeg 档   ← 倒挂

### 9.2 用户拍板的目标序

用户原话：「codeg 视为全局设置，然后项目级的设置肯定要覆盖全局设置啊，怎么可能是全局
设置覆盖项目设置」「即全局用户级 < codeg < 项目级」。就是 VSCode 的
「用户设置 < 工作区设置」。目标：

    ~/.claude(用户全局)  <  codeg 档  <  项目 .claude/settings.json  <  .claude/settings.local.json

### 9.3 落地方案（不改机制，改我们塞进 overlay 的内容）

`--settings` 只有一层且恒在顶上，改不了它的位置，所以**由我们自己把项目层折叠进
overlay**：

| 有 overlay | 开关 | `settingSources` | overlay 内容 |
| --- | --- | --- | --- |
| 有 | 开 | `["user"]` | 档 + 项目 `settings.json` + `settings.local.json` 深合并叠上 |
| 有 | 关 | `["user"]` | 只有档（不读项目文件） |
| 无 | 开 | 省略（适配器默认 user/project/local） | — |
| 无 | 关 | `["user"]` | — |

**有 overlay 时 `settingSources` 恒为 `["user"]`**：项目层已经由我们折叠进去了，不能让
适配器再加载一遍。用户全局留在最底下，正好对上 9.2 的目标序。

深合并规则：两边都是 object 就递归；其余（数组、标量、类型不一致）高层直接替换。

### 9.4 顺带排掉的一个坑

SDK 文档里 `settingSources: []` 会连项目 `CLAUDE.md` 一起不加载。**实测 CLI 不是这样**：
临时目录放 `CLAUDE.md`（"问 codeword 就回 MANGO"），`claude -p "codeword?"` 与
`claude --setting-sources user -p "codeword?"` **都回 MANGO**。
所以关掉项目 settings **不会**顺手砍掉项目指令。


## 10. 撤回 §9.3：折叠方案不做（2026-08-21 用户拍板）

### 10.1 §9 漏掉的关键事实

`--settings` 是**逐键叠加**，不是整份替换。官方文档原话：它填充的是
"flag-settings layer"，**全局和项目 settings 仍然加载**，只是被这一层压过冲突的键。
官方优先级：

    Managed（企业） > --settings / SDK options.settings > local > project > user

§9 只测了"两边都设 `ANTHROPIC_BASE_URL` 时谁赢"，就推出"项目层被整体压过"。
**这个推论过头了**：档没设的键，项目照样生效、照样压过用户全局。

### 10.2 因此当前行为已经是对的

档设了的键 → 档赢（这正是用户在 UI 里做的显式选择）；
档没设的键（permissions / hooks / statusLine / …）→ 项目赢过用户全局。

用户原话：「直接用 `--settings` 管理即可，即就是用 codeg 里的设置，codeg 里设置了
什么就用什么」。**主仓不用改。**

### 10.3 折叠方案作废

lane `cli-delegate-settings-overlay` 的 `e8bdbd38`（深合并 + spawn 折叠 +
`settingSources` 恒 `["user"]`）**不合并**。理由三条：

1. **多余** —— 见 10.2，`--settings` 本身就给了想要的语义。
2. **有并发缺陷** —— 它原地回写 `claude_profiles_dir/<档id>/settings.json`，
   那是**按档共享**的文件。两个会话用同一个档、在不同仓库并发启动会互相串配置。
   （对照：Monet 的做法是每次 spawn 合成 `~/.monet/runtime/<sessionId>-<nanos>.json`、
   用完即删——per-spawn 文件才是对的形状。）
3. **会砍掉项目的 skills / hooks** —— `settingSources` 的 `project` 源管的不只是
   settings.json，还有 `.claude/` 下的 skills 和 hooks。折叠只搬得动 settings.json 的
   键，搬不动 skills/hooks。开关明明是「开」，却把项目的 skills/hooks 静默关掉，
   是比原问题更糟的 bug。

### 10.4 `.claude/CLAUDE.md` 到底受不受 settingSources 管——**未能证实**

两次探针（项目根 `CLAUDE.md`、`.claude/CLAUDE.md`）在 `--setting-sources user` 下
都仍然生效。但 auto memory 无论 settingSources 如何**始终加载**，两个机制混在一起，
探针分不开。**结论：未能分离，不要引用 §9.4 那条"已排掉的坑"。**
（§9.4 的原始表述过于自信，此处更正。）

## 11. 空串能压掉下层——「官方订阅」必须显式写空(实测)

### 11.1 问题

§10 说清了 `--settings` 是**按键叠加**。由此推出一件当时没想到的事：

**档里「没填 Base URL / API Key」并不等于「走官方订阅」。**

没填 = 这一层对该键没意见 → CLI 继续往下读项目级、用户级。任一层设了网关，
session 就走那个网关。用户在 UI 上看到的「官方订阅」标签是假的。

那能不能用空串把下层压掉？两个子问题，都得测，不能猜：

- 合并语义：叠加层的 `""` 会覆盖下层的非空值，还是被当成「没写」跳过？
- 读取语义：CLI 拿到 `ANTHROPIC_BASE_URL=""` 是回落官方，还是拿空 URL 去请求然后炸？

### 11.2 测量

复用 §9 的探针（`/tmp/prec-probe`）。项目层 `.claude/settings.json` 指向本地
`127.0.0.1:4711`，监听器记录命中并回 401。改叠加层内容，跑
`claude --settings <overlay> -p "reply with the single word OK"`。

| 叠加层内容 | 4711 命中 | CLI 行为 |
|---|---|---|
| `{"env":{"CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY":"1"}}`（对照，不碰这两个键） | **7 次** | 走项目层网关 |
| `{"env":{"ANTHROPIC_BASE_URL":"","ANTHROPIC_AUTH_TOKEN":""}}` | **0 次** | 正常答 `OK` |

对照组证明探针是活的、项目层确实生效。实验组两件事同时成立：

1. **空串赢下合并** —— 项目层的 4711 被压掉了，一次都没打到。
2. **CLI 把 `""` 读成「未设置」** —— 干净回落官方订阅并**成功返回**，
   不是拿空 URL 去请求然后报错。

（用户全局 `~/.claude/settings.json` 已核实**不含** `ANTHROPIC_BASE_URL` /
`ANTHROPIC_AUTH_TOKEN`，用户层不构成混淆。）

### 11.3 结论：连接方式是三态，不是两态

| 状态 | settings.json 里的样子 | 含义 |
|---|---|---|
| 走本档网关 | `BASE_URL`/`TOKEN` 有值 | 明确指向某个第三方端点 |
| **强制官方订阅** | 三个连接键显式 `""` | 压掉项目级和用户级 |
| **不表态** | 键不在文件里 | 继承项目级 / 用户级 |

`ANTHROPIC_API_KEY` 也要一起写空——它是第二种凭据拼写，只清 `AUTH_TOKEN`
会让带 API_KEY 的下层配置活下来。

### 11.4 当前代码是错的

`src/components/settings/claude-settings-projection.ts`：

- 写路径 `applyClaudeConfig`：`authMode === "official_subscription"` 时执行
  `delete env[key]`（三个连接键）。**这产出的是第三态「不表态」，却贴第二态的标签。**
  在有项目级网关的仓库里，用户选了「官方订阅」并保存，会话照样在计费网关上。
- 读路径 `readClaudeConfig`：`authMode: baseUrl || authToken ? "custom" :
  "official_subscription"`。它只看本档文件就下「官方订阅」的结论，把第三态误报成第二态。

修法：三态显式建模，「官方订阅」写 `""` 而不是 delete，「不表态」才是 delete。
