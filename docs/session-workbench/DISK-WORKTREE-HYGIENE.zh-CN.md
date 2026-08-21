# 磁盘与 Worktree 卫生制度（2026-08-21 立）

> 起因：2026-08-21 凌晨 D 盘（1.9T）只剩 131G，且 02:52 dev 后端 exit-1 崩溃时
> 磁盘一度只剩 601MB——磁盘耗尽是崩溃的头号嫌疑。盘查确认元凶是 **worktree 里的
> cargo target 目录**：一次完整桌面 debug 构建约 16-23G，worktree 用完即弃但
> target 从不清理。当晚清理回收 91G+（131G → 222G+）。

## 一、事实基线（2026-08-21 盘点）

| 项目 | 体积 | 处置 |
|---|---|---|
| 主仓 `src-tauri/target` | 36G | 保留（dev 实例运行中） |
| 主仓 `src-tauri/target-gate` | 22G | 保留（门禁专用，见 §三.3） |
| 每个 worktree 的 `src-tauri/target` | 16-23G/个 | **禁止产生**，产生即删 |
| 每个 worktree 的 `.next` | ~130M/个 | 随 worktree 删除 |
| 每个 worktree 的 `node_modules` | 20M-1G/个 | 随 worktree 删除（pnpm 硬链接，实际占用小） |
| `.claude/worktrees/agent-*`（agent 隔离残留） | 共 1.4G/19 个 | 已并入即删 |

## 二、Worktree 生命周期纪律

1. **建**：一个任务一个 worktree，统一放 `../codeg-wt/<名字>`，分支 `wt/<名字>`。
2. **验收合并后立即拆**（同一个回合内，不过夜）：
   ```bash
   git worktree remove --force ../codeg-wt/<名字>
   git branch -D wt/<名字>
   git worktree prune
   ```
3. **Windows 陷阱**：`git worktree remove` 常报 `Directory not empty`（长路径/
   句柄占用），此时元数据已删但目录还在——必须补 `rm -rf` 并确认目录真没了。
   删完看一眼 `ls ../codeg-wt/`，不要只信 exit code。
4. **未并入的 worktree 不删树、只删产物**：`rm -rf <wt>/src-tauri/target` 永远
   安全（纯构建产物可再生）。
5. **每晚开工前**跑一次 `git worktree list` 对账：列表里每一项都要能说出留着的
   理由，说不出就按 2/4 处理。

## 三、构建产物政策

1. **worktree 里默认禁跑 cargo**。前端任务（eslint/tsc/vitest）在 worktree 里
   跑没问题；Rust 门禁统一回主仓跑。派工任务书里要写明这条。
2. worktree 确需 cargo（如后端任务的红绿验证）：跑完、验收合并后，target
   随 worktree 一起删。**不允许"留着下次快"**——下次的 worktree 是新的，缓存
   命中率低，不值 20G。
3. 主仓双 target 制：`target`（dev 实例用）+ `target-gate`（门禁用，
   `CARGO_TARGET_DIR=target-gate`），互不干扰。`target-gate` 允许在磁盘告急时
   `cargo clean` 重建（代价约一次全量编译时间）。
4. `.next` 缓存：turbopack 持久缓存可能把撕裂读固化成假 Build Error（2026-08-21
   两次实证，报 globals.css Invalid code point 而文件本身干净）。处方：杀 dev
   进程 → `rm -rf .next` → 重启。

## 四、磁盘水位线

- **地板 100G**：低于 100G 禁止派任何会跑 cargo 的工，先清理再派。
- **警戒 150G**：低于 150G 时开工前先做一轮 §二.5 对账 + 删无主 target。
- 检查命令：`df -h /d`。每晚自主作业开始时看一次，重活（门禁批、多 worktree
  并行）前再看一次。
- 崩溃教训：磁盘耗尽时 dev 后端直接 exit-1 且**日志无 panic 痕迹**——以后遇到
  无征兆退出，先查磁盘。

## 五、agent 隔离 worktree（`.claude/worktrees/`）

Claude Code 的 isolation:worktree 会在这里建 `agent-*` 树，理论上"无改动自动
清理"，实测会积累（一晚 19 个）。纪律：每晚收工时按"HEAD 已并入主干 && 工作区
干净"批量删（先 `git merge-base --is-ancestor` 分类，同 §二）。分类出 UNMERGED
的一律保留并上报，不猜。

## 六、当晚遗留（供次日决策）

- `repo_audit/worktrees/sessionmux-session-timer`（分支 codex/session-timer，
  未并入）：32G target 已删，树保留。是否还要这条线，待用户定。
- `.claude/worktrees/agent-a31e3493008230165`：领先主干 41 提交（qoder/chat
  功能线旧提交，非当晚工作），保留待用户定夺。

## 修订（2026-08-21）：常驻车道 vs 一次性树

原来那条「合并即删」是在还没用命名车道之前定的，现在分两种：

**常驻车道（保留）** —— 按子系统各留一条，反复派工进去：

| 车道 | 干什么 | 热缓存 |
| --- | --- | --- |
| `ui` | 前端组件 / i18n / vitest | `.next/`、`node_modules/` |
| `backend` | Rust（acp / commands / db） | `src-tauri/target/`（大头） |

保留的理由**不是省 git IO**（`git worktree add` 共享对象库，本来就便宜），
而是省**构建产物冷启**：新树 = Rust 全量冷编译 + turbopack 冷编译；
热车道 = 增量。代价约 20G/棵，所以常驻控制在 2–3 条。

`cli-delegate` 的车道语义配合这条：车道干净且无独有提交时，下一次 `run`
（不是 `resume`）自动快进到源 HEAD；`resume` 永不快进（会把分支从活着的会话底下抽走）。
目录删了但 `cli-delegate-<slug>` 分支还在的话，下次同名 `--worktree-name` 会重新挂回来。

**一次性树（合并即删）** —— 匿名 `--worktree`、实验、需要强隔离的改动。原规则不变。

复用车道的隐性收益：CLI 会话可以 `resume`，跟进任务不用重讲背景。
反过来，**不相关的新活别 resume 老会话**，要 `run --fresh`，否则背一堆无关上下文。
