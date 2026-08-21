<!-- 来源：grok 只读调研（cli-delegate, job run-mt2iudlo-ylbce3），2026-08-21。
     调研对象：Multica / OpenAgents / Trellis / Backlog.md / Conductor / Taskmaster。
     未能访问的部分工人已显式标注，未编造。-->

# 人与 Agent 共用任务系统：读写接口调研

codeg 现状（对照基线，不是调研对象）：任务在 **SQLite `work_task` 表**，`folder_id` 是项目根 folder、**永不挂 worktree**（`src-tauri/src/db/entities/work_task.rs:51-53`）。10 态 `todo→queued→preparing→running⇄awaiting_input→review→merging→done`（+failed/canceled），每次迁移是 **CAS**（期望 status + `run_seq`）（同文件 L4-10；`work_task_service.rs:5-8`）。MCP 伴生进程对 agent：**写** `create_work_task` / `task_progress(message)` / `task_complete(verdict, summary)`；后两者由 **per-launch token 解析到本卡**，不能指定别人的 id（`companion.rs:140-193,826-876`；`work_task_tools.rs:39-51`）。人侧已有 `work_task_list` / `work_task_get`（`commands/work_task.rs:56-71`）。

---

## 1. Multica

**身份：** 自托管「agent 当队友」看板（PostgreSQL + Go）。[README](https://github.com/multica-ai/multica/blob/main/README.md)

**任务在哪：** **全局 workspace DB（PostgreSQL）**，不跟 git 仓库走。Project 挂 repo；issue 是 workspace 对象。架构图：Next.js → Go → Postgres。官方文档页 `multica.ai/docs/{cli,tasks,assigning-issues,how-multica-works}` **未能访问正文**（只拿到标题）。

**Agent 怎么读：** 官方声明「Agents drive Multica through the same CLI you do」。**未核到官方 CLI 子命令签名**。社区 MCP 两套（均已读源/README）：

| 工具 | 入参（已核） | 返回（已核） |
|---|---|---|
| `multica_list_tasks` | `project_id? status? assignee? query? limit?`（默认 100） | JSON 任务数组 |
| `multica_get_task` | `task_id`（如 `MUL-123`） | 描述+状态+assignee+**comments+subtasks** |
| `multica_search_tasks` | `query` 必填；`project_id? status? limit?` | 标题/描述/**评论**全文 |
| `multica_list_issues`（Korkyzer） | `status[] project limit offset sort` | `{items, total, offset, has_more, next_offset}` |
| `multica_get_issue` | `issue_id, include_comments?` | 含 `comments, task`；可关评论 |

源：[`strider2038/multica-mcp` server.go](https://raw.githubusercontent.com/strider2038/multica-mcp/main/internal/mcp/server.go) L92-128, 331-345；[Korkyzer README](https://github.com/Korkyzer/multica-mcp)。官方 REST 被包成 MCP，不是 coding agent 直读文件。

**防撑爆上下文：** list 默认 cap 100；search 有 `limit`；Korkyzer **分页 + `include_comments:false`**。[SKILL.md §12](https://github.com/Korkyzer/multica-mcp/blob/feat/upstream-ready/SKILL.md) 明确：大 JSON 要压缩，list 不要当 dump。

**写：** `multica_update_task` 可改 title/description/status/priority/assignee（`status` 枚举：`backlog,todo,in_progress,in_review,done,blocked,cancelled`）；`dry_run`；`suppress_run` 避免误触发 agent。`assign_task` 可派给人/agent/squad。**没有看到乐观锁/version 字段**。所有权 = workspace 角色 + PAT；MCP 另有 `MULTICA_READ_ONLY`。Korkyzer：daemon 只 pickup **`todo` + assignee**；评论要走 MCP 才当 owner 触发（CLI 评论标 `authorType=agent` 会被忽略）。**未核到官方并发谁赢。**

**worktree：** README 称 runtime 在你机器上跑 CLI。`multica.ai/docs/project-resources` 摘要提到 worktree 在 **runtime workspace 目录、按清理周期回收、branch 留在 repo**。**不是「一张卡必须一个 worktree」的公开契约**；cwd 在 Korkyzer 里只是 markdown hint。

---

## 2. OpenAgents

**身份：** 「人+agent 协作 OS」：Workspace（浏览器线程/文件/任务板）+ Launcher。[概览 2026-08-21](https://openagents.org/docs/en/getting-started/overview)

**任务在哪：** Workspace 托管对象（**不是仓库 markdown**）。存储后端未在已读文档写清（云/自托管）。

**Agent 怎么读任务：** **未能访问到 Tasks MCP/API。** 已读：

- [Working with Agents](https://openagents.org/docs/en/workspace/multi-agent-collaboration)：Tasks 是 UI 看板（Backlog → In Progress → Done），人按 Run 派给 agent/workflow。
- [Connect Claude Code MCP](https://openagents.org/blog/posts/2026-01-15-connecting-claude-code-to-openagents-networks)：工具全是 **消息**（`send_channel_message`、`retrieve_channel_messages`、`list_channels`…），**没有 list/get task**。
- [Workspace Python API](https://openagents.org/docs/en/workspace/python-api)：channels/agents/files，**无 task CRUD**。

结论：公开文档里，**共用面是线程，不是任务 API**。任务板是人侧调度器。冲突/版本/worktree：**未能访问**。

---

## 3. Trellis（mindfold / docs.trytrellis.app）

另有 trellis.dev（IDE+Cases）、Task Trellis MCP、物业 Trellis——本条按「和 Thesis 同类的 repo 内任务系统」取 **mindfold Trellis**。[架构](https://docs.trytrellis.app/advanced/architecture) [everyday-use](https://docs.trytrellis.app/start/everyday-use)

**任务在哪：** **跟着 git 仓库走**：`.trellis/tasks/<MM-DD-slug>/`（`task.json` + `prd.md` + 可选 design/implement + jsonl）。**不是全局一张表。** 当前任务指针是 **session 级** `.trellis/.runtime/sessions/<session-key>.json`，多窗口可各绑一卡。

**Agent 怎么读：** **不是 MCP 任务工具。** 读文件 + CLI：

```
./.trellis/scripts/task.py list [--mine] [--status]
./.trellis/scripts/task.py start|finish|archive|set-branch
get_context.py          # 启动摘要：身份、git、active tasks
```

`task.json` 字段（文档给出）：`id,name,title,description,status,priority,assignee,branch,base_branch,worktree_path,pr_url,parent,children…`。`worktree_path` **文档写明是 schema 占位，0.5 脚本不填**。

**防撑爆：** JSONL 清单只列 **本卡需要的 spec/research 文件**，禁止预注册代码路径。hook 只注入当前 `workflow-state` 面包屑，不是整板。

**写：** agent/人都能改同一批 markdown。状态机很瘦：`planning → in_progress → completed`。无所有权锁；assignee 是字段。**无乐观锁。** 人/agent 冲突 = **git 文本合并**（last writer 或 merge conflict）。

**worktree：** 不再自带 `/parallel` worktree orchestrator；`branch` 靠 `task.py set-branch`。一卡可对应一分支，**不强制一卡一 worktree**。

---

## 4. 给 coding agent 的任务系统（实际查了三个）

### 4.1 Backlog.md（MrLesk）— 最像「人看看板、agent 用工具」

源：[README](https://github.com/MrLesk/Backlog.md) · [`src/mcp/tools/tasks/index.ts`](https://raw.githubusercontent.com/MrLesk/Backlog.md/main/src/mcp/tools/tasks/index.ts) · [`handlers.ts`](https://raw.githubusercontent.com/MrLesk/Backlog.md/main/src/mcp/tools/tasks/handlers.ts)

| | |
|---|---|
| 存在哪 | **仓库 markdown**（`backlog/` 等），可 git 提交。Git 可选。 |
| 读 | MCP：`task_list` `task_search` `task_view`；或 CLI `backlog task list --json`。默认也推 **CLI instructions**（`backlog instructions overview`），MCP 是可选。 |
| list 签名 | `status? type[]? assignee? unassigned? milestone? labels[]? search? ready? limit?` |
| list 返回 | **按状态分组的一行摘要**（`[P] [type] ID - title`），不是全量 JSON。 |
| view | `id` → 完整任务（`formatTaskCallResult`）。 |
| 写 | `task_create` `task_edit` `task_archive` `task_complete`。无 per-agent 所有权。跨 branch 任务标 `isLocalEditableTask`，别的 branch **不能 complete/archive**。 |
| 冲突 | 文件锁（`isTaskLockError` / `isCreateLockError`）；另有 duplicate ID 检测。无 version 号。 |
| worktree | MCP 跟 MCP roots / `BACKLOG_CWD` 解析项目。历史 bug：从 worktree 写到 **主仓库 backlog/**（[#558](https://github.com/MrLesk/Backlog.md/issues/558)）。任务≠worktree。 |

设计要点：**list 摘要 / view 全文**；`ready` 过滤器（依赖满足才给）。

### 4.2 Conductor（shannonbay）— 上下文窗口外的任务树

源：[README](https://raw.githubusercontent.com/shannonbay/Conductor/main/README.md) · [`schema.ts`](https://raw.githubusercontent.com/shannonbay/Conductor/main/mcp/src/schema.ts) · [`context.ts`](https://raw.githubusercontent.com/shannonbay/Conductor/main/mcp/src/context.ts)

| | |
|---|---|
| 存在哪 | **全局 SQLite** `~/.conductor/tasks.db`（可用 `CONDUCTOR_DB` 改）。人用 Next UI 看同一库。**不跟仓库走。** |
| 读 | **没有 list-all-tasks。** 核心是 `get_context(task_id?)`：当前卡全文 + parent 摘要 + siblings/children **摘要** + `tree_stats`。ID 是树地址（`1.2.3`）。 |
| 写 | `update_task`（result / state_patch / notes）；`set_status(pending\|active\|completed\|abandoned)`。无多 agent 所有权。 |
| 冲突 | 未文档化乐观锁；单 DB 最后写赢。 |
| worktree | plan 有 `working_dir`；任务树≠git 分支。 |

这是「**永远别把整棵树塞进上下文**」的极端形态。

### 4.3 claude-task-master / Taskmaster

源：[README](https://github.com/eyaltoledano/claude-task-master) · [MCP docs](https://tryhamster.com/docs/taskmaster/capabilities/mcp) · [task structure](https://tryhamster.com/docs/taskmaster/capabilities/task-structure)

| | |
|---|---|
| 存在哪 | **项目内** `.taskmaster/`：`tasks.json` + 生成的 per-task md。跟仓库走。 |
| 读 | MCP `get_tasks` / `get_task` / `next_task`；CLI `task-master list\|show\|next`。 |
| 防撑爆 | **工具表本身就贵**：36 tools ≈ 21k tokens。`TASK_MASTER_TOOLS=core` 只加载 7 个（`get_tasks,next_task,get_task,set_task_status,update_subtask,parse_prd,expand_task`）。`next_task` = 依赖已满足的下一张，避免扫全板。 |
| 写 | `set_task_status` 任意改；无所有权。metadata 默认禁止 MCP 改。 |
| 冲突 | 共享 JSON；**未核到锁/version。** |
| worktree | tags 当 workstream；**未核到 worktree 绑定。** |

---

## 对照（只写已核事实）

| | 存储 | Agent 读 | 默认返回量 | 写别人的卡 | 冲突 | 卡↔worktree |
|---|---|---|---|---|---|---|
| **codeg** | SQLite / folder | **无 list/get** | n/a | 否（token 绑本卡） | CAS+run_seq | 引擎建 worktree；卡挂根 folder |
| Multica | Postgres workspace | MCP/CLI list+get+search | list cap 100；get 带评论 | 是（角色/PAT） | **未核 version** | runtime 目录 worktree |
| OpenAgents | Workspace 托管 | **任务 API 未能访问** | n/a | UI 派人 | 未能访问 | 未能访问 |
| Trellis | git md | 读文件 + `task.py list` | jsonl 限本卡 | 是 | git merge | 占位字段 |
| Backlog.md | git md | `task_list` 摘要 + `task_view` | 一行/卡 + limit | 是（本 branch） | 文件锁 | 否（还踩过写错树） |
| Conductor | 全局 SQLite | `get_context` 邻域 | 邻域摘要 | 是 | 未核 | working_dir |
| Taskmaster | git json | `get_tasks`/`next_task` | 可全量；靠裁工具表 | 是 | 未核 | 否 |

---

## 判断（意见，不引用）

**最适合 codeg 的不是 Trellis/Backlog 的「任务即文件」，也不是 Taskmaster 的 36 工具。** codeg 已有 SQLite + 10 态 + git 合并 + CAS + 伴生进程。把卡改成 markdown 会和 `run_seq`/merge 抢权威。OpenAgents 的「任务只给人点、agent 只活在线程里」也不够——codeg 已经让 agent **建卡**，缺的是对称的读。

最接近的组合：

1. **Backlog/Multica 的 list 摘要 + get 全文**（人看板、agent 工具同一事实）
2. **Conductor 的邻域原则**（默认不给整板）
3. **codeg 已有的 token 作用域 + CAS**（写自己的进度；状态机仍由引擎收口）

不要抄 Multica 的「agent 任意 `update_task` 改别人状态」——会打穿 `queued/preparing/merging` 和 merge 泵。

### 最小可用读工具：2 个，最多 3 个

| 工具 | 返回 | 为什么必须 / 为什么停 |
|---|---|---|
| **`list_work_tasks`** | 本 folder 的 **紧凑卡**：`id, title, status, agent_type, work_branch, run_seq, updated_at, conversation_id?`。过滤：`status[]`（默认非 archived）、`limit`（建议 ≤50）。**不要** `config` JSON、不要 `result_summary` 长文、不要事件日志。 | 没有 list，chat 里的 `create_work_task` 只能盲建；协调 agent 看不见队列。人侧 `work_task_list` 已有，MCP 对齐即可。 |
| **`get_work_task`** | **一张**卡：list 字段 + `verdict, result_summary, last_error, base_branch, worktree_missing`。可选 `include_events:false` 默认。仍不要整份 `config`。 | list 不够写 follow-up；get 是按需放大。 |
| **可选 `search_work_tasks`** | 标题子串，同 list 字段，硬 `limit`。 | 看板小时用 list 过滤就够。第三工具的成本主要在 **工具 schema 占上下文**（Taskmaster 的教训）。 |

**不要加：** `next_task`（引擎已 claim/pump）、`update_task`/`set_status`（和 CAS/合并冲突）、全量 `list` 无 limit、把 `task_progress` 历史当 get 默认返回。

写接口保持现状：`task_progress` / `task_complete` 只能打自己那张；建卡继续 `create_work_task`。若以后要「抢活」，应做成引擎侧 `claim`（CAS `todo→queued`），不要让模型 PATCH 别人的 status。

### 两个反对意见

1. **上下文膨胀 / 看板变聊天记录。** list 若返回 `config`、prompt、每次 progress，一次调用就能烧掉一轮。agent 会把 get 当日记，循环 list→评论→再 list。Backlog 用一行摘要、Conductor 用邻域、Taskmaster 连工具表都要裁，都是在防这个。codeg 的 `task_progress` 已经是事件流；再开放读却不截断，看板会从「状态机」变成「第二个 Room」。
2. **互相抢活 / 状态机被绕过。** 一看到别人的 `todo`/`review`，agent 会 `create_work_task` 复制、或（若误开 update）把别人的卡标完成。Multica 用 `todo+assignee` 才 pickup；codeg 已用 CAS。读一旦开放，**抢活发生在决策层**（两个 session 同时开同一需求），不是发生在 SQL 层。没有「已 claim / 非本 token 只读」的默认过滤，看板会变成公地。

---

### 未能访问 / 未编造

- 官方 Multica CLI 子命令与乐观锁：**未能访问**文档正文。
- OpenAgents Tasks 的 MCP/SDK 方法：已读页面 **没有**。
- 用户提到的 Trellis↔Thesis 旧会话：`deja search` 当时卡在索引，**未能用上**。
- 未把 `gh agent-task`、`@agent-tasks/mcp-server` 当主对象展开（只扫到存在 claim-gate 产品）。

来源均在各节 URL 或 `file:line`。