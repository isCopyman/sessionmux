<!-- 来源：codex 只读设计调研（2026-08-22 夜），未改任何文件。
     取代 KANBAN-ASSIGN-SESSION-RECON-2026-08-21 的"等拍板"状态：
     指派已有会话推迟到阶段三，先做任务自有 session 的多回合闭环。 -->

# 多回合任务卡状态机建议

**结论：值得现在改，但只改“什么才算任务完成”，不要先做已有会话指派。** 最小可行方案是不新增第十一种状态，而是把 `running` 从“一个 ACP turn 正在执行”改成“这张卡已开始、尚未显式交付”；普通 `TurnComplete(end_turn)` 只结束本轮，卡继续 `running`，连接和并发槽都保留。只有 agent 调用 `task_complete`，或用户点击“提交验收”，才进入 `review`。现有 `review → return → 再跑一轮` 虽然能勉强模拟多轮，但每轮都谎称“已经完成、等待验收”，还要求用户反复退回，不能算更便宜的等价方案。当前 return 确实会复用原 worktree/session 再启动一代，但入口只接受 `review`。`src-tauri/src/work_task/engine.rs:526-570`

## 1. 最小而完整的状态机改动

### 1.1 不增加状态，只改 `running` 的生命边界

保留现有十态和四列映射：

| 状态 | 建议语义 |
| --- | --- |
| `todo` | 未开始的 backlog |
| `queued` | 已认领，等待项目并发槽 |
| `preparing` | 创建/恢复 worktree、初始化、启动 agent |
| `running` | **卡片处于执行期；可以正在跑一轮，也可以停在两轮之间** |
| `awaiting_input` | 当前轮被问题、权限或计划审批阻塞 |
| `review` | agent 或用户明确表示工作阶段结束，等待验收 |
| `merging` | 正在落地任务分支 |
| `done` | 已完成 |
| `failed` | 执行失败，可重试 |
| `canceled` | 用户取消，可重新入列 |

当前实体已经定义这十态，`running` 本身没有更窄的持久化说明；`awaiting_input`、`review`、`merging` 才有明确的特殊含义。`src-tauri/src/db/entities/work_task.rs:4-43`。前端也已把 `preparing/running` 放在 In Progress 列，不需要改四列地基。`src/components/tasks/board-columns.ts:22-59`

状态转移改成：

```text
todo → queued → preparing → running ⇄ awaiting_input
                                │
                                ├─ 普通 end_turn、无 verdict → running（不转移）
                                ├─ task_complete(success|needs_review)
                                │    + 随后的 end_turn → review
                                ├─ 用户“提交验收”（仅空闲时）→ review
                                ├─ task_complete(blocked) + end_turn → failed
                                ├─ cancelled → canceled
                                └─ 其他异常 stop_reason → failed

review → merging → done
review → done（无须合并）
review → queued（退回修改，沿用现有路径）
failed → queued（重试）
canceled → todo（重新入列）
```

今天的 `on_turn_complete` 会先移除索引、清理 awaiting、断连，然后把没有 verdict 的正常 `end_turn` 也送入 `review`；这正是需要改变的地方。`src-tauri/src/work_task/engine.rs:1599-1675`。`task_complete` 目前已经只负责保存 verdict/summary，真正结算本来就是等随后的 `TurnComplete`，因此无需发明第二套完成协议。`src-tauri/src/work_task/engine.rs:1773-1794`，`src-tauri/src/db/service/work_task_service.rs:1371-1418`

具体改法：

1. `on_turn_complete` 先读取当前 generation 的 verdict，再决定是否结算。
2. `end_turn + verdict=None`：

   - 卡保持 `running`；
   - 保留 `index[connection_id] = (task_id, run_seq)`；
   - 保留 `connection_id`；
   - 不调用 `disconnect`；
   - 不调用 `pump_folder`；
   - 可写一条轻量的 `turn_finished` 事件，但不要覆盖任务级 `result_summary`。

3. 有 verdict、取消或异常时，才执行现有的移除索引、断连、结算、preflight 和 pump。
4. `run_seq` 表示“一次卡片执行 generation”，不再等同于单个 turn；同一连接上的若干正常 turn 共用一个 `run_seq`。失败后 retry、review 后 return 仍按现有 claim 逻辑递增 generation。当前 claim 已在 `from → queued` 时递增 `run_seq` 并清掉旧 verdict。`src-tauri/src/db/service/work_task_service.rs:667-795`

这条路线可行，是因为底层 ACP 连接本来就支持多轮：一轮结束后连接循环恢复为 `Connected`，没有要求退出进程。`src-tauri/src/acp/connection.rs:8674-8681`。下一条 prompt 只有在 `turn_in_flight=true` 时才会被拒绝，空闲连接可以接下一轮。`src-tauri/src/acp/manager.rs:1438-1453`。当前真正强制“一轮即断”的是任务引擎自己的 `disconnect`。`src-tauri/src/work_task/engine.rs:1605-1608`

### 1.2 什么结束一张卡

`TurnComplete` 不再是完成信号，而只是 turn 边界。结束工作阶段必须有明确意图：

- agent：调用现有 `task_complete`，随后本轮结束；
- human：新增“提交验收”动作，仅允许任务为 `running`、连接存在且 `turn_in_flight=false` 时执行；
- cancel/error：仍按现有规则进入 `canceled/failed`。

人工动作应由 `TaskEngine` 执行，而不是给前端一个任意 `set_status`：它需要复用 `snapshot_diff_stats`、`settle_review`、post-review preflight、索引清理和连接断开。现有“完成”命令只接受已经在 `review` 的任务，因此不能冒充这个新动作。`src-tauri/src/work_task/engine.rs:1936-1995`

不支持 MCP 的 agent 也必须有人工出口。当前 `task_progress/task_complete` 只注入 `owner_window_label == "work_task"` 且支持 MCP 的连接。`src-tauri/src/acp/connection.rs:4599-4617`

### 1.3 并发槽和连接

**建议两轮之间都保留。**

- 连接保持 `Connected`，让普通会话 composer 可以自然发送下一轮，并继续拥有同一个 task-scoped MCP token。
- 卡保持 `running`，因此继续计入 `max_concurrent`。当前并发统计本来就把 `running/awaiting_input/merging` 算作 active。`src-tauri/src/db/service/work_task_service.rs:467-483`
- 这意味着 `max_concurrent` 的产品语义变成“项目同时进行中的任务数/WIP 上限”，而不是“此刻正在生成 token 的 turn 数”。自动 claim 当前也把 queued、preparing、running、awaiting_input、merging 一起计入预算。`src-tauri/src/db/service/work_task_service.rs:900-916`

这是有意的最小取舍。若两轮之间释放槽，就必须新增一个 `active/parked` 状态，并让下一条普通会话 prompt 先经过任务引擎重新准入、拿槽、重建索引；否则用户从 composer 发出的下一轮会绕过并发限制。那是第二个设计，不应偷偷塞进本次改动。

### 1.4 `engine.rs` 和实体的具体触点

- 修改 `on_turn_complete` 的无 verdict 分支和清理顺序。`src-tauri/src/work_task/engine.rs:1599-1703`
- 修改 `reconcile_once`：连接丢失时，不能再因为 conversation 是 `Completed` 就自动送审；只有当前 generation 已有 verdict 才可送审，否则仍记 `failed(interrupted)`。当前逻辑会把 `Completed/PendingReview` 直接解释为 review。`src-tauri/src/work_task/engine.rs:2922-2984`
- 保留 boot reconcile：重启后 live connection 不存在，活动任务进入 `failed(interrupted)`，由用户 retry。`src-tauri/src/db/service/work_task_service.rs:2167-2203`
- 新增 engine 级 `request_review`，并在 Tauri command 与 HTTP handler/router 两侧暴露。任务引擎本来就在桌面和 server 启动路径各构造一次。`src-tauri/src/lib.rs:852-866`，`src-tauri/src/bin/codeg_server.rs:544-555`
- 更新任务 prompt，明确“一个普通回复结束时不要调用 `task_complete`；只有整张卡已完成或阻塞才调用”。当前文案只说“right before you finish”，不足以区分 turn 与 task。`src-tauri/src/work_task/engine.rs:3522-3536`
- `work_task` 表和 `WorkTaskStatus` 不加字段、不加状态，只更新实体注释和前后端类型注释。当前实体字段已足够表达持久 session、live connection 和 generation。`src-tauri/src/db/entities/work_task.rs:63-72`

尚需实测而不能只靠源码下结论的一点：各 adapter 是否都能让 CLI 在一次 `end_turn` 后长期稳定驻留。共享 ACP 循环支持这样做，但外部 harness 可能自行退出。实现者应对每个受支持 agent 做“两次 prompt、同一 connection/session”的二值探针；退出者按现有 connection-loss 路径进入 `failed(interrupted)`，不要为它单独造后台保活协议。

## 2. Session 所有权

### 2.1 第一片只支持任务自有 session

本次多回合改动不需要“指派已有 session”。先让 engine 自己创建的 session 正确活过多轮，已经解决真实问题，而且保留当前完整的 worktree、diff、preflight、merge 和 cleanup 契约。

不能用 `conversation_id` 是否存在表示所有权：它现在同时被解释为“以前运行过，所以是 Retry”。`src-tauri/src/work_task/engine.rs:3308-3316`。官方创建路径又强制把它置空，随后只有 engine 在启动时写入。`src-tauri/src/db/service/work_task_service.rs:521-588`，`src-tauri/src/db/service/work_task_service.rs:1237-1284`

### 2.2 如果以后支持已有 session，必须新增显式 discriminator

建议新增：

```text
session_ownership TEXT NOT NULL
  values: task_owned | borrowed
  default: task_owned
```

不要用 nullable bool，也不要从 worktree 是否存在反推。`conversation_id` 表示“绑定了谁”，`session_ownership` 表示“卡有权对它做什么”，是两个独立事实。

所有权一旦任务开始即不可修改；只允许 pristine `todo`（`run_seq=0`、无 worktree）选择 borrowed session。

| 操作 | `task_owned` | `borrowed` |
| --- | --- | --- |
| Fresh start | 沿用现有路径：创建任务 worktree/branch，以 `owner="work_task"` 启动 agent，创建并锁定标题的 conversation。`src-tauri/src/work_task/engine.rs:906-934`、`:989-1075` | 不进入 `TaskEngine::launch`，不创建 worktree/branch；用 PromptQueue 给目标 conversation 投递任务说明 |
| 多轮 | 同一 connection、conversation、worktree、`run_seq` | 由原 session 自己的 connection/runtime 处理；卡只作关联跟踪 |
| Retry | 恢复同一任务 worktree/session；允许现有 resume fallback 创建新的**任务自有** session。`src-tauri/src/work_task/engine.rs:955-1033` | 只向同一 conversation 再投递；绝不 fallback 到 `session/new`。现有 session dispatcher 已坚持“无 external id 就不新建替身”。`src-tauri/src/session_dispatcher.rs:136-180` |
| Cancel | 可以 cancel turn、disconnect，并按当前规则收敛任务自有 conversation。`src-tauri/src/work_task/engine.rs:573-635` | 只取消卡和尚未 dispatch 的该卡队列项；若 prompt 已进入 session，不中断、不取消、不标记整个 conversation |
| Rename | 可保留当前卡名同步到任务自产 session 的行为。`src-tauri/src/commands/work_task.rs:236-330` | 永不改 borrowed session 标题 |
| Review/merge | 使用任务 worktree 的 diff/preflight/merge | 不提供 merge；人工确认后只能 `review → done`，并明确“未由 codeg 验证或合并代码” |
| Cleanup/delete | 可删除任务拥有的 worktree/branch。`src-tauri/src/work_task/engine.rs:2810-2917` | 只删任务关联；不得触碰 session、connection、folder、worktree 或 branch |

Borrowed 创建时至少校验：

1. conversation 存在且未删除；
2. 它的有效 cwd 属于任务项目根或该项目的已登记 worktree；
3. 同一个 conversation 同时最多绑定一张非终态 borrowed 卡。

PromptQueue 已能持久投递、唤醒现有 conversation。`src-tauri/src/commands/prompt_queue.rs:26-36`。它会在目标空闲时发送，忙时等待，并校验 live connection 的 cwd。`src-tauri/src/prompt_queue.rs:590-640`、`:667-721`

但 borrowed session 当前拿不到 `task_progress/task_complete`，因为普通连接没有 `tasks` feature。`src-tauri/src/acp/connection.rs:4604-4617`。因此最小 borrowed 模式只能由人提交验收；若 owner 要求 borrowed agent 自己结束卡，就需要另行设计带 task-id 和 claimant-identity 校验的工具，不能把它混入第一片。

## 3. 迁移

### 3.1 多回合核心无需数据库迁移

第一片不增加状态或列，现有行原样保留：

- `todo/review/failed/canceled/done/merging` 不改；
- 不把任何历史 `review` 猜测性地改回 `running`；
- 不重写 `run_seq`、conversation 或 worktree 指针。

### 3.2 更新时仍在飞的卡

凡更新伴随进程重启，现有 boot reconcile 会把 `queued/preparing/running/awaiting_input` 置为 `failed(interrupted)`，而不是假装它们已完成。`src-tauri/src/db/service/work_task_service.rs:2167-2203`

建议保留这一行为：

- session、worktree、branch 都保留；
- 用户点击 retry 后进入新的 `run_seq`；
- `launch_mode_for` 因已有 `conversation_id` 走 resume/retry，复用已有工作。`src-tauri/src/work_task/engine.rs:3308-3316`
- 不尝试在迁移期间恢复进程内 connection/index；两者本来就不跨进程持久。`src-tauri/src/db/entities/work_task.rs:63-72`

这会让一次更新产生可见的“被更新中断，请重试”，但不会错误送审，也不会丢工作。

### 3.3 Borrowed 模式以后单独迁移

若第二阶段获批，再增加 `session_ownership`：

- 所有既有行回填 `task_owned`；
- 新建行默认 `task_owned`；
- 只有显式选择已有 session 的新卡写 `borrowed`；
- 不从路径、标题或 conversation 状态猜历史所有权。

这种回填对官方数据路径是可靠的，因为当前创建服务不接受 conversation，只有任务 engine 后续创建/写入它。`src-tauri/src/db/service/work_task_service.rs:521-588`。迁移需登记到现有 migrator 顺序末尾；当前注册表在 `src-tauri/src/db/migration/mod.rs:68-139`。

## 4. 这次不要做什么

1. **不要新增 `active/parked` 状态。** 保留十态和四列；多轮靠“显式完成”而不是新列解决。`KANBAN-DESIGN-2026-08-21.zh-CN.md:72-77` 中“不改十态/四列”仍可保留，但“状态机不用改”的结论应改成“状态集合不改，完成边界要改”。

2. **不要在第一片做 session picker、`claim_task` 或 borrowed dispatch。** 它们会立即牵出 PromptQueue、cwd 校验、动态工具权限、同 session 串行、取消语义和无 worktree 验收，远大于多回合核心。

3. **不要新增通用 `set_status/update_task` MCP。** 状态仍由 engine CAS 收口；这一点与旧设计一致。`docs/session-workbench/KANBAN-DESIGN-2026-08-21.zh-CN.md:45-48`

4. **不要再实现 list/get。** 旧设计仍写着 agent 没有读取工具，但当前 schema 已经包含 `list_tasks/get_task`。`src-tauri/src/acp/delegation/tool_schema.json:457-493`，其 taskboard allowlist 也已存在。`src-tauri/src/acp/delegation/companion.rs:198-201`

5. **不要为了本改动重做分组、attention 分层或活动点。** 它们不是状态机前置条件；活动点当前已经能把 `running + Prompting` 显示为脉冲、`running + Connected` 显示为静止，正好适配多轮间歇。`src/components/tasks/task-activity.ts:39-52`

6. **不要改 merge、preflight、worktree cleanup。** 对 `task_owned` 卡继续使用现有链路；borrowed 模式未获批前不进入这些路径。

7. 不引入外部任务系统、不改成仓库 Markdown、不把 Room/Collection 合进任务生命周期，也不增加依赖。

## 5. 分阶段施工顺序

### 阶段一：任务自有 session 的多回合闭环——第一片即可发布

单独 agent 的完整任务范围：

- 修改 `engine.rs::on_turn_complete`：无 verdict 的正常 `end_turn` 保持 `running/index/connection`；
- 修改 connection-loss reconcile：无 verdict 不得因 conversation 为 Completed 而自动送审；
- 新增空闲态人工“提交验收”的 engine 方法；
- 在 Tauri commands、HTTP handlers/router、前端 API 和任务动作中同时接入该操作；
- 更新 engine prompt，解释 turn 完成与 task 完成的区别；
- 更新实体、TS 类型和 UI 文案注释，不加 schema、不加状态；
- 补足多轮、显式完成、人工送审、取消、断连、重启、并发 WIP 的状态机测试；
- 分别通过 desktop 和 `--no-default-features --bin codeg-server` 的编译/测试门禁。

验收场景必须包括：

1. 第一轮未调用 `task_complete`：卡仍为 `running`，连接为 `Connected`；
2. 同一连接成功发送第二轮，`run_seq` 不变；
3. 调用 `task_complete(success)` 后的 `end_turn` 才进入 `review`；
4. 空闲时人工提交验收成功，turn 进行中时被拒绝；
5. 多轮间仍占并发槽，后续 queued 卡不越过 `max_concurrent`；
6. 连接丢失或重启不会把无 verdict 的卡误送审；
7. cancel 后索引、连接、conversation 与任务状态按 task-owned 规则收敛。

本阶段明确不含：已有 session 指派、PromptQueue 任务投递、agent claim、槽位停驻释放、新列、新状态、看板分组改造。

### 阶段二：多 harness 实机兼容与文案收口

由另一 agent 在阶段一合并后执行：

- 对每种支持的 adapter 做“同 connection 连续两轮”探针；
- 记录哪些 harness 能稳定驻留，哪些会在 `end_turn` 后退出；
- 对退出者只改善错误提示和 retry 引导，不为单个 adapter 重写 engine；
- 走查桌面与 server/websocket 两种 UI：活动点、提交验收、取消、更新中断恢复；
- 校正 `max_concurrent` 的设置文案为“同时进行中的任务/WIP”，避免用户以为它只是瞬时进程数。

阶段二不改变第一阶段的状态集合和持久化契约。

### 阶段三：可选的 borrowed session 关联——需要再次单独拍板

只有 owner 在体验过 task-owned 多回合后仍明确需要“把卡交给已有 session”，才实施：

- 新增并迁移 `session_ownership=task_owned|borrowed`；
- 增加同项目/cwd 校验和单 session 单非终态卡约束；
- borrowed start/retry 走 PromptQueue，不走 task engine launch；
- borrowed cancel/cleanup/rename 永不影响原 session 或其 worktree；
- UI 明示“关联任务，不由任务引擎管理代码或合并”；
- 完成动作先只给人，不增加通用 agent 写状态工具。

若 owner 的真实要求是“borrowed agent 也能自行汇报进度、结束任务并进入完整 merge 链”，应停止阶段三：那已经不是关联模式，而是把运行中的 session 动态纳入任务引擎所有权，需要另一份设计和新的授权边界。
