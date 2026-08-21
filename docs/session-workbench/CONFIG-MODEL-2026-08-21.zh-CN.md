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

## 8. 提案：档上的「信任本仓库配置」开关（未实现，待实验）

用户 2026-08-21 晚提的问题："不同 settings 位于不同仓库和项目下，我想用项目下的配置怎么办？
为每个 profile 加一个开关，是否允许项目级的 settings 覆盖？"

**先纠正前提**：「想用项目下的配置」**今天已经是默认行为**——`.claude/settings.json`
本来就加载，而且压过用户全局（见 §7）。用户不需要做任何事。

**真正缺的是反向能力**，而且它的价值不在覆盖顺序，在**信任**：
仓库里的 `.claude/settings.json` 能定义 **hooks（任意命令）** 和 permissions。
克隆别人的仓库、在里面开 Claude，等于替对方执行他写的 hook。

### 形状

档上一个开关：**「信任本仓库自带的 Claude 配置」，默认开**。

| 开关 | `settingSources` | 效果 |
| --- | --- | --- |
| 开（默认） | `["user","project","local"]` | 今天的行为，零变化 |
| 关 | `["user"]` | 仓库的 `.claude/settings.json` / `.local.json` 一概不加载，hooks 不执行 |

不给「跟随默认」这个开关：它的定义就是零注入，想控制就建档。

### 机制（源码上成立，**尚未实测**）

适配器 `dist/acp-agent.js`：

```js
const options = {
    systemPrompt,
    settingSources: ["user", "project", "local"],
    ...(thinking !== undefined && { thinking }),
    ...userProvidedOptions,     // ← 在后面展开，所以客户端传的会盖掉上面那行
    ...
}
```

所以 `_meta.claudeCode.options.settingSources` 应该能覆盖写死的三层，
**不需要改适配器**。和 `extraArgs.settings` 走同一个 `_meta` 入口。

### 开工前必须先做的实验（L13）

建一个临时项目目录，放 `.claude/settings.json` 带一个可观测标记
（例如 `env.CODEG_PROJECT_PROBE`，或一个会打印的 SessionStart hook），
然后两次 `session/new`：一次不传 `settingSources`，一次传 `["user"]`，
看标记是否消失。**验完再动工**，不要照着源码就写。

### 顺带

`settingSources` 若真能传，也解释了另一件事：`--settings`（档）和项目层是两个正交的旋钮，
一个管"加什么内容"，一个管"从哪些地方加"。文案上要分清，别混成一句话。
