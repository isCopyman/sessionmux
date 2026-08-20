# 第三轮工作包规格（G / H）：forkAtMessage 实装（仅 claude）

> 用户已拍板开第三轮（2026-08-20）。**本文件是事实源；与任何消息指令冲突以本文件为准。**
>
> 基线：`wt/fork-rewind` 已快进到主线 `codex/session-message-v1`，`provider_anchor`
> 已在树上。**所有分支从 `wt/fork-rewind` 切。禁止 push，禁止 `pnpm tauri build`。**

## 0. 共同硬规矩（沿用前两轮的血教训，逐条都是真事故换来的）

1. **`git worktree add` 之后立刻 `cd` 过去**，写任何文件前先 `pwd` 确认。
2. **绝不在协调者工作树内运行任何写命令**（协调者的树 = `codeg-wt/fork-rewind`）。
   你的会话 cwd **初始就是它**——这是机制限制，你负责走开。
3. **禁止 `git add -A` / `git add .`**，只用显式路径。（第二轮三次误写全靠这条挡住。）
4. **碰前端门禁前先 `pnpm install --frozen-lockfile`**（新 worktree 没自己的 node_modules）。
5. **交付帖设 `expects_reply=false`。**
6. **门禁四条必须跑全**，少跑一条就可能漏——第二轮包 D 两个 clippy 面都绿，
   问题只在 `pnpm eslint` 那一面。
7. 禁止起 agent 进程做活体探测（烧用户额度）。**活体验证由协调者统一做。**
8. 同一步骤失败两次停手，Room 报事实等指令。判断题上交，不要猜。
9. 发现规格写错直接在 Room 说——前两轮工人各挑出协调者一处硬错，都省掉了下游连锁错误。
10. **磁盘**：每棵 Rust worktree 的 `target/` 约 20G。跑完门禁若磁盘吃紧，在 Room 说。

## 1. 已确立的事实（不要重新调研）

- **通道静态贯通、不需动上游**：`_meta.claudeCode.options.resumeSessionAt`
  → adapter spread（`acp-agent.js:4821`）→ SDK Options → `--resume-session-at=`
  （`sdk.mjs:118`）→ CLI。SDK 用 ProcessTransport + stream-json 起 CLI，即**武装 lane**。
- **锚点 = 被保留那一轮的最后一条 chain entry 的 uuid**，不是 assistant uuid
  （`sdk.d.ts:1886-1892`）。第二轮已把它落成 `MessageTurn.provider_anchor`。
- **取错锚点 → 确定性拒绝且不可重试**，错误以
  `Resume rejected by --resume-drops-turn:` 开头。
- codex 本轮**完全不碰**（不声明 fork，原生只到 turn 级，需上游）。
- 详见 RFC §7.1 / §7.6 与 `FINDINGS-A/B`。

## 2. 契约（G 与 H 都按这个写，先钉死才能并行）

**双模式端点 `acp_fork` 新增一个可选参数 `anchor`：**

```
acp_fork {
  connectionId: string,
  conversationId?: number | null,   // 既有
  folderId?: number | null,         // 既有
  anchor?: string | null            // 新增：要分叉到的那一轮的 provider_anchor
}
```

- `anchor` 缺省 / `null` ⇒ **head fork，行为与今天完全一致**（这条是硬回归红线）。
- `anchor` 非空 ⇒ forkAtMessage：后端把它放进
  `_meta.claudeCode.options.resumeSessionAt`。

**错误契约（关键，别搞错）：**

- 新增一个**独立的、非重试**错误变体（例如 `AcpError::ForkAnchorRejected`），
  由后端识别 `Resume rejected by --resume-drops-turn:` 前缀后返回。
- 前端**绝不可**把它当 `TurnInProgress` 那样重新入队。
  现有 `isTurnInProgressRejection` → `TurnBusyError` → 重排队是**反面教材**：
  这个错误重发永远失败。

**谱系契约**：带 anchor 的 fork 在 `fork_relation` 里写
`relation_kind = 'fork_at_message'`、`anchor = <JSON>`；head fork 维持 `'fork_head'` +
`anchor = NULL`。两个值第一轮的 migration 已经允许，不需要新 migration。

---

## 包 G：后端实装（Rust 全部）

分支 `wt/fork-at-message-be`，目录 `codeg-wt/fork-at-message-be`。

### 要做的事

1. **`acp/fork.rs`**：`session/fork` 请求带上 `_meta`。注意 sacp 11.0.0 没有 typed
   封装，现在是 `UntypedMessage::new("session/fork", &req)`——`_meta` 要手工拼进 JSON。
   **`anchor` 为 None 时不要发 `_meta` 字段**（保持与今天字节级一致）。
2. **`acp/manager.rs` `fork_session`** 与 **`connection.rs` 的 `ConnectionCommand::Fork`**：
   把 `anchor: Option<String>` 一路传下去。
3. **`persist_fork_outcome`**：带 anchor 时写 `fork_at_message` + `anchor`。
   `fork_lineage_service` 里加对应函数（不要改 `record_fork_head` 的签名，另加一个）。
4. **命令/handler 双模式**：`commands/acp.rs` 与 `web/handlers/acp.rs` 都加 `anchor`。
5. **错误映射**：识别 `Resume rejected by --resume-drops-turn:` 前缀，返回独立的
   非重试错误变体，并让 HTTP 侧映射到一个与 `TurnInProgress`(409) **不同**的状态码。

### 测试（至少）

- head fork 回归：`anchor=None` 时请求体**不含** `_meta`，且 `fork_relation` 仍是
  `fork_head` + `anchor IS NULL`。
- 带 anchor：`fork_relation` 是 `fork_at_message` 且 `anchor` 落库。
- 错误映射：给定那句拒绝前缀，返回的是非重试变体。

### 边界

- **只做 claude 系**。不要给 codex 或其它家加任何 anchor 逻辑。
- 不碰前端（那是 H）。不碰 `provider_anchor` 的产生逻辑（第二轮已落地，别动）。
- **不要为了测试去起真 agent 进程。**

### 门禁

```bash
cd src-tauri && cargo fmt
cargo clippy --all-targets --features test-utils -- -D warnings
cargo clippy --no-default-features --bin codeg-server --lib -- -D warnings
cargo test --no-default-features --bin codeg-server --lib
```

---

## 包 H：前端（TS 全部，按 §2 契约并行开发）

分支 `wt/fork-at-message-fe`，目录 `codeg-wt/fork-at-message-fe`。

### 要做的事

1. **`src/lib/api.ts` 的 `acpFork` 加 `anchor` 参数**，按 §2 契约传。
2. **顺手修掉一处已知漂移**：`src/lib/tauri.ts:160` 的 `acpFork` 至今只传
   `connectionId`，吃不到 `conversationId/folderId` 的 adopt 修复（RFC §3.4 记为已知
   问题）。这次一并对齐成与 api.ts 同签名。
3. **能力门控（诚实呈现，RFC §10）**：只有在
   **① 连接是 claude 系** 且 **② 该轮 `provider_anchor` 非空** 时，才显示
   「从这里分叉」。两个条件缺一不可——没有锚点就分叉会撞上那个不可重试的拒绝。
4. **错误处理**：`ForkAnchorRejected` **绝不重新入队**，给用户一句人话说明"这个位置不能
   分叉"，并保持原会话可用。对照 `isTurnInProgressRejection` 那条**重排队**路径，
   不要复制它。
5. **`src/lib/types.ts`**：镜像 `MessageTurn.provider_anchor`（后端已是
   `Option<String>` + `skip_serializing_if`，所以 TS 侧是 `provider_anchor?: string`）。

### 边界

- **不写 Rust。** 后端契约按 §2 假定存在；G 没落地前你的调用会编译通过但运行报错，
  这是预期的，不要为此改后端。
- UI 入口先做在历史轮次上即可，不做 editAndFork（那是后续切片）。

### 门禁

```bash
pnpm install --frozen-lockfile   # 先跑这个
pnpm eslint .
pnpm vitest run
pnpm build
```

---

## 3. 协调者自己做的事（不派给工人）

- **活体验证**：G 落地后，由协调者用一次预算受控的真实 fork 验证
  `--resume-session-at` 是否**真的截断**。这是整条路唯一尚未活体证实的环节
  （静态已贯通）。工人不许碰。
- 串行合并 G / H，并在合并后的树上**亲自跑全部门禁**（第二轮的教训：工人跑不全）。
