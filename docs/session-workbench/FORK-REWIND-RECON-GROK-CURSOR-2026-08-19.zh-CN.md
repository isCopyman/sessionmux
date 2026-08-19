# Grok / Cursor-CLI 按消息 Fork 可行性侦察：档位结论

> 状态：2026-08-19 只读调研（codeg 源码 + 三个参考仓只读 + 联网查证 xAI/Cursor 官方文档与社区实现）。
> 姐妹篇：[SESSION-FORK-REWIND-SURVEY-2026-08-19](./SESSION-FORK-REWIND-SURVEY-2026-08-19.zh-CN.md)、
> [ACP-ECOSYSTEM-ADAPTER-SURVEY-2026-08-19](./ACP-ECOSYSTEM-ADAPTER-SURVEY-2026-08-19.zh-CN.md)（已证实的事实本文不重复）。

结论先行：**grok 建议档位 A（原生协议路径大概率已存在，需实测重新确认）**——第三方 ACP 客户端
`RongleCat/grok-app`（commit `e9502bcd`，2026-08-19 当天提交）的代码显示它对 grok `agent stdio`
调用 ACP `session/fork` 做整会话拷贝，并调用两个私有扩展方法 `x.ai/rewind/points` /
`x.ai/rewind/execute`（0-based 用户轮次 `targetPromptIndex` 截断 + 可选文件回退），这与 codeg 自己
`registry.rs` 里"grok 1.0.4 只广告 `{list,resume,close}`"的注释直接矛盾，最可能的解释是 1.0.5
（08-18 发布，codeg 未跟）新增了这些能力，需要升级实测。**cursor 建议档位 C（重放降级）**——
cursor-agent CLI 的 fork-session 是论坛上一直未被官方回应的功能请求（2026-01-21 提出、2026-04-19
仍确认缺失），会话存储是 protobuf 内容寻址 DAG，结构上比 claude/grok 更适合截断手术，但零第三方
先例、需要新写 protobuf 编码器，第一版不值得投入。

## 1. Grok 原生底座

**ACP 面 vs CLI/TUI 面的落差**：codeg 自己的能力注释（`src-tauri/src/acp/registry.rs:808-821`）
称"验证过 1.0.0/1.0.4 二进制，`initialize` 只广告 `sessionCapabilities.{list,resume,close}`，
无 fork"。但 xAI 官方文档（`docs.x.ai/build/cli/reference`、`docs.x.ai/build/features/sessions`，
联网查证）明确记载 CLI 有独立于 ACP 的会话原语：TUI `/fork [directive]`（"branches the current
session into a peer that starts from a copy of the conversation"，整会话头 fork）、
`--resume <id> --fork-session`（"When resuming, fork into a new session ID"，同样是整会话拷贝，
可配 `--worktree`/`--ref` 隔离文件）、`/rewind`（"Rewind to a previous turn"，按 prompt
截断历史+可选恢复文件，语义上是"truncate 而非 branch"，与 claude `resumeSessionAt` 同类）。社区
指南（`gist.github.com/ben-vargas/579a41b91849c354d9c32d8dac590f1f`，未经官方确认）补充 `/rewind`
在 TUI 里也绑定 `Esc Esc`（idle 时）。**这些都是 CLI/TUI 层原语，不在 codeg 实际驱动的
`grok agent stdio` ACP 线上**——`docs.x.ai/build/cli/headless-scripting` 明确非交互 `-p` 模式不支持
把 `/rewind`、`/fork` 当 slash command 送入，只列了 `--session-id`/`--resume`/`--continue` 三个
会话 flag，无 rewind/checkpoint flag。

**第三方 ACP 客户端的实证（分量最重的证据）**：`RongleCat/grok-app`（Tauri 2 + Rust，README 自称
"Default `grok agent stdio` (ACP); host-owned session FSM"，与 codeg 走同一条 ACP 线）
`src-tauri/src/acp_client.rs:388-399` 注释："Host `agent stdio` uses ACP `session/fork` instead
of bare CLI flags (CLI errors without resume)"——即它对 ACP 连接直接发 `session/fork` 请求做整会话
fork；`parse_fork_session_id`（同文件 405-420）专门兼容"Grok extension `newSessionId`"这个非标准
响应字段名，这种细节只有真跑过才写得出来，不像凭空猜测。更关键的是 `rewind_execute_for`
（`acp_client.rs:3467-3504`）：调用方法名候选表是 `["x.ai/rewind/execute", "_x.ai/rewind/execute"]`
（`acp_client.rs:3697-3699`，stdio 下划线兜底），参数 `{sessionId, targetPromptIndex, restoreFiles,
restore_files}`（两种大小写都塞，注释"Some builds accept this camelCase alias"），失败按
`-32601`/"unknown method" 等特征判定为"未支持"并把 `rewind_supported` 缓存为 false（不是硬编码
猜测，是运行时探测结果）；配对的 `x.ai/rewind/points`（`acp_client.rs:3425-3439`，"List rewind
points (one per user prompt)"）用来枚举可回退点。**`targetPromptIndex` 与 codeg 自己解析器已经
读到的锚点完全对得上**：`grok.rs` 的 `user_message_chunk` 分支本就在读
`update.pointer("/_meta/promptIndex")`（`grok.rs:723-726, 759-760`）用于同一 prompt 多 chunk
合并判断，只是没有把这个字段保留到输出的 `MessageTurn` 上（当前 `turn.id` 是跨 user+assistant
全轮次的位置索引 `grok-turn-{i}`，`grok.rs:1002-1004`，与"仅数 user 轮次"的 `promptIndex` 不是一回
事）——留存这个字段是低成本解析器改动，不需要重新摸索锚点从哪来。

**会话文件结构**（`grok.rs:134-157` 目录树注释，本节不重复姐妹篇已证实的 `~/.grok/sessions/`
总体事实）：单个会话是**多文件目录**——`summary.json`（元数据）、`updates.jsonl`（ACP
session/update 流，会话主体）、`chat_history.jsonl`（模型侧原始消息，含 ask_user_question 答案）、
`plan.json`、`terminal/<id>.log`、`subagents/<id>/`。若走文件手术（B 档），至少要同步截断
`updates.jsonl` 与 `chat_history.jsonl` 两个文件，且 `updates.jsonl` 里存在
`auto_compact_completed`/`compaction_checkpoint`（`grok.rs:864-873, 2054-2065`）等压缩记账事件——
这是"检查点"字样在 grok 协议里唯一的出现，但只标记压缩发生点，与会话回退无关，不能当锚点用。

**结论**：**A 档**——`/fork`（或 `--fork-session`）做整会话拷贝、`x.ai/rewind/execute` 对拷贝出的
新会话截断到目标 `promptIndex`，两者组合即 forkAtMessage，且与 codex 的
`thread/fork`+`thread/rollback` 模式（详见 §2 已有惯例）完全同构。但 `x.ai/rewind/*` 是否真的在
codeg 当前钉的 1.0.4（或应升级到的 1.0.5+）上可用，**未经我方直接实机验证**，唯一证据是今天
（2026-08-19）提交的第三方逆向工程代码——建议动作是先把 grok 依赖升到 1.0.5+，现场重新探测
`initialize` 响应和这两个方法，而不是照抄 grok-app 的方法名就当作定论。

## 2. Cursor 原生底座

**官方 CLI 无原生 fork——已被论坛确认，非推测**：`forum.cursor.com/t/fork-session-or-duplicate-
chat-in-cursor-cli/149498`（2026-01-21 发起）明确是功能请求而非既有功能，2026-04-19 的跟帖仍在说
"the CLI doesn't have the fork session concept like the UI does"，无官方回应、无实现迹象。

**checkpoint/rewind 存在但作用域是当前会话，不是 fork**：`docs.cursor.com` 的 checkpoints 文档页
返回 403（未查到细节），但论坛 bug 报告 `forum.cursor.com/t/rewind-doesnt-work-when-opening-old-
chats-in-cli/163445`（2026-06-16，员工 deanrie 确认"this is on our side"后修复关闭）证实：
`/rewind` 的检查点历史此前会在 `/resume` 旧会话后失效，现已修好——说明这是**原地**的、绑定单个会话
生命周期的撤销机制，不产生新会话 id，与"fork 出一个独立分支"是两回事。

**存储结构**（`cursor.rs:49-90` 注释，本节不重复姐妹篇已证实的双路径事实）：cursor-agent 把每个会话
存成 **SQLite blob 库**——ACP 会话在 `~/.cursor/acp-sessions/<uuid>/{meta.json, store.db}`；
`store.db` 是 `blobs(id, data)` + `meta(key, value)` 两表，内容是**内容寻址**的 protobuf
(`agent.v1.*`) DAG：根 blob `ConversationStateStructure` 的 `turns` 字段（field 8）是一个**扁平
的、无反向指针**的 blob-id 列表（`cursor.rs:786-793`），每个 turn blob 自身也不引用"上一条"——这
点和 claude 的 `parentUuid` 链、grok 的隐式顺序流都不同，结构上**理论上更适合截断**：只需另起一个
根 blob，`turns` 字段只保留前 N 个原 id（原 blob 内容不用改，天然不可变、可直接复用），配一条新
`meta` 行指向它、放进新 uuid 目录即可，不需要逐条改写内容。但要落地此思路需要**新写一个 protobuf
编码器**（codeg 现在 `cursor.rs:501-661` 只有 `wire::Fields` 解码器，无编码侧）；解码字段清单
（`decode_state`，`cursor.rs:786-838`：token_details/workspace_uris/turn_timings/
tracked_git_repo_branches/started_ts）里**没有任何 checkpoint 或 parent 链接字段**，即会话级
fork/rewind 的状态大概率不落在这个 DAG 里，是另一套機制（可能是论坛 bug 报告暗示的、绑定进程生命
周期的独立子系统）。codeg 自己最终展平后重编的 `cursor-turn-{i}`（`cursor.rs:1079-1081`）同样不是
原生锚点——真正对应 protobuf `turns[]` 位置的是 `build_turns` 循环变量 `i`（`cursor.rs:925`），
换算关系存在但目前也未保留到输出。

**ACP 面**：codeg 自己 `registry.rs:843-901` 的 Cursor `AcpAgentMeta` 块只有二进制分发/下载 URL
注释，**没有任何关于 `initialize` 广告能力的说明**（对比 grok 块 808-821 详细记录了能力对照）——
这本身就是空白，说明 codeg 团队从未记录过 cursor 的 ACP 能力挡位，需要专门实测补上。

**结论**：**C 档**——官方已确认无原生 fork，ACP 面无任何文档或实证，文件手术路线虽结构上可行但需要
从零写编码器、且无第三方项目验证过手术产物能否被正常 resume，风险定性为高、不建议第一版投入；B 档
留作后续研究方向（先盯 `--fork-session` 论坛请求是否被官方接下，更省成本）。

## 3. 三个参考仓怎么处理这两家

- **Monet**：完全没接。`engines/` 目录下只有 `claude/` 和 `codex/` 两个引擎实现（`core/` 是框架
  代码），grok、cursor 均不存在（Glob 扫描 `monet/src-tauri/src/engines/**` 确认）。
- **Paseo**：接了但深浅不一。grok 有 `grok-acp-agent.ts`（459 行），除模型/推理选项转换外，还专门
  读 `chat_history.jsonl`+`summary.json` 补充导入预览（`enrichGrokImportableSessions`，
  行 368-390），但**没有 fork/rewind 逻辑**；cursor 有 `cursor-acp-agent.ts`，只有 46 行，是全部
  provider 里最薄的——仅设置 client capability meta 和一个"fast"模式选项，零历史相关代码。
  `providers/**/rewind.ts` 只存在 claude/codex/opencode/pi 四家，**grok、cursor 均无对应文件**。
- **desktop-cc-gui**：`capability_matrix.rs:5-33` 的 `EngineType` 枚举有 8 家
  （claude/codex/gemini/grok/opencode/kimi/pi/dsh），**没有 cursor**；grok 虽在引擎列表里，
  但 `capability_state()`（`capability_matrix.rs:46-48`）把 `session.fork`/`session.tree` 等对**所有
  引擎**都硬编码返回字符串 `"unknown"`（不是逐引擎真实探测），真正决定 UI 能否用 rewind 的是
  `rewindSupportedThreadId.ts`（29 行）——只放行 `claude:`/`codex:` 前缀，`grok:`/`grok-pending-`
  被显式列进拒绝名单（第 14-15 行），cursor 因为压根不是一个引擎，连拒绝名单都不需要。

## 4. Monet ActionAvailability 结构（供 RFC 参考形状）

`monet/src-tauri/src/engines/core/capability.rs:184-216`：

```rust
pub struct ActionAvailability {
    pub available: bool,
    pub reason_code: Option<String>,
}
// available() / unavailable(reason_code) 两个构造函数

pub struct SessionActions {
    pub resume: ActionAvailability,
    pub fork: ActionAvailability,
    pub send: ActionAvailability,
    pub send_while_running: ActionAvailability,
    pub interrupt: ActionAvailability,
    pub open_cwd: ActionAvailability,
}
```

每个动作独立标 available/reason_code，而不是整个 engine 一个总开关——`SessionActions::read_only`
证明了这个形状的用法：只读引擎场景下五个动作全 unavailable（同一 reason_code），只有 `open_cwd`
单独 available。

## 5. Paseo claude/codex 的 rewind.ts 与 desktop-cc-gui 的降级链

- **`paseo/.../claude/rewind.ts`**（58 行）：锚点是 `messageId`（可选 `resolveMessageId` 回调做
  UI id → SDK id 转换，无降级链，查不到直接抛错）；机制是 Claude Agent SDK 原生两件套——
  `forkSession(sessionId, {upToMessageId})`（会话分叉）与 `query.rewindFiles(messageId, {dryRun})`
  （文件回退，`canRewind` 判定），两者独立调用、可分别使用。这条路是**走 Agent SDK 直连**，不经
  ACP，与 codeg 的纯 ACP 架构不同。
- **`paseo/.../codex/rewind.ts`**（88 行）：锚点是 `messageId` → 通过
  `CodexUserMessageTurnIndex.resolve()` 转成 turn 序号，`numTurns = 当前user轮次数 - 目标序号`；
  机制是 `thread/fork`（非破坏性，源线程留在磁盘可用 `codex resume <old-uuid>` 找回）→ 对**新
  fork 出的线程**执行 `thread/rollback{numTurns}`（原线程完全不动）；无降级链，`resolve()` 返回
  null 直接抛错——比 desktop-cc-gui 的实现脆弱得多。
- **desktop-cc-gui 的四级降级链**（`src-tauri/src/codex/rewind.rs:120-187`，本次现场重新读取确认
  行号）：`resolve_target_message_id` 依次尝试①精确 `requested_message_id` 匹配（129-139）
  ②规范化文本+第 N 次出现匹配（141-161）③尾部对齐——当运行时消息数多于本地记录数时，按两者差值
  对齐索引，吸收 compaction/隐藏消息造成的漂移（163-175）④裸序号兜底（177-179），全部失败才报
  `[FORK_TARGET_NOT_FOUND]`（180-186）。这条链比 Paseo 的单次 resolve 更适合 codeg 抄——codeg
  同样要面对"本地记录的消息数"与"agent 运行时真实消息数"不一致的问题。
