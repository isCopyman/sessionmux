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
| A | 13 家 parser 的消息锚点清点 | 待派 | `wt/fork-anchor-inventory` | `FINDINGS-A-anchor-inventory.zh-CN.md` | 未开始 |
| B | claude-agent-acp 0.69.0 fork 能力实测 | 待派 | `wt/fork-claude-audit` | `FINDINGS-B-claude-fork-audit.zh-CN.md` | 未开始 |
| C | codex-acp 1.4.0 + app-server thread/fork 实测 | 待派 | `wt/fork-codex-audit` | `FINDINGS-C-codex-fork-audit.zh-CN.md` | 未开始 |

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

## 时间线

- 2026-08-20 协调者：切片 1 三件事完工（`bf7554d5` / `3df83596` / `2572edf5`），门禁全绿。
- 2026-08-20 协调者：写下本台账与 A/B/C 三份规格。
