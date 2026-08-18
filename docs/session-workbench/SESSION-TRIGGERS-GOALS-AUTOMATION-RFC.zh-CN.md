# Codeg Session Trigger、Goal 与 Automation RFC

> 状态：部分已实现——统一消息调度与催办配额修复已落地（见第 0 节对账）；Timer 的义务感知
> 退避与刹车机制已于 2026-08-19 拆除，现为纯退避 + `timer.reset_delay`（见 0.3）。
> 其余章节仍是方向性设计  
> 更新时间：2026-08-19  
> 适用范围：已有 Session 的自动续跑、提醒、计划执行与消息唤醒  
> 相邻设计：[Session 间通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md)、
> [Host 控制面 RFC](./HOST-CONTROL-SURFACE-RFC.zh-CN.md)、
> [Workbench 多视图同步 RFC](./WORKBENCH-LAYOUT-SYNC-RFC.zh-CN.md)、
> [群聊面板与 Session 协作 RFC](./GROUP-CONVERSATION-RFC.zh-CN.md)

> 第 0 节记录 2026-08-18 已落地的调度实现与原稿偏差，代码是这部分的实现事实源；
> 第 1 节起的其余内容仍只提出产品和架构方向，实施前必须结合当时的迁移、测试和并行
> 开发状态重新核对。

## 0. 2026-08-18 对账：统一消息调度已落地

本节对账迁移 `m20260818_000001_message_scheduler` 落地的三块实现，并记录与原稿的偏差。
实现事实源：[`prompt_queue.rs`](../../src-tauri/src/prompt_queue.rs)（worker claim）、
[`prompt_queue_service.rs`](../../src-tauri/src/db/service/prompt_queue_service.rs)（排序与准入）、
[`session_timer.rs`](../../src-tauri/src/session_timer.rs)（fire 判定链）、
[`session_timer_service.rs`](../../src-tauri/src/db/service/session_timer_service.rs)（claim_fire/reset_delay；auto_pause 已成仅测试引用的遗留代码，2026-08-19 对账）、
[`collaboration_service.rs`](../../src-tauri/src/db/service/collaboration_service.rs)（义务查询与催办配额）。

### 0.1 队列类别调度：user > letter/reminder > timer，类内 FIFO

`conversation_prompt_queue_item` 新增 `source` 列（`user | collaboration | reminder | timer`）。
调度规则：

- **claim 顺序 = 类别优先，类内 FIFO**：user(0) > collaboration/reminder(1) > timer(2)。
  你自己的消息永远先于自动化；自动化永远插不到你期待 Agent 读的信件前面。
- 排序常量 `CLASS_ORDER_SQL` / `CLASS_ORDER_SQL_Q` 被 snapshot、send_is_admitted、claim、
  reorder 四处共用——**改任何一处必须同步全部**，否则“准入门检查的头 ≠ worker 实际
  claim 的头”。
- `EnqueuePromptQueueItem.source` 是 `#[serde(skip_deserializing)]`：客户端伪造不了类别，
  只有宿主运行时（timer/reminder）能在 Rust 里设置。
- **局部优先级**：人类拖拽只能改自己类内顺序。快照按类别序返回，把 timer 项拖到自己
  消息前会被快照“弹回”，类内拖拽照常。
- `claim_first_steerable`：忙碌时定向 claim 第一个带 `steer_if_supported` hint 的信件项，
  修掉了“steer 信件被人类排队草稿挡死就永远注入不了”的旧缺陷。

### 0.2 投递激进度三档，与排序解耦

排序（谁先执行）和激进度（是否打断正在跑的 Turn）是两个正交轴：

| 档位 | 行为 | 触发方 |
|---|---|---|
| 等 | 排队，Session 空闲时按类别序执行 | 默认，所有来源 |
| 显式 steer | 忙碌时注入正在跑的 Turn（`steer_if_supported`） | 发信 Agent 显式选择 |
| 显式打断 | 中断当前 Turn | 仅人类显式操作 |

`urgency` 字段永不参与调度——它只是 UI 展示信号。这维持了第 7.4 节“中断不是 Timer
的职责”的原判断。

### 0.3 Timer 退避（2026-08-19 对账：义务感知退避已拆除）

本节原记“义务感知退避”：fire 前读 `latest_mailbox_info_at` 与 `outbound_awaiting_summary`
判断“在等回信”，等回信时按 max(grace, 300s)×2^strike 退避、strike 达 3 置
`auto_paused_at` 刹车、回信或用户编辑复活。该机制已被提交 **34ae1c36** 拆除——邮件、群帖
和 `@` 不再改变 timer 节律，通信不再刹停 timer；d81504fc 引入的“等待期一次巡检 poke 后
停靠”也一并移除。`auto_pause` 只剩测试引用的遗留代码，无生产调用方。

现行为（实现事实源：[`session_timer.rs`](../../src-tauri/src/session_timer.rs)）：

1. 宽限期未到 → 跳过。间隔 = `idle_grace` × 2^strike，上限 30 分钟
   （`MAX_REMINDER_DELAY_SECS = 1800`，`reminder_delay_secs`）。
2. Agent 有进展、或有新信息解锁目标时，自己调 `timer.reset_delay` 把间隔重置回
   `idle_grace`；仍在等待、无事可做就不调，间隔继续翻倍。
3. strike ≥ 1 起，fire 正文 = 用户配置文本原样 + 宿主附注（第 N 次提醒、下一档间隔、
   提示可调 `timer.reset_delay`）。
4. 队列里已有 queued 项 → 让位跳过（不叠加第二轮）。
5. 运行时缺失（被 idle_sweep 回收）→ 照样入队 + 唤醒，走 dispatcher ensure/resume，
   与信件同路。

阻塞式 `wait_message` 维持第 12 节的暂缓判断——退避 + 来信经 dispatcher 唤醒已经等价，
且不烧模型。

### 0.4 催办配额按“欠债周期”重置（修复“已读未回不提醒”）

原缺陷：`reminder_repeat_count`（上限 3）按会话终身累计、欠债不清就永不重置。未读阶段
每 5 分钟 steer 一次就烧光配额，信被读后进入“已读未回”阶段时一次也催不出来。修复：
`list_overdue_reminder_targets` 计算 `newest_due_at`（欠债集合里每个成员的到期时刻：
未读 = created_at，已读未回 = agent_received_at + 5min，取最大）；它晚于 `reminder_last_at`
即视为**新欠债周期，配额清零重算**（5 分钟冷却照旧防刷屏）。

### 0.5 与原稿的偏差

| 原稿 | 落地现实 |
|---|---|
| 7.1：第一版默认 one-shot while idle | 落地为 repeat-while-idle 默认，靠 0.3 的退避约束重复消耗 |
| 5.1 场景：空闲 15 分钟后提醒 | `idle_grace_secs` 默认 2 秒——Turn 结束即续，等待场景由退避接管 |
| 12：暂缓“没有限额、停止条件和失败暂停的永久自动续跑” | 约束曾为“等待退避 + 3 次刹车 + 可见 auto_pause”；2026-08-19 对账：刹车机制已删除（34ae1c36），现为纯退避 idle_grace×2^strike（30 分钟上限）+ `timer.reset_delay` |
| 7.3：等待外部消息时不应让 Goal 空轮询 | timer 曾读义务表感知“在等回信”；2026-08-19 对账：该耦合已拆除，等回信期间靠翻倍退避消解空轮询 |

### 0.6 Corner case 对账表

| 场景 | 行为 |
|---|---|
| 下发任务等回信，无新信息 | 间隔按 idle_grace×2^strike 翻倍（上限 30 分钟）；Agent 有进展可自行调 `timer.reset_delay` 回最短档。3 次刹车 / 回信复活机制已删除（2026-08-19 对账） |
| 回信与 fire 同秒到达 | “新信息”判据随义务感知退避一并删除（2026-08-19 对账）；来信直接经 dispatcher 唤醒，不经 timer |
| 退避期间运行时被 idle_sweep 回收 | 照样入队 + dispatcher 唤醒，与信件同路 |
| 队列已有 queued 项 | timer 让位跳过，不叠加第二轮 |
| steer 信件被人类排队草稿挡住 | `claim_first_steerable` 定向领取，不再被挡死 |
| 用户把 timer 项拖到自己消息前 | 快照按类别序弹回，类内拖拽照常 |
| 未读阶段烧光催办配额后信被读 | 已读未回构成新欠债周期，配额清零重催 |
| 纯本地 goal 循环 | strike 随未 reset 的 fire 累积、间隔翻倍；与出站义务无关（2026-08-19 对账） |
| 用户编辑 timer | update 无条件清 strike，回到最短档；auto_pause 机制已删除（2026-08-19 对账） |
| 服务重启时 timer 正在退避等待 | 启动播种“自进程启动起空闲”边界，等满一个正常间隔后照常 fire（重启前 idle 内存态丢失曾导致永久哑火，2026-08-18 修复） |
| 用户停止 turn 后立刻发新消息 | cancel 冻结队列防旧项抢跑；用户亲手 enqueue 自动解冻（cancel/stop 类原因白名单），新消息按 user 类立即领先；collaboration_interrupt 的等待暂停不受用户输入影响 |

### 0.7 前端呈现与尚未落地项

前端已同步落地：`PromptQueueItem.source` 与 `SessionTimer.strikeCount/autoPausedAt/autoPauseReason`
已镜像进 [`types.ts`](../../src/lib/types.ts)。timer 卡片曾在 `autoPausedAt` 非空时显示 amber
退避提示与“立即恢复”按钮——2026-08-19 对账：auto_pause 机制已随 34ae1c36 删除，这段是前端
遗留展示（文案已标注“旧版等待规则”），待清理。

尚未落地：转写增量启发式（判定“本地零进展”后刹车纯本地 goal 循环）记为 v2；真机
冒烟见交接文档
[`HANDOFF-2026-08-18-message-scheduler.zh-CN.md`](./HANDOFF-2026-08-18-message-scheduler.zh-CN.md)。

### 0.8 已知边界（显式接受，不在本轮修）

**新信件串可绕过所有按串记账的刹车。** 链深目前只按回复链记账（常量 =4，尚无强制拦截，
2026-08-19 对账），催办配额按欠债周期重置：两个 Agent 若被提示词驱动成“收到信就开新串回敬”，
每串深度 1、每封信都是新欠债，现有机制不会熄火——这是按串/按周期记账的结构性盲区，不是实现 bug。
真正的防线是**会话级自动 turn 预算闸**（如每小时自动触发上限 + 全局熔断可见化），
涉及新表、配置面与 UI，记为 v2 候选。当前依赖：对喷双方都要真实消耗自己的 turn，
且人类在信箱 UI 与会话列表中能看到异常流量。

## 1. 结论

Codeg 值得引入 SessionDock 的 Timer 能力，但不应把所有功能都改名为 Goal，也不应把
`Goal` 简化成一种 `idle timer`。

更合适的设计是：

```text
用户看到的三个功能                 后台共享的基础设施

Goal：持续做，直到完成       ┐
Timer：到时提醒或唤醒        ├── Session Trigger Service ── Session Prompt Queue
Automation：计划启动一项工作 ┘             │
                                             └── 状态、去重、暂停、恢复、重试
```

也就是说，**底层整合，产品语义分开**：

- `Goal` 解决“是否已经完成”，通常一个 Session 同时只有一个活动 Goal；
- `Timer` 解决“什么时候再提醒或唤醒”，一个 Session 可以同时拥有多个 Timer；
- `Automation` 解决“什么时候启动一项独立工作”，继续保留 Codeg 现有页面和运行记录；
- Session 间消息解决“发生了什么事件”，应走 Delivery Router，而不是伪装成 Timer。

第一阶段应先完成 **Goal 的统一产品表面和 `turn_idle` 续跑链路**。Goal 的正常触发点是一轮
Turn 完成并进入 idle 的当下，不需要再等待 15 分钟；“空闲 15 分钟后检查”属于可选的
`idle_for` watchdog/Timer，不是 Goal 的默认机制。Codeg 已经拥有 Codex 原生 Goal 的状态投影，
因此可以先把创建、编辑、暂停、恢复、清除、限额和统一队列语义做完整，再增加多 Timer。

这不表示立刻为每个 Harness 重写一套完成度判断。支持原生 Goal 的 Provider 仍以原生状态为事实，
Codeg 负责统一呈现和队列仲裁；Host-managed Goal 只在 Provider 缺少原生能力时作为后续降级。
Timer 随后复用同一 Trigger Service 和 Prompt Queue，覆盖提醒、watchdog、周期检查和绝对时间唤醒。

## 2. 为什么不能只做一个“Goal（idle timer）”

两者看起来都会向 Session 注入下一条提示，但终止条件完全不同。

| 能力 | 触发条件 | 结束条件 | 典型数量 | 是否理解任务语义 |
|---|---|---|---:|---|
| Goal | Turn 结束或 Session 进入空闲 | 目标完成、用户暂停、预算耗尽或阻塞 | 每 Session 0 或 1 个活动 Goal | 是 |
| Idle Timer | 连续空闲达到一段时间 | 用户暂停/删除；可选触发一次后自动结束 | 每 Session 可有多个 | 否 |
| Interval Timer | 距上次触发达到固定间隔 | 用户暂停/删除 | 每 Session 可有多个 | 否 |
| At Timer | 到达一个绝对时间 | 成功触发一次后结束 | 每 Session 可有多个 | 否 |
| Automation | cron、手动或未来事件 | 本次 Run 结束；计划本身可继续启用 | 全局可有多个 | 只执行保存的任务 |

Goal 需要完成度判断器；Timer 只负责时间和投递。把二者做成同一开关会导致两个问题：

1. 普通提醒也被迫消耗模型判断“目标是否完成”；
2. Goal 变成永远重复的一条提示，无法表达成功、阻塞、预算限制和完成状态。

因此可以让 Goal 在后台使用 `turn_idle` Trigger，但不能说 Goal 就是 Timer。

## 3. 当前 Codeg 已经有什么

### 3.1 Codex Goal 目前是原生能力的投影

Codeg 当前没有独立执行通用 Goal。Codex 的 Goal 状态来自
`session_info_update._meta.codex.goal`，Codeg 将它转换为合成的 `create_goal` / `update_goal`
事件供 Goal Card 展示。暂停和清除调用 Codex 私有的
`_codex/session/goal_control`；创建、编辑和恢复仍通过 `/goal` Prompt 完成。

实现依据：

- [`codex_goal.rs`](../../src-tauri/src/acp/codex_goal.rs)
- [`connection.rs` 的 Goal control](../../src-tauri/src/acp/connection.rs)
- [`goal-control-context.tsx`](../../src/components/message/goal-control-context.tsx)

这套适配应保留为 `native_goal` Provider Adapter，不应把 Codex 的 Goal 状态复制成第二份
Codeg 自有事实。

### 3.2 当前 Automation 是“保存并计划启动一项工作”

当前 `automation` 只有 `schedule` 和 `manual` 两种 Trigger；Action 是
`launch_session` 或 `enqueue_task`。Automation Engine 每 30 秒扫描 cron，启动无界面 Session
或向任务板入队，并记录每次 Run。它不是“向一个已有 Session 定时发送 Follow-up”。

实现依据：

- [`TriggerKind`](../../src-tauri/src/db/entities/automation.rs)
- [`AutomationAction`](../../src-tauri/src/models/automation.rs)
- [`AutomationEngine`](../../src-tauri/src/automation/engine.rs)

Automation 已经提供了可复用的服务器唯一所有者、持久状态、并发锁、Run 记录、失败恢复和定时
扫描基础，但当前表中还有 Folder、Git 隔离、Branch 和 Composer Snapshot 等任务启动语义。
不能为了少建一张表，直接把这些字段解释成 Session Timer。

### 3.3 当前 Session 消息队列是 Timer 的必要投递底座

Timer 到期不应从前端直接判断空闲并调用 ACP。它必须进入同一个服务器权威 Session Prompt
Queue：

- Session 空闲时由队列原子领取并创建 Turn；
- Session 忙碌时进入队列，不打断当前 Turn；
- 多窗口只投影同一队列，不各自保存或重复触发；
- `TurnBusy` 竞争时回队，而不是丢失或重复执行。

这与 [Workbench 多视图同步 RFC](./WORKBENCH-LAYOUT-SYNC-RFC.zh-CN.md) 和
[Session 间通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md) 的既有约束一致。

## 4. SessionDock Timer 提供了哪些已验证语义

SessionDock 当前已经实现了可参考的最小系统：

- Timer 是 Broker 驱动的直接系统注入，不写 Mailbox，也不创建回复义务；
- 支持 `interval` 和 `idle` 两种模式；
- `idle` 只在目标 `available + idle` 时计时，忙碌或离线会重置空闲窗口；
- 目标持续空闲时，每经过一个 timeout 可以再次触发；
- Timer 具有独立 ID，同一 owner/target 可以同时存在多个 Timer；
- 支持创建、编辑、暂停、恢复、删除和按目标查看；
- Timer 会随 Session durable rekey，并在 Session 删除时清理；
- 状态持久化，重载后仍能恢复。

源码依据：

- [`SessionTimerStore.ts`](../../../../../session-dock/src/core/SessionTimerStore.ts)
- [`RelayCoordinator.processSessionTimers()`](../../../../../session-dock/src/vscode/RelayCoordinator.ts)
- [`session-timer-v0.md`](../../../../../session-dock/docs/notes/session-timer-v0.md)
- [`sessiondock-timer` Skill](../../../../../session-dock/resources/shared/skills/sessiondock-timer/SKILL.md)

SessionDock 早期文档曾写“同一 owner/target 只有一个 Timer”，但当前 Store、CLI Skill 和 Inspector
已经支持多 Timer。Codeg 应以当前实现为参考，不继承过时的一槽限制。

## 5. 用户场景

### 5.1 长任务空闲后继续，而不是停在状态汇报

用户让一个 Session 做较长的调研，并设置：

```text
空闲 15 分钟后提醒：
“检查剩余 TODO。如果存在可执行步骤，直接继续；只有完成或需要用户决策时才停止。”
```

Session 正在工作时不注入。它连续空闲 15 分钟后，提醒进入该 Session 的队列并创建下一轮。
用户可以在 Inspector 中暂停、编辑或删除提醒。

这是 `idle_for + resume_session_with_message`，不是完整 Goal。它不判断论文调研是否真的完成。

### 5.2 同一 Session 同时拥有宏观和微观提醒

一个 Session 可以同时设置：

- 空闲 30 分钟：检查总目标是否仍有未完成部分；
- 每 2 小时：保存进度并更新阶段总结；
- 明天 9:00：检查外部实验或审查结果。

三个 Timer 都引用同一个稳定 Session ID，分别显示下一次触发时间。多个 Timer 同时到期时，
它们按 Session Queue 顺序执行；界面不能启动三个并发 Turn。

### 5.3 用 Goal 表达“做到完成为止”

用户设置：

```text
目标：完成当前设计文档的剩余章节并通过检查。
限制：最多 20 轮或 2 小时；需要用户选择时暂停。
```

每个 Turn 结束后判断：

- 已完成：标记 Complete，不再续跑；
- 有下一步：排入一条继续执行 Prompt；
- 需要用户、权限或外部状态：Paused/Blocked；
- 达到轮数、时间或 Token 上限：Budget limited。

有原生 Goal 的 Harness 优先使用原生能力。Host-managed Goal 只作为能力缺失时的降级方案。

### 5.4 定期启动独立检查

用户希望每天早上生成一份独立的项目检查报告。这仍使用当前 Automation：按 cron 启动新的
无界面 Session，保留独立 Run 记录。它不应悄悄写入某个长期研究 Session。

### 5.5 另一个 Session 发来协作请求

“Fable 完成审查并发回意见”不是 Timer。消息先进入 Delivery Router；目标忙碌时排队，空闲时按
`invoke_when_idle` 策略启动。时间只可以作为催办或超时辅助，不能替代消息身份、来源和回复关联。

## 6. 建议的数据和服务边界

### 6.1 一个内部 Trigger 核心

内部可以统一表示“什么事件导致什么动作”，但第一版不要向普通用户暴露通用规则编辑器。

```ts
type TriggerKind =
  | "turn_idle"       // Turn 结束，主要供 Goal
  | "idle_for"        // 连续空闲一段时间
  | "interval"        // 固定间隔
  | "at"              // 绝对时间，一次性
  | "cron"            // 日历计划
  | "message_arrived" // 事件触发，不是 Timer

type TriggerAction =
  | "resume_session_with_message"
  | "evaluate_and_continue_goal"
  | "launch_session"
  | "enqueue_task"
  | "notify_only"
```

这些名称只是语义草案，不是要求立即建立同名枚举。实施时应复用现有 Codeg Service，而不是让
GUI、CLI、MCP 各自执行 Trigger。

### 6.2 产品对象保持三类

#### Goal

建议字段：稳定 Session ID、Objective、状态、Provider backend、轮数/时间/Token 限制、最后一次
判断结果。每个 Session 同时最多一个活动 Goal，历史 Goal 可以保留为 Run 记录。

#### Session Timer

建议字段：Timer ID、目标 Session ID、模式、间隔/绝对时间、Prompt、是否重复、启用状态、
上次触发、下次触发、失败信息和创建来源。一个 Session 可以有多个 Timer。

第一版不必引入 SessionDock 的 owner/target 双 Session 所有权。由用户创建的 Timer 可以直接归属
目标 Session；未来 Agent 代表另一个 Session 创建 Timer 时，再记录 `created_by_session_id` 作为
来源和权限依据。

#### Automation

继续使用现有 Automation 和 Automation Run。不要静默改变现有 `trigger_kind`、`root_folder_id`、
`isolation` 或 `config` 的含义。若以后内部抽取共享 Scheduler，也应保持旧 API 和数据无损迁移。

### 6.3 服务器是唯一触发权威

Trigger 的状态、领取和触发必须由 Codeg Server/Daemon 维护：

- 桌面、Web、移动端和多个物理窗口只订阅状态；
- `(trigger_id, due_revision)` 只能被原子领取一次；
- 成功入 Session Queue 后才更新触发状态；
- App 重启最多补触发一次，不重放错过的每一个周期；
- Session 不在线时保留为 Waiting，不通过 DOM、终端按键或前端定时器注入；
- 同一 Session 的 Goal、Timer、协作消息和用户 Follow-up 最终都经过同一队列仲裁。

现有 Automation Engine 的单 Engine 文件锁、每 Automation Fire Lock、CAS claim 和重启 reconcile
可以作为实现依据，但 Session Timer 不应直接复用其任务启动数据模型。

## 7. 触发与队列规则

### 7.1 Idle Timer

- 只有目标 Session 已连接且 Runtime 为 idle 时才开始计时；
- busy、离线、权限等待或用户正在输入时清除当前 idle window；
- 到期后先进入 Session Queue，再更新 `last_triggered_at`；
- 第一版默认 **one-shot while idle**，避免 Session 长时间无人处理时反复消耗 Token；
- 用户显式选择“空闲时重复”后，才采用 SessionDock 的 periodic-while-idle 语义；
- Timer 触发的 Turn 完成后重新开始新的 idle window。

> 2026-08-18 落地偏差：实际实现直接采用 repeat-while-idle 默认（grace 默认 2 秒），
> “反复消耗 Token”的担忧改由义务感知退避 + 3 次刹车约束，见第 0.3/0.5 节。

### 7.2 Interval / At Timer

- 时间到但 Session 正忙时排队，不中断当前 Turn；
- 多个 Timer 同时到期时保持独立记录，但按队列顺序串行执行；
- `at` 成功入队后自动完成；
- `interval` 默认不追赶 App 离线期间错过的所有周期，恢复后最多补一次；
- 如果多条提醒正文和目标相同，可在短时间窗口内折叠，但必须在 UI 显示被合并的来源。

### 7.3 Goal

- 以 Turn complete/idle 事件驱动，不使用高频轮询；
- 续跑 Prompt 也进入统一 Session Queue；
- 用户 Follow-up、权限请求和紧急 Steer 优先于自动续跑；
- 必须具有 Turn、时间、Token 或失败次数上限；
- 目标完成判断失败时暂停并显示原因，不能无限重试；
- 等待外部消息时使用事件/队列，不应让 Goal 不断唤醒模型进行空轮询。

### 7.4 中断不是 Timer 的职责

普通 Timer 永远只排队。只有用户显式选择 `steer` 或 `interrupt_then_send` 时，才允许影响运行中
Turn；该能力属于 Session Communication/Runtime Control，并需要单独权限和 UI 警告。

## 8. 界面建议

不增加一个笼统的“高级自动化规则”页面。用户入口保持简单：

### Session Inspector：继续与提醒

```text
继续与提醒
├── Goal
│   ├── 当前目标、状态、消耗与限制
│   └── 编辑 / 暂停 / 恢复 / 清除
└── Timers (3)
    ├── 空闲 15m 后：继续剩余 TODO
    ├── 每 2h：更新阶段总结
    └── 明天 09:00：检查结果
        编辑 / 暂停 / 恢复 / 删除
```

- Goal 与 Timer 同处一个 Session 属性面板，便于理解“这个 Session 为什么会再次启动”；
- 多个 Timer 默认只显示最近将触发的几条，展开后查看全部；
- Session 列表只显示轻量徽标，例如 `Goal`、`2 timers`，不堆满倒计时；
- 触发生成的 Prompt 在会话中标明来源，例如“由空闲提醒触发”，并可跳回 Timer；
- Timer 可复制到另一个 Session，但不随 Fork 自动继承，Fork 时由用户明确选择；
- Timer 跟随稳定 Session ID，不跟随某个 Pane、Workbench 或物理窗口。

### Automations 页面

保留当前页面，继续面向“独立计划任务”。可以增加一个“恢复已有 Session”Action，但应作为后续
能力，不与第一版 Session Timer 一起扩大范围。

## 9. 能力适配

```text
GoalBackend
├── NativeCodexGoal
├── NativeClaudeGoal（确认适配接口后接入）
└── HostManagedGoal（后续降级）

TimerBackend
└── CodegSessionTriggerService（Harness 无关）
```

Timer 不需要 Harness 原生支持，因为它最终只向已有 Session 提交普通 Prompt。Goal 则优先使用
Harness 原生实现，因为 Provider 更了解自己的完成判断、预算和 Session 恢复语义。

当 Provider 不支持原生 Goal 时，Codeg 才使用 Host-managed Goal：在 Turn 完成后运行轻量判断，
再决定 Complete、Pause 或排入下一条 Prompt。能力探测必须显式，不能仅靠 Agent 名称猜测。

## 10. 分阶段实现

### P0：文档与能力核对

- 保留当前 Codex Goal 投影和 Automation 语义；
- 明确 Prompt Queue 是所有自动注入的唯一入口；
- 不在当前并行开发批次修改运行时代码。

### P1：Goal 统一表面与事件驱动续跑

- 完善 Codex 原生 Goal 的创建、编辑、暂停、恢复和清除；
- 以 Turn complete/idle 事件立即驱动下一轮，不设置隐含的 15 分钟等待；
- 所有续跑 Prompt 进入服务器权威 Session Queue，并让用户 Follow-up、权限请求和 Steer 优先；
- UI 统一显示目标、Provider、状态、轮数/时间/Token 限额和停止原因；
- 保留 Provider 原始状态，不复制出第二份互相竞争的 Goal 事实。

### P2：Session Timer MVP

- 支持一个 Session 多个 Timer；
- 支持 `idle_for` 和 `at`，`interval` 可同时做或紧随其后；
- 创建、编辑、暂停、恢复、删除；
- 忙碌时可靠入队，多窗口同步；
- 重启恢复、最多一次补触发、去重和失败可见；
- Inspector 展示 Timer 来源与下一次触发时间；
- 接入其他 Harness 的原生 Goal 能力可以与本阶段并行，但不阻塞 Timer。

### P3：Host-managed Goal

- 只为无原生 Goal 的 Harness 提供；
- 增加完成度判断、限额、失败暂停和审计记录；
- 与 Session Queue、协作消息和用户 Follow-up 做优先级仲裁。

### P4：抽取共享 Trigger Service

- Timer 与 Automation 共享调度、claim、恢复、事件广播和 Run 记录基础；
- 保留三种产品对象和兼容 API，不为“代码统一”牺牲用户语义；
- 视实际需求再增加 `cron -> resume existing session`，不提前开发通用规则平台。

## 11. 验收标准

Goal 第一阶段至少满足：

1. Turn 完成并进入 idle 后可立即排入下一轮，不依赖分钟级轮询或 `idle_for` 延迟；
2. Complete、Paused/Blocked、Budget limited 和失败状态都能停止续跑并说明原因；
3. 用户 Follow-up、权限请求和显式 Steer 不会被自动 Goal 续跑抢占；
4. 原生 Provider Goal 仍是状态事实源，Codeg 不生成一份相互漂移的影子状态；
5. 同一 Session 在多个窗口打开时只运行一条 Goal 状态机和一份 Prompt Queue。

Session Timer 阶段至少满足：

1. 一个 Session 可创建两个以上 Timer，并分别编辑、暂停和删除；
2. idle Timer 在 Session 忙碌时不计时，连续空闲达到阈值后只触发一次默认提醒；
3. Timer 到期遇到 busy 进入同一 Prompt Queue，不打断、不丢失、不重复；
4. 同一 Session 同时在两个窗口打开时，只显示和执行一份 Timer；
5. Server/App 重启后 Timer 恢复，错过多个周期最多补一次；
6. Session 归档或未打开不等于删除 Timer；真正删除 Session 时给出清理/迁移选择；
7. Timer Prompt 在 Transcript 中可识别来源，但不伪装成用户手工消息；
8. Timer 不创建 Mailbox unread、reply obligation 或 Group Conversation 消息；
9. 原生 Codex Goal 与现有 Automation 行为不因 Timer 上线而改变；
10. 所有状态由服务器广播，多客户端前端不各自运行定时器。

## 12. 明确暂缓

- 可视化任意 Trigger/Action 流程编辑器；
- 根据模型自己推测自动创建大量 Timer；
- 默认继承 Fork 父 Session 的所有 Goal/Timer；
- 把 Reminder、协作消息、Goal 和 Automation 全部显示成同一种 Chat Message；
- 通过终端按键、DOM 注入或前端 `setInterval` 作为正式投递路径；
- 没有限额、停止条件和失败暂停的永久自动续跑；
- 在 Session Queue 尚未成为服务器权威前抢先实现跨客户端 Timer。

## 13. 历史设计依据

本地历史讨论已经形成过两条仍适用的判断：

- SessionDock/CDP 的注入可以把 idle Session 拉回工作，但它属于唤醒通道，不等于消息存储或
  生命周期状态；
- blocking await 是“挂着不烧”，Goal/反复注入会持续消耗模型，因此等待消息应优先使用事件或
  阻塞等待，Goal/Timer 只负责真正需要再次启动模型的工作。

检索依据：Claude 历史 Session `4514c1a5-79a2-7be6-a5ff-a7e2a9d96735`、事件
`a1faef79-3ada-7f81-8265-90a62dbdaa96`；Claude 历史 Session
`3dcf9015-4f49-758c-b843-95d1a75bffff`、事件
`462ffe99-49e5-786c-969a-5173ad10d75f`。
