# 看板：把任务指派给已有会话——耦合点调研（2026-08-21）

来源：codex 只读调研（job `bbx32glez`），行号以调研当时的工作树为准。

## 一句话结论

**做不成「加个 conversation_id 参数」。** 任务引擎假定自己同时拥有 worktree、
分支、会话、连接四样东西；而且**一张卡的生命边界就是一轮 ACP turn**
（第一个 `TurnComplete` 就结算成 review/failed/canceled、断连、释放并发槽）。

用户要的「任务是复杂的、一轮回复搞不定、更像 multica 里的 issues」与此正面
冲突——这是**状态机层面的改动**，不是参数层面的。

未决，等拍板。下面是 codex 的原始清单。

---

结论：这不是在现有启动点旁边加一个 `conversation_id` 参数就能成立。当前任务执行模型同时拥有四样东西：任务 worktree、任务分支、任务会话、任务连接；从启动、单轮结束、取消、重试、验收、合并到清理都依赖这组所有权。外来会话如果直接塞进现有 `conversation_id`，会被识别成“上一次任务会话”，然后在新建的任务 worktree 中尝试 resume。

以下行号以当前工作树为准。全程只读，未改文件、未做 git 写操作、未运行 build/test。

## 1. 耦合点清单

### 1.1 `conversation_id` 被解释成“我以前启动过这个任务”

- `launch_mode_for` 只看 `conversation_id.is_some()`：有值就是 `Retry`，没有才是 `Fresh`。不存在“预先指派”“外来会话”“任务拥有的会话”之分。外来会话建卡时一旦写入该字段，第一次启动就会走重试语义。`src-tauri/src/work_task/engine.rs:3308-3316`
- 数据模型也没有会话来源/所有权字段，只有 `worktree_folder_id`、`conversation_id`、`connection_id`；创建 DTO 甚至没有 `conversation_id`。`src-tauri/src/db/entities/work_task.rs:63-72`，`src-tauri/src/models/work_task.rs:87-94`
- 创建服务目前强制把 `conversation_id`、`connection_id` 置空。`src-tauri/src/db/service/work_task_service.rs:549-583`

### 1.2 每次启动先拿任务根目录，再创建/恢复任务 worktree

- `launch` 始终以 `task.folder_id` 读取项目根。`src-tauri/src/work_task/engine.rs:847-857`
- 非 merge 一律 `ensure_worktree`；merge 一律 `existing_worktree`。`src-tauri/src/work_task/engine.rs:906-934`
- `ensure_worktree` 会优先复用任务记录的 worktree；目录丢失时尝试按任务分支恢复；否则从项目当前 HEAD 创建 `task/{id}` 分支和 `{repo}-task-{id}` 目录，并把新 folder 挂到任务项目根下。`src-tauri/src/work_task/engine.rs:1192-1284`
- `recreate_worktree_from_branch` 假定任务记录的 `work_branch/base_branch/base_sha` 就是待续工作的真实位置。`src-tauri/src/work_task/engine.rs:1286-1371`
- `existing_worktree` 没有 worktree 或目录不存在就直接报错；它只服务 merge generation，不会退回主工作树。`src-tauri/src/work_task/engine.rs:1373-1392`

外来会话如果原来在主工作树或另一个 folder 中工作，当前启动仍会给卡创建一个新 worktree；后续 diff、preflight、merge 看的都是这个新 worktree，而不是外来会话实际修改的位置。

### 1.3 “已有会话”实际会被拿到任务 worktree 中 resume

- Retry/Return/Merge 从任务 `conversation_id` 找 `external_id`。`src-tauri/src/work_task/engine.rs:955-969`
- 随后仍调用 `spawn_agent(..., working_dir = wt.path, session_id = external_id, owner = "work_task")`。`src-tauri/src/work_task/engine.rs:989-1002`
- 连接复用要求 agent、external session id、working directory 全部相同。主工作树里的现有连接与新任务 worktree 的 cwd 不同，因此不会复用那条 live connection。`src-tauri/src/acp/manager.rs:1048-1093`
- resume 失败会在同一个任务 worktree 中启动一个全新 session。`src-tauri/src/work_task/engine.rs:1005-1033`

所以，按现状写入外来 `conversation_id` 不等于“把任务投进那条现有连接”；它更接近“拿它的 external id，尝试在任务 worktree 里另起一个 resume 进程”。

### 1.4 会话行也被当成任务自产资产

- Fresh 或 resume fallback 会在任务 worktree folder 下创建 conversation，并锁定以卡片标题生成的会话标题。`src-tauri/src/work_task/engine.rs:1036-1075`
- 编辑卡片时，`work_task_update_core` 会把卡片改名同步到它“产生的 session”；只要会话标题仍等于旧卡片标题，就会改会话标题及其 chat-channel 标题。外来会话若碰巧满足值比较，也会被改名。`src-tauri/src/commands/work_task.rs:236-255`，`src-tauri/src/commands/work_task.rs:261-330`

### 1.5 `run_seq`、连接索引和结束判定都是“一卡一连接一轮”

- 内存索引是 `connection_id -> (task_id, run_seq)`，这是事件归属任务的唯一入口。`src-tauri/src/work_task/engine.rs:73-86`
- 启动发送 prompt 前，用 `HashMap::insert` 把连接绑定到一个任务 generation。同一连接不能同时表达两张卡；后插入会覆盖旧映射。`src-tauri/src/work_task/engine.rs:1104-1109`
- `mark_running` 把这次 generation 的 conversation 和 connection 写到任务行。`src-tauri/src/db/service/work_task_service.rs:1237-1284`
- 任意被索引连接的第一次 `TurnComplete` 都会：

  - 移除索引；
  - 清空 awaiting；
  - 断开连接；
  - 把任务结算为 review/failed/canceled；
  - 释放 folder 并发槽。

  `src-tauri/src/work_task/engine.rs:1599-1703`

这与“复杂任务需要同一条外来会话多轮、长命运行”直接冲突：当前任务的生命边界就是一轮 ACP turn。

### 1.6 外来会话默认拿不到任务报告工具

- `task_progress` / `task_complete` 只注入到 `owner_window_label == "work_task"` 的 engine 启动连接。普通已有会话即使后来被指派，也没有这组工具。`src-tauri/src/acp/connection.rs:4588-4610`
- 两个报告函数也只能通过上述 engine `index` 找任务；查不到就返回“this session is not executing a work task”。`src-tauri/src/work_task/engine.rs:1746-1794`
- MCP listener 最终仍把 parent connection id 交给这套索引映射。`src-tauri/src/acp/work_task_tools.rs:37-51`，`src-tauri/src/work_task/engine.rs:4072-4099`

### 1.7 `sort_order` / pump 只按项目和卡片排序，不按目标会话串行

- 看板列表、Start All、queued pump 都按 `sort_order,id`。`src-tauri/src/db/service/work_task_service.rs:211-228`，`src-tauri/src/db/service/work_task_service.rs:392-430`
- `pump_folder` 只受项目级 `max_concurrent` 控制，然后取下一张 queued 卡启动。`src-tauri/src/work_task/engine.rs:674-747`
- auto-process 同样按 `sort_order,id` claim；预算统计的是 queued/preparing/running/awaiting/merging，没有“目标 conversation 已经在做另一张卡”的维度。`src-tauri/src/db/service/work_task_service.rs:822-929`
- 若多张卡指向同一外来会话，任务 pump 本身不会按 conversation 串行；而底层 prompt 发送在同一连接已有 turn 时会拒绝 `TurnInProgress`。`src-tauri/src/acp/manager.rs:1430-1453`
- merge 队列又是另一套顺序：按 `pending_merge.queued_at`，再按 task id，不读 `sort_order`。`src-tauri/src/work_task/engine.rs:2527-2547`，`src-tauri/src/work_task/engine.rs:3279-3287`

### 1.8 取消和删除假定任务拥有整个会话

- 取消成功后会：

  - kill 该 `run_seq` 的 setup 子进程；
  - 对索引中的任务连接调用 manager cancel；
  - 删除索引并 disconnect；
  - 如果任务 conversation 仍为 InProgress，把整个 conversation 状态写成 Cancelled。

  `src-tauri/src/work_task/engine.rs:573-635`，`src-tauri/src/work_task/engine.rs:3091-3100`
- 删除 active 卡也会先调用同一 `engine.cancel`。`src-tauri/src/commands/work_task.rs:338-389`

对共享的外来会话，这会停止并断开整个会话，而不只是撤销其中一张卡。

### 1.9 retry / return / requeue 都假设原 worktree 与原 session 是任务资产

- `retry` 的函数注释明确写“same worktree / session reused”。`src-tauri/src/work_task/engine.rs:482-523`
- `return_task` 把 review 卡 claim 成新 generation 后直接重新 launch。`src-tauri/src/work_task/engine.rs:526-570`
- canceled → todo 的 `requeue_canceled` 明确保留旧 worktree；下一次启动又会因 `conversation_id.is_some()` 进入 Retry。`src-tauri/src/db/service/work_task_service.rs:1052-1123`，`src-tauri/src/work_task/engine.rs:3308-3316`
- prompt 文案也写死了“continue in this worktree”“the worktree already contains that session’s work”“same worktree”。`src-tauri/src/work_task/engine.rs:3391-3445`，`src-tauri/src/work_task/engine.rs:3563-3597`

### 1.10 review、diff 与 preflight 假定结果都在任务 worktree

- turn 结束前的 diff stat 从 `worktree_folder_id` 对 `base_sha` 计算。`src-tauri/src/work_task/engine.rs:1889-1901`
- post-review preflight 只在任务 worktree 中运行；无 worktree 就静默返回。`src-tauri/src/work_task/engine.rs:1815-1887`
- `complete_task` 只接受 review；有 worktree 时检查相对 base 的可落地变化，没有 live worktree 时直接按“worktree is gone”完成。它不会检查外来会话是否已经直接改了项目根。`src-tauri/src/work_task/engine.rs:1936-1995`

### 1.11 merge 全链路必须有任务分支/worktree

- `merge_task_inner` 首先调用 `merge_coordinates`，要求：

  - 项目根；
  - live worktree folder；
  - `base_branch`；
  - `work_branch`。

  缺一即拒绝。`src-tauri/src/work_task/engine.rs:2160-2179`，`src-tauri/src/work_task/engine.rs:2439-2473`
- review → merging 会 bump `run_seq`，然后再启动一个 `LaunchMode::Merge` agent generation。`src-tauri/src/work_task/engine.rs:2254-2313`
- merge prompt 要求 agent 先处理 worktree 分支，再去项目根执行 merge/squash。`src-tauri/src/work_task/engine.rs:3447-3493`
- 成功判定要求项目根 HEAD 已变化，并且任务 `work_branch` 是新 HEAD 的祖先或两棵 tree 相同。`src-tauri/src/work_task/engine.rs:2386-2410`
- crash recovery 同样从 `merge_state`、项目 git truth、work branch 判断。`src-tauri/src/work_task/engine.rs:2735-2805`

外来会话若直接在主工作树修改，当前模型没有“待落地分支”；`merging` 的现有实现没有可执行对象。

### 1.12 cleanup 假定 worktree 和其中会话都归这张卡管理

- 无 `worktree_folder_id` 时 cleanup 基本是 no-op，只清理残留的 `cleanup_state`。`src-tauri/src/work_task/engine.rs:2841-2852`
- 有 worktree 时会删除该 worktree/branch。`src-tauri/src/work_task/engine.rs:2873-2917`
- 删除成功后，会把该 worktree folder 下的所有 conversations 重挂到项目根、删除 tabs、软删 folder，并清空任务 worktree 指针。`src-tauri/src/work_task/engine.rs:3118-3202`

如果所谓“外来会话”正好位于任务拥有的 worktree folder，这个清理不是只处理一条任务关联，而是处理 folder 下全部会话。

### 1.13 reconcile / 重启也依赖任务拥有连接和会话状态

- 运行中任务连接丢失后，reconcile 读取整条 conversation 的状态来决定 review/canceled/failed；普通外来会话的整体状态会被当作任务 generation 的状态。`src-tauri/src/work_task/engine.rs:2922-2984`
- 应用重启时，所有 queued/preparing/running/awaiting_input 卡直接标为 `failed(interrupted)`；仅 merging 留给 git recovery。它不会尝试根据外来 session 是否仍可恢复来保留任务状态。`src-tauri/src/db/service/work_task_service.rs:2167-2203`

## 2. 十态对已有会话任务的适用性

| 状态 | 语义是否适用 | 当前耦合 |
|---|---|---|
| `todo` | 适用 | 仍可表达未认领/未开始。实体定义见 `src-tauri/src/db/entities/work_task.rs:14-18`。 |
| `queued` | 有条件适用 | 可表达已 claim、待交付，但当前含义是“等待项目 task-engine 并发槽”，不是等待目标 session；实际 session 另有 PromptQueue。`src-tauri/src/db/entities/work_task.rs:17-19`，`src-tauri/src/work_task/engine.rs:674-747` |
| `preparing` | 当前含义不适用 | 定义就是创建 worktree、运行 init command、spawn agent CLI；已存在会话没有这组准备动作。`src-tauri/src/db/entities/work_task.rs:20-25`，`src-tauri/src/db/service/work_task_service.rs:1159-1195` |
| `running` | 适用 | 可表达会话正在处理卡；但当前实现绑定单一 connection/run_seq，第一次 TurnComplete 就结束任务。`src-tauri/src/db/service/work_task_service.rs:1237-1284`，`src-tauri/src/work_task/engine.rs:1599-1703` |
| `awaiting_input` | 适用 | 长任务确实可能等问题/权限；但当前只有 engine index 中的连接事件才会翻转。`src-tauri/src/db/entities/work_task.rs:28-30`，`src-tauri/src/work_task/engine.rs:1705-1741` |
| `review` | 语义适用、现有机械部分不适用 | “工作完成，等用户验收”成立；但当前进入 review 时同步做任务 worktree diff/preflight。`src-tauri/src/db/entities/work_task.rs:31-33`，`src-tauri/src/work_task/engine.rs:1629-1675`，`src-tauri/src/work_task/engine.rs:1815-1901` |
| `merging` | 对直接在主工作树工作的外来会话不适用 | 当前含义是将任务 work branch 落到项目根，而且是不可取消的独立 agent generation。无 worktree/branch 时 `merge_coordinates` 会拒绝。`src-tauri/src/db/entities/work_task.rs:34-36`，`src-tauri/src/work_task/engine.rs:2439-2473` |
| `done` | 适用 | 完成态本身成立。当前前端对 review 且无 worktree 的卡显示 Complete，后端按“worktree gone”走 review → done；不会验证主工作树变化。`src/components/tasks/task-acceptance.ts:28-42`，`src/components/tasks/task-actions.ts:114-139`，`src-tauri/src/work_task/engine.rs:1950-1967` |
| `failed` | 适用 | 会话处理失败/中断仍需要失败态。实体定义见 `src-tauri/src/db/entities/work_task.rs:37-40`。 |
| `canceled` | 状态适用，现有副作用不适用 | 卡可以取消，但当前取消会 cancel/disconnect 整个 session，并可能把 conversation 标为 Cancelled。`src-tauri/src/db/service/work_task_service.rs:1995-2053`，`src-tauri/src/work_task/engine.rs:607-630` |

明确不适用的是当前语义下的 `preparing`、`merging`；`queued` 和 `review` 的业务含义还能成立，但当前执行机械分别绑定 task pump 和 task worktree。

## 3. 向既有会话投递

有，非邮箱的现成能力是 `PromptQueue`。

### 普通 PromptQueue

- 入参直接以 `conversation_id` 指定已有会话，并携带 prompt blocks/display text。客户端不能伪造 source；普通命令固定落成 `User`。`src-tauri/src/models/prompt_queue.rs:7-31`，`src-tauri/src/models/prompt_queue.rs:115-128`
- `prompt_queue_enqueue_core` 持久化、广播快照并唤醒 runtime。`src-tauri/src/commands/prompt_queue.rs:26-36`
- 前端现有会话 composer 已使用这条 API。`src/lib/api.ts:2131-2138`，`src/hooks/use-message-queue.ts:217-248`
- 服务会校验 conversation 存在，然后把内容写入 `conversation_prompt_queue_item`。`src-tauri/src/db/service/prompt_queue_service.rs:358-445`
- 目标空闲时，worker claim 队首并调用 `send_prompt_linked_with_message_id`。`src-tauri/src/prompt_queue.rs:590-772`
- 目标关闭时，worker 会请求恢复这条已有 session；恢复逻辑要求已有 `external_id`，明确不会 fallback 到 `session/new`。`src-tauri/src/prompt_queue.rs:526-588`，`src-tauri/src/session_dispatcher.rs:136-180`，`src-tauri/src/session_dispatcher.rs:196-230`
- 目标忙时，普通 user/automation prompt 等到 idle；只有带 collaboration origin、`steer_if_supported`、纯文本且底层支持 native steering 的消息才会 steer。`src-tauri/src/prompt_queue.rs:621-640`，`src-tauri/src/prompt_queue.rs:811-913`
- automation 的 `queue_prompt` 已证明可以把定时内容投到既有会话，并使用同一 enqueue core。`src-tauri/src/automation/engine.rs:434-495`

### 是否产生“欠回复”

普通 PromptQueue 不产生：

- PromptQueue item 没有 `expects_reply` 或 obligation 字段。`src-tauri/src/models/prompt_queue.rs:83-103`
- 普通 enqueue 只写 queue 表，没有 collaboration delivery/obligation 写入。`src-tauri/src/db/service/prompt_queue_service.rs:358-445`
- 欠回复是在 collaboration delivery 中由 `expects_reply` 显式生成 `AwaitingReply`，然后才可能把对应 origin 放入 PromptQueue。`src-tauri/src/db/service/collaboration_service.rs:1518-1563`

### Agent 今天能否直接做这件事

- 当前没有 agent-facing 的普通 PromptQueue enqueue 工具；任务 MCP 面只有下节五个工具。`src-tauri/src/acp/delegation/tool_schema.json:422-527`
- Agent-facing 的既有会话投递只有 mailbox `send_message`：它默认 `expects_reply=true`，可显式设 `false`，但无论是否欠回复，它仍然是持久 mailbox letter/协作事件语义，正是新需求排除的路径。`src-tauri/src/acp/delegation/tool_schema.json:146-182`

因此：底层已有“非邮箱、无欠回复、可唤醒既有会话”的投递能力，但尚未暴露成任务 claim/assignment 工具，也未接到任务状态机。

## 4. MCP 面

当前任务相关工具共五个，均在 `src-tauri/src/acp/delegation/tool_schema.json` 定义：

| 工具 | 参数 | 定义 |
|---|---|---|
| `create_work_task` | 必填 `title`, `prompt`；可选 `agent_type`, `folder_path`, `profile`, `model` | `:423-455` |
| `list_tasks` | 可选 `folder_path`, `status`, `limit` | `:457-479` |
| `get_task` | 必填 `task_id` | `:481-493` |
| `task_progress` | 必填 `message` | `:496-507` |
| `task_complete` | 必填 `verdict`；可选 `summary` | `:510-527` |

暴露范围不同：

- `task_progress` / `task_complete` 属于 `tasks` feature，只给 task-engine 启动的会话。`src-tauri/src/acp/delegation/companion.rs:145-153`，`src-tauri/src/acp/delegation/companion.rs:189-203`
- `create_work_task` / `list_tasks` / `get_task` 属于 `taskboard` feature，可给普通会话。`src-tauri/src/acp/delegation/companion.rs:150-153`，`src-tauri/src/acp/connection.rs:3895-3912`
- 普通会话的 `AuthoringContext` 已包含调用者自己的 `conversation_id` 和 working directory；listener 从 launch token/parent connection 实时解析。新增 claim 工具不缺 claimant identity。`src-tauri/src/acp/chat_authoring.rs:46-56`，`src-tauri/src/acp/delegation/listener.rs:725-739`

现成 CAS claim 原语是：

- `work_task_service::claim_for_run(conn, id, from, actor)`。它 CAS `from → queued`、`run_seq + 1`，并清理上一 generation 的 failure/verdict/schedule/merge/preflight 等字段。`src-tauri/src/db/service/work_task_service.rs:667-677`，`src-tauri/src/db/service/work_task_service.rs:720-795`
- 若 claim 同时需要记录一条 action，兄弟函数是 `claim_for_run_with_action`。`src-tauri/src/db/service/work_task_service.rs:696-714`

限制：现成 claim 不写 `conversation_id`；其 UPDATE 列表中没有 ConversationId。因此“claim 与绑定 claimant session 原子完成”目前不是一个现成函数。`src-tauri/src/db/service/work_task_service.rs:720-769`

## 5. `folder_id` 约束与外来会话

结论分两层：

### 数据库约束层：不会直接冲突

- `work_task.folder_id` 是项目根的软引用，没有 hard FK；`conversation_id` 也是独立 nullable 列，没有约束要求任务 folder 与 conversation folder 相同。`src-tauri/src/db/migration/m20260801_000001_work_task.rs:24-27`，`src-tauri/src/db/migration/m20260801_000001_work_task.rs:57-61`
- 当前模型本来就允许二者不同：任务 `folder_id` 保持项目根，而 engine 创建的 conversation 位于任务 worktree folder。`src-tauri/src/work_task/engine.rs:1036-1050`
- `mark_running` 只 CAS status/run_seq 后写 conversation/connection，不校验 conversation.folder_id。`src-tauri/src/db/service/work_task_service.rs:1237-1284`

所以，指派一个 worktree folder 里的 session，并不会违反“卡只能挂项目根”；卡仍挂根，session 仍挂 worktree。

### 运行语义层：跨项目或跨 cwd 会冲突

- task engine 始终按 `task.folder_id` 选项目根、创建任务 worktree。`src-tauri/src/work_task/engine.rs:855-914`
- PromptQueue 则按 conversation 自己的 `origin_cwd`，否则按 conversation.folder 的路径恢复和投递。`src-tauri/src/prompt_queue.rs:692-721`，`src-tauri/src/session_dispatcher.rs:112-133`
- 因此，任务属于项目 A、外来 session 属于项目 B 时，数据库不会阻止，但 task engine 的 git/diff/merge 坐标与真正执行 cwd 会指向不同项目。
- 现有 chat-authoring 只在“创建任务时默认项目”这一步把调用者的 worktree folder 归一到其项目根；这不是 task↔conversation 一致性校验。`src-tauri/src/commands/chat_authoring.rs:83-152`
- 前端活动连接的 canonical key 还使用 `task.folder_id`，不是 conversation.folder_id；外来 session 属于其他 folder 且任务不保存 live `connection_id` 时，活动点可能找不到对应连接。`src/components/tasks/task-activity.ts:19-37`

## 6. 前端数据链

### 卡片和列表从哪里拿

- `TasksViewProvider` 调 `workTaskList(null)` 拉全量卡片，订阅 `task://changed` 后整表 refetch。`src/contexts/tasks-view-context.tsx:49-55`，`src/contexts/tasks-view-context.tsx:88-134`
- `TasksPage` 直接消费 provider 的 `tasks`，客户端做 folder/filter/grouping。`src/components/tasks/tasks-page.tsx:138-160`
- API 调用是 `work_task_list`。`src/lib/api.ts:3629-3635`
- 后端 list 从 `work_task_service::list` 读取，按 `sort_order,id`，再由 command 批量补 `worktree_missing` 和解析后的 `agent_type`。`src-tauri/src/db/service/work_task_service.rs:211-268`，`src-tauri/src/commands/work_task.rs:56-70`

### `conversation_id` 是否已到前端

已到：

- 后端 DTO 包含 `conversation_id` 和 `connection_id`。`src-tauri/src/models/work_task.rs:35-40`
- `to_info` 原样投影。`src-tauri/src/db/service/work_task_service.rs:112-151`
- TS `WorkTask` 包含二者。`src/lib/types.ts:1959-1984`
- 页面用 `conversation_id` 做活动连接查找和打开 session。`src/components/tasks/tasks-page.tsx:362-383`，`src/components/tasks/tasks-page.tsx:429-433`

### 显示“谁在做”当前缺什么

- 卡片和列表目前只渲染 `agent_type` 图标，不显示 conversation 标题/id。`src/components/tasks/task-card.tsx:314-389`，`src/components/tasks/task-card.tsx:478-490`，`src/components/tasks/task-row.tsx:165-180`
- 详情页按 `conversation_id` 额外拉 conversation detail，但只取 agent type 和 token usage；标题没有被保存或展示。`src/components/tasks/task-detail-sheet.tsx:228-267`，`src/components/tasks/task-detail-sheet.tsx:338-356`
- 全局 workspace store 已经有所有 conversation summaries；summary 包含 id、folder、title、agent、model、external id。`src/stores/app-workspace-store.ts:32-40`，`src/stores/app-workspace-store.ts:243-252`，`src/lib/types.ts:454-488`

所以前端不是完全缺数据，而是缺：

- 创建框中的 session 指派控件及筛选；
- `WorkTaskDraft` 的 `conversation_id` 输入字段；
- `conversation_id → DbConversationSummary` 的 join/render；
- 卡片/行上的 session title/id 展示；
- 跨 folder session 的活动连接 key 修正。

现有 editor 只有标题、agent/config、prompt 和项目 folder；提交 draft 也只有 `folder_id/title/config`。`src/components/tasks/task-editor-dialog.tsx:93-140`，`src/components/tasks/task-editor-dialog.tsx:245-266`，`src/components/tasks/task-editor-dialog.tsx:343-441`，`src/lib/types.ts:2046-2050`

## 7. 实现①指派与②认领的最小改动面

这里只列现有函数/文件触点，不替实现方式做取舍。

### 数据契约与持久化

- `src-tauri/src/models/work_task.rs:87-94`：`WorkTaskDraft` 接受创建时指派信息。
- `src/lib/types.ts:2046-2050`：前端 `WorkTaskDraft` 镜像。
- `src-tauri/src/db/service/work_task_service.rs:521-588`：`create` 当前强制 `conversation_id=None`，需改为校验并保存指派。
- `src-tauri/src/db/service/work_task_service.rs:591-623`：若允许编辑未运行卡的指派，`update` 也在这里。
- 仅复用已有 `conversation_id` 列不要求迁移；但当前没有显式“外来/engine-owned”字段，而 `launch_mode_for` 又把字段存在直接解释为 Retry。如何保存这一区别属于尚未拍板的表示层取舍。事实触点是 `src-tauri/src/work_task/engine.rs:3308-3316`。

### 引擎执行与生命周期

至少需要在同一个引擎文件分开“任务拥有的运行”与“外来会话承担的任务”：

- `TaskEngine::launch`：worktree、init、spawn/resume、conversation create、index、mark_running、prompt dispatch。`src-tauri/src/work_task/engine.rs:838-1185`
- `launch_mode_for`：不能继续仅按 `conversation_id` 判断 Retry。`src-tauri/src/work_task/engine.rs:3308-3316`
- `on_turn_complete` / `track_request` / `record_progress` / `record_complete`：当前按单连接、单 turn 结算。`src-tauri/src/work_task/engine.rs:1556-1794`
- `cancel`：外来会话任务不能沿用“取消并断开整个 conversation”的无条件副作用。`src-tauri/src/work_task/engine.rs:573-635`
- `retry` / `return_task` / `requeue_canceled`：当前都承诺续用同一 task worktree/session。`src-tauri/src/work_task/engine.rs:482-570`，`src-tauri/src/db/service/work_task_service.rs:1052-1123`
- `run_preflight` / `snapshot_diff_stats` / `complete_task`：无任务 worktree 时的验收数据与完成判定。`src-tauri/src/work_task/engine.rs:1815-1901`，`src-tauri/src/work_task/engine.rs:1936-1995`
- `merge_task_inner` / `merge_coordinates` / `recover_merging`：无 work branch 的卡不能进入当前 merge generation。`src-tauri/src/work_task/engine.rs:2152-2313`，`src-tauri/src/work_task/engine.rs:2439-2473`，`src-tauri/src/work_task/engine.rs:2735-2805`
- `cleanup_task` / `remove_worktree_locked` / `converge_worktree_removal`：只能处理 engine-owned worktree，不应把外来 session 当清理对象。`src-tauri/src/work_task/engine.rs:2810-2917`，`src-tauri/src/work_task/engine.rs:3118-3202`
- `reconcile_once` 和 boot reconcile：外来 session 的存活/恢复不能继续等同于 engine connection 的存活。`src-tauri/src/work_task/engine.rs:2922-3045`，`src-tauri/src/db/service/work_task_service.rs:2167-2203`
- `work_task_update_core::rename_task_conversation`：避免卡片改名影响外来 session。`src-tauri/src/commands/work_task.rs:236-330`

若复用现成非邮箱投递通道，调用点是 `prompt_queue_enqueue_core`；它还需要 `PromptQueueHandle` 才能 wake/resume。`src-tauri/src/commands/prompt_queue.rs:26-36`。当前 `TaskEngine` 构造器没有该 handle；构造触点为 `src-tauri/src/work_task/engine.rs:73-132`、桌面启动 `src-tauri/src/lib.rs:855`、server 启动 `src-tauri/src/bin/codeg_server.rs:546`。

### `claim_task` MCP 链路

- `src-tauri/src/acp/delegation/tool_schema.json:422-527`：新增工具名、`task_id` 等参数定义。
- `src-tauri/src/acp/delegation/companion.rs:189-203`：加入 `taskboard` allowlist。
- `src-tauri/src/acp/delegation/companion.rs:382-404`、`:940-993`：解析 `tools/call` 并发起 broker round-trip。
- `src-tauri/src/acp/delegation/transport.rs:229-295`、`:347-540`：新增 claim request、`BrokerMessage` variant 和 client round-trip。
- `src-tauri/src/acp/delegation/listener.rs:218-375`、`:725-798`：从 token 解析 claimant conversation，并 dispatch claim。
- `src-tauri/src/acp/chat_authoring.rs:46-56`、`:294-324`：在 taskboard access trait 上增加 claim 操作/结果类型。
- `src-tauri/src/commands/chat_authoring.rs:330-654`：生产实现；这里已经能拿 claimant `AuthoringContext.conversation_id`。
- `src-tauri/src/db/service/work_task_service.rs:667-795`：复用/扩展 `claim_for_run` CAS；当前原语不会原子写 claimant conversation，需要在此层补齐相应写入原语。
- `src-tauri/src/acp/connection.rs:3750-3804`、`:3895-3912`：`taskboard` feature 已存在；若 `claim_task` 归该组，不需要新增 feature 名，但普通 claimant 后续仍没有 `tasks` 组的 `task_progress/task_complete`，相关限制在 `:4588-4610`。

### 前端

- `src/components/tasks/task-editor-dialog.tsx:93-140`、`:245-266`、`:424-441`：指派 session 控件与 draft。
- `src/components/tasks/tasks-page.tsx:148-160`、`:435-443`：把 workspace conversation summaries 交给 editor，并提交。
- `src/components/tasks/task-card.tsx:314-389`、`:478-490`：卡片显示 session 身份。
- `src/components/tasks/task-row.tsx:165-180`：列表显示 session 身份。
- `src/components/tasks/task-detail-sheet.tsx:228-267`、`:625-658`：详情身份区域。
- `src/components/tasks/task-activity.ts:19-37`：跨 folder 指派后的连接 key。
- 十个 locale 的 `Tasks` 文案块均从 `src/i18n/messages/*.json:4715` 开始。

现有 `work_task_create`/`work_task_update` HTTP 和 Tauri 命令都是整块透传 `WorkTaskDraft`，若只是给 draft 增字段，不必为①另建路由。现有入口见 `src-tauri/src/web/handlers/work_task.rs:212-230`、`src-tauri/src/web/router.rs:1514-1519`、`src-tauri/src/lib.rs:1500-1501`。
