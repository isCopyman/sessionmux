# Atrium 公开功能审计与 Codeg 对照

> 状态：Reference snapshot  
> 核对日期：2026-08-15  
> Atrium：`0.283.0`，macOS x86_64 安装包  
> Codeg：`b6e1d904a5c1f53849e8ea66672c65fdcd949546`（`0.25.0`）

## 1. 这份文档是什么，不是什么

这是一份竞品样本审计，不是“照着 Atrium 做”的需求单。产品目标仍以
[产品需求与使用场景](./PRODUCT-SPEC.zh-CN.md) 为准。

Atrium 可以帮助我们回答三类问题：

1. 一个成熟 GUI 如何组织多 Harness Session、布局和后台 Agent；
2. 哪些交互已经被真实产品证明可行；
3. Codeg 当前缺的是底层能力，还是只缺一层更好的 Session 管理界面。

它不能替我们决定领域模型。Atrium 是闭源 macOS 产品，公开的只有文档、适配器 SDK、
Issue 仓库和可下载安装包。因此本文是“官网公开功能 + 安装包静态资源 + Codeg 当前代码”
的对照，不是 Atrium 源码审计，也不把官网宣称自动视为实现质量证明。

## 2. 证据和可复查材料

官方入口：

- [Atrium 文档](https://getatrium.dev/docs)
- [功能总览](https://getatrium.dev/features)
- [发布页](https://getatrium.dev/releases)
- [Changelog](https://getatrium.dev/changelog)
- [FAQ](https://getatrium.dev/faq)
- [开源 Adapter SDK](https://github.com/jonnyasmar/atrium-adapters)
- [公开 Issue 仓库](https://github.com/jonnyasmar/atrium-issues)

本地证据归档位于：

```text
D:\code\revisiting\work\repo_audit\artifacts\atrium-0.283.0\
├── atrium_0.283.0_x86_64.dmg
├── unpacked\atrium\atrium.app\
└── official-docs-2026-08-15\
    ├── sitemap.xml
    ├── manifest.json
    ├── pages\       # 53 个页面的原始 HTML
    └── readable\    # 49 个 docs article 的可检索正文
```

安装包 SHA-256：

```text
3A6D54E88A43CA88E73C3F1CF43160FE891C0CE4DA2157B0CB3F619A18C0F4D9
```

官网 sitemap 中纳入本次审计的 53 个页面全部下载成功。安装包的 `Info.plist`、发布 API 和
文件资源均指向 `0.283.0`；个别网页页脚仍显示较旧版本，因此版本判断以发布 API 和包内元数据
为准。

## 3. 先给结论

### 3.1 Atrium 最值得借鉴的部分

- **把路径、会话组织和工作现场区分开。** Project 负责目录，Wing/Room 负责组织和布局，
  Pane 承载具体 Session 或工具。
- **Room 是可恢复的工作台，而不是一条聊天。** 它保存 mosaic、Pane、subtab、cwd、
  scrollback、浏览器和编辑器状态。
- **Library/Vault 把“找回工作现场”做成正式能力。** 保存布局、恢复最近关闭内容、自动快照、
  跨适配器全文搜索历史 Session。
- **运行配置与角色配置分开。** Launch profile 管模型、思考强度、权限和启动界面；named agent
  管角色提示和技能。
- **能力诚实降级。** 模型、思考强度、Rewind、图片、steering 等只在适配器报告支持时出现。
- **Agent 活动是独立控制面。** 后台、等待输入、权限请求、崩溃和限额状态集中可见，不要求
  用户逐个翻 Session 查找。

### 3.2 Codeg 最值得优先补的并不是 Atrium 的全部功能

结合我们的实际场景，优先缺口是：

1. 原生 Session 标题、活动时间和外部变化的可靠自动同步；
2. 与物理目录解耦的 Collection / 主题树和快速访问；
3. 多套命名 Workbench、重启恢复和真实系统多窗口；
4. Tab 拖边吸附、拖出新窗口、最大化和布局操作；
5. “最近关闭 + 已保存工作台 + 跨 Harness 历史全文搜索”的 Library/Vault-lite；
6. 后台 Agent 与待用户处理事项的统一 Activity 视图。

Team、群聊、项目记忆、任务看板和自动 Workflow 都可以后做。Atrium 自己也没有真正的共享
群聊：它的 Room 是布局容器，Agent 协作仍是带作用域的点对点消息。

### 3.3 Codeg 已经比 Atrium 更合适的部分

- Codeg 开源、跨 Windows/macOS/Linux，并有 Desktop、Server/Web 和移动端；Atrium 当前是
  闭源、macOS-only。
- Codeg 的一套分屏可以混放不同 `folder_id` 的 Session；Atrium Room 隶属于单一 Project。
- Codeg 已实现能力受限的 ACP `session/fork`；Atrium 文档只有 Claude checkpoint Rewind，
  没有通用分叉模型。
- Codeg 已有较广的内置 Agent 与自定义 ACP 接入，不需要复制 Atrium 的适配器注册表。
- Codeg 已有 `@` delegation、任务代理和跨客户端事件同步。我们缺的是“长期 Session 成员关系和
  可管理通信”，不是从零发明多 Agent 调用。

## 4. Atrium 的领域模型，以及为什么不能原样复制

```text
Workspace             命名的 Project 集合；自己不代表目录
└── Project            一个物理目录
    └── Wing           Room 的可选分组；一个 Room 只能属于一个 Wing
        └── Room       一个持久工作台 / 布局
            └── Pane   Agent Session、Terminal、Editor、Browser 等
```

这个层级在单个项目内非常清楚，但不完全符合我们的需求：

- 我们希望一个 Workbench 可以混放不同目录、不同 Harness 的 Session；
- 我们希望 Collection 像文件夹一样多层嵌套，并允许同一 Session 出现在多个 Workbench、
  快速视图和群聊；主要 Collection 本身保持唯一，避免树中重复归档；
- Team 是通信关系，不应因为几个 Session 摆在一个 Room 就自动建立；
- Session 是长期事实，Workbench 只是它的一种显示和恢复方式；
- 物理 `Folder`/cwd 应继续作为 Execution Context，不应变成语义分类树。

因此更适合 Codeg 的模型仍是：

```text
Session  ──运行于──> Folder / Execution Context
   │
   ├──唯一主要位置──> Collection
   ├──可显示在多个──> Workbench
   └──必要时加入──> Group Conversation（轻量群聊）

App Window ──可打开多个──> Workbench
Workbench ──同一设备只挂载到一个──> App Window
```

Atrium 的 Wing/Room 可以作为侧栏、拖拽和恢复交互的参考，但不能替换这个模型。

## 5. 完整功能族对照

标记说明：`✅` 已有或总体更强；`◐` 有底座但不完整；`❌` 当前没有；`↔` 路线不同，不能直接
判定为缺陷；`—` 对我们的近期目标价值很低。

### 5.1 组织、布局与召回

| Atrium 能力 | Codeg 0.25.0 | 判断 | 我们是否需要 |
|---|---|---:|---|
| Workspace：命名的 Project 集合、记忆焦点 | Folder 侧栏主要按物理目录组织 | ◐ | 用 Collection + Workbench 表达，不照搬 Workspace |
| Project：绑定一个目录 | `folder`/cwd 已成熟 | ✅ | 保留为 Execution Context |
| Wing：Room 文件夹、折叠、拖拽、最近焦点 | 无 Room/Wing 实体 | ❌ | Collection 树可借鉴交互；Session 主要归属保持唯一 |
| Room：持久 mosaic 工作现场 | 当前只有一套设备本地全局布局 | ◐ | P0：命名 Workbench |
| Room 自动命名 | 标题来自 Harness 或首条 Prompt；Codex 仍有缺口 | ◐ | 原生名 > 人工锁定 > 本地生成 > Prompt 兜底 |
| 二叉 mosaic 任意分屏和比例 | 已有 `groupLayout` 嵌套分屏和持久化 | ✅ | 复用，不重写 |
| Pane subtab 分组 | 每个 split group 已可容纳多个 Tab | ✅ | 复用 |
| 拖 Pane 到边缘创建 split | 可跨组拖动，但缺边缘 drop zone | ◐ | P0/P1：#456 |
| Pane 最大化、交换、等分、锁比例 | 部分布局操作，未形成 Atrium 完整交互 | ◐ | 最大化/等分优先，其他按反馈补 |
| Popout overlay、Resident dock | 无等价完整机制 | ❌ | 可后置，不是核心 |
| 水平 Room Tab / 垂直 Room Sidebar | 只有 Folder/Session 侧栏和主 Tab | ◐ | Workbench 切换器 + Session Library 优先 |
| Favorites、settled/hidden、运行状态徽标 | 有 pinned/opened/status 的部分基础 | ◐ | 纳入 Quick Access |
| Library：保存 Room/Pane 模板 | 无正式 Library | ❌ | 先做“保存/复制 Workbench” |
| 最近关闭 Room/Pane 并完整恢复 | 关闭 Tab 不删 Session，但无完整状态历史 | ◐ | P1：最近关闭与撤销 |
| Vault：全 App 快照、预览差异、安全恢复 | 有常规数据备份，缺工作现场快照系统 | ◐ | P1/P2：先做可恢复 Workbench 快照 |
| 跨适配器历史全文搜索 | 目前主要搜标题/目录等元数据 | ❌ | P1，高价值；比项目记忆更基础 |

### 5.2 Harness、Session 和聊天界面

| Atrium 能力 | Codeg 0.25.0 | 判断 | 我们是否需要 |
|---|---|---:|---|
| 调真实 CLI，不重新实现 Agent | ACP/CLI 适配路线一致 | ✅ | 坚持这个原则 |
| 10 个官方 Adapter | 多个内置 Agent + 自定义 ACP | ✅ | 不以数量竞赛，验证常用 Harness 即可 |
| Terminal 与 structured chat 两种 surface | 以结构化 GUI 为主，也有 Terminal | ◐ | 若同 Session 切换 surface 成本可控则 P2 |
| 原生 Session resume | 已支持导入、连接和继续多种原生 Session | ✅/◐ | 对每个 Harness 建回归矩阵 |
| 外部 Session 自动发现和增量刷新 | 需要手动导入，自动监听仍缺 | ❌ | P0：#458 |
| 模型、effort、权限的 capability-gated 配置 | ACP config 已有基础，依适配器能力 | ✅/◐ | 统一展示并诚实降级 |
| 图片、工具卡、Diff、Approval、Plan、Todo | 已有结构化消息与工具 UI，覆盖度依 Agent | ◐ | 保持 Harness fidelity，逐个补齐 |
| 长对话虚拟化、搜索、Prompt rail、Reader mode | 有消息列表和性能优化，交互不完全相同 | ◐ | 性能和搜索优先，Reader mode 后置 |
| 中途 queue、steer、interrupt | 已有队列/停止等运行控制，依协议能力 | ◐ | 做清晰状态，不强行统一语义 |
| Context 用量、Compact、供应商 quota | 部分 Agent 可显示，缺统一控制面 | ◐ | P1/P2 |
| Claude checkpoint Rewind | Codeg 依 Harness/协议能力 | ↔ | 只在真实支持时暴露 |
| 通用 Session Fork | Atrium 无公开通用 Fork；Codeg 有 ACP fork | ✅ | Codeg 保留优势，修好谱系和命名 |
| Chat 与 Terminal 之间保持同一 Session | 未形成所有 Agent 通用切换体验 | ❌/◐ | 有用但不是第一批 |
| 本地自动 Room 命名 | 0.283 包含 `atrium-room-namer` | ◐ | 可做本地兜底，绝不覆盖人工锁定名 |

Atrium 0.283 安装包新增独立的 `Contents/Helpers/atrium-room-namer`，说明自动标题是本地
辅助能力而非必须依赖 Harness。对 Codeg 更合理的优先级是：人工锁定名最高，随后使用原生
Session 名，再以本地生成标题和首条 Prompt 兜底。

### 5.3 Agent 活动与协作

| Atrium 能力 | Codeg 0.25.0 | 判断 | 我们是否需要 |
|---|---|---:|---|
| Activity fleet：按 Project 看全部 Agent 状态 | 有运行状态与后台提示，但不是完整 fleet 控制面 | ◐ | P1：等待用户、权限、崩溃、限额最有价值 |
| 后台运行、重新附着、关闭 UI 后由 daemon 持有 | 有服务端/运行时底座，桌面生命周期不同 | ↔ | 先保证切 Workbench 不终止 Session |
| Launch profile：模型/effort/权限/surface/env | 设置分散于 Agent/会话配置 | ◐ | P2：和角色定义解耦 |
| Named agent + skills | 有 custom agent、skills、delegation | ✅/◐ | 复用现有实体，不新建重复事实源 |
| Agent discovery scope | 无同构的 same-room/same-workspace/all 可见域 | ❌ | AgentBus/Team 实施时需要明确作用域 |
| Agent 到 Agent 点对点消息 | `@` delegation 与 AgentBus 可覆盖不同场景 | ◐ | 把 AgentBus 纳入 UI，而不是另造私有通道 |
| 人从 Activity 卡片向 Agent 发消息 | 可进入 Session 发送，缺统一消息入口 | ◐ | P2，可从 Session 跳转先满足 |
| Agent 启动另一个 Agent | delegation/MCP 已有 | ✅ | 已有优势 |
| 共享 Room 群聊、全员共同 transcript | Atrium 也没有；Codeg 也没有 | ↔ | P2/P3，先做定向消息和显式广播 |
| 持久 Session Team 成员关系 | 两者都不完整 | ❌ | #461；不阻塞 Session Workbench MVP |

Atrium 的 `atrium agent message` 会把点对点消息包装成新的 user turn，接收方按命令回信；
发送方是 fire-and-forget。Room 只决定发现范围和布局，不会自动把一条消息广播给所有 Pane。
这证明“群聊”与“工作台”必须分开建模，也说明我们的 AgentBus 可以作为传输层继续保留。

### 5.4 Timeline、Memory、Tasks 与自动化

| Atrium 能力 | Codeg 0.25.0 | 判断 | 我们是否需要 |
|---|---|---:|---|
| Timeline：Prompt、回复、Commit、Task、Note 的统一时间线 | 无跨 Session/资源的同构时间线 | ❌ | P2；先有可靠 Session 索引再做 |
| 按 Workspace/Room/Pane 搜索与生成 Brief/Recap | 无同构能力 | ❌ | 可从 Workbench/Collection 范围摘要开始 |
| 项目/全局/Agent Memory，分类、FTS、年龄 | 无同构正式 Memory | ❌ | 后置；Session 搜索先行 |
| Memory supersede，保留旧事实 | 无 | ❌ | 若做项目共识，必须有来源、版本和废弃链 |
| 离线 embedding 与 consolidation | 安装包带 MiniLM ONNX；Codeg 无同构 | ❌ | 不因 Atrium 有就照搬，先验证必要性 |
| 自我改进 agent/skill diff + trial/revert | 无同构完整流程 | ❌ | —，不是当前 Session 管理目标 |
| 内置 Task 看板、优先级、标签、评论、运行历史 | Codeg 有 coding-oriented `work_task` | ↔ | 不把长期主题强塞进任务状态机 |
| Task 绑定 Agent/skill/profile、重复调度、done-when | automations/work task 有部分基础 | ◐ | 保留 coding/automation 用途，和 Collection 分开 |

Atrium 对“过时记忆”的回答是 age、scope、supersede 和显式 consolidation，而不是假设一份项目
摘要永远正确。这一点值得保留为未来约束，但不应该在 Session 分类和 Workbench 尚未稳定时先做
复杂知识层。

### 5.5 富 Pane 与工作环境

| Atrium Pane/能力 | Codeg 0.25.0 | 判断 | 我们是否需要 |
|---|---|---:|---|
| Terminal | 有 | ✅ | 已满足 |
| Editor、Markdown、Diff、Git | 有 Monaco、文件、Diff、Git 等 | ✅/◐ | 不作为本轮重点 |
| Browser Pane、历史和自动化 | 无同构完整内嵌浏览器 | ❌ | 用户已有 IDE/浏览器，P3 |
| Notepad：Markdown、Sketch、Canvas、沙箱 HTML | 无同构一体化 Notepad | ❌ | —/P3，不把聊天做画布 |
| Repo search/replace | 有文件能力但非同构独立 Pane | ◐ | 依真实使用反馈 |
| Tasks Pane | 有任务/自动化页面 | ↔ | 保持现有路线 |
| Image/PDF/Video/Diff/Snapshot viewers | 有图片、Markdown、Office 等预览，覆盖不同 | ◐ | 读图体验重要，按常用格式补齐 |
| Observatory、Achievements | 无 | ❌ | —，不属于核心需求 |

Atrium 的富 Pane 展示了“Session 旁可以挂资源”的可能性，但我们的第一目标不是重做 IDE。
Codeg 只需保证 Workbench 将来可以保存 Session 之外的资源 Tab，并让资源明确选择
Session/Workbench/Window 作用域。

### 5.6 持久化、平台和可扩展性

| Atrium 能力 | Codeg 0.25.0 | 判断 | 我们是否需要 |
|---|---|---:|---|
| 原子状态写入、轮换快照、崩溃恢复选择器 | 有数据库和备份底座，缺完整工作现场恢复策略 | ◐ | P1：Workbench 迁移和恢复必须可回退 |
| 每 Project 保存 Room/Pane/cwd/scrollback/session | 会话在 DB，分屏主要存本地单一 key | ◐ | 把布局所有者改为 Workbench |
| daemon 收养仍在运行的 Session | 架构不同，Desktop/Server 已有后端 | ↔ | 先定义 UI 关闭与运行时生命周期 |
| CLI 覆盖 Workspace/Room/Pane/Agent/Task 等命名空间 | 有 CLI/MCP/后端 API，但产品对象不同 | ◐ | 新 Workbench/Collection 也应有可编程入口 |
| `atrium://` 协议、hooks、诊断、数据目录 | Codeg 有自身 IPC/API/日志/配置 | ◐ | 复用现有能力，不追求 API 同形 |
| Adapter SDK | 自定义 ACP/Agent 配置 | ✅/↔ | ACP 优先，避免维护大量私有桥接 |
| macOS 原生桌面 | Codeg 跨平台 | ✅ | Windows 是我们的硬要求 |
| 多客户端同步 | Atrium 主要本机 daemon；Codeg 有 Web/Server/Mobile | ✅ | Codeg 的明显优势 |
| 开源 | Atrium App 闭源；Codeg 开源 | ✅ | 便于我们长期维护和魔改 |
| 主题、快捷键、字体、缩放、壁纸、语音输入 | Codeg 有部分外观/快捷键能力 | ◐ | 低于 Session 管理优先级 |

## 6. 不要照搬 Atrium 的部分

1. **不要把 Workbench 绑定到单一目录。** Atrium Room 属于 Project；Codeg 当前 Tab 本身已经
   携带 `folder_id`，跨目录混放是我们的重要优势。
2. **不要把 Atrium Wing 的单层归属原样搬来。** 第一阶段采用可嵌套、唯一主要位置的 Collection；
   收藏、搜索、Workbench 和后续显式快捷方式提供多处引用，不默认开放含义含混的多 Collection 归属。
3. **不要把 Room 当作群聊。** 布局共处、共享 transcript、Team 成员关系和消息广播是四件事。
4. **不要先做 Git/worktree/Task 自动化。** 这些是 Atrium 的 coding 偏好，不是长期研究 Session
   管理的必要前提。
5. **不要先做 Memory 和自动 consolidation。** 缺少可靠来源、版本和 supersede 时，所谓共识会
   很快变成过时事实。
6. **不要重做每个 Harness。** 保留原生 Session 身份、resume 和能力协商；UI 只做统一控制面。
7. **不要把所有富 Pane 都塞进 MVP。** Browser、Canvas、Observatory、Achievements 对我们当前
   痛点的收益远低于自动同步、Collection 和 Workbench。
8. **不要依赖模糊的“在线”状态。** 外部 App、AgentBus mailbox 和远程 Session 必须显示真实的
   可达性与领取状态。

## 7. 建议从 Atrium 吸收的实施顺序

### P0：可靠的 Session Library

- 修复并回归原生标题、活动时间、外部新增和继续后的自动同步；
- Collection/主题树、唯一主要归属、收藏、最近、运行中、等待处理、未分类；
- 搜索结果显示 Harness、模型、cwd、Collection 和活动时间；
- 逐步增加跨 Harness transcript 全文索引。

对应上游：#457、#458、#459。

### P1：Workbench 与布局召回

- 将当前单一 `workspace:tab-groups:v1` 迁移为命名 Workbench；
- 保存 Tab、split tree、比例、active tab、draft 和必要资源页；
- 拖边吸附、最大化、等分、最近关闭；
- 安全快照和崩溃恢复；
- 后续再拆到真实 App Window。

对应上游：#456、#460。

### P2：Activity 与受管理协作

- 聚合所有受管 Session 的 working/idle/waiting/permission/error 状态；
- 从 Activity/AgentBus 直接跳转和发消息；
- 可见 project/role/Session 绑定和待回复请求；
- Team 只保存持久 Session 成员和路由策略，不自动复制全部私聊上下文。

对应上游：#461 和 AgentBus 协作 RFC。

### P3：知识与富工作环境

- Timeline、带来源的 Decision/Memory、supersede；
- Browser/Note/更多 Viewer；
- 模板、Launch profile、工作流和更高级自动化。

这批必须由真实使用验证，不因为 Atrium 已经做了就进入承诺范围。

## 8. 对现有产品文档的影响

本次审计没有推翻现有产品模型，反而确认了以下边界：

- Session 是核心事实；
- Folder 是运行上下文，不是分类；
- Collection 是长期语义组织；
- Workbench 是可切换、可恢复的工作现场；
- App Window 是物理呈现；
- 轻量群聊是可选通信方式，优先级低于 Session 管理；Team 暂不作为用户可见层级；
- Memory 是 Session 索引之后的派生知识层。

Atrium 新增或变化的功能应先更新本文，再判断是否修改产品需求。不能把竞品 Changelog 直接
变成本项目 Backlog。

## 9. 公开文档覆盖范围

本次镜像并审阅了官方 sitemap 中全部 `/docs` 页面，覆盖：

- 安装、onboarding、quickstart；
- Workspace、Project、Room/Wing、Pane、Agent/Adapter、Task、Timeline、Memory、Persistence、Daemon；
- Terminal、Agent Chat、Editor、Markdown、Notepad、Browser、Git、Search、Tasks、Library/Vault、
  Observatory、Achievements、Viewer；
- Agent account、activity、launching、messaging、sigil、task dispatch；
- Adapter overview/building；
- appearance、theme、keybinding、dictation、adapter environment；
- CLI、configuration、data directory、diagnostics、protocol、shortcuts、telemetry。

快捷键、样式选项和每个 Pane 的细枝末节没有逐条转写进本页；原始 HTML 与可检索正文已完整
保留在外部 artifact 中，后续遇到具体设计问题可回到证据快照，而不需要重新依赖网页当前状态。
