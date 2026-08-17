# 交接：统一消息调度（2026-08-18，会话中断于前端阶段）

> 上一轮会话因额度耗尽中断。后端已全部完成、测试全绿、已提交；前端与文档没做。
> 接手前先读本文件 + `docs/session-workbench/SESSION-TRIGGERS-GOALS-AUTOMATION-RFC.zh-CN.md`（注意：该 RFC 仍未入库、且与现实有偏差，见下文"文档欠账"）。

## 一、这轮已经完成并提交的东西（后端）

一个提交包含三块相互依赖的改动（迁移 `m20260818_000001_message_scheduler`）：

### 1. 修复催办配额饥饿 bug（用户报的"已读未回不提醒"的根因）

- 根因：`reminder_repeat_count`（上限 3 次）按**会话**累计，且只要还有欠债就永不重置
  （`reset_idle_reminder_cursors` 排除仍有 awaiting_reply 的会话）。未读阶段每 5 分钟
  steer 一次，3 次配额烧光 → 信被读了、进入"已读未回"阶段时配额已是 3/3，一次也催不出来。
- 修法：`list_overdue_reminder_targets` 现在算 `newest_due_at`（欠债集合里每个成员的
  "到期时刻"：未读=created_at，已读未回=agent_received_at+5min 取最大）。若它晚于
  `reminder_last_at`，视为**新欠债周期，配额清零重算**（冷却 5 分钟照旧防刷屏）。
  `record_successful_reminder` 改为写"观测值+1"而非盲目 +1（签名多了
  `completed_repeat_count` 参数）。
- 回归测试：`reply_phase_gets_a_fresh_reminder_budget_after_unread_nags`
  （collaboration_service.rs 测试区，用 `datetime('now','-X minutes')` 回拨伪造时间）。

### 2. 队列类别调度（prompt_queue 加 `source` 列）

- `conversation_prompt_queue_item` 新增 `source` 列：`user | collaboration | reminder | timer`，
  迁移按痕迹回填（origin_event_id→collaboration，dedupe `mailbox-attention:%`→reminder，
  dedupe `timer-fire-%`→timer，其余→user）。
- **claim 顺序 = 类别优先，类内 FIFO**：user(0) > collaboration/reminder(1) > timer(2)。
  两个常量 `CLASS_ORDER_SQL` / `CLASS_ORDER_SQL_Q`（带 q. 别名版），snapshot、
  send_is_admitted、claim、reorder 内部 SELECT 全部用同一顺序——改任何一处必须同步全部。
- `EnqueuePromptQueueItem.source` 是 `#[serde(skip_deserializing)]`：**客户端伪造不了类别**，
  只有宿主运行时（timer/reminder）能在 Rust 里设置。
- 前端零改动兼容：快照按类别序返回，用户把 timer 项拖到自己消息前会被快照"弹回"，
  类内拖拽照常。`reorder` 里位置池现在显式 `sort_unstable()`（类别序下行序不再单调，
  不排会打乱类内相对顺序——别删这行）。
- 新增 `claim_first_steerable`：忙碌时定向 claim 第一个带 `steer_if_supported` hint 的信件项
  （JOIN collaboration_delivery，queue item id == delivery id）。修掉了旧缺陷：以前严格
  claim 队头、steer 信件被人类草稿挡住就永远注入不了。`process_native_steer` 已换用它。
- 测试：`claim_walks_classes_user_then_letter_then_timer`、
  `steerable_claim_skips_queued_user_drafts`（注意 origin 项协议：claim →
  `mark_dispatch_started` → accept/release；`mark_origin_queued` 要求 embedding 态，
  测试必须走完整台阶）。

### 3. Timer 义务感知退避（用户点名的 corner case："下发任务等回信时别催"）

`session_timer.rs` fire 前判定链（顺序重要）：

1. 宽限期未到 → 跳过。
2. `latest_mailbox_info_at`（新收到的信 / 自己的出站义务被解决，二者取最新）对比
   `last_fired_at` 判"有没有新信息"。**秒级截断坑**：DB 时间经 `datetime()` 只有秒精度，
   `last_fired_at` 带亚秒，比较前必须 `fired.with_nanosecond(0)` 截断，否则同秒到达的
   回信会被判成旧信息（已修，有测试）。
3. `outbound_awaiting_summary` 查"我发出去还没人回的信"（obligation_state='awaiting_reply'）。
4. 无新信息 && 在等回信 = "重复戳"：间隔 = max(grace, **300s**) × 2^strike，上限 **30 分钟**
   （300s 起步是对齐收件侧催办的 5 分钟节律，给对端留回复时间；grace 默认才 2 秒）。
5. strike 达 **3** → 不再 fire，置 `auto_paused_at`/`auto_pause_reason='waiting_no_progress'`，
   **enabled 不动**（这是退避拉长到"直到有新信息"，不是替用户做决定）。任何新信息或用户
   任意编辑（update 无条件清 strike+auto_pause）自动复活。
6. 队列里已有 queued 项 → 让位跳过（队列自会续命，不叠加第二轮）。
7. 运行时缺失（被 idle_sweep 3 分钟回收——退避 5-30 分钟必然踩中）→ **照样入队+唤醒**，
   走 dispatcher ensure/resume，与信件同路。旧断言"无活连接不 fire"已按新设计反转。
8. fire 时正文 = 用户配置文本原样 + 宿主事实附注（第 N 次续跑/距上轮结束时长/几封信未回、
   最早发往 #谁、等了多久/连续第几次无新信息），中文、与催办摘要同文风，仅在
   有等待或有 strike 时附加。

新库函数：`collaboration_service::outbound_awaiting_summary` / `latest_mailbox_info_at`；
`session_timer_service::claim_fire(…, strike_count)`（签名变了）、`auto_pause`、
update 清零逻辑。timer 表新列：`strike_count`、`auto_paused_at`、`auto_pause_reason`。

测试（session_timer.rs 5 个 + service 1 个）：退避到 5 分钟线、3 次刹车+回信复活、
missing-runtime 入队、队列非空让位、auto_pause 不动 enabled 且编辑即清。

### 测试状态

`cargo test --no-default-features --lib`：**2372 过 / 1 挂**。挂的
`commands::host_control::tests::catalog_filters_writes_by_token_bound_policy`（5≠4）
**在 HEAD 上就挂**（已 stash 验证），属 dispatcher WIP 预存债务，与本轮无关。
另有既有预存：3 个前端 vitest 失败（compaction 卡×2、codex 子代理命名）+ 5 个 clippy
错误 + 全仓 rustfmt 不齐。桌面模式 `cargo check` 需先关掉正在运行的 codeg
（锁 codeg-mcp.exe）。

## 二、剩余工作（按优先级，接手照此执行）

### A. 前端（半天量，改动面小）

1. `src/lib/types.ts`：`SessionTimer` 加 `strikeCount: number`、`autoPausedAt?: string | null`、
   `autoPauseReason?: string | null`；`PromptQueueItem` 加
   `source: "user" | "collaboration" | "reminder" | "timer"`。
2. `src/components/chat/session-timers.tsx`：timer 行下加状态行——`autoPausedAt` 非空时显示
   amber 提示 + "立即恢复"按钮（调现有 `update(timer.id, { enabled: true })` 即可，后端任何
   update 都清 strike/auto_pause）；可顺带把 strikeCount>0 显示为小字。
3. i18n ×10（`Folder.chat.sessionTimers` 下加两个 key）：
   - `autoPaused`：zh-CN "已退避暂停：等待回信期间连续无新信息。收到新信息会自动恢复。"
     en "Paused by backoff — no news while waiting for replies. New information resumes it automatically."
     （其余 8 语言照译；上轮用 node .mjs 脚本改 10 个 json 最稳，Python 在这台机上写
     en.json 出过 OSError）
   - `resumeNow`：zh-CN "立即恢复" / en "Resume now"
4. `session-timers.test.tsx` 补两条：auto-pause 渲染、恢复按钮调 update。
5. 跑：`pnpm eslint .`、`pnpm test`（避开 3 个预存失败）、tsc（pnpm build 或编辑器）。
6. 提交：`feat(timer): surface backoff auto-pause with one-click resume`。

### B. 文档欠账（重要，用户多次强调"决策必须落库"）

1. `SESSION-TRIGGERS-GOALS-AUTOMATION-RFC.zh-CN.md` **至今未 git add**。要做对账修订后提交：
   - 标注已落地现实与原稿的偏差（timer 已是 repeat-while-idle 而非 one-shot 默认；
     grace 默认 2s；无限续跑的实际约束 = 本轮的等待退避+3 次刹车）。
   - 新增"统一消息调度"章节：类别优先级（user > letter/reminder > timer + 类内 FIFO +
     人类只能改自己类内顺序=局部优先级）、投递激进度三档（等/显式 steer/人类显式打断，
     与排序解耦，urgency 永不参与调度）、timer 退避判定链（照抄上文）、corner case 表。
2. 新写 rooms/人类信箱调研构思文档（用户明确说"不实现代码的话写详细调研构思文档"）。
   **先读并行会话的提交 `06c3309f`**（改了 GROUP-CONVERSATION-RFC.zh-CN.md 和
   PRODUCT-SPEC.zh-CN.md，标题就是 "unify mail threads rooms and human inbox"）——
   若它已覆盖下述结论则只做补充对账，别另起炉灶重写。核心结论已定：
   - **rooms ≠ 邮箱稍微改改**。cccc 实证（本地 `D:\code\revisiting\work\repo_audit\repos\cccc`）：
     room = 一本群 append-only 账本 + 每成员只进不退读游标 + 收件人过滤函数，私聊群发同库，
     持久层只有一份；codeg 邮箱相反是每收件人一行投递拷贝、各带已读/义务/收据。两种存储
     哲学服务不同保证（共享历史+成员制 vs 强投递+欠账追踪）。硬改会把义务表、回复链深度
     保险丝（=4，刻意防对喷，别动）搅进群聊语义。要做 room 就照 cccc 形状建独立小系统
     （room/room_member/room_event 单份日志+游标），**戳醒成员复用本轮调度**（room poke
     就是一种 class 1 队列项）。回复链 thread 已经覆盖"围绕一封信的连续交流"（信箱阅读面
     就是 thread），room 补的是"群体共享可见性"，两者不冲突。
   - **人类信箱：要做，但轻量化**。代码里 `ReminderAudience::Human` + `HumanOverlay` lane
     早已占位（collaboration_reminder.rs，注释 "not implemented"）。v1 方案：独立小表
     `human_notice`（不动 collaboration 表），MCP 加 `notify_user`（或 send_message 支持
     target "human"），全局铃铛 UI + 已读，**绝不 spawn turn**，人类"回复"=点跳转到对应
     session 输入框。人类不欠 agent 回执，不进义务机器。
   - **list_inbox 过滤参数**（用户问"过滤能不能收成一个变量"）：维持显式参数。三个参数
     （box/filter/peer_session_id）本就是可自由 AND 组合的正交轴；收成 DSL 字符串对弱模型
     （用户拿 deepseek/kimi 当 agent）是灾难——schema 枚举校验没了、拼写错误静默失败。
     等过滤维度超过 4 个再折叠成一个**结构化 where 对象**（仍是 JSON 严格校验，不是字符串）。
   - **不新增"等待声明"工具**：send_message(expects_reply=true) 本身就是声明（义务表），
     timer 现在会读它；阻塞式 wait_message 维持暂缓（退避+来信自动唤醒已等价）。
3. 提交 docs。

### C. 真机冒烟（可选，环境已备好）

- `codeg-server` 已在 target 编译好（debug）。思路：`CODEG_DATA_DIR=临时目录`、
  `CODEG_PORT=随机`、`CODEG_TOKEN=test` 起服务，HTTP API 建两个会话互发信，观察：
  信封投递、催办在"已读未回"阶段仍会发（本轮修的）、timer 退避（把 timer 表
  last_fired_at/strike 手动 UPDATE 加速）。
- 用便宜模型（用户指定 deepseek/kimi）：机器上 `claude` CLI 走本地代理 127.0.0.1:8317，
  档位映射 haiku→deepseek-v4-flash、sonnet→k3；codex/gemini CLI 也在。
- 提醒用户：**正在运行的 codeg 必须重启**才吃到新后端/迁移（上轮 MCP schema 变更同理）。

### D. 记忆维护

完成 A/B 后更新
`C:\Users\63036\.claude\projects\D--code-revisiting-work-repo-audit-repos-codeg\memory\codeg-trigger-goal-decisions.md`
（追加：本轮提交号、退避参数定案 300s/×2/30min 上限/3 次刹车、饥饿 bug 修复、
类别调度落地、rooms/人类信箱结论）。子代理纪律见 memory/subagent-cost-discipline.md
（探查必须钉便宜模型，fable 子代理默认禁用，重活自己干）。

## 三、最容易写错的五个地方（给接手者）

1. **claim/快照/准入/reorder 四处排序必须同一**（CLASS_ORDER_SQL 两个常量），改一处漏三处
   会出现"准入门检查的头 ≠ worker 实际 claim 的头"。
2. **origin 项状态台阶**：claim → mark_dispatch_started（delivery 进 embedding）→
   accept/release。跳台阶会报 "no longer being embedded"。
3. **时间比较全走 Rust 侧 + `datetime()` 归一 + 秒级截断**；DB 里 CURRENT_TIMESTAMP 与
   chrono 绑定值格式不同，SQL 里裸字符串比较会翻车。
4. **auto_pause 永不动 enabled**；用户 update 无条件清 strike/auto_pause——前端"恢复"按钮
   靠这个，别在后端加条件。
5. **strike 只在"无新信息且有未回义务"时累积**；纯本地干活的 goal 循环保持原行为
   （宿主判不准"本地零进展"，误刹更伤；转写增量启发式记为 v2）。
