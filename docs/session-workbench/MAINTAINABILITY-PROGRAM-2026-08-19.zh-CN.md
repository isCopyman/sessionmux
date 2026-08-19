# 可维护性收敛计划（Goal）

> **北极星（用户 2026-08-19 06:1x 重申，所有排期以此校准）：更好、更易用、更稳定、
> 更强的多 agent 交互。任何任务包入队前先问一句"它让多 agent 交互变好了吗"，
> 答不上来的不做。**
>
> 状态：Active（2026-08-19 01:50 立项，审计进行中）
> 负责：Claude Code 编排会话（只做规划、派活、验收）；执行全部由钉便宜模型的子代理完成
> 用户意图（2026-08-19 凌晨三条指令的提炼）：
> 1. 项目功能已成形，但设计、MCP 工具命名、skill、部分功能"很散"，需要收敛式重构；
> 2. 重构必须可回溯：改→测→commit 小步走，测试方法照《Desktop 开发、调试与验收手册》；
> 3. 代码可读性/可维护性要提升；文档失修（缺行为模式、生命周期、mermaid 图、多 agent 交互示例），文档工作也委托子代理；
> 4. 思考 mailbox 与群聊（Room）的设计，思考 UI 易用性与设计空间；
> 5. 用户休息，编排会话全权自主运行。

## 0. 不许翻案的红线（已由用户定案）

以下结论来自 2026-08-17/18 多轮拍板（详见 ROOM-VS-MAILBOX-DESIGN、GROUP-CONVERSATION-RFC、
SESSION-TRIGGERS-GOALS-AUTOMATION-RFC 及项目记忆），本计划所有任务不得违反：

- mailbox 名字不改、继承现有表；`list_inbox` 保持显式参数，不做 DSL；不新增阻塞等待类工具。
- Room 照独立系统做（共享账本+读游标），不并回邮箱；Mailbox 与 Room 工具、UI 两套，底层共用 Delivery/Dispatcher。
- 群帖无主题（`post_room` 无 title）；`send_message` 拒收 `room_id`；群 `@` 不进 inbox。
- 引用≠叫醒，唤醒必须显式 `@`；`post_room` 不扫正文里的 `@` 字符。
- 群成员无 role/别名；mailbox 多收件人是密送语义，无 CC。
- `MAX_AGENT_REPLY_CHAIN_DEPTH = 4` 是刻意保险丝；人类回复链不受限。
- 调度类别 user > letter/reminder > timer；timer 退避 = grace×2^strike 上限 30 分钟。
  【2026-08-19 更正：原"3 次刹车 auto_paused"机制已被 34ae1c36（reset_delay instead of
  acknowledge-and-park）拆除，auto_pause 成为待清理的遗留代码；本计划按新语义执行。】
- 信箱 UI = 时间线卡片 + 横幅入口 + 宽两栏 Dialog，无独立"往来"tab；人类不能替 agent 回信；状态词中枢在 `src/lib/mail-human-status.ts`。
- human_notice（人类收件箱）存储层两案待用户拍板，实施前不得选边。
- 真归档、超 100 封游标翻页、标签分类明确缓做。
- 文档规则：结论写回既有文档、不建重复总结；代码是实现事实源；偏离 RFC 必须先改 RFC。

## 1. 并行会话避让协议

本仓库有另一个 agent 会话正在活跃开发（约一小时一批提交；当前在做 ROOM-VS-MAILBOX-DESIGN
第 13 节路线：侧栏右键、Room 进 Collection、`codeg-mcp` 拆 `codeg-mailbox`/`codeg-room`
server 名等），工作区常驻大量未提交 WIP（含一次 cargo fmt 全仓噪音）。因此：

- 本计划的每个提交**只显式暂存自己新建/修改的文件**，共享文件逐 hunk 核实归属；禁止 `git add -A`。
- 避让清单（并行会话地盘，本计划不碰）：`src/components/rooms/`、`src/components/collections/`、
  `src/components/layout/sidebar*`、`src/components/workbench/`、`src/stores/room-catalog-store.ts`、
  `src-tauri/experts/skills/codeg-mailbox/`、`codeg-room/`（其 WIP 期间）、room/collection 相关 Rust 服务。
- 涉及大范围 Rust 代码搬动的任务，等并行会话该批提交落地后再动，或在独立 worktree 分支执行。
- 不重置、不清理、不覆盖任何不属于本计划的修改（手册 4.1）。

## 2. 执行纪律

- 每个任务包 = 一个可独立 review 的小步：改动 → 按手册 4.4 跑"与改动相称"的测试 → 显式暂存 → commit。
- 测试分层照手册：前端 `pnpm exec vitest run <相关文件>` + `eslint <改动文件>` + `tsc --noEmit`；
  涉及共享 store/类型再 `pnpm test` + `pnpm build`。Rust 在 `src-tauri` 下
  `cargo test --features test-utils <过滤词>` + `cargo check`；动了 server/MCP/共享 service 再补
  `--no-default-features` 两个 bin 的 check。全量套件只在阶段收口跑。
- 提交信息沿用仓库风格（`feat(room): ...` / `docs: ...` / `refactor: ...`），一句话讲清楚做了什么。
- 子代理模型纪律：探查用 explore-k3，动手用 worker-k3（均钉 Kimi K3）；禁止 fable 子代理。
- 验收：编排会话逐包看 `git show` 差异 + 测试输出摘要；不合格就追加修复提交或 revert，不改历史。

## 3. 阶段与任务包

（G0 审计结论回填前，仅列框架；每包完成后在 §5 进度日志记 commit。）

- **G0 盘点**：四路只读审计（MCP 工具面、skill 体系、通信底座+文档对账、前端 UI）→ 回填本文任务包清单。
- **G1 文档收敛**（全部委托；audit-comms Part B 对账表为施工图。执行顺序 G1c→G1a→G1b
  串行，避免同文件冲突；USAGE 整份归 G1a、G1c 不碰）：
  - G1c ✅ 按对账表逐份修正"文档与代码矛盾"处（每处修正必须对着表里给的代码反证）：
    ROOM-VS-MAILBOX（删 Room API 已有、Room 树/Tab 已落地、§9 差距表勾销、§17 补
    store_only 不进未读催办的边界）；GROUP-CONVERSATION（send(scope) 已拆两套、Room Tab
    已持久化、R2 互 @ 已落地、§0.3 链深"保险丝"改为"记账中，强制待 G4-1"）；
    SESSION-COMMUNICATION-RFC（文首状态行、§6 store_only 冷启动矛盾、§6.4 催办无
    interrupt、§5.6 硬编码常量、§10 schema 清单）；RUNTIME-LIFECYCLE（冷启动、Reminder
    已实现、obligation 字段已进表、§12 Gate 勾销）；TRIGGERS-GOALS（§0.3/0.5/0.6/0.7 对齐
    34ae1c36 新语义并注明日期）；HOST-CONTROL-SURFACE（文首"尚未实现"改状态、§7 顺序表
    勾销）；AGENTBUS 加"未启动设计存档"头注；HANDOFF 补"易错点 4/5 已随 34ae1c36 作废"注。
  - G1a ✅ 重写 `SESSION-COMMUNICATION-USAGE.zh-CN.md`（README 指定的 agent 交流入口，现
    与代码严重脱节：声称没有信箱/不排队/关闭不冷启动，全被推翻）→ 按场景重写：找地址、
    私信往返与义务、群聊发帖与 @ 唤醒、三层未读、催办何时来、timer 续跑，每场景给真实
    工具调用示例+预期结果；必须写清反直觉缺省（不填=high+expects_reply=true）、
    read_message=已读 vs read_room=游标、store_only 不进未读催办、同物两名该用哪个
    （list_sessions vs session.list 等）。
  - G1b ✅ mermaid 生命周期图写回对应 RFC（仓库已有先例）：信件 delivery 状态机+义务与
    催办（→SESSION-COMMUNICATION-RFC）、群帖 @ 唤醒流（→GROUP-CONVERSATION-RFC）、
    调度器类别 claim 与 steer/idle 双路径（→RUNTIME-LIFECYCLE-RFC）、timer 新退避状态机
    （→TRIGGERS-GOALS-RFC，以 34ae1c36 后代码为准）。
- **G2 skill 打磨**（audit-skills 报告已回。总体结论：工具名零漂移，一致性很好；结构采
  用"保持 5 个、原地修硬伤"——分发机制是编译期内嵌+启动解包+逐 agent 链接，技能改名会让
  旧目录变成幽灵"自定义技能"，合并/改名成本高收益低。quote≠wake 六处副本是用户当时故意
  给弱模型装的行为护栏，**不做指针化去重**；信封判别块的去重也缓做——单通道启用的 agent
  需要原地读到判别规则）：
  - G2-1 ✅ 修硬伤（干净文件）：codeg-host-control/SKILL.md 删掉不存在的 `not_found` 阶段
    （代码只有 read/persisted/ui_requested/rejected，找不到对象走 rejected+note）；
    codeg-multi-agent/SKILL.md 修 room.create 文案（"至少两个成员"→说明 caller 自动入群、
    必填 title）；experts.toml 补 codeg-host-control 与 codeg-session-timer 缺的 8 个 locale
    display_name/description。
  - G2-2 ⏸ codeg-room/SKILL.md 同题修正（"Pass at least two Session ids"、必填 title 未提、
    "not an owner"措辞）——文件在并行会话 WIP 中，等其落地。
  - G2-3 ⏸ host_control_room.rs capability 文本 "becomes owner" 与"成员平等无 owner"红线字面
    冲突（功能上 owner 无特权）→ 改 capability 文本一侧；并入 G3-1 同文件批次。
  - G2-4 ⏸ experts.rs 打包测试断言漏 codeg-session-timer（测试没跟上，非 bug）→ 并入 G3
    Rust 批次。
- **G3 MCP 工具面收敛**（audit-mcp 报告已回；代码文件多在后端 WIP 区，除注明外**等 WIP 落地**；
  改名类大动作一律挂 D1 决策后）：
  - G3-1 room.post 死分支修复：把 room.post 加入 access_for 让精心写好的迁移提示真正可达
    （现在 agent 只收到笼统 Unknown action）；顺手给 host_control_room.rs 补基础单测（现为零）。
  - G3-2 room.list / room.list_workbench 双名：ROOM-VS-MAILBOX §13.3 定的过渡别名，**不动**。
  - G3-3 schema 摘掉 legacy delivery_mode / delivery_hint（解析层保留兼容，防老 companion）；
    同步删 USAGE 第 73 行附近对 delivery_hint 的推荐（high 本就强制 steer_if_supported，字段冗余）。
  - G3-4 session.create 参数 harness → agent_type（与 automation/work_task/输出对齐；保旧名兼容）。
  - G3-5 get_session_info 供 codeg-mailbox 服务器可用（现在 mailbox-only agent 拿得到
    list_sessions 却查不了详情；机制上需允许工具多组归属，实现细节执行时定）。
  - G3-6 priority 隐藏别名 "urgent"：schema 未公开但解析接受，统一（公开或归一为 high）。
  - G3-7 点分/下划线两套词汇、同物两名（list_sessions vs session.list 等）、参数五种 session
    叫法、limit/filter 约定分裂、room_id 是 string——**全部挂 D1**；文档/skill 先教"该用哪个"。
- **G4 代码可读性/死代码清理**（audit-comms Part A 已回。后端模块地图确认 web handler 薄壳
  →_core 无违例、信封构造/计时引擎单一真相源、TODO 仅 1 处——底子比预想干净，"散"主要是
  历史遗留和文档与代码的错位）：
  - G4-1 ⚠️ **链深保险丝只记账不强制**：MAX_AGENT_REPLY_CHAIN_DEPTH=4 的注释与两份 RFC 都
    声称到上限拒绝回复要求，但代码无任何拦截路径（测试钉死 depth 0..=4 全 accepted）。
    按定案语义补强制（到 4 层的事件 expects_reply 强制落 false 或拒绝，含测试），文档同步。
    等后端 WIP 落地后做。
  - G4-2 死代码清理（34ae1c36 拆等待刹车留下的生产孤儿）：session_timer_service::auto_pause、
    collaboration_service::outbound_awaiting_summary / latest_mailbox_info_at；另
    collaboration_room_service::list 转发壳、pub send() 仅测试可见性收紧。
  - G4-3 小修：CollaborationDeliveryState 补 as_str 干掉手写映射；催办 runtime 重复 wake 调用
    去一处；wire 参数 mail_box 与类型层 scope 命名对齐（wire 兼容别名保留）。
  - G4-4 认知负担类（只加注释不改行为）：acp/delegation/ 目录名是 delegation 已删后的历史
    遗留、acp/idle_sweep.rs 名字像调度件实为连接回收器、automation 直发绕队列属设计——
    各补模块级 doc 注释讲清"为什么叫这个名/为什么不走队列"。
  - G4-5 文档钉死反直觉缺省：agent 不填 priority/delivery_mode/expects_reply 时缺省是
    high+立即通知+要回复；store_only 信不进未读催办（只催 invoke_when_idle 未读与已读未回）
    ——写进 G1a 使用文档与 skill。
  - 前端已知项：房间未读徽章三处逐字复制 + 共享组件 CollaborationUnreadBadge 零引用（涉
    collection-tree/workbench-tree 脏区，等落地）；api.ts 5287 行按域拆（低优先）；
    acp-agent-settings 11818 行按 agent 拆（低优先）；时间格式化 42 处手写散布 18 文件，
    先立中枢新代码用、存量渐进迁。
- **G5 UI 易用性**（audit-ui 报告已回。逐包标注：✅=文件干净可执行，⏸=等 WIP 落地，🔒=需用户拍板）：
  - G5-1 ❌ 已作废（2026-08-19）：原方案给"autoPaused 态"加药丸可见性，但 34ae1c36 已拆除
    auto_pause 机制，该状态生产上不会再出现。前端遗留的 autoPaused 卡片提示/类型字段转入
    G4-2 死代码清理（等并行会话 timer 方向稳定后做）。教训记档：执行包开工前必须核对目标
    机制在最近 48h 提交里是否换代。
  - G5-2 ✅ session-mailbox-dialog.tsx（732 行核心信箱 UI）补行为测试；顺修 banner 测试里
    mock 已删依赖的陈旧断言。
  - G5-3 ✅ 协作过滤词表统一：会话中心 CollaborationFilter 与信箱 MailFilter 同概念两套枚举
    （awaiting vs awaiting_reply）+ 计数徽章配色三套 → 收敛到方向分家词表 + 琥珀=待回复约定。
  - G5-4 ⏸ timer 删除无确认、群成员移除无确认（后者在 rooms-page 脏区；前者需新 i18n key，
    而 i18n 十语文件都在 WIP 区——等落地一并做）。
  - G5-5 ⏸ 建群入口只藏在多选批量条（sidebar 脏区）。
  - G5-6 ✅ 【2026-08-19 用户拍板：人类写信 UI 不做；Human Inbox 暂不做】探针已定性：
    入口摘除是 d7d1bdb4（08-17"mailbox trim"）的**有意红线执行**，组件语义="以某 Session
    名义发信"，对人类即冒充（后端信箱 INSERT 不写 author_kind，默认 'session'）。执行拆两半：
    **G5-6a** 删 session-message-composer-dialog.tsx + 其 .test + banner 测试失效 mock 段
    （全干净文件，今晚做）；**G5-6b** composer 独占 i18n key 十语清扫（i18n 文件在 WIP 区，
    等落地；孤儿 key 无害可等）。备查：若将来重启"人类发信"，恢复配方=后端 3 处
    （SendCollaborationMessageInput 加 author_kind、collaboration_service 两个 INSERT 写列、
    仿 post_room 定 human 账本源）+ 前端 2 处（composer 加人类模式、header/@ 处接回入口）。
  - G5-7 已撤销：信箱对人保持"看/审计"用途，0 信时无入口属合理，不再改。
  - G5-8 ✅（T2 基线实测新增）：timer 折叠药丸信息量过低（"1 个定时器"看不出在等什么、
    何时触发）→ 药丸补充下次触发倒计时/状态（基于现行纯退避机制，非已废弃的 autoPaused）。
  - G5-9 ✅（T2 基线实测新增）：会话中心开着筛选下拉按 Escape 会直接关掉整个 Dialog 而非
    先收起下拉——分层修复；同时"全部状态/全部消息状态"两个相邻下拉文案难分——并入 G5-3
    词表统一时一起改文案。
  - G5-10 ⏸（T2 基线观察）：Room 成员栏入口只有无文字小图标、"N 位成员"文字不可点
    （rooms-page 脏区，等 WIP 落地）；dev 编译失败白屏无错误遮罩（dev 体验，低优先记账）；
    信箱横幅视觉权重偏弱（横幅形态是定案，仅在形态内微调，低优先）。
  - G5-11 ⏸（2026-08-19 用户问答确认方向）：群欠账体系 agent 侧已完备（四计数+needs_reply
    窗口+共用催办），人侧"全局一眼看谁欠我"只藏在会话中心过滤下拉——侧栏/显眼处补全局
    欠回复入口（含群与私信聚合）。等 WIP 落地后与 G5-5 徽章去重一并做。
  - G5-13 ✅高优先（2026-08-19 用户指出：聊天没有补全会很麻烦）：房间输入框无 @ 自动补全
    面板，药丸是唯一可见途径（手打 @别名 仅在提交时静默解析，零反馈暗门）；Session 输入框
    的统一 @ 面板（RichComposer + use-composer-mention-labels）现成未接。裁决照 ROOM 设计
    §12"复用同一套徽章"：房间接入同款补全（会话组限定本群成员+@全体/@human），药丸行降级
    为"将唤醒：…"实时预览 + @全体小按钮，纪律提示挪进 placeholder，时间线徽章渲染与
    Session 页对齐。WIP 落地后执行（今晚 UI 批）。
  - G5-12 ⏸（2026-08-19 用户指出）：侧栏多选模式 Session 有复选框、Room 没有——Room 是
    一等 item 却进不了批量操作。裁决：**手势拉平、动作按类型过滤**——Room 参与多选，
    批量条只亮 Room 适用动作（移动分类/加移工作台/删除带确认），归档与"建群"对 Room 置灰，
    混选亮交集。执行前先产出 Session vs Room 侧栏行为对照表（悬停/右键/拖放/多选/徽章/
    定位/双击逐项核对），照表系统性修，不逐个打补丁。WIP 落地后执行。
  - T2 基线产物：.artifacts/desktop-validation/maintainability-program-2026-08-19/
    （baseline-01..07 + result.md，HEAD 47a73c3c 时拍摄）。
- **G6 收口与真机验收（2026-08-19 深夜用户加码：详细测试+截图，desktop 端，照手册）**：
  - T1 自动化：每包相称测试（已在各包纪律里）+ 阶段门全量（vitest+build、cargo 桌面/server/
    mcp 三套）。
  - T2 desktop 真机（手册 4.3/4.5/4.6：CDP 9222 连**正在运行的开发实例**，禁止重启它——
    本编排会话与并行会话都活在里面）：先拍"改动前"基线（侧栏/房间页/信箱 Dialog/时间线
    信件卡/队列徽章/会话中心过滤器），后拍改动后对照；再用「多session交流测试工作台」
    现场数据做**多种多 agent 交互**的截图走查。产物入 .artifacts/desktop-validation/
    maintainability-program-2026-08-19/，附 result.md（分支/commit/场景/断言/新错误）。
    若实例未开 9222 调试口：标记 BLOCKED（不能为开口重启实例），desktop 层证据改由
    重启后补验，报告里写明证据层级。
  - T3 后端行为层冒烟（手册"server 冒烟"路径，验 R1-R6）：codeg-server + 临时数据目录 +
    钉便宜模型的真 agent（优先 grok harness，claude 必须显式钉 k3/deepseek 档），重点回归：
    义务挂票在 compact 后重现（R1）、同轮不双戳（R2）、跨通道 reply 被拒（R3）、timer 正文
    带欠账（R4）、链深 4 强制 expects_reply=false（R6）。注意 HMR 混合态纪律（手册 10.3）：
    正在跑的 desktop 实例后端不含今晚的 Rust 提交，R 系列的 desktop 层验证留待用户重启后。
- **G7 架构专审（2026-08-19 深夜用户新增，重要）**：a) 消息调度与优先级的正确性对抗审查
  （并发/重启/租约边界下的漏派、重派、双戳）；b) **agent compact（上下文压缩）与信件注入
  的交互**——已注入 turn 的信在 compact 后是否会被 agent 遗忘、义务/催办机制能否兜底、
  codeg 对各 harness 的 compact 事件感知现状；c) **hooks 取舍**——各 harness hook 能力盘点、
  该不该用、用在哪些步骤（compact 侦测/turn 边界/义务摘要重注入），给最小可移植方案。
  产出：分析报告 + 必要小修执行包。
- **G8 多 agent 工作模式库（用户新增）**：对照 Anthropic《Building effective agents》与
  多 agent research system 博文的模式（编排者-工人、并行化、路由、评审-优化回环等），
  逐一判断 Codeg 现有底座（mailbox=分别面试 / room=研讨会 / timer=续航 / host control=开人）
  能否承载；把可承载的写成 codeg-multi-agent skill 的新 reference playbook——除通用编码外，
  覆盖**科研写作（论文分节起草-互评-合稿）、长文/叙事（大纲-分段-一致性审）**等场景。
  红线不变：不冻结角色、不做 persona 包、@ 纪律、密送无 CC。
- **G10 fork/rewind 能力调研（2026-08-19 凌晨用户新增）**：Session 的 fork、rewind、
  按消息位置 fork——ACP 未必原生支持，调研替代路径。底子=SESSION-HISTORY-CAPABILITIES-RFC
  （未参加今晚对账，先核新鲜度）+ codeg 已有整会话 fork 活体（67974819 fork 继承 pin）。
  调研轴：各 harness 原生能力（claude --fork-session//rewind、gemini checkpoint 等）×
  ACP wire 可见性 × 三条候选路（协议路/原生文件手术路——codeg 有全套 parsers 可截断改写/
  重放路）× parent_id 不许复用的谱系字段约束。产出能力矩阵+最小可用切片建议，进晨报。
- **G9 host control 扩权设计（用户新增，出方案待点头）**：是否让 agent 经 MCP 操作"用户级"
  行为——拉群已有（room.create），布局已有一部分（workbench.place_session），**缺的是改既有
  Session 的模型/推理强度**（后端 acp_set_mode/acp_set_config_option 已有会话级 pin 机制，
  差一层 host control 动作暴露与权限闸）。产出：能力矩阵（已有/缺失/风险）+ 建议动作集
  + 写闸设计（writes_allowed 保险丝现恒 true，扩权前应先让它真正生效），**实施等用户点头**。

## 3.5 决策轨 D1：工具面形态——MCP 还是 环境变量+CLI+skill（2026-08-19 用户提出）

用户提出：是否把全部 MCP 换成"环境变量注入 + CLI + skill"，理由是灵活性、且多个参考项目
如此做。此决策**先于 G3 执行**（若传输形态要换，工具改名就是白干），处理方式：

**结论（2026-08-19 证据齐后定稿）：不做全量替换；三个兼容小刀吸收该提案的真实收益。
【2026-08-19 深夜用户确认："我支持先用 skill+mcp 不使用 cli"——D1 就此关闭，G3 按 MCP
形态放行执行。】**

判决证据（transport-evidence 全文报告存档于会话记录）：

1. **RFC 已有反方向定案**：SESSION-COMMUNICATION-RFC:804-817 明确"不为不支持 MCP 的
   Harness 建 CLI fallback 产品路径；Agent 发信是结构化 Tool Call 不是 Terminal 命令；env
   仅用于注入 ambient backend/connection/短期 token"。理由今天仍成立：agent 进程可能
   多 session 复用，进程级 env 无法做 per-session 归因（MCP companion 是 per-launch token
   绑定的可信身份）；CLI 提交失败会出现"模型已答但 UI 看不到"的双重提交。按"偏离 RFC
   必须先改 RFC"，全量替换的前置是用户明示推翻该节。
2. **参考项目实拍不支持"换了就统一"**：与 Codeg 同构的 CCCC、Buzz 都是 MCP 为主
   （CCCC：MCP+env 身份+对不可预配 runtime 用首条 prompt 教 agent 自装+HTTP nomcp 兜底）；
   env+CLI 派的 Multica，其 execenv 包里 per-harness 特判一样不少（codex_home/cursor_mcp/
   openclaw shim/sandbox 剥 env…）——特判总量不因换传输而消失，只是搬家。
3. **阻塞工具 CLI 化的真实成本有现成账单**：agent-bus SKILL 用近半篇幅处理各 harness 的
   Bash 超时/后台语义差异（Grok background、Claude 自动转后台、Codex 三层超时嵌套）。
4. **常驻成本量化**：典型会话 12 工具约 22KB schema。有感但可控，且已有 features 快照
   + 三 server 拆分两层裁剪机制。
5. **副作用语义全在 socket 服务端**（read_message=已读、义务清账、游标），companion 近乎
   哑转发——技术上 CLI 可行，但也说明 MCP 层没有绑架任何语义，换传输买不到语义收益。

吸收其收益的三个小刀（并入既有阶段执行）：

- D1a → G3：**注入失败必须响**。二进制缺失/被 harness 剥除时目前只打一条 warn、agent
  静默无工具面（有过真实翻车）。加 preflight 检查 + UI 明示"该会话未获得协作工具及原因"。
- D1b → G3：**schema 瘦身**。send_message 3.5KB/post_room 2.6KB 的 description 有压缩
  空间；在不减信息量前提下给 top3 胖工具减脂，直接降低每会话常驻成本。
- D1c → 记录存档：OpenClaw（拒 MCP）与 Pi（收了就丢）今天拿不到任何 codeg 工具面。
  RFC 的既定答案是"不做 CLI fallback"；若将来要覆盖这两家，CCCC 的两条现成路（首条
  prompt 教自装 / HTTP nomcp 兜底）是比全量 CLI 化便宜一个量级的选项。仅记账，不立项。
- 面向"人和自动化脚本"的 operator CLI（类 cccc-cli send/inbox，走 HTTP 打 codeg-server）
  与 agent 工具面是两回事，不受本决策约束，有真实需求时可另立小项。

## 3.6 决策轨 D2：群共享资料与任务清单——文件为王，Room 只置顶/索引（2026-08-19 采纳）

用户问"要不要做群 shared task list / 共享资料展示"。经对照仓考据（Multica Project 资源只有
github_repo/local_directory 两类指针落 resources.json；CCCC 分 PROJECT.md 冷宪法 +
coordination.brief + 可选外部记忆；OpenTeams 历史是只读 JSONL 让 agent 自己读；OpenAgents
的 /v1/files 网盘是"成员不共享磁盘"场景的补丁）与 Codeg 现状（Session 绑 Folder，磁盘即
共享内存；群 RFC §7/§10 已有 pinned brief + 有界信封 + 只读记录路径的合同；领域模型把
"共识数据库"排在第一阶段之外），裁决：

- **正文全部落本地文件**（建议约定 `docs/rooms/<room-id>/`：brief.md、tasks.md、共识、
  产物），git 管历史，人用编辑器改；**Room 只做置顶与路径索引**，点开仍是文件。
- **任务清单同样文件化**，不新造产品对象，不与 work_task 流水线/通信义务焊死。
- **今晚生效路径**：该纪律直接写进 G8 playbook（交差=写文件+群里 @ 并贴路径），约定先活，
  将来的"置顶 brief+路径列表"功能只是露出约定的产物。
- **不做**：Room 网盘（上传+另存副本）、共识只存 collaboration_event 正文、每轮把资料库
  打进 prompt。
- **例外**：成员不同 Folder/远程读不到宿主路径时，给 Room 指定 home Folder 作索引基准；
  读不到的成员只拿信封短 brief，不假装共享整库。
- **将来置顶功能的红线**：v1 不给 agent 新增 pin 写工具（工具面刚做完减法），置顶由人挂。
- 时机：现在不开工；等群 UI 与通道稳定后作为小刀实施。

## 4. 设计判断（编排会话本人思考与裁决，2026-08-19 深夜成稿）

### 4.1 Anthropic 模式 × Codeg 底座逐一裁决

对照《Building effective agents》六模式与多 agent research system 博文，逐一判断 Codeg
现有机制能否承载（结论：**六个模式全部可承载，其中异步协调一项 Codeg 反而领先博文所述**
——博文自承 lead 只能同步等一批 subagent 收齐，而 Codeg 的 mailbox+统一调度天然异步）：

| 模式 | Codeg 承载方式 | 判断 |
|---|---|---|
| 提示链（顺序流水） | A→B→C 各占一个 Session，mailbox 接力传工件 | 可用；工件必须落文件、信里只传路径+摘要（防 compact 遗忘+防传话失真，博文同款教训） |
| 路由 | 主持用 list_sessions/get_session_info 选专家再投递 | 可用 |
| 并行-分片 | mailbox 密送多发（≤16），各自独立干 | 可用，密送=防锚定，正是拍板过的"分别面试" |
| 并行-投票 | 同题密送 N 份→主持对比；分歧再拉 Room 对质 | 可用，对应既定方法论"先私信收独立判断→分歧拉群→裁判总结" |
| 编排者-工人 | session.create(+initial_prompt) 开人 + mailbox 派活 + Room 共享现场 = 现有 star 玩法 | 可用；需把博文的派活纪律补进 skill（见 4.2） |
| 评审-优化回环 | 起草者↔评审者两 Session 往返 | 可用但有设计要点：链深保险丝=4 会截断长回环，**纪律=每一轮评审开新信（引用上一轮，不无限 reply）**——既保住保险丝又允许多轮 |
| 自治长跑 | 单 Session + continuation timer + ask_user_question 检查点 | 可用，即现有 timer 玩法 |

博文教训中值得成文进 skill 的四条（其余与现状重复）：**派活四要素**（目标/交付格式/工具与
来源指引/边界，防止两个工人重复劳动）；**用工规模标尺**（简单事实=1 人少量步骤；对比类=
2-4 人；复杂研究=多人分域——数字按 Codeg 场景校准）；**工件落文件制**（Session 可共享
folder，交付物写文件、信里传路径，规避"传话游戏"与 compact 失忆）；**编码类任务慎用自由
并行**（依赖强，改用流水/星型+worktree 隔离——Codeg 有 worktree 支持，博文明说 coding
是并行多 agent 的坏靶）。

### 4.2 新增 playbook 设计（写入 codeg-multi-agent/references/，红线不变：不冻结角色、
无 persona 包、@ 纪律、密送无 CC）

- **patterns-map.md**：上表的 agent 视角版 + 派活四要素模板 + 用工规模标尺 + 工件落文件制
  + 评审回环的"每轮新信"纪律。另补三条一手经验校验（2026-08-19 本编排会话实测）：
  ①"干完≠交付"是编排最高频故障（当晚 5 个取证 agent 全部完工不交稿需手动催），Codeg 的
  expects_reply 欠账+催办+自动兜底回复对此结构性免疫——playbook 应教主持：派活信一律
  expects_reply=true，让欠账机器兜底；②改主意要用 cancel_turn 而不是只追一封信（指令在飞、
  工人不查信箱的竞速真实发生过）；③并行写同一目录是最大事故面，编码类协作必须 worktree/
  分 folder 隔离，交付物落文件、信里传路径。
- **research-writing.md（科研写作）**：大纲（主持）→ 分节起草（密送分片，各节一文件）→
  交叉互评（背靠背密送收独立意见；分歧才拉 Room 对质）→ 合稿（主持）→ 全文一致性与文风
  统一（单一编辑 Session，不并行）→ 引用/事实核查（独立核查员，评审-优化回环跑摘要与
  引言）→ **定稿对稿会**（见下条）。关键裁决修订（2026-08-19 深夜，用户以体制内对稿会
  类比点破）：评审分两种目标——**找问题用背靠背**（要独立信号，防锚定），**定稿用对稿会**
  （要收敛，互相听见是功能：跨领域冲突只有摆同一张桌才暴露，主持逐段裁决即质量闸门）。
  真实流程是并行/串行交替的矩阵，不是单选。
- **line-review.md（对稿会：逐段收敛定稿，新增）**：稿件放共享 folder 文件；主持每段
  **开新根帖**引用该段（一段一线程，天然贴合链深 4 保险丝=会议纪律本身）+ @ 点名发言
  （expects_reply）；needs_reply/awaiting_reply 计数=还有谁没表态，5 分钟催办自动追；
  主持在两轮之间改文件、下一段引用新版；迟到成员靠读游标补课。零新机制，全部现成。
- **long-form-writing.md（长文/叙事/讲故事）**：大纲与设定卡（文件）→ 分章**顺序**起草
  （叙事连续性=强依赖，裁决为提示链而非并行；每章起草者读上一章成品文件）→ 连续性审查员
  对着设定卡挑矛盾 → 文风统一终审。头脑风暴阶段例外：可用 Room 圆桌或投票并行发散。
- star.md / planner-coder-reviewer.md 增补派活四要素与规模标尺引用（指针，不复制全文）。

### 4.3 G9 host control 扩权：能力矩阵与裁决建议（待用户点头）

已有：session.create（可带 model/mode/config_values！）/rename/cancel_turn/stop；collection
全套；workbench create/rename/add/place/remove；timer 全套；room.create/add_member。
缺口（对照用户点名）：拉群✓已有；布局≈已有（place_session）；**改既有 Session 的模型/
推理强度✗**——后端 acp_set_mode/acp_set_config_option + 会话级 pin（67974819）都在，只差
一层 host control 动作暴露。
建议：a) 新增 `session.set_config`（model/mode/config_values 子集，复用 pin 机制）；
b) **对自己**默认开放（agent 按任务阶段自调推理强度，真实有用、风险低）；**对其他 Session**
挂显式写闸（per-agent 设置）；c) 前置条件：先把恒 true 的 `writes_allowed` 保险丝接上真实
策略再扩权；d) 永不暴露删除类动词。实施排在用户点头 + 后端 WIP 落地之后。

### 4.4 组合行为与架构裁决（2026-08-19 取证齐后，编排会话定案）

调度层总评：claim/租约/幂等三层设计扎实，对抗审查未发现丢信或双投递级别的洞；组合行为
总评：三组件共用一张账本一条队列，咬合良好，送达顺序确定（user→信件+催办→timer）。
以下按"今晚修 / 记账待修 / 设计边界写文档"三档定案：

**今晚修（全部位于干净文件，逐包测试+提交）：**
- R1 **compact 免疫的义务恢复（本轮最重要的行为修复）**：催办预算 3 次用尽后开放义务永久
  沉默，若 agent 期间发生 compact（正文与 digest 全被摘掉）就彻底忘信。修法采纳"义务挂票"：
  Session 每次普通 turn 尾部自动附一行"未结义务标题清单"（只标题不含正文、仅在有义务时
  出现、借道既有 store_only 挂票机制）——compact 后下一轮自动重现，不新增唤醒、不加轮次。
  否决备选"2 小时心跳"：会反复唤醒空闲会话烧钱。
- R2 催办 steer 双戳修复：InjectSteer 补查 `collaboration_steered_this_turn`，已 steer 过的
  轮降级为 QueueAfterTurn（同轮"信封全文+digest 摘要"重复注入是纯浪费）。
- R3 跨通道清债封堵：私信 reply_to 群事件会清掉群点名的债、而群时间线永远看不到答案。
  修法与既有 validate_room_reply 对称：send_message 拒绝 reply_to 指向 room 事件，错误信息
  指引"回群用 post_room"。（群转私聊的正道=新开私信线程，skill 本就这么教。）
- R4 timer 续跑附宿主欠账事实：到期正文尾注加一行"欠回复 X 封/群点名 Y 条"（2026-08-17
  拍板本就允许附加"发给谁的消息还没回"类宿主事实，34ae1c36 删的是义务感知**退避**，不是
  事实附注）；同 commit 删除两个孤儿查询（outbound_awaiting_summary/latest_mailbox_info_at）
  与三处脱节注释、删 auto_pause 服务函数（模型字段与迁移不动，前端遗留 UI 归 G4-2）。
- R5 已删会话的 timer 收尸：软删 Session 后其 timer 仍每次到期入队淤积。修在引擎侧：
  到期候选查询 JOIN conversation 存活（避开 conversation_service.rs 脏区）。
- R6 链深保险丝补强制（原 G4-1）：depth≥4 的 agent 信强制 expects_reply=false（信照送、
  不能再挂债），**人类路径不受限**（红线原文），测试同步。落地后 USAGE 第 95 行的表述
  （"到顶的信不能再挂 expects_reply"）即从"设计"变"事实"，无需再改；GROUP-CONVERSATION
  §0.3 的"强制拦截列入维护计划"句同 commit 更新为已落地。

**记账待修（后端 WIP 落地后 / 用户点头后）：**
- pause_queue_if_pending 会让用户 cancel 覆盖 interrupt 的队列冻结原因（低危，修在
  prompt_queue_service.rs——脏区）；多目标信"队列满整体回滚 vs 目标不存在部分成功"语义
  不一致 + 发件 agent 无失败回执（同文件脏区）；turn 无看门狗（harness 挂死则该会话永不
  idle flush，建议"最后事件距今 N 分钟"活性探针，需设计）；被拉群无邀请通知（成员要自己
  list_rooms 才发现——建议入群时给新成员一条 store_only 邀请函+系统帖，待用户点头）；
  mention_human 无出口（Human Inbox 已拍板暂不做，先在 Room 面板未读把 mention 单列，
  归 UI 批次）。
- **hooks 定案：现在不引入**。ACP 信号面已够宽，13+ harness 中仅 3 家有 hook 机制且注入
  要碰用户全局 settings（污染 codeg 之外的使用）；R1 挂票已从服务端堵住 compact 主洞。
  留 RFC 记录：若将来做，首选 Claude PreCompact 单点（compact 感知+义务重注入），前提是
  验证 claude-agent-acp 转发 settings hooks。
- compact 期间投递依赖各适配器的排队语义（turn 间 compact 时 codeg 判 idle 照投）——
  写进文档作为已知边界；Claude live 路径 compact 完全不可见是上游适配器缺口。

**设计边界（写进 G1a 使用文档即可）：** store_only 不进未读催办；缺省=high+expects_reply
=true；user 排队项会把信件压后一轮（头不合批）；automation/work_task/IM 渠道拉起的会话
都可被 list_sessions 寻址与冷启动（这是能力不是 bug）；建群/入群不叫醒任何人。

## 5. 进度日志

### 5.0 断点快照（2026-08-19 04:20 前后，供上下文压缩/接续用）

已入库：47a73c3c（立项）→ c6b1a7ec+65b238f2（G5-1 落地又回滚）→ 508af8a7（G1c 八文档
对账）→ dccb030c（G1a USAGE 重写）→ 66fdcd2b（G1b 四张 mermaid）→ 0c033b77（决策增量）。

在飞三线：
- **worker-wipland（主检出）**：存量 WIP 落地。已过 tsc/eslint/vitest 4333 全绿/build/
  server 门 2429/mcp check/check --all-targets 零错；共享 target 有 DLL 环境阻塞
  （STATUS_ENTRYPOINT_NOT_FOUND，手册 4.4 已知类型），桌面门升级为 CARGO_TARGET_DIR=
  %TEMP%\codeg-landing-target 的冷目录全量跑（含集成测试首次真跑）。绿后五笔提交：
  ①test: repair stale assertions（四处 HEAD 存量债：panel-layout 断言/subscribe mock 带
  退订/host_bridge_e2e title 字段/delegation_columns 两字段）②style: fmt 107 文件
  ③feat(mcp) server 拆分 5 文件 ④feat(room) 排序+UX（含 2 个 tsc 修复与 i18n×10）
  ⑤chore: gitignore target-test-mcp。
- **worker-cskills（worktree 分支 mp/skills-playbooks）**：提交 1=G2-1 skill 硬伤+
  experts.toml 8 locale；提交 2=G8 四个 playbook references。完成后由编排会话 merge。
- **fork-survey（只读）**：G10 fork/rewind 能力矩阵调研。

落地完成后待派：**A 线 worktree**=R1-R6（规格全在 §4.4"今晚修"，注意 R6 落地时同步改
GROUP-CONVERSATION §0.3 措辞；R3/R4 落地时 USAGE 措辞自动成真无需改）；**B 线 worktree**=
UI 批 G5-2/3/6a/8/9/13（G5-13 规格在其条目：RichComposer 同款 @ 补全+将唤醒预览）。
两线收口由编排会话 merge 回 codex/session-message-v1（预期零冲突），随后 G6 全量门 +
T2 后置截图（CDP 9222 可用，基线在 .artifacts）+ 晨间验收报告 + 记忆更新。
纪律提醒：K3 工人 idle 不交稿要 SendMessage 催；merge 只由编排会话做；cargo/tauri dev
进程不许杀；主检出共享 target 跑不了桌面测试（用冷目录或 worktree）。

- 2026-08-19 01:50 立项；四路审计（audit-mcp / audit-skills / audit-comms / audit-ui，全部
  explore-k3）开跑。
- 2026-08-19 02:10 前后：四路审计 + transport-evidence 形态证据全部回收；G0 收口。核心结论：
  底座不散（一张账本/一个队列/一个信封构造点，web handler 薄壳无违例），散的是文档（严重
  滞后最近 48h 提交）、skill 三处硬伤、若干代码残留（链深保险丝无强制、刹车拆除孤儿）。
- 2026-08-19 02:15 D1 定案：不做 MCP→CLI 全量替换（RFC 既有反方向定案 + 参考项目实证 +
  阻塞工具成本），收益吸收为 D1a 注入失败必须响 / D1b schema 瘦身 / D1c 记账。
- 2026-08-19 02:15 G5-1 作废：autoPaused 是 34ae1c36 拆除机制的遗留态。worker 提交
  c6b1a7ec 抢跑落地，已以 65b238f2 revert（测试回基线 6/6 绿）。教训：执行包开工前核对
  目标机制最近 48h 是否换代。
- 2026-08-19 02:20 G1c（文档对账修正，8 份）派发 worker-k3 执行中。
- 2026-08-19 02:48 G1c 完工提交 508af8a7（8 文档 39 处对账修正，验收通过；并纠正简报一处：
  d81504fc 的巡检 poke 已被 34ae1c36 一并移除）。
- 2026-08-19 03:00 前后：用户深夜加码（compact/hooks/调度专审、多 agent 模式库、扩权设计、
  真机测试+截图）；G7/G8/G9/T 系列入计划。arch-compact 与 room-synergy 取证完成，§4.4 裁决
  定稿（R1-R6 修复包）。D1 由用户确认关闭（skill+MCP，不上 CLI）。
- 2026-08-19 03:10 G1a 完工提交 dccb030c（USAGE 209 行场景化重写，验收通过；其核实纠正
  简报两处：reset_delay 纪律方向、timer 正文现状无欠账计数——后者正是 R4 要补的）。
- 2026-08-19 03:05 T2 基线截图完成（CDP 通，7/7，无新 console 错误；新增 G5-8/9/10）。
- 2026-08-19 03:15 wip-mapper 归类完成：107 个 Rust 脏文件纯 fmt（含全部大 diff 与三个
  migration，零 schema 变更）；真特性两组（MCP server 拆分=完整；房间排序+UX=完整但携
  1 个新 tsc 笔误 + 1 个 HEAD 存量 tsc 死代码错）。落地顺序：修 tsc → 全套测试门 →
  fmt / MCP 拆分 / 房间 UX / gitignore 四笔提交。警示：01:44 的 cargo 进程疑为 tauri dev
  父进程，绝不可杀。
- 2026-08-19 04:05 C 线（worker-cskills）完工：worktree 分支 mp/skills-playbooks 两笔
  a0d85df8（G2-1 skill 硬伤修复 + experts.toml 8 locale×2）、fba95691（G8 四 playbook：
  patterns-map / research-writing / long-form-writing / line-review），全部工具名与
  host control action 逐一过了代码核对。工人核实顺带确认 G2-3 属实（host_control_room.rs:70
  能力文案仍称 owner）。待编排会话 merge 回主分支。
- 2026-08-19 04:05 wipland 停摆诊断（压缩期间无人落地，编排会话接管）：五笔提交零落地；
  后台产物考古：vitest 全量 03:24 剩 1 红（panel-layout:270 源码断言）→ 04:04 单文件复跑
  已全绿（修复在工作区未提交）；冷目录 cargo test --lib 三连 COLD_EXIT:127（cargo 报
  "error: test failed"，具体失败清单缺失）。已催工人交报告，编排会话并行自跑冷目录取证。
- 2026-08-19 04:05 用户新增 **G11 上游合并（排在最后）**：把 codeg 上游（origin=
  xintaofei/codeg）最新更新合并进本分支。**约束：若上游改动与我们的设计冲突（§0 红线、
  §4 裁决、RFC 拍板、mailbox/room 语义），必须先产出冲突分析、经用户同意后方可合并**；
  无设计冲突的部分才允许自主合并。执行时点：全部 G/R/UI/T 包收口、全量门绿之后。
- 2026-08-19 04:25 **WIP 落地完成（编排会话接管执行）**：wipland 停摆后，编排会话用
  rustfmt(HEAD) 逐字节比对重建分组（107 fmt-only 实锤；真改动仅 6 Rust + 前端一组），
  亲自评审全部特性 diff 后六笔落地：6fc87afe（test 修四处存量断言）→ d1f86680（style:
  fmt 107 文件）→ 7924e6cf（feat(mcp) 三 server 拆分：codeg-mcp/codeg-mailbox/codeg-room，
  legacy collaboration token 兼容、逐 server 独立 token、revoke_by_parent 清理）→
  4e4d0503（feat(room) 侧栏稳定排序 lastEventAt、mark_seen 不顶 updated_at、定位活动房间、
  多选对齐、行内回复/更多操作菜单、i18n×10）→ 887fcce8（gitignore）→ 77bbc5df（docs:
  G10 调研入库 SESSION-FORK-REWIND-SURVEY + 本文件增量）。工作树归零。
  门记录：vitest 347/347 文件全绿（单独跑；双负载并跑会出资源型 flaky）；服务器门 2429
  与 check --all-targets 沿用 wipland 已验绿；**桌面模式 cargo test 在本机以
  STATUS_ENTRYPOINT_NOT_FOUND 崩溃（冷目录与 PowerShell 均复现）——按手册 §4.4/§6 协议
  记录为本机 DLL 环境阻塞，编译成功+运行被阻两事实并存，不伪装成绿**。
  【08:0x 更正】wipland 所报"01:12 蓝屏记录"经编排会话查系统事件日志证伪（近 3 天无
  蓝屏/异常关机/重启事件，用户亦确认无蓝屏）——**K3 工人环境取证造假/误读**，其
  "7 个历史二进制全崩、PE 导入表审计、PATH 净化复测"等未经编排会话复核的取证一并
  降级为存疑，不作为结论依据。**编排会话亲手验证过的事实仅**：桌面 feature 测试
  二进制启动即 0xc0000139（冷目录重建、PowerShell 干净 PATH 均复现），同机 server
  套件全绿。根因未知；处置照旧走手册 BLOCKED 口径。教训：工人的环境类断言必须由
  编排会话亲测复核后才可入档（本次违反了 lead-executor-split 的机械验证纪律）。
- 2026-08-19 04:28 C 线 merge 完成：314321bf（--no-ff，skill 硬伤修复 + 四 playbook +
  experts.toml 8 locale×2，零冲突）。
- 2026-08-19 04:30 派发：**A 线 lane-a-rseries**（worker-k3 worktree，R1-R6 按 §4.4）、
  **B 线 lane-b-ui**（worker-k3 worktree，G5-2/3/6a/8/9/13）、**bonus-timer**（explore-k3
  只读考察 codex/session-timer worktree 遗产）、**bonus-web**（general-purpose@sonnet 档
  =K3 映射，调研 openteams workflow 形态 + linux.do 帖 2759945，本机有 opencli 可用）。
- 2026-08-19 04:55 **G12-B1 timer 遗产考察完成+裁决**：codex/session-timer 分支仅领先
  3 提交、落后主线约 150；其 timer 引擎已被主线同架构重写（同表同 migration，服务层收窄
  idle-only、类别调度、1s 扫描、host_control_timer），**不可 merge，只能摘想法**。净增量
  三件的裁决：**A at/interval 挂钟模式** = 真实能力空洞（automation 只会启动新会话，
  全系统没有"定时戳既有会话"的能力；at_time/interval_secs/next_fire_at 表列主线刻意保留）
  → 值得立项但属新特性且扩工具面，晨报提案**待用户点头**，接入时需补 source=timer 类别
  归位；**B repeat_idle 开关** = 琐碎，且主线有意选了"默认永续+指数退避限流"（RFC 0.5
  记录在案）→ 不做，记录即可；**C Goal 一键 Resume/Edit（队列重发 /goal）** = 与主线
  de470078 "No resume control" 的有意拍板相反 → 不擅动，晨报呈两面（人机友好收益 vs
  既有决策），用户裁。
- 2026-08-19 04:58 **G12-B4 完成**：lead-executor-split.md playbook 入库（编排会话亲笔，
  沉淀今晚实战纪律：机械验证不信执行者记忆、派单前查目标机制近史、接管先发停手令、
  同一步两次失败即停手上升、仓库台账为压缩后唯一事实源、报告必须带数字）+ SKILL.md
  与 patterns-map.md 索引指针。session.create 支持 model 钉档已核实（host_control_
  session.rs:477/616），playbook 所教均为真实能力。
- 2026-08-19 05:05 **G12-B2/B3 调研完成+裁决**（全文与裁决在
  MULTI-AGENT-ECOSYSTEM-SURVEY-2026-08-19.zh-CN.md）：**B2 workflow**——方向认可但属
  大特性，今晚不做；若立项走"Room 看板 + work_task 执行器 + 计划卡片"复用账本，先写
  RFC 回答与 taskboard/automation 的归并，**待用户点头**；白捡三件：三级验收词汇进
  delegation brief 惯例、ChainDepth 佐证链深=4、worktree 隔离已是纪律。**B3
  pi-shadow-mind**——确系多 agent（影子审计编排）；codeg 的 steer 注入即其插话原语，
  今天就能写 shadow-auditor playbook（候选，待点头）；其 300s debuff/插话循环翻车反向
  印证我们 R2 每轮一 steer + 5 分钟冷却 + 链深保险丝的保守缺省。Multica=issue 队列制，
  taskboard 远亲，无行动项。
- 2026-08-19 06:0x **G11 只读侦察完成（未合并）**：fetch origin 后，上游领先 27 提交
  （分叉点 ea5177ea = v0.26.1），我们领先 181。逐提交扫描：全部落在 markdown 数学渲染
  修复、codex 会话标题同步、grok token 统计、workspace 快捷操作菜单、composer @/斜杠
  面板打磨（2fd356a2）、ACP "responding 卡死"修复（1e3e5a10）、右键添加到会话
  （9d716876）——**零设计领域冲突**（不碰 mailbox/room/collaboration/queue/timer/MCP/
  skills）。文件重叠 36 个，预计纯文本冲突：connection.rs（我方 MCP 拆分+fmt vs 对方
  lifecycle 修复）、conversation_service/parsers（fmt 殃及）、i18n×10（双方加键，取并
  即可）、message-input.*（对方 @ 面板加宽 vs 我方 B 线 G5-13 复用同面板）。执行方案
  （主线收口后）：**worktree 里试合并**→解冲突→跑门→绿了再落主分支；如解冲突中发现
  语义级碰撞（非纯文本），停手写分析等用户裁。（拿本夜编排当参照负载：Claude Code 里子代理
  隐形、用完即弃；codeg 里 session.create 拉的工人=顶级常驻 Session，侧栏会炸）。现状
  查实：① `ConversationKind::Delegate`（⟺ parent_id 非空，插入即定不可改）已实现
  "harness 内部子代理嵌进父会话工具视图、不占侧栏"——这层已对齐 Claude Code，管不到
  host-control 拉的正规军；② parent_id 红线锁给 delegation，不可复用作编队树；
  ③ conversation.archived_at 列在、人类 UI 能归档，**agent 无 session.archive 动作**
  （session.stop 文案明示 never archives）；④ closed ≠ 隐藏，closed 可被信件冷启动
  （mailbox 根基，不可破坏）；⑤ Collection 全套动作（create/add_session/…）agent 已可用，
  session.create 已收 collection_id，且 **collection 自身有 parent_id 可嵌套**（战役→
  子编队纯靠 Collection 已可表达）。
  【05:5x 修正+补充，用户记忆核实】主动委派机制确已被用户删除（3ebcfa25 remove legacy
  delegation workflow）；`Delegate` kind 现为**只读兼容残留**——生产零写入点（残余写入
  全在 #[cfg(test)]，注释明言 "Production releases after delegation removal never call
  this path"），读路径仅剩 dispatcher 跳过历史行 + fork 把历史 delegate 行升为 regular。
  **harness 内部子代理不占侧栏的真实机制**是：导入层用 harness_internal/thread_source=
  subagent 标记隐藏（import_service.rs:329）+ claude 子代理转录由 parser 折进父会话
  工具视图（subagents/agent-*.jsonl 不参与会话发现）。G13-d 改判：Delegate 残留保持
  只读兼容、禁新增写入点，不清理（清理需迁移且伤历史库）。
  **session-as-node 裁决（用户问"要不要让 session 当节点、下挂 Collection"）：不做。**
  理由：①这正是用户删过一次的"特例"陷阱——第二棵树让每个 UI 面都要回答级联语义
  （父 session 归档/fork/删除时子容器怎么办）；②Collection 嵌套已能表达全部层级，
  编排者与其舰队同进一个 Collection 即可——**队长是编队成员，不是编队容器**（与
  5cbb3670 "members as equals, not owners" 同一哲学）；③指挥关系是账本内容（谁给谁
  发过任务信），不该固化成结构——层级会过时，账本不会。缺口用 G13-a（playbook 教
  带 collection_id 拉人）+ G13-c（created_by 元数据做过滤/分组）补齐即可。**立项 G13 编队生命周期（提案，待用户点头分级）**：
  G13-a 零代码=playbook 教编排者"先 collection.create 编队容器再拉人"（工具已存在，
  仅 skill 文案）；G13-b 小=host control 增 session.archive/unarchive（归档对私信投递的
  语义要先过 RFC：建议拒收+提示发件人，绝不能让信悄悄躺死）；G13-c 中=侧栏/列表加
  来源与归档过滤（codeg_owned/harness_internal 列可用；"谁创建的"目前未落库，RFC 题）；
  G13-d 记录=delegate 层无需动。原则：**不学 Claude Code 的"隐形舰队"**——codeg 的
  session 是同事不是工具，正解是可见性分层+编队容器+可逆归档，不是消失。（明确说是 bonus，排主线后、G11 前）：
  B1 timer worktree 考察（在飞）；B2 是否加 workflow 功能（openteams/multica 参照，在飞）；
  B3 linux.do 帖探索（在飞）；B4 **把本次"fable 规划者 + K3 执行者"编排模式沉淀为 codeg
  可复用参考**（待做：合并两线后作为第 5 个 playbook 写入 codeg-multi-agent/references/，
  内容含：贵脑便宜手分工、开工前 48h 机制核对、停手令/催稿、逐字节验证代替记忆、
  提交锁纪律）。另：用户提示 deja skill 可回溯聊天历史核对身份定位。
- 2026-08-19 08:3x **用户晨间拍板（六项决策清单逐条定案）**：
  ① G13 三级全批：G13-a playbook 编队纪律（零代码）；G13-b agent 可归档（语义定案：
  归档号**拒收新信并提示发件人**；用户澄清后确认归档=已自动隐藏，不重复做）；G13-c
  收窄为**来源筛选**（只看我开的/隐藏 agent 开的；需补 created_by 落库；"隐藏已归档"
  已存在不做）。② G9 按编排建议批：**开放 session 模型与思考强度调整**给 host
  control，**布局类不开放**。③ 挂钟需求定案为**整合进 Automation**：不给 timer 加
  at/interval 模式，而是给自动化补动作类型"定时给既有 session 排提示"（走队列
  normal 优先级，最低类不打断）；timer 保持空闲续跑专职。④ workflow **整体暂缓**
  （含 RFC，以后再说）。⑤ shadow-auditor playbook 批（零代码教程）。⑥ **Goal Resume
  批**（用户明确翻案 de470078："暂停理应能继续"）：Resume=把暂停前镜像的目标文本
  作为 /goal follow-up 经队列重发（忙时排队）；含 Goal Card Resume 按钮。
  执行序：先完成今晚收口（G6 门→T2 截图→G3 小批→G11 上游），新批次其后开工。
- 2026-08-19 15:3x **收口推进 + 新批次并行开工（用户加令："工人只写码、验证集中做"）**：
  ① 合并后唯一红测的修复落库 3c86beae：义务提示/欠账计数只统计 state='embedded' 的
  投递——信还在 pending/queued/本轮随行时是信封的职责，不入提示；三个测试补 embedded
  种子。门禁全绿：server 2436/0/1、vitest 348 文件 4348 全过、pnpm build 成功
  （gate-finisher 执行，全程未杀进程）。② wt/playbooks 验收合并 2950aaa8：
  lead-executor-split.md 增 Fleet hygiene 节（G13-a：一次任务一个 Collection、验收即
  归档、parent_id 红线），新增 shadow-auditor.md（决策⑤：疑点只升不横传、审计员不改
  码不指挥），SKILL.md 登记；语言按 references/ 目录惯例保持英文。③ 分工升级落地：
  开发工人各占独立 worktree 只写码+写测试、不编译不跑测试，主会话串行合并+集中跑
  门禁（避免 worktree 冷编译互抢 CPU 与 K3 在编译报错上空转）。在飞五工人：
  w-archive（G13-b 归档动作+拒收提示发件人、G3-3 schema 瘦身、G2-3 措辞、USAGE 同步）、
  w-g9-hostctl（决策②模型/思考强度）、w-automation（决策③排提示动作 + G13-c
  created_by 落库）、w-goal-resume（决策⑥ Resume 按钮 + G5-6b 孤儿键）、t2-capture
  （T2 截图对基线取证）。clippy 三连在 2950aaa8 上后台补跑；全仓 eslint（28 存量、
  零新增）仍在跑，收敛留待集中 prettier 一次性处理。
- 2026-08-19 15:5x **clippy 存量清账 + T2 截图 7/7 亲验通过 + 两个新发现**：
  ① clippy 三模式全红（lib 11 + 桌面测试 2，皆为 R 系列/车道存量非 3c86beae 引入）。
  逐条裁决后交 gate-finisher 清账：collaboration_service.rs:837 "identical if blocks"
  经亲读**判非 bug**（SQL 分支真分叉、values 本应相同，塌缩即可）；两处 too_many_arguments
  裁 `#[allow]` 不重构签名（G11 在即，改签名最大化冲突面，记为已接受债）；余者按建议机械修。
  ② T2：7 张 current-0x 对基线逐张亲验**全过**——工作区/分类树9项/房间成员栏/信箱
  收11发18/信件卡#291/会话中心筛选(等待回复1命中SessionC)/定时器药丸；差异均为数据演进
  （信件状态已读→已回复、筛选默认文案改"全部消息"），零渲染回归。Escape 三连截被
  dev 实例连环重启阻断（根因=我方主工作区 clippy/合并触发 cargo tauri dev 重编译，
  **非事故**；next dev :3000 一度 LISTENING 不响应，t2 已令低频观察待命，不许动进程；
  若施工完毕后仍挂死→上报用户申请重启授权）。**新发现（G5 备选）**：Session C 时间线
  首开时虚拟化列表只撑高度不渲染条目（scrollTop=0 视口全白，innerText 齐全，滚底再回
  才挂载）——待收口后立项。③ G3-1 亲核确认仍是死分支（access_for:107-113 无 room.post，
  195 行迁移提示不可达），已追加给 w-archive 与 G2-3 同文件批次做；G4-2 的 timer 前置
  条件（方向稳定）已满足，排 w-archive 落地后的下一后端批次。
- 2026-08-19 16:0x **用户拍板：G10 fork/rewind 实施整体暂缓**（"这个再说吧，估计比较
  复杂"）。含 §6 建议的全部四步（底座/claude 按消息 fork/codex thread-fork/重放降级）
  ——一步都不排。调研报告留档作资产；RFC 对账修订随暂缓一并冻结。与决策④ workflow
  暂缓同列"以后再说"栏。
- 2026-08-19 17:1x **换棒未遂 + dev 实例复活 + T2 关账 + 舰队换血**：① Fable 额度一度
  耗尽，交接单 HANDOFF-2026-08-19-leadership-relay 写成落库（a2fef815）后额度恢复、
  Fable 回归继续，交接未发生（单子留作快照）。② 八个 K3 工人被用户停止——活已全部
  合并落库，零损失；K3 当日额度尽，**用户令：子代理改用 Opus 5**（settings.json 已核
  无档位映射残留，"opus" 真解析）。③ dev 实例复活：僵尸 next dev（pid 63392，从
  LISTENING 不响应恶化为拒连）经用户授意击杀，`pnpm dev` 重拉 4.4s 就绪，webview 经
  CDP Page.reload 复活（codeg 进程 64608 全程未动）。④ **T2 真机验收关账**：Esc 三连
  截亲验合格——@ 补全弹层开→Esc 后仅弹层关、输入框"@"保留、页面未动、控制台零错误；
  加上此前 7 张场景图，T2 全部通过。⑤ 新开两个 opus 工人：w-source-filter（G13-c
  来源筛选 UI）、w-instructions-survey（用户新需求：跨 harness 指令文件管理调研）。
  ⑥ 全仓 eslint 后台任务在切换中丢失，结果作废，G4 批次时重跑。
- 2026-08-19 17:4x **G10 以收窄范围重开（用户发起）+ 分层架构定案**：用户研判了本地三个
  参考仓（Monet=纯原生无 ACP、Paseo=Claude/Codex 自写+长尾 ACP、ccgui=per-engine
  runtime adapter），提出 codeg 分层改造并点名 claude/codex/grok/cursor 四家要
  fork+rewind。编排裁决（与两份仓内调研收敛一致，Paseo 承重文件已抽验属实）：
  ① **运行时不换底**——ACP 继续当全部 13 家的默认运行时和长尾，不做 Monet 式全原生
  重写；② 新增**历史动作层**（history provider）：私约补丁打在现有 ACP 连接上
  （claude=桥加 _meta.claudeCode.rewindTo 循 #872，codex=桥内翻译 thread/fork，
  **绝不旁开第二个 SDK/app-server 通道——单一活写入方是铁律**，fork 必须在原 wire
  返回新 session id 以复用 persist_fork_outcome 全套落盘/防重导入）；③ v1 里
  fork=rewind：forkAtMessage 即非破坏回退，原地截断与文件回退划出 v1；④ 能力位
  照 Monet 形状（available/unavailable+reason_code），UI 不认引擎名；⑤ 锚点采集
  优先走 parser 输出带 uuid/turn-id（可能免建表，RFC 定）；⑥ **grok/cursor 两家
  先侦察后承诺**（wire 无 fork 广告、无同行先例），已派 w-fork-recon（Sonnet）出
  可行性档位报告（A原生/B文件手术/C重放降级/D不可行）。分工：RFC 增补由编排出稿，
  实施排 G11 上游合并之后。另：用户令子代理档位再降 **Sonnet 5**（opus 也太贵），
  在跑的 opus 工人跑完为止。
- 2026-08-19 晚 **"今晚能用"冲刺：新建会话修复 + 两个门禁漏网 + G13-c 落地 + 两份调研**：
  ① **新建会话死点修复**（8e6e8652）：房间标签 conversationId 为空撞上"草稿单例"裸
  谓词，任何分组内有房间标签时新建会话被吞成聚焦房间。三处查找改走 isDraftTab（该
  helper 及其"房间≠草稿"测试早已存在，只是查找没用）；回归测试双向证明（无修复红/
  有修复绿），CDP 真机鼠标事件验收通过（标签 18→23 草稿打开）。诊断全程按手册 §4.3
  活实例取证法。② build 类型门抓到 automation 来源漏网：SOURCE_LABEL_KEY 与 10 语
  i18n 补 sourceAutomation（349bfba9）。③ **G13-c 来源筛选合并**（c4bd6d2a，opus 工
  w-source-filter）：会话中心第 8 个筛选下拉 + 侧栏漏斗两开关（默认全显，未知来源按
  user，列表不可被筛空，Pinned/计数豁免沿既有先例）。④ **fork-recon 侦察入库**
  （8c19ad26）：四家档位 claude=A、codex=A、**grok=A 待实测**（重大新发现：第三方
  grok-app 实证 grok CLI 有 x.ai/rewind/points|execute 私扩展 + targetPromptIndex
  锚点，与我方 grok.rs:723 已读的 _meta/promptIndex 对齐；我方注释停在 1.0.4 旧握手，
  1.0.5 已发——**先导任务：升 1.0.5 亲测 initialize**）、cursor=C（重放降级，protobuf
  DAG 手术零先例高风险）。⑤ **指令文件调研入库**（4477320b）：AGENTS.md 事实标准
  （12/13 家）、Claude 用 @AGENTS.md import 一行接入、Windows symlink 不可用、
  "大而全"反模式有两篇 arXiv 硬证据、codeg 自身即漂移标本（CLAUDE.md 15 次 vs
  AGENTS.md 6 次提交）；建议排序 lint 诊断 > 幂等一行写入 > playbook（前置）> 现状，
  不自研全量生成器。**待用户拍板**：诊断扫描范围只管工作区还是含用户级目录（隐私
  预期）。派工模板修正：worktree 路径一律改正斜杠（反斜杠进 bash 被吞已两见）。
- 2026-08-19 深夜 **G10 fork/rewind 二次冻结（用户拍板）+ 属性跟随问题立案**：用户确认
  fork/rewind 现在不实施，调研历史全部保留（G10 survey、ACP survey §8 ccgui 案例、
  fork-recon 四家档位、4.4 分层定案，均已在库）。grok 1.0.5 升级先导任务随之一并冻结。
  用户同时提出关键设计问题记入未来 RFC 必答清单：**fork 时邮箱、义务、定时器等
  session 绑定属性是否跟随**。现状（今日头部 fork 链路 persist_fork_outcome）：
  folder/kind/model/git_branch/origin_cwd/选择器 pin/集合归属**跟随**，邮箱账本、
  义务、定时器、房间成员身份**不跟随**——新 fork 是白户。RFC 需逐字段裁决并给
  "fork 语义=分叉时点快照 or 白户"的统一原则，倾向按"消息历史分叉、协作身份不分叉"
  处理（fork 出来的会话不该自动继承房间席位和未清义务，否则一个债务人 fork 十次
  就有十一个欠条主体），但此裁决冻结至 RFC 阶段，不预定案。
- 2026-08-19 深夜 **Room 文件@ 缺失诊断定案 + 派工**（用户报障：room 里 @ 不出任何
  文件）：根因不是坏而是**从未接线**——rooms-page.tsx:391 roomMentionSearch 是手写
  单组搜索（仅成员+@all/@human），未接 useReferenceSearch；popup tab 栏渲染固定
  TAB_ORDER 四 kind（suggestion-popup.tsx:34），空组照样出 tab，故"文件"tab 可见
  却永远空。方案四步（已派 w-room-file-mention，Sonnet，wt/room-file-mention）：
  ① popup 加可选 tabOrder prop（默认原序，主 composer 零变化）；② room 组合搜索=
  session 组（原样）+ 标准搜索的 file/commit 组，agent 组砍（room 内无唤醒语义），
  folder root 取 detail.rootFolderId 解析，null 则文件组空（不猜当前激活 folder——
  room 跨 folder，猜错仓比空更糟）；③ room 传 tabOrder=[session,file,commit]，
  默认 tab 停在成员，@ 唤醒主流程不动；④ 时间线渲染补 file/commit 徽章（现状
  room-message-body.ts:291 把非 mention 引用降级 raw markdown 文本）。wake 解析
  天然不受 file URI 影响（只认 codeg://session|all|human），测试钉住。
- 2026-08-19 深夜 **新想法立案（用户提出，后议）：room/session 附加路径**——除绑定
  folder 根之外允许手动添加额外路径进 @ 补全/文件上下文；对远程 session（server
  模式下本地无 folder）尤有用；用户提及 paseo 似有此功能，待调研其形态。编排初判：
  数据模型上是 room/conversation 增 extra_paths 列表；前端 useReferenceSearch 需
  支持多 root（现为单 defaultPath）；server 模式文件树本就走后端读取，扩展点在
  use-file-tree 的多源合并与去重。定位为 room 文件@（本次 fix）的 v2 延伸，
  **不阻塞当前派工**，排队顺位在 G11 之后与 fork/rewind RFC 同批评审。
- 2026-08-19 深夜 **G4 死代码批次合并关账**（fcc303f8 + 修补 2fefd9ff，门禁全绿：
  clippy×3 零警告、server 测试 2464 绿/1 忽略、vitest 350 文件 4378 绿、build 过）：
  9 项落地——list 转发壳删除、send 收紧、DeliveryState::as_str、提醒唤醒去重、
  mail_box→scope 内部改名（wire 键 `box` 逐字节不动）、acp/delegation 与 idle_sweep
  历史名注释、房间未读徽章三处统一进组件、timer 死 autoPaused UI 清除（含 10 语
  i18n 键）。两项查证后免改：A1 字段早已随 R4 删净、C11 automation 直发路径已不
  存在（queue_prompt 落地后三分支全走队列/新会话）。编排裁决两则：**B8 不造
  mail_box serde alias**（git 史+tool_schema 证明 wire 从来是 `box`，无凭空兼容）；
  **A5 旧行残留可接受**（琥珀条消失但下次触发自愈+通用重置可手清）。门禁抓到工人
  两处漏网并由编排整合修补：同文件测试模块的裸 `list(` 调用（工人搜索只匹配带
  路径前缀的限定名——**教训入派工模板：删符号必须补搜模块内裸调用**）、send 降
  pub(crate) 后生产编译 dead_code（可见性收紧要连带想 cfg 归属）。
- 2026-08-19 深夜 **两项新分析并行启动（用户指令：都先分析）**：① 性能——用户体感
  Monet 丝滑、codeg 不够，派 w-perf-recon（Sonnet 只读）做静态对比诊断（zustand
  订阅粒度/消息流虚拟化/motion 高频路径/流式更新粒度/blur 密度），报告后编排用
  CDP 真机 profile 定罪再定改法；② G11 上游——已 fetch，**上游新 37 提交
  （ea5177ea..0935e1eb，v0.26.2）**，标题可见 qoder 内置 agent（疑似第 14 家）、
  两个 @ 面板改动（撞在飞的 room 文件@，我方先落、冲突编排解）、两个 ACP 修复、
  markdown 数学系列、Windows 路径修复、ACP bump（含 grok——待核是否覆盖冻结的
  1.0.5 先导）。派 w-upstream-recon（Sonnet 只读）出 A可直拿/B需适配/C设计矛盾
  三类清单，**C 类逐条交用户拍板**；正式合并仍排清理收尾之后。
- 2026-08-19 深夜 **两报告验收入库 + room 文件@ 关账**：① 上游分析入库（c90a6f35）：
  27 实质提交=12 直拿/10 适配/标题链待拍板；grok bump 1.0.5 命中冻结先导；ACP 绑定
  两修复"靶子函数已不存在+DbError::Conflict 同名反语义"禁止顺手合，立独立深挖任务。
  编排抽验三支柱属实。② 性能分析入库（57025081）：shiki 流式重分词/warm-cache=8/
  React Compiler 未用三嫌疑+六项证伪。上游合并**防复活红线（用户 2026-08-19 深夜
  补充）**：我方已抛弃/替代的功能绝不许经由合并回流——旧委派工作流（3ebcfa25 已删，
  上位替代=codeg-mcp 异步子智能体委托+邮箱/房间协作面）、ACP replay（13 家原生
  解析器已替代）、timer autoPaused UI、旧 CollaborationUnreadBadge、list shim、
  outbound_awaiting_summary 等清理产物均入黑名单；执行上 A 类 cherry-pick 逐个过
  黑名单，B/C 类手工重放天然免疫，一次性 merge 已禁；**拿不准是否属复活的一律
  停下交用户判断**。③ room 文件@ 合并关账（eee45d6c+修补
  6ed6bc8f，vitest 351 文件/build/eslint 全绿）：popup tabOrder 可配、room=会话|
  文件|提交、时间线 file/commit 徽章；门禁漏网两处编排修补（parsed.uri 可空、
  prettier×5）。真机验收：tab 结构/默认激活/零控制台错误全对，但**弹层选项数为 0
  （连 @all/@human 都没出）——定性中**；DB 亲查 agent-created room 的
  root_folder_id=1 非空，排除"设计内降级"，疑组合搜索把同步 session 组 block 在
  标准搜索 await 后，或 search 引用不稳定导致 popup 永远 stale。
- 2026-08-19 深夜 **用户双拍板 + 三项新需求**：① **性能方向纠偏**——静态分析两嫌疑
  被否"不是本质"；真实痛点=**打开 tab/拖拽布局/缩放卡**（布局交互路径），且用户
  点出 dev vs release 混杂变量（体验的是 next dev 未压缩+React development 构建）。
  行动：release 构建（--no-bundle）已在后台打，出来后同机对比复现，仍卡再对布局
  交互路径 profile；教训入长期记忆（perf-diagnosis-discipline）。**→ 实证闭环
  （同夜稍后）：用户亲测 release"确实相当丝滑"——dev 构建税定案为主因，静态分析
  的代码级嫌疑（shiki 流式重分词/warm-cache=8/React Compiler）全部降级为 backlog
  可选优化，性能主线关闭。顺带：release 首启白屏定性为 single-instance 插件互斥
  （dev 实例活着时二实例被拦），用户关 dev 后 release 正常；dev/release 数据库
  分文件（codeg-dev.db/codeg.db），"release 用 dev 数据"=停 release 后备份拷库，
  等用户配合执行。**
- 2026-08-19 深夜 **"开始用起来"交付批**（用户定验收线：bug 修+上游第一批→重打
  release 日常用，dev 线继续开发）：① **上游第一批 11 提交 cherry-pick 全部零冲突
  落地**（数学五连+Windows 路径+快捷菜单三连+Office 隐藏文件+ACP bump 含 grok
  1.0.5；跳过 881a2d06 版本号；防复活黑名单过检无复活）。唯一门禁红=上游新测试
  硬编码旧文案"Manage conversations"，我方已改名"Session Center"，断言对齐
  （422ba9ce）——上游测试断我方文案的先例记入 B 类适配套路。② **[DEV] 标题合并**
  （b71db438）：debug 构建主窗口题"Codeg [DEV]"（cfg! 常量折叠写法）。工人重要
  发现：**单实例互斥 5 月起就 cfg(not(debug)) 排除 dev**——白屏定性修正为
  "release 首启慢+重复双击，第二个 release 被第一个自己拦"（56K 空壳进程佐证），
  dev/release 共存本来就支持。图标调查结论：窗口运行时图标可做（icon-dev.png+
  set_icon，任务栏可辨），exe 资源图标需独立 bundle 不做。③ **@human Discord 式
  行高亮+@ 搜索 3 秒超时降级合并**（0029dc5e）：article 单事件粒度、border-l-2
  透明基线防跳动、超时后成员组必出（悬死根因另查，真机计时需 dev 实例空档）。
  ④ 三轮门禁全绿（vitest 356 文件/build/eslint/clippy×3/server 2464），worktree
  全部清理（dev-coexist/room-human-highlight/source-filter 遗留），**release
  重打中**——交付后用户换用，dev 线继续第二批（qoder 第 14 家/grok token/侧栏
  右键/@ 面板两连）与标题同步重放。② **标题同步采
  上游语义（DB 单一事实源）**——用户点出 overlay 双源实害：@ 面板/搜索读 DB 标题，
  overlay 只改 parser 输出，同一会话两处名字不一致。定案：列表同步 codex index→DB、
  用户改名锁定、频道传播异步化，在我方代码结构上重放实现，f56ec5e6 overlay 撤除或
  收编为 DB 同步数据源，排第三批。③ 新需求（Discord 参照截图）：**room 时间线
  @human 消息整行高亮**（淡底色+左侧竖条，Discord 式；@human 结构化提及底座已有，
  缺的是行级视觉）；**应用内存消耗检测**并入性能轮（dev vs release 对比时一并测
  进程内存）；用户定"开始用起来"验收线=room 高亮+性能内存+上游整合完成。
- 2026-08-19 深夜 **room @ 悬死根因定案 + 扫描护栏批合并 + 真机验收关账**：①
  根因（代码实证）：`list_workspace_files` 在 async 运行时线程上直接跑同步
  `ignore::WalkBuilder` 磁盘遍历且无条目上限；agent-created room 绑定 folder id=1
  = Thesis 巨型目录（kind=regular，无 .gitignore 剪枝）→ 遍历堵死 runtime →
  后续 invoke 全排队 → @ 弹层永久"搜索中…"，连同步 session 组都被组合搜索的
  await 一起卡住（上一条"选项数 0 定性中"就此闭案）；git_log 无辜
  （ensure_git_repo 快速失败）。② 修复合并（a6f968e2，工人 426b7430）：
  spawn_blocking 挪出运行时（弃 run_file_io 以避 FILE_IO_SEMAPHORE 串扰）+
  MAX_WORKSPACE_FILE_ENTRIES=50_000，主遍历/链接目录共享预算；get_file_tree
  同病同治（budget:&mut usize 穿递归）；3 个上限测试。③ room UI 打磨合并
  （39d1711f，工人 7f434623）：@全体冗余按钮+insertBadge 孤儿删除（grep 实证）、
  wake 预览人话化 ×10 语言（已知缺口：wakeHint 在 8 个非中文语言仍英文占位，排
  孤儿 i18n 第二轮）、头部留白。④ 真机验收（修复版 dev + CDP）：@ 弹层 ~2s 齐
  活——会话 4/文件 50/提交 50，控制台零错，Escape 清理干净；窗口标题
  "Codeg [DEV]" 共存标识生效。⑤ 门禁：server 测试 2467 绿/1 忽略、vitest 356
  文件、build、eslint、clippy ×3 零警告（desktop 档曾被 dev 实例文件锁挡，停
  dev 后补跑通过）。**用户手中 release（22:01）早于本批两合并（22:26/22:30），
  其 room @ 仍会悬死 → 重打 release #3 交付换用。**
- 2026-08-20 凌晨 **release #3 交付 + Tier-1 撞限额重派 + 用户休前指令批**：①
  release #3 打好（22:54，含扫描护栏+UI 打磨），已通知换用。② Tier-1 三工人
  （卡 responding 重放/标题同步重做/ACP 绑定审计）22:50 出发即撞会话限额全灭，
  零产出零损失；限额 00:10 重置后清掉两个空 worktree 原简报重派（-r 后缀）。
  ③ 用户休前指令：**梯队计划全量确认**（第一梯队=在跑三工人，第二梯队 G5 四项
  随后，第三梯队 grok token/qoder/附加路径/playbook 按排）；**扫描超时建议 10 秒**
  （防超大路径，落进附加路径批设计）；**全局 skill/用户级目录是否入 @ 与路径范围
  ——授权我参考 claude code/codex 源码后自定**；**换库授权自理**。④ 换库已执行：
  release 库三件套备份至 app.codeg/backup-release-db-2026-08-20-0016/，dev 库
  wal_checkpoint(TRUNCATE) 后整库覆盖，integrity_check=ok，会话 311/文件夹 3/
  房间 2 与 dev 逐表一致；下次启动 release 即 dev 全量数据。⑤ 派
  w-at-completion-recon（Sonnet 只读）：我方 @/$ 补全现状图 + CC/codex/opencode
  对标（扫描算法/上限/超时/取消/全局目录态度/多 root 合并）+ 附加路径批设计建议
  （数据模型/入口/10s 超时落点/值得引进的算法），报告出在
  AT-COMPLETION-RECON-2026-08-20。⑥ 用户问"指令文件扫描范围"何意——已解释
  （指令文件 lint 诊断只扫工作区 vs 连用户级目录，隐私预期问题）；该裁决与全局
  目录入 @ 的研究同源，合并为一份建议随侦察报告回来再定。⑦ 用户重贴两张
  22:08/22:11 截图均为打磨前旧 UI，所指问题已随 release #3 修复；composer 区
  "空荡"观感留待新构建真机复查，残留并入下一轮 UI 打磨。
- 2026-08-20 凌晨 **ACP 绑定审计验收合并（bc0aeaf1）+ 修复即刻派工**：审计判定
  **两洞俱在**——我方就是上游修复前的原始形态（update_external_id 裸写覆盖=
  fbb0ca06 删掉的旧函数逐字节同款；Branch A 先宣告后绑定、失败后状态焊死、二次
  调用跳过绑定块直发 turn；session_event_subscriber `let _ =` 丢弃返回值后无条件
  发 pending_prompt 为三处最裸）。编排抽验三处承重论断全属实。意外资产三件：
  自定义 agent 靠 read_chain_in+continues_from 对历史失联**半免疫**（原生 agent
  全裸）；fork 方向与上游相反 → 上游 A5 竞态**结构性免疫**；persist_fork_outcome
  / fork_session 已有"抢锁→校验→INSERT"事务与取消保护模板可直接抄。确认我方
  DbError::Conflict（CAS 可重试）与上游同名变体语义相反，修复禁用该变体（新设
  ExternalIdTaken 类命名）。7 条修复设计验收通过（含"设计 1 不带设计 7 会把自定义
  agent 重启误拆两会话"的交叉依赖），已派 w-acp-binding-fix（Sonnet）按报告实施，
  与在跑的标题同步工人同文件冲突归编排合并时解。
- 2026-08-20 凌晨 **卡"responding"修复批关账（bb689da1）+ 分析文档更正**：工人
  重放上游 1e3e5a10 全部 27 条意图，**增删行集合与上游逐字节一致**（编排亲自
  diff 复核为空），17 个回归测试整体移植；唯一跳过=seedDelegationsFromSnapshot
  上下文（我方 3ebcfa25 已删旧委派，防复活红线正确拦截）。**重要更正**：该文件
  4100-5900 区"我方大改"前提不成立——自 merge-base 的全部分叉只有删委派+
  conversationId 透传两件正交事（编排亲验 diff 属实）；上游分析文档所称"hunk
  大量重叠"是 cherry-pick 文本冲突层面，非语义冲突，后续 B 类适配的"重叠"标注
  按此理解。门禁全绿：vitest 356 文件 4514 测试（含新 17 个）、build、eslint
  零红。修复内容=状态机对后端失联的全面兜底（连接活性探测/markConnectionGone/
  多路由 reverseMap/重键代际/echo 门控通知），会话不再永久卡"回复中"。另：门禁
  后台任务的进程级 exit code 会被收尾命令污染，**以日志内显式 EXIT 标记为准**
  （本批任务级 exit 1 实为三绿）。
- 2026-08-20 凌晨 **标题同步批关账（95300618+fmt 916b4711+可见性修补）**：按用户
  拍板的"DB 单一事实源"落地——list_all 读路径先 spawn_blocking 读 codex
  session_index.jsonl，refresh_codex_auto_titles 分块（500/批）做带锁 CAS 同步
  （复查 title_locked/kind≠loop/harness_internal/活 folder/旧值相等，绝不 bump
  updated_at，精确返回改动集）；侧栏 upsert 内联保一致、Telegram 传播 tokio::spawn
  摘挂+收敛循环（每轮重读标题，防止迟到的自动同步盖掉手动改名）。**免迁移**：
  title_locked 列及 lock/retitle 机制我方早已有（m20260608，比上游更通用），上游
  5 提交也从未建迁移——工人核实非假设。编排批准四项设计偏离：① overlay 保留但
  **收编为同步数据源**（撤的是双源实害而非解析逻辑；无 DB 行的导入扫描仍需它，
  上游同样保留）；② 3e8148c5 证实为无关测试修复，**标题链实为 4 提交**（分析
  文档更正）；③ CAS 范围比上游更严（对齐 list_all 可见集，防隐藏行被 upsert
  广播进侧栏）；④ 只接 list_all（导入路径本就 DB 为准）。门禁：server 测试
  2467→2483 全绿、clippy ×3 零警告；漏网两处编排修补——fmt 残留、新参数结构体
  pub(crate) 撞 pub 函数（private_interfaces）。用户可感收益：别处改的 codex
  会话名下次列表刷新即达侧栏/@ 面板/搜索，手动改名永不被盖，频道 topic 迟到但
  必收敛。
- 2026-08-20 凌晨 **@ 补全对标侦察验收合并（b2ac633c）+ 三项编排裁决**：报告结论
  ——我方缺的只是"单 root 假设"（useFileTree/list_workspace_files/RoomDetail 全
  链单路径），四类来源+分组降级架构够用；对标最值得抄的是 Codex file-search
  "一次遍历吃所有 root、统一预算与取消"的架构，CC 位图前缀索引/opencode frecency
  在 50k 量级用不上。裁决：① **全局目录（~/.claude 等 skill/指令目录）不自动入
  @ 范围**——Room 是多 Session 共享空间，隐私语境异于单用户 CLI；想要就走手动
  "加路径"同一入口（用户委托事项就此定案）。② **10 秒超时落后端**：deadline 加
  在 walk_workspace_files 循环内与 50k 硬顶并列为第二跳出条件——前端 Promise.race
  不取消底层调用、治不了线程占用（Thesis 故障机制本身），采纳用户 10s 建议。
  ③ 附加路径批组成照报告：collaboration_room_path 表+命令对+useFileTree 多
  root+后端 deadline 为第一批；"…"菜单细节/截断原因标记/Session 侧对等第二批；
  frecency/前缀索引/fs 监听/完整取消令牌链一律不做。**paseo 线索证伪**（编排
  亲验）：其 additionalDirectories 是 Claude Agent SDK 沙箱权限透传（agent 可
  读写哪些目录），非 @ 补全多路径，UI 层零命中——用户记忆混淆了两个概念，此条
  已可向用户澄清。指令文件 lint 扫描范围联动建议：默认工作区、用户级目录做
  显式开关（诊断是用户本机自用，语境异于 room 共享，不同结论并不矛盾），待用户
  确认。
- 2026-08-20 凌晨 **ACP 绑定修复批关账（合并+四轮门禁修复），Tier-1 三件套完成**：
  工人 9 提交全落——bind_external_id（抢锁→锁下读→先拒→续接豁免→拆分保留）、
  ExternalIdTaken 新变体（事务内局部枚举防 TransactionError 拍扁）、先绑后宣+
  取消保护+Branch B 失败软删、lifecycle/host_control/subscriber/spawn_bound_agent
  （工人自查出的审计遗漏第五写点）全迁移、subscriber 冲突拆路由带"事件仍当前"
  防误杀、header 落盘 2s durability；故意保留 update_external_id 唯一调用方=内建
  agent 死 id 纠偏（过拆分逻辑会造幽灵行，注释写死理由）——编排验收批准。状态
  映射 InProgress→Cancelled 循 handle_terminal_event 先例。**门禁四轮修复
  （全编排出手）**：① E0382 emitter 移动后借用（.clone()，EventEmitter 移动语义
  惯犯）；② BindOutcome 缺 Debug（expect_err 约束）；③ **真逻辑 bug**——拆分
  路径先插保留行后改原行，撞逐语句执行的唯一索引；单事务内原子性使"先保留"的
  崩溃安全论证失效，改为先释放 prev 再插入，**工人自写的 5 个测试逮住自己的
  bug**（测试先行纪律样板）；④ 既有 fork stale-binding 测试期望对齐新语义
  （setup 重绑如今合法产出保留行，断言 1 行→2 行并验 S1/S3 归属）。终局：server
  测试 2483→2496 全绿、clippy ×3 零警告；前端未动（上批 vitest/build/eslint 绿
  仍有效）。风险留档：_prompt_guard 未随 detached task 移交（残窗最坏=多一行
  可见可删空行，对比修复前必然焊死错投为净改善）。
- 2026-08-20 凌晨 **release #4 交付（01:53，含 Tier-1 三件套）+ 体验批派工 + G5
  快赢批关账**：① release #4 打好已通知换用。② 派三工人：w-room-extra-paths
  （附加路径第一批照侦察报告：collaboration_room_path 表+命令对+多 root 统一预
  算+后端 10s deadline+"…"菜单最小入口）、w-pane-maximize（窗格临时放大，视图层
  标记不动布局树，herdr 参考）、w-g5-quickwins。③ G5 快赢批合并关账（bdecb872+
  8c5a501c+编排修补 68099938）：定时器删除/成员移除加 AlertDialog 确认（工人先
  核实真实删除语义再写文案——**纠正编排简报的臆测**：成员读游标实际会删，保留的
  是时间线；采 AlertDialog 主流模式而非同文件 Dialog 孤例，裁决正确）；成员数
  "N 位成员"改原生 button 开合成员面板（title 而非 aria-label，避免与图标按钮
  同名撞 getByRole）。门禁漏网编排修补：两个**既有**测试拿整串副标题当单一文本
  断言，成员数包进 button 后文本被拆——断言对齐新结构（正则半串+按钮角色）；
  prettier ×2 eslint --fix。终局 vitest 356 文件全绿、build 绿。工人再确认：
  Room 成员管理键组在 7 非中文语言仍英文占位——孤儿 i18n 第二轮存量+1。
- 2026-08-20 凌晨 **窗格临时放大批关账（合并+编排修补 25539084）**：用户需求
  （tmux zoom 式 focus）落地。工人关键发现：仓内已有同构先例 filesMaximized
  （workspace-context，文件面板最大化），隐藏手法（invisible+inert）、按钮视觉
  （Maximize2/Minimize2）、十语言词条全部沿用——全应用交互一致。实现：tab-store
  加 maximizedGroupId 纯视图标记（**不进持久化白名单**，重启即复原；
  switchWorkbench 显式清空防 ROOT_GROUP_ID 跨工作台假匹配）、纯函数
  shouldExitMaximizedGroup/isMaximizeRestoreEscape（树引用变/放大组不再是叶/
  焦点移到别组→自动退出；Esc 让位弹层与可编辑焦点）、放大组 rect 强制铺满其余
  组隐藏不卸载、分屏把手放大时不渲染、双击标题栏切换（closest 排除菜单/输入）、
  tab 带尾部按钮。herdr 对照印证布尔标记设计，其"拒绝操作"式改为更贴 GUI 的
  "自动退出再执行"。门禁漏网编排修补：**工人自己的测试逮住实现分支缺失**——
  activeGroupId=null（瞬时无活跃 tab）被当焦点错位误退出，改为 null=无信号不
  退出（测试意图为准）；prettier ×2。终局 vitest 358 文件全绿（+2 新测试文件）、
  build 绿。真机验证清单挂账：重命名弹窗开着时双击标题栏不得误触放大。
