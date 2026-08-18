# 群聊、Mailbox 与人类角色

> 状态：设计拍板稿（2026-08-18）。第 13、14 节已落地。归档 Session 的 Room `@` 不再入队唤醒。第 15 节冻结：群历史要搜，不要把 `read_room` 当全文检索。第 16 节：第一次投递带正文，催办仍摘要。队列合并 / 连续 `@` 不打断是下一刀。  
> 问题：群聊是不是另一套消息系统？Mailbox 是不是完全私聊？人类该打字还是该发邮件？CCCC 怎么做？  
> 已有长文：[群聊 RFC](./GROUP-CONVERSATION-RFC.zh-CN.md)、[通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md)、[产品场景](./PRODUCT-SPEC.zh-CN.md#48-建立一个共享讨论室)  
> 本文只补那三份没讲清的东西：协议边界、人类三条入口、和 CCCC 的真实差别。  
> CCCC 对照仓库：`D:\code\revisiting\work\repo_audit\cccc-upstream-main`

## 0. 先回答“要不要先写文档”

要。R1 已经有表、有 `visibility=room`、有 Host Control / MCP 入口，但**人类作者、生命周期和 UI 心智还没拍板**。半截实现里最别扭的一点就是：群里发言仍必须挂一个 Session 当 `source_conversation_id`，等于把人伪装成群主 Session。

在作者模型和三条入口没写清之前，继续堆“加人按钮 / 树里像 Session”只会把错误心智做进 UI。

**本轮继续做：** Room 工作区 / 侧栏树 / Tab 仍先不动，直到作者模型和一等 item 落地。Agent 工具表面已按第 6.7 节拆开。

## 1. Codeg 其实有三条人类入口

这是和 CCCC 最大的差别。CCCC 几乎只有一条正式写路径；Codeg 产品是 Session-first，所以人有三条嘴。纯文本续写是 ①；一旦跨出“对自己的 Agent 说话”，就掉进 ② 或 ③。

```text
① Session 输入框（纯文本）
   人在某个 Session 页面对“自己的 Agent”说话
   → 原生 ACP user turn
   → 进入该 Session 的 Harness 历史
   → 立刻（或排队）跑这一轮
   → 不是 collaboration_event，不是邮箱，不是群聊

② Mailbox / Direct
   人（或 Session A）给 Session B 写信
   → collaboration_event.visibility = direct
   → 每个目标一条隔离 Delivery
   → 收件人彼此看不见
   → 进 inbox / unread / 义务机器
   → 要不要唤醒由 invocation_policy 决定
   现有/应有入口：分别发送、输入框里的结构化 @（产品已写，投递未完）、Human Inbox（信是写给人的才回）
   人不能占用某个 Session 的邮箱作曲或代回。那个邮箱是 Agent 的。

③ Room / 群聊
   人在共享面板说话
   → collaboration_event.visibility = room + room_id
   → 全体成员能查同一条公共账本
   → 只有结构化 @ 才创建 Delivery、才可能唤醒
   → 不进 mailbox inbox
   从私信分享到群：新的 Room event + 来源引用，不改旧信可见域
```

① 是**工作**。② 是**跨 Session 投递**。③ 是**共享可见的讨论**。  
把 ① 改成“人也必须先给自己发一封邮件”能换来协议一致，但会毁掉 Codeg 的主界面。

Session 输入框里的结构化 `@session` 徽章属于 ① 的**引用**（`get_session_info`），不是 ②。写信走邮箱面板和 `send_message`。群点名走 Room 目标条和 `post_room`。详见第 14 节。通信 RFC 8.3 把输入框 `@` 算成邮箱，按本节作废，避免和已落地的只读查找打架。

## 2. Mailbox 是不是完全的私聊？

是**定向隔离**，不是“对人类保密”。

| 问题 | Mailbox / Direct | Room |
|---|---|---|
| 谁能查询正文 | 发送方、每个目标自己、本机所有者审计 | 该 Room 全体成员 |
| 多收件人彼此 | 看不见对方，也看不见对方回复 | 看得见同一条公共消息和公开回复 |
| 回复默认去哪 | 只回原发送方（私信 Thread） | 回同一 Room / 同一回复串 |
| 进 Session 邮箱？ | 是 | 否 |
| 没 @ 会不会唤醒 | 由调用策略决定；V1 写信通常会投递 | 默认不唤醒，只记账 |
| 算不算群聊 | 不算。多目标 = 分别私发 | 算。一条公共事实 |

所以：Mailbox **不是**“人类和 Agent 的普通聊天”，也 **不是** 隐藏群聊。它是 Session↔Session（将来也含 Session↔human）的信件。

产品场景里“分别发送三个模型、互不可见”必须走 ②，不能走 ③ 的 `@all`。

## 3. 群聊需要哪些协议，Mailbox 能覆盖哪些

群聊至少要同时回答四件事。Mailbox 只完整覆盖其中两件。

```text
可见性   这条内容属于谁以后能查到？
投递     这次要不要创建 Delivery？
唤醒     这次要不要花一次 Agent turn？
作者     这句话是谁说的（人 / 某个 Session）？
```

| 协议能力 | Mailbox 已有 | 群聊还要 |
|---|---|---|
| 不可变正文 + `event_id` | 有 | 复用 |
| 多目标 Delivery、排队、steer、链深保险丝 | 有 | 复用；@ 才创建 Delivery |
| `reply_to_event_id` 串成 Thread | 有，但是私信串 | 复用，但可见域继承 Room |
| 调用策略 / 是否欠回复 | 有 | 复用 |
| 公共成员集合 | **没有** | Room + member 表 |
| 公共时间线查询 | **没有**（inbox 是按目标切开的） | `visibility=room` + `room_id` |
| “只记录、不叫醒” | store_only 接近，但仍是私信投递语义 | 零目标 Room post |
| 人类作为作者，而不是某个 Session | **没有**（H1 未做） | **必须有**，否则群里的“你”是假的 |
| 加/移成员、退群、从现在开始 | **没有** | Room 生命周期 |
| 新成员不自动灌全部历史 | 无此对象 | 游标 + 摘要（R2） |

结论：群聊 **fork 的是可见性与成员，不是消息引擎**。  
不要第二套 PromptQueue，不要第二套 Dispatcher，不要把 Room 伪装成 `conversation` 去骗 ACP。

Mailbox **做不到** 的，恰恰是用户感知里的“群”：

- 一条话被所有成员当公共记录；
- 没被点名的成员以后仍能读到，但不为此花钱；
- 加人、退群、改名，不复制每个成员的原生历史。

## 4. 人类在两种机制里干什么

### 4.1 在 Session 输入框（①）

人是**这个 Session 的使用者**。

- 行为：继续这段 Agent 的工作上下文。
- 代码：走 ACP `session/prompt` / PromptQueue，**不**写 `collaboration_event`。
- 谁看见：只有打开这个 Session 的人，以及这个 Agent 自己的原生历史。
- 其他 Session：不知道。

这不是邮件。也不应该改成邮件。通信 RFC 已经写过：人在当前 Session 输入框说话，是续写，不是 mailbox 提醒。

### 4.2 在 Mailbox（②）

人有两种尚未完全落地的身份：

| 身份 | 现在 | 应该 |
|---|---|---|
| 旁观/审计 | 人可以打开某 Session 的收件箱看信 | 继续看。人看过 ≠ Agent 已读。**不能在这个邮箱里代回、代发** |
| 收件人 `human` | RFC 有，运行时没有 | Agent 要人拍板、报完成、报阻塞时才写信。普通最终回答不准抄进人类收件箱。人只在 Human Inbox 回这类信 |
| 发件人 `human` | 半截 UI 会让人站在 B 的邮箱给 A 写信 | **禁止。** B 的邮箱是 B 的。人要指点 A，打开 A 的对话框打字（①）。人要跨 Session 委托，用分别发送或输入框里的结构化 @，不要“借用”某个 Agent 的收件箱 |

人用协作信给 Session 发信，和人在该 Session 输入框打字，**完全不一样**：

```text
输入框：这句话进入原生历史，通常马上变成这一轮的用户消息
邮件：  这句话是一封协作信。目标可能在忙、在睡、或只先收信不跑。
        它出现在 inbox / 待摄入条，默认不应假装已经进了聊天记录。
```

感觉“怪”是对的：同一个人、同一个 Agent，两种嘴。但怪在**场景不同**，不在协议没对齐。

- 要它现在接着干：用输入框。
- 要它在空闲时处理一份跨 Session 的委托，或留下可追踪回执：用邮件。

第一版不要强迫“所有人类输入都先变成邮件”。那是 CCCC 的产品形状，不是 Codeg 的。

### 4.3 在 Room（③）

人是**主持者 / 旁观者**，不是又一个 Session 成员。

- 成员表只引用已有 Session（角色）。
- 人默认出现在成员栏，但不是 `collaboration_room_member` 里的假 conversation。
- 人在群里说话：写 Room event，作者是 `human`。
- 人 `@D`：同一条公共 event + 对 D 的 Delivery。D 被唤醒时，信封说明“这是群聊点名，不是私信”。
- 人点成员头像：离开群，进入该 Session 的 ①。那里打的字不回流群，除非显式“分享到群聊”。
- Agent 需要人拍板：仍走 ② 的 Human Inbox，不要把所有群消息复制成人类邮件。

当前 R1 实现违反了这一节：`post_room` 要求 `source_conversation_id` 必须是成员。人被逼成“以 Session C 发言”。这是文档和代码不一致的根因，也是 UI 看起来不像群聊的根因。

## 5. CCCC 实际怎么做

本地源码结论（不是口号）：

```text
所有正式发言（人 / Agent / IM / MCP / CLI）
  → 同一条 chat.message
  → append 到该 Working Group 的 ledger.jsonl
  → Inbox 只是按 to + 已读游标切出来的未读队列
```

没有独立 mailbox 库。没有 Codeg 那种 Session composer 协议。

| 表面动作 | CCCC 真实路径 |
|---|---|
| 人在群里说话 | `chat.message`，`by=user`，空 `to` 默认物化成 `@foreman`，不是全员广播 |
| 人点名某个 Actor | 同一 `chat.message`，`to=[actor]`。群 UI 仍看得见；其他 Actor 的 inbox 进不去 |
| “给 Actor 发邮件” | **没有这条写协议**。Inbox 是读面 |
| Agent 终端里键入 | 直写 PTY，**不进 ledger**，也不是邮箱 |

所以 CCCC **不是**“用邮箱做成了群聊”。它是：

> 先有 Group 共享账本，再用 Inbox 做个体消费。

Codeg 可以学这层分层，但不能学“取消 Session 输入框”。Codeg 的 Session 已经是一等 ACP 资产；Room 是叠在上面的共享面板。CCCC 的 Group 才是工作区本身。

Foreman 在 CCCC 里是**第一个 Actor**，不是人类。人类 principal 固定是 `user`。这一点 Codeg 应对齐：人不是 Session C。

## 6. 拍板（实现前必须守住）

1. **不新造消息引擎。** Room = 成员 + `visibility=room` + 公共查询。投递、排队、唤醒继续用现有 Delivery / Dispatcher。
2. **Mailbox 保持私信。** 多目标邮件永远互相隔离。需要公共记录就建 Room，不要把 inbox 渲染成群。
3. **人类有三条嘴，不合并成一条。** 输入框、私信、群发言语义不同，UI 也必须不同。
4. **群里的作者必须能是 `human`。** 在此之前，不要把“你主持”做成会骗人的气泡。过渡期宁可显示“未完成：人类作者”，也不要再把人写成 Session C。
5. **Room 是单独的一等 item，不是 Session 的附件。** 身份是这段共享讨论，不是成员集合。同一批 Session 可以再建另一个 Room。它可以像 Session 一样出现在 Collection、Workbench、搜索和最近使用里，用群图标，占一个 `kind: "room"` 的内容 Tab。禁止写入 `conversation` / ACP session，禁止永远挂在某个创建者 Session 下面。
6. **加人是 Room 生命周期，不是邮件抄送。** 后端 `add_members` / `remove_member` 已有；UI 等作者模型拍板后再做。新成员默认从现在开始。
7. **Mailbox 工具和 Room 工具拆开。** `send_message` / `list_inbox` / `read_message` 只做私信。Room 读写走 `list_rooms` / `read_room` / `post_room`。Host Control `room.*` 只管创建、列表、加人；不再用 `room.post` 或 `send_message + room_id` 发群帖。内部仍共用 Delivery / Dispatcher。
8. **同一 Thread 可以连发补充。** 回信和“忘了一句”都必须带 `reply_to_event_id`。省略父节点 = 新开一个根，往来续不上。第一封 linked reply 清债；后面的补充还挂在这条链上。

## 7. 生命周期（人能看见的）

```text
创建
  多选 ≥2 个 Session → 建 Room
  人成为主持（不是成员行里的假 Session）
  被选 Session 成为 member
  默认 Collection：未分类，或建议“成员都在同一 Collection 时用那个位置”；人可以改到任意 Collection 或保持未分类

打开
  树/搜索/最近 → Room Tab
  关 Tab ≠ 退群 ≠ 停成员
  切 Workbench 可以丢掉未持久化的 Room Tab，树行仍在

发言
  人：Room event，作者 human，默认可不 @
  @ 某个成员：同一 event + Delivery + 可选唤醒
  成员 Agent 回复：默认回 Room，作者是该 Session
  成员 Agent 要私聊另一个 Session：必须显式 direct，不回写 Room

加人 / 移出
  加人：引用已有 Session，不复制历史
  移出：不能低于 2 个 Session 成员；最后群主不能被移走
  被移出的 Session 不再能查后续公共事件（已发出的旧 event 仍属于当时可见域，第一版不改写历史）

结束
  删除 Room：删成员关系和公共时间线
  不删任何 Session，不删任何原生聊天
```

## 8. UI 原则（先画心智，后写组件）

人打开 Room 时，应一眼看出这不是又一个 Agent 会话：

```text
左树：普通 Session 用 Harness 图标；Room 用群图标或“群聊”徽标
中间：共享时间线。人的气泡是“你”，Session 的气泡是角色名
右侧：你（主持）+ Session 角色列表 + 加入/移出
底部：你在对群说话。目标条：[只记录] [@A] [@B] [@all]
```

不要出现“以谁的身份发送”下拉。那是把人当成可以扮演的 Session。

从 Room 点进某个 Session 后，输入框恢复 ①：你在对**这个** Agent 工作，不是在群里发言。

Mailbox 继续留在 Session 面板里，当“跨 Session 的收件箱”，不要做成第二个群。

## 9. 和现有代码的差距（只记账，本轮不改）

| 项 | 现在 | 本文要求 |
|---|---|---|
| Room event 作者 | 必须是成员 Session | 允许 `human` |
| 群 UI 发言 | “Post as Session …” | 你主持 |
| Human Inbox | 未做 | 独立于 Room，Agent→人 |
| 树/Tab 里像 Session | 半截、未审完 | 可以做，但是内容面板，不是 conversation |
| 加人 UI | 后端有，界面无 | 作者模型之后再做 |
| Agent 互 @ | R2 | 先不要扩大 |

## 10. 四个问题的建议拍板

审查后不再空着。你若不同意，改这里，不要在代码里另选。

1. **群里不 @：只记录。** Codeg 没有 CCCC 那种必须有人接活的 Foreman。默认点名“群主 Session”等于叫醒多选名单里的第一个模型。产品 4.8 已经是“仅发到群聊”。
2. **人不用 Agent 的邮箱作曲。** 已拍板。B 的 inbox 只给人看，不给人代回。要指点 A：打开 A 的对话框。信是写给 `human` 的：只在 Human Inbox 回，作者是人。Agent 自己的 `send_message` 仍记该 Session。
3. **Room 自己进 Collection / Workbench。** 已拍板。它是独立 item，不是创建者 Session 的子行。同一批成员可以有多个 Room。现表绑 `workbench_id` 且 CASCADE、没有 Collection 列，这是要改的，不是要迁就的。
4. **旧消息显示「你（当时记在 C）」。** 不改不可变 `source_conversation_id`。

## 11. 审查后必须补进实现前的缺口

2026-08-18 子代理对照旧 RFC 和现码后的高优先级缺口：

- 人类作者和 H1 Human Inbox 是**同一刀地址模型**（`author_kind` + 可空 session 引用）。不能先在 Room 里用 `0` 冒充人，再另做一套 human 表。
- 现码没有删除 Room API；删 Workbench 却会 CASCADE 掉 Room，而 `collaboration_event.room_id` 无外键，账本会变孤儿。
- “最后群主不能移走”在“人不是成员”之后应改成：活着的 Session 成员不能少于 2。
- Session 归档仍可留在群里，但不能被 `@` 唤醒（`post_room` 已落地：仍建 Delivery、时间线仍显示点名，不入队、不打断、不挂回复义务）。Session 删除后成员必须标死或移出。Mailbox 写信给归档 Session 的语义本轮不动。
- UI 还缺：Delivery 状态、只记录 vs @、回复某条时继承目标、Session 页“来自 Room”回跳、Room 时间线搜索（见第 15 节）。现 UI 已用 created_by 冒充“你”，作者模型落地前不要再加深这条假路径。
- 通信 RFC 文首仍写“Room 仍为拟议”，和 R1 存储已落地不一致，改代码前先改那一行。

## 12. `@` 语法（对照 Buzz / CCCC / Multica 后拍板）

群 RFC 5 节已经写过：“点名”和“正文提及”必须区分。对照仓库只是再次确认，不另选一条路。

| 来源 | 实际做法 | Codeg 学什么 | Codeg 不学什么 |
|---|---|---|---|
| CCCC | `@` 是自动完成/高亮；真正叫醒谁由 **recipient chips / `to`** 决定。`@user` 是人类 principal，不是 Actor。空 `to` 默认 `@foreman`。 | chip 路由；`@human`/`@user` 是人，不是 Session | 空目标默默叫醒群主 / Foreman |
| Buzz | 投递用结构化 p-tag，不是扫正文 `@word`。`buzz-acp` 只听 mention。附件是另一条协议。 | 没结构化点名就不叫醒；文件不是 mention | 把文件链接当成 Delivery |
| Multica | `@Agent` = 启动一次执行；`@all` 只通知人类、不含 Agent。共享记录 / 通知人 / 启动 Agent 三套语义。 | 三种 `@` 必须拆开 | 把 `@all` 做成“也通知人、也叫醒全部 Agent”的混合物 |
| Codeg 输入框 | 已有结构化徽章：`file` / `agent` / `session` / `commit` / `skill`，URI 如 `codeg://session/12` | Room 复用同一套徽章，不新发明 `@alice` 扫描 | 后端再扫一遍自由文本 |

因此 `post_room` **不解析语句里的 `@` 字符**。邮件地址、Rust 属性、`@/src/foo.rs`、代码块里的 `@param` 都不是点名。

三种 `@`，三种结果：

```text
1. Session 点名
   入口：post_room.mention_session_ids / mention_all
         或人类编辑器选中的 session 徽章 / codeg://session/<id>
   结果：同一条 Room event + 对该 Session 的 Delivery（可能唤醒）
   不进 mailbox inbox

2. 人类点名（@human / @user，同一地址）
   入口：post_room.mention_human 或 codeg://human / codeg://user
   结果：同一条公共 Room event 打上 mention_human
         不叫醒任何 Session
         将来进 Human Inbox（本轮只落标记，不造第二套人表）

3. 文件 / commit / skill 引用
   入口：composer 的 file/commit/skill 徽章（file:// 或已有 reference URI）
   结果：只是正文里的上下文，零 Delivery
```

`@all` 只叫醒**可投递的 Session 成员**，不含 `@human`。要拍人的肩膀，必须另选 `@human`。

Agent 工具继续走结构化字段。人类群输入框走目标条 + 徽章（群 RFC 6 节）。正文里写下 `@Claude` 但没选中实体，不静默路由。

## 13. 本轮执行顺序（功能为主，重构只收拾歧义）

不一边大重构一边堆功能。顺序：

1. **文档冻结**（本节）：`@` 三义、不扫自由文本、Room 自己进 Collection。
2. **侧栏右键**：Collection 行整行进 ContextMenu；树容器 `preventDefault`；dnd-kit 忽略右键。原生浏览器菜单不再冒出来。
3. **歧义 API 改名（行为不变）**：
   - Rust `collaboration_room_service::list` → `list_for_workbench`
   - Host Control 增加 `room.list_workbench`，`room.list` 仍是别名
   - 前端 `listWorkbenchRooms`；`listCollaborationRooms` 仍是别名
   - MCP `list_rooms` **不改名**（成员范围，skill 已教过）
4. **Room 一等 Collection item**：表加 `collection_id` / `root_folder_id`；树按这两列摆，不再挂创建者 Session；补测试。
5. **人类作者 + `@` 合同**：`author_kind` + `mention_human`；UI 不再冒充群主发言；正文 `@word` 不投递；结构化 URI 才并进目标。

Human Inbox 本体、删 Room API、Agent 互 `@` 仍按第 11 节留到后面，本轮不夹带。

## 14. `codeg://session/<id>` 会不会和邮件抢语义？

不会。号码可以共用，动词必须分开。不要给 URI 加 `?kind=mail` 这类字段。

Slack 同一个 `@user`：频道里是点名，DM 里是私信。CCCC 正文 `@` 只是高亮，真正路由看 `to` chips。Buzz 投递用 p-tag，文件是另一条协议。Multica 把叫醒 Agent、通知人、文件引用拆成三套。Codeg 学这个：地址一个，入口三个。

| 入口 | 动词 | 用什么 | 不要做成 |
|---|---|---|---|
| Session 输入框徽章 | 引用这段历史 | `get_session_info`（已落地） | 发送后变成私信 |
| 邮箱 / 分别发送 | 定向隔离私信 | `send_message`；人用邮箱面板 | 用 `post_room` 或输入框 `@` 偷发 |
| Room 目标条 / 群徽章 | 公共账本上的点名 | `post_room.mention_session_ids` | 进 mailbox inbox |

Agent 看见同一 URI 时靠**已经落地的信封**分流：`kind=room_mention` → 正文已在信封里，用 `read_room` / `post_room` 看前后文并回群；`kind=letter` → 私信第一次投递，正文已在信封里，用 `read_message` 标已读。`kind=system_notify` 只出现在旧 transcript 里（当时第一次投递是“请去读”）。现在的超时催办是独立中文摘要，不再走这套信封，也不重发正文。

真缺口是人类 Session 作曲器：若把同一徽章再接到 `send_message`，就会和 `get_session_info` 抢。所以 **不要接线**。人要写信，用邮箱；人要在群里点名，用 Room。

归档 Session 的 Room `@`：公共账本仍记下这次点名，但 `invocation_policy` 落成 `store_only`，不进 `prompt_queue`，高优先级 dispatch 看到没有 queue item 也不会去打断。Mailbox 的 `send_message` 不跟这刀。

## 15. 群历史要不要搜？

要。微信 / Discord / CCCC 都有。新成员补课、老成员找“上次怎么定的”，都不能靠把整本账本塞进一次 `read_room`。

`read_room` 现在不是搜索，连“看最近”都没做好：默认 `LIMIT` 50（服务端最多 200），`ORDER BY created_at ASC`，等于房间变长之后只能看到**最早**的一段。工具文案写 recent，实现是 oldest。人的 Room 面板也是一次性拉 timeline，没有搜索框。

现有三套“搜索”不是同一件事，不要合成一个 `search()`：

| 入口 | 实际在搜什么 | 和群历史 |
|---|---|---|
| Cmd+K / `search_session_content` | ctx 扫 Harness 会话文件 | 不是 `collaboration_event` |
| Agent `list_inbox` | 未读 / 待回等**状态**过滤；列表故意没有正文 | 私信，不是群 |
| 邮箱对话框的输入框 | 对**已经加载进内存的信**做 `includes` | 不是服务端全文，信一多就漏 |

V1 形状（学 CCCC `search_messages` 和 Discord 的“搜到再跳回上下文”）：

1. 先让 `read_room` 变成**最近 N 条** + `before_event_id` 往回翻。这比搜索更基础。
2. 另做 `search_room(room_id, query, limit)`：必须已是成员；只查 `visibility=room`；返回 snippet + `event_id` + 作者。命中后再 `read_room` 看前后文。不要把 query 塞进 `read_room` 让一个工具身兼打开窗口和 grep。
3. 人的 Room 面板用同一条 SQL。Cmd+K 以后可以加“群”页，现在不必。
4. 房间还小，V1 用大小写不敏感的 `LIKE` 即可，先不上 FTS5。
5. 可抽前端搜索框 / 高亮命中。**禁止**把 ctx、邮箱过滤、群账本并成一个后端函数。

下一刀功能（第 11 节）仍是删 Room API；删 Workbench 不再 CASCADE 出孤儿 `collaboration_event`。`read_room` 改尾窗 + `search_room` 作为独立小刀插在删 Room 之后、Room 回跳之前。文件搬家只插空做，不合并写路径。

调度器下一刀：同 Session 排队的通知 claim 时合并；连续 `@` 禁止一条一个 interrupt（能 steer 就 steer，否则等本轮结束再一批投）。人的输入框和“停掉再发”仍单独优先。

## 16. 第一次投递带正文，催办才用系统摘要

Mailbox 和 Room `@` 共用一个 Dispatcher，但第一次进 Turn 的信封不是“请去读”的说明书。

| 时机 | 进 prompt 的是什么 | 已读？ |
|---|---|---|
| 第一次投递 | 标题 + 正文（超长截断；余下 `read_message` / `read_room`） | 否。投递成功 ≠ 消费 |
| Agent 调 `read_message` | 全文 | 是。已见过正文再调一次 = 点已读 |
| 过了 5 分钟仍未 consume | 短摘要 + `event_id`，不重发全文 | 仍未读 |
| 已读但 `expects_reply` 且 5 分钟没回 | 短摘要 | 已读未回 |

群帖没有 title。`kind=room_mention` 的正文就是那条 `content`。没 `@` 的群发言不进调度器。

不要学 CCCC 的 `auto_mark_on_delivery`：塞进 prompt 只证明 Host 投了。

