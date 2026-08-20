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
| A | 13 家 parser 的消息锚点清点 | 316（grok-4.6） | `wt/fork-anchor-inventory` | `FINDINGS-A-anchor-inventory.zh-CN.md` | 已派工 |
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

观察但未验证：`session.create` 回显 cwd 带 `\\?\` Windows 扩展长度前缀。

摩擦 5/6 绕法复查：`session.rename` 锁定的标题（316）至今未被覆盖；补 pin 的三个会话
模型仍为 grok-4.6。两条绕法都成立。

注：摩擦 5 的 `title_locked` 正是切片 1 里 fork 给 C2 打 `[Fork]` 前缀所依赖的字段
（`acp/manager.rs:2221`）。fork 那条路径显式锁了标题，是对的；`session.create` 漏了。

## 时间线

- 2026-08-20 协调者：切片 1 三件事完工（`bf7554d5` / `3df83596` / `2572edf5`），门禁全绿。
- 2026-08-20 协调者：写下本台账与 A/B/C 三份规格（`b8512285`）。
- 2026-08-20 协调者：建 Collection 5、会话 316/317/318（grok-4.6）、Room
  `rm_3d3711fe`，三个包全部派工，等回报。
