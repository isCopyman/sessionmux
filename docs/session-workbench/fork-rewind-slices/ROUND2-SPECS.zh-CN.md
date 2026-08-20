# 第二轮工作包规格（D / E / F）

> 用户已认可拆法（2026-08-20）。本文件是三个包的规格事实源。
> **本文件与任何消息指令冲突时，以本文件为准。**

## 0. 共同硬规矩（违反即打回，全部来自第一轮的真实教训）

1. **先建自己的 worktree，建完立刻 `cd` 过去。**
   ```bash
   cd D:/code/revisiting/work/repo_audit/repos/codeg-wt/fork-rewind
   git worktree add ../../codeg-wt/<你的目录> -b <你的分支> wt/fork-rewind
   cd ../../codeg-wt/<你的目录>          # ← 这一步第一轮漏了，出了事故
   ```
2. **绝不在协调者工作树内运行任何写命令。**
   协调者的树是 `repos/codeg-wt/fork-rewind`。你的会话 cwd **初始就是它**——这是机制
   限制，不是你的错，但从现在起你负责 `cd` 走。任何 `git`、写文件、`cargo` 产物落在那里
   都算违规。第一轮包 A 就是这样把交付物写进了协调者的树，导致 add/add 冲突。
   **动手写任何文件之前，先 `pwd` 确认自己在哪。**
3. **禁止 `git add -A` / `git add .`**，只用显式路径 `git add <file>`。
4. **碰前端门禁前先 `pnpm install --frozen-lockfile`。** 新 worktree 没有自己的
   `node_modules`，直接跑 `pnpm vitest` 会 `Cannot find module '@vitejs/plugin-react'`。
   （13.6s，不改锁文件。）
5. **交付帖设 `expects_reply=false`。** 派工帖的债是 hub 追 spoke；你交付完就不欠了，
   设 true 会让协调者反欠你一笔，污染 needs_reply 队列。
6. 禁止 push，禁止 `pnpm tauri build`，禁止起 agent 进程做活体探测。
7. 同一步骤失败两次就停手，在 Room 报事实（命令 + 报错原文）等指令。判断题上交，别猜。
8. 报告要带证据：`file:line`、命令原文输出、测试计数。没有对应工具输出的"我验证了"
   等于没验证——协调者会抽查。
9. 发现规格本身写错了，直接在 Room 说。第一轮包 A 就挑出了协调者把 14 家 parser 写成
   13 家，这是好事。

## 1. 第一轮已确立的事实（不要重新调研，直接用）

- **claude 路可行且不需动上游**：`_meta.claudeCode.options.resumeSessionAt` 经
  `acp-agent.js:4821` 的 spread → SDK Options → `sdk.mjs:118` 的
  `--resume-session-at=` argv → CLI。SDK 用 ProcessTransport + stream-json 起 CLI，
  即 SDK 文档所称的 headless lane，**参数处于武装状态**。
- **锚点要的是"被保留那一轮的最后一条 chain entry"**，不是 assistant 消息 uuid
  （`sdk.d.ts:1886-1892`）。取早了校验器**确定性拒绝且不可重试**
  （错误以 `Resume rejected by --resume-drops-turn:` 开头）。
- **codeg 恰恰把那类记录扔了**：`claude.rs:1551-1591` 的 `attachment` 分支只保留
  `goal_status`，其余整条丢弃；`group_into_turns`（`:2565-2571`）吸收 tool_result 正文
  但丢弃其 uuid。实测 transcript 佐证：一条 prompt 之后链尾是四条带 uuid 的 attachment
  （`deferred_tools_delta` / `agent_listing_delta` / `skill_listing` /
  `total_tokens_reminder`）。
- **codex 路本轮不碰**：codex-acp 1.4.0 不声明 fork，原生粒度只到 `lastTurnId`（turn 级），
  `thread/rollback` 已 DEPRECATED。需要上游改动，本轮只更正文档。
- 14 家 parser 的锚点三档见 `FINDINGS-A-anchor-inventory.zh-CN.md`。

---

## 包 D：锚点管道（**切片 2 的硬前置**）

分支 `wt/fork-anchor-pipeline`，目录 `codeg-wt/fork-anchor-pipeline`。

### 目标

让"轮末 chain entry 的原生 uuid"活到能被 fork 使用的地方。**没有这一步，
forkAtMessage 在一整类会话上会确定性失败**，且失败不可重试。

### 范围（严格限定在 claude 系）

只动 claude parser 及其共用者（Qoder 走同一份 `group_into_turns`，`qoder.rs:828`）。
**不要动其余 12 家**——它们的 id 不是 claude chain uuid，填进 `resumeSessionAt` 无意义。

### 要做的事

1. **让 `MessageTurn` 能携带一个原生锚点。** 现在 `MessageTurn.id` 是
   `format!("turn-{}", n)`（`claude.rs:2551/2585/2597`），把原生 uuid 冲掉了。
   **不要改 `id` 的现有语义**——`acp/manager.rs` 的 `is_reserved_turn_id` 依赖
   `turn-<digits>` 形状，前端也在用。**新增一个可选字段**（例如
   `provider_anchor: Option<String>`），装该轮**最后一条 chain entry 的 uuid**。
2. **让 attachment 记录的 uuid 可见。** `claude.rs:1551-1591` 现在把非 goal 的
   attachment 整条丢弃。**不要**把它们变成可见消息（那会污染详情页），只要让它们的
   uuid 能参与"这一轮最后一条 chain entry 是谁"的计算。
   Qoder 那边 `qoder.rs:261-263` 的遍历**本来就步进过 attachment**（"indexed but not
   returned"），可以参考它的做法。
3. **锚点定义**：该轮所有 chain entry（user / assistant / tool_result / attachment）中
   **parentUuid 链上最后的那一条**的 uuid。不是 assistant 的 uuid。
4. **测试**：至少覆盖 (a) 普通一问一答轮；(b) 轮末带 attachment 的轮（用真实形态夹具：
   `deferred_tools_delta` / `skill_listing` / `total_tokens_reminder`）；
   (c) 轮末是 tool_result 的轮。断言取到的锚点是**链尾**而非 assistant uuid。
5. **不做**：不碰 ACP 发送路径（那属于后续的 forkAtMessage 包，要等本包落地才派；
   包 E 是纯文档，与发送路径无关——原文此处笔误，工人 317 指出，已更正），不碰 UI，
   不动 `fork_relation`。

### 门禁

```bash
cd src-tauri && cargo fmt
cargo clippy --all-targets --features test-utils -- -D warnings
cargo clippy --no-default-features --bin codeg-server --lib -- -D warnings
cargo test --no-default-features --bin codeg-server --lib
```
前端只在你改了 TS 时才需要（大概率不需要）。改了才跑，跑前先 `pnpm install --frozen-lockfile`。

### 交付

commit（`feat(fork): surface the last chain entry uuid as a turn anchor` 之类），
Room 回帖带分支 / commit / 测试计数原文 / 锚点定义落在哪个 `file:line`。

---

## 包 E：RFC 第二轮更正（纯文档，可与 D 并行）

分支 `wt/fork-rfc-round2`，目录 `codeg-wt/fork-rfc-round2`。

### 目标

把第一轮三份 FINDINGS 的结论回灌进
`docs/session-workbench/SESSION-HISTORY-CAPABILITIES-RFC.zh-CN.md`。
**逐处引用 file:line 或 FINDINGS 路径。**

### 必改项

1. **§4 补上实测结果**（原文标着"需实测"）：claude-agent-acp 0.69.0 **声明** fork
   （`dist/acp-agent.js:715`，空对象 `{}`）；codex-acp 1.4.0 **不声明**
   （`dist/index.js:29697-29703`）。
2. **§7.1 claude**：改成"不需要上游改动"，写清 `_meta.claudeCode.options.resumeSessionAt`
   这条通道，并把**三颗雷**写进去：① lane 限制（现已静态确认 codeg 在武装 lane 上，
   但仍是"非文档化旁路"）；② `resumeDropsTurn` 拒绝**确定性、不可重试**，必须映射到
   recovery 而非退避；③ 它是 spread 副产品不是契约，上游加一条覆盖即失效。
3. **§7.2 codex**：原生粒度是 **turn 级（`lastTurnId`）不是 message 级**——SURVEY 的
   messageId 说法是错的。产品语义因此是"从这一轮分叉"。`thread/rollback` 已标
   DEPRECATED 且不回文件，**rewind 不押它**。
4. **§2 能力表**：`forkAtMessage` 对 claude / codex 要分别标注粒度（消息级 / 轮级）。
   诚实呈现能力差异是 §10 已有的要求，这里要落到表上。
5. **§9 谱系**：切片 1 的 `fork_relation` 表已落地（migration `m20260820_000002`），
   把"拟议"改成"已实现"，并注明 `anchor` 字段就是留给 forkAtMessage 的。
6. **新增一节：锚点的真实形状。** 引 `sdk.d.ts:1886-1892`（fork 在"被保留那一轮的最后
   一条 chain entry"）+ 我们 parser 现在丢弃它的事实 + 实测 transcript 链尾是四条
   attachment。这是切片 2 的核心约束，RFC 必须有。

### 边界

只改 RFC 一个文件。**不要改** SURVEY（那是历史调研快照，留着看演进）、不要改三份
FINDINGS、不要改台账。不写代码。

---

## 包 F：`is_reserved_turn_id` 注释与缺口评估（小包，可并行）

分支 `wt/fork-reserved-id-audit`，目录 `codeg-wt/fork-reserved-id-audit`。

### 背景

`acp/manager.rs:61-62` 的注释宣称 `turn-<digits>` 是 "which **every parser** assigns via
`format!(\"turn-{}\", n)`"。**这句话是错的**（第一轮已确认）：`cline.rs:285` 用
`{conversation_id}-{turn_counter}`，`cursor.rs:1080` 用 `cursor-turn-{i}`。

该函数服务于一段**安全用途**逻辑：挡住客户端伪造的 `message_id` 撞上持久化 turn id，
避免 id-keyed 跨客户端去重把真实 prompt 静默吞掉。

### 目标（先评估，后动手）

1. **把 14 家 parser 的 turn id 形状逐一列出**（`file:line` + 格式串）。包 A 的
   FINDINGS 里有部分素材，但那是**消息级**口径，你要的是 **turn 级**，别直接抄。
2. **判定是不是真缺口**：形状不是 `turn-<digits>` 的那几家，其 turn id 会不会真的被
   客户端 `message_id` 撞上？要顺着 `is_reserved_turn_id` 的调用点往上追，看
   `message_id` 从哪来、去重键怎么算、撞上会发生什么。
3. **给结论**，三选一：
   - 「只是注释错」→ 提交注释修正，说清实际情况；
   - 「真缺口但低危」→ 修注释 + 在 Room 说明风险与建议修法，**先不改逻辑**；
   - 「真缺口且可利用」→ **停手**，在 Room 报告细节，由协调者决定，**不要自己改安全逻辑**。

### 边界

- 未经协调者拍板，**不要修改 `is_reserved_turn_id` 的判定逻辑**。改注释可以。
- 不碰 fork 相关代码（那是 D / E 的地盘）。
- 改了 Rust 就跑门禁（同包 D 那四条）。

### 交付

Room 回帖带：14 家 turn id 形状表的路径、三选一结论、以及**你追调用点得到的证据链**。
