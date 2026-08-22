# codeg 看板设计（历史定稿，2026-08-21；Backlog 边界于 2026-08-22 修订）

依据：`O46-TASKBOARD-RESEARCH.zh-CN.md`（组织维度）、`O63-AGENT-TASK-INTERFACE-RESEARCH.zh-CN.md`
（别人怎么让人和 agent 共用任务系统）、`TASKBOARD-RFC-2026-08-21.zh-CN.md`（缺口清单）。

用户原话：「我以为是维护一套任务系统，然后有前端，也有 mcp 给 agent 用。」

## 0. 一句话结论

**codeg 已经是那套系统了，只是三个口子没开，所以看起来像"新建任务"按钮。**
不要造第二套任务系统——`work_task` 的权威来自状态机 + git 合并 + CAS，
把卡改成仓库里的 markdown（Trellis / Backlog.md 那一派）会和 `run_seq`、merge 泵抢权威。

## 1. 四条设计原则

### ① 一级容器是项目 Folder，不是 Collection / 群聊 / 工作台

硬约束，不是口味（证据见 O46 调研）：
- `work_task` 唯一必填的容器就是 `folder_id`，且**只许项目根**（`work_task_service.rs:429`）。
- 引擎不变量按 folder：合并互斥槽、`sort_order` 队列。
- Collection 有根时不能跨 folder；Room 硬挂 workbench 且成员可跨仓；Workbench 表里根本没有 folder 维度。

对照物 vibe-kanban / Multica 的一级也都是 Project。**Collection 只能做项目内的二级分段。**

### ② 卡片既能保存想法，也能承诺执行；Backlog 与 Todo 分开

用户复核实际用法后推翻了“`todo` 就是 backlog”的旧裁决。两者仍是同一张 `work_task` 卡，
不是第二套任务系统，但拥有两个稳定业务状态：

```text
backlog = 原始想法 / 未承诺；可预先归属，但绝不启动
todo    = 已梳理、可执行；仍需显式开始
```

`engine.rs` 的显式 start/claim 边界继续保留。Backlog 指派不得创建 PromptQueue 项；Todo 也
不允许后台自动 pickup。人或 Agent 必须通过可审计的“开始/指派并开始”动作才进入执行队列。

### ③ agent 侧读写不对称，必须补"读"

今天 agent 只能写（`create_work_task` 建卡、`task_progress` / `task_complete` 汇报**自己那张**卡，
后两者靠 per-launch token 绑定，改不了别人的卡）。**没有任何 list / get。**

补齐方案（O64 施工中），采纳 Backlog.md 的「list 摘要 / get 全文」+ Conductor 的「默认只给邻域」：

| 工具 | 返回 | 边界 |
| --- | --- | --- |
| `list_tasks` | 紧凑行：`id / title / status / agent_type / updated_at / has_worktree / conversation_id?` | 默认只看**调用者自己的项目**（worktree 自动跳项目根）；`limit` 默认 30、硬上限 100；**绝不返回 prompt 正文、config、事件流** |
| `get_task` | 单卡放大：加 `prompt_excerpt`(≤1000 字) / 分支 / `last_error`(≤500 字) / 最近 10 条事件（每条 ≤200 字） | 每一项都有硬上限 |

**明确不做的写工具**：`update_task` / `set_status` / `next_task`。
理由：状态机是 CAS + 合并泵收口的，让模型 PATCH 别人的 status 会打穿 `queued/preparing/merging`；
`next_task` 是引擎的 claim/pump 职责，不该有第二个调度器。
Multica 允许 agent 任意改别人的卡——**这条不抄**。

### ④ 防止看板退化成第二个群聊

调研点出的两个真实失败模式，设计上各有对策：

| 风险 | 对策 |
| --- | --- |
| 上下文膨胀（list 变 dump，agent 循环 list→评论→再 list） | list 只给一行摘要 + 硬上限；`get_task` 默认**不**返回事件流；工具 schema 本身也要短（Taskmaster 36 个工具 ≈ 21k token 的教训） |
| 抢活（两个 session 同时对同一需求建卡） | 抢活发生在**决策层**不是 SQL 层。list 每行带 `status` 和 `conversation_id`，工具描述明写"这些卡可能已有别的会话在做，不要重复建卡"；真要抢活必须走引擎侧 CAS claim，不给模型直接改状态 |

## 2. 前端还缺的三件（按价值排序）

1. **分组维度**：无（默认）/ 按项目 Folder / 按 agent。做成**列内二级分段**，四列地基不动。
   Collection 与 Room 不进一级分组。
2. **attention 列分层**：一列里挤着"等你回答 / 等你验收 / 正在合并 / 已失败"四种语义。
   甲案（推荐）：不拆列，列内按严重级排序 + 视觉分层（failed 红、awaiting_input 琥珀、
   review 中性、merging 弱化）。拆第五列会动到拖拽与双视图共享的地基。
3. **卡片活动点**：running 且连接在 Prompting → 绿色脉冲；running 但不在 → 静止点。
   数据源复用 `acp-connections-context` 的连接快照，纯前端推导。

已顺手修：全部项目视图下 To-do 拖拽静默失效（顺序按项目存，跨项目没有顺序可存）——
现在列头说明原因（O62）。

## 3. 明确不做

- 不引入外部任务系统（Trellis / Multica / Taskmaster）：我们的状态机更细，缺的是仪表盘器官不是引擎。
- 不把卡片改成仓库 markdown。
- 不做 Room 编队进 work_task：两者生命周期语义不同，硬统一会造出第二个"双时间线"问题。
- 不改 Engine 10 态状态机、不改 CAS 转移不变量；业务看板在其上增加独立 `backlog`，形成
  五列稳定投影，不把 Backlog 变成后台自动抢活队列。
