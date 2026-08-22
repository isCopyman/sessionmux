# Task / Kanban 实施定稿（2026-08-22）

> 状态：已裁决，分批实施中。S1 的双状态轴、人工车道、标题即可建卡、执行方式徽标和
> 非 Engine 动作裁剪已经落地；S2 已完成“中性任务卡 → 已有 Session”的可靠指派主链，
> 并补齐“中性任务卡 → 新普通 Session / 新 Worktree Session”的显式启动配置；Agent
> 建卡、修改、自领和指派已经落地；Session 对话顶部的独立活动任务入口也已落地，
> collaborator 仍待实施。本文覆盖
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
task_status = backlog | todo | in_progress | blocked | review | done | canceled
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

### 3.6 内容与讨论边界

任务卡不复制 Multica Issue 的独立评论系统。四种内容各归其位：

- Task 保存标题、简报、状态、负责人、简短进度和稳定引用；
- Session 保存执行者自己的完整工作上下文；
- Room 保存围绕主题的多人共享讨论；
- 项目文件保存会持续修改的计划、共识、证据和产物本体。

卡片详情的推进记录继续复用 append-only `work_task_event`，允许人或 Agent 留一条简短备注、
进度摘要和文件路径，但不提供回复、`@`、子 Thread 或另一套未读状态。需要讨论时引用 Task ID
进入既有 Session/Room；需要长期维护时直接编辑项目文件。Task 是索引和状态事实，不是第四个
聊天面板，也不是把项目 Markdown 再复制进数据库。

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

目标状态语义现已固定并迁移为七类，不允许每个项目创建互不兼容的状态机。项目可在以后自定义列显示名
或隐藏某列，但底层 `task_status` 仍保持稳定，使全局汇总、Agent 工具和自动化可以可靠工作。

### 4.1 七个固定状态，七个可隐藏列

默认看板显示七列：

```text
Backlog | 待办 | 进行中 | 审核中 | 已完成 | 已阻塞 | 已取消
```

每列与一个业务状态一一对应：

| 看板列 | `task_status` | 含义 |
| --- | --- | --- |
| Backlog | `backlog` | 原始想法或尚未承诺的工作；也可以直接指派执行 |
| 待办 | `todo` | 已梳理好、可领取和执行，但尚未开始 |
| 进行中 | `in_progress` | 人或 Session 正在推进 |
| 审核中 | `review` | 等待人类或其他 Session 审核 |
| 已完成 | `done` | 已确认完成 |
| 已阻塞 | `blocked` | 当前无法继续 |
| 已取消 | `canceled` | 不再继续的任务 |

列可按个人习惯隐藏。拖动中性/人工卡只改变状态，不启动 Agent；明确“指派/领取/新建 Session
执行”才进入 PromptQueue、唤醒执行者并转为进行中。因此看板整理与模型执行保持解耦。

新建任务保持兼容地默认进入 Todo，并在创建器中提供清楚的 Backlog 选择；Agent 建卡也使用同一
个可选初始状态，默认仍是 Todo。模糊需求可主动选择 Backlog，不把所有快速建卡都强制多走一步。

截至 `m20260822_000004_work_task_backlog`，七态、七列投影、人/Agent 创建入口和显式指派均已落地。
跨列拖拽复用仓库已有的 `@dnd-kit/core`，不增加依赖；真实 Desktop WebView2 已验证卡片浮层、
目标列高亮、跨列持久化和删除临时验收卡。

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
- Session 对话顶部提供与“往来信件”同级的独立任务入口，显示该 Session 负责和参与的活动
  任务，可跳到卡片或看板；低频“会话详情”只保留身份、模型、Token 与时间等元数据；
- Room 可以插入/引用同一张任务卡并显示简洁状态，但不复制任务，也不把 Room 变成第二个任务
  数据库；Room 中的讨论仍留在共享时间线；
- 任务详情显示主要负责人和协作者，不把多个 Agent 压成一个含混的“已分配”标签。

### 5.5 Kanban 拖拽

普通中性/人工卡可在七列之间自由拖动。拖动只修改业务状态，绝不领取、指派、启动、唤醒或
中断 Session；显式“指派/领取”才建立责任关系并进入统一 PromptQueue。已有 Session/Engine
执行卡继续由其真实执行生命周期更新，避免把仍在运行的 ACP/Worktree 伪装成已完成；用户可在
详情中的明确动作推进或取消。列内自定义排序与跨列状态移动分开，第一版优先保证跨列移动可靠。

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
- 可领取/指派任意列里尚无 owner 的普通卡；成功后统一进入进行中。CAS 保证多人抢同一任务时
  只有一个成功；
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

**实现状态（2026-08-22）**：已完成。新任务默认是未分配的中性卡；七个固定状态可以直接
选择，普通卡可自由拖动并写入 `task_status_changed` 审计事件，整个过程不启动 ACP、Session
或 Worktree。显式“指派/领取”才进入 Agent 执行；人工卡和 Engine 卡分别展示自己的合法动作。
桌面端已真实验证“只填标题创建 →
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
- Session 独立任务入口的“负责 / 参与任务”。

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
以下面的补充记录为准。collaborator、Session 任务投影及其余恢复边界仍未完成。

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

**补充实现状态（同日，Session 任务入口）**：Session 的活动责任任务不放在低频“会话详情”
元数据中，而是在对话顶部提供与“往来信件”同级的独立入口。入口显示活动任务数量和最高优先
任务，展开后列出当前 Session 负责的未归档、未结束任务；点击使用稳定 Task ID 切到同一任务
看板并打开精确卡片。已完成、已取消、已归档和其他 Session 的任务不会混入。组件测试、路由
恢复测试与真实 Desktop CDP 的“指派 → Session 入口出现 → 展开 → 精确卡片”均已通过，临时
任务已删除。

仍未完成：collaborator 及其余恢复边界。Worktree Engine 继续作为一种明确执行模式，不再冒充
所有任务的默认创建方式。

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
