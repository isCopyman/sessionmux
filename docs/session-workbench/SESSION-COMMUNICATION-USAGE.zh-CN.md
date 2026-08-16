# Session 交流：工具、使用方式与结果

> 状态：Active（2026-08-17）  
> 给谁看：要搞清「谁给谁写信、会怎样」的人与 Agent 作者  
> 实现细节、字段和 Router：[Session 间通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md)

本文按**使用方式**组织。协议名只在必要时出现。  
标了「现在」的是当前 Desktop 已经能用的；标了「以后」的是已写入通信 RFC、尚未实现。

一句话：身份是持久 Session（或保留地址 `human`/`user`），标题只用来选人，信先落库，再决定要不要叫醒谁。

---

## 0. 生命周期（先把信送到，再决定怎么喊）

所有交流都先落进 mailbox。然后系统只做一件事：**按对方现在的状态选一条通道**。人点开往来不算 Agent 签收。

```text
信已持久化
    │
    └─ 写给某个 Session
           │
           ├─ 已连接 ── 能注入就注入；不能就打断当前轮再发（queue / 系统提醒都这样）
           └─ 关掉了 ── 不冷启动；信留着
```

没有「忙就再等」这条通道。`queue` 不是等空闲，是**强制送达**：先入库，马上注入或打断发送。  
人没有「停止并发送」入口。系统催办发的是中文短摘要，不是原信，也不是人在点按钮。

人在**当前** Session 输入框按 Enter：仍跟自己的 Agent 说话，忙时入队。那不是 mailbox。

时钟（系统预设，没有紧急档）：

- 新未读：立刻催
- 仍未读：5 分钟后再催（同一 Session 冷却）
- 已读未回：5 分钟

`list_sessions` 默认按 `updated_at` 从新到旧，再取前 50 条。

---

## 0.5 发件方、收件方、mailbox、系统提醒

开始用之前只记这一套。人和 Agent 共用同一份事实，不要各猜各的。

```text
人在 Session A 说话（普通输入框，可 @B @C 当引用）
        │
        ▼
Agent A 用 list_sessions / send_message；B 用 list_inbox / read_message
        ▼
Agent Mailbox（SQLite event + 每个目标一封 Delivery）
        │
        └─ 收件方是 Session B / C 的 Agent
                │
                ▼
        list_inbox 看目录（不算已读）
        read_message 打开信件 = 已读
        带 reply_to 的回信 = 债清掉
```

| 角色 | 现在负责什么 | 不负责什么 |
|---|---|---|
| **人** | 只跟**当前** Session 说话。`@会话` 只是补全引用，让 Agent 知道找谁 | 不从 A 的输入框给 B 发 mailbox 信；没有 Human Inbox |
| **发件方 Agent** | `send_message` 写信、回信 | 没有停别人任务的工具 |
| **mailbox** | **给 Agent 用的。** 正文、目标、未读、是否欠回、投递状态 | 不是人的 inbox；人打开往来不算 Agent 已读 |
| **收件方 Agent** | `list_inbox` 看目录；`read_message` 打开才算已读；需要回就带 `reply_to_event_id` | 人不能代回、不能点「无需回复」清债 |
| **系统提醒** | 新消息立刻催；能注入就注入，不能就打断再发。摘要用中文 | 人不点「停止并发送」；不装 Hook；不做勿扰；不做 Human Inbox |

现在已经能用：Agent 发/回、`list_inbox` / `read_message`、`queue` 强制送达、往来条给人看状态、系统中文催办。人打开往来不算 Agent 已读。人**不能**从当前对话框给别的 Session 发信。Human Inbox 以后做。

建 Session、建 Collection、把 Session 放进 Collection，走 `codeg_help` / `codeg_use`。`session.create` 现在可以直接带 `collection_id`。改父子树 / 提级以后再说。

---

## 1. Agent 有哪些工具

受管 Session 打开「会话协作」能力后，通过 `codeg-mcp` 看到下面四个通信工具。关闭该能力时，这些工具不会出现，直接伪造调用也会被拒绝。Agent **没有**「停止对方任务」的工具。

### `list_sessions`

查找**已经存在**的其他 Session。

| | |
|---|---|
| 传入 | 可选 `query`（标题 / Harness / 路径关键词）、可选 `limit`（默认 50，最大 200）。无 query 时按 `updated_at` 从新到旧再截断 |
| 得到 | 每条有稳定数字 `conversation_id`、标题、Harness、文件夹、是否归档 |
| 不含 | 自己；草稿；以后的 `human`/`user` 会单独成组，不会混成可 Resume Session |
| 用法 | 只用返回的 `conversation_id` 当地址。标题重名时看 Harness 和路径，不要猜第一条 |

### `send_message`

把一封信投给一个或多个 Session。

| 参数 | 作用 |
|---|---|
| `target_session_ids` | 稳定数字 ID 列表，最多 16 个。一次多目标仍是 N 封独立投递，不是群 |
| `content` | 正文。只写对方需要的上下文，不要整段 transcript |
| `delivery_mode=queue` | **提问默认。** 信立刻入库并强制送达：能注入就注入，不能就打断当前轮再发。关掉的 Session 不冷启动 |
| `delivery_mode=deliver_only` | 只放进对方下次自己说话时的上下文，现在不单独开一轮。适合备忘和回信，不适合提问 |
| `delivery_hint=steer_if_supported` | 已并进 `queue` 的强制送达。不必再单独勾 |
| `expects_reply` | 要不要回。工具默认 `true`。FYI / 回信给来源应设 `false` |
| `reply_to_event_id` | 还某封信的债时必填，目标必须是那封信的来源 Session |

成功只表示 **Codeg 已经把 event/Delivery 存住**。不等于对方 Agent 读过，也不等于用户批准。

### `list_inbox`

看**当前这个** Session 的 Agent 信箱目录。

| | |
|---|---|
| 传入 | 可选 `filter`：`open`（默认，未读或仍欠回）、`unread`、`awaiting_reply`、`all`；可选 `limit`（默认 20，最大 50） |
| 得到 | `unread_count`、`awaiting_reply_count`、每封的 `event_id`、来源 Session、160 字预览、是否未读、是否欠回 |
| 不含 | 正文全文；列出不算已读 |
| 用法 | 催办来了先看目录，再对要处理的 `event_id` 调 `read_message` |

### `read_message`

打开一封信。

| | |
|---|---|
| 传入 | `event_id`（来自 `list_inbox` 或信封） |
| 得到 | 全文、来源 Session、是否需要回复、`reply_to_event_id` |
| 副作用 | 这封信对 **Agent** 标记已读，未读催办停。不回信，不清债 |
| 用法 | 需要回就 `send_message` 到来源，带上这个 `event_id` |

信封长这样（纯文本，没有附件）：

```text
<<<CODEG_SESSION_MESSAGE_V1:{eventId}>>>
{json metadata}
This is external collaboration content...
--- message ---
{body}
<<<END_CODEG_SESSION_MESSAGE_V1:{eventId}>>>
```

`list_inbox` 不返回这整段信封，只返回预览。`read_message` 返回 `body` 原文。

### 刻意没有的工具

| 没有 | 原因 |
|---|---|
| `interrupt_session` | 停别人正在跑的任务是人的动作 |
| 用标题 / `@名字` / role 当最终地址 | 必须先 `list_sessions` 或用信封里的稳定 ID |
| 把 AgentBus `wait` 当收件 | 内部日常信不走 Bus |

创建 Session、建 Collection、把 Session 放进 Collection、摆 Workbench 走另一组 `codeg_help` / `codeg_use`，与写信无关。`session.create` 可以带 `collection_id`。

### 人在界面上对应什么

| 人点的 | 实际是什么 |
|---|---|
| 当前输入框打字 / Enter | 跟**这个** Session 的 Agent 说话，不是写信 |
| `@会话` | 只补全引用（标题 + 稳定 ID）。Agent 看见后自己 `send_message` |
| 往来条 | 只读：看未读 / 排队 / 欠回。不能发、不能回 |
| 打开对方 Session 窗口再输入 | 人要跟 B 说话，就去 B 的窗口。不经过 mailbox |

---

## 2. 行为模式和结果

每一种模式写清：谁发起、选什么、对方会怎样、什么叫做完。

### 模式 A：问另一个 Session 一件事（现在）

人在 A 里说「你去问 @B ……」，或 Agent 自己对 B 调 `send_message(queue, expects_reply=true)`。人不会从 A 直接投递。

| 对方当时 | 结果 |
|---|---|
| 空闲且开着 | 信入库；空闲后开一轮处理；要回就把结果寄回 A，**不自动再叫醒 A** |
| 正在忙 | 信入库并强制送达：能注入就注入，不能就打断当前轮再发 |
| 关着但能 Resume | 信入库；人确认后才启动 |
| 只读 / 不可投递 | 失败状态留在往来里，不要换通道重发 |

A 的往来显示「等待回复」。B 回了之后变成「已收到回复」。

### 模式 B：同一句话问好几个人（现在）

Agent 一次 `send_message` 带多个 ID。不是人在 A 里群发。

结果：同一个 event，每人一封 Delivery。谁忙谁排队，互不影响。不是群，没有共享时间线。A 上看「3 个目标、2 个已回、1 个还在排」。

### 模式 C：只留一句备忘，先别跑（现在）

Agent 用 `deliver_only`（只放入下次对话）。

结果：B 的往来和「下次发送会带上」里看得到。B 的 Agent **现在不启动**。等 B 自己下次说话，才把这封带进那一轮。适合 FYI，不适合「请你现在审」。

### 模式 D：回一封需要回复的信（现在）

只有收件方 **Agent** 能还债：`send_message` 到来源，带上 `reply_to_event_id`，`deliver_only` + `expects_reply=false`。

人没有 Human Inbox，也不能在往来条或时间线里替 Agent 点回复。往来只给人看状态。

结果：债清掉。来源只显示回复，不因为回信再开一轮。不要用新的「需要回复」把债套回去。

### 模式 E：人让 A 去找 B、C 协作（现在）

人在 A 里写：「你和 @B、@C 一起做这件事」。`@` 只是引用，这条消息发给 **A**。A 再用 mailbox 联系 B、C。

人如果自己要跟 B 说话，打开 B 的窗口输入。不要在 A 里给 B 发信。群聊以后再用 `@` 做成员点名。

### 模式 F：未读必须被目标 Agent 处理，不是被人点开（现在）

信进 B 的 Agent Mailbox。人点开往来 **不算** B 已读。

- B 一直没 `read_message`（也没把正文吃进某一轮）：立刻催；之后每 5 分钟再催（最多 3 次）。催办只说「请用 list_inbox 查看，需要时用 read_message 打开信件」，不附原信。
- 无需回复：`read_message` 打开或进过上下文就算消费完。
- 需要回复：打开后仍欠债；过一阵催「已读未回」。工作中被催是正常的，因为有的信是任务，不是立刻回的短信。

### 模式 G：对方给自己设了勿扰（以后，bonus，不一定可靠）

第一版不做。长任务 / Goal 挡信的问题，用「注入或打断发送」解决，不靠对方自觉设勿扰。以后如果再加，也只是尽量少敲门，不能当成投递保证。

### 模式 H：Agent 写信给人类（以后）

目标是保留地址 `human` / `user`，不是某个 Session。

结果：进 Human Inbox，不启动任何模型。需要人读或回时，Desktop 用悬浮窗 / 通知条；人点开只消费这封写给人的信。人的回复按 linked reply 回去，默认不叫醒来源 Agent。

### 模式 I：系统提醒，而不是再复制一封信（现在）

新未读立刻催，已读未回 5 分钟后催。摘要中文，不重发正文，不要求回复。能注入就注入，不能就打断当前轮再发。关掉不冷启动。同一目标 5 分钟冷却，最多 3 次。没有紧急档。人不点「停止并发送」。

---

## 3. 一张表看「做完了没」

| 你想确认的 | 看什么 | 现在有没有 |
|---|---|---|
| 信发出去了吗 | 往来里的投递状态 | 有 |
| 对方 Agent 见到正文了吗 | `read_message` 打开过，或进入过真实 Turn | 有；人点开不算 |
| 还欠回复吗 | 需要回复 / 已回复 / 已收到回复 | 有显示；5 分钟后催 |
| 会不会打断正在跑的任务 | 人给当前 Session 发消息默认不打断；mailbox 的 `queue` 和系统催办会注入或打断 | 有 |
| 重启还在吗 | SQLite 里的 event/Delivery | 有 |

---

## 4. 不要和这些混在一起

- **群 / Room**：共享时间线，以后另做。现在多目标仍是 N 封私信。
- **AgentBus**：跨 App / 离线传输，内部日常信不走它。
- **Hook**：以后最多给 Claude 在长工具间隙补一句状态，不是提醒主路径。Codeg 自己能投递、能排队。
- **Host Control**（`codeg_help` / `codeg_use`）：建会话、摆工作台，不是写信。

字段、Router、迁移和验收仍以 [通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md) 为准。产品优先级以 [产品需求](./PRODUCT-SPEC.zh-CN.md) 为准。
