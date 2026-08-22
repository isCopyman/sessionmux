# Task / Kanban 实施定稿（2026-08-22）

> 状态：已裁决，进入实施。本文覆盖 `KANBAN-DESIGN-2026-08-21` 中“任务天然等于
> Worktree 执行”的旧边界；现有 WorkTask Engine、看板 UI 和多轮生命周期继续保留。

## 1. 产品目标

Codeg 的任务系统服务于单 Agent、多 Agent、多项目和多 Harness。第一版必须完成以下闭环：

1. 用户或 Agent 创建任务；
2. 任务可以暂不分配、人工处理、交给已有 Session、交给新 Session，或交给隔离
   Worktree Session；
3. 用户可以指派，Agent 可以受控地指派或原子领取；
4. 任务在全局和项目看板中是同一个对象；
5. 指派给 Session 的任务可靠排队，不越过用户消息、不隐式打断；
6. 用户能从卡片看到任务进度和执行状态，并完成、审查、重试或取消。

目标是覆盖约 90% 的日常使用，不做 Jira、自定义工作流、无限层级和复杂权限系统。

## 2. 一句话模型

> Task 记录“要做什么”；Execution Mode 记录“怎样推进”；Session 记录“谁在做”；
> Worktree 只是一种执行环境；Board 是同一批 Task 的视图。

```text
Task（唯一任务事实）
├── task_status：用户在看板上看到的业务进度
├── execution_mode：未定 / 人工 / Session / Worktree Engine
├── project：可空
└── execution fields：按 execution_mode 解释
    ├── session → conversation_id + PromptQueue item
    └── engine  → run_seq + worktree + branch + merge/preflight
```

第一版继续演进 `work_task` 表，不建立第二套任务 UI 或第二套 Task ID。为避免同一个
`status` 在不同模式下产生互斥含义，增加独立的业务状态轴；现有十态 `status` 收窄为 Engine
执行状态。将来若确实需要一个 Task 同时保留多个并行 Execution 的结构化历史，可以把执行列
机械抽取到 `task_execution`，而不改变任务业务轴或 UI API。

## 3. 数据模型裁决

### 3.1 业务状态轴

新增：

```text
task_status = todo | in_progress | blocked | review | done | canceled
```

看板、过滤、徽标和 Agent 任务列表只读取 `task_status`。现有 `status` 仍由 WorkTask Engine
使用：

```text
todo → queued → preparing → running ⇄ awaiting_input
     → review → merging → done
     ↘ failed / canceled
```

迁移回填：

| 旧 status | task_status |
| --- | --- |
| todo / queued | todo |
| preparing / running | in_progress |
| awaiting_input / failed | blocked |
| review / merging | review |
| done | done |
| canceled | canceled |

所有业务状态双写集中在 `work_task_service`，用穷举测试保证不会漏状态。非 Engine 模式不进入
Engine pump、reconcile、merge 和并发槽。

### 3.2 执行方式

新增：

```text
execution_mode = null | manual | session | engine
```

- `null`：尚未决定怎样处理；
- `manual`：用户人工推进；
- `session`：交给普通持久 Session，不拥有、不删除该 Session；
- `engine`：复用现有 WorkTask Engine，拥有 Worktree 执行代并进入审查/合并流水线。

`conversation_id` 不再隐式表示 Retry。所有启动、恢复、重命名和清理逻辑先检查
`execution_mode`。

### 3.3 项目可空

`folder_id` 改为真正的 nullable，不使用 `0` 哨兵，也不创建假的 Folder。原因：当前任务查询
会 inner join live folder，哨兵任务会从看板和 Agent 工具中静默消失；真实 `Option<i32>` 还能让
编译器暴露所有需要处理“无项目”的触点。

- 项目看板：自动过滤该 project root；
- 全局看板：不过滤项目；
- 未归属：`folder_id = NULL`；
- Worktree Folder 永不作为任务项目，始终解析到 project root；
- 删除项目后沿用现有软删除行为，不自动把历史任务倒进未归属桶。

## 4. 全局与项目看板

只有一个任务事实库，不建立物理上相互复制的“全局板”和“项目板”。

```text
任务
├── 全部任务
├── 未归属
└── 项目
    ├── Codeg
    ├── Thesis
    └── ...
```

从项目入口创建任务时自动带上项目；从全局入口创建时允许不选项目。项目看板和全局看板可
分别记住过滤、分组、排序和板/列表模式，但修改的是同一张任务卡。

第一版不做保存任意复杂 View，不做按 Team/Room/Collection 建立另一套任务归属。

## 5. 第一版 UI

### 5.1 新建任务

必填只有标题。描述和项目可选。

```text
[创建任务]              默认：只落卡，不启动
[创建并执行 ▾]
  ├── 人工处理
  ├── 指派已有 Session
  ├── 新建 Session 处理
  └── 新建 Worktree Session
```

创建框不默认展开模型/Profile/Worktree 设置。选择 Engine 执行时才展示 Harness、Profile、
模型和项目要求；选择已有 Session 时复用目标 Session 的运行配置。

### 5.2 卡片

保留现有卡片、活动点、注意力层级和列表视图，增加：

- 执行方式徽标：人工 / Session 名称 / Worktree / 未分配；
- Session 投递状态：排队、处理中、暂停、失败；
- 打开关联 Session；
- Blocked 的明确文案；
- 非 Engine 卡不显示 Merge/Branch 操作。

人工或 Session 任务允许没有 merge 地完成；Engine 任务继续保持“完成必须由合并或现有的无
变更验收路径产生”的不变量。UI 不用吓人的“未经验证”标签，只通过执行方式和是否有 Diff /
Merge 区域如实表达。

### 5.3 任务详情

未分配任务提供四种处理方式；Session 任务显示目标、队列位置、投递/运行状态和打开入口；
Engine 任务继续显示 Worktree、Diff、Preflight、Review 和 Merge。

第一版不提供“只关联但不投递”。它会制造无人负责启动的悬空状态。

### 5.4 Kanban 拖拽

跨列拖拽是领域动作，不是任意写 status：

| 卡片 | 拖到进行中 | 拖到审查/关注 | 拖到完成 |
| --- | --- | --- | --- |
| manual | 直接开始 | 送审 | 直接完成 |
| 未分配 todo | 弹出“如何处理” | 不允许 | 不允许 |
| engine | 使用 Start 按钮；不偷跑 | 空闲时可确认送审 | 必须走 Merge/验收 |
| session | 已指派后自动进入进行中 | 第一版由 `task_complete` 或人工送审 | review 后人工确认 |

非法目标不显示 Drop 高亮。列内排序仍与跨列领域动作分开。

## 6. Session 指派与消息调度

### 6.1 基本裁决

指派默认等于“建立稳定 Session 关联 + 可靠投递任务简报”，两步在一个领域命令内完成。

任务简报使用后端权威 PromptQueue，不使用 Mailbox 的未读/欠回复义务：

```text
用户 follow-up                 最高优先
Task / Collaboration 消息      其后
Timer / Automation background  最后
```

新增 `PromptQueueSource::Task` 和 `task_id` 引用。任务项永不越过已经存在的用户 follow-up，
也没有 steer/interrupt 权限。

### 6.2 目标状态

| 目标 Session | 行为 |
| --- | --- |
| idle | 立即派发下一 Turn |
| busy | 进入队列，当前 Turn 不变 |
| unloaded 但可 resume | 由 SessionDispatcher 恢复后派发 |
| stopped / archived | 拒绝指派 |
| 没有稳定 native identity、无法恢复 | 原子失败，要求选择其他 Session |

“新建 Session 处理”先创建普通持久 Session，再走同一指派命令；不会给 WorkTask Engine 再加
一条特殊启动路径。

### 6.3 精确状态边界

- 指派事务成功：`task_status = in_progress`，展示“排队”；
- PromptQueue claim 不等于已送达；
- ACP prompt 成功提交并接受 claim 后：展示“处理中”；
- `DispatchUncertain`：队列暂停，任务显示原因，绝不静默重放；
- 硬失败：任务进入 blocked，可显式重试；
- 队列项尚未 claimed 时取消：原子删除队列引用并取消任务；
- 已 claimed/submitted 时取消：任务记取消，但不能假装撤回已经送入 Session 的内容，也不能
  静默重放；
- `task_complete(success|needs_review)`：进入 review；
- `task_complete(blocked)`：进入 blocked；
- 人工验收：进入 done。

同一 Session 第一版最多有一张 active 的 session-mode 任务，避免 `task_complete` 归属歧义。

## 7. Agent 能力与权限

保留：

- `create_work_task`（后续在 UI/文案中逐步改称 create task）；
- `list_tasks`；
- `get_task`；
- `task_progress`；
- `task_complete`。

新增一个最小工具：

```text
assign_task(task_id, target_session_id?)
```

- 不传目标：原子领取给调用者自己；
- 传稳定 Session ID：受控地指派给目标 Session；
- 调用者身份由 Codeg 托管的 MCP token 提供，模型不能自报来源 Session；
- 只能领取/指派未分配、未开始的任务；CAS 保证多人抢同一任务时只有一个成功；
- Engine/Worktree 任务不能被普通 Session 抢走；
- 默认允许受管 Agent 指派，因为 Codeg 已允许它们通过 `send_message` 驱动其他 Session；结构化
  指派反而更可见、更可审计。设置中保留一个实例级开关；
- 每次指派写事件，记录来源和目标；
- Agent 第一版不能任意改别人任务状态、解除别人的指派或删除任务。

## 8. 大任务、小任务和 Checklist

第一版不增加 `parent_task_id`。一个主 Agent 可以创建多张普通任务并在自己的计划/任务描述中
记录任务编号；项目分组、筛选和责任 Session 已覆盖主要场景。

细步骤继续使用 Markdown checklist，不单独进入全局看板。父子任务和跨项目伞任务等任务与
指派闭环稳定后再设计，避免先做层级再改语义。

## 9. 实施切片

### S1：双轴与人工车道

- 添加 `task_status`、`execution_mode` 并回填；
- 看板、过滤和 Agent 读取改用业务状态；
- Engine 写侧集中双写；
- 人工开始、送审、完成；
- 新建任务 brief 可选；
- 模式徽标和非 Engine 动作裁剪。

验收：现有 Engine 卡逐状态仍落在原来的列；人工卡跨重启保持业务状态；Engine 回归全绿。

### S2：Session 指派闭环

- PromptQueue Task 来源与 `task_id`；
- 用户指派 UI；
- `assign_task` MCP（含自领）；
- 目标验证、排队、恢复、取消、暂停和失败；
- 普通 Session `task_progress/task_complete` 归属；
- 新建 Session 并指派。

验收：idle/busy/unloaded/stopped/archived/不可恢复、并发领取和取消竞态均有测试。

### S3：无项目与 Kanban 拖拽

- nullable project 迁移；
- 全局、未归属和项目入口；
- 全局快速创建；
- 跨列 DnD 层和能力矩阵；
- 保持列内排序与布局拖拽互不干扰。

验收：未归属任务在全局 UI 和 Agent `list_tasks` 同时可见；不可启动 Engine 直到选择项目；
每个合法/非法 Drop 目标有交互测试。

## 10. 明确不做

- 第二套 Task 数据库或第二套看板页面；
- 自动让空闲 Agent 抢任务；
- 无限层级和复杂依赖图；
- 自定义列/状态机；
- Team/Room 自动等同于任务组；
- 任务消息自动 interrupt/steer；
- 为看板重新实现 Mailbox、SessionDispatcher 或全文搜索。

## 11. 上游与后续

上游 `v0.27.0` 的 Forge Issue/PR → WorkTask 当前仍直连 Engine 任务模型。在 S1/S2 完成前不
合并这部分；后续接入时应先创建中性 Task，再让用户选择普通 Session 或 Worktree Engine。

Room/Session 行级 UI 抽象、Timer 和完整上游整合不进入本 Goal。
