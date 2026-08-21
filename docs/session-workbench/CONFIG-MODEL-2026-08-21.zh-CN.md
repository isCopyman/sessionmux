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
| 怎么生效 | 不设 `CLAUDE_CONFIG_DIR` | `CLAUDE_CONFIG_DIR` 指过去 |
| 谁在管 | 你自己的 `~/.claude` 和项目级配置 | codeg |
| 面板里 | 页签下是你自己的 settings.json 编辑器 | 页签下是这份档的编辑器 |
| 切换粒度 | —— | **每个会话**（输入框上方的 chip） |

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
