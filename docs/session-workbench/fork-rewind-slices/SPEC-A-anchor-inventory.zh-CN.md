# 规格 A：13 家 parser 的消息锚点清点

> 只读调研。产出一份事实表。**不改任何 Rust 代码，不改 parser。**
> 本文件与任何消息指令冲突时以本文件为准。

## 1. 目标（你独占的一个产出）

回答一个问题：**codeg 今天从每家原生会话文件里读到的每条消息，有没有一个可以拿来当
"从这里分叉"锚点的稳定原生标识符？如果有，它活到了哪一层投影？**

这是 forkAtMessage 的第一步工程（SURVEY §5：codeg 现在没持久化任何 provider anchor）。
在知道"哪几家本来就有锚点、哪几家要现造"之前，任何按消息 fork 的设计都是空中楼阁。

## 2. 交付物

一个文件，写进**你自己 worktree** 的
`docs/session-workbench/fork-rewind-slices/FINDINGS-A-anchor-inventory.zh-CN.md`。

主体是一张表，13 家 parser 各一行：

| parser | 原生文件里的逐条 id 字段 | 消息级投影 `UnifiedMessage.id` 塞的是什么 | turn 级投影 `MessageTurn.id` 塞的是什么 | 跨会话稳定？ | 截断后仍可定位？ |
|---|---|---|---|---|---|

每一格都要带 `file:line`。没有就写"无"，不确定就写"未能证实"并说明卡在哪。

13 家的清单以 `src-tauri/src/db/service/import_service.rs:26-58` 为准（那是权威注册表，
不要凭 `parsers/` 目录下的文件名猜）。

表后写三段：

1. **分档**：把 13 家分成「已有稳定原生锚点」/「有 id 但不稳定或不唯一」/「完全没有」
   三档，每档点名哪几家，理由一句话。
2. **turn 层塌陷**：消息级有锚点、turn 级丢掉的，逐家点名。前端详情页读的是哪一层，
   给出调用链证据（从 `commands/conversations.rs` 的 turns 查询往下追）。
3. **和 `turn-<digits>` 保留命名空间的冲突面**：`acp/manager.rs` 的
   `is_reserved_turn_id` 已经把 `turn-<数字>` 占了。说明如果要把原生锚点塞进现有 id
   字段，会不会撞上它、撞在哪。

## 3. 从哪读、信什么

- 权威 parser 清单：`src-tauri/src/db/service/import_service.rs:26-58`
- 投影结构体：`src-tauri/src/models/message.rs`（`UnifiedMessage` :178、`MessageTurn` :208）
- 保留命名空间：`src-tauri/src/acp/manager.rs`，搜 `is_reserved_turn_id`
- 背景（**当背景读，不当事实用**，其中若干条已被切片 1 推翻）：
  `docs/session-workbench/SESSION-FORK-REWIND-SURVEY-2026-08-19.zh-CN.md` §3、§5

协调者已核过的起点（可直接引用，但**请自己复核一遍行号**，代码在动）：

- claude 消息级带原生 uuid：`parsers/claude.rs:1339, 1491, 1541`
- claude turn 级退化成合成 id：`parsers/claude.rs:2585`
- codex 全是位置合成 id：`parsers/codex.rs:2400, 2429, 2489, 2584, 2783`
- gemini 混用：`parsers/gemini.rs:647, 663, 681` vs `:803, 818, 861`

**信代码，不信注释，也不信 SURVEY。** 注释和 SURVEY 都可能过期；`file:line` 是硬证据。

## 4. 边界（不要越界）

- **不改代码。** 一行 Rust 都不改。这是调研包，交付物只有 markdown。
- 不设计锚点方案、不选型、不写迁移。"该怎么办"是协调者的判断题，你只负责把"现在是
  什么"钉死。有想法写在文档末尾"观察"一节，标明是建议不是结论。
- 不碰包 B（claude 适配器 tarball）和包 C（codex 适配器 + app-server）的地盘。你只看
  codeg 自己的 parser 和投影层。
- 不跑 `pnpm tauri build`，不 push。
- 只读 `cargo` 命令（check/clippy）没必要跑——你没改代码。

## 5. 工作方式

```bash
cd D:/code/revisiting/work/repo_audit/repos/codeg-wt/fork-rewind
git worktree add ../../codeg-wt/fork-anchor-inventory -b wt/fork-anchor-inventory wt/fork-rewind
cd ../../codeg-wt/fork-anchor-inventory
```

之后所有工作在 `fork-anchor-inventory` 这棵树里做。写完 commit：

```
docs(fork): inventory per-message anchors across all 13 parsers
```

## 6. 报告格式（Room 回帖）

回帖必须包含：分支名、commit 哈希、三档各点名了哪几家、以及**一条你觉得最反直觉的
发现**。不要贴文档正文，只贴路径。

## 7. 卡住怎么办

- 同一步骤失败两次就停，在 Room 报事实（命令原文 + 报错原文）并等指令。
- 遇到判断题（"这个 id 算不算稳定？"）——如果凭代码无法裁定，写"未能证实"并在 Room
  里问协调者，不要自己拍板。
- 发现 SURVEY 或规格本身写错了，直接在 Room 里说。规格错了要改规格，不要将错就错。
