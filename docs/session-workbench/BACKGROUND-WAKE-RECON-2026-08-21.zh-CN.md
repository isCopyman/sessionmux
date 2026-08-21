# 后台任务唤醒：codeg 不是聋的，是哑的（2026-08-21 实证）

调研缘起：用户提出「后台任务跑完 → 叫醒 agent 续跑」是产品缺口，并对照了
grok-app / Paseo / Monet 三家。原假设是 **codeg 收不到官方 wake 事件**。

**实证结论：假设是错的。内容收得到，而且已经进转录。缺的是它周围的一切。**

本篇每条结论都附 `文件:行号`。派给 grok 的两轮都只回过程叙述没交报告，
下面是领导会话自己查的。

## 1. 链条三环，环环通

### ① Claude CLI：后台任务结束会开一个自主回合

不用我们做任何事。CLI 自己在后台任务 settle 后注入 followup。

### ② 适配器 0.69.0 认识它，而且**照发内容**

`@agentclientprotocol/claude-agent-acp@0.69.0`（`bin` = `dist/index.js`，
真正的逻辑在 `dist/acp-agent.js`）：

```js
// acp-agent.js:114
const AUTONOMOUS_RESULT_ORIGINS = new Set([
    "task-notification", "peer", "coordinator", "observer", "observer-activity",
]);
```

`acp-agent.js:2507` 的注释写得很直白：

> A result from an autonomous cycle — a task-notification followup … — is not
> the user's prompt's. Autonomous results must never touch the user-turn
> lifecycle (stop reason, settles, failActive, slash-command output
> forwarding), though their cost is real.

**注意它排除的是「用户回合生命周期」，不是「内容转发」。** 唯一的发送口
`sendUpdate`（`acp-agent.js:1249-1265`）只在 file-change-audit 阶段抑制，
**没有任何「没有活跃回合就不发」的守卫**。

附带发现：适配器还维护 `session.liveBackgroundTasks`，并**为异步子代理推迟回合结算**
（`acp-agent.js:2294-2309`）。但注释明说后台 Bash「对归属而言是惰性的」
（`acp-agent.js:2291`）——子代理会 hold 住回合，后台 Bash 不会。

### ③ codeg 的 idle 循环一直在读，而且走同一个渲染函数

`connection.rs:7857-7870`：

```rust
update = session.read_update() => {
    match update {
        Ok(SessionMessage::SessionMessage(dispatch)) => {
            ...
            .if_notification(async |notif: SessionNotification| {
                emit_conversation_update(&st, &h, agent_type, notif.update, ...).await;
```

`emit_conversation_update` 和回合内路径用的是同一个函数。

## 2. 那到底缺什么

缺的全在「回合语义」这一层，不在「收不收得到」：

| 缺口 | 证据 |
| --- | --- |
| **系统通知不响** | 通知只在 `TurnComplete` 分支里发（`acp-connections-context.tsx:3952-3965`）。自主回合不产生 `TurnComplete` |
| 会话状态不翻成「在跑」 | 同上，状态机挂在回合生命周期上 |
| 不进任何调度器 | PromptQueue / mailbox / Room 都没有入口 |
| 没有后台任务列表 | 只能在转录的 tool 卡片里猜 |

所以用户看到的症状是：**agent 在后台干完活、还接着说了话，但界面一声不吭。**

## 3. 最小补丁的形状

**不要重写 adapter。** 三家参照实现（grok-app / Paseo / Monet）没有一家写了协议
适配器，写的都是宿主胶水。而且 ACP 正是 codeg「一次集成接多个 agent」的立身之本，
换成每家手写适配器等于把维护成本乘以 agent 数量。

改动点就在 `connection.rs:7857` 那个 idle 分支——**它按定义就是「没有活跃回合」，
所以落到这里的 agent 活动天然就是自主回合**：

1. 在该分支里识别「这是 agent 活动」而不是协议噪音：只认
   `agent_message_chunk` / `agent_thought_chunk` / `tool_call*`，
   不认 `available_commands_update` 这类。
2. 认出来后发一个新事件（例如 `AcpEvent::AutonomousActivity { .. }`），
   让前端把会话翻成「在跑」。
3. 活动静默后补一条终结事件，走**和 `TurnComplete` 同一个**通知 + 未读徽章路径。
4. 后台任务列表是第二期，先把「有动静看得见」做出来。

**对 Grok 线适不适用**：不适用同一套。Grok 走的是 ACP 的
`sessionUpdate: task_backgrounded | task_completed`，是另一条协议事件，要单独接。
本轮没查 codeg 对这两个事件的处理（**未查**）。

## 4. 顺带澄清

`/cli-delegate` 这类「宿主后台 shell + 跑完唤醒」的 skill，在 codeg 会话里
**内容其实回得来**（走的就是上面那条链），但**没有任何提示**——所以体感是「跑丢了」。
修好第 3 节的显示层，这类工作流就能用。

## 5. 本轮没做的

- Grok 的 `task_backgrounded` / `task_completed` 在 codeg 里怎么处理 —— **未查**
- `terminal_runtime.rs` 的 `wait_for_exit` 是不是只能在回合内等 —— **未查**
- GitHub issues 是否有人提过 —— **未查**（grok 报称 origin 是 `xintaofei/codeg`、
  issue 679 在该仓不存在，**此条我没有独立复核**）
