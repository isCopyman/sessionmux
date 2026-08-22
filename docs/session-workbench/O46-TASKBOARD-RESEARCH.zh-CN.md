<!-- 来源：grok 只读调研（cli-delegate, session c930cefd），2026-08-21。
     注意：该 run 的工作区停在 015d2319，行号相对主干可能有偏移；
     主干上 `companion.rs:1461` 确为 parse_work_task_spec（已由领导复核）。-->

# 任务看板组织维度调研（只读）

**结论先说：** codeg **已经有一张 work_task 看板**（看板 + 列表，按 folder 过滤）。一级容器应继续是 **项目 Folder（仓库根 / Path）**；Collection 只适合做列内二级分组。不要按 Workbench / Room / agent 当一级泳道。列读取独立 `task_status` 业务轴，Engine 10 态只作为执行事实输入，不要用 `ConversationStatus`。

> 2026-08-22 修订：实施后已形成独立 `task_status` 业务轴；用户又明确要求区分
> `backlog`（未承诺想法）与 `todo`（ready）。本文早期“四列 / todo 兼任 backlog”的描述是
> 当时事实，不再是产品裁决；当前定稿以 `TASK-KANBAN-IMPLEMENTATION-PLAN-2026-08-22` 为准。

---

## 1. 仓库现状：谁挂谁、谁能跨谁

```
Folder (项目根, parent_id=NULL)     ← 执行/Git 边界 = Path
  ├─ Folder (worktree, parent_id→根)
  ├─ Conversation.folder_id         ← 必挂一个 folder
  ├─ Collection.root_folder_id      ← 语义树挂在 Path 下（可 NULL=遗留）
  │     └─ collection_conversation   ← Session 0或1 个 Collection
  └─ WorkTask.folder_id             ← 只许项目根，不许 worktree

Workbench                           ← 无 folder_id、无 collection_id
  └─ opened_tab (folder_id + conversation_id|room_id)  ← 可混多个 folder

Room                                ← 必挂 workbench_id
  ├─ 可选 collection_id / root_folder_id
  └─ members = conversations        ← 创建时不要求同一 folder
```

### Folder = 项目 / 仓库根

- `folder.path` 唯一；`parent_id` 指向仓库根（worktree 扁平指向根，不链式）。`kind`: `regular` | `chat`。证据：`src-tauri/src/db/entities/folder.rs:9-45`。
- Conversation **必挂** `folder_id`：`conversation.rs:54`。
- Collection 的 Path 解析：`canonical_root_folder` 把 worktree 折成根，chat scratch 不能拥有 Collection。`collection_service.rs:50-65`。

### Collection 能不能跨多个 folder？

**有根时不能。** 写入校验：Session 的 `parent_id.unwrap_or(id)` 必须等于 Collection 的 `root_folder_id`，否则 `"A Session can only be placed in a Collection under its own Path"`。`collection_service.rs:408-426`。嵌套同样禁跨 Path：`collection_service.rs:186-189`、`239-242`。

**无根（`root_folder_id IS NULL`）的遗留 Collection 可以混 folder**——校验只在 target 有根时触发。回填只给「成员全部来自同一仓库家族」的旧行打根，混源/空的保持 NULL。`m20260815_000004_collection_root_folder.rs:9-11,32-50`。

一对一归档：`collection_conversation.conversation_id` 是 PK，「零或一个语义家」。`m20260815_000003_collection.rs:63-66,71-76`。

Collection **不改** cwd / Git / 权限。`models/folder.rs:49-50`。

### Workbench

- 表字段只有 `id/name/position/is_pinned/timestamps`，**没有** `folder_id` 也 **没有** `collection_id`。`entities/workbench.rs:4-13`。
- 领域文档说 Workbench 可以有 0 或 1 个 Collection 归档位，且「应能混放不同 Folder」。`DOMAIN-MODEL.zh-CN.md:90-93,209-210`。**代码未实现 Workbench→Collection FK**（搜 `workbench` 实体与 `workbench_service.rs` 无 `collection_id`）。
- 跨 folder 是事实：每个 `opened_tab` 自带 `folder_id`，同一 `workbench_id` 可混。`entities/opened_tab.rs:7-11`。
- 删 Workbench **不删** Session，Room 被 rehome 到 Main（id=1），不级联。`commands/workbenches.rs:429-458`；`m20260818_000008_room_workbench_restrict.rs:1-2`。

### Room 属于哪个容器？

**硬挂 Workbench，软挂 Collection / Path。**

- 创建必填 `workbench_id`。`models/collaboration.rs:485-493`；migration `m20260818_000003_collaboration_room.rs:24-34`。
- 另有可选 `collection_id`、`root_folder_id`。`m20260818_000008_room_workbench_restrict.rs:26-29`。
- 放置算法：显式 collection → 显式 root_folder → 成员共享的 collection → **创建者 Session 的 Path**。不继承创建者 Collection。`collaboration_room_service.rs:215-244`。
- 成员只需 live session，**无「同一 folder」约束**。`collaboration_room_service.rs:270-302`。Room 因此可以跨项目；sidebar 用 `collection_id` 优先、`root_folder_id` 其次，两者都 NULL 会进 orphan 桶。`m20260820_000003_room_root_folder_backfill.rs:3-14`。
- 还可加裸路径 `collaboration_room_path`（不必先 open 成 Folder）。`m20260820_000001_room_additional_path.rs:1-4`。

### work_task 今天挂在哪？

**只挂项目 Folder。** 没有 `collection_id` / `workbench_id` / `room_id`。

- `folder_id` 注释：「The project folder (never a worktree folder)」。`entities/work_task.rs:51-53`。
- 创建拒绝 `parent_id.is_some()`：`"tasks must target a project folder, not a worktree"`。`work_task_service.rs:429-439`。
- MCP `create_work_task` 的 `folder_path` 缺省=调用者 cwd；worktree 会 hop 到项目根。`chat_authoring.rs:84-95,301-348`；测试 `chat_authoring.rs:720-741`。工具定义在 `companion.rs:145,193,897,1421-1430`（用户给的 `:1461` 已漂移，现为 `parse_max_messages`）。
- 运行产物：可选 `conversation_id`、`worktree_folder_id`。`entities/work_task.rs:68-69`。
- **每 folder 一个 merge slot** + `sort_order` 按 folder。`engine.rs:16-17,2475-2477`；`tasks-page.tsx:173-174,295`。
- 看板 UI 已存在：默认全 folder 混看，filter 收成一个项目；**未选 folder 不能持久化拖拽排序**。`tasks-page.tsx:132-154,265-295`。

### 催办层（不是会话聚合）

`tasks-view-context.tsx` 聚合的是 **WorkTask**，不是 Conversation。

| 路径 | 行为 |
|---|---|
| `:28` `ATTENTION_STATUSES` | badge 计 `awaiting_input` + `review` + `failed` |
| `:170` `notifyFlips` | ~~系统通知只放行 `review` 和 `failed`，排除 `awaiting_input`~~ **报告此处过时**：主干（`tasks-view-context.tsx:176-199`）**会**为 `awaiting_input` 发通知，只是加了每卡 5 分钟冷却（`AWAITING_INPUT_NOTIFY_COOLDOWN_MS`），因为 `running ⇄ awaiting_input` 每次提问都会翻转。领导已复核，无需修 |
| 四列 `attention` | 还含 `merging`（`board-columns.ts:25,48-55`） |

---

## 2. 一级分组推荐

**推荐：项目 Folder（仓库根 / Path）。**

理由全是第 1 节的硬约束，不是产品感觉：

1. 卡片（`work_task`）的 **唯一必填容器** 就是 `folder_id`。
2. 引擎不变量按 folder：merge 互斥、queue `sort_order`、settings 继承、worktree 落地。换一级容器会把这些切碎。
3. Collection / Room 的 Path 都 **折到同一个 folder 根**，Folder 是它们的公共祖先。
4. 现看板已经用 Folder 当 filter；缺的是「全项目混看时的列内分段」，不是新的一级。

**第二选择：Collection（且只能做 Folder 之下的二级）。**

- 它回答「语义上属于哪一摞」，和看板「按主题找卡」接近。
- 但 `work_task` **没有** `collection_id`；要当分组必须经 `conversation_id` 反查，或新加 FK。无会话的 `todo` 卡会落 Unclassified。
- Collection 自己也挂在 Path 下，不能当跨仓一级。

**明确不适合当一级：**

| 维度 | 原因 |
|---|---|
| Workbench | 布局/现场，非法 FK 到任务；设计上跨 folder。同一任务可出现在 0–N 个工作台。 |
| Room | 通信通道，挂 workbench；成员可跨仓；与 work_task 无边。TASKBOARD-RFC 已写「Room 编队不进 work_task」。 |
| Agent | 是 `config.agent_type` 覆盖 / folder 默认，不是容器。同任务可换 agent、retry。适合列内分段，不适合泳道所有者。 |

和 vibe-kanban 对齐：它的一级也是 **Project**（不是 agent workspace、不是 layout）。

---

## 3. 参照物：vibe-kanban

已访问 GitHub `BloopAI/vibe-kanban` README、`crates/db/src/models/{task,project}.rs`、`shared/types.ts`，以及 [Issue Management](https://vibekanban.com/docs/issue-management) / [Kanban View](https://vibekanban.com/docs/cloud/kanban-board)。

**一级容器 = Project**（`Task.project_id` → `projects`）。Repo 是另一实体（git 路径），Workspace 是执行沙箱（branch + worktree），**挂在 issue/task 下，不是看板分组**。Agent 是 executor，不是容器。

本地 `TaskStatus`（`crates/db/src/models/task.rs:14-21`）：

`todo | inprogress | inreview | done | cancelled`

文档列（Cloud 看板，默认可自定义）：

| 列 | 含义 |
|---|---|
| To do | 未开始 |
| In progress | 人/agent 在做 |
| In review | 做完等人审 |
| Done | 完成且核实 |
| Backlog / Cancelled | 默认隐藏，All tab 才见 |

Cloud 另有 Organization、assignee、tag、sub-issue、多 workspace/issue。跨项目「Master Kanban」是 issue #1106，不是默认。产品正在 sunset。

对 codeg：对照物支持「一级=项目，列=状态，agent/workspace 是卡上的执行细节」。codeg 的 Folder ≈ 他们的 Project+Repo 合体。

---

## 4. 状态机：列用哪套枚举

**两套 status，不要混。**

### A. `ConversationStatus`（会话库标签，4 态）

`entities/conversation.rs:17-26` / `src/lib/types.ts:766-770`：

`in_progress | pending_review | completed | cancelled`

这是人改的归档点（侧栏点、Session Center），**不是** 引擎流水线。没有 queued / preparing / awaiting_input / merging / failed。

### B. `WorkTaskStatus`（看板事实源，10 态）——现成

`entities/work_task.rs:4-43`：

`todo → queued → preparing → running ⇄ awaiting_input → review → merging → done`，旁路 `failed` / `canceled`。CAS + `done ⟺ merged`。

现成四列（`board-columns.ts:22-27,37-59`）：

| 列 | 覆盖状态 | 相对 ConversationStatus |
|---|---|---|
| todo | `todo`,`queued` | **新概念**（会话无排队） |
| inProgress | `preparing`,`running` | 粗对应 `in_progress`，但会话没有 preparing |
| attention | `awaiting_input`,`review`,`merging`,`failed` | 仅 `review`≈`pending_review`；其余 **新** |
| done | `done`,`canceled` | ≈ `completed`/`cancelled`，但 work_task 的 done 绑定 merge |

`awaiting_input` 的精确含义：引擎对 Question / Permission / PlanApproval 的 outstanding-id 集合翻转 `running ⇄ awaiting_input`。`engine.rs:10-13,28-29`。

**建议：** 列继续用 B 的四列，不改 10 态。不要用 A 当列——映射会丢掉排队、等人授权、合并中、失败。若要「会话也进看板」，那是另一张板，且会话没有 merge/worktree 不变量。

相对 vibe-kanban 缺的是独立 Backlog（可用 `todo`+`scheduled_at` 凑）；多出来的是 `queued/preparing/awaiting_input/merging/failed`，这是 agent 执行板该有的分辨率。

---

## 5. 这张板最可能失败的两个原因

1. **容器选错，和引擎打架。** 若一级改成 Collection / Workbench / Room：大量 `todo` 卡无会话、无 membership，变成孤儿；拖拽 `sort_order` 和 merge slot 仍按 folder，UI 分组与可持久化顺序分裂（现已有预兆：`tasks-page.tsx:295` 混 folder 时关掉 reorder）。Room/Workbench 跨仓，还会把不共享 Git 锁的卡排进同一条队列。

2. **两套「任务」叠在一张板上。** 看板卡片是 work_task；侧栏「会话」是 Conversation + 4 态库标签；催办还漏掉 `awaiting_input` 通知（`:170`），而 badge（`:28`）和 attention 列又把它算进去。用户会在「会话 pending_review / 任务 review / 任务 awaiting_input 不响」三套语义里迷路——看板看起来有了，真正等人的时刻却对不上。

（次级风险，不占两个名额：attention 列把「问你一句 / 请你验收 / 正在 merge / 已经失败」挤在一起，vibe-kanban 的 In review 没有这个问题。）

---

## 6. Agent Orchestrator 对照审计（2026-08-22）

本地审计对象：`Untrivial-ai/agent-orchestrator`。它值得参考，但它不是 Codeg 要复制的 Task
领域模型。

### 6.1 它的“任务”与看板到底是什么

Agent Orchestrator 没有独立的本地 Task 表。最接近任务的是 GitHub / Linear 等外部 Tracker
Issue，`Session.issueId` 只是把一个 worker Session 关联到 Issue。它的 Dashboard 列由
`getAttentionLevel()` 根据 Session runtime、Agent activity、PR、CI、Review、mergeability
等事实推导：Working / Pending / In review / Needs you / Ready to merge；拖动列不是修改业务
任务状态。

它也有 Backlog，但语义是带 `agent:backlog` 标签的外部 Issue。轮询器会自动抢取并启动
worker，随后把标签改成 `agent:in-progress`。这与 Codeg 的停车场语义相反：Codeg Backlog
必须允许整理、预先归属，却不能自动启动或消耗额度。

### 6.2 借鉴什么

- 把 activity、业务状态和“需要人关注”三者分开；
- 将 runtime、CI、Review、未解决评论、mergeability 作为卡片的事实投影输入；
- 外部事实暂不可用时显示 stale / unknown，不伪造成失败；
- Project/Folder 是运行边界，Workspace/Worktree 是执行产物，不是任务容器；
- Session detail 适合调查运行、日志、PR 与 Review，而任务责任入口应独立、醒目。

### 6.3 不借鉴什么

- 不把 Issue 等同于 Task，也不假设一张 Issue 只有一个 worker；
- 不让 Backlog 标签触发自动 pickup；
- 不用 runtime / PR 状态替代可编辑的 Task 业务状态；
- 不假设每张任务都需要 branch / worktree / PR / CI；
- 不采用 flat metadata 或进程内 Set 作为任务权威与并发 claim；
- 不把 orchestrator Session 变成任务数据库的所有者。

因此 AO 对 Codeg 的价值是“Session Execution + 工程事实投影”的参考实现；Multica 对
Backlog/Todo 分界更接近 Codeg，Codeg 自己的 SQLite、CAS、Task 事件和 PromptQueue 仍是唯一
事实源。
