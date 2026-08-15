# Codeg Session Workbench 需求与落地 RFC

> 状态：Draft  
> 日期：2026-08-15  
> 第一实现载体：Codeg 0.25.0 当前基线及后续版本  
> 核心原则：以 Session 为事实源，以 Collection 保存唯一语义归档位置，以 Workbench 保存工作现场，以 App Window 承载多套工作台，以 Folder/Execution Context 承担实际运行边界。

> 如果只关心最终实现效果，请阅读
> [产品需求与使用场景](./PRODUCT-SPEC.zh-CN.md)。本文主要供实现和维护时查阅。

本文是 Codeg Session Workbench 主 RFC，概念定义以
[领域模型](./DOMAIN-MODEL.zh-CN.md)为准，文档入口和兼容性政策见
[Session Workbench 文档索引](./README.zh-CN.md)。本文出现的新增表、字段和类型均为
**提案**，除非明确标注“现有实现”，不得当作当前代码事实。

Workbench Tab、App Window、View Instance 与 Session Runtime 的详细状态边界见
[Workbench 层级、多窗口与 Session 多视图同步子 RFC](./WORKBENCH-LAYOUT-SYNC-RFC.zh-CN.md)。

## 1. 决策摘要

第一版直接在 Codeg 上实现，不单独新建产品仓库。

Codeg 已具备多 Harness、原生会话导入与恢复、SQLite 会话索引、跨目录 Tab、嵌套分屏树、
Web/桌面双运行时、多 Agent 委托，以及一个 Session 被其他客户端附着时的 viewer/snapshot/
stream 广播。当前缺口主要集中在会话的长期组织、命名工作现场、视图状态分区以及分屏交互，
不需要重写 Harness 或会话运行层。

本 RFC 保持产品概念与 Codeg 实现解耦。若未来满足下列任一条件，再考虑抽成独立项目：

1. 需要同时服务 Codeg、Paseo、Codex Desktop 等多个前端；
2. 需要把 Workbench、Collection 和 Session 路由做成独立后台服务；
3. Codeg 上游无法接受必要的数据模型或交互改动；
4. 实现过程中发现必须替换大部分会话存储、运行和同步机制。

在这些条件出现前，新建项目只会重新承担多 Harness、会话恢复、流式渲染、权限、文件预览和跨平台打包等已经解决的问题。

## 2. 背景与问题

重度使用多个 Agent Harness 时，同一个长期主题往往散落在不同应用中：

- Codex Session 负责调查或实现；
- Claude Code Session 负责推敲和写作；
- Grok、Gemini 或 OpenCode Session 负责补充意见；
- 每个应用都要重复命名项目和会话；
- 恢复工作时需要重新打开多个应用并手动还原布局；
- 文件路径只能说明 Session 在哪里运行，不能说明它在讨论什么。

Codeg 已经统一了会话入口，但当前界面仍主要围绕物理 Folder 和一套全局分屏布局组织。随着 Session 增多，会出现以下问题：

1. 物理路径无法表达主题、章节、任务阶段等人类语义；
2. 同一路径下可能有多个完全不同的主题；
3. 同一主题也可能包含不同路径、不同 Harness 的 Session；
4. 当前布局不能保存成多套可命名、可切换的工作现场；
5. 分屏需要先手动创建，再把 Tab 拖入分组，缺少 IDE 式边缘吸附；
6. 外部 Harness 中发生的重命名和活动必须可靠同步，否则统一索引失去意义；
7. 群聊是偶发协作方式，不应反过来成为全部 Session 的基础组织结构。
8. 现有 AgentBus 能联系未托管 App、跨 Backend 和远程主机，但 project、role、Session 绑定、
   pending/owed 和消息关系主要靠 CLI 与人脑管理，缺少 Codeg 内的统一控制面。

## 3. 概念模型

### 3.1 Session

Session 是系统的核心事实，也是最原始的知识资产。

它对应一个可恢复的原生会话，保留 Harness 类型、原生 Session ID、标题、消息历史、运行目录、模型和能力信息。Collection、Workbench、App Window 或群聊被删除时，不得删除 Session。

### 3.2 Execution Context

Execution Context 是 Session 的运行边界，第一版不要求创建同名数据库表。

在 Codeg 中，它主要由现有的 `folderId`、`workingDir`、Folder、Harness 设置和可能的 worktree
共同表达。第一阶段一个 Session 必须且只能解析到一个主 Folder/cwd；不新增 `root_paths`、
`additional_directories` 或多根 Project。用户界面不必频繁暴露“Workspace”概念，但后端仍利用
这个唯一执行位置处理：

- cwd 和文件访问范围；
- Git、终端和 Diff 上下文；
- Agent 启动与恢复；
- 权限和生命周期。

组织关系不得修改 Execution Context。把 Session 拖入某个 Collection 或 Workbench，不应改变它的 cwd、Folder 或 worktree。

### 3.3 Collection

Collection 是面向人的长期语义分类，类似文件系统目录，但不对应磁盘路径。

示例：

```text
研究主题 A
├── 资料调查
├── 写作与措辞
└── 逻辑审查

实验主题 B
├── 设计
└── 结果解释
```

Collection 负责回答“这个 Session 或 Workbench 属于什么主题”，不负责回答“它在哪里运行”。
Session 和 Workbench 各自最多有一个主要 Collection；Workbench 的位置不约束其中 Session 的
Collection、Folder 或 Harness。

侧栏保留轻量的当前/最近 Workbench 快捷区，下面的日常浏览默认使用 `Execution Locations`；
`Collections` 通过视图菜单或 Session Center 进入。前者是唯一语义归属树，可同时容纳 Session
与 Workbench；后者沿用 Codeg 原 Folder/cwd、Git 仓库和 worktree 树，并保留从节点新建、导入
和设置默认 Agent 的能力。不得删除路径树，也不得让 Collection 操作修改 `folder_id`。路径树可选按 Harness 增加
一个纯展示层：`Folder → Harness → Session`；它不是新的实体或归属关系。两种视图只是对同一
`conversation` 集合的不同查询，不新增重复 Session。

#### 3.3.1 Session Library View

Library View 是 Session Library 的查询与呈现状态，回答“这一刻侧栏要显示哪些 Session”。它由
Scope、Filters、Sort 和 Grouping 组成，不是新的归档位置，也不复制 Conversation。Library View
与后文的 View Instance 不同：前者过滤会话列表，后者标识同一 Session 的一个具体渲染位置。

### 3.4 Workbench

Workbench 是一套可命名、可恢复、可快速切换的逻辑工作现场。旧稿把它称为逻辑 Window；
从本版起只把 Window 用于操作系统物理窗口，避免继续混淆。

一个 Workbench 保存：

- 打开的 Session 引用；
- 分屏布局树；
- Tab 与 Pane 的对应关系；
- 当前活动 Tab；
- 每个 Pane 的选中项；
- 相关 Session 草稿的恢复引用（草稿身份按设备 + Session，而非 Workbench）；
- Tile 等显示设置；
- 后续可扩展的文件、Diff、终端和浏览器资源页。

Workbench 可以混放不同 Folder、不同 cwd、不同 worktree 和不同 Harness 的 Session。
Workbench 自己可选择一个主要 Collection 作为人类可读的归档位置。不要新增
`Workbench.parent_workbench_id` 来复制 Collection 的树；所谓“树形工作台”在产品上表达为
`Collection → 子 Collection → Workbench → Session references`。顶部 Workbench Tab 和快捷区
仍按最近/打开状态展示，不代表第二份归属。

### 3.5 Pane 与 Tab

Pane 是 Workbench 布局树的叶节点，Content Tab 是 Pane 中打开的内容。一个 Session 可以出现在
多个 Workbench 中，但同一 Workbench 内默认只保留一个 View Instance，重复打开时应聚焦已有
内容标签。一个 App Window 可以同时打开多个 Workbench Tab；顶层称“工作台”，Pane 内称
“内容标签”，不把两层都只叫 Tab。

### 3.6 Group Conversation / Chatroom

群聊是多个持久 Session 的共享事件流和内容面板，不是 Session 的所有者，也不是新的组织层。
产品交互上 Session 可以直接作为群成员和可 `@` 的 Actor；成员关系使用稳定 Session Address：
同一 Backend 内是 Conversation ID，跨 Backend 时增加稳定 `backend_ref`；不要求先创建 Agent Profile、
Team 或角色模板。

群聊属于核心 Session Workbench 稳定后的协作轨道。先基于现有 `@` 委托、Codeg MCP 或
AgentBus 增加多选发送、定向转发和结果比较，再实现共享时间线。群聊与普通 Session 一起显示，
但实现上不得为图省事假装它就是一个单 Harness `conversation`。

群聊必须分开表达共享可见性、实际激活目标和模型上下文摄入。群里的消息对成员可查询，但只有
显式 `@` 到的 Session 启动 turn；未打开在 Workbench 中的后台 Session 也可以是成员。

Codeg 已有的 `chat_channel` 是 Telegram 等外部消息渠道，
`chat_channel_thread_binding` 是外部线程到单个 Conversation 的绑定；二者都不等同于本文的
多 Session 群聊。后续实现群聊时可以复用其消息接入能力，但不得改变已有表的语义。

### 3.7 Project、Topic、Task 与 Decision

第一阶段不新增独立 Project 类型。Project、Subproject、Topic、Chapter 和 Workstream
统一由可嵌套 Collection 表达；只有未来出现独立权限、同步或生命周期需求时，才评估把
Project 提升为实体。

Codeg 已有 `work_task` 是带队列、worktree、审查与合并状态机的编码执行任务，不是长期
Topic。普通讨论不得为了进入 Collection 而创建 `work_task`。

Decision、项目记忆和自动话题路由属于后续派生层。它们必须引用 Session 或 Artifact 作为
来源，并用状态和 `supersedes` 关系保留被取代的结论，不能覆盖原生 Session。

### 3.8 Fork 与其他 Session 关系

Codeg 当前 `conversation.parent_id` 专用于 delegation 子会话，不能扩展成通用 Fork 父指针。
原生 Fork、跨 Harness handoff、咨询、审查和依赖等关系需要时，应新增独立关系表，或先由
适配器生成只读关系视图。

自动检测到话题变化时不得立即 Fork。第一阶段只保留人工 Fork 和显式恢复；后续话题路由
应先建议恢复已有 Session，或创建“新 Session + 精简交接摘要”。只有探索同一问题的替代
路线时才建立 Fork 谱系。

Codeg 0.25.0 已具有 ACP 当前末端 `session/fork` 与“分叉发送”，但这不等于从任意历史消息
分叉，更不等于恢复文件。三者的术语、provider 选择、JSONL 防护与分阶段实现见
[Session 历史能力子 RFC](./SESSION-HISTORY-CAPABILITIES-RFC.zh-CN.md)。

## 4. 产品结构

```text
Codeg
├── Collections
│   └── 长期语义分类
│       └── Session primary locations
├── App Windows
│   └── opened Workbench tabs
├── Workbenches
│   └── 可保存的工作现场
│       ├── Pane / Tab layout
│       ├── Session view references
│       └── Resource views
├── Sessions
│   └── 原生会话事实
├── Optional collaboration
│   ├── multi-select send / forward / compare
│   ├── AgentBus advanced bindings / messages
│   └── group conversation panels
└── Execution Contexts
    └── Folder / cwd / worktree / Harness runtime
```

各层相互独立：

- Session 可以不属于任何 Collection；
- Session 最多只有一个主要 Collection；收藏、最近、搜索和快捷视图不算第二个归属；
- Session 可以出现在多个 Workbench 和 App Window，但仍只有一份 Conversation 与运行时；
- Workbench 不属于某个固定 Folder；
- 一个 Workbench 同一时刻只挂载到一个 App Window；复制 Workbench 会生成新身份；
- 群聊可以引用任意 Session，不改变 Collection、Workbench 和 Session 私聊；
- AgentBus project 可以映射到外部群聊成员或 Collection，但不与它们合并；
- 最近使用、收藏和搜索属于视图，不需要复制 Session。

## 5. 目标与非目标

### 5.1 第一阶段目标

1. 在一个界面统一管理不同 Harness 的原生 Session；
2. 用层级 Collection 组织 Session，不受物理路径限制；
3. 保存多套命名 Workbench，并在 App Window 顶部作为工作台标签打开、切换和恢复；
4. 支持把不同目录、不同 Harness 的 Session 混放在同一 Workbench；
5. 提供拖动到边缘自动分屏的 IDE 式吸附交互；
6. 保证标题、活动时间和新 Session 能从外部 Harness 自动同步；
7. 保留 Codeg 当前会话恢复、文件、Git、终端和多 Agent 能力；
8. 明确打开、关闭、停止、归档和删除的生命周期边界，避免界面动作产生隐式数据或运行时副作用。

### 5.2 第一阶段非目标

- 自动检测话题变化并自动 Fork；
- 项目共识、知识图谱和长期记忆自动维护；
- 复杂 Workflow 或 Autopilot；
- 一次完成完整群聊协议、自动主持人或复杂 Workflow；
- 替换 ACP 或统一抹平所有 Harness 的能力差异；
- Workbench 布局跨设备实时协同编辑；
- 把聊天正文做成自由画布；
- 强制把现有 Folder、Git 或 worktree 概念删除。

## 6. 核心用户场景

### 场景 A：围绕一个主题组织多个 Harness

用户创建 Collection“主题 A”，把已有的 Claude、Codex 和 Grok Session 放入其中。移动或添加引用不会改变这些 Session 的运行目录。

### 场景 B：保存工作现场

用户创建 Workbench“主题 A 写作”，左侧放资料调查 Session，右上放正式写作 Session，右下放审查 Session。关闭应用后重新打开，分屏、活动内容标签和草稿均恢复。

### 场景 C：快速切换工作主题

用户从“主题 A 写作”切换到“实验 B”，界面整体切换到另一套 Session 和分屏布局；后台运行中的 Session 不因切换而中断。

### 场景 D：外部会话继续活动

用户在 Claude Code 或 Codex Desktop 中继续某个原生 Session，并在那里重命名。Codeg 自动检测变化；如果 Codeg 中没有人工锁定显示名，则更新标题和活动时间，并把该 Session 排到正确位置。

### 场景 E：直接拖动建立分屏

用户把一个 Tab 拖到目标 Pane 的右边缘。界面显示半透明投放预览，松手后自动创建右侧 Pane 并移动 Tab；拖到中央则加入目标 Pane 的 Tab 列表。

## 7. 功能需求

### 7.1 Session 同步与可靠性

#### SYNC-001 标题优先级

标题解析必须遵守明确优先级：

1. 用户在 Codeg 中显式锁定的标题；
2. Harness 原生的最新自定义标题；
3. Harness 提供的自动标题；
4. 第一条用户消息摘要；
5. “未命名会话”。

外部标题变化不得覆盖 Codeg 中 `title_locked = true` 的标题。用户应能执行“恢复使用源标题”。向原 Harness 反向写入标题仅在适配器明确支持时提供，不作为第一阶段硬要求。

#### SYNC-002 活动时间

导入器重新扫描已有 Session 时，必须同步真实的最新活动时间。继续过的会话应按最新活动排序，而不是停留在首次导入时间。

#### SYNC-003 增量刷新

系统应自动发现：

- 新创建的原生 Session；
- 新增消息；
- 外部重命名；
- Session 删除或文件失效；
- Session 的模型、cwd 等可用元数据变化。

实现可以采用文件监听加周期性校验；界面仍保留手动刷新作为故障恢复入口。

原生来源暂时不可读、路径移动或单次扫描失败时，Conversation 先标记来源不可用，并提供重扫、
重新定位或忘记索引。只有确认原生 Session 已删除且用户明确选择后，才清理 Codeg 索引；不得把
I/O 故障当作删除事件。

#### SYNC-004 原生身份

导入不得复制聊天形成另一份事实源。Codeg 的 Conversation 继续保存并使用原生 `external_id`，恢复时调用对应 Harness 的 resume 能力。

#### SYNC-005 能力保真

UI 只展示当前 Harness 实际支持的模型、思考强度、Fork、Rewind 和 Resume 能力。ACP 未暴露的原生能力可由专用适配器补充，但不得用无效按钮制造“统一能力”的假象。

Fork 必须进一步区分当前末端 Fork、历史消息分叉和文件恢复。运行时能力矩阵、provider 优先级
与降级策略以 [Session 历史能力子 RFC](./SESSION-HISTORY-CAPABILITIES-RFC.zh-CN.md) 为准。

#### SYNC-006 一个 Session、多个视图

同一个 `conversation_id` 可以由多个 Workbench、App Window 或客户端同时显示。它们必须附着
到同一个活动 Session runtime：已发送消息、流式输出、工具调用、审批/问题、状态、取消和用量
实时广播；不得因为打开第二个视图而创建或 Resume 第二份原生会话。

布局、焦点、滚动位置和选区属于 View Instance。未发送草稿在同一设备内按 Session 共享，
输入框采用可接管的单编辑者规则；send/cancel 由后台按 Session 串行化，并使用幂等请求 ID
避免重复提交。跨设备草稿同步不是第一阶段硬要求，已发送消息和运行时事件同步是正确性要求。

#### LIFE-001 生命周期动作不得级联

- 关闭 Content Tab 只移除当前 View Instance；
- 关闭顶部 Workbench Tab 只从当前 App Window 卸载并保存工作台；
- 删除 Workbench 只删除布局和引用；
- 删除 Collection 只解除或迁移分类；
- 归档只改变日常可见性；
- 停止 turn/runtime 必须由用户显式执行；
- 删除 Session 是独立危险操作，并清楚说明是否影响原生 Harness 历史。

任何普通关闭、移动和拖放都不得暗含删除 Session 或停止 Runtime。系统应保存可恢复的最近关闭
记录；它只指向原 Session 或 Workbench，不复制 Conversation。

### 7.2 Collection

#### COL-001 层级分类

支持创建、重命名、移动、排序、着色和归档任意层级 Collection。

#### COL-002 Session 成员关系

支持通过拖放、右键菜单和命令面板把 Session 移入 Collection。删除 Collection 只删除分类
关系或把成员移到未分类，不删除 Session。

一个 Session 第一版最多只有一个主要 Collection。数据库可使用 `conversation.primary_collection_id`
或带唯一约束的关联表表达，实施前由 schema RFC 根据当时 Codeg `main` 决定。不得仅为假想的
未来需求提前开放多归属；以后若确有必要，应新增显式 Shortcut/Alias，并与主要位置分开。

当前实现已经选择独立的 `collection_conversation` 关联表，并以 `conversation_id` 为主键保证唯一
主要归属。这样没有修改现有 Conversation 字段和导入写路径，也不会把物理 Folder 误当成语义
分类。Collection 删除由服务层在同一事务中解除成员关系并把直接子分类上移，不级联删除 Session。

#### COL-003 系统视图

提供最近使用、未分类、收藏、运行中和归档等系统视图。系统视图由查询生成，不创建重复 Session。

#### COL-004 与 Folder 解耦

Collection 操作不得修改 `conversation.folder_id`、cwd、worktree、Git 分支或 Agent 设置。

#### COL-005 Workbench 归档

Workbench 可以拥有零或一个主要 Collection，使大量已保存工作台能按项目、主题和阶段形成树。
该关系只决定导航位置；移动 Workbench 不移动其中 Session，也不改变布局、cwd 或运行状态。
实现优先复用 Collection，不另建一棵可嵌套的 Workbench Folder 树。

#### VIEW-001 一键范围切换

Session Library 顶部只常驻“当前 Workbench”和“全部”两个高频 Scope。当前 Workbench Scope
随活动 Workbench 切换；点击 Collection 树时进入该 Collection 及全部后代，并显示可清除的
`Collection: <name>` 范围标签。“全部”始终可见，并能一步退出局部视图。这样不需要定义在没有
明确选中目录时含义不清的“当前 Collection”。

#### VIEW-002 快捷视图与轻量筛选

运行中、等待处理、收藏、最近、最近关闭、未分类和归档作为系统快捷视图。Harness、状态、Execution
Context 和活动时间作为按需筛选；启用项必须以可单击删除的标签显示，并提供“清除全部”。普通
筛选不得自动创建 Saved View。

#### VIEW-003 搜索、排序与分组

搜索默认作用于当前 Scope，并提供显式的全局搜索入口。默认按最近活动排序；Collection 内可选
手工顺序。用户可以按 Collection、Harness、状态或 Execution Context 分组。排序和分组不得
改变主要 Collection 或 Workbench 成员关系。

#### VIEW-004 可见性不影响运行

Library View 只过滤侧栏摘要，不关闭 Tab、不卸载 Workbench、不停止 Runtime、不取消等待任务。
当前打开的 Session 被筛掉时，内容区保持不变，并显示“当前会话不在此视图”和清除入口。运行中
和等待处理的全局数量不受 Scope 影响，防止用户因过滤漏掉任务。

#### VIEW-005 状态归属与 Saved View

临时 Library View 状态第一阶段按 App Window 本地保存，使不同窗口可以观察不同范围；不写入
Conversation、Collection 或 Workbench 表，也不跨设备覆盖。命名 Saved View 延后实现；若增加，
它只保存查询定义和显示偏好，不拥有 Session。

#### VIEW-006 Session Center 与打开语义

侧栏、Quick Open 和完整 Session Center 必须复用同一查询参数与打开动作。完整交互、ctx 可选
全文检索和 Focus 语义见
[Session Center、全文检索与打开行为子 RFC](./SESSION-DISCOVERY-OPENING-RFC.zh-CN.md)。主 RFC
固定以下不变量：默认打开属于当前 Workbench；已打开则聚焦；未打开则新增固定 Content Tab；
不得用普通点击替换非空固定 Tab。

#### VIEW-007 单 cwd、Collection 与 Execution Location

侧栏默认显示 Execution Location，不为 `按分类 / 按运行位置` 永久占用第二行主标签。Collection
作为可记忆的组织模式放在视图菜单，并继续由 Session Center 提供显式筛选和批量整理。Execution
Location 视图继续支持多个已登记 Folder、远端、本地仓库、分支/worktree、Folder 别名、默认 Harness、
从节点新建 Session 和原生 Session 导入。新会话草稿在发送前可以切换 Folder；已开始的原生
Session 保持自己的 cwd。全局新建默认继承活动 Session 的 Folder；没有活动 Session 时使用当前
Workbench 最近创建所用的 Folder，再允许用户修改。

“多个已登记 Folder”只表示应用能管理许多单 cwd Session，不表示一个 Session 同时运行在多个
Folder。Workbench 可以混放这些 Session，但切换 Workbench 或 Collection 不得扩展任何 Session
的文件权限范围。

路径视图可选 `Folder → Harness → Session` 显示分组。Collection 内如需按路径或 Harness 查看，
使用筛选/分组，不把 Execution Location 嵌入 Collection 的所有权模型。

### 7.3 Workbench 与 App Window

#### WIN-001 命名与切换

支持创建、重命名、复制、删除、排序和快速切换 Workbench。侧栏显示全部已保存 Workbench，
App Window 顶部显示当前打开的 Workbench Tab。Workbench 可以放入主要 Collection 形成树形
归档；顶部仅显示打开的工作台。删除 Workbench 不删除其中 Session。

#### WIN-002 跨上下文混放

同一 Workbench 必须允许放入不同 `folderId`、不同 cwd 和不同 Harness 的 Session。每个内容标签
从自身保存的上下文派生文件、终端、Diff 和 Agent 运行信息，不能依赖一个全局活动 Folder。

#### WIN-003 完整恢复

切换或重启后恢复：

- 布局树；
- 打开的会话 Tab；
- Tab 顺序及 Pane 归属；
- Pane 当前选中项；
- 活动 Tab；
- 草稿；
- 分隔比例；
- Tile 等显示状态。

#### WIN-004 Session 多 Workbench 引用

一个 Session 可以出现在多个 Workbench。打开已存在于当前 Workbench 的 Session 时聚焦已有
内容标签；从另一 Workbench 打开时创建新的 View Instance，不复制 Conversation。所有 View
Instance 订阅同一个 runtime，具体同步语义见 SYNC-006。

#### WIN-005 后台生命周期

切换 Workbench 只切换界面，不得自动中断其他 Workbench 中正在运行的 Session。资源回收需要
独立策略，不与可见性直接绑定。

#### WIN-005A 切换缓存与性能

Workbench 切换必须是应用内状态切换，不允许通过整页导航重建侧栏和全部 Provider。切换前保存
当前布局，目标 Workbench 的 Tab snapshot 与布局先从内存/本地缓存恢复，再按版本号后台校验；
`tabs://changed` 或删除事件负责使缓存失效。Session 消息与 Runtime 状态继续由按
`conversation_id` 的全局 Store 持有，不随 Workbench 隐藏而清空。

第一阶段缓存最近使用的少量 Workbench snapshot 即可，不长期保活所有 React 子树、文件树、
终端和 WebView。先测量切换耗时；只有组件重挂载仍是主要瓶颈时，再为最近 2–3 个 Workbench
引入有上限的 keep-alive。索引/snapshot 缓存与完整聊天内容缓存必须分开。

#### WIN-006 原生系统窗口

一个 App Window 可以打开多个 Workbench Tab。Workbench 可以在窗口之间移动；同一 Workbench
默认不能同时挂载两次，避免布局双写。需要两份不同布局时执行复制并生成新的 Workbench ID。
把 Pane 或内容标签拆为独立 Tauri 窗口、再重新吸附，列为第二阶段。

### 7.4 分屏与拖放

#### LAYOUT-001 边缘投放区

拖动 Tab 时，目标 Pane 提供左、右、上、下和中央五个投放区：

```text
左 / 右 / 上 / 下 → 创建新 Pane 并移动 Tab
中央              → 加入现有 Pane
```

命中几何采用 Paseo 式中央安全区：Pane 中央 `40% × 40%` 为“加入”，其余区域按归一化距离选择
最近的分屏方向。不能只在最外侧设置细窄边带；窄 Pane 和大屏高分辨率下都应能轻松命中。边缘
预览进入后需向中央再移动约 4% 才退出，避免指针在 30%/70% 边界附近抖动。

#### LAYOUT-002 吸附预览

悬停边缘投放区时显示半透明预览，明确松手后的新 Pane 位置和尺寸。拖动期间只维护瞬态预览，
不改正式 Tab 顺序、活动 Session 或布局树；放下时以一个 Store action 提交。组内视觉换序不得
搬动已挂载的会话正文 DOM，Tile 模式使用 CSS 顺序表达展示位置。

#### LAYOUT-003 保持现有能力

右键拆分、按钮拆分、Tab 组内排序、分隔条缩放和 Tile 模式继续可用。拖边吸附是新增快捷交互，不替换已有无障碍入口。

#### LAYOUT-004 边界行为

必须覆盖：

- 取消拖动；
- 拖入既有 Pane 中央且搬空源 Pane 时，源 Pane 折叠；
- 唯一持久 Session 拖边新建 Pane 时，原 Pane 留下同上下文的新会话草稿，否则新建的分屏会因
  源 Pane 为空而立即折叠；
- 草稿 Tab 与持久 Session 使用相同的跨 Pane 拖放规则；草稿内容按稳定 Tab ID 跟随；
- 嵌套布局；
- 投放到当前 Pane；
- 超出窗口范围；
- 触摸或指针取消；
- 运行中 Session 移动时流式输出不中断。

跨 Pane 移动属于 View reparent：旧 Pane 卸载不得清除 Session runtime 缓存或断开 ACP 连接。
连接清理需给同一 `contextKey` 的替代挂载一个事件循环的接管窗口，以兼容 React StrictMode 的
模拟清理；明确关闭 Tab 时仍在下一任务正常断开。

### 7.5 Session 资源页

Codeg 0.25.0 已有以下底座：

- Aux Panel 包含 Session Details、File Tree、Git Changes 和 Git Log；后三者根据活动
  Conversation 的 `folderId`/active folder 切换；
- `WorkspaceProvider` 维护一套应用 Workspace 内共享的 `fileTabs`，类型包括 file、diff 和
  rich-diff，普通文件以绝对路径作为身份；
- 文件区可渲染代码、Diff、图片、HTML 和 Office 预览；当前没有通用嵌入式 Browser Pane；
- commit、push、stash、merge 等是按 `folderId` 打开的独立工具窗口；
- 当前 `fileTabs` 不是 Conversation 资源集合，也不随拟议 Workbench 持久化；
- 文件树已有“在系统文件管理器中显示”和“在终端中打开”，但没有可配置的 VS Code/Cursor/Zed
  等外部编辑器目标。

目标交互只暴露两种状态：

- Session-following：由 Session 消息、工具调用或当前上下文打开，切换 Session 时跟随；
- Workbench-pinned：用户点图钉后，在当前 Workbench 中保持。

应用设置、导入器和 Git 操作窗口可以继续作为内部 utility/global 对象，不进入资源作用域选择器。
实现上需要把资源关联与布局状态分开：Session 保存引用来源，Workbench 保存固定、位置、焦点和
视图状态。相同文件路径或 URL 可以被多处引用，但不复制磁盘内容或创建第二份编辑缓冲区。

资源归属必须显式保存，不能仅依赖当前 Folder；但 UI 只显示“跟随会话 / 固定到工作台”。

#### RESOURCE-001 外部编辑器是能力边界

Codeg 内部文件区用于查看 Agent 修改、快速编辑、预览、Diff 和附加上下文，不以替代完整 IDE
为目标。文件树、文件标签/标题栏和 Session/Execution Context 菜单应提供统一“打开方式”：

- 文件以绝对路径打开，能获得可靠位置时附带行号和列号；
- 目录打开当前 Session 的真实 cwd，而不是应用全局 active Folder；
- 首次选择并记住默认编辑器，同时保留 VS Code、Cursor、Zed、系统文件管理器等临时目标；
- 编辑器探测和启动使用参数数组，不拼接 shell 字符串；Windows 路径、空格和特殊字符必须测试；
- 本地桌面直接启动；远程/Web/Docker 必须显式降级，后续才考虑 Remote URI 或服务端启动；
- Codeg 内有未保存缓冲区时先处理保存冲突，外部修改则复用文件监听刷新和冲突提示。

Paseo 已实现 editor target registry、常见编辑器探测、按文件/目录生成启动参数并记住选择，可作为
交互和跨平台实现参考。Codeg 已有 `platform.ts` 的本地桌面判断与 opener 封装，适合在其上增加
专门的 external-editor command；系统默认 `openPath` 不能替代显式编辑器选择。

### 7.6 协作动作与可选群聊（后置）

#### COLLAB-001 先提供无组织负担的动作

协作的第一步不是创建 Team 或 Chatroom，而是在已有持久 Session 上提供：

- 多选 Session 并发送同一问题；
- 将一个 Session 的结论定向转发给另一个；
- 并列比较回复；
- 查看投递、排队和等待回复状态。

这些动作不创建新的组织层，也不自动把成员私聊暴露给其他 Session。

#### COLLAB-002 可选持久群聊

群聊用于“同一组 Session 需要共享一条持久时间线”的场景，不替代普通 Session 私聊或并排比较：

- 成员必须引用已有持久 Session，不复制或接管原生历史；
- 群聊作为 Content Tab 打开，成员私聊与共享时间线严格分离；
- 删除群聊不删除成员 Session；
- “仅记录”、定向发送、显式 `@all` 和人工转发必须可区分；
- 群成员关系独立于当前 Workbench 的打开状态；
- 点击成员头像按普通打开规则进入原 Session 私聊；
- 第一版仍不增加 Team、角色模板、Workflow、自动主持人或关系图。

可见性、激活和上下文摄入必须独立保存。一条 Room 消息可以对全部成员可见，但只生成 A、B
两个 Delivery；被激活的 Session 默认只获得触发消息、回复链、置顶背景和有界近期消息，并可
在本地通过稳定的只读 Room 记录使用 Harness 现有文件读取/搜索能力。远程目标先使用有界信封，
第一版不为 Room 额外拆出 context/read/search 三套工具。

#### COLLAB-003 Delivery Router 与 AgentBus

目标由同一个 Codeg Backend 管理时，无论本地、远程还是 Docker，优先复用 Conversation runtime、
现有 `@` delegation 或 `codeg-mcp`，使用稳定 Conversation ID 路由。已连接的另一个远端
`codeg-server` 使用 Backend-qualified Session Address 和受认证 Codeg API。AgentBus 只在未托管
App、缺少直接 federation 的跨 Backend 或需要离线持久 mailbox 时进入路径。所有路径必须经过
同一 Delivery Router，并为每条消息选择唯一 adapter；
不得让 Codeg 直接注入后，同一 Session 又从 AgentBus 重复领取同一消息。

AgentBus daemon/CLI 继续独立存在。未来 Operator UI 可以展示 project、role、绑定 Session、
pending、leased、owed 和真实投递状态，但不得把“已入邮箱”写成“已唤醒或已处理”。完整设计见
[AgentBus 协作子 RFC](./AGENTBUS-COLLABORATION-RFC.zh-CN.md)。

#### COLLAB-004 Agent 发起的连续交接

Agent 在群里 `@` 另一个成员时，交接消息和后续回复进入共享时间线；没有被 `@` 的其他成员可
查询但不启动。Agent direct 私信和 Room 交接共用 `expects_reply`、链深、总 turn 数和可选预算；
普通最终回复固定为不期待再回复，只进入来源记录，不自动启动原发送者。只有新的显式目标投递才
延长协作链。系统支持停止单成员、停止本条链和停止当前 Room 活动；thinking、状态和工具日志不得
自动触发下一位成员。

完整 Room 行为和拟议数据边界见
[群聊面板与 Session 协作 RFC](./GROUP-CONVERSATION-RFC.zh-CN.md)。

## 8. Codeg 落地设计

### 8.0 防破坏规则

本节中的数据表和字段都是拟议实现。开始编码前必须重新核对当时的 `main`，并遵守：

1. 当前实体、迁移和测试是实现事实源；文档映射错误时先修文档；
2. 不改变现有字段已经承担的语义来“容纳”新概念；
3. 数据库变更必须提供旧数据迁移、失败回退和双运行时测试；
4. 新组织层只保存引用，不复制或接管原生 Session 正文；
5. 删除 Collection、Workbench、App Window、群聊或关系边不得级联删除 Conversation；
6. Folder、Work Task、Chat Channel、Custom Agent 和 Delegation 保持现有行为。

需要特别防止以下误用：

| 现有 Codeg 对象 | 当前语义 | 不得用作 |
|---|---|---|
| `conversation.parent_id` | delegation 子会话父指针 | 通用 Fork/Topic 关系 |
| `folder.parent_id` | worktree Folder 的根 Folder | Collection 父节点 |
| `folder_link` | 符号链接和路径授权 | Session 快捷方式 |
| `ConversationKind::Chat` | folderless chat 分类 | 多 Session 群聊 |
| `chat_channel` | 外部消息渠道 | 多 Session 群聊 |
| `work_task` | 编码执行状态机 | Topic 或普通 Issue |
| `custom_agent` | ACP Agent 注册 | 持久 Session 成员 |
| `TAB_ORIGIN` | 当前页面实例的回显抑制 ID | Workbench/App Window ID |

### 8.1 复用的现有底座

- `conversation`：原生 Session 索引及 `external_id`；
- `folder`：物理路径和运行上下文；
- `opened_tab`：持久化打开的 Conversation Tab；
- `src/stores/tab-store.ts`：Tab、草稿、分组和恢复逻辑；
- `src/stores/conversation-runtime-store.ts` 与 `src/hooks/use-connection.ts`：按
  Conversation 保存运行态，并区分 owner/viewer 附着；
- `src/contexts/acp-connections-context.tsx`：viewer 向 owner connection 发送或取消，后台串行化；
- ACP manager 的 user-message、snapshot、stream 与 pending question/approval 广播：同一 Session
  被另一个窗口或 Web 客户端附着时的同步底座；
- `src/lib/tab-group-layout.ts`：布局树；
- `splitTab`、`moveTabToGroup`：分屏和移动原语；
- `sidebar-conversation-list.tsx`：已有 Folder 分组、Pinned/Chat/Recent 区段、完成态开关和虚拟
  列表，可作为 Library View 渲染底座；
- `conversation-manage-dialog.tsx` 与 `listAllConversations`：已有搜索、Folder、Harness 和状态
  facet，可复用筛选参数与交互，不应在主侧栏重写另一套不兼容语义；
- `search-command-dialog.tsx`：已有 `Ctrl/Cmd+K` Quick Open，但当前只查询 active Folder；
- `tab-store.ts` 的 `openTab(..., pin=false)`：当前实现为 Pane 内单一 Preview，下次侧栏单击会
  替换旧 Preview，而 Quick Open 使用 `pin=true`。实现 Workbench 前必须统一默认打开语义；
- `workspace:tab-groups:v1`：现有单布局本地状态；
- Tauri/Web 共享 Rust 服务及事件桥：后续多端读取 Workbench 元数据的基础。

当前 `makeConversationTabId(folderId, agentType, conversationId)` 和 `bindConversationTab` 会在全局
Tab Store 中去重同一 Conversation，适合单一渲染树，但无法表达“同一 Session 在两个 Workbench
各有一个 View Instance”。改造时必须把 `view_instance_id` 与 `conversation_id` 分开：前者标识
显示位置，后者继续标识唯一 Session 和 runtime。

产品概念必须映射到现有字段，而不是创建平行事实：

```text
Session native id      → conversation.external_id
Harness                → conversation.agent_type
Local display name     → conversation.title + title_locked
Execution Context      → folder_id + folder.path + origin_cwd + runtime settings
Delegation relationship→ conversation.parent_id（仅限现有 delegation 语义）
```

第一阶段不新增 `session`、`harness` 或 `execution_context` 表，也不把 Conversation 正文复制到
Workbench 或 Collection 表中。

### 8.2 建议新增表

```text
workbench
├── id
├── name
├── color
├── sort_order
├── created_at
└── updated_at

collection
├── id
├── parent_id
├── name
├── color
├── sort_order
├── archived_at
├── created_at
└── updated_at

collection_conversation
├── collection_id
├── conversation_id
├── sort_order
└── created_at
```

`collection_conversation` 对 `conversation_id` 增加唯一约束，使一个 Session 最多只有一个主要
Collection；若更适合当前迁移，也可改为在 Conversation 侧增加可空的 primary collection 引用。
两种方式只能择一，不同时制造两份归属事实。

`opened_tab` 增加 `workbench_id`，使同一个 Conversation 能通过不同记录出现在多个 Workbench。
建议增加 `(workbench_id, conversation_id, agent_type)` 的唯一约束；记录自身 ID 是 View Instance
身份，不能再由 Conversation canonical ID 充当。草稿继续保持惰性创建，不在第一次发送前生成
Conversation。

App Window 挂载、窗口几何和活动 Workbench 属于设备本地状态，第一阶段不要求建立服务器共享
领域表。至少保存：`window_instance_id`、打开的 Workbench ID 顺序、活动 Workbench、位置、尺寸
和显示器。一个设备内同一 Workbench ID 只允许一个活动 mount；“复制工作台”创建新 ID。

`collection.parent_id` 是新增 Collection 自身的嵌套关系，与现有 `folder.parent_id` 无关。
Collection 成员关系不得使用 `folder_link`，因为后者具有真实文件系统链接和路径授权语义。

暂不新增 Project 表、群聊表、Decision 表或通用 Conversation 关系表。它们进入对应里程碑前
必须单独编写 schema RFC。若未来需要 Fork 谱系，建议新增类似
`conversation_relation(source_conversation_id, target_conversation_id, relation_kind, metadata)`
的关系表，不复用 `conversation.parent_id`。

### 8.3 Workbench 与 App Window 状态

第一阶段把当前单一键：

```text
workspace:tab-groups:v1
```

迁移为按 Workbench 和设备隔离的键，并增加窗口挂载状态：

```text
workbench:{workbenchId}:tab-groups:v1
app-window:{windowInstanceId}:workbench-tabs:v1
app-window:last-active-id
```

SQLite 保存 Workbench 元数据和持久 Session 视图关系；本地存储保存 App Window 挂载、布局树、
Pane 选择、焦点和设备相关显示状态。Session 草稿另按 Conversation 保存并通过单编辑者租约在
同一设备视图间共享。这样不会让桌面、Web 和移动端互相覆盖焦点与布局。

当前 `tab_service` 使用全局 `opened_tabs_version` 做 CAS，并通过替换整张 `opened_tab` 集合保存
状态，并把 `is_active` 作为跨客户端共享事实镜像。引入 Workbench 和独立 App Window 后不能
只给表增加 `workbench_id`；还必须把查询、删除、保存、版本号和 `tabs://changed` 事件按
Workbench 分区，并停止在窗口之间同步焦点，否则一个客户端保存某个 Workbench 时会清空其他
Workbench，两个物理窗口也会互相抢活动标签。迁移前应先为现有全局 CAS 行为增加回归测试。

如果以后需要跨设备同步布局，再增加带 `client_id` 和版本号的 Workbench state，而不是让多个客户端争写同一个 JSON。

### 8.4 状态边界

`TabItemInternal` 已经携带 `folderId`、`conversationId`、`agentType` 和 `workingDir`，满足跨 Folder
Workbench 的基本条件。新增 Workbench 与 View Instance 后应保持：

- Tab 自己决定运行上下文；
- `activeFolderId` 只是当前聚焦结果，不是其他 Tab 的数据源；
- Workbench Store 管理已保存工作台；App Window Store 只管理本窗口打开顺序和活动 Workbench；
- Tab Store 的可见状态按 Workbench 装载；
- 一个 View Instance 引用一个 Conversation，但多个 View Instance 共享一个 Runtime Session；
- 后台运行连接独立于 Workbench 是否可见；
- Conversation Store 不因切换 Workbench 而重新导入会话正文；
- domain/runtime 事件广播到全部视图，focus/scroll/layout 事件只留在本 View/Workbench/App Window。

Library View Store 独立于上述 Runtime 和布局状态，建议最小结构为：

```text
appWindowLibraryView
├── scope: all | collection(id, includeDescendants) | activeWorkbench
├── query
├── harnesses[] / statuses[] / executionFolderIds[]
├── archivedMode / pinnedOnly / activityRange
├── sort
└── groupBy
```

它只参与 Conversation Summary 查询和侧栏 selector。第一阶段保存在 App Window 本地状态，不必
新增 SQLite 表。若会话量使前端筛选成本明显上升，再把同一参数对象下推到
`listAllConversations`，不得让客户端和服务端各自定义一套状态含义。

### 8.5 旧数据迁移

升级时自动创建“默认工作台”：

1. 现有 `opened_tab` 全部关联到默认 Workbench；
2. 现有 `workspace:tab-groups:v1` 移动或复制到默认 Workbench 的键；
3. 当前系统窗口挂载默认 Workbench；
4. 保留旧键一个版本周期作为回退；
5. 迁移失败时仍能以单 Workbench 兼容模式启动；
6. 任何迁移都不得删除 Conversation 或原生 Session 文件。

## 9. 界面建议

左侧导航与顶部工作台栏分别承担“管理”和“切换”：

```text
[顶部] [工作台 A] [工作台 B] [+]

快捷视图
├── 最近使用
├── 收藏
├── 运行中
└── 未分类

Collections
├── 主题 A
│   ├── 调查
│   └── 写作
└── 主题 B

Saved Workbenches
├── 工作台 A
├── 工作台 B
└── 工作台 C（未打开）

Execution folders
└── 按需展开物理路径
```

顶部 Workbench Tab 负责快速切换已打开的工作现场；侧栏 Saved Workbenches 负责发现、打开、
重命名和归档全部工作台；Collection 树负责定位长期 Session。三者不能合并为同一个树，因为
同一主题可能有多套布局，同一布局也可能临时包含多个主题。

Session 搜索结果应显示：

- 显示标题；
- Harness；
- 所属 Collection；
- cwd 或 Folder；
- 最近活动时间；
- 当前出现在哪些 Workbench；
- 运行状态。

Session Library 顶部建议保持为两行以内：

```text
[当前工作台] [全部]                          [搜索]
[Collection: 主题 A / 调查 ×]
[等待处理 2] [Claude ×] [运行中 ×] [清除全部]
```

Collection 范围行只在用户选中目录时出现；筛选行只在快捷视图或筛选启用时出现。低频 Harness、
状态、Execution Context 和活动时间选项
进入同一个轻量弹层；排序与分组放在视图菜单，不把所有 facet 永久平铺在侧栏。点击 Collection
树直接进入对应 Scope，搜索空结果必须同时显示当前 Scope、已启用筛选和一键清除入口。

如果当前打开 Session 不匹配视图，Pane 不关闭，侧栏在活动项附近显示“当前会话不在此视图”。
全局运行中和等待处理数量保持可见，点击数量可临时切换到对应系统视图。

## 10. 实施顺序

这里是 Codeg 的工程交付顺序，不等同于领域模型中的长期产品优先级。拖边吸附等局部改动
依赖少、风险低，可以提前交付；它不表示分屏在产品价值上高于 Session 可靠性和长期分类。

### Milestone 0：基线可靠性

- 为标题优先级、`title_locked`、外部重命名和 `updated_at` 增加回归测试；
- 验证自动刷新及恢复原生 Session；
- 统一侧栏和 Quick Open 的固定打开/聚焦语义；
- 明确关闭、停止、归档、删除及最近关闭恢复；
- 明确各 Harness 的 Resume、当前末端 Fork、历史分叉和文件恢复能力矩阵；
- 保留并加固 Codeg 现有 ACP“分叉发送”，不把它误写成完整 Rewind。

### Milestone 1：拖边吸附分屏

- [x] 增加五区投放检测和吸附预览；
- [x] 中央投放复用 `moveTabToGroup`，四边投放在指定目标 Pane 旁建立新叶节点；
- [x] 左/上投放显式固化旧的“首叶默认归属”，避免插入新首叶时其他 Tab 静默换组；
- [x] 唯一持久 Session 拖边时为原 Pane 留下同上下文草稿；草稿可带着未发送内容跨 Pane 移动；
- [x] 覆盖四边检测、嵌套目标、草稿和单 Tab 源 Pane 的单元测试；
- [x] 在真实 Tauri WebView2 中完成桌面端指针手势、吸附预览、松手分屏与控制台回归；
- [x] 在真实 Tauri WebView2 中移动已连接 Codex Session，验证连接 ID 不变且无 disconnect/spawn；
- [ ] 用正在流式输出的 Session 验证重挂载期间连接不中断。

该阶段的代码路径已经可用；完成最后一项真实运行态验收后即可视为交付。

### Milestone 2：逻辑 Workbench 与顶部工作台标签

- [x] 新增 Workbench 表及 `opened_tab.workbench_id`；
- [x] 把 Tab Group 状态按 Workbench 分区；
- [x] 增加创建、命名、切换、删除和基础恢复；
- [x] 完成默认 Workbench 迁移，并保留旧 API 与默认布局键兼容；
- [x] 让“当前 Workbench”属于物理窗口的会话状态，避免多个窗口共享焦点；
- [x] 增加一个 App Window 内的顶部 Workbench 标签栏，并显示正在恢复的目标；
- [x] 增加 Workbench 复制和持久化排序；复制 Session 引用与布局，不复制设备本地草稿；
- [x] 增加 Workbench 置顶；置顶区和普通区分别排序；
- [ ] 增加归档和最近关闭恢复；
- [ ] 增加 Workbench 元数据跨窗口事件及系统窗口 mount。

当前实现是 Milestone 2 的可用基础，不代表 Milestone 已完成。侧栏切换器已能恢复不同 Session
组合和分屏布局，顶部标签提供当前窗口的快速切换、复制与排序；归档/最近关闭、物理多窗口和
同一 Session 多视图同步仍属于后续子阶段。

### Milestone 3：Collection

- [x] 新增层级 Collection 和唯一主要归属；
- [x] 增加左侧树、未分类，以及 Collection 本身和全部后代的会话中心 Scope；
- [x] 增加创建、重命名、移动、非破坏性删除和 Session Center 批量移动；
- [ ] 增加 Session/Collection 拖放、同层手工排序与更紧凑的范围标签；
- 增加运行中、等待处理、收藏、最近、最近关闭快捷视图和轻量 Harness/状态筛选；
- 复用现有管理弹窗 facet 与 Conversation 查询参数，保持主侧栏和管理页筛选语义一致；
- 增加 Session Center；统一侧栏、Quick Open 和管理页的固定打开/聚焦行为；
- ctx 全文搜索作为可选后续子阶段，失败不得影响 Codeg 元数据搜索；
- 确认任何分类操作都不改变 Folder/cwd。

当前实现已完成 Collection 的最小可用闭环，并通过真实 Tauri 数据库验证父子分类、唯一归属、
按子树筛选及删除回到未分类。拖放、排序、颜色和 Collection 自身归档仍是后续增强，不阻塞普通
Session 分类使用。

### Milestone 4：多视图同步底座、资源作用域与可选系统窗口

- 复用现有 viewer/snapshot/stream 底座，实现同一 Session 多 View Instance 实时同步；
- 拆分全局 `opened_tabs`、`is_active` 与焦点，防止窗口互相覆盖；
- Session 资源跟随和 Workbench 图钉固定；
- 文件、Diff、预览与资源视图状态随所属对象恢复；
- 增加文件/目录“在外部编辑器打开”，本地记住默认目标并对远程路径明确降级；
- 处理桌面/Web/未来系统窗口同时显示同一 Session 的输入并发；
- 只有上述同步通过故障测试后，才把 Tauri 原生窗口、窗口几何和跨窗口拖放作为 Bonus 开放。

单窗口多 Workbench 已经满足核心工作流。当前 Tauri 第二窗口与跨窗口标题刷新只作为可行性
Spike 保留，不进入前三个 Milestone 的承诺，也不能挤占 Collection、路径视图、Session Center
和布局可靠性的开发时间。

### Milestone 5：可选协作动作

- 先实现多选发送、定向转发、结果比较和真实投递状态；
- 复用现有 delegation/MCP/AgentBus；
- 不要求 Team、角色模板或关系图；
- 在稳定 Session 投递和多视图同步之后增加轻量群聊面板；
- 第一版群聊只做共享时间线、显式目标、成员私聊跳转和真实状态；
- Agent 间连续 `@`、上下文游标和链深/预算保护作为下一子阶段。

AgentBus 管理不必全部等到 Milestone 5。只读 project/role/绑定总览和 Operator 消息中心可在
Session 身份稳定后作为并行 Collaboration Track 实施；直接 Harness turn 投递和完整群聊
仍依赖稳定的 Resume、去重和 Conversation 路由模型。具体阶段见协作子 RFC。

受管 CLI Gateway 不进入 Milestone 5 的完成条件。它在 Codeg 原生协作和 AgentBus 外部桥稳定后
作为独立后续里程碑实施，不能以 PTY wrapper 取代结构化 ACP/原生运行时。

### 10.1 GitHub Issue 对应关系

当前已提交的标题、同步、Collection、Workbench、新窗口和 Team/Chatroom Issue 统一记录在
[GitHub Issue 与需求追踪](./ISSUE-TRACKER.zh-CN.md)。实施前必须核对实时 Issue 状态，关闭的
问题仍需保留回归验证，不能仅凭 Closed 状态删除本 RFC 的可靠性要求。

## 11. 验收标准

核心日常闭环完成的最低标准：

1. 用户能创建至少两套命名 Workbench，并在一个 App Window 顶部一秒内切换；
2. 每套 Workbench 可保存不同的跨 Folder、跨 Harness Session 组合；
3. 重启后 Workbench 挂载、布局、Tab、活动项和草稿正确恢复；
4. 把 Tab 拖到 Pane 四边能创建对应分屏，中央能加入现有 Pane；
5. 运行中的 Session 在移动内容标签或切换 Workbench 时不丢流式输出；
6. 用户能建立层级 Collection，并在不改变 cwd 的情况下分类 Session；
7. 删除 Workbench 或 Collection 不删除 Session；
8. 外部新建、继续或重命名 Session 后，Codeg 能自动刷新标题和时间；
9. 现有单布局用户升级后自动进入默认 Workbench，不丢会话和草稿；
10. 用户能一步切换到当前 Workbench 或全部 Session；点击 Collection 后范围标签清楚且可一步退出；
11. 常用快捷视图和筛选能在两步内启用或清除，启用状态清楚可见；
12. 筛选不会移动、关闭或停止 Session；当前项被筛掉时内容保留，全局待处理提醒仍可见；
13. 侧栏、Quick Open 和 Session Center 对默认打开行为一致，普通点击不会替换非空固定 Tab；
14. 用户能区分关闭、停止、归档和删除，删除 Workbench 或 Collection 不删除 Session；
15. Session Center 单击预览不启动 Runtime，显式打开才加入或聚焦当前 Workbench；
16. 可选 ctx 全文搜索不可用时，Codeg 标题、属性和 Collection 搜索仍可正常使用。

Milestone 4 还必须满足：同一 Session 在两个 Workbench/窗口显示时只启动一份 runtime，双方实时
看到消息和 stream；两个 App Window 可以独立切换工作台和焦点；桌面与 Web 读取同一 Workbench
和 Collection 元数据时，不用设备焦点和布局互相覆盖。

若交付 Milestone 5 的群聊部分，还必须满足：群里 `@A` 后全体成员可查看但只有 A 被执行；无目标
消息明确标为仅记录；`@all` 发送前显示目标数量；成员私聊不回流；关闭 Room 不停止或删除成员；
后台成员能排队或显示真实不可投递原因；Agent 协作链达到限制或被用户停止后可靠终止。

## 12. 暂缓决策

下列问题不阻塞前三个 Milestone：

- 是否允许一个 Session 在同一 Workbench 中出现两次（当前默认只聚焦已有视图）；
- Codeg 内重命名是否反向写回所有 Harness；
- 跨设备是否同步未发送草稿和输入编辑权；
- 是否有足够真实复用需求，值得在未来增加隐藏或显式 Team 预设；
- 是否需要在“仅记录/显式目标”之外增加可见的主持或自动路由模式；
- 项目记忆、共识和资产是否建立独立对象。

当前默认选择是：数据模型保留扩展能力，界面先实现最简单、最可预测的行为。

## 13. 结论

本需求不是新建另一个 Agent 平台，而是补齐 Codeg 作为重度 Session 工作台的组织层和工作现场层。

第一阶段最有价值的四项交付是：

1. 可靠的原生 Session 身份、同步和 Resume；
2. 一致且不破坏内容的打开与生命周期语义；
3. 命名、保存和切换多套 Workbench，以及 Tab 拖边吸附分屏；
4. 与物理路径解耦的 Collection 层级树和轻量搜索筛选。

完成这四项后，用户已经可以在一个 GUI 中统一管理不同 Harness、长期分类 Session，并恢复多个
复杂工作现场。同一 Session 多视图同步是 Web、移动端和未来系统多窗口共用的正确性底座；打开
第二个物理窗口本身则是可后置的 Bonus。Codeg 内部
协作先做无组织负担的多选发送、转发和比较；受管远端优先走 Codeg API，AgentBus 只作为未托管
App、缺少 federation 的跨 Backend 和离线邮箱桥后置融入。群聊、项目记忆和自动编排继续建立在
可靠 Session 管理之上，Team 只有在真实复用需求出现后再评估。
