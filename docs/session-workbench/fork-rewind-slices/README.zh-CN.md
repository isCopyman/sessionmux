# Fork/Rewind 切片 2–4：调度台账

> 事实源。协调者（Codeg Session 314，Claude Opus 5）与协作者（Grok Build /
> grok-4.6）都以本文件为准；任何人的记忆都不算数。上下文压缩后从这里恢复。
>
> 分支：全部从 `wt/fork-rewind` 切。**禁止 push，禁止 `pnpm tauri build`。**

## 为什么第一轮是调研而不是写代码

SURVEY §6 把切片 2–4 排成了"claude 按消息 fork / codex thread-fork / 其余家降级"，
但同一份 SURVEY 在 §2、§4 里留了四个**必须实测才能回答**的问题，它们决定这三条路
里哪几条真的存在：

1. claude-agent-acp 0.69.0 是否仍声明 `sessionCapabilities.fork`（registry 注释停在
   0.67 的快照，没重申）；
2. codex-acp 1.4.0 是否声明 ACP `session/fork`（注释明确"未记录，需实测"）；
3. `session/fork` 的 `_meta` 能不能承载消息锚点，两家适配器是否会透传；
4. codeg 现在到底有没有可用的消息锚点——SURVEY §5 说"没持久化任何 provider
   anchor"，但没说 parser 输出层有没有。

在这四个问号闭合前写 forkAtMessage，等于赌协议形状。所以第一轮三个包全是**只读调研 +
写文档**，产出是可复核的事实表；第二轮才按结论派编码包。

协调者已先行核过一条（见下），证明这批问号确实有货。

## 已核事实（协调者，2026-08-20，直接读代码）

- `UnifiedMessage.id` / `MessageTurn.id` 都是 `String`（`models/message.rs:179, 209`），
  字段本身**已经存在**，问题只在于各 parser 往里塞什么。
- **claude 已经在消息级投影里带原生 uuid**：`parsers/claude.rs:1339, 1491, 1541`
  直接 `id: uuid`。
- **claude 的 turn 级投影丢了它**：`parsers/claude.rs:2585` 是 `format!("turn-{}", ...)`。
- **codex 完全没有原生锚点**：`parsers/codex.rs:2400, 2429, 2489, 2584, 2783` 全是
  `format!("user-{}", messages.len())` 这类纯位置合成 id。
- `turn-<digits>` 这个命名空间**已经承重**：`acp/manager.rs` 的 `is_reserved_turn_id`
  专门挡住客户端伪造该形状的 id。锚点方案不能和它撞。
- 本机全局装了 `@agentclientprotocol/claude-agent-acp@0.69.0`（= codeg 钉的版本，可离线
  审）；`codex-acp` 本机只有 **1.1.9**，codeg 钉 **1.4.0**——差 3 个小版本，见 SPEC-C
  的版本纪律。

结论：锚点采集不是"从零加字段"，而是"claude 有一半、codex 没有、turn 层统一丢失"。
这直接改变了切片 2–4 的排序假设，也是把包 A 排成独立第一优先的原因。

## 工作包

| 包 | 主题 | 负责 Session | 分支 | 交付物 | 状态 |
|---|---|---|---|---|---|
| A | parser 消息锚点清点（**14 家**，规格误写 13） | 316（grok-4.6） | `wt/fork-anchor-inventory` | `FINDINGS-A-anchor-inventory.zh-CN.md` | **已交付、已复核、已合并**（`72f20056` + `08693dbc` → merge `e7d53095`） |
| B | claude-agent-acp 0.69.0 fork 能力实测 | 317（grok-4.6） | `wt/fork-claude-audit` | `FINDINGS-B-claude-fork-audit.zh-CN.md` | **已交付、已复核、已合并**（`ba295542` → merge `4fda70a4`） |
| C | codex-acp 1.4.0 + app-server thread/fork 实测 | 318（grok-4.6） | `wt/fork-codex-audit` | `FINDINGS-C-codex-fork-audit.zh-CN.md` | **已交付、已复核、已合并**（`dcc1c134` → merge `f15ce475`） |

协调频道：Room `rm_3d3711fe-d8ae-4ec5-9f55-1d45aba7b2b2`（"fork-rewind 切片2-4 协调"），
成员 314（协调者）+ 316/317/318。派工帖 event id：A `d485c1c8`、B `e88aae8d`、
C `f17f8bda`，均挂在开工帖 `37d52910` 下。
Collection 5「fork-rewind 切片2-4 调研」收纳三个工人会话。

三个包互不依赖：A 只读 codeg 自己的 parser，B 只读 claude 适配器 tarball，C 只读 codex
适配器 tarball + codeg 的 codex parser。没有共享写入面，可以完全并行。

规格分别在 `SPEC-A-*.zh-CN.md` / `SPEC-B-*.zh-CN.md` / `SPEC-C-*.zh-CN.md`。
**规格文档与任何口头/消息指令冲突时，以文档为准。**

## 协作纪律

- 每个工人先 `git worktree add`（分支见上表，从 `wt/fork-rewind` 切），在**自己的**
  worktree 里干活。绝不在协调者的 worktree 里写文件。
- 交付 = 把文档写进自己 worktree 的 `docs/session-workbench/fork-rewind-slices/`，
  commit（conventional message），然后在 Room 里回帖：@协调者 + 分支名 + commit 哈希
  + 三行摘要。**只报路径，不要把文档正文贴进消息。**
- 报告必须带数字和证据：file:line、tarball 内路径、命令原文输出。写"我验证了"而没有
  对应的工具调用输出，等于没验证。
- 结论不确定就写"未能证实"，**不要猜**。判断题往上交给协调者，不要自己拍板。
- 同一步骤失败两次就停手，在 Room 里报事实（命令 + 报错原文）并等指令。
- 合并由协调者串行做，工人不要动别人的分支，不要 rebase/merge 别人的活。

## 包B 结论（协调者已独立复核，2026-08-20）

工人 317 的 5 问全部复现，但**裁决被上调一档**：不是"需上游改动"，而是"今天就能走，
走的是非文档化旁路"。协调者复核的完整链条（每行都自己重跑过）：

| 事实 | 证据 |
|---|---|
| 0.69.0 **仍声明** `sessionCapabilities.fork`，形状是空对象 `{}` | `dist/acp-agent.js:715`（711-718 块内） |
| ACP `session.fork` 路由到 `unstable_forkSession` | `:6819` → `:756` |
| 它**不调** SDK `forkSession()`，走 `createSession(..., {resume, forkSession:true})` | `:757-765` |
| `_meta` 被原样透传进 `createSession` | `:761` |
| SDK **有**消息级 fork：`forkSession(_sessionId, {upToMessageId})` | `sdk.d.ts:699, 702, 709` |
| SDK **有**截断式 resume：`resumeSessionAt`（+ 校验参数 `resumeDropsTurn`） | `sdk.d.ts:1843, 1894` |
| adapter **从不**传这两个（`grep` 零命中） | — |
| adapter 自己承认在建 messageId→uuid 映射但"Not read yet" | `:2982-2985, 5154` |

**关键旁路（协调者静态追完，工人只标了"未验证"）**：

```
4817  const options = {
4821      ...userProvidedOptions,   // = _meta.claudeCode.options ← resumeSessionAt 从这进
4835-4870 cwd/mcpServers/permissionMode/extraArgs…   // ACP 强控覆盖清单
4936      ...creationOpts,          // = {resume: 父 id, forkSession: true}
4938  };
4952  query({ prompt: input, options })
```

`resumeSessionAt` / `resumeDropsTurn` **不在** 4835-4870 的覆盖清单里，原样活到 `query()`。
该通道非野路子：`:748` 显式读 `_meta.claudeCode.options.resume`，`:4942` 注释称其为
"SDK pass-through"。

**⇒ codeg 不改上游即可试 forkAtMessage**：`session/fork` 带
`_meta.claudeCode.options.resumeSessionAt = <锚点 uuid>`。

**三颗雷（`sdk.d.ts:1836-1894`），必须进 RFC**：

1. **静默失效**：这对参数 **PRINT/HEADLESS LANE ONLY**。交互式 `claude --resume` 与后台
   job **忽略它们——加载完整历史，不截断、不报错**。走错 lane = "看起来 fork 成功，实际
   没截断"。
2. **拒绝不可重试**：`resumeDropsTurn` 校验失败给 `Resume rejected by --resume-drops-turn:`
   开头的错误，**确定性失败**，必须映射到 rewind-recovery，禁止退避重试。
3. **它是 spread 的副产品不是契约**：上游给 `resume*` 加一条覆盖就静默失效。

**锚点形状（改了包 A 的判据）**：`sdk.d.ts:1886-1892` 要求 fork 在
**"被保留那一轮的最后一条 chain entry"**，不是 assistant 消息 uuid——end-turn 工具会话要取
`structured_output` 附件，`shouldQuery:false` 的 transcript append 也算 entry，取早了校验器
**故意拒绝**。已在 Room（`34134260`）通知 316 按此调整判据。

待定（需烧 Anthropic 额度，等用户拍板）：活体验证 codeg 走的是否确为 print lane。

## 包C 结论（协调者已独立复核，2026-08-20）

工人 318 守住了版本纪律（这是本包最担心的失败模式）：`npm pack` 取 1.4.0 到临时目录、
没装全局、没起进程。协调者复核：`package.json` version = 1.4.0 ✓；tgz shasum
`f7c4364a9c5a6e2836e847cd9ac474b807c2e8ca` 与 npm `dist.shasum` 逐字符相同 ✓。
（工人把该 shasum 标成了 `dist/index.js` 的，实为 tarball 的——数字对，对象标错。）

| 事实 | 证据（1.4.0 tarball 内 `dist/index.js`） |
|---|---|
| **不声明 ACP fork**：广告面只有 `{resume, list, close, delete, additionalDirectories}` | `:29697-29703` |
| `:3710` 的 `session_fork: "session/fork"` 只是方法名常量表，不是声明 | `:3710` |
| `thread/fork` 唯一 caller 在 AIR 报告路径 `runAgentFileChangeReport` | `:27569` → `:27579` |
| 参数含 **`lastTurnId: params.turnId`**（registry 注释漏了它和 `developerInstructions`） | `:27579-27587` |
| `thread/rollback` **零调用点**，只有 `threadRollbackFailed` 错误码 | grep |
| codeg parser：`fork_turns` 只在注释与夹具中，**代码从不读** | `parsers/codex.rs:1826, 6753` |
| codeg parser：`forked_from_id` 只在单测中，断言恰是"**不是** harness_internal" | `parsers/codex.rs:4310-4316` |
| codeg parser：`parent_thread_id` 被读来做 harness_internal 分类 | `parsers/codex.rs:348` |

**裁决维持「给 codex-acp 提能力」。** 三个上交的判断题裁决：

1. **`lastTurnId` 而非 `messageId` —— SURVEY 错了，工人对。** codex 原生 fork 粒度是
   **turn 级**。这改变产品语义：codex 只能"从这一轮分叉"，不能"从这条消息分叉"。**进 RFC。**
2. **rewind 不押 DEPRECATED 的 `thread/rollback`**（标了 "will be removed soon" 且不回
   文件）。codex 侧 rewind 搁置。
3. `forkedFromId` 谱系记下但本轮不做——切片 1 的 `fork_relation` 表正好能装，等导入侧动。

## 包A 结论（协调者已独立复核，2026-08-20）

**工人挑出了规格的错，已认领**：`ALL_PARSER_AGENTS` 是 `[AgentType; 14]`
（`import_service.rs:27-42`），规格误写"13 家"、误引行号 `:26-58`，漏的是 **Qoder**。

三档（口径：原生文件有无非位置逐条锚，不论今天是否投影）：

- **档1 已有稳定原生锚（9 家）**：ClaudeCode、Qoder、OpenCode、OpenClaw、Hermes、
  Gemini、Pi、DeepSeek、Cursor
- **档2 有 id 但不稳定/不覆盖全部消息（4 家）**：KimiCode、Grok、Codex、CodeBuddy
- **档3 完全没有（1 家）**：Cline

**核心结论（改变了工作量量级）**：问题不是"大家都没锚点"，而是
**"有的家读了再扔、有的家文件里就有却根本不读"**。Claude/OpenCode/OpenClaw/Gemini/
Hermes/Qoder 已把原生 id 写进 `UnifiedMessage.id`，但 `group_into_turns` **无例外**改写成
`turn-{n}`，详情页只见得到后者。Pi 每条 JSONL 都带 `id`/`parentId`（`pi.rs:71-73` 模块
注释自述）却只产 `turn-{n}`（`:695/707/745`）；DeepSeek 同理（`:524/722`）；Cursor 把
turn blob id 读进 `turn_blob_ids` 又写成 `cursor-turn-{i}`（`:1080`）。

⇒ SURVEY §5「codeg 没持久化任何 provider anchor」**对外部 turn 层成立，对 parser 内部
消息层与原生文件不成立**。锚点采集从"造轮子"降级成"别扔"。

**协调者复核额外确认的两处代码事实**：

1. claude turn 合成不止 `:2585`，是 **2551 / 2585 / 2597** 三处（规格只引了一处）。
2. **`acp/manager.rs:61-62` 的注释是错的**：它宣称 `turn-<digits>` 是"every parser
   assigns via `format!(\"turn-{}\", n)`"，但 `cline.rs:285` 用
   `{conversation_id}-{turn_counter}`，`cursor.rs:1080` 用 `cursor-turn-{i}`。
   该注释服务于一段**安全用途**逻辑（挡客户端伪造 `message_id` 撞持久化 turn id）。
   **注释错已确认；是否构成可利用缺口需另开评估**，本轮不下结论。

**§补（工人响应协调者的轮末 chain entry 约束后追加）**：单独标出"拿得到 assistant uuid
但不够当 `resumeSessionAt` 锚"的两家——ClaudeCode（tool_result uuid 进了消息层
`claude.rs:1491` 但被 `group_into_turns` `:2565-2574` 吞掉；`structured_output` 类附件在
`:1551-1590` 整行丢弃）与 Qoder（同形，走同一份 `group_into_turns`）。
`shouldQuery:false` 裸 user append：parser 零读取该字段，真实落盘形态**未能证实**。

## 两个包合看：一个没预料到的不对称（本轮真正的产出）

| | claude-agent-acp 0.69.0 | codex-acp 1.4.0 |
|---|---|---|
| ACP 声明 fork | **是**（`fork: {}`） | **否** |
| 按位置分叉的原生能力 | **有**，`resumeSessionAt` 收任意 chain-entry uuid | **有，但只到 turn 级**（`lastTurnId`） |
| codeg 今天够得着吗 | **够**（`_meta.claudeCode.options` spread 直通） | **够不着**（适配器没接 fork） |
| rewind 原语 | `resumeSessionAt` 截断式 resume | `thread/rollback` **已 DEPRECATED** |
| 要不要动上游 | **不要** | **要** |

SURVEY §6 把 claude 排 2、codex 排 3 的**顺序对，但代价差被严重低估**：claude 是"codeg
侧就能做"，codex 是"提 PR + 等 maintainer + 语义只到 turn 级"。下一轮按此重排编码包。

## 交叉结论：A × B 对撞出的硬约束（没有任何单个包能得出）

协调者已逐条复核 §补：`claude.rs:1491/1541` 确为 `id: uuid`；`:2565-2571` 的
`while is_tool_result_only(…) { blocks.extend(…); i += 1 }` **吸收正文、丢弃 id**（只有
assistant 的 `id` 活到 `:2574`）；`:1551-1591` 的 `attachment` 分支只认
`goal_status_transition`，注释明写其余附件"是给模型的上下文，不是对话"，**整条丢弃**；
`qoder.rs:261-263` 写着附件 "**indexed but not returned**"。

三段事实并排：

1. **包B**：`_meta` 旁路能把 `resumeSessionAt` 送到 CLI，但校验器对错误 fork 点
   **确定性拒绝且不可重试**。
2. **包B/SDK**（`sdk.d.ts:1860-1871`）：end-turn 工具会话一轮结束在 tool_result carrier 上、
   **无尾随 assistant 消息**，其后跟一条 `structured_output` 附件装着该轮真正输出。
   **唯一合法 fork 点就是那条附件**；在 assistant uuid 上分叉会把它留在丢弃区间，
   校验器**故意拒绝**。
3. **包A/§补**：codeg 的 claude parser **恰好把附件整条丢弃**，tool_result 的 uuid 也在
   `group_into_turns` 里被吞掉。

⇒ **对这一类会话，codeg 根本看不到那个唯一能用的 uuid。**

**切片 2 的真实形态因此不是"把 `resumeSessionAt` 塞进 `_meta`"**，而是：

- **必须先做锚点管道**（让附件 / 轮末 entry 的 uuid 活到投影层）；否则
- forkAtMessage 在这类会话上**确定性失败**（且是不可重试的那种），或
- 必须先识别并禁用入口——而识别本身同样需要读到那些被丢弃的记录。

便宜之处：`qoder.rs:261-263` 表明链遍历**本来就步进过附件**（只是不返回），所以"别扔"
远比"重新解析"便宜——与包 A 的核心结论是同一件事的两面。

**方法论备注**：这是本轮唯一一个**没有任何单个包能得出**的结论。B 知道校验器会拒，
A 知道 parser 会扔，只有并排才看得见"我们扔掉的正好是它要求的那条"。并行三包 + 交叉复核
的价值在此，不在三份报告本身。

## 第二轮（用户已认可拆法，2026-08-20）

规格：`ROUND2-SPECS.zh-CN.md`（commit `b740cfe3`）。派工帖 Room event `15d544e4`。

| 包 | 主题 | Session | 分支 | 状态 |
|---|---|---|---|---|
| D | 锚点管道（**切片 2 硬前置**） | 316 | `wt/fork-anchor-pipeline` | 已派工 |
| E | RFC 第二轮更正（纯文档） | 317 | `wt/fork-rfc-round2` | **已交付、已复核、已合并**（`f97312b2` → merge `7cf017a5`） |
| F | `is_reserved_turn_id` 注释与缺口评估 | 318 | `wt/fork-reserved-id-audit` | **已交付、已复核、已合并**（`a68d06e3` → merge `a6a665c8`） |

**包 E 结论（已复核）**：六项必改全部落地，引用逐条与协调者独立复核结果一致。新增
`§7.6 锚点的真实形状` 是本包最有价值的部分，并**多提取了一种协调者漏掉的情况**——中断
回合（已完成的非错误 tool_result 留在尾巴上，停在 assistant uuid 会被故意拒绝）。
§2 已按家标注 forkAtMessage 粒度（Claude 消息级 / Codex 轮级 / 通用 ACP 不承诺），
§9 已把 `fork_relation` 从「拟议」改为「已实现（切片 1）」。

**规格自身第二处笔误（工人 317 指出，已改 `b9ab7670`）**：包 D 的「不做」原写
「不碰 ACP 发送路径（那是包 E…）」——包 E 是纯文档，无发送路径；正确说法是「那属于后续
的 forkAtMessage 包」。§0 第 9 条「发现规格错直接说」两轮内已两次生效
（第一次是包 A 的「13 家 parser」）。

forkAtMessage 本体要等 D 落地，本轮不派。codex 编码等上游，本轮只在包 E 更正文档。

### 包F 结论与协调者拍板（已复核，2026-08-20）

工人结论「**真缺口但低危**」，判定逻辑未改（只改注释 + 表征性测试）。协调者复核：

- 谓词本体一行未动 ✓；14 家 turn 级 id 的 11/3 划分逐家核实 ✓
  （例外：`cline.rs:285` `{conversation_id}-{n}`、`grok.rs:1014` `grok-turn-{i}`、
  `cursor.rs:1080` `cursor-turn-{i}`；Qoder 经 `qoder.rs:828` 复用 Claude 的
  `group_into_turns`，属 `turn-{n}` 一档）
- **低危依据成立**：`commands/conversations.rs:1684-1694` 的 collide 守卫真实存在且
  **与命名空间无关**（按"id 是否已存在于别的 turn"判断）。其注释明写这是对
  `is_reserved_turn_id` 的**纵深防御**，失败模式为 "a recoverable visible duplicate,
  **never a hidden prompt**"。
- 合并后重跑 `is_reserved_turn_id_matches_only_the_parser_namespace`：**1 passed**。

**协调者拍板：暂不放宽 matcher，也不改 allowlist。** 理由：

1. 真正的防线是那个 **namespace-agnostic** 的 collide 守卫，覆盖现在与未来所有 parser
   命名空间；`is_reserved_turn_id` 是第二道而非第一道。
2. 枚举式 matcher 注定再次过期——这条注释正是这么坏掉的，再补三个前缀只是重演。
3. Cline 的 `<数字>-<数字>` 形状过泛，加入黑名单会误伤合法客户端 id，而**误伤的后果
   （prompt 被拒）比现状（旁观窗口短暂重复）更严重**。

**可推翻本裁决的证据**（留给将来）：若发现某条路径上客户端 `message_id` 能进入持久化
投影而**绕过** `apply_in_flight_message_id` 的 collide 守卫，则第一道防线成为唯一防线，
届时必须放宽或改 allowlist。本轮未查该问题（不在规格范围内）。

### 活体探针结果（用户批准的一发，已用掉）

**收获大于原计划，且主要来自零成本的静态部分：**

- **print lane 那颗雷静态排除。** `sdk.mjs:118` 把 `resumeSessionAt` →
  `--resume-session-at=` argv；SDK 用 `["--output-format","stream-json","--verbose",
  "--input-format","stream-json"]` + ProcessTransport 起 CLI，正是 SDK 文档所称
  "print-mode CLI, Agent SDK, ProcessTransport" 的武装 lane。整条链
  `_meta.claudeCode.options.resumeSessionAt` → `acp-agent.js:4821` spread → SDK Options
  → `sdk.mjs:118` argv → CLI **全部静态贯通**。
- **探针本身未中。** 隔离 scratch 目录（`%TEMP%/fork-lane-probe`）起 `claude -p "hi"`，
  transcript 只有 user + 4 条 attachment，**无 assistant 记录**，turn 没跑完。诊断：本机
  `ANTHROPIC_BASE_URL=http://127.0.0.1:8317` + `ANTHROPIC_AUTH_TOKEN`，spawn 的 claude
  继承了本地代理 env。**按预算纪律未重试。** "是否真截断"留到包 D 落地后从 codeg 内部驱动。
- **意外实测收获（比原计划要验的更重要）**：真实 transcript 一条 prompt 之后链尾是
  **四条各带 uuid、以 parentUuid 串联的 attachment**——
  `deferred_tools_delta` / `agent_listing_delta` / `skill_listing` /
  `total_tokens_reminder`，正是 `claude.rs:1552-1554` 注释点名、并被整条丢弃的那几种。
  **A×B 交叉约束由此获得真实数据佐证。**
  另附带实锤了 `background_watch.rs:1256` 说的文件头 `queue-operation` 元数据（记录 0、1，
  且**无 uuid**）。
- **诚实边界**：本次无 assistant 记录，故只证明了 attachment 会挂在 user 之后，
  **未证明**完成的 turn 末尾也是 attachment。后者仍是待验项。
- 证据文件保留在
  `~/.claude/projects/C--Users-63036-AppData-Local-Temp-fork-lane-probe/d71224df-….jsonl`。

## Dogfooding 摩擦记录

组队阶段实际踩到的 codeg 工具/流程摩擦，逐条发在 Room 里（前缀【摩擦】，
event `e199466b`）。摘要：

1. `session.create` 的 `harness` 没有可发现的取值域，且仓库里 Grok 有两套 id
   （`registry_id_for()` 给 `grok-build`，实际接受 `grok`）——只能靠 `list_sessions`
   旁证反推。
2. Room 花名册把"整条首 prompt 截断"当会话标题，人类手打长 prompt 起的会话完全无法区分。
3. "先建会话还是先建 Room"是鸡生蛋：skill 推荐 `initial_prompt` 携带 room 信息，但
   `room.create` 需要成员 id，room_id 写不进 `initial_prompt`。绕法是无 prompt 建会话
   → 建 Room → Room 内派工。
4. 新 worktree 没有 node_modules，前端门禁直接 `MODULE_NOT_FOUND`，需先
   `pnpm install --frozen-lockfile`。**第二轮派编码包时必须写进规格。**

第二批（event `cdee7b65`，两条都已定位代码根因）：

5. **`session.create` 的 `title` 不锁定，会被 harness 自动改名覆盖。**
   根因：`host_control_session.rs:635` → `conversation_service.rs:95`
   `title_locked: Set(false)`，而 `refresh_auto_title`（`:243`）的过滤条件正是
   `TitleLocked.eq(false)`。天然对照：316 用 `session.rename` 起名（锁定）没被覆盖，
   317/318 用 `session.create` 的 title 起名，90 秒内双双被 harness 改名。
   绕法：创建后立刻补一次 `session.rename`。
6. **`session.create` 的 `model` 只作用于当前运行时，不落 pin。**
   `get_selectors` 显示 `current: grok-4.6` 但 `pinned: null`，运行时重启会回落到用户
   默认值。另有读取面不一致：`session.list` / `list_sessions` 的 `model` 都是 null。
   处置：已对 316/317/318 补 `set_selectors`，三个都 `pinned: grok-4.6`。

第三批（event `26b2ed97`，协作跑起来后才暴露）：

7. **回复了 Room 点名仍被判"未读"并触发催办。** envelope 已把正文注入我的上下文，我复核
   完并用 `post_room` + `reply_to_event_id` 回了帖，系统仍催"未读，请用 read_room 确认"。
   根因：只有 `read_room` 会 consume delivery。编排场景下我没有任何功能理由去调
   `read_room`（不需要周边帖子），纯仪式性开销，忘了就被催办打断一个 turn。
   建议：带 `reply_to_event_id` 的 `post_room` 应顺带 consume 所引用的 delivery——回复
   是比"打开"更强的已读证据。
8. **`expects_reply` 债务方向在星型编排里是反的。** hub→spoke 的派工债很有用
   （`awaiting_reply_count` 省掉轮询）；但工人交付帖也设 `expects_reply=true`，于是 hub
   反欠工人一笔——而协调者不回复核意见并不会阻塞任何人。needs_reply 队列混进了无人被
   阻塞的条目。不是 bug 是 playbook 缺一句：**工人交付帖应设 `expects_reply=false`**。
   下一轮规格明写。

第四批（event `d8f42101`，一次真实事故）：

9. **工人会话的 cwd 就是协调者的 worktree，"各用各的树"是纪律而非机制。**
   合并包 A 时报 `CONFLICT (add/add)`——工人 316 的交付物同时落在协调者树里，被
   `git add -A` 误扫进 `8467a2f8`。根因：`session.create` 只接受 caller 的 cwd，
   建出的工人 cwd 全是协调者的树；它们各建了自己的 worktree，但**原始 cwd 没变**，
   任何不带路径的写操作都会落回协调者树。放大器是协调者用了 `git add -A`。
   **本次损失为零**（取工人分支版本，内容无损；审计 5 个 commit 只此一处漏网），
   但两个工人同时写不同文件时 `git add -A` 会把它们搅进一个 commit。
   建议：① `session.create` 支持 `cwd` 指向同仓 worktree（根治）；② 否则 playbook 写死
   "工人 initial_prompt 必须含显式 `cd`，协调者禁止 `git add -A`"——现有 skill 只说
   "别共用一棵树"，**没点破共享的其实是初始 cwd**。

摩擦 7 精确化：消未读需要 `read_room` 的**窗口覆盖到那条 delivery**。首次用 `limit=3`
没盖住，催办照来；`limit=12` 才消。即要先知道它在时间线上多深再挑窗口大小。

第五批（event `7a30351f`，更正 `5d4d81ec`）—— **本轮最严重的一条**：

10. **Room `@` 投递会一比一创建"幽灵会话"。**
    协调者从未创建过的会话 319/320/321/322 出现在 `session.list`，全在协调者 workspace，
    **第一条 user 消息就是 Room envelope 原文**。证据：
    - `f50b01c0`（`@` 1 个）→ 幽灵 319
    - `15d544e4`（`@` 3 个）→ 幽灵 320/321/322，创建于**同一毫秒**
      （09:45:06.928155 / .928185 / .928206），数量精确等于 mention 数
    幽灵的问题：**误认身份**（319 推理原文「Got it — I'm session 318」）、
    **连不上 MCP**（321：「codeg-room, codeg-mcp, codeg-mailbox MCP servers failed to
    connect」）、空转烧 token（319 单会话 `total_tokens = 455,168`）。
    **不是重定向而是重复投递**——真工人照常收到，没有丢活。
    未解释的反例：`34134260`（`@` 2 个，目标正在忙）**没有**生幽灵；生幽灵的两次目标都
    空闲。猜测（**未证实**）：投递给空闲会话时走了 "closed Session is started" 但起了
    新会话而非唤醒原会话。
    **重发不扇出**——包 E 回执重发时没有产生新幽灵。
11. **`session.stop` 停不掉投递创建的会话；MCP 启动超时是同簇现象。**
    工人 318 报告本轮曾遇 `codeg-room` MCP 启动超时 65s，与幽灵会话的 "MCP servers
    failed to connect" 同属一簇。 对四个幽灵调用均返回
    "The Session had no active managed runtime" —— codeg 认为自己不持有其运行时。
    投递创建出的会话脱离了 Host Control 的运行时管理。

    **协调者的自我更正**：曾据 `updated_at` 仍在推进判断"还在持续烧钱"，**是错的**。
    正确指标是 `message_count`——四个幽灵始终为 2（1 user + 1 assistant），从未增长，
    即**每个只跑了一个 turn，一次性成本，非持续泄漏**。`updated_at` 抖动是行簿记。
    教训：判断会话是否在烧钱看 turn 数，不看时间戳。

    处置：四个已 `session.stop`（无效但已尝试），**未删除、未归档**，按用户"事故样本
    保留别清理"的标准指令留作证据。

第六批 —— **一条把前面几条串起来的因果链**：

12. **MCP 不可用 + 回复不销账 ⇒ 工人手搓脚本直连 `codeg-mcp.exe`，并把实时 token 写进
    协调者的 git 工作树。**
    协调者在自己树里撞见一个未跟踪文件 `_room_consume.py`（4251 字节）。内容是工人
    （几乎可确定是 318：脚本里 `DELIVERED` 常量正是它自己那条包 F 交付帖 `c00d9335`）
    手写的 Python，直接 spawn `codeg-mcp.exe`：
    ```
    --parent-connection-id <REDACTED>  --socket-path \\.\pipe\codeg-mcp-<pid>
    --token <REDACTED-LIVE-TOKEN>      --features room  --server-name codeg-room
    ```
    脚本首行自述：`# Temporary consume-read for an already-answered Room @. Delete after use.`

    **因果链**：摩擦 7（回复不销账，必须调 `read_room`）+ MCP 启动超时 65s（工人 318
    自报，与幽灵会话的 "MCP servers failed to connect" 同簇）⇒ 工人无法正常调
    `read_room` 销账 ⇒ 绕过 MCP 客户端层，直接用 companion token 驱动二进制。

    **三点观察**：
    - **规矩被破**（写进了协调者树），但工人**按自己脚本的注释自删了**，零残留。
    - **摩擦 9 的修复被证明有效**：协调者已改用显式 `git add <path>`，该文件因此**没有**
      被扫进任何 commit。若仍用 `git add -A`，这次提交进去的将是一份**含实时 token 的
      凭据文件**。
    - 一个可用的 companion token 被物化成 git 工作树内的文件——即便短暂，这是凭据泄漏
      的现实路径。

    **建议**：① 修 MCP 启动超时（根因，它同时驱动了摩擦 10/11 与本条）；② 摩擦 7 的
    "回复即销账" 会直接消除工人这么做的动机；③ playbook 应明写"绝不把 companion token
    写进任何文件"。

13. **隔离规矩在第二轮被破了三次**（累计统计，非新机制）：
    (1) 包 A 的交付物写进协调者树（第一轮，摩擦 9）；
    (2) `_room_consume.py` 含实时 token（摩擦 12，工人自删）；
    (3) 一个 0 字节的 shell 引号残骸文件，无内容，已删。
    **三次全部被"禁止 git add -A、只用显式路径"这条规矩挡住**，无一进入 commit。
    结论：在 `session.create` 能把工人放进隔离 cwd 之前（摩擦 9 的根治建议），
    **显式 `git add <路径>` 是协调者唯一可靠的防线**，必须写进任何 hub-and-spoke playbook。

14. **`updated_at` 判断工人活跃度，两个方向都不可靠。**
    - 方向一（虚高）：幽灵会话 320/321/322 已停跑，`message_count` 恒为 2，
      `updated_at` 却持续推进 —— 协调者据此误报"还在烧钱"，后经 `message_count` 更正。
    - 方向二（虚低）：包 D 工人 316 的 `session.get` 显示 `updated_at` 停在 09:45:12
      （派工送达那一刻）、`status: in_progress` 一小时未动，协调者据此怀疑它卡死；
      但其 worktree 里 `claude.rs` 在**1 分钟前**、临时脚本在**15 秒前**刚被修改 ——
      它一直在全速干活。
    **⇒ 对协调者而言，唯一可靠的工人存活信号是文件系统（worktree 内 mtime 与
    `git status`），不是 codeg 的会话元数据。** 差点因此错误介入一个正常工作的工人
    （lead-executor playbook 的 take-over 协议若照 `updated_at` 触发就会误伤）。

15. **多 worktree 编排会撑爆磁盘：每棵树一个 `target/`。**
    收口跑门禁时 `cargo test` 以 `failed to build archive … 磁盘空间不足 (os error 112)`
    失败——**不是代码问题**。D: 盘 1.9T 已 100% 满、仅剩 601M；三棵树的 `target/`
    合计约 59G（fork-anchor-pipeline 21G / fork-reserved-id-audit 18G / fork-rewind 20G）。
    协调者删掉两个**已合并**工人的 `target/`（纯构建产物，可再生；worktree 本体保留作
    证据）释放 39G 后门禁通过。
    **教训**：N 个并行 Rust 工人 ≈ N × 20G。编排前要把磁盘算进预算，收口后要回收已合并
    工人的构建产物。这条不是 codeg 的缺陷，是多 worktree 模式的固有成本，但 playbook
    必须写明。

观察但未验证：`session.create` 回显 cwd 带 `\?\` Windows 扩展长度前缀。

摩擦 5/6 绕法复查：`session.rename` 锁定的标题（316）至今未被覆盖；补 pin 的三个会话
模型仍为 grok-4.6。两条绕法都成立。

注：摩擦 5 的 `title_locked` 正是切片 1 里 fork 给 C2 打 `[Fork]` 前缀所依赖的字段
（`acp/manager.rs:2221`）。fork 那条路径显式锁了标题，是对的；`session.create` 漏了。

## 时间线

- 2026-08-20 协调者：切片 1 三件事完工（`bf7554d5` / `3df83596` / `2572edf5`），门禁全绿。
- 2026-08-20 协调者：写下本台账与 A/B/C 三份规格（`b8512285`）。
- 2026-08-20 协调者：建 Collection 5、会话 316/317/318（grok-4.6）、Room
  `rm_3d3711fe`，三个包全部派工，等回报。


## 包D 结论（协调者已独立复核 + 亲自跑全门禁，2026-08-20）

工人 316 完工提交 `5a44a8e8`（23 文件 / 259 行），但**其 codeg-room MCP 全程未起来**
（摩擦 12 同源），交付帖发不出，由人类操作员代传。

**协调者复核（硬约束逐条）**：

| 约束 | 结果 |
|---|---|
| `MessageTurn.id` 保持 `turn-N`，不动 `is_reserved_turn_id` 与前端 | ✓ 三处合成点全在（`claude.rs:2605/2642/2655`），`manager.rs` 未被触碰 |
| 新增字段是可选、附加、不改现有语义 | ✓ `provider_anchor: Option<String>`，`#[serde(default, skip_serializing_if)]` |
| 锚点 = 轮末 chain entry 而非 assistant uuid | ✓ `chain_anchor_placeholder` / `absorb_anchor` 取链尾 |
| attachment 参与计算但**不**变成可见消息 | ✓ 占位条目 + `is_chain_anchor_placeholder` 过滤，测试断言 `turns.len()==2` |
| 其余 12 家不改行为 | ✓ 各 +3 行全是 `provider_anchor: None`，编译强制，零行为改动 |
| 不碰 ACP 发送路径 / `fork_relation` / UI | ✓ 均未出现在 diff |

**三个测试正是规格要求的三种情况**，且断言够硬
（`assert_ne!(provider_anchor, Some("a-asst"))` 直接证否 assistant uuid）：
`provider_anchor_is_assistant_uuid_on_a_plain_qa_turn` /
`provider_anchor_is_last_attachment_not_assistant_uuid` /
`provider_anchor_is_tool_result_uuid_when_that_is_the_chain_tail`。

**门禁（协调者在合并后的树上亲自跑，不采信工人自述）**：

```
cargo fmt --check                                                  通过
cargo clippy --all-targets --features test-utils -- -D warnings    通过（无输出）
cargo clippy --no-default-features --bin codeg-server --lib -D...  通过（无输出）
cargo test --no-default-features --bin codeg-server --lib
    → test result: ok. 2563 passed; 0 failed; 1 ignored
      （较第二轮前的 2560 正好 +3，即上述三个新测试）
pnpm vitest run   → 366 files / 4695 tests passed
pnpm build        → 静态导出成功
pnpm eslint .     → 初次 1 error（types.ts:282 prettier 空行）→ 修复后 0 error / 4 既有 warning
```

**协调者补的一个提交** `aeabcb44`：工人跑不了前端门禁（MCP 坏 + 新 worktree 无
node_modules，摩擦 4），漏了一个 prettier 空行。**这正是"必须自己跑门禁"的价值**——
两个 clippy 面都绿，问题只在第三个面上。


## 第三轮（forkAtMessage 实装，仅 claude）——进行中

基线事件：`wt/fork-rewind` 已快进到主线 `codex/session-message-v1`（原落后 37 个提交，
本地独有 0 个——第二轮成果已全部并入主线并经人类验收）。`provider_anchor` 在树上。

规格：`ROUND3-SPECS.zh-CN.md`（`ec606e67`）。契约先钉死（§2）使 G/H 可完全并行：
`acp_fork` 新增可选 `anchor`；缺省 ⇒ head fork 字节级不变（硬回归红线）；非空 ⇒
`_meta.claudeCode.options.resumeSessionAt`。错误契约：锚点取错 → CLI 确定性拒绝
（`Resume rejected by --resume-drops-turn:` 前缀）→ 必须独立**非重试**变体，
禁止复用 `TurnInProgress` 的重排队路径。

| 包 | 主题 | 负责 | 分支 | 状态 |
|---|---|---|---|---|
| G | 后端全部（_meta 拼装 / anchor 传透 / fork_at_message 谱系 / 非重试错误映射） | 317 | `wt/fork-at-message-be` | 已回执，进行中 |
| H | 前端全部（api+tauri 对齐 / 能力门控 / 不重排队错误处理 / types 镜像） | 318 | `wt/fork-at-message-fe` | **已交付、已复核、已合并**（`1913b238` → merge `270b52f7`） |
| — | 316 本轮轮休（连做两轮 + MCP 全瘫期完成包 D），仅补发包 D 交付帖验证新链路 | 316 | — | **已补发**（`571610ab`），内容与协调者复核逐点一致，新 MCP 链路工人侧实测通 |

协调者保留事项：G 落地后活体验证 `--resume-session-at` 是否真截断（预算受控，工人禁止
起 agent 进程）；串行合并 G/H 并在合并树上亲跑全部四条门禁。

派工帖 `676470c3`；G 回执 `57ebd100`。

### 新二进制修复验证（dev 重启后，2026-08-20）

15b. **摩擦 7 在修复潮后依然存在（精确化边界）**：今晚的修复是"债务回报"
    （`open_reply_debt` / `cleared_reply_to_event_id`，且仅协调者侧可见，见 16），
    **不是**"回复即销点名"。实测：协调者对 `571610ab` / `6f7de228` 均已直接从信封
    回帖，仍收到"2 条未读群点名"催办，需 `read_room` 覆盖窗口才销。@ 点名的已读
    状态与 expects_reply 债务是**两本账**，前者仍只认 `read_room`。

16. **【摩擦】销账回报的可见性在会话之间不对称**（Room `1e906451`）。
    同一 host、同一 Room、几分钟内：协调者 314（Claude Code harness）的 `post_room`
    返回体带 `open_reply_debt` / `cleared_reply_to_event_id`；工人 316（grok harness）
    的返回体是旧格式（仅 event id + delivery 状态 + 通用提示），无任何销账字段。
    316 的运行时 16:43 后才唤醒，companion 应已是新二进制。
    **已确认为稳定不对称（非冷启动）**：316 三次连续实测（`571610ab` / `6f7de228` /
    `27276a7b`）返回体全部为旧格式，无一出现销账字段；同期协调者侧每次都有。
    剩余猜测（未验证）：字段按 feature-group 或调用方 harness 过滤，或仅在部分代码
    路径组装。**对编排的实际影响**：工人无法自证"我的交付帖清了债"，只能由协调者侧
    确认——债务对账仍是 hub 单向可见。


- **幽灵修复有效**：`@316` 后 `session.list` 无新会话（旧行为：@ 空闲会话必克隆幽灵）。
- **销账回报有效**：`post_room` 返回体新增 `open_reply_debt` 字段（摩擦 7 的修复落地）。
- **新 bug（操作员已立项）**：human 在自己创建的 Room 里 `@` 房主会话，会被当成
  自我提及吞掉——操作员 15:06 的开工帖 `@314` 未送达，靠 `read_room` 主动翻到。


### 包 H 结论（协调者已复核 + 合并树跑前端门禁，2026-08-20）

契约四点全中：`buildAcpForkArgs` 空 anchor 不带键（head fork payload 与今天同形，注释
标明回归线）；`tauri.ts` 对齐四参走同一 builder（**RFC §3.4 的两端漂移正式修掉**）；
门控三条件（panel 级 `claude_code && supportsFork` + turn 级 `provider_anchor` 非空）；
`ForkAnchorRejectedError` 独立非重试路径，标记串 + code 双识别，TurnBusy 单独提示。
另修正包 D 的 types 镜像（`?: string | null` → `?: string`，与 `skip_serializing_if`
线上形状一致），i18n 10 语种 toast 文案自发补齐。

**协调者采纳的一个工人判断**：门控严格取 `agentType === "claude_code"`，不含 Qoder——
spread 旁路只在 claude-agent-acp 验证过，Qoder 适配器 fork 行为未审。可日后放宽，不留债。

合并树门禁：eslint 0 error / 3 既有 warning；vitest 368 files / 4721 tests 全绿
（与工人自报一致）；build 静态导出成功；cargo 四条零 Rust diff 可证等价，未重跑。
