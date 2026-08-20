# Fork/Rewind 第一切片任务简报（2026-08-20，dogfooding 批）

> 给本 worktree（wt/fork-rewind）内工作的编码 agent。你在 codeg 的多智能体协作里
> 工作：有问题在 Room 里问，完成节点在 Room 里报告。**只在本 worktree 改代码，
> 禁止 push，禁止跑 pnpm tauri build。**

## 背景（先读这两份，都在 docs/session-workbench/）

- `SESSION-FORK-REWIND-SURVEY-2026-08-19.zh-CN.md` —— 能力矩阵与路径调研（事实源）
- `SESSION-HISTORY-CAPABILITIES-RFC.zh-CN.md` —— 原 RFC（已知 5 处与代码脱节）

## 任务（调研 §6 的第 1 切片，三件事）

### 1. RFC 对账修订（文档）

把 RFC 与代码的 5 处脱节改对（脱节清单见 SURVEY §1）：
fork 行布局已反转（C1/S1 不动、INSERT C2）、acpFork 签名已扩展（含 tauri.ts:160
两端封装不一致要在 RFC 里记为已知问题）、[Fork] 前缀已移到后端、版本快照更新到
0.26.1 + claude-acp 0.69.0 / codex-acp 1.4.0、补记 fork 继承 pin 配置与 claude
fork 文件层布局两个新机制。修订以代码为准，逐处引用 file:line。

### 2. fork_relation 谱系表（代码，核心）

新建独立谱系表（**绝不复用 conversation.parent_id**，三重背书见 SURVEY §5）：

- migration：`fork_relation` 表，字段按 RFC §9 的设计落地：
  `id, source_conversation_id, target_conversation_id, relation_kind
  ('fork_head' 起步，留扩展), anchor (nullable JSON，先不填), created_at`。
  外键 + 索引（source、target 各一）。参照 db/migration/ 现有 migration 的写法。
- entity + db/service：新 service（如 fork_lineage_service.rs），提供
  `record_fork_head(conn, source, target)` 和 `lineage_for(conn, conversation_id)`
  （双向查询：它 fork 自谁、谁从它 fork 出）。
- 接线：`acp/manager.rs` 的 `persist_fork_outcome`（约 :1993 起）在写 C2 的同一
  事务里 INSERT fork_relation 一行。
- 暴露查询：commands 层加 `_core` + `#[cfg_attr(feature = "tauri-runtime",
  tauri::command)]` + web handler 双模式端点 `conversation_fork_lineage`，
  前端 api.ts 封装 + types.ts 镜像。UI 先不做，端点就位即可。

### 3. head fork 回归测试（Rust）

给现有 ACP fork 管线补测试（现在零覆盖）：
- persist_fork_outcome 级：fork 后 C2 继承 folder/kind/model/pin 值 + collection
  成员、C1 完全不动、fork_relation 有记录、parent_id 仍为 None。
- lineage 查询双向正确。
- 放在 `cargo test --no-default-features --bin codeg-server --lib` 编译面。

## 门禁（你自己跑，绿了才算完）

```bash
cd src-tauri && cargo fmt
cargo clippy --all-targets --features test-utils -- -D warnings
cargo clippy --no-default-features --bin codeg-server --lib -- -D warnings
cargo test --no-default-features --bin codeg-server --lib
cd .. && pnpm vitest run && pnpm build
```

## 纪律

- Rust 2021 / thiserror / SQL 对齐同文件格式；TS strict / Prettier 无分号。
- 提交拆成 3 个 commit（RFC 修订 / 谱系表 / 测试），conventional message。
- 不动 fork 之外的任何行为；不做 forkAtMessage（那是下一切片）。
