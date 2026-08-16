# Codeg Host 控制面与 Agent 可编程工作台 RFC

> 状态：Design draft，尚未实现
>
> 更新时间：2026-08-16
>
> 上位产品需求：[产品需求与使用场景](./PRODUCT-SPEC.zh-CN.md)
> 相邻设计：[Session 间通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md)、
> [Workbench 布局同步 RFC](./WORKBENCH-LAYOUT-SYNC-RFC.zh-CN.md)、
> [Atrium 功能审计](./ATRIUM-FEATURE-AUDIT.zh-CN.md)

> 本文记录未来实现依据，不表示当前 Codeg 已经提供下列全部工具。当前代码、数据库迁移和
> Harness 能力仍是事实源；工具名和参数只是语义草案，实施前必须再次核对现有接口。

## 1. 核心判断

Codeg 不需要分别建设“一套多 Agent 系统”“一套 Session 系统”“一套布局自动化系统”。这些
能力共用一个 Host command bus/API；GUI 与 Codeg 自有渐进式 MCP 是其受支持的薄入口：

```text
                         Codeg Host Core
                ┌──────────────┼──────────────┐
                │              │              │
        Session Registry   Runtime Manager   Workbench Manager
        身份/历史/检索      启动/Resume/Turn    引用/Pane/资源/布局
                │              │              │
                └──────────────┼──────────────┘
                               │
                         ┌──────┴──────┐
                         │             │
                        GUI     Progressive MCP
```

Host command bus/API 是唯一能力实现和事实源；GUI 与 MCP 都不能复制 Session、权限、并发、持久化
或审计逻辑。受管 Session 的正式 Agent 控制路径只有“Skill + Codeg 自有渐进式 MCP”。本文提到的
第三方 CLI 仅是竞品事实，不构成 Codeg 的产品要求。

## 2. Delegation 三工具已移除

完整源码链路、文件清单、删除边界与分阶段迁移见
[`delegate_to_agent` 子系统源码审计与 Session 化方案](./DELEGATION-SUBSYSTEM-AUDIT.zh-CN.md)。本节只
保留 Host 产品边界，不重复维护实现清单。

删除前 `delegate_to_agent` 的真实行为是：创建一个全新的 ACP 子 Session，注入一条自包含任务，
异步运行，保存父子关系，再由父 Session 查询或等待结果。历史工具 Schema 也明确说明子 Session
冷启动、不能看到父对话，适合独立任务而不适合持续往返。完整证据保留在审计文档中；下列路径中
的专属 engine 文件已随 Removal 删除：

- 保留下来的 `src-tauri/src/acp/delegation/tool_schema.json` 历史版本；
- 已删除的 `src-tauri/src/acp/delegation/spawner.rs` 与 `ConnectionSpawner`；
- [`conversation.parent_id` 子会话查询](../../src-tauri/src/db/service/conversation_service.rs)。

因此它不是另一种与 Session 无关的 Agent 对象，而可以分解为：

```text
delegate_to_agent(task, agent_type, cwd)
  = create_session(kind = delegated_child, harness, cwd)
  + send_initial_prompt(task)
  + link_parent_and_child()
  + wait_or_poll_result()
  + project_result_into_parent()
```

这说明现有 delegation 不是一类必须继续扩张的 Agent 产品，而只是 Session 原语的一种**一次性
任务宏**。对以长期 Session 为中心的用户，它的价值确实有限：任务只发一次、子 Session 冷启动，
父会话只能等结果，第一轮不满意时又缺少自然的后续往返。它不应成为 Codeg 未来协作体系的主入口，
也不值得为它继续建设第二套独立的 Session UI。

三个旧工具 `delegate_to_agent`、`get_delegation_status`、`cancel_delegation` 已在独立批次直接
删除，没有 compatibility toggle、task_id wire 协议或等价替代前置条件。删除以后 Agent 暂时
不能主动创建 Session，是已接受的开发期缺口；新功能不得再依赖 delegation `task_id`。

删除没有带走共享基础设施。鉴权、MCP companion、ACP 启动、普通 MCP 取消、持久 Session、
`list_sessions/send_message` 和通用 Tool Call 投影仍由中性的 Host bridge 承载。后续直接让这些
底层服务普通的 `create/resume/send` Session 原语，不恢复 delegation 专用产品层。

需要调整的是产品边界：

- 未来一次性冷任务也由普通 `create_session + send_message` 组合，不恢复 delegation 预设；
- 要继续追问时，向已有 Session 发送 follow-up，而不是再次冷启动；
- Session 做得不好时，可以继续同一 Session、Fork 一个替代 Session，或明确新建另一个；
- 用户可以把有长期价值的子 Session“提升/固定”为普通可管理 Session，但必须保留原父子来源，
  不能通过清空 `conversation.parent_id` 抹去 provenance；
- 侧栏默认仍把临时子 Session 嵌套在父 Session 下，避免一次 fan-out 产生大量顶层噪声；
- 清理策略只能归档或隐藏临时子 Session，不能在未经用户同意时删除原生历史。

### 2.1 悬浮子会话只做状态速览，真实交互回到 Workbench

当前 `SubAgentOverlay` 点击后打开的是 `SubAgentSessionDialog`，其说明和实现都把它定义为只读的
完整会话查看器；Pet 面板甚至明确把 delegation child 视为“不能作为 Tab 打开”的对象：

- [`SubAgentOverlay`](../../src/components/chat/sub-agent-overlay.tsx)
- [`SubAgentSessionDialog`](../../src/components/message/sub-agent-session-dialog.tsx)
- [`SessionRow` 的 child 跳转规则](../../src/app/pet-panel/_components/SessionRow.tsx)

这正是当前交互受限的原因。目标设计不应在 Dialog 里再造一套可写聊天界面，而应改为：

- 父 Session 中保留一张轻量状态/结果卡，适合看“运行中、阻塞、完成、摘要”；
- 主动作是“在当前 Workbench 打开”，把**同一个真实子 Session**加入普通 Pane；
- 打开后使用普通 Session 的发送、Resume、Fork、搜索、资源和布局能力，可以持续追问；
- 可选保留只读 Peek，但它只是同一 Session 的投影，不拥有第二份 transcript、draft 或 runtime；
- 同一子 Session 已经在 Workbench 中时直接聚焦或提示所在位置，不重复创建会话。

因此悬浮面板的视觉和状态代码可以复用，专用只读 Dialog 不应继续成为主路径。等普通子 Session
打开和多视图同步可靠后，再决定保留 Peek 还是删除，而不是现在先大规模重写或直接删掉。

未来只保留统一 Session Lifecycle：

```text
新建临时或长期 Session              → create/resume/fork
发送初始任务或联系已有 Session       → send_message
查询进度与往来                      → get/list session + mailbox projection
取消当前轮或停止 Session             → explicit cancel-turn / stop-session
把 Session 放到眼前                 → open/arrange workbench
```

## 3. 控制面能力分层

### 3.1 Session 发现与生命周期

目标语义包括：

```text
list_sessions / search_sessions / get_session
create_session / import_session / resume_session / fork_session
rename_session / archive_session / open_session / focus_session
send_session_prompt / cancel_turn
```

`create_session` 应支持 Harness、模型、思考强度、权限、cwd、Profile、初始任务，以及可选的父
Session/来源关系。provider 特有选项通过能力探测和结构化 overrides 表达，不能伪造一个所有
Harness 都能生效的统一模型字段。

#### 3.1.1 `session.create` 是通用 Session 原语

Host Control 不再定义独立的 `create_subagent` 或一次性 delegation 对象。无论是用户新建普通
对话、Agent 临时找另一个 Harness 调研，还是创建长期协作者，底层都只创建一种真实、持久、可
Resume 的 Session。所谓“子 Agent”只是这个 Session 的来源关系、默认展示位置和生命周期策略。

这里的“子 Agent”只指 **Codeg Host 创建并受管的 Session**，不包括 Harness 在一次 Turn 内自行
派生的原生内部 subagent。后者没有 Codeg 创建意图、Host capability 授权或受管生命周期，默认不
进入 Host Control 的 Session Registry 和普通 UI。Codeg 不应因为扫描到一份 Codex/Claude/Grok
内部 transcript，就自动把它变成可寻址的协作者或顶层 Session。

归属判断不能只看 Harness 的 `thread_source=subagent`：Codeg 创建的受管 Session 也可能由适配器
映射到类似的原生能力。判定优先级为 Codeg 持久化的创建/provenance 事实优先，原生 metadata
其次。该分类是导入和投影规则，不要求继续向 `conversation` 主行堆叠一个同时承担来源、展示和
生命周期的 `is_subagent` 字段。

截至 2026-08-17，已实施的 `session.create` 接受：

```text
harness
folder_id?
cwd?
title?
model?
mode_id?
config_values?
initial_prompt?
collection_id?
```

`collection_id` 在 Session 持久化成功后再做一次 `collection.add_session`；放置失败不删 Session，
返回里会写明 `collection_placement=failed`。父子 `relation` / `session.promote` 仍未做：带
`parent_id` 的 Session 目前不能当 Host Control 调用方。

它会创建 Codeg Conversation、启动真实 ACP/native Session、持久化原生 Resume identity，并可选
启动第一条 Prompt；默认后台创建，不打开 UI、不抢焦点。当前安全边界仍要求 `folder_id` 和 `cwd`
位于调用者绑定的 Path/cwd，思考强度等 provider 能力暂由 `mode_id/config_values` 表达。

目标契约在保持上述字段兼容的前提下扩展为：

```text
SessionLaunchSpec
├── harness
├── title?
├── model?
├── reasoning_effort?           仅在 Harness 明确声明时出现
├── permission_profile?
├── profile?
├── execution
│   ├── cwd
│   ├── folder_id?
│   ├── mode                    cwd | existing_worktree | new_worktree | remote
│   ├── worktree_ref?
│   └── backend_ref?
├── provider_options?           结构化且经 Harness capability 校验
├── initial_prompt?
├── start_policy?               create_only | run_initial_prompt
├── context_seed?
│   ├── mode                    none | summary | recent_turns | explicit_refs
│   └── refs/count?
├── relation?
│   ├── kind                    spawned_child | related | handoff
│   ├── parent_session_id?      默认可由可信 caller context 绑定
│   ├── source_turn_ref?
│   ├── role?
│   └── task_name?
└── placement?
    ├── visibility              nested | top_level
    ├── display_parent_id?
    ├── collection_id?
    ├── workbench_id?
    └── open_mode               background | current_pane | split
```

`fork_session` 仍是独立动作，因为它必须调用 Harness 原生 Fork 或经过明确降级；不能通过
`relation.kind` 假装复制了原生上下文。`reasoning_effort`、权限、Worktree 和远端等字段同样必须由
`codeg_help` 返回的实时 Schema/Capability 决定；不支持时拒绝或省略，不能静默伪装成功。

创建和第一条 Prompt 是一个可恢复 Saga，而不是含糊的布尔成功：至少区分 Session 已持久化、原生
identity 已确认、初始 Turn 已启动、Prompt 投递结果未知、创建失败但需人工恢复等阶段。关系和
Collection/Workbench 放置失败不得删除已经成功创建的 Session；返回稳定 Session ID 后可安全重试
组织动作。

#### 3.1.2 子 Session、归属与提级

不要使用单个 `is_subagent` 布尔值同时表达历史来源和当前展示。至少区分：

```text
provenance              spawned_from_session_id / source_turn_ref，创建后不抹除
relationship            spawned_child / related / handoff
presentation            display_parent_id + nested/top_level，可变
lifecycle_policy        independent / follow_parent，可变且需显式策略
organization            Collection / Workbench 引用，可变
```

`session.promote` 的用户含义是“固定为普通顶层 Session”，内部只改变 presentation 和可选的组织位置；
它不得更换 Session ID、原生 Session identity、transcript、runtime、Mailbox 或 provenance。反向的
`session.demote` 也只是重新折叠展示。父 Session 可以协调子 Session，但不因此拥有删除权；子
Session 可以继续被追问、Fork、加入其他 Workbench，并按权限联系其他 Session。

子关系可递归形成协作树，但 fan-out、深度、并发和费用受 Host Policy 限制。默认临时子 Session
嵌套且后台创建；只有用户明确要求或既有策略授权时才打开、聚焦或创建新 Worktree。

上述嵌套、提级、Mailbox、Workbench 和生命周期规则仅作用于用户拥有或 Codeg Host 拥有的
Session。Harness-internal subagent 默认不参与这些组织关系；需要排障时通过独立的高级诊断投影
查看，不能污染日常 Session 树。

Removal 批次删除旧 Schema、Broker/task_id、专属 UI 和设置；共享 companion、transport、可信 caller
identity、`list_sessions/send_message` 等 Host bridge 必须保留。纯展示卡片/浮窗可抽成普通 Session
activity primitive，但不得因此保留旧状态机。

Collection 是组织投影，不替代 Session 身份。控制面至少需要
`create_collection / rename_collection / move_collection / add_sessions / remove_sessions`。Session
移动和 Collection 重命名不改变稳定地址；后台创建和整理默认不抢用户焦点。

### 3.2 Session 间通信

`send_message` 负责给一个或多个**已有 Session**投递消息，保留目标原有上下文。它与新建 Session
正交：先创建再发送得到 delegation；直接向已有目标发送得到持续协作。消息持久化、是否调用模型、
是否要求回复和 UI 展示仍按 [Session 间通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md) 分层。

#### 3.2.1 紧急消息不等于取消授权

普通 `send_message` 最多请求 `steer_if_supported`，不直接停止目标。破坏性的运行控制使用独立
`interrupt_session(target, then_event_id, reason, client_dedupe_id)` 语义：消息必须先持久化，随后
取消旧 turn，确认运行锁释放后再发送同一事件。UI 可以组合成一个“停止当前任务并发送”动作，
Host Core 中仍保留两个边界，便于鉴权、幂等和审计。

人类可以显式执行；Agent 只有在单独的 Execution Control capability 被预授权时才能调用。等待
时长、`urgent` 标记、会话标题或来自另一 Agent 的文本均不构成授权。Harness 支持原生 steering
时优先尝试非破坏性插话，不支持时回队；未托管外部会话才退到 hook、AgentBus 或人工入口。

### 3.3 Workbench 与布局控制

Agent 可以操作工作现场，但第一版只暴露语义操作：

```text
list_workbenches / create_workbench / rename_workbench
open_workbench / focus_workbench
add_session(session_id, workbench_id, placement?, focus = false)
split_workbench / apply_layout / save_layout
open_resource(path_or_url, owner = session | workbench, placement?)
```

不向模型公开像素坐标、拖拽轨迹或前端布局树的内部结构。`compare`、`review`、`research`、左右/上下
并排等可读预设由 Workbench Manager 翻译成 Pane 变更。每次变更应可撤销、幂等，并返回实际影响
的 Workbench、Pane 和 Session 引用。

当前 Codeg 分屏布局仍主要保存在前端按 Workbench 分区的 `localStorage`，见
[`tab-store.ts`](../../src/stores/tab-store.ts)。因此可分两步实施：

1. MCP 调用后端语义命令，后端把命令定向发送给拥有该 Workbench 的活动客户端，并等待 ACK；
2. 多窗口和远端同步成熟后，把需要共享的布局结构与 revision 提升为后端事实，焦点、滚动和窗口
   几何仍保留在设备/窗口本地。

不能因为 Agent 发出了“打开/聚焦”命令就立即回报“用户已经看到”；只有活动客户端 ACK 后才能
声称已经展示。没有前端连接时可以保存待应用意图，或返回“已记录，尚未显示”。

### 3.4 资源与应用操作

文件、Diff、图片、网页和终端也应走同一控制面，但 Codeg 不因此变成完整 IDE。优先支持：

- 在外部 VS Code/系统默认应用打开文件；
- 在 Codeg 已有预览中打开常用资源；
- 把资源跟随某个 Session，或显式固定到 Workbench；
- 查询真实打开位置和失败原因；
- 不默认关闭、移动或覆盖用户正在使用的内容。

## 4. Skill、渐进式 MCP 与上下文预算

### 4.1 渐进式 MCP 是唯一正式 Agent 端口

Codeg 已经在 ACP `session/new`、`session/load` 和可支持的 `session/resume` 中注入 per-launch
`codeg-mcp` companion。新增 Session、Collection、通信和 Workbench 能力继续复用这条结构化路径。
MCP 是受管 Agent 的正式适配入口，Host command bus/API 才是业务实现；不为不同 Harness 再维护
CLI、原生 Tool 或自然语言解析等第二套语义。

用户担心的上下文成本成立，但应准确表述：消耗上下文的主要是每次暴露给模型的工具名、描述和
JSON Schema，不是 MCP 进程本身。当前 `tool_schema.json` 中仅 delegation 三个工具的描述与压缩
Schema 就约 5,600 字符，其中 `get_delegation_status` 的说明尤其长。即使用户完全不需要委派，
只要该 feature 在本次 companion 启动时开启，这些定义仍会进入工具目录。

现有 `CompanionFeatures` 已经能按启动参数隐藏整组工具，但它是**启动时粗粒度开关**，不是“用户
此刻提出协作需求后才加载”的渐进披露；当前 companion 也没有声明 `tools.listChanged`。因此继续
增加十几个永远可见的 Session、Workbench 和协作工具不是合适方向。

### 4.2 调用者身份由 Host 绑定，不由模型自报

工具参数只应描述模型想执行的业务动作，例如目标 Session 和消息正文；当前调用来自哪个 Session，
属于可信的 Caller Context，不应要求模型填写 `from_session_id`。否则模型可能填错、使用重名标题，
也可能伪造另一个 Session 的身份。推荐链路是：

```text
受管 Harness 中的当前 Session
  → Host 为本次工具通道绑定不可伪造的 caller context
  → 模型只提交目标和业务参数
  → Host Core 根据 caller context 做鉴权、审计和来源标记
```

Codeg 当前已经采用这一原则：每次注入 `codeg-mcp` 时注册临时 token，并把 token 绑定到父 ACP
`connection_id` 和 cwd；companion 经 UDS/named pipe 调用主进程时携带 token，后端再映射真实
Conversation。模型既不需要知道自己的数据库 ID，也不能通过工具参数冒充 `from_session`。完整
源码链路见 [Session 间通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md#91-当前已经存在的注入链路) 和
[delegation 子系统审计](./DELEGATION-SUBSYSTEM-AUDIT.zh-CN.md#31-完整调用链)。

现有产品也提供了同方向的实现依据：

- 2026-08-16 对 Codex Desktop Windows 包的本地解包核对显示，跨任务发送工具公开 Schema 只要求
  目标 `threadId` 和 `prompt`；宿主 dispatcher 在工具参数之外携带 `sourceThreadId`、
  `sourceHostId`。这说明来源是调用环境属性，不是模型正文的一部分。该结论来自压缩产物符号和
  运行时工具面的交叉核对，不把私有字段名视为稳定公共 API。
- Claude Code Desktop（CCD）的会话管理工具在实际运行中同样只要求目标 `session_id` 和消息，
  来源由宿主展示为当前会话；目前这是行为观测，尚未完成可引用的源码审计，因此只作为产品先例，
  不作为字段兼容依据。

必须区分 **Host Session 身份** 和 **MCP 协议会话**。前者是 Codeg 的 Conversation/Connection
安全上下文；后者只是工具传输协议的状态。MCP 2026-07-28 已移除协议层 `Mcp-Session-Id` 和
初始化会话，见 [官方发布说明](https://blog.modelcontextprotocol.io/posts/2026-07-28/) 与
[SEP-2567](https://modelcontextprotocol.io/seps/2567-sessionless-mcp)。因此不能写成“MCP 天然知道当前
Codeg Session”；身份必须由宿主绑定 token、连接上下文或受保护的请求元数据，并在 Host Core
验证。Codeg 当前 per-launch stdio companion 的 token 方案不依赖 HTTP `Mcp-Session-Id`，与此边界
一致。

#### 环境变量只承担 Ambient Context，不单独承担可信身份

环境变量仍是一条有价值的接线方式，但必须区分三个层次：

| 注入位置 | 当前能力 | 边界 |
|---|---|---|
| Codeg 启动 ACP Adapter 进程 | 已支持；Agent 设置的 `env_json` 会合并到 `runtime_env` | 属于 Host 进程管理，不是 ACP wire 字段 |
| ACP `session/new` | 可传 `cwd`、`mcpServers`、附加目录和 `_meta` | 没有通用的“给 Agent Session 设置环境变量”字段 |
| `session/new.mcpServers` 中的 stdio MCP | MCP server descriptor 可以携带 `env` | 只保证 MCP 子进程获得变量，不保证任意 Agent shell 都继承 |

当前 Codeg 已把 `runtime_env` 注入 ACP Adapter，但还没有 Atrium 式统一上下文契约；`codeg-mcp`
身份目前通过 `--parent-connection-id`、`--socket-path` 和 per-launch `--token` 传给 companion。另一个
容易忽略的事实是：Codeg 托管的 ACP Terminal 为避免泄漏模型凭据，并不继承完整 Agent
`runtime_env`。因此仅给 Adapter 添加 `CODEG_SESSION_ID`，不能保证所有 Harness、MCP shell 和
Codeg Terminal 都看到相同上下文。

环境变量若需要，只注入 Codeg 托管 MCP/Harness 所需的少量 ambient identity/context：

```text
CODEG=1
CODEG_BACKEND_REF=<opaque backend ref>
CODEG_ENDPOINT=<local socket or managed backend endpoint>
CODEG_CONTEXT_TOKEN=<opaque, scoped, revocable capability token>
```

标题、角色名和可变布局不复制进环境变量。token 只能代表当前托管连接获准的能力，不能是全局管理
Token，更不能采用长期签名私钥。模型不传自己的 `session_id` 或 `from`；MCP handler 从 Host 绑定
的 connection/token 反查 `backend_ref + conversation_id`，并把该身份写入审计。

环境变量是进程级而非协议 Session 级。只要一个 Adapter 进程未来可能承载多个 ACP Session，就
不能用一份进程环境区分它们；每 Session 注入的 MCP companion/capability context 仍是受管通信的
主路径。Atrium 的方案成立，是因为 Pane、进程和 Harness Session 通常近似一一对应；Buzz 的
`BUZZ_PRIVATE_KEY` 则绑定 Agent Actor 身份，具体 Channel 仍由 Prompt Context 和 CLI 参数指定。

### 4.3 产品裁决：Skill + Codeg 自有渐进式 MCP

推荐模型是：

```text
极小常驻面
  codeg_help / codeg_use（名称待定，也可合并为一个 codeg 元工具）
                         │
           用户出现明确需求后加载逻辑 Skill
                         │
     session-lifecycle / collaboration / workbench 等能力包
                         │
          同一个 Codeg Host Core 执行动作并审计
```

Skill 负责渐进披露“何时用、怎样组合、有哪些坑和例子”，Capability Catalog 负责告诉 Agent
当前可用能力和参数，渐进式 MCP 负责发现、分域加载和结构化调用。Skill 可以声明依赖能力包；
加载后通过极小网关发现或直接调用对应动作。

这里必须区分两种“渐进式披露”：Skill 按需加载知识、流程和示例，元工具按需发现并执行能力。
二者思想相同但执行语义不同；Skill 本身不产生副作用，元工具的外层参数由 Provider/Harness 校验，
真实 action 的内层参数仍必须由 Codeg Host 再做严格校验。

这里作出明确端口决策：**受 Codeg 管理的 Session 只维护 `Skill + 自有渐进式 MCP` 一条正式
Agent 控制路径。** 业务能力只在 Host command bus/API 中实现，MCP 做类型化适配：

```text
Skill：说明何时使用、怎样组合
                 │
        Host Capability Catalog
                 │
      Codeg progressive MCP
                 │
             Host Core
```

选择 MCP 的原因是：Codeg 已有 per-session companion 与 caller token；结构化参数可避免 shell
quoting、命令注入和 stdout JSON 解析；工具调用、审批、取消与结果可以直接进入现有 UI。Schema
成本由极小 gateway、按需 capability 和 Skill 共同控制，不以通用 shell 或 CLI 作为退路。

对支持运行时 Tool List 更新的客户端，可在启用能力包后重新列出语义工具。对会缓存工具 Schema、
不支持可靠热刷新的客户端，不要强求“动态启动另一个 MCP”——由常驻的 `capability_use` 网关直接
路由到未展开工具即可。这样既保持结构化参数、权限和审计，也不会把全部 Schema 永久放进上下文。
普通客户端无法同时保证“初始不携带大量工具定义”“早期 Prompt Cache 始终稳定”“每个隐藏工具仍
作为独立 Provider 原生工具接受完整 Schema 约束”。第一阶段固定极小网关，把具体 action 的发现、
授权、校验和执行收敛到 Host。厂商 Tool Search 或动态工具协议只能在宿主内部透明优化这条路径，
不能产生第二套工具语义或破坏跨 Harness 一致性。

### 4.4 Skill 教 Agent 何时使用

Skill 负责：

- 什么时候继续已有 Session，什么时候新建临时子 Session；
- 如何为冷启动任务携带必要上下文；
- 怎样避免未经用户要求创建大量 Session；
- 怎样在安排布局时不抢焦点、不打乱用户现场；
- 如何处理重名、忙碌、不可 Resume、工具缺失和远端降级。

Skill 不负责传输、持久化和权限，也不能靠解析普通回答中的自然语言来产生副作用。加载 Skill
本身也不代表动作已经发生；真正的 Session 创建、发信或布局变更仍必须出现可审计的结构化调用。

### 4.5 不建立 CLI fallback 产品线

不支持 MCP 注入的 Harness 暂不提供 Agent Host Control Surface；Codeg 不为此承诺或建设第二套
CLI 产品路径。仓库已有的开发者内部命令、服务进程和诊断入口可以保留，但不进入产品验收，也不
要求与 MCP 保持公开语义 parity。只有未来出现明确需求，且 CLI 能从同一 command catalog 近乎零
维护成本地生成时，才重新讨论；当前环境变量也不构成 CLI 承诺。

### 4.6 厂商 Tool Search 只是透明优化

OpenAI Responses API 与 Anthropic Messages API 已分别提供按需加载工具定义的 Tool Search；MCP
2026-07-28 也把传输核心改为无握手的无状态请求，并增加发现与缓存提示。这些能力说明渐进披露的
方向合理，但不证明每个 Codex/Claude Desktop、CLI 或 ACP Host 都已经启用，也不改变 Codeg 的
产品接口：

- Codeg 继续维护统一的小型渐进式 MCP，厂商 Tool Search 若由宿主透明使用，只是减少 Schema
  上下文的优化；
- 当前不实现 Tool Search capability negotiation、专用 adapter 或原生优先分支；
- Tool Search 不解决权限、身份绑定、返回 payload、生命周期或 retrieval 失败；
- MCP 2026-07-28 只作为未来传输升级参考，不触发当前 companion 重写。

参考：[OpenAI Tool Search](https://developers.openai.com/api/docs/guides/tools-tool-search)、
[Anthropic Tool Search](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool)、
[MCP 2026-07-28 发布说明](https://blog.modelcontextprotocol.io/posts/2026-07-28/)。

## 5. 参考实现及可借鉴部分

### 5.1 Atrium：CLI 就是控制面

Atrium 在每个受管 Pane 中注入 `$ATRIUM_CLI_PATH` 和 Pane/Workspace 身份，再通过 Skill 教 Agent
调用 CLI。Workspace、Room、Pane、Agent、Task、Browser 等都可寻址；Agent 能创建/拆分 Pane、
启动其他 Agent、跨 Pane 发消息并打开资源。官方文档明确把 CLI 视作 API：

- [Atrium Quickstart](https://getatrium.dev/docs/quickstart)
- [Atrium CLI/产品说明](https://getatrium.dev/)

最值得直接吸收的不是命令名字，而是两条规则：同一个控制面同时服务人和 Agent；所有会改变用户
当前焦点的命令默认不聚焦，只有用户明确要求时才切换 Room/Pane。

### 5.2 Paseo：共享 Tool Catalog，MCP 只是适配器

Paseo 同时提供人类 CLI 和 Agent 工具。其 Agent 工具包括创建 Agent、向运行中 Agent 发送 Prompt、
查询状态、更新模型/思考选项、管理 Workspace 和 Terminal。支持原生工具的 Provider 直接注册同一
catalog，只支持 MCP 的 Provider 通过 MCP 注入：

- [Paseo MCP reference](https://github.com/getpaseo/paseo/blob/main/public-docs/mcp.md)
- [Paseo provider architecture](https://github.com/getpaseo/paseo/blob/main/docs/providers.md)

这支持 Codeg 把工具定义与 MCP transport 分离，避免未来增加原生 Host Tool 时复制业务逻辑。

### 5.3 CCCC：薄端口与真正的渐进能力披露

CCCC 当前把 Web、CLI、MCP 和 IM 定义为同一个 daemon 状态模型的薄入口，并允许 CCCC 调度 Agent，
也允许 Agent 经 MCP 反向管理 CCCC 工作流。CLI 与 MCP 不是互相套壳：`cccc mcp` 启动 stdio MCP
端口，二者分别把请求转成同一个 daemon operation。更直接相关的是它已经实现了 MCP 与 Skill 的
组合：

- 当前默认面包含 16 个 basic 和 5 个 admin，共 21 个核心工具，而不是整个工具库；
- `cccc_capability_search` 负责发现能力，`cccc_capability_use` 可以一步启用并调用未展开的工具；
- 可选能力按 tool pack 管理，Skill 以 capsule runtime 按需返回指导文本，还可以声明所需能力；
- stdio MCP 当前声明 `listChanged: true`，在客户端支持时发送工具列表变更通知；对于会缓存 Schema
  或不能可靠热刷新的客户端，内置能力仍可藏在固定 `capability_use` 后直接调用；
- capsule skill 只是运行时指导文本，并不等于安装了带脚本、资源和完整引用文件的本地 Skill。

- [CCCC](https://github.com/ChesterRa/cccc)
- [CCCC 当前 MCP 工具说明](https://github.com/ChesterRa/cccc#MCP-Tools)
- [CCCC capability packs 与 capsule skills](https://github.com/ChesterRa/cccc/blob/main/src/cccc/kernel/capabilities.py)
- [CCCC capability-use 网关](https://github.com/ChesterRa/cccc/blob/main/src/cccc/ports/mcp/handlers/cccc_capability.py)
- [CCCC MCP 客户端兼容处理](https://github.com/ChesterRa/cccc/blob/main/src/cccc/ports/mcp/main.py)

Codeg 应借鉴“一个权威核心、多种端口”和“核心 + 按需能力 + Skill capsule”，而不是照搬 CCCC
当前仍然偏大的核心工具面或 Group/Actor 产品层级。对 Codeg 而言，能力目录的主体仍是 Session、
Workbench 和资源，caller 身份继续以 per-session token 到 ACP connection 的 Host 绑定为准，不能
退化成仅信任调用参数或普通环境变量。

### 5.4 pi-mcp-adapter：更纯粹的固定元工具

`pi-mcp-adapter` 默认只把一个约 200 tokens 的 `mcp` 网关放进模型上下文；工具元数据缓存到本地，
MCP Server 延迟到首次调用时连接。`mcp({search})`、`mcp({describe})` 和 `mcp({tool,args})` 分别
完成发现、查看契约和执行。可选 `directTools` 才把特定能力提升为一等 Pi 工具。

它还提供可关闭的 `mcpScript`，让 Agent 生成受控 JavaScript，在一次调用中循环、串联、过滤或并发
调用多个 MCP 工具。这不是面向人类的脚本 CLI，但仍扩大了执行面；Codeg 第一阶段已有多目标
`send_message`，不需要为了批量通信引入通用脚本运行器。

CCCC 和 `pi-mcp-adapter` 属于同一类渐进能力网关，但程度不同：CCCC 是“常用业务工具常驻、长尾
能力走 gateway、必要时动态展开”的混合型；Pi 默认是“几乎全部真实 MCP 工具藏在一个 gateway
之后”的纯网关型。Codeg 更适合采用 CCCC 的 daemon/身份/审计思想，加上 Pi 的极小 Agent 入口：

```text
Codeg Skill：何时用、怎样协作、错误与示例
                 │
codeg_help / codeg_use：发现契约并执行
                 │
Codeg Host Core：身份、参数校验、权限、队列、审计
```

- [pi-mcp-adapter](https://github.com/nicobailon/pi-mcp-adapter)
- [固定 `mcp` 网关与 `mcpScript` 实现](https://github.com/nicobailon/pi-mcp-adapter/blob/main/index.ts#L608-L680)
- [LinuxDo：关于渐进式披露工具上下文的几种方向讨论](https://linux.do/t/topic/2708576)

### 5.5 CC-Panes：语义化 Pane/Session Orchestrator

当前 CC-Panes 已拆出 core、HTTP/WebSocket API、daemon、Web 客户端和 `cc-panes-ctl` 控制 CLI/MCP
proxy。其 Orchestrator MCP 已覆盖 `launch_task`、`list_panes`、`open_file`、`open_browser_tab`、
`write_to_session`、`submit_to_session`、`wait_for_session` 等；启动任务可指定 `pane_id`、
`layout_id`、`layout_name` 和 `placement`。源码参考：

- [`orchestrator_service.rs`](../../../cc-pane-latest/src-tauri/src/services/orchestrator_service.rs)
- [`cc-panes-ctl`](../../../cc-pane-latest/cc-panes-ctl/)

它证明“让 Agent 操作布局”不需要暴露拖拽坐标：列出 Pane、选择目标布局和语义 placement 已能覆盖
大部分工作流。Codeg 不应照搬其 PTY/Terminal 中心模型，但可复用这一工具粒度。

### 5.6 cc-haha：SubAgent 与 Team 是两个层级

cc-haha 的普通 `Agent(...)` 是冷任务/后台 SubAgent；Agent Teams 在其上增加名字、Team、
`SendMessage` 和广播。这证明一次委派与持续通信需要分开，但也说明 Team 不必成为所有 Session 的
前置容器：

- [cc-haha Multi-Agent guide](https://github.com/NanmiCoder/cc-haha/blob/main/docs/en/internals/agent.md)

Codeg 的优势应是让任意可 Resume 的持久 Session 参与通信，而不是只允许同一个 Team 生命周期中
新创建的成员。

### 5.7 Buzz：CLI 是平台语义接口，也暴露了失败边界

Buzz 的 `buzz-acp` 负责监听 Relay、按 Channel 维护 Session、组装上下文并经 ACP 驱动 Harness；
Agent 要把结果变成 Channel 中的公开消息，则由基础提示明确要求调用 `buzz messages send`。对拥有
原生 shell 的 Harness，Agent 直接执行 Buzz CLI；`buzz-dev-mcp` 提供的也主要是通用 `shell`、文件、
图片和 todo 工具，Buzz 消息操作仍是 `MCP shell -> buzz CLI`，而不是类型化 `send_message` MCP。

这一设计展示了 CLI 作为跨 Harness 接口的可能性；其代价同样重要：Agent 已经在 ACP Turn 中生成
回答，却可能因为没有成功调用 CLI 而不出现在频道；
shell quoting、PATH 和返回 JSON 解析成为业务可靠性的一部分。Buzz 还把 `BUZZ_PRIVATE_KEY` 注入
CLI/MCP 子进程，用 Nostr 签名确定 Actor 身份；这适合其跨 Relay 身份模型，却让有 shell 权限的
Agent 理论上能够读取长期私钥，不应复制到 Codeg。

Codeg 是 Session-first：普通 ACP 回答天然属于当前 Session，不应再次执行 CLI 才能“发布”。只有
跨 Session 发信、创建/Fork Session、Goal/Timer、Workbench 和资源操作等显式副作用进入 Host
Capability。Codeg 受管 Session 只走结构化渐进式 MCP；Buzz 式 CLI 不进入产品兼容矩阵。调用者
身份继续使用短期、限权、可撤销的 Host 绑定 token，不使用长期身份私钥。

## 6. 安全与交互约束

1. 默认不抢用户焦点，不切换 Workbench，不移动现有 Pane；显式 `focus=true` 需要用户请求或确认。
2. 关闭 Session、杀死 Runtime、删除 Workbench/Collection、覆盖草稿属于破坏性操作，不和普通
   布局工具混在一起。
3. Agent 只能操作调用者有权访问的 Backend、Folder、Session 和资源。
4. 工具返回“请求已接受”“已持久化”“前端已 ACK”“模型已开始”“任务已完成”等真实阶段，不能
   用一个 `success` 混淆。
5. 所有写操作接受幂等键；布局写入携带 revision 或由唯一 mount 所有者串行化。
6. Agent 普通 Markdown、代码块或自然语言中的 `@名字`、`open`、`arrange` 不产生副作用；必须是
   用户结构化动作或可审计 Tool Call。
7. 新建临时 Session 应显示预计 Harness、模型、cwd、权限与是否进入顶层列表；大规模 fan-out
   受并发、Token 和 Session 数量限制。
8. 多步写操作携带 `request_id`，由 Host Core 提供事务边界、幂等结果、审计记录和可说明的撤销
   边界；模型不靠重复调用猜测前一次是否成功。
9. 创建/重命名 Collection、把 Session 加入 Workbench 等可逆组织操作可以按策略默认允许；冷启动
   Harness、发送 Prompt、改变 cwd/model、steer 或 interrupt 必须来自用户指令或显式策略；删除
   Session、Collection、Workbench 等破坏性操作必须确认。

## 7. 建议实施顺序

### H0：统一现有服务边界

- 删除三项 delegation 工具、task_id Broker 和专属设置/UI；
- 保留共享 `codeg-mcp` companion、transport、caller identity 和普通 Host 工具；
- GUI、HTTP 和 `codeg-mcp` 不各写一套启动逻辑；
- 把子会话悬浮层降为状态 Peek，增加“在 Workbench 打开真实 Session”，不在 Dialog 里重造聊天。

### H1：完整 Session Lifecycle Tool

- list/search/get；
- create/import/resume/fork/rename/archive/open/focus；
- send prompt、status、cancel；`wait_sessions` 仅为 Optional/Later 的宿主 orchestration 能力；
- Collection create/rename/move/add/remove；
- 未来新建的 Session 天然可继续、Fork、重命名和固定，不再提供 delegation 预设；
- 引入极小 capability gateway，避免把完整 Lifecycle/Collaboration/Workbench Schema 永久暴露。

### H2：持久 Session 通信

- 实现 Collaboration Core、`list_sessions`、多目标 `send_message`；
- 目标忙碌、休眠、远端和不可 Resume 时显示真实投递状态；
- 不要求 Team、Room 或 AgentBus 注册。
- 能力门控的 `steer_if_supported` 与用户触发的“停止当前任务并发送”，并保证消息先落库、取消
  幂等、旧 turn 可追溯。

### H3：语义化 Workbench/资源工具

- list/open/arrange/switch；
- 活动客户端 ACK、默认不聚焦、撤销和 revision；
- 文件/网页/图片跟随 Session 或固定到 Workbench。

### H4：远端 MCP 与 Host 传输

- 保留 Web/移动端和远端 Backend 的相同权限、身份与状态语义；
- 渐进式 MCP 在远端仍调用同一 Host command bus/API，不复制业务实现；
- AgentBus 继续作为 Codeg 之外的传输 Adapter，不和 Host Core 争夺同一 Delivery。

## 8. 验收要点

1. 三个 delegation 工具、task_id Broker 和专属 UI 已移除，不再形成第二套 Agent 产品；
2. 未来 create_session 创建的是普通持久 Session，可继续追问而不是一次性 task；
3. 新建、继续、Fork 和联系已有 Session 在 UI、API 与 Agent Tool 中使用同一身份和运行服务；
4. Agent 能把指定 Session 打开并排成可读布局，默认不抢焦点，操作可撤销；
5. 没有 GUI 连接时不会谎报“已展示”，多个窗口不会同时覆盖同一 Workbench 布局；
6. GUI 与渐进式 MCP 不复制业务状态机，Agent 来源由 Host 绑定上下文确定；
7. 关闭工具或 Harness 不支持 MCP 时，普通 GUI Session 仍能完整工作，但 UI 不声称 Agent Host
   Control 可用；
8. 所有竞品参考只影响实现选择，不自动增加用户必须理解的 Workspace、Team、Room 或 Task 层级；
9. 未触发协作、委派或布局需求的普通 Session 不承担这些完整工具 Schema 的常驻上下文成本；
10. 不支持动态 `tools/list` 刷新的 Harness 仍可经极小 gateway 调用按需能力，不要求重启会话。
