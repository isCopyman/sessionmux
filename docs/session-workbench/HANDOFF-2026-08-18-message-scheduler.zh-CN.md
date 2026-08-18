# 交接（已完结）：统一消息调度（2026-08-18）

> **状态：本交接单上的全部事项已完成，本文档仅作存档。**
> 当日下午至晚间的并行会话把剩余工作全部落地；2026-08-18 晚全量认证：
> 后端 `cargo test --no-default-features --lib` **2414 过 / 0 挂**，
> 前端 `pnpm test` **4302 过 / 0 挂**（含此前 3 个预存失败的修复）。

## 各项落点（找规范去这些地方，别再读旧交接指令）

| 事项 | 状态 | 落点 |
|---|---|---|
| 调度后端（类别 claim / timer 退避 / 催办配额修复） | ✅ | a6c7cfaa，规范见 SESSION-TRIGGERS-GOALS-AUTOMATION-RFC（已对账入库 e46ce62a） |
| 前端 timer 退避状态 UI + 队列类别徽章 + i18n | ✅ | 4bdf4894 及后续（session-timers.tsx 已渲染 autoPaused/恢复） |
| 首次投递带正文（上限 8000 字符，read_message 仍为消费/已读） | ✅ | d1c72444；未读催办改为 5 分钟后仅标题 |
| 多信合并投送（空闲一轮 flush 多封；忙碌一次 steer 批量） | ✅ | claim `OriginFlushKind`（ConsecutiveQueuedOrigins / RemainingSteerable），测试 `idle_flush_sends_consecutive_letters_in_one_prompt`、`busy_steer_flushes_once_then_holds_later_letters_for_idle` |
| high 信打断风暴节流 | ✅（结构性） | `collaboration_interrupt_service::prepare` 的队列 pause 互斥：同会话同时只有一个打断操作，后续 high 信降级为入队唤醒，终局后随批量一轮送达 |
| Room 与 Mailbox 拆账拆工具 | ✅ | 85656c48 / c8f5abb5 / d0a5e8ad；设计见 ROOM-VS-MAILBOX-DESIGN.zh-CN.md 与 GROUP-CONVERSATION-RFC.zh-CN.md |
| read_room 补课窗口 | ✅ | `unread` + `before_event_id` + `limit`；search_room 冻结（0b20c2a1） |
| RFC 对账入库 | ✅ | e46ce62a（含 human-inbox 存储拆分标记） |

仍开放（有意为之，非遗漏）：post_room 的可选 `title` 是否移除（讨论倾向"群帖无主题"，
但 Room 前端正在迭代中）；人类信箱 human_notice v1（RFC 已标记存储拆分，未实现）。

## 给维护者留的五个易错点（长期有效）

1. **claim/快照/准入/reorder 四处排序必须同一**（CLASS_ORDER_SQL 两个常量），改一处漏三处
   会出现"准入门检查的头 ≠ worker 实际 claim 的头"。
2. **origin 项状态台阶**：claim → mark_dispatch_started（delivery 进 embedding）→
   accept/release。跳台阶会报 "no longer being embedded"。批量 flush 对批内每一项都要走完台阶。
3. **时间比较全走 Rust 侧 + `datetime()` 归一 + 秒级截断**；DB 里 CURRENT_TIMESTAMP 与
   chrono 绑定值格式不同，SQL 里裸字符串比较会翻车。
4. **timer 的 auto_pause 永不动 enabled**；用户 update 无条件清 strike/auto_pause——前端
   "恢复"按钮靠这个，别在后端加条件。
5. **strike 只在"无新信息且有未回义务"时累积**；纯本地干活的 goal 循环保持原行为
   （宿主判不准"本地零进展"，误刹更伤；转写增量启发式记为 v2）。

另：host_control 能力目录测试已改为**逐 id 断言**（新增动作时失败信息会直接点名，
不再是 "7 != 6"）；新增 Host Control 动作后请同步更新该列表。
