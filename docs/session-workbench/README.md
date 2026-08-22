# session-workbench 知识索引（第一入口）

> 仿 Thesis `.project` 体系：本目录是 codeg 开发的分层知识库。**开新调研/新任务前先来这里查重**；
> 本文件只做路由，不承载内容。教训看 [LESSONS.zh-CN.md](LESSONS.zh-CN.md)，
> 当前在飞的活看 [NOW-BOARD.zh-CN.md](NOW-BOARD.zh-CN.md)。

## 路由表

| 当前任务 | 第一入口 |
| --- | --- |
| 想知道有哪些已知问题/修没修 | `DOGFOODING-LOG-2026-08-20.zh-CN.md`（O 编号台账，事实源） |
| 想知道交互行为"应该是什么样" | `INTERACTION-CONTRACT.zh-CN.md` |
| 想知道某晚干了什么/为什么这么决策 | `MAINTAINABILITY-PROGRAM-2026-08-19.zh-CN.md` §5 执行日志 |
| 深层重构（schema/模型）动刀顺序 | `MODEL-AUDIT-RFC-2026-08-21.zh-CN.md` |
| 写 grok 任务书 | `LESSONS.zh-CN.md` 纪律段 + 近期 `*-REPORT` 的任务书样例 |
| 磁盘/worktree/门禁惯例 | `DISK-WORKTREE-HYGIENE.zh-CN.md` |
| dev 与 release 实例边界 | `DEV-RELEASE-INSTANCE-BOUNDARY.zh-CN.md` |

## 专题文档（按主题）

- **任务看板**：`TASK-KANBAN-IMPLEMENTATION-PLAN-2026-08-22.zh-CN.md`（**当前实施定稿**：
  Task/Execution 双轴、全局/项目看板、Session 指派与 PromptQueue 调度）；
  `TASKBOARD-RFC-2026-08-21.zh-CN.md`（早期设计 RFC）；
  `TASKBOARD-P1-REPORT.zh-CN.md`（第一期施工报告）；
  `KANBAN-DESIGN-2026-08-21.zh-CN.md`（**看板定稿设计**：一级容器、agent 读写不对称、
  防退化成群聊的两条对策）；
  `O63-AGENT-TASK-INTERFACE-RESEARCH.zh-CN.md`（Multica / Backlog.md / Conductor /
  Taskmaster / Trellis 的 agent 侧任务接口对照）；
  `O46-TASKBOARD-RESEARCH.zh-CN.md`（**组织维度调研**：一级容器＝项目 Folder，
  含容器关系图、两套 status 的区别，以及 Agent Orchestrator 的 Session/PR/CI 注意力投影
  审计；内有已标注的过时结论）
- **回合失败重试（O8）**：`O8-TRANSIENT-RETRY-AUDIT.zh-CN.md`（审计）；
  `O8-RETRY-REPORT.zh-CN.md`（施工报告）
- **群聊/协作**：`ROOM-READ-REPORT.zh-CN.md`（逐步披露接口）；
  `ROOM-CREATE-REPORT.zh-CN.md`（先建群后拉人）；
  `O54-FOLDER-ROOM-REPORT.zh-CN.md`（分类树右键新建群聊，带文件夹作用域）；
  `AT-COMPLETION-RECON-2026-08-20.zh-CN.md`（@ 补全与多路径）
- **序列化/持久化**：`SERDE-PERSISTENCE-INVENTORY.zh-CN.md`（红黄绿盘点）；
  `SERDE-PINS-REPORT.zh-CN.md`（24 条钉死测试）
- **Provider 切换（O39）**：`CONFIG-MODEL-2026-08-21.zh-CN.md`（**配置模型定稿，先读这份**：
  只保留「跟随 CLI」和「codeg 档＝一份 settings.json」两种，逐条记了删掉哪些层、
  为什么删，以及 `build_runtime_env_from_setting` 的真实覆盖顺序）；
  `O39-PROVIDER-SWITCH-RECON.zh-CN.md`（paseo/monet 调研）；
  `CLAUDE-PROFILE-RFC-2026-08-21.zh-CN.md`（**启动配置档设计**：CLAUDE_CONFIG_DIR 机制、
  认证优先级、MCP 暴露与泛化到其它 harness 的成本）；
  `O59A-CLAUDE-PROFILE-BACKEND-REPORT.zh-CN.md`（**后端已落地**：磁盘布局、解析顺序、
  第 7 节是给前端的 API 契约）；
  `O59B-CLAUDE-PROFILE-UI-REPORT.zh-CN.md`（组合框 chip + 设置页目录）；
  `O59C-PROFILE-MCP-REPORT.zh-CN.md`（**档对 agent 可见**：`list_profiles` 工具、
  建任务/自动化的 `profile` / `model` 参数、inherit 路径会丢档的陷阱）
- **fork/rewind**：`fork-rewind-slices/`（切片调查）；
  `FORK-REWIND-RECON-GROK-CURSOR-2026-08-19.zh-CN.md`
- **转录面骨架**：`CHARTER1-PACKAGE-I-REPORT.zh-CN.md`、`CHARTER1-PHASE2-REPORT.zh-CN.md`
- **生态调研**：`MULTI-AGENT-ECOSYSTEM-SURVEY-2026-08-19.zh-CN.md`、
  `ATRIUM-FEATURE-AUDIT.zh-CN.md`

## 维护规则

- 新文档落本目录时**必须**在上面加一行；不加索引=没交付。
- 台账（DOGFOODING-LOG）永远是问题状态的唯一事实源，报告不覆盖台账。
- 教训沉淀进 LESSONS 而不是散在日志里；日志记过程，教训记规则。
