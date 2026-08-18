# Codeg AgentBus 协作集成 RFC

> 状态：Draft（2026-08-19 对账注：未启动的设计存档，代码零实现；何时启动见维护计划）  
> 日期：2026-08-15  
> 依赖：稳定的 Session 身份、原生 Resume、Session Registry 及 Workbench 基础  
> 定位：把现有 AgentBus 的跨 App/主机通信能力融入 Codeg UI 和受管 Harness turn，
> 同时保留独立 daemon 与 CLI。

> 优先级说明：本文是后置协作轨道，不是 Session Workbench 第一阶段的前置设计。第一阶段先完成
> Session 身份、Collection、Workbench、搜索和布局；协作先从多选发送、转发、比较与投递状态
> 开始，再在稳定 Session 投递和多视图同步之后实现可选群聊关系。

## 1. 背景

AgentBus 已经解决了跨 Codeg、Codex/Claude Desktop、独立 CLI、Docker 和远程主机发送持久
消息的问题，但当前主要依靠命令行和 Agent 自觉执行协议，长期使用存在明显管理成本：

- 用户不记得有哪些 Bus project；
- 不记得每个 project 有哪些 role；
- 不知道 role 当前绑定到哪个真实 Session；
- Session 标题、Harness、cwd 与 Bus session id 缺少统一映射；
- pending、leased、owed 和回复关系只能通过 CLI 查看；
- `takeover`、`eject`、`leave`、`quit` 缺少影响预览和可视化入口；
- 非受管 App/CLI 必须主动 `recv --wait --all`，消息不能稳定唤醒会话；
- 消息出现在 Terminal/工具输出里，没有自然进入 Codeg 的会话或协作 UI。

因此 AgentBus 的下一步重点不是重写传输，也不是成为 Codeg 内部协作的强制中转站，而是增加
一个以 Session Registry 为基础的外部通信控制面。

### 1.1 哪部分增强 Codeg，哪部分增强 AgentBus

| 问题 | 主要归属 |
|---|---|
| Session 标题、Harness、cwd、Collection、Workbench、Room 和联系人 UI | Codeg |
| 选择目标、内部 Session Resume、turn 队列和回复展示 | Codeg |
| 外部 project/role 与 Codeg Session 的持久绑定和人类可读别名 | Codeg 集成层 |
| 跨 App/主机 mailbox、lease、owed、reply-to、附件和去重 | AgentBus daemon |
| 外部 CLI 的主动唤醒 | 可选 Gateway/Wrapper，不是普通 mailbox daemon |

不优先给 AgentBus 单独建设另一套 Session Library、Collection、Room 和 Workbench UI；那会与
Codeg 重复，并且仍然不能直接控制未托管 Harness。AgentBus 自身只增加所有客户端都需要的传输、
诊断或 Gateway 契约，用户日常管理与协作体验在 Codeg 中完成。

## 2. 架构决策

### 2.0 Codeg 原生路由优先，AgentBus 负责跨边界

当发送者和接收者由同一个 Codeg Backend 管理时，无论 Backend 位于本机、远程主机还是 Docker，
都直接使用 Conversation 稳定 ID、已有 connection manager、owner/viewer runtime 和内部事件
路由。用户不需要为此另建 Bus project、role、daemon 或 Terminal wait。网络位置不是边界，
Backend 是否掌握 Session 身份与运行时才是边界。

当前桌面端的 remote-workspace 已经通过认证 HTTP 与 WebSocket 将一个独立窗口绑定到远端
`codeg-server`，并为不同远端窗口隔离事件流。因此远端 Server 内部的 Session 已经可以由该
Server 原生管理。尚未具备的是一个跨多个 Backend 的全局 Registry 和持久 Server-to-Server
协作通道；跨 Backend 地址至少要表达 `backend_ref + conversation_id`，不能假设各数据库中的
Conversation ID 全局唯一。这里的 `backend_ref` 必须来自远端 Server 的稳定实例身份或受信绑定，
不是本机可删除重建的 `remote_workspace_connection.id`、显示名称或可变 URL。

AgentBus 之所以要求 `project + role + session credential`，是因为不同 App 之间没有共同的
Session Registry，daemon 既无法知道“这个会话是谁”，也无法直接控制它的 Harness 生命周期。
注册占座是在缺少共享身份与主动注入能力时建立地址的兼容机制，不应倒推成 Codeg 内部的产品
模型。Codeg 已经索引 Session 身份和能力后，发送者只需选择或 `@` 一个现有 Session；底层再把
稳定 Session Address 解析到本 Backend 运行时、远端 Codeg API、AgentBus binding 或不可投递状态。

因此，Codeg 中“联系过谁”可以由 collaboration event 和最近联系人自动形成；只有确实需要
共享公共时间线时才创建 Room。一次 direct 消息不创建 Team、成员关系或新的 Agent 身份。

只有以下情况进入 AgentBus：

- 目标 Session 运行在未被 Codeg Backend 接管的 Codex/Claude Desktop 等外部 App；
- 两个 Codeg Backend 尚无直接 federation，且需要离线可靠或 Agent 自主跨 Backend 交接；
- Codeg 关闭后仍必须接收、排队和追踪消息；
- 现有外部工作流已经采用 AgentBus 协议。

UI 可以把两条路径统一呈现为联系人、群成员和投递状态，但后台必须保留来源与能力差异。

### 2.1 AgentBus 保持独立传输层

AgentBus daemon 继续拥有自己的持久邮箱和网络协议。在本协作轨道的初始阶段，Codeg 作为 AgentBus 客户端、
Operator UI 和受管运行时适配器，不把 Bus 数据库合并进 Codeg SQLite。

保留 daemon/CLI 的原因：

- Codeg 未运行时，远程、Docker 和其他 App 仍能通信；
- CLI 适合自动化、恢复、诊断和无 GUI 环境；
- daemon 可以部署在 Windows、局域网或远程主机；
- Bus 生命周期可以独立升级，不与 Codeg 数据库迁移强耦合；
- 非 Codeg Session 仍可加入同一个通信空间。

Codeg 可以以后提供“启动/停止本地 AgentBus daemon”的便利功能，但它仍是独立服务，而不是
隐藏在某个前端进程中的临时内存总线。

### 2.2 Codeg 提供管理与运行时适配

```text
                    ┌──────────────────────────┐
                    │      AgentBus daemon     │
                    │ projects / roles / mail  │
                    └─────────────┬────────────┘
                                  │ HTTP
           ┌──────────────────────┼──────────────────────┐
           │                      │                      │
┌──────────▼─────────┐  ┌────────▼────────┐  ┌──────────▼─────────┐
│ Codeg Operator UI  │  │ Codeg managed   │  │ external App/CLI   │
│ inventory/messages │  │ Session adapter │  │ bus CLI / hooks    │
└────────────────────┘  └─────────────────┘  └────────────────────┘
```

Codeg 不取代 Bus mailbox；它在跨边界路径中补充：

1. project、role、Session 绑定的可视化管理；
2. 收件箱、回复债务和消息线程；
3. 用户发信、回复和广播；
4. 对 Codeg 自己管理的 Session 进行确定性投递；
5. 把消息状态和协作事件自然呈现在私聊 Session 或群聊 UI 中。

纯 Codeg 内部通信不出现在 AgentBus project/role 清单中，除非用户显式启用外部桥接。

### 2.3 统一 Delivery Router，不把投递方式暴露成三套产品

群聊、私聊转发和 Agent-to-Agent 请求先进入 Codeg Collaboration Core，再由 Delivery Router
按目标能力选择适配器：

```text
Group Conversation / direct request
              │
              ▼
     Collaboration Core
     message id / reply-to / policy
              │
              ▼
        Delivery Router
        ├── codeg-runtime://conversation-id
        │   └── 活动连接直接注入，或显式 Resume 后发送
        ├── codeg-server://backend-ref/conversation-id
        │   └── 受管远端 Backend API；不需要 AgentBus 注册
        ├── agentbus://endpoint/project/role
        │   └── 未托管 App 或缺少 federation 的跨 Backend 持久 mailbox
        └── managed-cli://gateway/session-id       （未来）
            └── 受管 PTY/mux 或 provider 原生协议主动唤醒
```

三条路径共享稳定 message ID、sender/target、reply-to、去重和投递状态，但不能重复投递同一消息。
路由记录必须说明由哪个 adapter 获得投递权；不能同时“直接注入 + AgentBus skill 再领取”。

### 2.4 `wait` 是降级能力，不再是 Codeg 内部会话的生命周期

Codeg 已掌握活动 Session connection 时，不需要目标 Agent 先执行 `recv --wait`。消息可以作为
结构化 inbound event 进入同一个运行时；目标正忙时进入明确队列，空闲或离线时按用户策略选择：

- 只进入协作 inbox，不自动消耗 Token；
- 恢复 Session，并在下一 turn 发送；
- 目标明确允许时排为当前 turn 的 follow-up；
- 高优先级且用户授权时才中断当前 turn。

外部、不可控 Session 仍可使用 `recv --wait --all`。它的价值是无需接管 CLI、跨环境简单、行为
安全且 mailbox 可恢复；缺点是合作式 pull，wait 调用结束后不能保证再次唤醒模型。AgentBus
承诺的是消息和回复债务不丢，而不是 Agent 永久在线。

Codeg 集成后，长轮询由一个应用级 BusBridge/Operator 后台任务承担，不由每个受管模型 Session
执行 AgentBus skill 的永久 LOOP。Bridge 领取消息、检查绑定、选择唯一 Delivery Adapter，再把
消息放入目标 Session 的协作 inbox、队列或 Resume turn。模型完成回复后可以正常结束；下一封
消息仍由 Bridge 接收。裸 Codex/Claude Desktop 或普通 CLI 不受 Codeg 管理时，才继续由各自
Session 执行注册、owed/inbox 和 `recv --wait` 协议。

这里要区分两种“等待”：Codeg 原生路由消除的是**接收方先挂起 mailbox wait 才能被唤醒**；
发送方如果要求同步拿到结果，可以在同一个发送动作上显式选择等待，由 Codeg 后台持有订阅并
返回结果，不需要再暴露第二套重叠工具。异步模式则立即返回 job/message ID，回复进入群聊时间线
或来源 Session 的协作记录；是否恢复发送方由显式策略决定。两者都不要求目标模型自行轮询
AgentBus。

普通 `send_message` 默认异步：发送者结束当前 turn，UI 保存 pending 状态，回复到达后再通知或
按显式策略恢复发送者。同步等待只能是本次调用的选择，不能变成“只要注册过 AgentBus，以后每次
回答用户都必须重新 park”的应用级生命周期。

### 2.5 未来 Managed CLI Gateway / Wrapper

未来可以给 AgentBus 增加可选 Session Gateway：Gateway 由自己启动或接管 CLI 的 PTY/mux，注册
到 daemon，收到消息后主动向目标输入通道写入 prompt，再把完成结果和 reply-to 回传。它类似
CCCC、claude_codex_bridge 的托管模式，而不是当前 AgentBus daemon 自身突然获得唤醒能力。

优先级应为：

1. provider 官方结构化接口、SDK、app-server 或 ACP；
2. Codeg 已拥有的 Harness connection；
3. 受管 PTY/ConPTY/Herdr/tmux Gateway；
4. `tmux send-keys`、外部终端自动化等兼容适配；
5. 普通 AgentBus mailbox + `wait/recv`。

Wrapper 不能只是 `claude "$prompt"` 的 shell 别名。可靠实现必须拥有真实 PTY，并通过独立控制
socket 接收注入；终端 stdout/stderr 字节、ANSI、alternate screen、光标、raw mode、窗口 resize
应原样转发，控制消息不得混进终端输出。只有确认 CLI 位于可输入状态时才允许注入，还要处理：

- 人类正在输入与远程注入的仲裁；
- approval、密码、选择菜单和 copy mode 等非 prompt 状态；
- 多行文本、括号粘贴、Enter 延迟和特殊字符；
- 当前 turn 忙、取消、排队和中断语义；
- 原生 Session ID、Resume、重启和 Pane 重建；
- prompt 与最终回复的相关 ID，避免把中间屏幕内容误当成完成；
- 断线、重放和 msgid 去重；
- 远程消息触发工具执行时的权限、允许列表和 Token 预算。

因此 Gateway 是独立后续里程碑。Codeg 自己管理的 ACP/原生 Session 不应绕路使用 PTY 注入；
Gateway 的意义是把 Codeg 之外、但愿意被托管的 CLI 接入同一 Collaboration Core，同时让
AgentBus CLI/daemon 在没有 Codeg 的地方继续可用。

## 3. 术语与边界

### 3.1 Bus Project 不等于 Collection 或 Folder

AgentBus project 是通信命名空间；Codeg Collection 是语义分类；Folder/Execution Context 是
运行路径。它们可以建立可选映射，但不能默认同名或共用主键。

```text
Bus Project ──optional mapping──> Collection / Group Conversation
Bus Role    ──binding───────────> Session
Conversation───────────────────> Folder / Execution Context
```

一个 Collection 可以没有 Bus project；一个 Bus project 也可以包含跨 Collection、跨 Folder
甚至不在 Codeg 中打开的 Session。

### 3.2 Role 不等于 Agent Profile

Role 是某个 Bus project 中的邮箱地址和协作席位，例如 reviewer。Agent Profile 或
`custom_agent` 是启动 Harness 的配置。真正的绑定关系必须指向具体 Conversation/Session。

### 3.3 AgentBus Session ID 不直接等于 external_id

AgentBus 用 Session ID 防止角色误绑定；Codeg `conversation.external_id` 是 Harness 原生会话
身份。两者可能相同，也可能完全不同。集成必须显式保存绑定来源和校验状态，不能通过字符串
相等偷偷推断。

### 3.4 不虚构在线状态

AgentBus 只知道绑定、显式 leave、最近 Bus 活动、pending、leased 和 owed，不知道 Agent
正在思考、窗口关闭还是网络断开。UI 可以分别显示：

- AgentBus 最近活动；
- Codeg 自己的 ACP/运行时连接状态；
- mailbox 事实状态。

不得把三者合并成不可靠的“在线/离线”灯。

### 3.5 产品显示名与稳定投递标识分离

Session 标题、群内昵称和 AgentBus `project/role` 都可能修改、重复或被重新绑定，只适合人类
显示和作用域内寻址。实际投递应保存稳定的 Conversation、Bus session、endpoint 和消息 ID。

普通用户只看到可读名称；稳定 ID 放在高级详情、诊断和冲突处理界面。不得要求用户靠复制
Session ID 或人工拼接 `project/role` 才能完成日常群聊。

## 4. 用户界面

### 4.1 协作总览

```text
AgentBus advanced
├── Connection
├── Bindings
│   └── project-a
│       ├── planner  → Session A
│       ├── writer   → Session B
│       └── reviewer → external session
├── Unbound roles
├── Pending messages
└── Owed replies
```

这个页面是高级控制面，不是用户日常组织 Session 的主导航。日常多 Agent 交流表现为已有
Session 上的发送/转发动作，或可选的普通群聊面板；只有绑定、远程邮箱、takeover、诊断和 owed
恢复需要进入本页。

绑定一旦建立就由 Codeg 持久保存，并以 Session 显示名、头像和可选联系人别名呈现。普通用户不
需要记住 `project/role`；标题或别名改变也不修改底层稳定绑定。应用启动或连接恢复时由 Bridge
核对绑定和邮箱状态，不能要求用户靠记忆重新注册全部席位。

每个席位显示：

- project / role；
- 绑定的 Codeg Session，或“外部 Session”；
- Session 显示名、Harness、cwd；
- session id 的安全短前缀；
- explicitly left；
- last Bus activity；
- pending、leased、owed 数量；
- Codeg 是否掌握该 Session 的运行时连接。

### 4.2 消息中心

支持：

- 按 project、role、状态过滤；
- 按 `reply_to` 显示消息串；
- 区分 request 与 notify；
- 显示 pending、leased、ACKed、owed、abandoned；
- 由用户向 role 或跨 project role 发信；
- 显式 broadcast；
- 附件上传、校验和下载状态；
- 从消息跳转到绑定 Session、群聊或 Workbench；
- 把某条回复转发给另一个 Session。

普通消息默认不广播，避免无意 fan-out 和 Token 开销。

### 4.3 绑定管理

用户可以从 Session 菜单执行：

- 加入 AgentBus project 并选择 role；
- 查看当前绑定；
- leave / quit；
- 在明确预览后 takeover；
- 对已消失 Session 执行批量 eject；
- 多选若干已绑定 Session 创建一个普通群聊。

危险操作必须先显示 pending、leased、owed、可能 abandoned 的请求和继任影响，再允许确认。

## 5. 消息如何进入 Harness turn

Codeg 应按运行时能力选择投递模式，而不是假装所有 Session 都能被主动唤醒。

| Session 状态 | 建议投递方式 | UI 表达 |
|---|---|---|
| Codeg 当前管理且有活动连接 | 通过现有 ACP/运行时连接发起或 steer turn | 已投递到运行时 |
| Codeg 可原生 Resume，但当前休眠 | 经用户授权或策略恢复后发送 follow-up | 正在恢复 / 已投递 |
| 外部 App/CLI 已接 AgentBus | 保留 mailbox，等待 `recv --wait --all` 或 hook | 已入邮箱，未声称已唤醒 |
| 只读导入、Harness 不支持 Resume | 只显示消息并提供复制/创建 continuation | 无法直接投递 |

### 5.1 Turn 中的呈现

AgentBus 消息不应只作为 Terminal 文本。对 Codeg 受管 Session，可以在时间线中显示一个明确的
协作事件或结构化输入：

```text
AgentBus request
from: project-a:planner
msgid: 42
body: ...
attachments: ...
```

它必须保留来源和 msgid，回复时写入 `reply_to`。是否把它持久化为 Harness 原生 user turn、
Codeg overlay event，或两者组合，需要按 Harness 能力验证，不能为追求统一 UI 破坏原生 Resume。

### 5.2 ACK 与模型处理不是一回事

AgentBus ACK 只表示客户端收到完整字节和附件，不表示模型已经理解或完成任务。Codeg UI 应
分别表示：

```text
mailbox accepted
delivered to Codeg runtime
turn started
turn completed
reply sent
```

如果在 ACK 后、turn 开始前崩溃，request 仍需通过 owed 恢复；notify 则只能标记为已接收。

### 5.3 防止双重投递

Codeg UI 和外部 CLI 可能同时领取同一角色邮箱。实现必须依赖 AgentBus 稳定 msgid、lease 和
attempts 去重，并为 Conversation 保存最近处理游标。不能同时直接注入 turn 又让同一 Session
的 skill 再次处理相同邮件。

## 6. 可选群聊与 AgentBus 的关系

群聊不是协作轨道的前置对象。先实现对已有 Session 的多选发送、转发、比较和状态显示；群聊在
稳定 Session 投递之后作为可选共享时间线。AgentBus 始终只是消息传输，二者即使结合也不互相
拥有：

```text
Group Conversation
├── members: Conversation references
├── shared timeline
└── optional AgentBus project mapping
```

群聊与普通 Session 一起出现在会话列表、Collection、搜索和 Workbench 中。用户点击
群成员可直接打开它的原生私聊，不需要先进入 Team、project 或 role 页面。

群聊可以使用 AgentBus 向外部成员投递；纯 Codeg 成员也可以通过内部运行时直接收发。所有
路径在 UI 中必须呈现一致的来源、目标和投递状态。

群聊候选版本不建立用户可见的 Team 层。一个 Bus project 可以辅助绑定一个群聊，但 project
仍只是外部通信命名空间；多个群聊复用成员的问题留到真实需求出现后再评估。

群聊上下文与成员私聊必须分离：群聊消息先进入共享时间线，再只为显式 `@` 目标生成 Delivery；
成员的其他私聊内容不回流。所有成员拥有查询共享时间线的权限，但未被目标选中的 Session 不会
因此启动或自动摄入消息。新成员不自动读取全部历史，`@all` 也必须显式触发。

AgentBus 私信本身不自动写入群聊。只有携带可信 Room event 关联的 Delivery 及其回复才回写
共享时间线，防止把 A→B 的私密协作泄漏给 C。完整 Room 语义见
[群聊面板与 Session 协作 RFC](./GROUP-CONVERSATION-RFC.zh-CN.md)。

跨 Backend Room 仍由一个 home Codeg Backend 保存公共时间线和成员关系。AgentBus 只持久化正在
传输的 Delivery/回复及其关联 ID；它不复制或接管 Room 账本，也不在 home Backend 离线时自行
生成第二条可写时间线。

现有 `chat_channel` 仍用于 Telegram 等外部渠道，不复用为多 Session 群聊或 AgentBus project。

## 7. Codeg 数据边界

AgentBus 集成不规定最终表名，但 C0-C3 至少需要表达：

```text
bus_connection
- daemon URL
- protocol/version metadata
- credential reference（不是明文 token）

bus_project_mapping
- bus connection
- project name
- optional collection/group-conversation reference

bus_session_binding
- backend_ref
- conversation_id
- project / role
- bus session identity reference
- binding source and verification state

bus_delivery_cursor
- conversation_id
- stable msgid / attempts
- delivery and turn state
```

这些都是提案。编码前必须核对 AgentBus 当时协议，并为 token、Session 凭证和远程连接设计
安全存储。不得把 AgentBus token 写入会跨设备同步、日志或普通导出文件的字段。

## 8. 分阶段实施

### C0：只读控制台

- 配置一个 AgentBus daemon；
- 展示 project、role、绑定、last activity、pending、leased、owed；
- 从绑定跳转到 Codeg Session；
- 不改变 AgentBus 协议和投递行为。

### C1：Operator 消息 UI

- 查看 inbox、owed 和消息线程；
- 用户发信、回复、广播和附件；
- takeover/eject 预览；
- 清楚区分 mailbox 状态与运行时状态。

### C2：Session 绑定与轻量协作动作

- Conversation 与 Bus role 显式绑定；
- Workbench 显示目标 Session 与未处理消息；
- 多选发送、定向转发、结果比较和投递状态；
- 不增加用户可见 Team 或群聊层级。

这里的 Bus 绑定只适用于外部桥接目标。纯 Codeg Session 先由原生路由处理，不为了统一 UI
虚构一个 Bus project。

### C3：受管 Harness turn 投递

- 对 Codeg 活动连接进行结构化 turn 注入；
- 对可 Resume Session 恢复并发送；
- 投递去重、owed 恢复和状态机；
- 不支持的 Harness 保持 mailbox/复制降级。

### C4：可选持久群聊

- 在稳定 Session 身份、投递和多视图同步后启动；
- 群成员引用持久 Session，私聊历史不自动公开；
- 提供仅记录、显式目标、`@all`、共享回复和真实 Delivery 状态；
- 群成员可处于后台，不要求在当前 Workbench 打开；
- 群聊与 Bus project 可以可选映射；
- 不增加用户可见 Team 层级。

### C5：可选 daemon 托管

- Codeg 启停本地 daemon；
- 诊断版本、协议、TLS 和连接；
- 保留独立 CLI、外部 daemon 和远程部署能力。

### C6：可选 Managed CLI Gateway（未来）

- 定义 provider-neutral gateway/session 注册和能力协商；
- 优先实现一个可验证的 provider，不同时包办全部 CLI；
- PTY 输出透明转发，控制面走独立 socket；
- 只在确认 input-ready 后注入，繁忙时进入可见队列；
- 将 terminal request、原生 Session ID 和 AgentBus msgid 关联；
- 完成 tmux/Herdr/Windows ConPTY、人工输入冲突、resize、重连和安全测试；
- Gateway 可由 Codeg 启动，也可独立连接 AgentBus daemon。

## 9. 非目标

- 根据沉默自动回收角色；
- 猜测 Agent 在线、工作或死亡；
- 默认让所有目标自动互相辩论；
- 把所有群聊历史复制进每个成员的原生上下文；
- 要求用户先创建 Team、Membership 或角色模板才能群聊；
- 把可修改 Session 标题或 `project/role` 当成唯一投递 ID；
- 要求全部 Agent 必须由 Codeg 启动；
- 为 UI 方便而取消 AgentBus daemon/CLI；
- 把 AgentBus project、Collection、Folder 和 Workbench 合并为同一对象。
- 第一阶段通过模拟按键“支持所有 CLI”，或把不可靠终端注入伪装成结构化 Harness API。

## 10. 验收要点

1. 用户能在 Codeg 中看清所有 project、role 和真实 Session 绑定；
2. 不查看 Terminal 也能定位 pending、leased、owed 和需要回复的请求；
3. UI 不虚构在线状态，也不把 ACK 显示成“Agent 已完成”；
4. Codeg 受管 Session 可以收到结构化 Bus 消息，并使用原 msgid 回复；
5. 外部 Session 继续通过原 CLI/daemon 通信，不因 Codeg 退出而失效；
6. 同一消息不会被 UI 注入和 Session skill 重复处理；
7. Collection、Workbench 和 AgentBus project 保持独立生命周期；
8. token 和完整 Session 凭证不会进入日志、同步布局或普通导出。
9. 用户无需理解 Team 或 AgentBus ID，即可对几个现有 Session 执行发送、转发和比较。
10. 每条消息只能由一个 Delivery Adapter 获得投递权，直接注入与 AgentBus pull 不会重复处理。
11. 远端 `codeg-server` 已管理的 Session 通过 Codeg API/运行时投递，不因为位于远程而被迫注册
    AgentBus；
12. 跨 Backend Session Address 使用稳定 Server 身份，不把本机连接行 ID、显示名或 URL 当作
    永久身份。

若实施 C4，还要额外验收：纯 Codeg 群聊不要求运行 AgentBus daemon；成员私聊不自动回流；
群聊、Collection 和 AgentBus project 可以关联但保持独立生命周期。
