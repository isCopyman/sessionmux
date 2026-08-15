# Workbench 层级、多窗口与 Session 多视图同步子 RFC

> 状态：Draft  
> 日期：2026-08-15  
> 定位：裁决 Workbench Tab、Pane、Content Tab、App Window 与 Session Runtime 的边界。  
> 实现约束：源码事实变化时先修本文，不得为了匹配提案改变 Codeg 现有字段语义。

普通使用者只需阅读[产品需求与使用场景](./PRODUCT-SPEC.zh-CN.md)。本文记录参考项目核对、
Codeg 当前底座、同步正确性和实施约束。

## 1. 已裁决的产品结构

```text
Session Library
└── Collection tree
    └── Session 的唯一主要归档位置

App Window
├── Workbench Tab A
│   └── Pane layout
│       ├── Content Tab / View Instance ──> Session A
│       └── Content Tab / View Instance ──> Session B
└── Workbench Tab B
    └── Pane layout
        └── Content Tab / View Instance ──> Session A
```

核心规则：

1. Collection 是可嵌套文件夹；一个 Session 最多一个主要 Collection；
2. Workbench 是可命名、可保存的整套布局，既出现在侧栏，也可在窗口顶部作为工作台标签打开；
3. App Window 是操作系统物理窗口，一个窗口可打开多个 Workbench；
4. 一个 Workbench 在同一设备上只挂载一次，跨窗口是移动，复制会生成新 Workbench；
5. 同一 Session 可以出现在多个 Workbench 和 App Window；每处只是 View Instance；
6. 多个 View Instance 共享一个 Conversation、原生 Session 身份和活动 Runtime Session。

## 2. 为什么需要顶层 Workbench Tab

只有侧栏 Workbench 切换器时，每次切换都像“关闭当前桌面、载入另一个桌面”，难以同时保留
几个高频工作现场。顶层工作台标签提供浏览器式快速切换，侧栏继续负责完整管理：

| 入口 | 负责什么 |
|---|---|
| 侧栏 Saved Workbenches | 查找、创建、重命名、复制、排序、归档全部工作台 |
| 顶部 Workbench Tabs | 当前 App Window 已打开工作台的快速切换、关闭和跨窗口移动 |
| Pane 内 Content Tabs | Session、文件、Diff、网页、终端等具体内容 |

两层标签不能都只显示为 `Tab`。界面文案使用“工作台”和“内容标签”，命令 ID 也应区分
`workbench.*` 与 `contentTab.*`。

## 3. 参考项目核对

### 3.1 Herdr

Herdr 的公开概念明确写成 `Workspace → Tabs → Panes`：Workspace 是项目级容器，Tab 是
Workspace 内一套 layout，可分别命名为 agents、logs、server、review。这个 `Tab` 的语义最接近
本文 Workbench，而不是某个 Session 内容标签。

可借鉴：

- 一个上层容器下同时保留多套命名布局；
- 服务端拥有运行状态，客户端附着/脱离不等于杀死 Pane；
- 工作现场和运行生命周期分离。

不照搬：Herdr Workspace 仍带项目/cwd 色彩，且 TUI Pane 主要是真实终端进程；Codeg 的
Workbench 必须能混放不同 Execution Context。

### 3.2 Orca

Orca 当前 `TabsSlice` 以 `unifiedTabsByWorktree`、`groupsByWorktree`、
`activeGroupIdByWorktree` 和 `layoutByWorktree` 保存状态；`TabGroup` 是布局叶节点中的内容标签组。
这更接近：

```text
worktree/workspace → 一棵 Pane/TabGroup 布局 → Content Tabs
```

它证明分屏和标签组可以围绕执行上下文稳定实现，但并没有提供 Herdr 同等语义的“一个 Workspace
内多套命名布局 Tab”。Codeg 应借鉴其拖放/分组实现，不应把 Orca Content Tab 当作顶层 Workbench。

### 3.3 Paseo

Paseo 主要层级是 Project → Workspace → Agent/Terminal/Browser tabs，布局和文件工具围绕
`serverId + workspaceId` 路由。它的 Workspace 同时承担 Execution Context 和一套工作现场，
适合“一路径一桌面”，不适合本文要求的跨路径混放与一个窗口多工作台。其完善前端和
WebSocket daemon 仍可作为交互和多客户端参考。

### 3.4 Codex Desktop Windows 包审计

2026-08-15 本地审计 Windows 包 `OpenAI.Codex 26.810.7004.0`，内部 Electron 应用版本为
`26.810.52044`。这不是公开 API 契约，只用于验证可行边界；产品行为仍以实际测试为准。

解包后的 main/renderer bundle 显示：

- Electron main 维护统一窗口注册表和 renderer/app-view 映射；每个窗口拥有自己的初始路由、
  当前页面、焦点和展示状态；
- 本地 Thread 目录使用 SQLite 保存，并带 `catalog_revision`。App Server 发出
  `thread/name/updated` 后，后台更新统一目录，再向全部窗口广播查询缓存失效；
- 因此，在另一个窗口重命名后延迟刷新并不是依赖定时扫描 JSONL。`@parcel/watcher` 虽然存在，
  但标题同步的主链路是**领域通知 → 后台目录变更 → 多窗口失效/刷新**；Watcher 更适合作为
  外部文件变化的补偿入口；
- 同一 Thread 的实时运行采用 owner/follower 角色。一个窗口拥有 stream，其他窗口通过
  `thread-follower-*` 协调开始 turn、加载完整历史、steer、interrupt、设置更新和审批，避免两个
  CLI Runtime 同时写一个原生 Session；
- 后台可以向全部窗口广播共享元数据和全局状态，但打开哪个 Thread、激活哪个页面仍属于物理
  窗口，不会把所有窗口强制切到同一路由。

这里可借鉴的是：**统一 Thread 目录 + 单一运行所有者 + 多个 follower 视图 + 窗口局部导航**。
Codeg 不需要复制 Codex Desktop，也不应把压缩 bundle 中的内部方法名当成稳定接口。

### 3.5 OpenChamber

OpenChamber 当前公开产品同时支持 Desktop 多窗口、Web/PWA、VS Code 和移动端。其公开源码把
浏览器 WebSocket 与上游 OpenCode SSE 分开：服务端只维护一套共享上游事件 Hub，再把事件扇出
到客户端；短暂断线使用 `Last-Event-ID` 回放，缺口或上游重连后通过 authoritative snapshot
校正状态。

客户端进一步区分：

- Session、消息、运行状态等目录级共享 Store；
- 当前选择、草稿、滚动、弹窗等应用或窗口局部 Store；
- snapshot generation、mutation revision、bounded cache 和 reconnect repair，防止旧快照覆盖
  新事件。

这说明桌面多窗口、Web 与移动端虽然外壳不同，正确性问题本质相同：客户端不是事实源，实时事件
也不是最终事实源；**后端快照是权威，事件负责低延迟通知，序号/版本和重连用于修复遗漏**。

## 4. Codeg 当前事实

### 4.1 已经存在的 Session 同步底座

Codeg 0.25.0 当前代码已经区分 owner 与 viewer：

- `conversation-runtime-store` 按 Conversation 保存运行态；
- `use-connection` 允许 viewer 附着，viewer detach 不终止 owner；
- `acp-connections-context` 让 viewer 的 send/cancel 指向 owner connection；
- Rust ACP manager 广播用户消息，串行化 prompt；
- snapshot 与 stream 可让另一窗口、Web 客户端或重连客户端恢复中间状态；
- pending question/approval 等也有快照或广播路径。

因此，多窗口最难的“同一原生 Session 只有一份运行时、输出广播到多个客户端”并非从零开始。

### 4.2 已完成的单窗口 Workbench 分区，以及剩余边界

当前本地分支已经完成：

- 独立 `workbench` 实体和 `opened_tab.workbench_id`；
- `tab_service` 按 Workbench 读写 Session 引用，旧 API 继续映射到 `Main`；
- 分屏树、组内选择、Tile 状态和设备本地草稿按 Workbench 键保存；
- 当前物理窗口的活动 Workbench 使用 `sessionStorage` 保存，不再由多个窗口共享一个焦点键；
- 顶部标签可创建、切换、复制和持久化排序 Workbench；复制生成新 ID，只复制 Session 引用与
  布局，不复制未发送草稿。

仍然存在的多窗口边界是：`makeConversationTabId(folderId, agentType, conversationId)` 在当前页面
Store 内仍是 canonical Tab ID；同步版本与 `tabs://changed` 还没有完全拆成 Session、Workbench
和 App Window 三层事件；同一 Workbench 也尚无显式 mount 所有权。因此可以安全使用单窗口多
Workbench，但不能只增加一个 Tauri Window 就宣称完成多窗口，否则仍可能出现布局双写和焦点覆盖。

### 4.3 Workbench 切换与缓存边界

本机 Claude Desktop 工作区补丁的历史调试说明了一个容易误判的性能问题：当主 Session 不同时，
旧补丁用 `location.assign(...)` 做整页导航，侧栏和组件树全部销毁重建；Monet 同时拥有 Session
摘要/索引缓存。实际结论是先消除整页重载，再讨论缓存，而不是把完整聊天内容常驻内存。
来源：Codex ctx session `3aa9acb9-7628-7732-9bd3-e8555e87c071`，event
`c87d807d-667e-7d68-819c-069b24b6061b`。

Codeg 当前 `switchWorkbench` 已是应用内 Store 切换：先立即保存当前 Workbench，再获取目标的
Tab snapshot，最后替换活动布局；页面、侧栏、Conversation Runtime Store 和后台 Agent 不会因
此整体重载。分屏布局已经按 Workbench 保存在本地，真正还可优化的是目标 snapshot 获取和可见
Session View 的重新挂载。

因此采用分层缓存：

1. Conversation 索引、标题、路径和状态由全局 Store 缓存并通过事件增量更新；
2. 最近 Workbench 的 Tab snapshot + version 使用小型内存 LRU，切换时先显示缓存，再后台校验；
3. Session Runtime 按 `conversation_id` 全局存在，隐藏 Workbench 不清空 stream/snapshot；
4. Pane 焦点、滚动和布局属于 Workbench/View，按 Workbench 保存；
5. 不默认 keep-alive 所有完整 React 树、文件树、终端或浏览器 WebView。只有性能测量证明重新
   挂载是瓶颈时，才保活最近 2–3 个 Workbench，并设置内存上限。

`tabs://changed` 需要携带 Workbench ID 和 version，以更新或失效相应缓存；删除 Workbench、
关闭 Folder 或 Session 失效时不得从陈旧缓存恢复幽灵标签。

## 5. 身份与所有权模型

```text
Conversation / Session
├── stable conversation_id
├── native external_id
└── at most one active Runtime Session

Workbench
├── stable workbench_id
├── layout tree
└── many View Instances

View Instance
├── stable view_instance_id / opened_tab id
├── workbench_id
├── conversation_id or resource reference
├── pane placement
└── view-local state

App Window Mount
├── device-local window_instance_id
├── ordered workbench_ids
├── active_workbench_id
└── geometry / monitor
```

`conversation_id` 不能再同时充当 View Instance ID。关闭一个 View 只删除显示引用；只有明确关闭
或归档 Session，或运行时租约归零且生命周期策略允许时，才处理底层连接。

## 6. 同步状态矩阵

| 状态 | 所有者 | 多视图行为 |
|---|---|---|
| 原生消息历史、已发送用户消息 | Session/Conversation | 全部视图共享并实时更新 |
| assistant stream、tool call、progress、usage | Runtime Session | 全部附着视图广播 |
| question、approval、cancel、运行状态 | Runtime Session | 全部视图一致；操作走同一后台队列 |
| 标题、Harness、模型、cwd 等元数据 | Conversation 索引 | 全部视图共享 |
| 未发送草稿 | device + Session | 同设备镜像；单编辑者，可显式接管 |
| Pane 布局、内容标签顺序 | Workbench | 只属于该 Workbench |
| 活动 Workbench、焦点、滚动、选区 | App Window / View | 不向其他窗口镜像 |
| 窗口位置、尺寸、显示器 | device + App Window | 设备本地恢复 |

第一阶段不承诺跨设备同步未发送草稿、焦点和布局，但不能降低已发送消息与运行事件同步的
正确性。Web 客户端和桌面窗口可以看到同一 Session，却不应被迫显示同一个活动 Pane。

同步实现不采用“所有状态都塞进后台”的极端做法，而是遵守四条边界：

1. 共享事实后端化：Session 身份、消息、标题、运行状态和队列只有一个权威来源；
2. 视图状态窗口化：活动 Workbench、焦点、滚动、拖拽和弹窗不跨窗口镜像；
3. Session Runtime 单实例化：第二个 View 只附着，不再次 Resume；
4. 变化事件化：应用内部写入立即发领域事件，断线或序号缺口再拉 snapshot；文件 Watcher 只处理
   Codeg 外部对原生会话文件的修改。

最小事件信封应能表达 `entity_id`、`revision/sequence`、`event_id`、`origin_client_id` 和变化提示。
事件可以丢失，数据库与快照不能因此失去权威性；发送命令另带幂等 `request_id`。

## 7. 输入并发规则

同一 Session 多视图不能靠“最后写入者获胜”处理输入：

1. 每个发送请求带稳定 client request ID，后台保证幂等；
2. 同一 Session 的 prompt/send/cancel 在 owner runtime 串行化；
3. 同一设备草稿按 Session 共享；获得输入焦点的 View 取得短期 composer lease；
4. 其他 View 显示“正在另一窗口编辑”，可观察镜像草稿或点击“接管输入”；
5. 接管必须转移 lease，不复制草稿；
6. 如果 Harness 支持 queued follow-up，第二条发送显示为明确队列；否则拒绝并说明当前 turn 忙。

composer lease 是 UI 防冲突，不是安全锁。真正的重复发送防护必须在后台依靠串行化与幂等键。

## 8. Workbench 多窗口规则

- 一个 App Window 可以打开多个 Workbench；
- 一个 Workbench 同一设备只允许一个活动 mount；
- 拖动顶层 Workbench 到另一个窗口执行 move mount；
- 拖出 Pane 内 Content Tab 时创建新 Workbench：默认移动 View Instance，显式复制时才让同一
  Session 在两个 Workbench 各保留一个 View；
- “复制工作台”复制布局和 Session 引用，但产生新 `workbench_id`；
- 同一 Session 因两个 Workbench 同时可见而拥有两个 View Instance 是正常情况；
- 关闭 App Window 只关闭/保存 mounts，不终止 Session；
- 恢复窗口时先创建 Window，再挂载 Workbench，最后附着可见 Session runtime；
- 布局保存失败不能删除 `opened_tab`、Conversation 或原生 Session。

限制一个 Workbench 只挂载一次，解决的是布局双写，不限制 Session 多处显示。两者不能混淆。

## 9. 事件分层

后续事件至少分三类：

```text
session://*    共享领域与运行事件，广播给所有相关 View
workbench://*  某套布局的结构事件，只更新引用该 Workbench 的客户端
window://*     物理窗口焦点、挂载和几何，只在设备本地处理
```

现有 `tabs://changed` 不能继续同时代表三层。`TAB_ORIGIN` 只用于页面实例回显抑制，不得复用为
Workbench 或 App Window 身份。

## 10. 实施顺序与启用门槛

1. 给现有跨客户端 owner/viewer、snapshot、stream 和 send serialization 增加回归测试；
2. 将 `view_instance_id` 与 `conversation_id` 分开，保持单 Workbench 内默认去重；
3. 新增 Workbench 实体，把 `opened_tab`、布局键、CAS version 和事件按 Workbench 分区；
4. 实现一个 App Window 内的顶部 Workbench 标签与侧栏 Saved Workbenches；
5. 拆分共享 Session runtime state 与 View/Window local state；
6. 到此先交付单窗口多 Workbench；物理多窗口不属于核心可用性的完成条件；
7. 作为 Bonus Track，先只允许“在新窗口查看目标 Workbench/Session”，同一 Workbench 第二次
   挂载默认只读或转移所有权，不允许两处并发修改布局；
8. 通过同一 Session 双客户端 stream、问题/审批、取消、幂等发送和故障重连测试后，才开放
   双窗口可写交互；
9. 窗口几何、跨窗口拖放、恢复全部窗口和跨设备 Workbench 布局同步最后评估。

因此，当前已经完成的 Tauri 第二窗口与标题同步验证只算技术 Spike，不算主线产品承诺。它证明
Codeg 的事件底座可以跨 WebView 工作，但不能证明同一 Workbench 布局双写、输入并发和 Runtime
所有权已经解决。

## 11. 必须通过的故障测试

1. 两个 App Window 同时显示同一 Session，只有一个原生 runtime；
2. Window A 发送，A/B 都先看到用户消息，再看到同一顺序的 stream；
3. B 在 stream 中途打开，也能通过 snapshot + 增量事件追平；
4. B 关闭不终止 A；A 关闭后 runtime 按所有权策略继续或安全转交；
5. 两窗口同时发送不会生成重复 turn；
6. 一个窗口切换活动 Workbench 不改变另一个窗口焦点；
7. 保存 Workbench A 不会删除或覆盖 Workbench B 的 opened tabs；
8. App 崩溃重启后 Session、Workbench、mount 和布局分别恢复；
9. 删除 Collection、Workbench 或 App Window 不删除 Conversation；
10. Web 与桌面同时附着时，运行事件共享，布局与焦点互不覆盖。

## 12. 结论

需要引入顶层 Workbench Tab，但它不是再造一层复杂项目管理，而是让多个已保存布局能像浏览器
标签一样同时打开。Collection 保持唯一文件夹归档，Workbench 允许 Session 多处出现，Runtime
保证 Session 只有一份事实。单窗口多 Workbench 已经覆盖主要用户需求；物理多窗口保留为 Bonus，
只有同步门槛通过后才继续。Codeg 已有的 viewer 和流式广播使路线可行，当前不值得让窗口几何、
跨窗口拖放和布局并发拖慢 Session 管理主线。
