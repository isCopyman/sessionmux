# 任务看板补课 RFC（O46，2026-08-21）

- 来源：用户提议（引入 Trellis/multica 式看板）→ r-taskboard 只读侦察
  （前提修正：看板早已存在）→ 领导出本 RFC。
- 用法：**过目后排期施工**。每个"待拍板"点用户点头才动手；不点头的项
  默认不做。
- 说人话版一句话：看板已经有了（四泳道、看板/列表双视图、拖拽都在），
  这份 RFC 只补四件事——**任务等人时主动喊人、看板能按 agent/仓库分组、
  attention 列轻重分明、卡片能看出 agent 是不是活着**。另加一个可选的
  第五件：烧钱可见（O47 遗产）。

## 1. 现状地图（全部本人读码核实，file:line 可点）

- **状态机比 Trellis 细**：10 态
  `todo→queued→preparing→running⇄awaiting_input→review→merging→done`
  + `failed`/`canceled`，带 CAS 转移不变量（models/work_task.rs:4-40）。
- **awaiting_input 的确切含义**：引擎订阅 Question / Permission /
  PlanApproval 的请求与解决事件，用 outstanding-request-id 集合翻转
  `running ⇄ awaiting_input`（work_task/engine.rs:10-13）。翻进这个态
  = **智能体正卡在一个只有人能回答的问题上**。
- **四泳道映射已钉死**：board-columns.ts:22-27 ——
  todo={todo,queued}、inProgress={preparing,running}、
  attention={awaiting_input,review,merging,failed}、done={done,canceled}；
  board-columns.test.ts 保证每个状态恰好归一列（加状态漏归列=测试红）。
- **双视图共享过滤词**：列表视图按整列过滤（filterTasksForList，
  board-columns.ts:114-128），看板/列表说同一套话。
- **召唤层已经存在，但只盖了一半**：tasks-view-context.tsx:162-183
  `notifyFlips` 在 fetch-to-fetch 状态 diff 里对翻转发系统通知——但
  :170 的过滤条件只放行 `review` 和 `failed`。**awaiting_input 翻转被
  这一行排除**，这就是"等授权却不喊人"痛感的确切位置。
- **通知管道全链路已通，不用新修路**：后端推 WORK_TASK_CHANGED_EVENT →
  refetch → notifyFlips → sendSystemNotification（lib/notification.ts；
  窗口可见时静音 :8；桌面走 commands/notification.rs 的系统通知，web 走
  浏览器 Notification API）。Provider 挂在 workspace 布局层
  （app/workspace/layout.tsx:1247），**不需要打开任务页就能收到**。
- **attention 徽标已有两处消费**：sidebar.tsx:216、
  quick-actions-dropdown.tsx:266（计数=attention 列非归档任务数）。

结论：r-taskboard 的判断成立——"做个看板"是伪需求，缺的是看板上的
四个具体器官。

## 2. 四个缺口的设计

### 缺口 ①：awaiting_input 不喊人（最痛、最便宜，建议第一期）

- **改法**：notifyFlips 的放行条件加 `awaiting_input`，新 i18n 键
  `notifyAwaitingInput` ×10 语言。改动就在现有函数里，有 review/failed
  两个先例照抄。
- **防疲劳**（必须一起做）：`running ⇄ awaiting_input` 会反复翻转
  （每个 Question/Permission 都翻一次）。同一任务短时间内多次翻入
  awaiting_input 只通知第一次——前端 per-task cooldown Map 即可，
  不落盘。
- **待拍板 A**：cooldown 时长。建议 5 分钟。
- **待拍板 B**：要不要按轻重分级（multica 的 inbox_item 按 severity
  升级是参照）：failed=高、awaiting_input=中、review=低。桌面通知本身
  没有优先级 API，分级只能体现在文案和重复策略上。**建议第一期不做
  分级**，只补 awaiting_input 这个洞。
- 点击通知跳转到任务：桌面通知目前只 show 不接点击回调
  （commands/notification.rs:22-31）。第一期不做，记为后续项。

### 缺口 ②：分组维度写死（只按状态）

- **改法**：加"泳道分组"选择器：无（默认）/ 按 agent / 按仓库(folder)。
  实现为**列内二级分段**（四列不动，列内按组分段显示），列表视图加
  同样的分组头。不改 columnForStatus，不碰拖拽语义。
- **待拍板 C**：默认关；选择是否记进 localStorage（viewMode 已有先例，
  tasks-view-context.tsx:79）。建议记住。

### 缺口 ③：attention 列轻重混杂

一列里挤着四种语义：awaiting_input（等人回答）、review（等人验收）、
merging（正在合并，其实不等人）、failed（死了）。

- **甲案（推荐）**：不动列结构。列内按严重级排序 + 视觉分层：
  failed 红、awaiting_input 琥珀脉冲、review 中性、merging 弱化。
  改动小，10 态→4 列的测试钉不动，拖拽/过滤词地基不动。
- **乙案**：拆第五列。布局破坏大（四列是拖拽与双视图过滤词共享的
  地基），不推荐。
- **待拍板 D**：甲 / 乙。
- 附注：merging 归 attention 是有意设计——卡片不能在用户点合并的
  瞬间跳列（board-columns.ts:50-53 注释）。甲案保留这个语义，只把它
  在列内弱化。

### 缺口 ④：卡片无 agent 活动指示

- 参照：openagents 的 IssueAgentActivityIndicator。
- codeg 已有同族先例：O34 的工作台活动点（busy=Prompting 的连接状态,
  workbench-tree，本批已上线）。任务在 running 态有关联会话。
- **改法**：任务卡加活动点——running 且其会话连接在 Prompting →
  绿色脉冲；running 但不在 Prompting → 静止点（在等工具/子进程）。
  数据源复用 acp-connections-context 的连接快照，纯前端推导。
- **待拍板 E**：要不要加"最后输出 X 秒前"文本。需要 last-activity
  时间戳，连接快照里是否现成、刷新频率是否够，施工时先核实再决定
  （核实结果若要新增后端字段则升级回拍板）。

### 缺口 ⑤（可选，O47 遗产）：烧钱可见

- O47 定性结论：额度剧耗不是失控 bug，是"燃烧不可见"。要求"正在烧钱"
  成为一个可见维度（每会话调用频次 + 近时开销估算）。
- 数据源 token_usage_turn 表已存在（fork-rewind 侦察核实过 turn_key）。
- 这是独立小项目（聚合查询 + 刷新策略 + 卡片/会话卡两处 UI），
  **建议单独排期，不挤本批**。
- **待拍板 F**：做不做；这批做还是下批。

## 3. 明确不做（负空间）

- **Room 编队派工不进 work_task**。两者生命周期语义不同（编队消息
  没有 preparing/merging/review 这些阶段），硬统一会造出第二个
  "双时间线"问题（模型体检 RFC 切面 ① 的教训）。编队状态可见性走
  O34/会话中心一族，不走任务状态机。
- **不引入外部任务系统**（Trellis/multica）：我们的状态机更细，缺的
  是仪表盘器官不是引擎。
- **不改 10 态状态机、不改 CAS 转移不变量、不动四列地基**。

## 4. 施工切分（过目后排期；每期独立可交付）

| 期 | 内容 | 量级 | 依赖 |
| --- | --- | --- | --- |
| 一 | 缺口①（awaiting_input 通知 + cooldown + i18n×10）+ 缺口③甲案（列内排序+视觉分层） | 半天，纯前端 | 无 |
| 二 | 缺口②（分组选择器）+ 缺口④（活动点） | 1 天，纯前端 | 无 |
| 三（可选） | 缺口⑤（燃烧率可见） | 独立立项 | token_usage 聚合 |

## 5. 待拍板清单

| # | 问题 | 领导建议 |
| --- | --- | --- |
| A | awaiting_input 通知的 per-task cooldown 时长 | 5 分钟 |
| B | 通知要不要分轻重级 | 第一期不做 |
| C | 分组选择记不记 localStorage | 记 |
| D | attention 列：甲案（列内分层）还是乙案（拆列） | 甲案 |
| E | 活动点要不要带"最后输出 Xs 前"文本 | 施工时核实数据源再定 |
| F | 燃烧率可见做不做、哪批做 | 做，第三期单独排 |
