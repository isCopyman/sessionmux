# 规格 F 产出：`is_reserved_turn_id` 注释与缺口评估

> 行号以本分支 `wt/fork-reserved-id-audit`（基 `f45cd0e1`）为准。
> 路径相对 `src-tauri/src/`，除非另标。
>
> **口径是 turn 级 `MessageTurn.id`，不是消息级 `UnifiedMessage.id`。**
> 包 A 主表第三列有部分素材，本文件全部按当前分支源码重核，不抄消息级列。
>
> 判定逻辑**未改**（规格：未经协调者拍板不得改）。只修正了
> `acp/manager.rs` 里已经确认为错的注释，并在单测里钉下当前匹配范围。

---

## 三选一结论

**真缺口但低危。**

- 注释宣称 `turn-<digits>` 是 every parser 的 `format!("turn-{}", n)`：**错。**
- 谓词本身只挡 `turn-<digits>`。14 家里 **11 家**落在这个命名空间，**3 家落在外面**
  （Cline / Cursor / Grok）。按该函数自己声明的安全目的，那 3 家是覆盖缺口。
- 诚实 UI **触发不了**：`optimistic-${randomUUID()}`。要撞上，必须有人把
  `client_message_id` 设成一条**已存在的 user turn id**。
- 撞上之后的最坏可见效果是：其它窗口在 in-flight 期间把新 prompt 的广播回声
  按精确 id 压掉；agent 侧照样发送；`apply_in_flight_message_id` 的 collide
  守卫拒绝盖章，persist 之后新 turn 带着自己的 parser id 出现。不是静默吞 prompt，
  也不是跨会话伪造。
- **建议修法（本包不动逻辑）**：把谓词扩成「任一 parser 的 turn 级命名空间」，
  或改成 allowlist（只放行 `optimistic-` / `user-` / `host-control:` 前缀）。
  扩 matcher 是安全用途变更，交给协调者拍板。

---

## 1. 14 家 turn 级 id 形状

`ALL_PARSER_AGENTS` 顺序（`db/service/import_service.rs:27-42`）。

| # | parser | `MessageTurn.id` 形状 | 构造点 `file:line` | 被 `is_reserved_turn_id` 挡住？ |
|---|---|---|---|---|
| 1 | ClaudeCode | `turn-{n}`（`n = turns.len()`，0-based） | `parsers/claude.rs:2551`（assistant，先算 id 再 `2573` push）、`:2585`（system）、`:2597`（user）。`group_into_turns` | 是 |
| 2 | Codex | `turn-{n}` | `parsers/codex.rs:4185`（user）、`:4197`（system）、`:4237`（assistant） | 是 |
| 3 | OpenCode | `turn-{n}` | `parsers/opencode.rs:1135`（user）、`:1147`（system）、`:1186`（assistant） | 是 |
| 4 | Gemini | `turn-{n}` | `parsers/gemini.rs:803`（user）、`:818`（system）、`:861`（assistant） | 是 |
| 5 | OpenClaw | `turn-{n}` | `parsers/openclaw.rs:1110`（user）、`:1122`（system）、`:1156`（assistant） | 是 |
| 6 | Cline | `{conversation_id}-{turn_counter}`，`turn_counter` **1-based**，本轮发出的 turn 序号 | `parsers/cline.rs:285`（assistant）、`:305`（system / tool-result）、`:320`（user）。`conversation_id` = task 目录名（`:207`） | **否** |
| 7 | Hermes | `turn-{n}` | `parsers/hermes.rs:719`（user）、`:731`（system）、`:768`（assistant） | 是 |
| 8 | CodeBuddy | `turn-{n}` | `parsers/codebuddy.rs:944`（user）、`:956`（system）、`:994`（assistant） | 是 |
| 9 | KimiCode | `turn-{n}` | `parsers/kimi_code.rs:929`（user）、`:941`（system）、`:979`（assistant） | 是 |
| 10 | Pi | `turn-{n}` | `parsers/pi.rs:695`（user）、`:707`（system）、`:745`（assistant） | 是 |
| 11 | Grok | 先 `String::new()`，最后 `grok-turn-{i}` | `parsers/grok.rs:769`（user 占位）、`:1643`（assistant 占位）、`:1013-1015`（enumerate 赋值） | **否** |
| 12 | Cursor | 先 `String::new()`，最后 `cursor-turn-{i}` | `parsers/cursor.rs:985/1001/1033/1050`（占位）、`:1079-1080`（enumerate 赋值） | **否** |
| 13 | DeepSeek | `turn-{n}` | `parsers/deepseek.rs:524`（user）、`:722`（assistant 开张） | 是 |
| 14 | Qoder | 调用 Claude 的 `group_into_turns` → `turn-{n}` | import `parsers/qoder.rs:13-16`，调用 `:828` → `claude.rs:2551/2585/2597` | 是 |

谓词实现（`acp/manager.rs:66-69`）：

```
strip_prefix("turn-") 之后 rest 非空且全是 ASCII digit
```

因此 `cursor-turn-0`、`grok-turn-0`、`{taskId}-1`、`turn-1a`、`turnabout-1` 都不匹配。
单测 `acp/manager.rs:3617-3629` 钉的也是这个范围。

### 脚注（不在 14 家里，但同属 turn 级 id）

- **Custom / ACP native**：`acp-{seq}`（`parsers/acp_native.rs:381, 438, 537`）。
  `import_service.rs:60-64` 给 Custom 走这条，不在 `ALL_PARSER_AGENTS`。同样不被挡住。
- **测试夹具** `parsers/mod.rs:1244` 的 `t-{start_s}` 不是生产 parser。

---

## 2. 调用点与 `message_id` 从哪来

`is_reserved_turn_id` **生产路径只有一处**：

`acp/manager.rs:1433-1437`，在 `send_prompt_linked_with_message_id` 里：

```
viewer_message_id = match client_message_id {
    Some(id) if !is_reserved_turn_id(id) => id,          // 原样采用
    _ => format!("user-{}-{}", conn_id, event_seq),      // 拒绝 / 缺省
}
```

然后：

1. `viewer_message_id` 成为 `ConnectionCommand::Prompt` 的
   `user_message: Option<(message_id, blocks)>`（`:1554-1563`）。
2. 连接循环广播 `AcpEvent::UserMessage { message_id, blocks }`。
3. `SessionState` 把它收进 `pending_user_message`（`acp/session_state.rs:956-963`），
   快照字段同名，给中途 attach 的客户端补这条 user turn。
4. 详情接口 `apply_in_flight_message_id`（`commands/conversations.rs:1648-1693`）
   用 `pending.message_id` **盖** 匹配到的那条持久化 user turn 的 `MessageTurn.id`，
   好让前端按 id 把「广播回声」和「parser 刚写进 transcript 的那条」收成一条。

`client_message_id` 的来源（全部是调用方供给，服务端不生成乐观 id）：

| 来源 | 形状 | 证据 |
|---|---|---|
| 诚实 UI 发送 | `optimistic-${randomUUID()}` | `src/components/conversations/conversation-detail-panel.tsx:211, 1592, 1667`；经 `clientMessageId: optimisticTurn.id`（`:1208, 1312, 1609, 1684`）进 `send_prompt` |
| HTTP / 服务器模式 | 请求 JSON 的 `clientMessageId`，**无格式校验** | `web/handlers/acp.rs:171, 186` → 原样传入 `send_prompt_linked_with_message_id` |
| Host Control 首 prompt | `host-control:{request_id}` | `commands/host_control_session.rs:291` |
| 非 UI / 缺省 | `user-{conn_id}-{event_seq}` | `acp/manager.rs:1436` |

函数注释写的「untrusted client-supplied `message_id`」指的就是这根线：
UI 把乐观 turn id 交给后端，后端当 `UserMessage.message_id` 广播出去，
**别的窗口按精确 id 做去重**。

`conversations.rs:1679` 只在注释里提到这个谓词，**不调用它**。第二道防线是
同函数的 collide 检查（`:1685-1690`）：如果 `pending.message_id` 已经出现在
另一条 turn 上，就拒绝盖章，把那条 turn 留在 parser id 下。

---

## 3. 撞上会怎样（证据链）

前端 id 去重是精确匹配 + **限定 user 角色**：

`src/stores/conversation-runtime-store.ts:2026-2031`（`APPEND_VIEWER_USER_TURN`）：

- `optimisticTurns` / `localTurns` / `detail.turns` 里只要有
  `t.id === message_id && t.role === "user"`，就 **丢掉这条广播回声**
  （只更新 batch 边界，不追加可见 turn）。
- 同文件 `:2020-2024` 写明：角色限制就是为了防止「client id 滑进另一个
  命名空间、撞上一条 assistant turn」时把新 prompt 压掉。

时间线合并 `dedupeTimeline`（`:2742-2759`）的 key 是
`` `${turn.role} ${turn.id}` ``（`:2704`）。user turn **keep-first**：
两条 user turn 共享 id 时，留下先看到的那条，后面的静默消失。
`commands/conversations.rs:1622, 1671` 把这个称为
「frontend's keep-first user dedup」，并说明一旦把**历史** user turn 错盖成
新 prompt 的 broadcast id，新 prompt 会被藏起来。

把两条拼起来，对 **未覆盖** 的命名空间走一遍：

假设 Cursor 会话已有 user turn `cursor-turn-0`，调用方把
`client_message_id = "cursor-turn-0"` 送进来（HTTP 可以；诚实 UI 不会）：

1. `is_reserved_turn_id("cursor-turn-0") == false` → 广播 id 就是 `cursor-turn-0`。
2. 其它窗口 `APPEND_VIEWER_USER_TURN`：`detail.turns` 里已有同 id 的 user turn
   → **压掉 in-flight 回声**。在 persist 之前，旁观窗口看不到这条新 prompt。
3. Agent 照常收到 prompt（谓词不拦发送）。
4. parser 给新 turn 分配 `cursor-turn-N`（N≠0）。
5. `apply_in_flight_message_id` 发现 `pending.message_id == "cursor-turn-0"`
   已在另一条 turn 上 → **拒绝盖章**（`:1685-1690`）。新 turn 保持 `cursor-turn-N`。
6. 详情刷新后，旁观窗口从 `detail.turns` 看到 `cursor-turn-N`。prompt 重新可见。

对 `turn-<digits>`（11 家 + Qoder），步骤 1 会改写成 `user-{conn}-{seq}`，
广播 id 与历史 turn 无交集，步骤 2 不会误伤。这就是谓词存在的原因。

Cline / Grok 同构，只是碰撞串分别是 `{taskDir}-{k}` 和 `grok-turn-{i}`。
Cline 的 task 目录名通常不是攻击者随口猜的，但知道 `conversation_id` 的调用方
（已经握着该会话）可以构造。

### 为什么判「低危」而不是「可利用」

「可利用」在这里指：不需要改 UI、就能造成**持久**的 prompt 消失或跨客户端吞消息。
当前做不到：

- 诚实 UI 的 id 前缀是 `optimistic-`，与三家例外命名空间不相交。
- 能设任意 `client_message_id` 的人已经能发 prompt（持有连接 + 鉴权）。
- collide 守卫挡住了 keep-first 那条**持久隐藏**路径。
- 效果限于「旁观窗口 in-flight 一段时间看不见」；persist 后恢复。
- 不能把别人的历史 turn 删掉，也不能冒充其它会话。

所以是覆盖缺口，不是能直接打的洞。扩 matcher 或改成 allowlist 即可补上，
但那是逻辑变更，本包按规格停在注释 + 报告。

---

## 4. 本包改动

- `acp/manager.rs`：函数注释去掉「every parser assigns `turn-{n}`」；写明 11/14
  覆盖、三家例外、以及扩 matcher 需协调者拍板。
- 同文件单测：钉 `cursor-turn-0` / `grok-turn-0` / `{taskId}-1` **当前不被保留**
  （记录缺口，不是把缺口当成目标终态）。
- 未改 `is_reserved_turn_id` 的布尔实现。
