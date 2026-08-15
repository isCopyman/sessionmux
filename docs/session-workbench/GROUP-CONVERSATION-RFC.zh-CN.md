# Codeg 群聊面板与 Session 协作 RFC

> 状态：Draft  
> 最近调研：2026-08-15  
> 定位：定义多个持久 Session 如何在一个共享面板中交流，以及共享可见、运行时投递和模型上下文之间的边界。  
> 前置条件：稳定的 Session 身份、Resume、Workbench、多视图同步和统一 Delivery Router。

## 1. 决策摘要

1. **群聊是一个可打开的内容面板，不是 Workbench，也不是 Session 的父级。** 它可以像普通
   Session 一样加入 Collection、Workbench、搜索和最近使用，但拥有群组图标和成员栏。
2. **群成员直接引用已有持久 Session。** 产品交互上可以把 Session 看成一个可被 `@` 的 Agent；
   第一版不再要求用户创建 Agent Profile、Team、role slot 或另一套身份。
3. **群聊、私聊和底层投递严格区分。** 在群聊中发布的内容进入共享时间线；在 Session 面板中的
   对话仍是用户与该 Session 的私聊；Session 间的定向私信也不会自动出现在群聊。
4. **共享可见、唤醒对象和模型上下文是三个独立维度。** 一条消息可以对全体成员可见，但只唤醒
   `@` 到的一个或几个 Session；未被唤醒的 Session 不因此消耗 Token。
5. **Workbench 共处不等于群成员关系。** Workbench 可以摆放互不相关的 Session；群成员也可以
   没有打开在任何前台 Workbench 中，仍在后台排队、恢复或运行。
6. **第一版不增加 Team。** 一个群聊已经保存自己的成员集合。只有同一套成员需要反复复用于多个
   群聊时，才评估“保存成员模板”，而不是提前建立新的主导航层。
7. **AgentBus 只负责跨边界传输。** 群聊自己的共享时间线和成员关系由 Codeg 保存；Delivery
   Router 再为每个目标选择 Codeg Runtime、AgentBus mailbox 或未来的 CLI Gateway。

## 2. 为什么群聊不能只理解为“广播”

群聊至少同时回答三个不同问题：

| 维度 | 回答的问题 | 例子 |
|---|---|---|
| 可见性 | 这条内容属于群聊还是私聊，谁以后能查到？ | 群内公开、用户与 A 私聊、A 与 B 私信 |
| 激活/投递 | 这次实际要求谁开始一个 turn？ | 仅记录、`@A`、`@A @B`、`@all` |
| 上下文摄入 | 被激活的 Session 实际拿到哪些内容？ | 当前消息、回复链、置顶背景、最近摘要、完整历史链接 |

把三者合并会产生两种极端：

- 群里每出现一句话就唤醒全部 Agent，造成抢答、循环和 Token 浪费；
- 只把消息秘密投递给被 `@` 的 Agent，其他成员和用户却看不到共同记录，这又不再是群聊。

因此本 RFC 采用“**共享时间线对成员可读，显式目标才触发执行，模型按需获得有界上下文**”的
组合。

## 3. 现有项目提供了哪些证据

### 3.1 Buzz

Buzz 最接近通讯工具语义：Channel、Thread 和 DM 是不同的共享空间，Channel 成员有权读取该
Channel；`buzz-acp` 默认只监听 `@mention`，因此普通群消息不会自动启动 Agent。Agent 被触发后
在同一 Channel 回复，并可通过 CLI 读取近期或完整消息。运行时按 Channel 维护 ACP Session，
Thread/DM 自动上下文还有独立的消息数量上限。

它证明了：**房间可见性不等于每条消息都进入每个模型 turn**。

### 3.2 OpenAgents

OpenAgents 把 Channel 定义为具名事件流，并为每个 Channel 保存参与者。人类界面能看到共享
时间线，但后端在事件上写入 `target_agents`，Connector 只拉取发给当前 Agent 的事件。多成员
Thread 可以用动态路由、主 Agent 星形路由或 Workflow 路由；各 Harness Adapter 通常按 Channel
维护独立原生 Session，恢复失败时再用 Channel recap 和置顶决定重建。

它证明了：**同一群聊中可以只有一个“下一位发言者”，其他成员仍属于群聊但不被执行**；同时也
暴露了隐式 LLM Router 的问题，即用户未必知道普通消息最终去了谁。

### 3.3 Multica

Multica 的 Chat 是用户与单个 Agent 的完全私密对话；多人协作主要发生在 Issue 评论时间线。
`@Agent` 是一次执行触发，不是通知，`@all` 只通知人类成员而不包含 Agent。一个评论可以显式
触发多个 Agent，每个 Agent 得到自己的 Task；Squad 先唤醒 Leader，再由 Leader 在 Issue 中
`@` 成员。Agent 回复、进度和结果都回到同一 Issue 记录。

它证明了：**共享记录、通知人类和启动 Agent 必须使用不同语义**，也证明 Team/Squad 更适合
复用路由关系，不是群聊的必要前提。

### 3.4 OpenTeams

OpenTeams 把 ChatSession 直接设计为共享协作容器。用户或 Agent 的 `@mention` 会启动对应成员，
成员可互相 `@`，结果和状态回到一条共享时间线。它不会在每轮把完整群历史塞入 Prompt，而是把
历史写入 `message.jsonl`，让 Agent 在需要时读取；每个成员还有独立队列、运行状态和外部 Agent
Session ID。系统用 `ChainDepth` 限制 Agent 间连续传播。

它证明了：**真正群聊也不要求全量 Prompt 共享**，但必须保存一个人类可检查的公共账本，并为
Agent 间链式触发设置预算和停止条件。

## 4. 产品对象

### 4.1 Room / Group Conversation

Room 是一段持久共享时间线及其成员引用：

```text
Room：某主题讨论
├── shared timeline
├── member → Session A
├── member → Session B
└── member → Session C
```

Room 可以被打开为 Content Tab，并被放进任意 Pane。删除 Room 只删除群聊关系和共享时间线，
不得删除任何成员 Session 或原生历史。

Room 还可以拥有零或一个主要 Collection，在目录树中以群组图标与普通 Session 并列。创建 Room
时，如果所选成员都属于同一 Collection，UI 可以把该位置作为默认建议，但必须允许确认、修改或
保持未分类。移动 Room 只改变它自己的语义归档位置，不移动成员 Session，也不改变成员各自的
Collection、cwd 或 Runtime。删除 Collection 时，Room 与 Session 一样进入未分类或迁往用户
确认的位置，不能级联删除共享时间线。

### 4.2 Session 作为协作 Actor

第一版用稳定 Session Address 标识成员：同一 Backend 内可以是 `conversation_id`，跨 Backend 时
必须是 `backend_ref + conversation_id`。用户看到的是 Session 的显示名、Harness、Backend/位置、
头像和可选群内身份徽标；底层不使用易变标题作为地址，也不假设多个 Codeg 数据库共用 ID 空间。
`backend_ref` 指向经过认证的稳定 Codeg Backend 身份，不直接复用本机连接表的行 ID、显示名或 URL；
本文字段名仍是提案，编码前必须与当时 Codeg schema 和 Server 握手能力核对。

“Session 就是 Agent”适合作为交互心智，但不改变现有领域含义：

- Session 仍是一段原生可恢复上下文，不是可复制的角色模板；
- Harness、模型、cwd、权限和运行状态仍属于 Session/Execution Context；
- Room 可为该 Session 设置局部昵称或“调研/写作/审查”徽标，但不改 Session 标题；
- 同一 Session 进入多个 Room 时仍是同一个 Actor，而不是自动创建多个副本。

成员名称采用“稳定身份与可读名称分离”的三层表达：

```text
Session Address   系统投递身份，不因改名变化
Session 标题      跨 Room 的默认可读名称
Room 昵称/职责    只在当前 Room 生效，例如“文献调研”或“审稿”
```

这允许同一个 Session 在不同 Room 使用不同的群内身份，类似通讯工具中的群昵称。Room 昵称和职责
都是 `room_member` 的展示与寻址元数据，不是新的 Session、Agent Profile 或全局 Role。编辑器可以
让用户按任一可读名称找到成员，但提交时必须保存解析后的稳定 member/session 引用。

### 4.3 同一 Session 的记忆边界

Room 无权读取成员的私聊历史，但 Session 会记得自己实际接收过的群聊 turn。这与同一个人参加
多个群后仍保留自己的记忆类似：群成员看不到他的私聊，但他本人不会自动失忆。

因此，同一 Session 参加多个 Room 可能产生跨主题记忆。如果场景要求盲评、保密或严格上下文
隔离，应创建独立 Session，或在 Harness 支持时从原 Session Fork 一个参与分支；不能假装同一
原生 Session 同时拥有互不相知的多份记忆。

## 5. 群聊与私聊的行为规则

| 动作 | 共享时间线 | 启动谁 | 其他成员是否获得模型 turn |
|---|---|---|---|
| 用户发布“仅记录”消息 | 显示 | 无 | 否 |
| 用户在群里 `@A` | 显示 | A | 否 |
| 用户在群里 `@A @B` | 显示 | A、B，各自一次 | C 不启动 |
| 用户在群里 `@all` | 显示 | 所有可投递成员，各自一次 | 是，但不自动继续循环 |
| A 在群里普通回复 | 显示 | 无新目标 | 否 |
| A 在群里回复并 `@B` | 显示 | B | C 可读但不启动 |
| 用户点击 A 头像后发消息 | 不显示 | A | 无 |
| A 私信 B | 不显示 | B | C 无权读取 |
| 用户或 Agent 执行“分享到群聊” | 显示一条带来源的引用 | 由显式目标决定 | 不隐式启动 |

凡是从 Room 消息触发的 Agent 回复，默认回到同一个 Room，并保留 `reply_to` 和来源 Session。
若 Agent 认为内容只应发给某一个 Session，必须使用私信动作；不能先公开到 Room 再试图隐藏。

“点名”和“正文提及”必须区分。例如 `@GPT 请你和 Claude 协作` 只启动结构化 mention 指向的 GPT；
正文里的 Claude 不会被语言猜测自动激活。GPT 可以在自己的 turn 中显式调用 `send_message`，选择
在原 Room 公开 `@Claude`，或向 Claude 发送 direct 消息。若用户希望两者立即并行开始，应在发送
前的目标条中明确选择 `@GPT @Claude`。

### 5.1 两种发送范围

产品层不得用一个含义模糊的“Send Message”让系统事后猜测消息属于哪里。用户和 Agent 都只有
两种明确动作：

| 动作 | 保存位置 | 谁能查询 | 默认回复去向 |
|---|---|---|---|
| 发送到 Room | Room 共享时间线 | 当前 Room 全体成员 | 原 Room 与原回复串 |
| 私信 Session | 目标 Session 的定向交互记录 | 用户、发送方和目标执行链 | 原私信来源 |

这里的“私信”首先是**路由与展示范围**，不是对本机所有者隐藏的权限边界。用户作为工作台所有者
仍可审计投递；关键保证是私信不会自动复制到 Room，也不会被其他 Room 成员作为公共历史查询。
人类从 Room 点击成员头像后进入原 Session 私聊；Agent 使用统一 `send_message` 并明确
`scope=direct`。公开交接使用同一工具但明确 `scope=room`，不能先私信再依赖 UI 猜测是否应该
公开。

`targets` 接受一个或多个稳定 Session Address。因此 direct + 多目标表示“分别发送”：系统创建多个
互相隔离的 Delivery，接收者彼此不可见；room + 多目标表示只写入一条公共 Room event，但为每个
目标各创建一次执行。若多个接收者需要共同看到回复，它就是 Room，而不是另一种“多人私聊”。

### 5.2 回复串，而不是完整 Discord 层级

第一版以“群聊 + 私聊”的通讯工具心智为基础，同时给 Room event 保存 `reply_to`。当一条根消息
产生多个 Agent 回复时，UI 可以像 Slack/Discord 一样在右侧展开回复串：

```text
Room 主时间线
└── 根消息：请三位分别审查这个方案   [5 条回复]
    └── Thread drawer：围绕该消息的回复和继续讨论
```

Thread 第一版只是对同一 Room event 图的过滤视图：

- 没有独立成员表、权限、Collection 归属或原生 Session；
- Room 全体成员仍可查询其中内容，但只有被 `@` 的成员会被启动；
- 从 Thread 触发的 Agent 默认获得根消息、当前回复链和有界近期内容，回复仍回到该 Thread；
- 主时间线可以只显示根消息、最新摘要和回复数量，避免多个 Agent 的来回讨论淹没其他话题；
- Thread 变成长支线时，可显式“提升为新 Room”，选择成员并带入根消息、摘要和双向来源链接。

因此不在第一版引入 Discord 的 Server → Channel → Thread 层级。Collection 已负责长期语义分类，
Room 已负责共享成员和时间线，Thread 只解决一条群消息下面的局部阅读问题。

## 6. 消息编辑器与默认路由

群聊编辑器不依赖隐藏规则猜测用户想唤醒谁。输入框上方始终显示一个目标条：

```text
[仅发到群聊，不启动 Agent] [@调研] [@写作] [@审查] [@all]
```

默认规则：

1. 新建顶层消息且没有选择目标时，作为“仅记录”发布，并在发送前明确提示“不会启动 Agent”；
2. 回复某个 Agent 的消息时，默认目标继承为该 Agent，用户可以取消或增加目标；
3. 文本中的有效 `@` 与目标条双向同步；只输入相似名字但未选中实体时不静默路由；
4. `@all` 必须显式选择，并在发送前显示将启动的 Session 数量；
5. 第一版不使用 LLM 猜测“最合适的下一位发言者”，也不设置隐藏的默认 Master；
6. 后续若引入主持模式，必须在 Room 头部显示当前模式、主持 Session 和本轮路由结果。

`@会话名` 是人类交互，不是底层地址。自动补全项应同时显示 Harness、Collection/cwd 和必要的
身份后缀；选择后编辑器保存结构化 mention 及稳定 Session Address。重名必须消歧，重命名不能
让旧消息失去目标，Agent 也不能仅凭一段同名纯文本触发另一个 Session。

Room 内自动补全优先显示当前成员的群内昵称/职责，再显示其 Session 标题；同名时列出 Harness、
Backend 和路径供用户选择，不得静默广播给全部同名成员。若选择的 Session 尚不是 Room 成员，
第一版应提示“加入本群”或改为私信，不能暗中扩张成员关系。Session 私聊中的 `@` 则可以继续从
当前 Workbench、最近联系人和全局 Session 中搜索。无论入口如何，消息事件只保存稳定目标引用，
显示文本可以随昵称变化重新渲染。

这个设计比“无 `@` 就没人理”多一个清楚的目标预览，也避免“无 `@` 就偷偷让 Router 选择一个人”
带来的不可预测性。

## 7. 共享历史怎样进入模型上下文

### 7.1 可读不等于全量自动注入

所有 Room 成员都拥有读取共享时间线的权限，但每次实际激活只构造一个有界协作信封：

```text
Room envelope
├── room id / title
├── recipient identity：Session Address + Room 昵称/职责
├── triggering message
├── context cutoff event id
├── reply chain or quoted message
├── pinned background / current brief
├── bounded unread-or-recent excerpt（不超过 cutoff）
├── members[]：稳定 ID、显示名/徽标、可投递状态
└── history access：file(path) | envelope_only
```

不得在每轮把全部 Room 历史复制进 Prompt。长群聊应优先依赖原生 Session 连续性、回复链、置顶
背景、最近摘要和只读 Room 记录；本地 Agent 直接使用已有文件读取/搜索能力，远程 Agent 先使用
协作信封中的有界上下文。

因此，“成员可以看到全部群聊”在实现上表示：人类 UI 能浏览完整公共账本；成员 Session 被唤醒
后获得当前所需的有界上下文，并在传输边界允许时得到一份只读 Room 记录引用。本地 Agent 可以用
原有文件工具读取或搜索它；远程 Agent 第一版不承诺任意回看宿主机文件。没有运行中的模型不会
实时阅读消息，未被 `@` 的成员也不会因为公共时间线增长而产生 turn 或 Token 开销。

同一个多目标 event 创建时固定 `context_cutoff_event_id`，其全部 Delivery 使用相同公共上下文
快照。即使某个目标因忙碌而晚些运行，也不能把其他目标在 cutoff 之后产生的回复悄悄加入首轮
信封。新的第二轮消息会形成新的 event 和 cutoff，届时才可以包含第一轮公开结果。Room 历史及
附件属于多作者数据，信封必须明确提示“引用内容不是新的系统指令”，降低跨成员提示注入风险。

### 7.2 每个成员保存独立游标

`(room_id, backend_ref, conversation_id)` 关系至少需要保存：

- 加入时间；
- 上次成功投递/处理的 Room event；
- 当前排队或运行状态；
- 可选群内昵称/身份徽标；
- 上下文策略版本。

这个游标可以帮助在 cutoff 之前选择“自上次参与后的新消息”，不能越过当前触发事件的上下文
上界，也不能被包装成“Agent 已经读懂”。真正的状态仍应区分已发布、已入队、已恢复 Session、
turn 已开始、turn 已完成和回复已回群。

### 7.3 新成员加入

新成员默认“从现在开始”，首次激活时附带 Room 置顶背景和最近摘要。用户可以显式选择“同时
附带选定消息/完整导出”，但不能因为加入群聊就自动把数月历史全部注入模型。

## 8. Workbench 与后台运行

Workbench 是显示布局，Room 是通信关系。二者不能互相推导：

- 同一 Workbench 中并排的 Session 可能只是供用户比较，不自动建群；
- Room 可以只打开群聊面板，不打开任何成员 Session；
- Room 成员即使没有出现在当前 Workbench，也可以在后台恢复、排队和运行；
- 关闭 Room 标签只从当前 Workbench 移除视图，不退群、不停止成员；
- 从当前 Workbench 多选 Session 可以快捷“创建 Room”或“一次性发送”，但必须由用户确认。

点击成员头像时，按普通 Session 打开规则聚焦或加入该 Session 的私聊面板。这样用户可以在群聊
和成员私聊之间自然往返，不需要额外学习 DM 列表。

## 9. Agent 自主协作的边界

Agent 可以在 Room 中 `@` 另一个成员，也可以 direct 私信其他 Session；所有 Agent 发起的连续
投递共用同一套停机限制：

- `expects_reply=false` 的消息是通知或最终回复，不产生回复债务，也不自动启动原发送者；
- 一次普通最终回复只写回来源记录，不自动构成下一次 Agent turn；
- 只有 Agent 再次显式调用 `send_message` 并指定目标时，协作链才继续；
- 每条根消息保存跨 direct/room 的最大链深、最大 turn 数和可选 Token/费用预算；
- 拒绝立即回到刚发言者的无内容自循环；
- 同一目标忙碌时进入该 Session 的有序队列，不悄悄创建另一份上下文；
- 用户可以停止单个成员、本条协作链或整个 Room 当前活动；
- Agent 的普通状态、thinking 和工具日志不自动触发其他成员；
- `@all` 只触发一轮，不代表此后所有 Agent 对每条回复都继续响应。

若以后需要 Master/Worker、审查闭环或 Workflow，把它作为 Room 的显式协作模式，而不是所有群聊
的默认行为。

## 10. 投递与 AgentBus

Room 先写入共享事件，再为显式目标生成 Delivery：

```text
Room event
   ├── Delivery → same-backend Codeg runtime
   ├── Delivery → managed remote Codeg server
   ├── Delivery → AgentBus mailbox / cross-backend fallback
   └── Delivery → unavailable/read-only
```

AgentBus 的 `send --to`、broadcast、`reply_to`、mailbox、lease 和 owed 可承担跨 App/主机目标；
但 AgentBus 自己的定向消息不会自动成为 Room 消息。只有带有可信 `room_id/event_id` 的 Room
Delivery 及其回复才回写共享时间线，防止把所有 Bus 私信泄漏到群聊。

每条 Delivery 只能选择一个 adapter。Codeg 已直接注入 turn 后，不能再让同一 Session 从
AgentBus 重复领取。完整传输规则见
[AgentBus 协作子 RFC](./AGENTBUS-COLLABORATION-RFC.zh-CN.md)。

### 10.1 第一版只需要一个发送工具

不要因为 UI 中存在私聊、群聊和分别发送，就给 Agent 暴露多套功能重叠的消息工具。第一版使用
一个 `send_message`，`targets` 自然接受一个或多个稳定 Session Address：

```text
send_message(
  scope: "direct" | "room",
  targets: [{ backend_ref, conversation_id }, ...],
  body: string,
  expects_reply: boolean,
  room_id?: room_id,
  reply_to?: event_id,
  context_refs?: [event_id | file_ref, ...]
)
```

语义保持简单：

- `direct + 一个目标`：普通私信；
- `direct + 多个目标`：分别私发，创建互相隔离的 Delivery；
- `room + 零个目标`：只写公共记录，不启动 Agent；
- `room + 一个或多个目标`：写一条公共消息，并分别启动明确目标；
- UI 中的 `@all` 在确认目标数量后展开为当时可投递成员的 ID 列表；
- `scope=room` 必须有 `room_id`，`scope=direct` 不允许悄悄回写 Room；
- `expects_reply=true` 表示请求，需要目标产生一次定向回复；`false` 表示通知或最终结果；
- `reply_to` 只能引用同 scope 且同 Room 的事件；从 Room 转私聊或把私聊分享到 Room 时使用
  `context_refs`，生成带来源的新事件。

普通 Agent 最终回复由当前 Runtime 自动写回触发它的 Room 或私信来源，不要求模型再次调用
`send_message`，并固定为 `expects_reply=false`，避免重复发言和 A/B 自动互相唤醒。direct 回复
进入原发送者的协作记录和 UI 状态，但第一版不自动启动原发送者。只有需要主动发起下一次私信或
公开交接时才调用工具。Agent 输出中的普通文本 `@名字` 可以显示，但不得仅靠字符串解析启动目标；
人类 UI 和 Agent 上下文都应提供稳定成员 ID。

### 10.2 Room 历史先作为资源，不先拆成三个工具

第一版不增加 `room_context`、`room_read` 和 `room_search` 三个专用工具。Codeg 在触发 turn 时已经
自动注入当前消息、回复链、置顶背景和有界近期内容；绝大多数回复不需要再次查询。

Room 可以实现为追加写事件文件，或者由数据库事件生成只读 JSONL/Markdown 视图。无论底层采用
哪种方式，Agent 都不应直接追加文件；所有写入仍经过 `send_message`，由 Codeg 分配 event ID、
校验作者、原子写入并创建 Delivery。对于本地可访问的 Agent，协作信封可以附上只读 Room 记录
路径，让它使用 Harness 原有的文件读取与文本搜索能力，不重复提供 Room 专用工具。

每个 Room 的只读资源使用稳定 URI/路径；JSONL 每条记录和 Markdown 投影都暴露可引用的 event
ID。附件引用保存事件创建时的快照身份和校验信息，不能只保存会随磁盘变化的模糊路径。

远程、容器或严格沙箱中的 Agent 可能无法读取宿主机路径。第一版先在协作信封中携带足够的有界
上下文；只有真实使用证明需要在 turn 中继续翻阅较早历史时，才增加一个通用的只读资源能力，
例如 `read_resource(uri, cursor?, limit?)`。这个能力应同时服务 Room、附件和其他 Codeg 资源，
而不是先为 Room 建立 context/read/search 三套 API。全文搜索也可以在数据量和真实需求出现后再加。

### 10.3 一次群聊 turn 的协议

```text
1. 先提交不可变 collaboration event，获得 event_id 和 context cutoff
2. 从结构化 mention 解析目标 member_id
3. 为每个目标创建唯一 Delivery(event_id, member_id)
4. Delivery Router 选择且只选择一个 adapter
5. 恢复目标原生 Session，并注入不超过 cutoff 的有界协作信封
6. 流式状态和最终回复关联 delivery_id 回写来源，最终回复不自动激活原发送者
7. Agent 若调用 scope=room 的 send_message，再生成新的公开 event 和下一层 Delivery
```

`event_id + member_id` 是同一轮投递的幂等键；重试只能续用或恢复原 Delivery，不能重复启动 turn。
每个 Agent 触发的消息都必须记录 `caused_by_event_id`、`delivery_id` 和链深，使 UI 能画出回复链，
用户也能停止某一成员、某条协作链或当前 Room 的全部运行。Room event 是公共账本；原生 Session
ID 和 Harness turn ID 是执行证据，两者通过引用关联而不互相冒充。

### 10.4 跨 Backend Room 只有一个事实源

每个 Room 必须有一个 `home_backend_ref`，由它保存有序 collaboration event、成员关系、Thread、
上下文截止点和 Delivery 状态。成员可以位于其他 Codeg Backend，但远端 Backend 只负责恢复自己
管理的 Session、执行 turn，并把带原 event/delivery 引用的结果送回 home Backend。第一版不做
多主 Room 数据库或双向可写时间线同步。

AgentBus 可以在 Backend 暂时不可达时保存跨 Backend Delivery 和回复，却不成为 Room 历史的事实
源。若 home Backend 不可用，UI 应显示 Room 暂时不可写或消息待同步，不能在另一个节点悄悄形成
分叉时间线。最小 Room 阶段可以先限制成员来自同一 Backend；跨 Backend 成员在 R3 开放。

## 11. 拟议数据表达

以下只表达关系，不是已批准表名。实现前必须重新核对当时 Codeg `main`：

```text
collaboration_room
- id / home_backend_ref / title / status / optional primary collection / created_at

collaboration_room_member
- room_id
- backend_ref
- conversation_id
- alias / role_badge
- joined_at
- context_cursor

collaboration_event
- id / optional room_id / author_kind / author_ref
- body / attachments / reply_to
- visibility = direct | room
- expects_reply / context_cutoff_event_id / optional promoted_room_id
- created_at / edited_at / deleted_at

collaboration_delivery
- event_id / backend_ref / conversation_id
- adapter / status / attempts
- native_turn_ref / error
```

兼容要求：

- 不复用 `conversation.parent_id`、`folder.parent_id` 或 `work_task`；
- 不把现有 `chat_channel` 改成 Room，它仍表示 Telegram 等外部渠道；
- 不复制成员的 `conversation` 或 `external_id`；
- Collaboration event 是协作层记录，不能冒充 Harness 原生历史事实源；
- 若某 Harness 必须把协作输入持久化为原生 user turn，应同时保存关联引用并验证 Resume 行为。

## 12. UI 草图

```text
┌──────────────── Room：方案讨论 ────────────────┬──────── 成员 ────────┐
│ 用户：@调研 @审查 请分别检查这个结论           │ ● 调研 / Codex       │
│   ├─ 调研：找到三处证据不足                     │ ◐ 写作 / Claude      │
│   └─ 审查：第二个结论不能由当前实验支持          │ ○ 审查 / Gemini      │
│                                                │                      │
│ 写作：@审查 我修改后请再看这一段                │ 投递：2 完成 / 1 排队 │
│   └─ 审查：修改后逻辑成立                       │                      │
│                                                │                      │
│ [仅记录 ▼] 输入消息……          [发送]           │ [管理成员]           │
└────────────────────────────────────────────────┴──────────────────────┘
```

成员状态必须使用真实可验证的词：可直接投递、正在恢复、已入邮箱、运行中、只读/不可投递。不得
把 Room 成员关系画成虚假的“在线”。

每条公开消息还应显示作者 Session、Harness、回复对象和投递状态；点击作者进入其私聊，点击回复线
聚焦来源事件，点击状态展开本次 Delivery。成员在自己的 Session 面板中处理 Room 消息时，应显示
“来自 Room：名称”的来源标记和返回群聊入口，避免用户误以为这是一条普通私聊。Collection 树、
搜索结果和 Workbench Tab 都用同一个群组图标标识 Room。

多目标根消息显示“已回复 n/m、运行中、失败/不可投递”聚合状态。Thread 被提升后，原根消息显示
“已提升到 Room：名称”并默认折叠，继续回复前提供跳转或明确的“仍在原 Thread 回复”选择。

## 13. 分阶段实施

### R0：已有轻量协作动作

- 多选 Session 发送同一问题；
- 定向转发和并列比较；
- 统一 Delivery 状态；
- 不创建共享时间线。

### R1：最小 Room

- Room 面板、稳定成员引用和共享时间线；
- “仅记录”、`@一个/多个`、显式 `@all`；
- Agent 回复回到 Room；
- 点击头像打开原 Session 私聊；
- Room 与 Workbench 生命周期解耦；
- 受管 Session 的真实排队、运行、失败和停止状态。

### R2：Agent 间协作

- Agent 在 Room 中 `@` 其他成员；
- 回复链、成员上下文游标、置顶背景和稳定的只读历史资源；
- 链深、轮数、预算和循环保护；
- 新成员摘要与历史选择。

### R3：跨边界成员

- 受管远端 Codeg Server 投递与跨 Backend Session Address；
- 缺少直接 federation 时的 AgentBus fallback；
- mailbox、lease、owed、重试和去重；
- 只读或不可 Resume Session 的降级；
- Room event 与外部回复的可信关联。

### R4：仅在真实需求出现后

- 保存/复用成员模板；
- 显式主持人或自动路由模式；
- Workflow、共享黑板或项目记忆；
- 更复杂权限和多用户协作。

## 14. 验收标准

1. 用户能从若干现有 Session 创建 Room，不需要复制会话或创建 Team；
2. Room 能归入一个 Collection；移动或删除该归档位置不改变成员及成员自己的 Collection；
3. 群里 `@A` 后，所有人能看到消息，但只有 A 被执行；
4. `@all` 的目标数量和费用风险在发送前可见；
5. A 的 Room 回复回到 Room，A 的其他私聊不会泄漏；
6. 成员被唤醒后获得当前消息、回复链和有界近期上下文；本地可访问时附带只读 Room 记录引用，
   无需每轮全量注入；
7. Agent 只有通过稳定 member ID 的结构化交接才能启动另一成员，普通文本不会误触发；
8. 未打开在 Workbench 的成员仍可正确排队或显示不可投递原因；
9. 关闭/删除 Room 不关闭、归档或删除成员 Session；
10. 同一 Session 的多个视图不会产生重复 turn 或双重流式输出；
11. AgentBus 与 Codeg Runtime 不会对同一 Delivery 双重投递；
12. 群历史可以完整检查，但长历史不会每轮全量注入模型；
13. 同一条多目标消息的首轮 Delivery 使用同一个上下文截止点，较晚启动的目标不会看到其他目标
    在本轮稍后产生的回复；
14. direct 和 Room 的最终回复都不会自动重新唤醒原发送者；只有新的显式 `send_message` 才延长
    协作链，且二者共用链深、turn 数、预算和停止限制；
15. Room 历史具有稳定的只读 URI/路径和 event ID；本地 Agent 可用已有文件工具读取，远程目标
    无法访问时获得明确的有界信封，而不是无效宿主机路径；
16. 附件保留事件创建时的快照身份、来源与校验信息，远程不可访问状态不会被伪装成已送达内容；
17. Thread 提升为 Room 后，原回复串保留跳转标记并默认折叠，不产生两条无提示的并行主线；
18. 受管远端 Codeg Session 与本地 Session 使用同一 `@` 和 Room 交互；仅跨 Backend federation
    缺失或目标未托管时才使用 AgentBus fallback；
19. 自动协作达到链深、预算或用户停止条件后可靠终止。

## 15. 本轮调研依据

- Buzz：[`buzz-acp` 工作方式与 Channel 订阅](https://github.com/block/buzz/blob/1f4c69eccf012dc58737e9265498397215c706c5/crates/buzz-acp/README.md)、
  [Channel/DM/Thread 与 per-channel queue 架构](https://github.com/block/buzz/blob/1f4c69eccf012dc58737e9265498397215c706c5/ARCHITECTURE.md)。
- Multica：[Chat 私聊与 multi-turn context](https://github.com/multica-ai/multica/blob/2c0912b6ec764b373d44eeea1e80f0d9f11ab417/apps/docs/content/docs/chat.mdx)、
  [Agent mention 路由](https://github.com/multica-ai/multica/blob/2c0912b6ec764b373d44eeea1e80f0d9f11ab417/apps/docs/content/docs/mentioning-agents.mdx)、
  [Squad Leader 协作](https://github.com/multica-ai/multica/blob/2c0912b6ec764b373d44eeea1e80f0d9f11ab417/apps/docs/content/docs/squads.mdx)。
- OpenAgents：[Channel/participant 数据模型](https://github.com/openagents-org/openagents/blob/2978f985e5ec9560194be9eb44306941984377bd/workspace/backend/app/models.py)、
  [动态/主 Agent 路由](https://github.com/openagents-org/openagents/blob/2978f985e5ec9560194be9eb44306941984377bd/workspace/backend/app/mods/workspace_mod.py)、
  [Connector 的 `target_agents` 过滤](https://github.com/openagents-org/openagents/blob/2978f985e5ec9560194be9eb44306941984377bd/packages/agent-connector/src/workspace-client.js)。
- OpenTeams：[共享 ChatSession 与按需历史](https://github.com/openteams-lab/openteams/blob/bcdd7ea0a984722de7c967a6890e7e78e9f8fb25/docs/core-features/chat-session-overview.mdx)、
  [`@mention` 到 run record](https://github.com/openteams-lab/openteams/blob/bcdd7ea0a984722de7c967a6890e7e78e9f8fb25/docs/articles/from-mention-to-run-records.mdx)。
