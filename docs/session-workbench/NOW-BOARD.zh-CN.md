# TODO 作战板

> 这是领导会话的**待办清单**（没有 todo 工具，所以清单落在文件里，好处是跨会话不丢、
> 用户随时能翻）。规则：每次醒来先读这里，收尾最后一件事更新这里。
> 只记"现在"，做完的移进 DOGFOODING-LOG 台账，不在这里堆历史。
>
> 状态记号：`[ ]` 待办 / `[~]` 进行中 / `[x]` 本批已完成待归档 / `[?]` 等用户拍板

最后更新：2026-08-21 深夜（配置面板统一 + 催办失效根因定位）

## ⚠️ 先读这条：清单会腐烂

2026-08-21 夜核查发现，`HANDOFF-2026-08-20-release8` 里的 P0/P1/P5/P6 **有一半早就做完了**，
文档没更新。差点为此派了三个工人重做已完成的活。

**规则：动手前先对着代码核一遍"这条真没做吗"，别信清单。** 核完顺手把状态改掉。
本批已核实的过期项见文末「已核实为过期」。

## 进行中（派出去了，等回收）

- [~] **会话级开关「应用项目自带的 .claude 配置」**（grok，lane `settings-overlay`）——
      机制已实测（见下），后端 + 前端 + 十语一起做

## 待办（用户已拍板，不需要再问）

- [ ] **换档/换配置后自动重启 ACP**（2026-08-21 夜用户拍板"这个要做"）。
      现状是标 stale + 挂横幅，用户得自己去点，而且横幅能 dismiss——
      dismiss 之后就跑在一个"以为换了其实没换"的会话上，这才是真 bug。
      按下表做：

      | 谁在切 | 会话状态 | 行为 |
      | --- | --- | --- |
      | 人 | 空闲 | 直接重启，**不弹框**（没有东西会丢，弹框是纯噪音） |
      | 人 | 正在跑一轮 / 队列有货 | **弹框**，写清丢什么（中断这一轮、队列还有 N 条） |
      | agent（MCP） | 任何 | **永不弹框**，不隐式重启，标下次生效并把这句话作为工具返回值告诉它 |

      判定条件是现成的：连接的 `turn_in_flight` + 队列待办数。
      涉及 `acp/manager.rs`（`mark_conversation_config_stale`）和
      `src/components/chat/session-config-stale-banner.tsx`。

- [ ] **把锁死的会话在全局露出来**。今晚查出三个会话的队列被会话级锁死 1–3 天
      （`paused_reason`），但这个状态**只在会话内部**的输入框上方显示
      （`message-queue-display.tsx`），不点进去根本不知道。
      这就是"催办为什么没响"用户查不出来的直接原因。
      需要一个全局提示（侧栏徽章 / 会话中心一列）。

- [ ] **只读的「有效配置」视图**。用户看不到某个键最终是谁赢的（§7 的五层）。
      **只读**——只读就不可能造出第二个写入口。显示合并结果 + 每个键来自哪层 +
      "在编辑器里打开项目那份"的链接。设计见 `CONFIG-MODEL-2026-08-21` §7。

- [ ] **冷启动白屏**。dev 冷编译时 Tauri 窗口先弹出来、前端还在 turbopack 编译
      （实测 compile 13.5s → 白屏 16 秒），用户以为坏了。
      **注意**：改窗口背景色**没用**，用户是浅色主题，主题背景本来就是白的。
      真修法是 splash 窗口或 Rust 侧 `navigate`，都是中等风险改动。生产不受影响
      （静态文件秒开），所以别为它冒险，挑白天做。

- [ ] **`model_provider` 回写清理**。绑定仍会把连接键写回 `agent_setting.env_json`
      （O69-A 报告 §8 提出，O69-C §5 明确没做）。O69-C 之后档已不再注入连接 env，
      所以污染只影响 `follow-default`——而那层正是配置模型里说要删掉的。
      顺带考虑把「模型供应商」从第三种认证方式降级成"往 URL/Key 里填值"的快捷方式。

- [ ] **P2 搜索体系**：`SEARCH-SYSTEM-SURVEY-2026-08-20` 已存在，
      但 handoff 里记着"用户四个子问题待答"。**先读那份调研，确认还剩哪几问没答**，
      别重新调研一遍。

- [ ] **催办残留问题**（工人在 `REMINDER-PAUSE-REPORT` §4 提问，等我拍板）：
      `newest_due_at` 的未读分支仍未排除 `failed`。若会话同时有 failed 未读 + 有效欠账，
      到期时刻可能被 failed 那封拉动。要不要和 SUM 对齐。

- [ ] **`store_only` 广播永不催办**——今晚查明这是**设计行为**不是 bug
      （群发没 @提及 → Normal 优先级 → StoreOnly → `state='pending'` →
      永远不满足 sweep 的 `invocation_policy='invoke_when_idle'` 前提）。
      但 dev 库里有 08-18 的这种投递至今 `attention_state='unread'`。
      要判断的是：UI 上把它显示成"未读"合不合适，还是该有别的呈现。

## 等用户拍板（不许自己动）

- [?] **GitHub**：fork（必须公开）还是自建私库？现在 origin 是上游主仓，一次没推过
- [?] **O47 建议 C**：后端重启后滞留的队列催办要不要禁止自动重放
- [?] **schema 手术批**：human 一等公民化等（模型体检 RFC ②③①）
- [?] **看板 RFC A–F**：一期已按建议值施工，其余等确认
- [?] **P3 待回复面板定位**：侧栏独立行撤掉 → 会话中心入口的琥珀徽章（方向认可，未最终拍板）
- [?] **P4 会话中心过滤拥挤**：要先出设计稿再实施
- [?] **旧 worktree 处置**：sessionmux-session-timer、agent-a31e34…
- [?] **产品改名**
- [?] **`set_profile` MCP**：我判断不值得做（档在启动时生效，中途换只标 stale；
      让 agent 改**别人**会话的档是脚枪）。真正有价值的那一半（新建会话选档）
      `work_task` / `automation` 的 `profile` 字段已经有了。等你否决或确认。

## 本批已完成（下次收尾时归档进台账）

- [x] **O69-C** 档改用 `--settings` 叠加生效，不再换配置目录。门禁绿：**2764 passed / 0 failed**
- [x] **O70 配置面板统一**：「跟随默认」和每个档现在渲染**同一个** `ClaudeConfigFields`，
      档因此拿到认证方式 + 五个模型字段 + effort。档的新字段投影进它自己的
      settings.json（`claude-settings-projection.ts`），**零 schema 改动**。
      前端全套绿：**4968 passed / 0 failed**
- [x] **层级文案说反了**：面板称全局 settings.json「对所有跟随默认的会话生效」，
      实际它是**最低**一层，项目的 `.claude/settings.json` 压过它。十语改正
- [x] **eslint 全仓 EXIT:0**：586 个报错几乎全是嵌套 worktree 被扫进去
      （`.cli-delegate/**` 加进 ignore），外加让 eslint 和 tsconfig 在 `_` 前缀上一致
- [x] **催办空转修复**：会话队列被锁时不再往里塞催办；`overdue_unread` 补上
      `state <> 'failed'` 与另两处对齐
- [x] **`has_unread` 注释与实现不符**已改注释（实现改动会牵动
      `auto_reply_for_completed_turn`，另案）
- [x] **worktree 政策修订**：常驻车道（`ui` / `backend`）留着复用省构建冷启，
      只删一次性树。L4 已同步改正
- [x] **磁盘卫生**：board-ux / profile-conn / profile-own 三棵已合并的树删掉

## 今晚查清的机制（写进 CONFIG-MODEL，动工前先读）

- **`settingSources` 能从 `_meta` 覆盖**（**实测双向正向信号**）：不传 → 项目层加载（401）；
  传 `["user"]` → 跳过（pong）。不需要改适配器。适配器入口是
  **`dist/index.js`** 不是 `dist/acp-agent.js`（踩过坑）
- **适配器原生有 `providers/list|set|disable`**（PR #1002）。但我们装的 0.69.0
  **没有**那个 PR 的改进（`clearAuth`/`restoreAuth`/`apiKeyHelper` 符号全无）。
  它只带 `apiType`/`baseUrl`/`headers`，和 `--settings` 是互补不是替代
- **effort / 模型是热的**，`applyFlagSettings` / `session/set_model` 不用重启；
  **env 块要重启**（启动时导出进进程环境）。档的实质大半在 env 块，所以换档要重启
- **适配器宣告 `sessionCapabilities: { close, delete, fork, list, resume }`** ——
  **`fork` 是原生的**，P7 的 rewind/fork 课题动工前先读这条
- **档记录是 JSON 文件**不是数据库表（`claude_profile.rs:433`），加字段零迁移
- **会话级配置存 `conversation.preferred_config_values`**（`__codeg_profile__` 就在里面），
  加键零 schema 改动

## 已核实为过期（HANDOFF-2026-08-20-release8 的条目，别再做）

- **P0 IME bug #518** —— 已修。commit `0c85ff8f`，测试
  `ime-suggestion-gate.test.ts:164`「keeps one panel alive across the @美 → @美术 sequence (#518)」
- **P1 @ 搜索缓存** —— 已做。`use-reference-search.ts` 懒取 + ref 键缓存 + 窗口聚焦失效
- **P5a Room 欠回复聚合** —— 已做，而且比 handoff 描述的做法好：
  `total_room_needs_reply_count` 独立字段，`sidebar.tsx:262` 相加。
  handoff 说的「Room 侧 host 聚合硬编码 0」也过期了，`HOST_NEEDS_REPLY_SQL` 是真查询。
  ⚠️ 按 handoff 原文去做会**双重计数**（我派的工人正是这么做的，已回滚）
- **P5b `visibleCollectionSessionIds` 死代码** —— 已删，全仓零命中
- **P6a eslint/prettier 扫尾** —— 本批完成
- **P6b dev 任务栏角标** —— 已做，`lib.rs:905` `set_icon(icon-dev.png)`
