# Session Workbench 领域模型

> 状态：Draft  
> 定位：记录从长期多 Harness 使用中提炼出的稳定概念，不直接规定 Codeg 数据库字段。  
> 实现约束：所有映射必须遵守[兼容性政策](./README.zh-CN.md#兼容性政策)。

> 普通使用者请阅读[产品需求与使用场景](./PRODUCT-SPEC.zh-CN.md)；本文是设计参考。

## 1. 产品定义

Session Workbench 不是另一个 Agent-first 或 Issue-first 的多智能体平台，而是：

> 一个以原生 Session 为事实源，支持多 Harness、自由 Collection 和持久 Workbench 的工作环境。

它首先解决长期 Session 的发现、恢复、分类和工作现场恢复；自动编排、项目知识和复杂群聊
建立在这些能力之上，而不是反过来成为前置条件。

## 2. 真实用户模式

系统需要同时容纳以下行为：

1. 一个本地路径下存在很多长期 Session，每个通常围绕一个主题；
2. 一个长 Session 可能逐渐覆盖多个话题；
3. 几个 Session 围绕同一主题分工，但彼此不需要通信；
4. 几个持久 Session 偶尔需要审查、转发、广播或比较；
5. 一个 Session 偶尔向另一个 Session 发起一次临时咨询；
6. 同一 Session 只有一个主要归档位置，但可以同时出现在多个工作台、快速视图和群聊中；
7. 用户多数时候仍会逐个查看 Session，多 Agent 自动协作只是部分场景。

因此，“属于同一主题”“当前摆在同一个桌面”和“需要持续通信”必须由不同对象表达。

## 3. 核心对象

产品界面只要求用户理解三个核心对象：

| 概念 | 回答的问题 | 定位 |
|---|---|---|
| Session | 哪一段真实、可恢复的连续上下文？ | 第一等核心资产 |
| Collection | 以后到哪里找到它？ | 可嵌套、唯一主要位置的人类分类目录 |
| Workbench | 我现在把哪些内容怎样摆在一起？ | 可命名、可保存和切换的逻辑工作台 |

实现仍需要若干支撑概念，但不把它们提升为用户必学层级：

| 支撑概念 | 作用 |
|---|---|
| Execution Context | 保存 cwd、worktree、主机、权限等实际运行边界 |
| Library query state | 保存列表的范围、筛选、排序和分组，不拥有 Session |
| Workbench Tab / Pane / Content Tab | 表达工作台和工作台内部的显示结构 |
| App Window | 操作系统物理窗口，不是归档或项目层 |
| View Instance | 同一 Session 在某个窗口和 Pane 中的局部显示 |

Project、Topic、Task、Decision、群聊和 Team 都是可由基础对象派生或后置的概念，不进入第一
阶段的主导航和必学模型。

## 4. Project、Topic 与 Collection

### 4.1 使用一个通用容器类型

`Project`、`Subproject`、`Theme`、`Chapter`、`Workstream` 和 `Topic` 都表示人类
对长期内容的语义组织。第一阶段不必为它们分别创建实体，可以统一表示为可嵌套的
Collection，由用户通过名称、图标或可选标签表达含义。

```text
工作资料
└── 项目 A
    ├── 资料调研
    ├── 实验分析
    └── 写作讨论
```

只有当最上层对象需要独立权限、同步、归档或生命周期边界时，才考虑把 Project 提升为
独立实体。在此之前，Project 可以只是顶层 Collection。

### 4.2 Collection 只负责语义归类

Collection 回答“以后到哪里找到这段会话”，不决定 cwd、Git 分支、运行权限和 Agent
生命周期。

一个 Session 可以拥有：

- 零或一个主要 Collection，用作唯一归档位置；
- 标签、收藏和归档状态；
- Quick Access、最近使用等派生视图；
- 多个 Workbench 和群聊引用。

同一 Session 在 Workbench、群聊、收藏、最近和搜索等位置出现时只创建引用，不复制会话正文或
原生历史，也不形成第二个 Collection 归属。拖到另一个 Collection 表示移动。若未来确有稳定
需求，可增加显式“快捷方式”，但快捷方式必须与主要位置清楚区分。

### 4.3 文件系统隐喻

```text
Project / Library  ≈ 磁盘或资料库
Collection / Topic ≈ 文件夹
Session            ≈ 文件
Quick Access / Workbench reference ≈ 快速访问或打开方式
Group Conversation ≈ 群聊
```

Collection 可以多层嵌套，前端像文件树一样浏览。Theme、Project、Chapter 和 Topic 是用户
赋予某层 Collection 的语义，不是额外层级。默认的一对一归档关系避免同一 Session 在树中
重复出现；复杂联系由标签、搜索、工作台、群聊和属性面板表达。

## 5. Session

### 5.1 Session 是事实源

Session 是有连续上下文、可以恢复的原生 Harness 会话，而不是 Agent 配置，也不是一次
Issue 运行。产品层希望掌握的信息包括：

```text
Session
├── local display name
├── harness and native session identity
├── model / thinking / permissions
├── execution context
├── optional primary collection
├── lineage and typed relationships
├── status / last activity
└── session-scoped UI resources
```

这些是概念属性，不代表 Codeg 必须增加同名字段。Codeg 当前映射见文档索引中的兼容性表。

### 5.2 显示名与运行配置分离

扁平客户端经常迫使用户把主题、任务和模型都塞进标题。理想系统应分别保存语义：

```text
显示名：资料调研
Collection：主题 A / 综述
Harness：Codex
角色：Researcher
运行目录：项目根目录
```

界面可以组合显示，但移动 Collection、切换角色或更新模型不应要求反复重命名原生 Session。

### 5.3 原生历史不复制

原生 Codex、Claude Code、OpenCode 等 Session Store 继续作为对话事实源。管理层保存索引、
别名、关系和 UI 状态。删除 Collection、Workbench 或群聊不得删除 Session；卸载管理层
也不应导致原生会话丢失。

### 5.4 生命周期动作相互独立

View、Workbench、Collection、Runtime 和 Session 不是同一个生命周期：

- 关闭 Content Tab 只删除当前 View Instance；
- 关闭顶部 Workbench Tab 只从当前 App Window 卸载并保存工作台；
- 删除 Workbench 只删除布局和 View 引用；
- 删除 Collection 只解除分类，成员进入未分类或迁移到用户指定位置；
- 归档只改变日常可见性；
- 停止 Runtime 必须是显式运行控制；
- 删除 Session 是单独的危险操作，并按 Harness 能力说明是否影响原生历史。

这些动作不得互相暗含或级联。最近关闭属于可恢复的 UI 历史，不是新的 Session 副本。

## 6. Execution Context 与 Workspace 术语

`Workspace` 容易同时表示路径、项目和界面布局，应避免继续用它承担多种含义。

技术运行边界统一称为 Execution Context：

```text
Execution Context
├── cwd
├── worktree
├── local / WSL / SSH host
├── permissions and environment
└── runtime lifecycle
```

用户界面的工作现场称为 Workbench。理想关系是：

```text
用户看到：Workbench → Session
后台保存：Session → Execution Context → cwd
```

把 Session 加入 Collection、Workbench 或群聊不得改变 Execution Context。

## 7. Workbench、工作台标签与物理窗口

### 7.1 Workbench 保存问题现场

Session 保存一个 Agent 的微观工作现场；Workbench 保存用户处理整个问题时的宏观现场。

```text
Workbench：主题 A 调研
├── Session：资料调查
├── Session：结构讨论
├── Session：审查
├── 分屏布局和活动 Tab
├── 焦点、面板宽度和资源查看状态
└── 工作台公共文件、网页和笔记
```

Workbench 应能混放不同 Folder、cwd、worktree、Harness，必要时也能引用不同顶层
Collection 的 Session。

### 7.2 Collection 与 Workbench 不形成固定父子层级

- Collection 回答“语义上属于哪里”；
- Workbench 回答“现在把什么摆在一起”。

同一主题可以有多套 Workbench；一个 Workbench 也可以临时引用多个主题的 Session。

### 7.3 工作台标签与内容标签

借鉴 Herdr“一个项目容器下有多套命名布局”的优点，Codeg 应允许一个系统窗口同时打开多套
Workbench：

```text
App Window
├── Workbench Tab：资料调研
│   └── Pane layout
│       ├── Content Tab：Session A
│       └── Content Tab：文件 / 网页
└── Workbench Tab：结果审查
    └── Pane layout
        ├── Content Tab：Session B
        └── Content Tab：Session A
```

侧栏列出全部已保存 Workbench，顶部标签只表示当前系统窗口打开了哪些 Workbench。产品语言
不把这两层都简称为 Tab：顶层叫“工作台”，Pane 内叫“内容标签”。Herdr 的 Tab 大致对应这里的
Workbench；Orca 的 TabGroup 更接近这里的 Pane/内容标签组；Paseo 的 Workspace 则把运行边界
和一套工作台捆在一起，不能直接照搬其名称。

### 7.4 物理 App Window

逻辑 Workbench 与操作系统窗口是不同对象：一个物理窗口可以同时打开并切换多个 Workbench，
多个物理窗口也可以同时显示不同 Workbench。为避免两处同时修改同一布局，一个 Workbench
在同一设备上默认只挂载到一个 App Window；跨窗口操作是“移动”，需要两套布局则执行“复制
Workbench”。

同一 Session 可以通过不同 Workbench 同时出现在多个窗口。它仍对应一个 Conversation 和一个
活动运行时，消息、发送、流式输出、工具调用、审批、状态和用量向所有 View Instance 广播；
Pane 布局、焦点、滚动位置和选区属于各视图，不应互相覆盖。

同一设备上的未发送草稿按 Session 共享，避免两个窗口产生两份互不知情的输入。一个时刻只有
一个 View Instance 持有输入编辑权，另一个视图可观察草稿或显式接管；后台对 send/cancel 使用
同一 Session 队列和幂等请求 ID 串行化。跨设备草稿同步可以后置，但已发送消息和运行时事件的
同步属于正确性要求。

### 7.5 资源作用域

后台需要区分资源归属，但不应把完整作用域模型暴露给用户。用户只看到两种行为：

| 用户行为 | 语义 |
|---|---|
| 跟随 Session | 激活对应 Session 时显示，随 Session 切换和恢复 |
| 固定到 Workbench | 在当前工作台保持，不随活动 Session 消失 |

Agent 主动打开的资源、从 Session 消息或工具调用打开的资源，默认跟随该 Session。用户点击
图钉后，它成为 Workbench 的公共参考。应用级设置、导入器和 Git 操作窗口可以在后台保持
Global/utility 作用域，但不作为普通资源归属选项。

资源归属和资源本身应分开：同一文件或 URL 可以被 Session 与 Workbench 同时引用，不复制
文件内容。Session 保存资源关联，Workbench 保存当前如何摆放、选择和固定这些资源。

Git 文件树、Changes 和 Log 本质上属于 Execution Context；界面默认根据活动 Session 的 cwd
切换。某个具体 Diff、文件或预览一旦由 Session 打开，则按上述跟随/固定规则处理。不能仅凭
Folder 推断所有资源都应该对全部 Session 永久可见。

### 7.6 Active、Scope 与 Focus

Active 表示当前输入目标，Scope 表示列表查询范围，Focus 表示临时视觉强调。它们不形成新的
Session、Workbench 或归属关系：

- Active Workbench：当前窗口选中的顶层工作台；
- Active Session：当前接收输入、上下文资源跟随的 Session；
- Current Workbench Scope：Library 只查询当前 Workbench 的 Session；
- Focus Session：暂时放大某个 View Instance，保留并隐藏其他 Pane。

Focus Session 属于 App Window 临时 UI。退出后必须原样恢复布局，不关闭 Session 或中断
Runtime。收起侧栏或进入 Zen Mode 是普通显示偏好，不另建 Focus Workbench 概念。第一阶段不要求
重启后恢复 Focus，也不把 Focus 状态跨设备同步。

## 8. 协作与通信（后置）

普通 Collection 表示相关性，不产生通信：

```text
主题 A
├── 调研 Session
├── 写作 Session
└── 审查 Session
```

第一步不创建任何新对象，用户在 Workbench 中分别阅读、发送和比较即可。需要重复投递时，可先
提供多选发送、定向转发、结果比较和投递状态。确实需要保存共同时间线时，再创建群聊：

```text
群聊：主题 A 讨论
├── 调研 Session
├── 写作 Session
└── 审稿 Session
```

群聊可与普通 Session 同样被打开、搜索并加入 Collection 和 Workbench，只用群组图标区分。
它是一个内容面板和共享事件流，不是 Workbench、Collection 或 Session 的父级。点击群成员即可
回到它的原生私聊。

群聊只需要呈现：

```text
members
shared messages or routing log
@member / @all
delivery status
```

判断规则是：

> 以后一起找到，用 Collection；眼前一起摆放，用 Workbench；需要成员共同查看一段持久时间线，
> 才创建群聊。

群聊不向用户暴露 Team、role slot、发现作用域或关系图。一个 Session 可以在多个群聊中出现，
后台保存引用即可；用户不需要维护多对多网络。若同一 Session 参与多个群聊，它仍是同一个原生
上下文，会记得自己实际接收过的群聊 turn；严格盲评或隔离必须使用不同 Session 或显式 Fork。

成员可以拥有一个群内昵称或简单身份徽标，但它只是当前群聊中的显示和 `@` 寻址信息，不是
Session 的固有属性，也不得拼进 Session 标题。底层投递使用稳定 ID；会话名、群内昵称和
AgentBus `project/role` 都不能作为唯一身份。

群聊保存自己的共享时间线，但不复制所有成员的原生私聊历史。群聊需要把三个维度分开：

| 维度 | 含义 |
|---|---|
| Visibility | 内容属于群聊还是私聊，哪些成员可以查询 |
| Activation | 本条消息实际启动哪个 Session：无、一个、多个或显式 `@all` |
| Context ingestion | 被启动的 Session 本轮获得当前消息、回复链、置顶背景和多少近期历史 |

具体规则：

- 所有发布到群聊的消息进入共享时间线，对成员可查询；
- 只有显式目标进入 Harness turn，普通消息可以只是群内记录；
- 针对这条群聊消息的回复显示回群聊；
- 该 Session 的其他私聊内容不自动回流群聊；
- Agent 在群里 `@` 另一成员时公开显示交接，但只启动目标；
- 新成员不自动吞入全部历史，首次启动时可获得摘要、置顶材料和选定消息；
- 广播必须显式触发，避免所有 Agent 默认抢答和无意 Token 消耗。

“成员可以查看”不表示“模型每轮自动摄入”。长群聊提供稳定的只读 Room 记录；本地 Agent 使用
Harness 已有的文件读取与搜索能力，远程 Agent 先依赖有界协作信封。自动上下文只保留有界窗口、
回复链和成员自己的处理游标，第一版不另造 Room 专用的 context/read/search 工具组。

如果以后反复出现“同一套成员和角色需要用于多个群聊”的真实需求，再评估把成员预设保存为
Team。Team 在此之前只是未来优化，不进入主导航或用户必学概念。

Codeg 受管成员应优先使用稳定 Session Address 和已有运行时直接路由；同一 Backend 内使用
Conversation ID，跨 Backend 使用 `backend_ref + conversation_id`，不要求另外创建 AgentBus
project/role。远程 `codeg-server` 中的 Session 仍是受管成员，不因为网络位置走 mailbox。
AgentBus 保留为未托管 App、缺少直接 federation 的跨 Backend 场景及离线持久邮箱的传输桥；
Codeg MCP 或现有 delegation 也可以承担特定路径。无论使用哪种传输，它都不应拥有 Session、
Collection 或 Workbench 的生命周期。

产品层只表达“向哪个 Session/群成员发送”，投递层再选择 Codeg Runtime、AgentBus mailbox 或
未来受管 CLI Gateway。`wait` 是不可控外部 Session 的合作式降级，不是 Codeg 内部 Session 的
身份或团队状态。未来 Gateway 即使使用 PTY/tmux 注入，也只是 Delivery Adapter，不成为新的
Session 事实源。

群成员关系独立于 Workbench 可见性。Session 没有打开在前台，也可以作为成员排队、恢复和运行；
同一 Workbench 中并排显示的 Session 也不会自动加入任何群聊。完整消息语义、默认编辑器目标和
链式协作限制见[群聊面板与 Session 协作 RFC](./GROUP-CONVERSATION-RFC.zh-CN.md)。

## 9. Topic、Issue、Task 与 Decision

### 9.1 Topic 不等于 Task

- Topic：长期演进的问题领域，可能持续数月，没有明确完成时刻；
- Task/Issue：有输入、负责人、状态和完成条件的一次行动；
- Session：围绕 Topic 或 Task 展开的连续上下文。

合理关系是：

```text
Collection / Topic
├── Sessions
├── Files / Artifacts
├── optional Tasks
├── optional group conversations
└── optional Decisions
```

长期讨论不得为了适应任务看板而被强制包装成 Task。

### 9.2 项目记忆以后再做

Session 是原始证据，但不适合作为唯一工作记忆。未来的 Decision 或 Memory 必须记录来源、
状态和版本，不能让 Agent 随意覆盖一份无来源的 `memory.md`。

```text
Decision B
状态：current
supersedes：Decision A
来源：Session X、结果 Y
记录时间：...
```

Decision、Summary 和 Memory 都是可重新生成、可审计的派生资产，不能覆盖原始 Session。

## 10. 话题变化、恢复与 Fork

检测到话题变化时不得立即自动 Fork。默认采用渐进策略：

```text
短暂插曲
→ 只添加软 Topic 标记，不切 Session

持续发展的已有主题
→ 建议恢复对应 Session

持续发展的新主题
→ 新建 Session + 精简交接摘要

同一问题的替代路线
→ 才执行真正 Fork
```

管理层未来可以记录父 Session、分叉时点、继承内容、分支目的和 Harness 变化。跨 Harness
通常无法复制原生会话，应创建新 Session，并附带可审计的 handoff 包和管理层关系。

这里还要区分三个容易混淆的动作：

- 当前末端 Fork：复制 Session 当前完整上下文；
- 历史分叉：从某条旧消息创建新 Session，原 Session 保留；
- 文件恢复：把工作目录恢复到检查点，可能影响当前文件。

编辑旧消息默认属于“历史分叉”，不得原地改写或截断原 Session。对话分叉也不得默认恢复
文件；文件恢复必须独立展示影响并确认。具体 provider、ACP 边界和 JSONL 安全规则见
[Session 历史能力子 RFC](./SESSION-HISTORY-CAPABILITIES-RFC.zh-CN.md)。

为避免分支爆炸，自动路由只能提供可撤销建议。历史 Session、临时 Topic 和同一主题的不同
epoch 应折叠显示，不应全部占据主侧栏。

## 11. Session Library 与快速访问

```text
Session Library
├── Quick Access
│   ├── Pinned
│   ├── Running
│   ├── Waiting for me
│   ├── Recent
│   └── Recently closed
├── Collections
├── All Sessions
├── Uncategorized
└── Archive
```

Quick Access 是查询或恢复视图，不是存储位置。未来若增加 Suggested recovery，它必须由用户
选择恢复、引用或忽略，不能自动 Resume 或 Fork。

Library View 与 View Instance 是两个不同概念：前者查询“列表里显示哪些 Session”，后者表示
“某个 Session 正在哪个窗口和 Pane 中渲染”。Library View 由四部分组成：

```text
Library query state
├── Scope：全部 / 当前 Workbench / 显式选中的 Collection 子树
├── Filters：状态 / Harness / Execution Context / 标签 / 活动时间
├── Sort：最近活动 / 标题 / 手工顺序
└── Grouping：无 / Collection / Harness / 状态 / Execution Context
```

Scope 决定候选集合，Filters 缩小候选集合，Sort 和 Grouping 只改变呈现。它们都不修改 Session
的主要 Collection、Workbench 引用、Execution Context、归档状态或 Runtime。

第一阶段只提供两个始终可见的一键 Scope：当前 Workbench 和全部 Session。点击 Collection 树会
把 Scope 切换到该 Collection 子树，并显示可清除的 Collection 范围标签，从而避免“当前
Collection”在没有明确选择时产生歧义。运行中、等待处理、收藏、最近、未分类、最近关闭和归档
属于系统视图。Harness、状态、运行目录和活动时间属于临时筛选。临时筛选默认按 App Window
保存，使两个物理窗口可以显示不同列表；它们不是跨设备共享的领域事实。

命名 Saved View 是后续可选能力，本质仍是用户保存的一组查询条件，不是 Collection，也不拥有
Session。系统不应把每次临时筛选自动保存成新视图，以免出现与 Session 分支相同的管理负担。

过滤后的列表不得掩盖运行事实：已打开但不匹配的 Session 仍留在 Pane，运行中与等待处理的
全局计数仍然可见，并提供一键清除筛选。这样“只显示某些会话”不会变成丢任务的来源。

Session Center 是 Library 的完整管理入口，不是新的领域容器。它可以组合 Codeg 元数据索引和
ctx 等可选 History Search Provider，但 Search Hit 只是派生结果：不能拥有 Session，也不能成为
Collection、Workbench 或 Runtime 的事实源。管理页中的只读 Preview 同样不是 View Instance；
只有用户执行“打开”后，才在活动 Workbench 中创建或聚焦 Session View。

## 12. 前端层级与后台关系

核心原则是：

> 前端以层级浏览，后台以关系组织。

```text
Collection ──contains one primary location──> Session
Workbench  ──lays out────> Session
Library View ──queries────> Session
View       ──renders──────> Session
Group chat ──routes──────> Session
Session    ──runs in─────> Execution Context
Session    ──related to──> Session
Task       ──uses────────> Session
Decision   ──derived from> Session / Artifact
```

复杂网络不应通过无限增加目录层级表达。默认界面显示主要位置、关联项和反向链接；仅在确实
需要时提供关系图视图。

## 13. 产品优先级

长期产品优先级是：

1. 统一发现、导入、重命名和恢复不同 Harness 的原生 Session；
2. 一致且安全的打开、关闭、停止、归档与删除语义；
3. 唯一主要位置的 Collection，以及范围清楚、操作轻量的 Session Library、搜索和筛选；
4. 可命名、可切换、可恢复的 Workbench、顶层工作台标签和分屏布局；
5. 大量会话管理、物理多窗口与同一 Session 多视图同步；
6. Fork 谱系及其他 Session 历史关系；
7. 多选发送、转发、比较和 AgentBus 投递，再增加可选群聊面板与受限的 Agent 间交接；
8. Decision、项目记忆、Task 关联和自动路由；
9. 只有真实复用需求出现后，才评估 Team、角色模板和更复杂协作。

这表示依赖和价值顺序，不要求 Codeg 的工程里程碑严格按同一顺序交付。某些局部改动，
例如拖边吸附，风险低且可独立交付，可以先于完整 Collection 数据模型实现。工程顺序以
[Session Workbench 主 RFC](./SESSION-WORKBENCH-RFC.zh-CN.md#10-实施顺序)为准。

## 14. 第一阶段边界

第一阶段聚焦：

- 可靠的原生 Session 索引、同步与 Resume；
- 一致的 Session 打开与生命周期语义；
- 自由 Collection、系统视图和轻量搜索筛选；
- 命名 Workbench、分屏布局和快速切换；
- 工作台恢复与必要的数据迁移基础。

第一阶段不要求：

- 自动识别并切换 Topic；
- 自动 Fork；
- 完整 Chatroom 协议；
- Workflow、Autopilot 或自动主持人；
- 项目共识数据库或知识图谱；
- 把聊天正文做成无限画布；
- 用新模型替换 Codeg 已有 Folder、Task、Channel 或 Agent 能力。
