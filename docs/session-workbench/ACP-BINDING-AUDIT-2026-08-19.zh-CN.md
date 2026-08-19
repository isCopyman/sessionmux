# ACP 会话绑定审计：两个上游修复在我方机制里是否仍有洞

审计对象：上游 `522ab922`（"stop a turn landing in a session its conversation doesn't own"）与
`fbb0ca06`（"stop a new agent session from orphaning a conversation's history"）所治的两类 bug，
在我方重写后的 ACP 绑定机制（`send_prompt_linked` / `update_external_id` / `ConversationLinked`）
里是否仍然存在。审计方式：只读，全部结论基于对 worktree `wt/acp-binding-audit`（从
`codex/session-message-v1` 分出）内文件的实际读取与对两个上游 commit 全量 diff 的阅读，未编译、未运行、未跑测试。

结论先行：**两个 bug 都还在，且我方代码就是上游修复前的原始形态**——`bind_external_id` 这个上游靶
函数在我方从未存在过，我方的 `update_external_id`（`src-tauri/src/db/service/conversation_service.rs:430-444`）
逐字节对应上游 `fbb0ca06` diff 里被替换掉的**旧版本**，三处调用点（`acp/manager.rs`、`acp/lifecycle.rs`、
`chat_channel/session_event_subscriber.rs`）也都还是上游修复前的调用形态。但我方并非一张白纸：数据库层已经有
和上游同名的唯一索引，`fork_session` 已经独立实现了一套"拆分保留"的事务模式，自定义 agent 的
`continues_from` 续接链也已经存在——这些都可以直接复用为修复的地基，且改动范围可以比字面复刻上游 diff 更小。

## 一、场景清单（从两个上游 commit 提炞）

### 来自 `fbb0ca06`（历史失联 / codeg#500）

- **A1 · 重连丢会话 ID 导致覆盖**：一个连接以 `session_id = None` 重建（重连丢失了 id），或者
  `session/load` 遇到一个未分类的失败、降级走 `session/new`，拿到一个全新的 external_id（记为 S2）。
  随后一条携带既有 `conversation_id`（该行 DB 里记的是旧会话 S1）的 prompt 通过 `send_prompt_linked`
  的 Branch A"认领"这一行，把 S2 直接写死到这一行的 `external_id` 上。因为会话列表是纯 DB 行查询、
  从不扫描 agent 自己的历史存储，S1 从此不被任何行引用——用户看到的是"这个进行了 49 分钟的会话消
  失了"。
- **A2 · 续接豁免（防止引入新 bug 的设计约束）**：memory-only 的自定义 agent 每次重启都会忘记会话，
  codeg 开一个新会话并靠 `continues_from` 把新旧两份 transcript 接起来，读取端应当把这条链渲染成同一
  个会话。如果修复 A1 时不做这个豁免，每次自定义 agent 重启都会把同一个会话在侧边栏拆成两个、并把
  token 计数翻倍。
- **A3 · 三处写入点必须统一守护并广播**：`SessionStarted` 在我们仓库里有三个消费点（`acp/manager.rs`
  的 `send_prompt_linked`、`acp/lifecycle.rs` 的 `handle_event`、`chat_channel/session_event_subscriber.rs`
  的 `handle_acp_envelope`），任何一处仍然裸写都会重新打开 A1 的洞；而"拆分保留出一行"只完成一半——
  侧边栏只在收到 `conversation://changed` upsert 时才会显示一个它从未见过的会话，新行不广播等于用
  户依然看不到。
- **A4 · chat-channel 订阅者要用长生命周期 emitter**：该订阅者异步消费总线上的事件，而连接清理会在
  排队完终态事件后立刻摘掉 manager 里的连接条目；如果广播用的 emitter 是按 `connection_id` 现查的，
  就可能在这个窗口期查到空、把刚保留出来的行的广播直接吞掉。
- **A5 · fork 的 sibling INSERT 要幂等**：fork 的回复先于它自己的 `SessionStarted{S2}` 发出，所以负责
  持久化 fork 结果的任务和 lifecycle worker 是真并发的；如果两边都想为"被让出的旧会话"插入一行保留
  记录，后插入的那个会撞上 `(external_id, agent_type)` 的唯一索引，把整个 fork 事务回滚掉。

### 来自 `522ab922`（turn 落错会话）

- **B1 · 冲突必须是一个不可重试、和"绑定成功"/"瞬时故障"都能区分的信号**：当目标 external_id 已经被
  另一行持有时，写入函数不能返回一个调用方会当成"绑定成功"来用的值（哪怕是 `Ok(None)`），也不能让
  它被拍扁成和 SQLite 锁竞争同一种 `DbError::Database`——前者直接导致误发，后者会被重试逻辑当瞬时故
  障反复重试，而这个冲突永远不会因为重试而消失。
- **B2 · 必须先绑定、后宣告**：`send_prompt_linked` 一旦 `emit_with_state(ConversationLinked)`，就会把
  `state.conversation_id` 焊死，后续调用的 `already_linked` 判断就此为真。如果宣告在绑定之前，绑定失
  败时状态已经焊死——下一次调用会整体跳过"绑定"这个代码块,直接把 turn 发出去,而这一行在 DB 里从未
  真正拿到那个 external_id。
- **B3 · 绑定阶段要防止调用方任务被取消撕裂**：宣告和绑定之间如果有 await 点，调用方（典型场景：服
  务器模式下 HTTP 客户端中途断开）中止 future 会让操作停在中间——要么绑定已提交但没广播（行对谁都
  不可见），要么宣告已发生但绑定没提交（状态永久焊死在一个数据库里其实没拿到会话的行上）。两种顺序
  都不安全，唯一出路是把这一段做成不可被外部取消的。
- **B4 · chat-channel 订阅者收到 Conflict 要拆路由，但必须判断事件是否仍是"当前"**：仅仅跳过这一次
  kickoff 是假象——`pending_prompt` 还在，后续的 `TurnComplete` 会不做归属检查就把它重新发出去；桥接
  条目还在，对方下一条消息一样会顺着这条路由派发进错的会话。但拆除动作必须先确认这条（可能滞后到达
  的）事件说的还是连接当前真正持有的会话，否则一个 fork 之后才姗姗来迟的旧 `SessionStarted`，会把一
  条已经健康切换到新会话的路由错杀掉。

## 二、我方机制图

亲自追踪的写入路径（均已用 `Read`/`Grep` 核对到具体行）：

1. **`update_external_id`**（`src-tauri/src/db/service/conversation_service.rs:430-444`）：一条裸的
   `UPDATE conversation SET external_id = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL`。不读旧
   值、不比较、不拆分、不检测冲突——这就是上游 `fbb0ca06` diff 里 `-` 掉的那个函数，字段名、guard 条
   件（只有 `deleted_at IS NULL`）完全一致。
2. **三个调用点，全部是上游修复前的形态**：
   - `acp/manager.rs:1226-1318`（`send_prompt_linked_with_message_id` 的链接段）：Branch A（第 1229-1241
     行）在 `!already_linked` 时先 `emit_with_state(..., AcpEvent::ConversationLinked{conversation_id:
     caller_conv_id, ...})`（第 1230-1240 行），再往下（第 1299-1302 行）才 `update_external_id(&db.conn,
     cid, eid)`，中间没有事务、没有取消保护、失败了也不撤销刚才的宣告。
   - `acp/lifecycle.rs:89-106`（`handle_event` 的 `SessionStarted` 分支）：第 96 行直接
     `update_external_id(db, conversation_id, session_id.clone()).await?`，`?` 会把任何失败（包括撞唯一
     索引）交给 `handle_event_with_retry`（`acp/lifecycle.rs:57-81`）用 100ms/500ms 重试两次——重试对一
     个"另一行已经占了这个 id"的永久性冲突毫无意义，只是白白重试三次再打一条 ERROR 日志。
   - `chat_channel/session_event_subscriber.rs:107-164`（`handle_acp_envelope` 的 `SessionStarted` 分
     支）：第 120-125 行 `let _ = conversation_service::update_external_id(...).await;`——**返回值被直
     接丢弃**，不管绑定成不成功，第 127-161 行都会无条件把 `session.pending_prompt` 取出来经
     `send_prompt_queue_aware` 发出去。这是三处里最裸的一处：连"发生冲突时不要重试"这一层都没有，冲
     突第一次出现就会误发，不需要等到第二次调用触发 B2 的锁存。
   - 额外的第四处（不在上游两个 commit 范围内，但同样在写 `external_id`）：
     `commands/host_control_session.rs:706-729`，Host Control 创建原生会话后的落库写入。这一处**已经
     在检查 `Result`**（第 706 行 `if let Err(error) = ... { 把行置为 Cancelled 并返回失败 }`），而且
     写入对象通常是刚分配的全新行（没有旧会话要保护），风险明显低于前三处，但仍然共享同一个"冲突到
     底是什么" 的语义空白。
3. **`DbError::Conflict` 已经存在，但语义和上游要的相反**（`src-tauri/src/db/error.rs:13-16`）：文档写
   明"A compare-and-swap guard lost the race... Retry by re-reading."——这是我们自己给 CAS 竞争用的、
   **可重试**的变体，唯一现有调用方是 `session_timer_service.rs`（4 处）。上游新增的同名变体表示的是
   "唯一键已被占用、任何重试都没用、调用方必须放弃"，和我们现有语义正好相反。任何修复都**不能**复用
   `DbError::Conflict` 来表示绑定冲突，否则会把两种截然不同的"重试策略"混进同一个类型里。
4. **数据库层的唯一索引已经存在，且和上游同名**：`src-tauri/src/db/migration/m20260211_000001_init.rs:268-273`
   建了 `idx_conversation_external_agent`，UNIQUE 在 `(external_id, agent_type)` 上，没有 `deleted_at`
   谓词（软删行照样占着这个 id，和上游 `522ab922` 注释里说的完全一致）。这解释了为什么 B1（两行撞车）
   在 `send_prompt_linked` 里第一次调用不会静默成功、而是会报一个泛化的 DB 错误——真正的洞不在"会不
   会报错"，而在"报错之后状态已经焊死，第二次调用会跳过检查直接发送"（B2）以及"chat-channel 那处根
   本不看这个错误"（B1 在那里最赤裸）。
5. **`continues_from` 续接链已经存在，但只服务于读取端，从未接入写入端的绑定判断**：
   - `acp_transcript.rs`：`TranscriptHeader::continues_from` 字段（约 104/116/122 行）、
     `TranscriptHeader::continuing()`。
   - `acp/connection.rs:568-598`：`record_transcript_header` / `record_transcript_header_continuing` ——
     后者是**同步、fire-and-forget**的（`drop(crate::acp_transcript::record_header(dir, &header));`），
     没有像上游 `fbb0ca06` 那样改成 `async` 并在 `continues_from.is_some()` 时等落盘 ack。
   - 真正触发续接的地方：`acp/connection.rs:4846-4863`——`session/load` 失败降级到 `session/new` 拿
     到 `fallback_sid` 后，调用 `record_transcript_header_continuing(agent_type, &fallback_sid, ...,
     Some(sid.as_str()))`（不 `await` 任何东西，因为函数本身不是 async），然后立刻
     `emit_with_state(..., AcpEvent::SessionStarted{session_id: fallback_sid})`。
   - 读取端：`parsers/acp_native.rs:85,107` 用 `acp_transcript::read_chain_in(&self.root, dir,
     conversation_id)`——只要行的 `external_id` 最终指向链条里**最新**的那个 id，`read_chain_in` 会自
     动往回追 `continues_from` 拼出完整历史。**这是我方代码里一个意料之外、但确实存在的"半免疫"**：
     对自定义 agent（`transcript_dir_for` 返回 `Some` 的那些）而言，A1 场景下即便 `update_external_id`
     裸写覆盖了 `external_id`，只要新值就是链条的最新一环，用户重新打开这个会话时看到的还是完整历史
     ——不是因为谁刻意保护了它，而是读取端的链式合并顺带兜住了。这份免疫是不完整的：header 写入没有
     durability 等待，存在一个理论上的竞态窗口（新 `SessionStarted` 广播出去、其他消费者据此渲染时，
     `continues_from` 还没落盘）；而且它完全不覆盖 A1 的另一半——**没有 transcript 目录的原生 agent**
     （ClaudeCode、Codex、Gemini、OpenCode、Cline 等——即 `AgentType::Custom(_)` 之外的所有类型）。这
     些 agent 的历史只活在它们自己的存储里，`external_id` 是唯一的钥匙，一旦被覆盖就是真正的、无任何
     兜底的数据丢失。
   - 我方**没有**上游 `acp/mod.rs` 里那个 `continued_session_ids()` 辅助函数，也没有任何等价的"读取
     `continues_from` 链、喂给绑定判断"的调用；grep 全仓库确认不存在。
6. **`fork_session` 已经独立实现了一套安全的"拆分保留"模式，可以直接借用**：
   - `persist_fork_outcome`（`acp/manager.rs:2030-2168`）：一个事务里先"自赋值 UPDATE"抢写锁
     （2051-2059 行）、再 SELECT 校验 `current.external_id` 还等于调用方预期的旧会话（2081-2086 行，
     不等就整体中止 fork）、然后 INSERT 一行全新的会话承接 fork 出的新分支（2114-2147 行）。这个形状
     和上游 `bind_external_id` 的"claim → read-under-lock → 校验 → insert"如出一辙。
   - 但我方 fork 的方向和上游相反：**原行 C1 永远留着它原来的会话 S1 不动，fork 出来的新会话 S2 落在
     一行全新插入的记录上**（不是像上游那样把 C1 推进到 S2、把 S1 挪到新插的 sibling 上）。这意味着我
     方 fork 从设计上就不存在"C1 释放 S1、另一条并发路径也想认领 S1"这种竞争——A5 描述的那种唯一索引
     冲突回滚整个 fork 的场景，在我方当前实现里结构性地不会发生（见下表判定）。
   - `fork_session`（`acp/manager.rs:1869-1928`）本身已经有一套**取消保护**的模板：`tokio::spawn` 出
     一个 detached task 持有 `_prompt_guard`（1881-1884 行），协议往返和 `persist_fork_outcome` 都在这
     个 task 内部完成，外层只 `await` 这个 handle 把结果转交给还活着的调用方（"the result is harmlessly
     discarded if the caller is gone"）。这正是 B3 需要的形状，而且已经在我们自己的代码里跑着——修复
     `send_prompt_linked` 时不需要发明新模式，照抄这一段就行。
   - `chat_channel/session_event_subscriber.rs:31-37` 的 `spawn_session_event_subscriber` 签名里**没有**
     `emitter` 参数（和上游修复前一致），说明 A4 描述的那类"按 connection_id 现查 emitter 可能查空"的
     风险目前虽然没有实际后果（因为这条路径根本没有任何"保留行广播"的逻辑），但一旦要修复 A1/A3，这
     里必须新增一个长生命周期的 emitter 参数，不能简单地在处理函数内部按 `connection_id` 去 `conn_mgr`
     现查。

## 三、逐条判定表

| 场景 | 判定 | 依据（file:line） |
|---|---|---|
| A1（重连/降级覆盖导致历史失联）——**原生/内建 agent**（无 transcript 目录的类型） | **仍有洞**，且是完全裸奔：无任何拆分、无任何告警，静默覆盖 | `conversation_service.rs:430-444`（裸写）；三处调用点 `manager.rs:1300`、`lifecycle.rs:96`、`session_event_subscriber.rs:120-125`；触发源 `connection.rs:4830-4863`（session/load 失败→session/new） |
| A1——**自定义 agent**（有 transcript 目录、走 `continues_from`） | **部分免疫**：DB 行本身依然被裸写覆盖，但读取端 `read_chain_in` 顺着 `continues_from` 往回拼历史，用户在常见路径下看不到数据丢失；免疫不完整（header 落盘无 durability 等待，且完全不检查"旧 id 是否已被另一行持有"这一半） | 触发链 `connection.rs:4846-4863` → `record_transcript_header_continuing`（`connection.rs:568-598`，非 async、fire-and-forget）；读取端 `parsers/acp_native.rs:85,107` 调 `acp_transcript::read_chain_in` |
| A2（续接豁免，防止拆分逻辑本身引入新 bug） | **不适用于"现状"判定**（我方根本没有拆分逻辑可言），但是**修复设计的强制约束**：任何引入拆分/保留的修复都必须先查 `continues_from` 链再决定要不要拆分，否则会在自定义 agent 每次重启时把同一个会话复制成两个 | 续接机制已存在：`acp_transcript.rs` 的 `TranscriptHeader::continues_from`；我方**没有**上游 `acp/mod.rs::continued_session_ids()` 等价物（全仓库 grep 确认缺失） |
| A3（三处写入统一守护 + 保留行必须广播） | **仍有洞**：三处（含第四处 `host_control_session.rs`）各写各的，没有共享守护；因为我方目前没有"保留行"概念，广播缺失这半目前无从谈起，但同样是修复必须覆盖的范围 | 见上文四处调用点列表 |
| A4（chat-channel 订阅者用长生命周期 emitter） | **目前无实际后果，但结构性缺失**：函数签名里根本没有 emitter，一旦修复引入保留行广播，必须同步把这个参数加上，否则会复现上游描述的"按 connection_id 现查、连接已清理、广播丢失"竞态 | `chat_channel/session_event_subscriber.rs:31-37`（签名无 `emitter`） |
| A5（fork sibling INSERT 幂等） | **结构性免疫**：我方 fork 从不释放原行的旧会话，新会话永远落在新插入的行上，不存在"两条路径抢着为同一个旧会话插入保留行"的竞争 | `persist_fork_outcome`，`acp/manager.rs:2030-2168`，尤其 2081-2086（校验旧 id 不变）与 2114-2147（INSERT 新行装新会话，不是旧会话） |
| B1（冲突要有独立、不可重试的信号） | **仍有洞**：三处写入点都只会拿到一个笼统的 `DbError::Database`（唯一索引撞车时）或者被直接丢弃（chat-channel 那处）。我方现成的 `DbError::Conflict` 语义与需求相反，**不能**复用 | `db/error.rs:13-16`（"Retry by re-reading" 的 CAS 语义）；三处调用点同上 |
| B2（先绑定、后宣告） | **仍有洞**，且可以逐步走通复现：`manager.rs:1230-1240` 的 `emit_with_state(ConversationLinked)` 在 `manager.rs:1300` 的 `update_external_id` 之前执行；`ConversationLinked` 落地为 `self.conversation_id = Some(*conversation_id)`（`session_state.rs:1001-1008`），这个赋值和后面绑定成不成功无关。第一次调用里绑定失败会 `?` 提前返回（`manager.rs:1302`），但 `already_linked` 已经永久为真；第二次调用直接跳过整个 `if !already_linked` 块（`manager.rs:1226`），带着连接当时真正持有的会话把 turn 发出去，而这一行在 DB 里从未真正拿到那个 external_id | `manager.rs:1226-1318`；`session_state.rs:1001-1008` |
| B3（绑定阶段防止调用方取消撕裂） | **仍有洞**（未发现任何保护）：`send_prompt_linked_with_message_id` 里从宣告到绑定之间是纯内联 `await`，没有 spawn、没有 detached task；如果外层 future 被中止（服务器模式下 HTTP 客户端断连是最直接的触发方式），会停在宣告和绑定之间的某个 await 点，效果和 B2 描述的错误路径一样——状态焊死但 DB 未提交 | `manager.rs:1226-1318`（对照：`fork_session` 已有的取消保护模板在 `manager.rs:1869-1928`，`send_prompt_linked` 没有等价物） |
| B4（chat-channel 冲突时拆路由，且要判断是否过期） | **仍有洞，而且是三处里最严重的**：这里连"检查一下 `update_external_id` 是否失败"都没有做（`let _ =`），冲突第一次发生就会把 `pending_prompt` 无条件发出去；不需要像 B2 那样等第二次调用触发锁存 | `session_event_subscriber.rs:107-164`，尤其第 120-125 行（丢弃返回值）与 127-161 行（无条件派发） |

## 四、修复设计（只说策略，不写实现代码）

以下均以"贴我方现有机制"为原则：复用已经存在的唯一索引、`fork_session`/`persist_fork_outcome` 的事务
与取消保护形状、`continues_from` 续接链，新增的东西控制在"一个新的绑定函数 + 一个新的错误变体 + 若干
调用点改造"。

**1. 用一个新函数取代 `update_external_id` 作为唯一写入口，形状抄 `persist_fork_outcome`。**
新函数（可以叫 `bind_external_id` 或任何不撞现有名字的名字）内部：自赋值 UPDATE 抢写锁 → 在同一把锁下
SELECT 出当前行的旧 `external_id` → 判断"旧值是否等于新值 / 旧值是否在 `continues_from` 链里"（这一步
直接调用一个新增的、类似上游 `continued_session_ids()` 的辅助函数，读 `acp_transcript` 的
`continues_from` 链——只需要给自定义 agent 返回非空结果，内建 agent 恒定返回空，逻辑上等价于 A2 的豁
免）→ 如果是"无需拆分"的情况就地更新并返回；如果需要拆分，先查有没有别的活着的行已经占着旧
`external_id`（`already_preserved`，避免对一个已经被 fork 或者被别的路径处理过的旧会话重复插入）→ 没
有就在同一事务里 INSERT 一行，把标题、时间戳、文件夹、状态（`InProgress` 映射成一个"没有 agent 挂着"
的终态，可以照抄 fork 现有对状态的处理方式）搬过去 → 返回"被保留的新行 id"，供调用方广播。这一步直接
解决 A1（内建 agent 那半）和 A3 的"要不要拆"部分。

**2. 新增一个独立的错误变体表示"目标 external_id 已被另一行占用"，绝不复用 `DbError::Conflict`。**
命名要能让读代码的人一眼看出"这个不能重试"，例如 `DbError::ExternalIdTaken` 或
`DbError::SessionBoundElsewhere`（避免任何形式的"Conflict"字样，防止未来有人望文生义地把它和
`session_timer_service.rs` 那个可重试的 `Conflict` 混用）。新函数的绑定逻辑发现唯一索引会被撞上时，
不能让 `sea_orm::DbErr` 从事务闭包里直接抛出——那样会被 `TransactionError` 拍扁成 `DbError::Database`，
和锁竞争无法区分。要在闭包里返回一个显式的"结果枚举"（`Bound(Option<i32>)` / `Refused{holder_row_id}`
这种形状，事务提交之后在外面再翻译成新的错误变体），这样调用方能明确分辨"绑定成功"“瞬时故障，可以
重试”“永久冲突，必须放弃"三种情况。这一步解决 B1。

**3. `send_prompt_linked_with_message_id` 改成先绑定、后宣告。**
把 `manager.rs:1226-1318` 这一段的顺序倒过来：先解析出这条 prompt 应该落在哪一行（Branch A/B 的判断
本身不涉及 DB 写入，可以留在前面），但**不要**在这时候就 `emit_with_state(ConversationLinked)`；等新
的绑定函数返回"确实绑定成功"之后再宣告。绑定失败（无论是 `ExternalIdTaken` 还是其他 DB 错误）直接
`?` 返回，此时 `state.conversation_id` 从未被设置，`already_linked` 保持假，下一次调用会重新走一遍完
整的绑定检查，而不是抄近路直接发送。这一步解决 B2。

**4. 把"链接 + 绑定 + 宣告"整段包进一个取消保护的 detached task，直接照抄 `fork_session` 的形状。**
`manager.rs:1869-1928` 已经是现成模板：`tokio::spawn` 一个 task，把 `_prompt_guard` 的所有权转进去，
里面完成"必要时创建新行 → 绑定 → 宣告 → 广播"，外层只 `await` 这个 handle 把结果转交给可能已经不在
的调用方。绑定失败时如果这一路径是 Branch B（本函数刚刚新建了一行），要仿照上游那样把这行清理掉（软
删），不然每一次被拒绝的重试都会在侧边栏刷新时冒出一个空的、无标题的幽灵会话。这一步解决 B3，同时是
A3"新建行不广播就等于看不见"这条要求在 Branch B 失败路径上的收尾。

**5. `acp/lifecycle.rs:89-106` 和 `commands/host_control_session.rs:706-729` 改调新绑定函数，并按错误
类型分流。**
`lifecycle.rs` 这处遇到 `ExternalIdTaken` 应该直接放弃、记录一条 WARN 就返回，不必再走
`handle_event_with_retry` 的两次退避重试（重试对这类错误没有意义）；遇到其他 `DbError` 才保留现有的
重试语义。`host_control_session.rs` 已经在检查 `Result`，只需要把分支从"任何 Err 都当创建失败处理"细
化成"`ExternalIdTaken` 明确提示是绑定冲突，其余错误保留现有措辞"，行为上不需要大改。两处只要绑定成功
返回了"保留行"就都要调 `emit_conversation_upsert`（`commands/conversations.rs` 里已有这个辅助函数，直
接复用）。这一步覆盖 A3 里"三处（实际四处）统一走同一套守护"的剩余部分。

**6. `chat_channel/session_event_subscriber.rs` 是修复里改动最大的一处，两件事都要做。**
第一，`spawn_session_event_subscriber`（`session_event_subscriber.rs:31-37`）要新增一个长生命周期
`emitter` 参数，从进程级的广播器传入，而不是在处理函数内部按 `connection_id` 现查——这是 A4 的要求，
必须先做，否则第二步新增的广播会在连接清理的竞态窗口里丢失。第二，`handle_acp_envelope` 的
`SessionStarted` 分支（`session_event_subscriber.rs:117-164`）要把 `let _ = update_external_id(...)`
换成检查新绑定函数的返回值：`Ok(Bound(preserved))` 才继续走现在的"取出 pending_prompt 并发送"逻辑，
`preserved` 非空时先广播保留行；`Err(ExternalIdTaken)` 要仿照上游那样撤掉这条路由——从 `bridge` 里移
除这个连接的 session 条目、把它指向的会话行状态置为 `Cancelled`、清掉持久化的 channel→session 路由,
但这个撤除动作必须先确认这条事件里的 `session_id` 仍然等于这个连接**当前**实际持有的 external_id（
从 `conn_mgr.get_state(connection_id)` 现读一次），否则一条因为 fork 之类操作而滞后到达的旧事件会把
一条其实已经健康切到新会话的路由错杀。`Err` 是其他 DB 错误时保留现在"记日志、继续尝试发送"的行为，
避免瞬时故障也白白牺牲掉一次任务的开场 prompt。这一步解决 B4，同时收尾 A3/A4。

**7. `acp/connection.rs:568-598` 的 `record_transcript_header_continuing` 要在 `continues_from.is_some()`
时等落盘 ack。**
现在是同步函数、`drop()` 掉写入句柄；需要仿照上游把它改成 `async`，`continues_from` 非空时用一个短
超时（上游用的是 2 秒）`await` 底层写入线程的完成通知，再返回，调用方（`connection.rs:4846-4863`）跟
着改成 `.await`。原因和第 1 条的绑定函数直接相关：新绑定函数要读 `continues_from` 链来判断"要不要拆
分"，如果这条链还没落盘就已经在广播 `SessionStarted`，绑定逻辑会读到一条空链，把一次正常的自定义
agent 重启误判成"无关会话"，从而永久性地把同一个会话拆成两个——这正是 A2 要求防止的情形，如果不修
这一处，第 1 条修复反而会主动制造这个新 bug。

## 五、遗留问题

- **B3 的取消保护范围没有验证到底层调用方**：我确认了 `send_prompt_linked_with_message_id` 函数体本
  身没有取消保护，但没有逐一追踪它在服务器模式（`web/handlers/acp.rs`）和桌面模式
  （`commands/acp.rs`）下的每一个调用点是否可能在 axum 请求被取消时真的把这个 future 中止掉——只读审
  计的时间预算内确认了"函数自身无保护"，这个结论足以支撑"仍有洞"的判定，但"实际触发有多容易"取决于
  服务器模式下请求取消的具体频率，需要有写权限的实现者在改的时候顺手确认一下调用栈。
- **A1 的"部分免疫"边界需要更精确的复现**：我依据代码读出了"自定义 agent 靠 `read_chain_in` 兜底"这
  条推理链，但没有跑测试或者手动搭一个自定义 agent 环境去实际复现"重连丢 id → 覆盖 → 重新打开会话
  → 历史还在"这个完整路径；如果某个自定义 agent 的 parser 分支和 `AcpNativeParser` 的行为不完全一致
  （比如 Gemini/Cline 那条"stale-external-id fallback"路径，上游注释里提到过 `origin_cwd` 相关的兜底），
  这份"部分免疫"的结论可能需要针对具体 agent 类型再细化。
- **第四处写入点（`host_control_session.rs`）是否还有其他姊妹路径**：Host Control 是一个我在这次审计
  里只看了一个函数的大子系统（`codeg-host-control` 相关），没有把它所有创建/恢复会话的路径都过一遍；
  如果那里还有别的地方写 `external_id`，需要在实现阶段一并拉出来核对。
- **修复设计第 1 条里"状态映射"的具体规则没有深入设计**：只指出"可以照抄 fork 现有对状态的处理方
  式"，但我方的 `ConversationStatus` 枚举和状态转换规则是否和上游完全一致（例如是否也有
  `PendingReview` 这个终态、`InProgress` 是否总是应该映射成它）需要实现者对照我方 `conversation`
  实体的完整状态机确认，这次审计没有逐一核对。
