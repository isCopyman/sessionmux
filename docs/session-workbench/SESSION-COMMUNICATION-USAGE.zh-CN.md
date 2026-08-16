# Session 交流：工具、使用方式与结果

> 状态：Active Session-message V1（2026-08-17）  
> mailbox 产品（inbox / 未读债 / 催办 / 往来条）留在 `codex/session-workbench-foundation` 快照，本分支不做。  
> 完整 mailbox 设计见 [Session 间通信 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md)

现在能用的是 Codex 式 Session 消息：`list_sessions` + `send_message`，时间线上渲染「来自某某」。

```text
send_message(target, content)
        │
        ├─ 目标在跑且能 steer → 注入当前 Turn
        ├─ 目标已连接且空闲 → 开一轮
        ├─ 目标在跑但不能 steer → 落库；本轮 TurnComplete 后再开一轮
        └─ 目标没加载 → 落库；打开/连上且空闲后再开一轮
```

没有信箱、没有往来条、没有回复债、没有催办、不排队。回信就是再 `send_message` 回去。

## 工具

### `list_sessions`

查找已经存在的其他 Session。传入可选 `query`、`limit`（默认 50）。用返回的稳定数字 `session_id` 当地址。不含自己、不含草稿。

### `send_message`

| 参数 | 作用 |
|---|---|
| `target_session_ids` | 稳定数字 ID，最多 16 个 |
| `content` | 对方需要的正文 |

成功只表示 Codeg 已落库并尝试插入。不等于对方做完。关掉的 Session 不冷启动。

建 Session / Collection / 摆工作台走 `codeg_help` / `codeg_use`。

## 人在界面上

| 人点的 | 实际是什么 |
|---|---|
| 当前输入框 Enter | 跟**这个** Session 说话 |
| `@会话` | 只补全引用。Agent 自己 `send_message` |
| 时间线卡片 | 「来自某某」+ 正文。不是 Inbox |

## 不要和这些混在一起

- **mailbox**：以后再做。本分支没有 `list_inbox` / `read_message`。
- **群 / Room / AgentBus**：不是这条路。
- **人自己的 follow-up 队列**：还在，只给当前 Session 的人用，不承载跨 Session 消息。
