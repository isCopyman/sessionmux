# 可维护性收敛计划（Goal）

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
  - G3-3 schema 摘掉 legacy delivery_mode / delivery_hint（解析层保留兼容，防老 companion）。
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
  - G5-6 ✅ 【2026-08-19 用户拍板：人类写信 UI 不做（对 agent 说话=直接在它对话框打字）；
    Human Inbox 暂不做】→ SessionMessageComposerDialog 死代码转为**删除包**：删组件+其测试+
    banner 测试里的陈旧 mock；等 composer-probe 探针确认完整引用集后执行。human_notice
    存储层两案之争随之封存（不实施，不再列拍板项）。
  - G5-7 已撤销：信箱对人保持"看/审计"用途，0 信时无入口属合理，不再改。
- **G6 收口**：全量测试（前端 vitest+build、Rust 桌面+server+mcp 三套）、本文档终版、记忆更新、总结报告。

## 3.5 决策轨 D1：工具面形态——MCP 还是 环境变量+CLI+skill（2026-08-19 用户提出）

用户提出：是否把全部 MCP 换成"环境变量注入 + CLI + skill"，理由是灵活性、且多个参考项目
如此做。此决策**先于 G3 执行**（若传输形态要换，工具改名就是白干），处理方式：

**结论（2026-08-19 证据齐后定稿）：不做全量替换；三个兼容小刀吸收该提案的真实收益。**

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

## 4. 多 agent 交流与 UI 的设计判断（编排会话本人负责思考，审计后回填）

- mailbox/Room 协议层：设计已由拍板稿冻结且基本落地；本计划聚焦"让 agent 和人真正会用"
  （文档示例、skill 清晰度、工具描述一致性），不动协议。
- 待回填：agent 视角链路走查发现的摩擦点；UI 设计空间清单。

## 5. 进度日志

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
