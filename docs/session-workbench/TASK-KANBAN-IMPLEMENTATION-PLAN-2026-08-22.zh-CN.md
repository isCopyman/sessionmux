# Task / Kanban 实施定稿（2026-08-22）

> 状态：已裁决，分批实施中。S1 的双状态轴、人工车道、标题即可建卡、执行方式徽标和
> 非 Engine 动作裁剪已经落地；S2 已完成“中性任务卡 → 已有 Session”的可靠指派主链，
> 并补齐“中性任务卡 → 新普通 Session / 新 Worktree Session”的显式启动配置；Agent
> 建卡、修改、自领和指派已经落地，collaborator 与 Session 属性投影仍待实施。本文覆盖
> `KANBAN-DESIGN-2026-08-21` 中“任务天然等于 Worktree 执行”的旧边界；现有 WorkTask
> Engine、看板 UI 和多轮生命周期继续保留。

## 1. 产品目标

Codeg 的任务系统服务于单 Agent、多 Agent、多项目和多 Harness。第一版必须完成以下闭环：

1. 用户或 Agent 创建任务；
2. 任务可以暂不分配、人工处理、交给已有 Session、交给新 Session，或交给隔离
   Worktree Session；
3. 用户可以指派，Agent 可以受控地指派或原子领取；
4. 任务在全局和项目看板中是同一个对象；
5. 指派给 Session 的任务可靠排队，不越过用户消息、不隐式打断；
6. 用户能从卡片看到任务进度和执行状态，并完成、审查、重试或取消。

任务既可以从人类的模糊想法开始，也可以由主 Agent 根据一段需求拆出，还可以从 GitHub
Issue 等外部条目导入。创建来源不改变任务语义：卡片先成为 Codeg 的独立事实，再决定是否
关联外部来源、由谁执行以及是否需要 Worktree。

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

### 3.4 负责人、协作者与外部来源

任务责任关系不塞进标题、描述或 `conversation_id`。S2 引入独立 Assignment：

```text
TaskAssignment
├── task_id
├── conversation_id（稳定 Session ID）
├── role = owner | collaborator
├── state = assigned | active | completed | removed
└── actor / timestamps
```

- 一张任务同一时间最多一个主要负责人；CAS 领取只竞争 owner；
- 可以有多个协作者，用于调研、实现、审查等分工；
- 多个 Session 的进展最终汇总到同一 Task，但各自仍保留自己的 Session 历史；
- 更换负责人不改 Task ID，也不删除旧 Session；
- Session 属性面板通过 Assignment 投影“我负责 / 我参与”的任务。

外部 Issue、PR、文档或 Room 帖子使用可选 Source Link，而不是冒充 Task 或强制同步：

```text
TaskSourceLink
├── task_id
├── provider / external_id / url
├── title_snapshot
└── sync_policy（第一版只读引用）
```

因此一个 Issue 可拆成多个 Task，一个 Task 也可引用多个来源。第一版先完成手动关联和审计字段；
Forge 自动导入在核心闭环稳定后接入，默认不会因为 Issue 变更而静默覆盖人或 Agent 已整理的卡片。

### 3.5 修改与审计

用户和受管 Agent 都可以创建、补全和拆分任务。Agent 修改采用稳定 Task ID、调用者的托管
Session 身份和乐观 revision：

- 标题、描述、项目、来源和未开始任务的执行建议可修改；
- 所有创建、编辑、指派、领取和状态变化写入 `work_task_event`；
- Agent 不可静默删除任务、覆盖已经提交的执行结果或跳过 Engine 合并不变量；
- 发生并发编辑时返回冲突和最新快照，不使用最后写入者悄悄覆盖；
- 人类原始描述保留在活动历史中，主 Agent 可以把模糊任务整理成更具体的卡片。

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

状态语义第一版固定为六类，不允许每个项目创建互不兼容的状态机。项目可在以后自定义列显示名
或隐藏某列，但底层 `task_status` 仍保持稳定，使全局汇总、Agent 工具和自动化可以可靠工作。

### 4.1 为什么界面是四列、底层却是六态

默认看板只保留四列：

```text
待办 | 进行中 | 等你处理 | 已完成
```

它们是六个业务状态的稳定投影，而不是完整状态机：

| 看板列 | `task_status` | 含义 |
| --- | --- | --- |
| 待办 | `todo` | 已记录、尚未开始，也包含尚未决定执行方式的想法 |
| 进行中 | `in_progress` | 人或 Session 正在推进 |
| 等你处理 | `blocked` / `review` | 分别表示受阻和等待人类验收；卡片徽标继续区分两者 |
| 已完成 | `done` | 已确认完成 |

`canceled` 默认从活动看板隐藏，可由过滤器查看。第一版不再增加 Backlog：`todo` 本身就是“已
捕获但尚未承诺开始”的池，是否已排期、由谁负责、何时开始分别由执行方式、负责人、计划时间和
排序表达。如果再加 Backlog，会迫使人和 Agent 在两个都表示“还没开始”的桶之间反复搬卡，
同时削弱跨项目汇总的一致性。

项目可以保存列隐藏、分组、排序和筛选；不能任意新增状态或改变迁移规则。这样保留足够的视图
自由度，又不会让 Agent 工具、全局汇总和自动化面对每个项目各自发明的一套状态机。

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

这里的边界是产品不变量，不只是界面取舍：Profile 属于 Session 启动身份，不属于 Task。
因此“保存任务”永远不会顺手启动一个 ACP Harness；“指派已有 Session”也不能借任务卡改写
该 Session 的 Profile/模型。只有“新建 Session 处理”和“新建 Worktree Session”进入共享的
Session Launch 配置页，并复用普通新建会话相同的 Harness、Profile、Model、Mode 数据源和
稳定 `conversation_id` 建连顺序，禁止在任务模块复制第三套选择器或 ACP 探测状态机。

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

### 5.4 列表、Session 与 Room 投影

- 看板视图用于按状态快速推进；列表视图用于密集查看项目、负责人、协作者、来源和最近活动；
- 全局与项目入口共享视图组件和查询，只改变过滤条件；
- Session 属性面板显示该 Session 负责和参与的任务，可跳到卡片或看板；
- Room 可以插入/引用同一张任务卡并显示简洁状态，但不复制任务，也不把 Room 变成第二个任务
  数据库；Room 中的讨论仍留在共享时间线；
- 任务详情显示主要负责人和协作者，不把多个 Agent 压成一个含混的“已分配”标签。

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

通过渐进式 Host Control 增加两个最小动作：

```text
task.claim(task_id)
task.assign(task_id, target_session_id?)
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

同时把现有任务工具收敛为一组领域动作：

```text
create_task(title, description?, project_id?, source_links?)
task.update(task_id, title?, description?)
list_tasks(filters?) / get_task(task_id)
assign_task(task_id, target_session_id?, role=owner)
task_progress(...) / task_complete(...)
```

`create_work_task` 在 Engine 兼容期保留旧名称，但工具契约现已只创建中性卡，不再接受
Harness / Profile / Model；这些配置只在新建 Session 或 Worktree Session 并指派时出现。Agent
可以先整理看板，再由自己领取、指派别的 Session，或留给人类。

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

**实现状态（2026-08-22）**：已完成。新任务默认是未分配的中性卡；人工开始、送审、完成、
取消、重开和归档通过后端 CAS 领域命令推进，并写入 `task_status_changed` 审计事件，不启动
ACP、Session 或 Worktree。未分配卡不能靠拖拽偷跑，必须显式选择“人工开始”或“交给
Agent”；人工卡和 Engine 卡分别展示自己的合法动作。桌面端已真实验证“只填标题创建 →
未分配 → 人工处理 → 待验收 → 完成 → 归档”，数据库中 Engine `status` 始终保持 `todo`，
业务状态独立推进为 `done`。

### S2：Session 指派闭环

- PromptQueue Task 来源与 `task_id`；
- 用户指派 UI；
- `assign_task` MCP（含自领）；
- 目标验证、排队、恢复、取消、暂停和失败；
- 普通 Session `task_progress/task_complete` 归属；
- 新建 Session 并指派。
- Agent `create_task/update_task` 与 revision 冲突保护；
- 一个 owner + 多 collaborator 的 Assignment 投影；
- Session 属性面板的“负责 / 参与任务”。

验收：idle/busy/unloaded/stopped/archived/不可恢复、并发领取和取消竞态均有测试。

**实现状态（2026-08-22）**：已完成第一段主链。新建/编辑弹窗只维护标题、说明、附件和项目，
默认落为未分配卡；旧 Engine 卡在编辑正文时只保留历史执行快照，不会被任务编辑器重新探测或
改写 Harness。用户可从卡片/详情选择同项目（含项目 Worktree）的已有持久 Session；后端在
同一事务中建立稳定 Assignment，并把带 `task_id` 的任务简报放入后端权威 PromptQueue。
用户 follow-up 仍优先，busy 时排队，idle/unloaded 时由现有 Dispatcher 恢复并发送；取消/删除
会撤回尚未提交的简报并释放 owner。普通 Session 的 `task_progress/task_complete` 已能按稳定
`conversation_id` 回写任务。Rust 事务/队列测试和真实 Tauri WebView2 的“建卡 → 指派 →
Session 收到 → 卡片进入进行中”已通过。

该段主链完成时尚未补齐新建 Session / 新建 Worktree Session 的统一启动配置；其后续状态
以下面的补充记录为准。collaborator、Session 属性投影及其余恢复边界仍未完成。

**补充实现状态（2026-08-22）**：已完成新 Worktree Session 的显式启动边界。任务创建仍只
创建中性卡；用户随后选择“新建 Worktree Session 执行”时，才进入复用普通 Composer
组件的 Harness / Claude Profile / Mode / Model / Effort 配置窗口。选择已有 Session 时仍继承
目标 Session 配置，不在任务卡上重复设置。后端用原子 CAS 同时冻结启动快照并 claim 任务，
且在调用 ACP `session/new` 之前先创建稳定 Codeg `conversation_id`，避免 Session 启动后再
补身份的竞态。Profile 作为 Session 启动配置写入运行环境和持久配置，不再只是一个前端 chip。

同批修正了“等你处理”提醒的事实源：侧栏计数和通知现在读取业务 `task_status` 的
`blocked/review`，不再误读只属于 Worktree Engine 的 `status`。因此人工任务和普通 Session
任务进入受阻/验收时也会被正确看见。

自动测试已覆盖配置快照的原子 claim、Profile 传递、人工/Session 业务状态提醒；真实 Tauri
WebView2 已验证“只填任务内容建卡 → 卡片保持未分配 → 显式打开新 Worktree Session →
出现同一套 Profile/Mode/Model/Effort 控件”，未启动昂贵的真实 Agent 回合，临时任务随后清理。

**补充实现状态（同日）**：已有 Session 指派弹窗现可继续进入“新建普通 Session”。普通与
Worktree 两条路径复用同一个 Harness / Claude Profile / Mode / Model / Effort 启动面板；普通
路径先创建稳定 Codeg Session、持久化 Profile，再建立 ACP 连接，Harness ready 后才执行同一
`work_task_assign_session` 事务。启动或指派失败时保留已经创建的普通 Session，便于修复和再次
指派，不生成隐藏的半条 Task Runtime。相关编排测试和真实 Desktop CDP 验证已通过，CDP 临时
任务已清理；验证未点击最终“创建并指派”，避免启动真实计费回合。

**补充实现状态（同日，Agent 闭环）**：`create_work_task` 已收敛为中性建卡，建卡时不再向
Agent 暴露 Harness / Profile / Model。渐进式 Host Control 新增 `task.update / task.claim /
task.assign`：受管 Session 可在领取前整理卡片，也可修改自己正在负责的卡片；可用稳定 Task ID
原子自领或指派同项目的另一个持久 Session。领取复用 UI 的 Assignment + 后端 PromptQueue
事务，竞争者不会偷走同一张卡。Agent 仍不能删除、取消或把卡片直接置为 done；执行后通过
`task_progress/task_complete` 进入 blocked/review，由人类验收或取消。

仍未完成：collaborator、Session 属性投影及其余恢复边界。Worktree Engine 继续作为一种明确执行模式，
不再冒充所有任务的默认创建方式。

### S3：无项目与 Kanban 拖拽

- nullable project 迁移；
- 全局、未归属和项目入口；
- 全局快速创建；
- 跨列 DnD 层和能力矩阵；
- 保持列内排序与布局拖拽互不干扰。
- 列表/看板视图共享过滤与排序；
- Room 中插入同一 Task 的轻量卡片（不复制 Task）。

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
