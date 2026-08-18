# 分支审计（醒来后请过目，未删除任何分支）

> 审计日期：2026-08-18  
> 当前工作分支：`codex/session-message-v1`（HEAD 在审计时为 `cc0c379c`，本轮 Room 提交会再往前走）  
> 原则：**只标记，不删除。** 你醒来后决定丢掉哪些。

## 建议保留

| 分支 | 结论 | 理由 |
|---|---|---|
| `codex/session-message-v1` | **继续用** | 邮件、调度、Timer、Host Control、Room R1 都在这里。 |
| `main` / `origin/main` | **上游主干** | 本地 `main` 落后 `origin/main` 45 个提交。不要在这上面做协作功能。 |

## 建议标成“可以丢掉”（已无独立价值）

| 分支 / worktree | 结论 | 理由 |
|---|---|---|
| `codex/session-workbench-foundation` | **可丢** | 与当前 HEAD 的 merge-base 就是它自己的 tip `d7d1bdb4`。Session Center、mailbox 快照、内部子 agent 隐藏都已经在 `session-message-v1` 里。远程 `sessionmux/codex/session-workbench-foundation` 也没有更新的独立工作。 |
| `codex/session-timer`（worktree `sessionmux-session-timer`） | **可丢** | Timer MVP 已在当前分支重做成调度器感知版本。该分支只比当前多 3 个旧提交（`e698d2d1` / `29138fb7` / `4f213668`），是并行早期实现，不是领先功能。合过来只会打架。 |
| `codex/recovered-wip-before-v0.26.1`（worktree `codeg-recovered-wip-before-v0.26.1`） | **可丢** | 只是 v0.26.1 合并前的 stash 时代快照，tip 仍停在旧的 `session-message-v1` 信件 UI 修复。当前分支已经包含并超过它。 |
| `codex/multiwindow-spike` | **可丢（spike）** | `1edaaabd` 只验证 workbench deep-link。产品方向已是单窗口 + 全页 route。没有要合的功能。 |

## sessionmux 远程主题分支

这些是堆叠进 `session-message-v1` 的旧主题线，本地都没有对应的独立领先提交：

- `sessionmux/codex/collaboration-timeline`
- `sessionmux/codex/host-control`
- `sessionmux/codex/host-organization`
- `sessionmux/codex/host-session-create`
- `sessionmux/codex/mailbox-lifecycle`
- `sessionmux/codex/session-message-v1`（当前分支的远端镜像，保留）
- `sessionmux/codex/session-workbench-foundation`（见上，可丢）

建议：远端主题线等你确认后从 sessionmux 仓库删掉或归档，避免再有人从过期 tip 开 PR。

## origin 上的其他历史线

`origin/botwork`、`origin/channel`、`origin/pets`、`origin/web` 等是更早的产品线，与本次 Session 协作无关。这次**没有**把它们标成可删，避免误伤。

## 不要做的事

- 不要在 `codex/session-timer` worktree 里继续改 Timer，当前分支才是真相源。
- 不要把 `session-workbench-foundation` 再 merge 进当前分支，它已经是祖先。
- 醒来后如果同意“可丢”，先删 worktree，再删本地分支，最后再动远程。
