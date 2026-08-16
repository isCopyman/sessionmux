# Codeg `delegate_to_agent` 子系统源码审计与 Session 化方案

> 状态：Architecture audit / 三个旧工具已授权直接移除，实现批次待执行
>
> 调研日期：2026-08-16
>
> 源码基线：Codeg `origin/main` `84274701e6dbb9380c3db95dd99880ccfc92577b`
> 目的：回答现有 delegation 到底包含什么、哪些能删除、哪些必须保留，以及怎样把它收敛为普通 Session 能力。

本文是专项技术审计，不替代人类可读的
[产品需求与使用场景](./PRODUCT-SPEC.zh-CN.md)。产品最终效果以产品文档为准；本文只约束未来修改
不能误删共享基础设施，也不能继续为一次性 delegation 扩张第二套 Session 系统。

## 1. 结论

不能把 `src-tauri/src/acp/delegation/` 整个目录直接删除，也不应继续把它当作一套独立的 Agent
产品扩建。

当前实现实际上混合了三层：

1. **通用 Host Bridge。** `codeg-mcp`、per-launch token、UDS/named pipe、MCP
   `tools/list`/`tools/call`、父进程守护和功能开关。它还承载反馈、提问、Session 查询、任务进度、
   自动化和任务板，不属于 delegation 私产。
2. **可泛化的 Session 运行能力。** 创建 ACP Runtime、创建 Conversation、发送初始 Prompt、
   关联来源、等待、取消、阻塞检测、事件广播和跨客户端显示。这些正是未来 Session Lifecycle 与
   Session 间通信需要的底座。
3. **delegation 专属的一次性产品语义。** 三个 MCP 工具、`task_id` 轮询协议、首轮结束强制断开、
   父 Tool Call 卡片、专用只读 Dialog、结果文本缓存和大量 Tool Call 关联补丁。这一层应压缩为
   兼容宏，最终可以从默认产品面移除。

目标不是简单把 `delegate_to_agent` 改名为 `send_message`，而是得到下面的关系：

```text
Session 原语：create / resume / send / wait / cancel / fork / open
                         │
                         ├── 联系已有 Session
                         │     = send(existing_session, message)
                         │
                         └── 一次性委托兼容宏
                               = create(new_session)
                               + link(source)
                               + send(initial_task)
                               + optional wait/report
```

三个旧工具直接删除；未来对应行为只由普通 Session、Mailbox 与 Host Control 组合提供。

## 2. 审计边界与一个文档矛盾

审计同时核对 Rust 后端、数据库、MCP companion、前端运行时、专用 UI、设置、API 和测试。源码
基线是独立的最新上游工作树；当前本地开发分支仍有 Workbench 未提交改动，因此本轮只写文档，
没有把上游合并进脏工作树，也没有删除功能代码。

源码顶部存在一处已经过时的说明：

- [`delegation/mod.rs`](../../src-tauri/src/acp/delegation/mod.rs) 仍把 v1 描述为等待子 Agent 第一轮
  完成后直接返回；
- [`broker.rs`](../../src-tauri/src/acp/delegation/broker.rs) 和当前工具 Schema 已经实现异步模式：
  `delegate_to_agent` 先返回 `task_id`，再由 `get_delegation_status` 轮询或长轮询。

实施前应先修正文档注释并用现有测试锁定真实行为，不能按旧模块说明进行删改。

## 3. 当前系统实际做了什么

### 3.1 完整调用链

```text
Codeg 启动
  └─ build_delegation_stack
       ├─ DelegationBroker
       ├─ TokenRegistry
       ├─ UDS / Windows named pipe listener
       └─ feedback / ask / session-info / authoring runtime config

父 Session 创建或恢复
  └─ inject_codeg_mcp
       ├─ 检查 Harness 是否真的支持 wire MCP
       ├─ 根据设置和 Host Tools Policy 选择 feature groups
       ├─ 注册临时 token → parent_connection_id / cwd
       └─ 注入 codeg-mcp stdio server

模型调用 delegate_to_agent
  └─ codeg-mcp companion
       ├─ 校验 tools/call 参数
       ├─ 创建 external_handle 处理 MCP cancel
       └─ 经本地管道发送 BrokerMessage::Call

主进程 listener
  └─ token → 真实父 Connection / Conversation / cwd
       └─ DelegationBroker::start_delegation
            ├─ 关联父 Tool Call
            ├─ 检查开关和递归深度
            ├─ ConnectionManagerSpawner::spawn
            ├─ send_prompt_linked_for_delegation
            ├─ 创建 kind=delegate 的真实 Conversation
            ├─ 写 parent_id / parent_tool_use_id / delegation_call_id
            ├─ 写父 Tool Call meta
            ├─ 发 DelegationStarted
            └─ 返回 running + task_id

子 Session 运行
  ├─ 普通 ACP 流式事件、权限请求、问题和 TurnComplete
  └─ lifecycle subscriber → broker.complete_call / cancel path
       ├─ running → completed cache
       ├─ 写终态 meta
       ├─ 发 DelegationCompleted
       └─ 强制 disconnect 子 Runtime

父模型收集结果
  └─ get_delegation_status(task_ids, wait_ms)
       ├─ 内存结果
       ├─ live reply / blocked_on
       ├─ DB 终态兜底
       └─ 结果回到父模型上下文
```

### 3.2 它创建的不是“临时假 Agent”，而是真实 Session

[`ConnectionManagerSpawner`](../../src-tauri/src/acp/manager.rs) 使用与用户手动创建 Session 相同的
运行环境构建和 `spawn_agent`；随后
[`create_with_delegation`](../../src-tauri/src/db/service/conversation_service.rs) 创建真实
`conversation` 行。该行拥有 Harness、cwd/Folder、原生 `external_id`、状态、消息数和完整会话文件，
只是额外标记为 `ConversationKind::Delegate` 并嵌套在父 Conversation 下。

因此：

- 结果不是只存在父 Tool Call 中；完整历史仍在子 Session 的原生会话和 Codeg Conversation 中；
- Broker 首轮结束后断开的是 ACP 进程，不是删除 Session；
- 以后可以用原生 Session ID 恢复同一子 Session；
- “Session 化”主要是让普通 Session 生命周期正式接管它，而不是把结果复制进一张新表。

### 3.3 前端已经具备部分普通 Session 能力

最新版代码已经存在以下能力：

- 侧栏按 `parent_id` 延迟加载多层子 Session 树；
- 子 Session 可被 `openTab` 打开；
- [`tab-store.ts`](../../src/stores/tab-store.ts) 会为根列表中不可见的已打开子 Session 单独补取
  Summary，并同步标题和状态；
- 跨客户端连接发现可把另一个窗口中的运行 Session 作为 viewer/co-controller 附着；
- 子 Session 的实时输出、权限、问题和计划审批已经走普通 Connection reducer。

但产品入口仍不一致：

- 父会话内的卡片和右上角悬浮层打开
  [`SubAgentSessionDialog`](../../src/components/message/sub-agent-session-dialog.tsx)，它只有会话查看面，
  没有普通 Composer；
- [`SessionRow`](../../src/app/pet-panel/_components/SessionRow.tsx) 明确把子 Agent 点击跳转到父 Session；
- 前端把 live child 标成 `isDelegationChild`，拒绝普通 reconnect/reapply config，并声明 Broker 独占其
  生命周期；
- Broker 在第一轮完成后无条件 `disconnect`。

所以现状不是“Codeg 不会打开子 Session”，而是**底层已经能打开，专用 delegation 流程仍在阻止
它成为完整、持续的普通 Session**。

## 4. 文件与职责清单

### 4.1 Host Bridge：保留并改名，不得随 delegation 删除

| 代码 | 当前职责 | 处理意见 |
|---|---|---|
| [`bin/codeg_mcp.rs`](../../src-tauri/src/bin/codeg_mcp.rs) | per-launch stdio MCP 进程、参数、父进程守护、stdin/stdout relay | 保留，未来名称可从 delegation companion 中解耦 |
| [`companion.rs`](../../src-tauri/src/acp/delegation/companion.rs) | MCP initialize、tools/list、tools/call、cancel、Schema 过滤和调用渲染 | 保留共享框架；移走三项 delegation 专属 dispatcher |
| [`listener.rs`](../../src-tauri/src/acp/delegation/listener.rs) | token 校验、Windows named pipe/Unix UDS、请求分发 | 保留共享 listener；将 broker 变成一个可选 handler |
| [`transport.rs`](../../src-tauri/src/acp/delegation/transport.rs) | 长度前缀 JSON、本地跨进程 request/response | 保留 transport；把 `BrokerMessage` 改成中性的 Host 请求枚举 |
| [`parent_watcher.rs`](../../src-tauri/src/acp/delegation/parent_watcher.rs) | 主进程消失时退出 companion | 原样保留 |
| [`connection.rs`](../../src-tauri/src/acp/connection.rs) 的 `inject_codeg_mcp` | 把 companion 注入 session/new、load、resume | 保留并作为未来 Session/Workbench 工具入口 |

当前 companion 不只暴露三项 delegation 工具，还承载：

- `check_user_feedback`；
- `ask_user_question`；
- `get_session_info`；
- `create_automation`、`create_work_task`；
- `task_progress`、`task_complete`。

因此直接删除 companion/listener/transport 会同时破坏这些功能。更合理的目录演进是以后把共享文件
迁到 `acp/host_bridge/`，但不应为了改名先制造一次大范围无行为收益的文件移动。

### 4.2 Delegation Engine：拆成通用服务与兼容适配器

| 代码 | 当前职责 | Session 化方向 |
|---|---|---|
| [`broker.rs`](../../src-tauri/src/acp/delegation/broker.rs) | 启动、状态、长轮询、取消、竞态、缓存、Tool Call 关联 | 拆为 Session Operation Registry、关系服务和 legacy adapter |
| [`spawner.rs`](../../src-tauri/src/acp/delegation/spawner.rs) | 小型 spawn/send/cancel/disconnect 抽象 | 泛化为 Session Runtime Service，不再带 delegation 命名 |
| [`types.rs`](../../src-tauri/src/acp/delegation/types.rs) | task/report/error/blocked/defaults | 通用部分迁入 Session Operation；wire-stable legacy 类型保留兼容 |
| [`depth.rs`](../../src-tauri/src/acp/delegation/depth.rs) | 沿 `conversation.parent_id` 算递归深度 | 迁为通用 spawn/fan-out policy，读取显式 relation |
| [`live_reply.rs`](../../src-tauri/src/acp/delegation/live_reply.rs) | 查询子连接最新回复和阻塞状态 | 迁为普通 Session Runtime Status |
| [`event_emitter.rs`](../../src-tauri/src/acp/delegation/event_emitter.rs) | 对父连接发 started/completed | 泛化为 Session Operation 事件；保留 legacy 事件投影 |
| [`meta_writer.rs`](../../src-tauri/src/acp/delegation/meta_writer.rs) | 把状态写入父 ToolCallState meta | 降为发起位置的 UI projection，不再是任务事实源 |
| [`tool_schema.json`](../../src-tauri/src/acp/delegation/tool_schema.json) | 全部 Codeg MCP Schema | 保留共享 Schema 载体；直接删除三项 delegation 工具 |

`broker.rs` 很大并不只是因为“启动一个 Agent”复杂，而是同时解决了：

- ACP Tool Call 与 MCP `tools/call` 到达顺序不一致；
- 并行相同任务如何匹配正确 `tool_use_id`；
- Cursor identity-less MCP 卡片重写；
- cancel 早于 spawn、spawn 与 park 之间完成、父/子同时终止等竞态；
- 多任务长轮询、阻塞提示只上报一次及丢包后重新浮现；
- 完成结果内存缓存、按父会话隔离、FIFO 内存阀门和 DB 状态兜底；
- 父 Session 结束或断开时的级联取消。

这些测试资产不能整批删除。迁移时要区分两类：运行时竞态仍适用于通用 Session Operation，应改写
后保留；Cursor 卡片重写和 delegation Tool Call 关联若只服务旧三工具，在共享 fixture 验证后删除。

### 4.3 ACP 生命周期与运行时：大部分复用

| 代码 | 依赖 | 处理意见 |
|---|---|---|
| [`manager.rs`](../../src-tauri/src/acp/manager.rs) | 真正 spawn、prompt、cancel、disconnect、跨客户端连接发现 | 作为通用 Session Runtime 核心 |
| [`lifecycle.rs`](../../src-tauri/src/acp/lifecycle.rs) | TurnComplete/error/disconnect 映射到 broker | 把终态通知改投通用 Operation Registry，再兼容投影给 broker |
| [`connection.rs`](../../src-tauri/src/acp/connection.rs) | MCP 注入、Tool Call 标准化、Grok/Cursor 兼容、事件产生 | 保留；删减 delegation 专属匹配前先保留快照兼容 |
| [`app_state.rs`](../../src-tauri/src/app_state.rs) | 建 Broker、token、listener 和各功能配置 | 重命名为 Host Bridge stack，并把 broker 变为可选服务 |
| [`acp-connections-context.tsx`](../../src/contexts/acp-connections-context.tsx) | child live attach、viewer/co-controller、权限和流式状态 | 统一 child 与普通 Session owner/viewer 规则，避免第二套 context key |

需要重点解决的冲突是：同一运行中的子 Conversation 可以同时以 `child_connection_id` 被 delegation
context 附着，又以普通 Tab context key 被打开。现有跨客户端 viewer 机制可以复用，但必须增加测试，
保证两个视图共享一个后端 Runtime、同一流式事件和同一 Prompt 锁，关闭任一视图不会误杀另一视图。

### 4.4 数据库：保留历史字段，不继续滥用

当前数据模型包括：

```text
conversation.kind = delegate
conversation.parent_id
conversation.parent_tool_use_id
conversation.delegation_call_id
```

相关迁移和服务位于：

- [`m20260522_000001_delegation_columns.rs`](../../src-tauri/src/db/migration/m20260522_000001_delegation_columns.rs)；
- [`conversation.rs`](../../src-tauri/src/db/entities/conversation.rs)；
- [`conversation_service.rs`](../../src-tauri/src/db/service/conversation_service.rs)。

这些列已经进入用户数据库和导入/侧栏/统计逻辑，不能通过回滚迁移删除。兼容政策如下：

- 旧 `delegate` 行继续可读、可恢复、可在父树下显示；
- 不改变 `parent_id` 已有 delegation 含义，不拿它表达 Fork、消息往来或任意 Team 关系；
- 普通 Session Lifecycle 首先仍创建同一张 `conversation` 表，不造第二套 Session 表；
- 若要支持多种来源关系、多个来源或 relation metadata，再新增通用 `session_relation`，并把旧三列
  作为 legacy projection/迁移来源；
- 不因删除父卡片、关闭 Workbench 或禁用 delegation 删除子 Session。

### 4.5 前端专用层：能删一部分，但先把入口改到真实 Session

| 代码族 | 当前功能 | 建议 |
|---|---|---|
| `delegation-context`、`use-delegation-card-model` | 把 started/completed、Tool Call input/output/meta 合并为卡片模型 | 删除 task_id 协议；仅低依赖视觉 primitive 可抽取保留 |
| `delegated-sub-thread`、status card/group/badge | 父时间线内的委托状态 | 保留轻量状态卡，但不承载子 transcript |
| `sub-agent-overlay` | 最近一次回复中的子任务浮层 | 可保留为 Peek；主动作改为打开真实 Session |
| `sub-agent-session-dialog` | 只读/可处理权限的完整子会话 Dialog | 普通 Session 打开可靠后删除，或降为无状态 Peek |
| `use-subsession-sync`、侧栏子树 | 多层父子 Session 显示与实时同步 | 保留；关系来源以后可从 legacy 列切换到 relation service |
| Tab child summary cache | 让隐藏子 Session 也能成为普通 Tab | 直接复用，是 Session 化已有底座 |
| Pet panel child routing | 当前强制跳父 Session | 改为优先聚焦/打开真实子 Session，父 Session 作为来源链接 |
| delegation 设置 UI | 开关、深度、每 Harness 默认项、结果缓存 | 模型/cwd/profile 迁入通用 Session Launch Profile；只保留兼容开关 |

### 4.6 解析器和旧记录兼容：最后删除

多个 Harness 对 MCP Tool Call 的命名、raw input 和 result envelope 不一致。Codeg 在
`connection.rs`、tool normalization、adapter、snapshot denormalize 和 delegation card parser 中有
兼容逻辑。即使新版本不再默认暴露 delegation，用户历史 Session 仍包含这些 Tool Call。

当前是尚未正式使用的开发者状态，不保留旧三工具协议兼容期。只为 delegation round-trip 存在的
parser/normalizer 可以删除，但必须先用普通 MCP Tool Call fixture 证明通用工具卡片、权限、取消和
结果渲染不依赖它；已发布数据库 migration 不回改。

## 5. 功能组合与开关

### 5.1 直接移除裁决与未来替代语义

`delegate_to_agent`、`get_delegation_status`、`cancel_delegation` 已授权直接删除，不提供
compatibility toggle，不等待等价替代落地，也不保留 delegation `task_id` wire protocol。未来若
补 Host Control，组合语义为：

| 旧工具 | 目标组合语义 |
|---|---|
| `delegate_to_agent` | `create_session + send_message(initial task) + optional lineage/workbench placement`，返回稳定 Session ID |
| `get_delegation_status` | `get_session_info/list_sessions + Mailbox Delivery/Reply/Obligation`；可选 wait 仅观察 |
| `cancel_delegation` | 明确拆成 `cancel_current_turn / stop_session / cancel_queued_message`，不再用一个动作暗含终止、删除和撤信 |

替代 Session 是普通持久 Session，可以继续交流、重命名、加入 Collection/Workbench；不会因父 Turn
结束而自动失去身份或被删除。删除以后 Agent 暂时不能主动创建 Session，是已接受的开发期缺口；
本批不顺手实现 `create_session`。

`codeg-mcp` 是否出现不是一个 delegation 布尔值决定，而是多项能力的组合：

| 情况 | 当前结果 | 迁移要求 |
|---|---|---|
| feedback、ask、sessions、tasks、authoring 全关 | 不注入 companion | 保持 |
| 任一共享 Host 能力开启 | companion 继续注入，但 tools/list 不再包含三项 delegation 工具 | 证明不能删除 companion |
| Harness `supports_mcp=false` 或实际不传 wire MCP | 不注入 Codeg companion | UI/HTTP 仍应可操作 Session，Agent 工具明确降级 |
| Host Tools Policy 不允许宿主通道 | delegation 被隐藏，避免受限 Agent 让兄弟 Agent 代读文件 | 通用 create/send 也必须沿用同等权限边界 |
| task engine 启动 Session | 即使普通设置关闭，也可注入 task reporting tools | 与 delegation 拆开 |
| 用户禁用某 Harness | companion 动态从 `agent_type` enum 移除该目标 | 迁入通用 Session launch capability |
| 用户配置每 Harness mode/model 值 | Broker spawn 时传给子 Session | 迁入统一 Launch Profile，不保留第二份默认值 |

当前 Schema 一共包含十项 Host 工具，其中三项属于 delegation。三项工具仅描述和压缩后的 Schema
约 5,600 字符，且 `get_delegation_status` 注入了大量轮询行为提示。未来不应把更多 Session 和
Workbench 工具继续永久追加到这个静态列表；采用 Host 控制面 RFC 中的极小 gateway、capability
pack 与按需 Skill。

## 6. 哪些删除、哪些 Session 化、哪些原样复用

### 6.1 本批直接删除的旧语义

经共享边界审计后直接删除：

- `delegate_to_agent`、`get_delegation_status`、`cancel_delegation` 三项常驻 Schema；
- delegation 专属 `task_id` 轮询说明和模型必须反复调用 status 的工作流；
- 首轮完成后无条件断开子 Runtime 的规则；
- 绑定 `task_id`、一次性 terminal 状态和轮询协议的专属 UI；
- 父会话专属的 512 MB completed-result text cache；
- 只为把 MCP 调用重新贴回父 Tool Call 而存在的 identity-less/FIFO 关联状态机；
- delegation 专属模型/cwd/mode 默认设置；
- “子 Session 永远不能作为 Tab/不能 reconnect”的前端硬编码（若仍被其他真实 Session 路径引用，
  则记录并留给普通 Session 打开批次修正）。

不为保留皮肤而保留 Broker、task_id 或一次性状态机；无引用的命令、模型、设置字段和测试一起清理。

### 6.2 必须 Session 化的内容

| 现有概念 | 通用概念 |
|---|---|
| `ConnectionSpawner` | `SessionRuntimeService` |
| `DelegationLink` | `SessionRelation` / `SessionOrigin` |
| `RunningTask` / `CompletedTask` | `SessionOperation` |
| `task_id` | operation id；返回中同时包含稳定 conversation id |
| depth limit | spawn relation depth / fan-out policy |
| agent defaults | Session Launch Profile |
| `get_delegation_status` | `get_session_info/list_sessions` + Mailbox Delivery/Reply/Obligation；wait 仅可选观察 |
| `cancel_delegation` | `cancel_current_turn` / `stop_session` / `cancel_queued_message`，与 archive/delete 分离 |
| `blocked_on` / live reply | Session Runtime Status |
| parent Tool Call meta | Operation 在来源 Session 中的投影 |
| delegation started/completed | Session created/operation changed + legacy projection |

通用服务建议至少拆成四个边界：

```text
SessionRegistry
  └─ 稳定 Session 身份、查询、原生 external_id

SessionRuntimeService
  └─ create / resume / prompt / cancel-turn / disconnect

SessionRelationService
  └─ source / delegated-from / forked-from / handoff 等关系

SessionOperationRegistry
  └─ queued / running / blocked / completed / failed / canceled
     + wait / subscribe / idempotency / result reference
```

UI、内部 API 与渐进式 MCP 都只调用这些应用服务，不分别维护状态机。

### 6.3 可以几乎原样复用的内容

- Conversation 表与原生 Session ID；
- ConnectionManager 的 spawn/resume/prompt/cancel/disconnect；
- 跨客户端 connection discovery、viewer/co-controller 和 Prompt 串行锁；
- Internal Event Bus、Tauri/WebSocket 广播和 Snapshot；
- codeg-mcp 的进程、token、管道、取消和 feature gating；
- 父进程退出清理；
- Permission、Question、Plan Approval 的 child connection 路由；
- 侧栏/悬浮容器、卡片壳、进度与状态展示、展开结果、“打开 Session”等纯展示 primitive；
- 竞态、取消、去重、阻塞和丢包恢复测试所表达的行为要求；
- 普通 MCP Tool Call 解析与旧 Conversation 数据库字段读取。

UI 审计必须分三类：

1. **纯展示组件**：低依赖的卡片壳、状态、结果展开、悬浮/侧栏容器可抽成
   `SessionActivityCard` / `BackgroundSessionPreview` 后保留；
2. **可改造组件**：当前绑定 task_id 的组件若能低成本改绑稳定 `conversation_id`，可记录为未来
   后台 Session 预览、消息请求状态、回复通知和“加入 Workbench”的复用候选；
3. **专属协议 UI**：轮询 task_id、一次性 completed/failed/canceled、首轮结束即终止、parent Tool
   Call 专属等待状态直接删除。

若抽取会显著扩大 removal 批次，当前只保留最小纯展示 primitive 或记录候选，不在本批新造普通
Session activity 产品。未来浮窗只做可选预览，真实交互仍回到 Workbench Tab。

## 7. 已授权 Removal 批次顺序

### R0：先画删除地图并冻结共享边界

- 为三个旧工具、feature/settings、Broker、ACP round-trip、UI 和测试建立引用图；
- 用普通 MCP Tool Call、`check_user_feedback`、`ask_user_question`、`get_session_info`、
  `list_sessions/send_message`、task/automation fixtures 证明共享路径；
- 保留 Broker 的 setup/cancel/complete race 测试；
- 标记 UI 的纯展示、可改造和专属协议三类。

### R1：删除公开产品面

- 从 `codeg-mcp` schema、feature flags、设置、Prompt/Skill 和 i18n 删除三工具；
- 删除 delegation 专属浮窗/Dialog/Card/store/API，或只抽取已证明低依赖的展示 primitive；
- 保留共享 companion、transport、caller identity、tools/list/tools/call/cancel 和普通工具渲染。

### R2：删除 task_id/Broker 和专属 ACP 往返

- 删除 pending call、status wait/poll/cancel、首轮结束强制断开和 completed result cache；
- 删除只为 delegation MCP round-trip 存在的 FIFO/identity-less correlation；
- 若 `acp/delegation` 目录仍承载共享 Host bridge，先保留目录名，避免本批大搬家。

### R3：清理死代码和安全收尾

- 删除无引用 spawner/broker/models/commands/设置字段和专属测试；
- 已发布 migration 不回改；遗留列若无害可保留，必要时只做 additive cleanup；
- 运行 Rust/前端高相关或全量测试和真实 Tauri WebView 冒烟；独立提交。

未来 `create_session + send_message + get_session_info/list_sessions + cancel_turn/stop_session` 另开
实施批次，不是 removal 的阻塞条件。

## 8. 验收场景

### 8.1 三个旧工具完全消失

`tools/list`、直接伪造 `tools/call`、设置、Prompt/Skill、UI 和 i18n 都不再暴露三个旧工具；代码中
不存在可运行的 task_id Broker/status/cancel 协议。

### 8.2 共享 Host 工具不受影响

`codeg-mcp` 仍可注入并调用 feedback、ask、session info、`list_sessions/send_message`、task 和
automation 工具；caller token、named pipe/UDS、工具取消和普通 MCP Tool Call 渲染仍工作。

### 8.3 UI 不保留第二套生命周期

删除专属 task 状态后，保留的卡片/浮窗 primitive 不读取 task_id、不轮询 Broker、不拥有第二份
Session 状态。未来改绑 conversation_id 之前，可以暂不出现在产品入口。

### 8.4 多 Harness 与权限

创建子 Session 时，Harness、模型、思考强度、模式、cwd 和权限来自统一 Launch Profile 与能力
探测。某 Harness 不支持 MCP 时，UI 创建/打开仍可用，Agent 主动调用明确显示不可用，不静默失败。

## 9. 风险与待验证项

1. **共享连接误删。** delegation child attach 使用 `child_connection_id`，普通 Tab 使用自己的
   context key；删除前必须证明 viewer discovery、prompt lock 和普通 Session owner 不依赖 Broker。
2. **关系模型。** 当前 `parent_id` 同时驱动嵌套、深度和隐藏策略；抽 relation 前不能先改变其语义。
3. **结果持久化。** Broker completed cache 可以删除，但不能误删普通 child transcript 或通用 Tool
   Call 输出渲染。
4. **取消含义。** 删除 task cancel 时保留普通 MCP request cancel、ACP current-turn cancel 和
   PromptQueue 操作；不能按函数名相似度批量删除。
5. **历史解析。** Cursor/Grok/Claude/Codex 的 Tool Call 形态不同，删除 normalize/parser 前必须用
   真实 fixture 验证。
6. **脏分支同步。** 本审计基于更新后的 `origin/main`，实施前须先把本地 Workbench 改动整理提交，
   再进行可审计的 rebase/merge；不能在脏工作树直接覆盖上游。

## 10. 决策

- 已授权在独立 Removal 批次直接删除 delegation 功能代码；
- 删除 delegation 产品语义和三个 MCP 工具，不保留 compatibility toggle 或 task_id 协议；
- 不再以 delegation 为未来多 Agent 协作的核心对象；
- 替代 Host `create_session` 可以后续实现，不阻塞本次删除；
- 把 Host Bridge 从 delegation 命名和目录职责中逐步解耦；
- 纯展示 UI 可以复用，但不能保留旧 Broker/task_id 状态机；
- Session 间持续交流按
  [Session 间通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md) 实现，不在 Broker 上继续叠私聊/群聊；
- 群聊若以后实现，只复用稳定 Session 投递和 Operation，不复用一次性委托的 `task_id` 时间线。

这套顺序在直接删除旧产品层的同时保留 Codeg 已有的 ACP、远端、多客户端、MCP Host bridge 和
可复用视觉 primitive；共享目录的重命名可以以后单独进行。
