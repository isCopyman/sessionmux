# Session Center、全文检索与打开行为子 RFC

> 状态：Draft  
> 日期：2026-08-15  
> 适用范围：Codeg Session 发现、搜索、筛选、打开、Workbench 加入和 Focus Session 行为  
> 依赖：[产品需求](./PRODUCT-SPEC.zh-CN.md)、[领域模型](./DOMAIN-MODEL.zh-CN.md)、[主 RFC](./SESSION-WORKBENCH-RFC.zh-CN.md)

## 1. 问题

当本地存在大量 Claude、Codex、Gemini、OpenCode 等历史会话时，Collection 树和侧栏过滤只能
解决“浏览”，不能完整解决：

1. 用户只记得历史对话中出现过一句话，不记得标题、Harness 和路径；
2. 搜索结果来自多个 Harness，有些尚未导入 Codeg；
3. 找到 Session 后，不清楚点击会替换当前内容、加入 Workbench，还是切走整个 Workbench；
4. 多 Pane 状态下，“当前 Workbench”“当前 Session”和“放大当前 Session”容易被混为同一概念；
5. 管理、搜索、预览和真正 Resume Session 如果由同一个点击动作触发，会产生意外布局修改。

本 RFC 的目标是把这些行为收敛成一个可预测的 Session Center 和一套明确的打开决策表。

## 2. 决策摘要

1. 保留轻量侧栏和 `Ctrl/Cmd+K` Quick Open，另提供完整 Session Center；Quick Open 与 Session
   Center 共用元数据和正文搜索，不要求用户先选择搜索类型；
2. Codeg 自身索引负责 Session 身份、标题、Collection、Workbench、状态和运行配置；
3. ctx 是可选全文检索 Provider，只返回历史正文命中、片段和来源，不接管 Codeg 数据模型；
4. 所有真正打开的内容都属于某个 Workbench；不引入“游离在 Workbench 外的 Session”状态；
5. 默认打开不会替换已有非空内容：已打开则聚焦，未打开则加入当前 Pane 的内容标签；
6. 新分屏、新 Workbench、新窗口和后台加入都必须由显式动作触发；
7. Session Center 单击只选择并预览，双击、Enter 或“打开”才把 Session 加入当前 Workbench；
8. “只看当前 Workbench”是 Library Scope；“Focus Session”是临时放大当前 Pane，二者都不改数据。

## 3. 三个入口，共用一个搜索模型

### 3.1 Session Library

左侧长期可见，用于 Collection 浏览、快捷视图和少量筛选。它不承担复杂全文结果展示和批量
管理。单击 Session 直接执行默认打开行为。

### 3.2 Quick Open

继续使用 Codeg 现有 `Ctrl/Cmd+K` 入口，既支持“我知道大概叫什么”，也支持“我只记得聊过的
一句话”：

```text
┌ 搜索 Session ─────────────────────────────────────────┐
│ 方案审查                                               │
├───────────────────────────────────────────────────────┤
│ 标题与属性                                             │
│  Claude  结构讨论           Collection A     2 小时前  │
│  Codex   方案核对           Collection A       4 天前  │
│ 正文命中                                               │
│  Gemini  ……这个判断需要区分运行目录与语义分类……  3 天前 │
└───────────────────────────────────────────────────────┘
```

Quick Open 向元数据索引和可用的正文 Provider 发出同一个查询。标题、Collection、Harness、cwd
等 Codeg 元数据先返回，正文命中异步补充。结果以 Session 为单位去重；一段长会话即使命中多处，
也只占一个主结果，并显示最相关片段和命中数量。正文结果不得挤掉明显的标题命中。

第一阶段尚未接入正文 Provider 时，Quick Open 可以只查询元数据；接入 Phase 3 后，正文搜索直接
出现在同一入口，不再要求用户跳入 Session Center。Enter 执行默认打开，修饰键或右侧菜单提供
“在侧边打开”“在新 Workbench 打开”等动作。

### 3.3 Session Center

Session Center 是 App Window 级管理页面，不是 Workbench 的 Content Tab，也不进入工作台布局。
关闭后回到原 Workbench 和原活动 Session。推荐三栏：

```text
┌ Filters ─────┬ Results ─────────────────┬ Preview / Properties ──────┐
│ Scope        │ 标题、Harness、时间       │ 会话摘要、正文命中片段      │
│ Collection   │ Collection、cwd           │ 所在 Workbench             │
│ Workbench    │ 状态、命中数量             │ 原生身份与打开动作           │
│ Harness      │                           │                            │
└──────────────┴───────────────────────────┴────────────────────────────┘
```

它承担：

- 全局标题、元数据和正文搜索；
- 多条件筛选、排序、分组和 Saved View；
- 批量移动 Collection、收藏、归档、加标签；
- 查看 Session 出现在哪些 Workbench；
- 预览命中片段而不启动 Harness、不改变 Workbench；
- 显式打开、加入当前 Workbench、侧边打开、新建 Workbench 或新窗口打开。

三个入口复用同一套 Session 结果、搜索 Provider 和打开动作，避免侧栏、Quick Open、管理弹窗
各有一套查询及点击语义。Session Center 只额外展示完整筛选、更多正文命中、属性和批量管理数据。

## 4. ctx 集成边界

### 4.1 为什么适合

本机 ctx 已能索引 Codex、Claude 等多种本地历史。其默认 lexical 后端使用 SQLite FTS/BM25；
本地 semantic sidecar 完整时可选 hybrid/semantic。`ctx search --json` 提供 `schema_version: 1`
机器契约，包括：

- `ctx_session_id`、`ctx_event_id`；
- provider 与 `provider_session_id`；
- title、snippet、rank、timestamp、cwd；
- source path/cursor 与原文件是否仍存在；
- 一个 Session 中是否还有更多命中；
- 索引 freshness 和 semantic 降级原因。

这比 Codeg 为每种 Harness 重新解析全文并设计统一 FTS 更适合做第一版可选全文搜索。

### 4.2 不把 ctx 变成事实源

ctx 只提供 Search Hit。下列事实仍由 Codeg 掌握：

```text
Session identity       → conversation + (agent_type, external_id)
Collection membership  → Codeg Collection
Workbench membership   → Codeg View Instance / opened tab
Runtime and resume     → Codeg Harness adapter
Search hit and snippet → optional ctx provider
```

优先使用 `provider + provider_session_id` 映射到 Codeg Conversation；必要时用经过规范化的
source path 辅助定位，但路径不是长期主键。找不到映射时，结果显示为“外部已索引 Session”，
用户可显式导入并 Resume；导入仍走 Codeg 现有去重逻辑。

### 4.3 接口选择

第一版建议增加通用 `HistorySearchProvider`，ctx 适配器通过受控子进程调用：

```text
ctx status --json
ctx search <query> --json --refresh off --limit <n>
ctx show event <ctx_event_id> --format json
```

选择 CLI JSON 而不是直接读取 ctx 内部 SQLite 表：

- CLI 是 ctx 的主要接口，JSON v1 是明确的机器契约；
- 可以获得 freshness、retrieval 和稳定 ID；
- 不绑定 ctx 内部表结构；
- 后续可在用户启用 semantic 后使用 hybrid。

ctx MCP 适合 Agent 工具调用，但当前 MCP 搜索只查询已有索引、只走 lexical，且 Codeg 本身不是
必须经过 MCP Host 才能调用本地工具，因此不作为桌面搜索的首选集成。稳定 `ctx_*` SQL view 只
用于诊断或统计，不作为交互搜索主路径。

### 4.4 性能、刷新与降级

- Codeg 元数据搜索先返回；ctx 查询异步、可取消，并设置硬超时；
- 输入防抖，旧查询结果不得覆盖新查询；
- 前台默认 `--refresh off`，保证搜索不触发导入或额外写入；
- 索引刷新由用户显式操作或可选 ctx daemon 承担，UI 显示最后刷新和 stale/failed 状态；
- ctx 不存在、版本不兼容或查询失败时，只隐藏“全文命中”，不影响 Codeg 元数据搜索；
- semantic 未启用时明确显示 lexical，不把关键词检索宣传成语义搜索；
- 搜索输出包含私有正文和本地路径，不写普通应用日志，不上传远端。

## 5. 默认打开行为

### 5.1 Workbench 是所有打开内容的容器

Codeg 始终至少有一个默认 Workbench。因此“直接打开 Session”的准确含义是：

> 在当前活动 Workbench 的当前聚焦 Pane 中打开一个持久 Content Tab。

不创建独立的“非 Workbench 模式”。Workbench 成员第一阶段就是其已打开的 View Instance，
不额外维护一份“属于 Workbench 但目前隐藏”的成员表。关闭 Content Tab 只移除该 View Instance，
不删除 Session。

### 5.2 打开决策表

| 情况 | 默认行为 |
|---|---|
| Session 已在当前 Workbench | 聚焦现有 View Instance，不重复打开 |
| Session 不在当前 Workbench | 作为固定 Content Tab 加入当前聚焦 Pane 并持久化 |
| 当前只有一个未发送、未修改的空白 Draft | 可以安全复用该空白 Tab |
| 当前 Pane 已有其他 Session | 新增同组 Content Tab，不替换已有固定 Tab |
| Session 只在其他 Workbench 中 | 默认仍加入当前 Workbench；结果上的 Workbench 徽标可用于切过去 |
| 没有活动 Workbench | 使用或创建默认 Workbench 后打开 |

默认不因一次点击自动分屏，因为连续查看多个结果会制造大量 Pane。以下动作保持显式：

- 在右侧/下方打开；
- 拖到 Pane 边缘建立分屏；
- 加入当前 Workbench 但不切换焦点；
- 在新 Workbench 打开；
- 在新 App Window 打开；
- 仅切换到已包含该 Session 的另一个 Workbench。

可保留 IDE 风格的“单击预览并替换上一个预览”作为用户设置，但不作为默认值。任何非空固定
Tab 都不得被普通点击静默替换。

### 5.3 不同入口的手势

| 入口 | 单击 | 双击 / Enter | 其他动作 |
|---|---|---|---|
| Session Library | 默认打开或聚焦 | 同单击 | 中键可后台加入；拖动可指定 Pane |
| Quick Open | 移动选择 | 默认打开或聚焦 | 菜单选择侧边、新 Workbench、新窗口 |
| Session Center | 只选择并在右侧预览 | 默认打开或聚焦 | 批量管理与所有显式打开方式 |

Session Center 的 Preview 是管理页自己的只读预览，不是 Workbench View Instance，不触发 Resume。

## 6. 当前 Codeg 行为与改造点

截至 2026-08-15，当前代码已有：

- `search-command-dialog.tsx`：`Ctrl/Cmd+K` 默认搜索全部 Folder，可一键缩小到当前 Folder，并支持
  Harness 过滤；
- `session_search.rs`：通过 `ctx search --json --refresh off` 查询正文，按
  `(agent_type, external_id)` 映射回可打开的 Codeg Session，同一 Session 只展示最佳命中；
- 正文搜索设有输入防抖、查询竞态保护、15 秒超时和安全降级，不会因 ctx 缺失而破坏标题搜索；
- `conversation-manage-dialog.tsx`：已经升级为侧栏可直接进入的全局 Session Center；一个搜索框可
  切换“标题 + 正文 / 标题与元数据 / 会话正文”范围，并支持 Folder、分支、Workbench、Harness、
  状态筛选、独立归档/恢复和批量状态/删除，也可把多条 Session 一次加入当前
  Workbench；单击只读预览最近历史，双击、Enter 或明确按钮才真正打开；窄窗口在列表和预览之间
  切换，不让双栏挤压正文；
- `list_conversation_workbench_refs`：批量返回 Session 当前出现在哪些已保存 Workbench；预览中的
  归属标签可切到目标 Workbench 并聚焦同一 Session，查询失败只降级归属信息，不阻断会话列表；
- `sidebar-conversation-list.tsx`：单击调用 `openTab(..., pin=false)`，双击传 `pin=true`；
- `tab-store.ts`：未固定 Tab 是每个 Pane 的 preview，下一次单击会替换该 Pane 内的旧 preview；
- Quick Open 选中结果时传 `pin=true`，行为与侧栏单击不一致。

这解释了用户看到的“有时打开、有时替换”。第一阶段应：

1. 统一侧栏和 Quick Open 的默认打开动作为固定 Tab；
2. 保留 `isPinned=false` preview 原语，但只给显式启用 Preview Mode 的用户使用；
3. 继续复用已经落地的 Library Scope、只读预览、Workbench 条件筛选、批量加入、独立 Session
   归档、Collection 子树筛选/批量移动与 ctx Provider，后续补收藏；`completed` 仍只是进度状态；
4. 视使用密度决定是否把当前全局 Dialog 提升为独立路由；名称和查询行为已经统一，不再保留第二套
   “管理会话”入口；
5. 已导入结果现已展示“当前 Workbench”“其他 Workbench”“未在已保存 Workbench 打开”；后续补
   “尚未导入”状态；
6. 后续再支持 ctx 已索引但尚未导入 Codeg 的外部 Session；当前正文结果只返回能够安全 Resume 的
   已导入 Session。

## 7. Active、Scope 与 Focus Session

这些词必须分开：

| 概念 | 含义 | 是否持久改变工作台 |
|---|---|---|
| Active Workbench | 当前窗口顶部选中的 Workbench | 否，只改变当前选择 |
| Active Session | 当前接收键盘输入和上下文面板跟随的 Session | 否 |
| Current Workbench Scope | Session Library 只显示当前 Workbench 成员 | 否，只是过滤 |
| Focus Session | 临时放大当前 Session View，隐藏其他 Pane | 否 |

Focus Session 类似 Pane Zoom：原布局完整保留，其他 Session 继续运行，再次执行或按 Escape 即
恢复。它属于 App Window 的临时 UI，不跨设备同步，默认不在重启后恢复。收起侧栏或 Zen Mode
只是普通显示偏好，不再定义第二个 Focus Workbench 模式。

## 8. 分阶段实施

### Phase 1：统一打开语义

- 侧栏和 Quick Open 默认固定打开；
- 已在当前 Workbench 时只聚焦；
- 不静默替换非空固定 Tab；
- 增加打开到侧边、新 Workbench 和后台加入动作；
- 增加决策表回归测试。

### Phase 2：Session Center

- 合并现有搜索和管理 facet；
- 全局 Scope、三栏管理页和批量操作；
- 显示 Collection、Workbench、Harness、cwd、状态与活动时间；
- 管理预览不启动 Runtime。

### Phase 3：ctx 全文检索

- 在 Quick Open 和 Session Center 中接入同一个 `HistorySearchProvider` 与 ctx CLI JSON 适配器；
- Codeg 元数据结果和 ctx 正文命中分区或融合；
- Quick Open 按 Session 去重并展示最相关正文片段与命中数量；
- Session 身份映射、索引状态和安全降级；
- 第一版打开 Session，不承诺精确跳转到每个 provider 的原始消息；
- provider cursor 与 Codeg Turn 能稳定映射后再增加“跳到命中位置”。

### Phase 4：Focus Session 与 Saved View

- Focus Session；
- 显式保存高频筛选组合，不自动制造 Saved View。

## 9. 验收标准

1. 只记得历史正文关键词时，用户能从一个入口找到跨 Harness Session；
2. ctx 不存在或索引失败时，标题、Collection、Workbench 和 Harness 搜索仍正常；
3. 侧栏、Quick Open 和 Session Center 对“打开”的含义一致；
4. 普通点击不会替换任何非空固定 Content Tab；
5. 已在当前 Workbench 的 Session 不重复打开，而是直接聚焦；
6. Session Center 单击预览不启动 Harness、不修改布局；
7. 直接打开的 Session 明确加入当前 Workbench，关闭 Tab 不删除 Session；
8. 用户能显式选择侧边、新 Workbench、新窗口和后台加入；
9. Current Workbench Scope 和 Focus Session 在文案与行为上不混淆；
10. ctx 查询可取消、超时可恢复，私有正文和本地路径不进入普通日志。
