# TODO 作战板

> 这是领导会话的**待办清单**（没有 todo 工具，所以清单落在文件里，好处是跨会话不丢、
> 用户随时能翻）。规则：每次醒来先读这里，收尾最后一件事更新这里。
> 只记"现在"，做完的移进 DOGFOODING-LOG 台账，不在这里堆历史。
>
> 状态记号：`[ ]` 待办 / `[~]` 进行中 / `[x]` 本批已完成待归档 / `[?]` 等用户拍板

最后更新：2026-08-21 深夜（项目设置开关撤销 + 后台唤醒改判为显示层）

## ⚠️ 先读这条：清单会腐烂

2026-08-21 夜核查发现，`HANDOFF-2026-08-20-release8` 里的 P0/P1/P5/P6 **有一半早就做完了**，
文档没更新。差点为此派了三个工人重做已完成的活。

**规则：动手前先对着代码核一遍"这条真没做吗"，别信清单。** 核完顺手把状态改掉。
本批已核实的过期项见文末「已核实为过期」。

## ⚠️ 板子纪律（2026-08-21 用户当场点名）

用户原话：「你都没有遵循任务规范，你做的 board.md 一直没有更新了。」

**规则收紧：派活的那一刻就写进「进行中」，不要等回收再补。** 收尾时再把状态改掉。
只在脑子里记 = 等于没记，换会话就丢。

## 进行中（派出去了，等回收）

- [x] **后台任务唤醒缺口调研** —— 已交，见
      `BACKGROUND-WAKE-RECON-2026-08-21.zh-CN.md`。**结论跟原假设相反**
- [x] **遗漏任务盘点**（codex）—— 已交，结论见下

## 待办（用户已拍板，不需要再问）

- [ ] **后台任务唤醒：补显示层，不重写 adapter**（实证已完成，见
      `BACKGROUND-WAKE-RECON-2026-08-21`）。原假设「codeg 收不到官方 wake」**是错的**：

      1. 适配器 0.69.0 的 `AUTONOMOUS_RESULT_ORIGINS` 里就有 `task-notification`
         （`acp-agent.js:114`），唯一发送口 `sendUpdate`（`:1249`）**没有**
         「没活跃回合就不发」的守卫
      2. codeg 的 idle 循环一直在读，每条 notification 都过
         `emit_conversation_update`（`connection.rs:7857`）——跟回合内同一个函数

      **所以内容早就进转录了。缺的是回合语义**：没有 `TurnComplete` →
      系统通知不响（`acp-connections-context.tsx:3952`）、状态不翻「在跑」、
      不进任何调度器、没有后台任务列表。

      施工点就在 `connection.rs:7857` 那个 idle 分支（它按定义就是「没有活跃回合」）：
      只认 `agent_message_chunk` / `tool_call*` 这类真活动，发个新事件让 UI 翻状态，
      静默后走和 `TurnComplete` 同一条通知路径。**Grok 线要单独接**
      （`task_backgrounded` / `task_completed`，本轮未查）。

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

## 文档腐烂：审计结论（codex，2026-08-21）

**仓库里根本没有 `board.md`** —— 用户记忆里的 board 就是本文件。

**6 个会被误认为「当前真相」的状态源**：NOW-BOARD、DOGFOODING-LOG、
MAINTAINABILITY-PROGRAM、release8 handoff、fork-rewind-slices/README、
旧 ISSUE-TRACKER（近 6 天没动）。其中真正互相竞争的是前三个 + handoff。

已发现的实际冲突：release8 handoff 的 P0/P1/P5/P6 部分早已完成却仍写成未做；
NOW-BOARD 与它重复；MAINTAINABILITY 同时兼计划 / 决策 / 执行日志，三头重复。

- [ ] **收敛状态源**：NOW-BOARD 是当前状态的**唯一**事实源；handoff 用完即冻结归档；
      MAINTAINABILITY 只留自己的执行记录；ISSUE-TRACKER 标注「历史存档，勿当现状」。

### ⚠️ 唯一的高置信度遗漏

- [?] **自动导入从未见过的本地会话**。用户 2026-08-20 16:34 问过
      「要不要做自动导入会话。还是说不做。」—— **既没做、也没进任何看板、也没回答**。
      `ATRIUM-FEATURE-AUDIT:167` 记着「需要手动导入，自动监听仍缺」；
      旧 issue #458 只是「自动同步**已导入**的」，不是同一件事。
      等你拍：做还是明确不做。

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

- [x] **会话级开关「应用项目自带的 .claude 配置」→ 建了又撤了**
      （`7a86e751` → revert `cc2a5f58`）。教训见 LESSONS L14。
      一句话：`--settings` 是**逐键叠加**，档只压过它真正设了的键，其余仍归项目管——
      本来就是想要的语义，开关守的是一个不存在的问题。lane
      `cli-delegate-settings-overlay` 的折叠改动（`e8bdbd38`）**作废，不合并**
- [x] **催办 `newest_due_at` 补 `failed` 守卫**（`a1660e05` 里）——
      一封从没送到 agent 的失败信曾能把重复计数重置成 0、绕过上限一直催。
      测试双向验过（撤掉修复会红）
- [x] **后台唤醒实证**：`BACKGROUND-WAKE-RECON-2026-08-21.zh-CN.md`

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

## 2026-08-21 夜：settings 面板收敛 + 连接层三态

### 已落地

- [x] **连接状态是三态不是两态**（`361630c8`）。`--settings` 按键叠加，所以「删掉
      连接键」= 不表态、项目级接管；「写成 `""`」才是强制官方订阅。旧代码选
      「官方订阅」执行的是 delete，**产出第三态却贴第二态标签**——在有项目级网关的
      仓库里，用户选了官方订阅、保存，会话照样在计费网关上。三态：
      `custom`（有值）/ `official_subscription`（键在、值空）/ `inherit`（键不在）。
      加 `inherit` 是为了让老档读回来不变——否则任何"什么都没填"的老档一保存
      就会开始压项目配置
- [x] **档 = 它的 settings.json，没有别的存储**（`e64f0136`）。删掉 JSON 上面那排
      重复字段。**关键：字段不只是 UI**，它们喂着三个 record 字段，而
      `materialize_managed_profile` 的合并顺序是
      原始 JSON → `record.env` → 三个字段，**后者盖前者**。只删 UI 会让两层变成
      隐形却仍然生效。所以：打开时按同样顺序折进编辑器（**后层覆盖**，不是
      "缺失才补"——后者会让"只改名字"的保存把端点从 A 换成 B，codex 抓到的），
      保存时反推回去并发 `env: {}`。折叠→推导是构造上行为保持的，而且
      **只在用户按保存时才收敛**，不静默改写已存记录
- [x] **`stripCredentials` 对齐后端 `is_secret_env_key`**（TOKEN|KEY|SECRET），
      复制档不会给别家厂商的密钥留一串圆点
- [x] 十语提示文案改正（原文还在说"上面的 Base URL、密钥和模型会在保存时写进它"，
      那些字段已经不存在了）
- [x] **「跟随默认」页签也只剩一个编辑器**（`bf9e0b4b`，合入 `91cdb60c`）。上一批
      只收敛了档页签，隔壁那页还留着整套表单 + 底下一个编辑同一个文件的 JSON 框。
      同样处理。连带删掉只为喂那套表单存在的东西：`ClaudeConfigFields`、
      `applyClaudeProviderToConfigText`、`configTextForClaudeSave`、
      `setClaudeEnvFlagInConfigText`、`materializeClaudeHardeningFlags`、
      `readClaudeConfig`、`applyClaudeConfig`。**净 −1531 行**，加的只有 23 行。
      门禁：tsc / eslint / 343 前端测试 / cargo clippy 全绿，
      新增 Rust 回归测试 `folded_legacy_env_round_trip_preserves_secret_and_ordinary_values`
      逐条核过（不是看退出码——那次退出码取的是 `tail` 的，无效）

### 今晚实测钉死的事实（都进了 CONFIG-MODEL §11，动工前先读）

- **空串能压掉下层，且被 CLI 读成"未设置"**：项目层指向本地 4711，叠加层不碰
  → 命中 7 次；叠加层写 `""` → 命中 0 次且 CLI 正常作答。**双向对照**
- **项目级 settings.json 赢过进程环境变量**：env 指向 4712、项目层指向 4711，
  七次真实调用全打 4711。**后果：`apply_claude_env_policy`（`connection.rs:162`）
  和虚拟档 `OfficialDirect`（`claude_profile.rs:734`）都靠改进程环境来"强制官方
  订阅"，它们都压不住项目配置**——两条独立路径都是坏的，未修
- **`ANTHROPIC_API_KEY` 与 `ANTHROPIC_AUTH_TOKEN` 是两套 HTTP 认证且互斥**
  （从 claude.exe 里挖出来的：`withOptions({apiKey: null, authToken: t})`）。
  前者发 `x-api-key`，后者发 `Authorization: Bearer`。第三方中转要的是后者，
  所以"一般不设 API_KEY"是对的，不是省略是不需要

### 等签字（改已存数据，不擅自做）

- **一次性迁移收掉 record.env 和三个字段**（codex 推荐）：在 `db/mod.rs:113-119`
  的启动迁移点，按现有优先级把 env + 三字段压平进 `settingsJson` 并清空，之后
  `materialize_managed_profile` 只写 `settingsJson`，前端的折叠/推导全部可删。
  长期最干净、代码最少。代价：改写已存的档记录文件

### 已知仍坏，未修

- **「跟随默认」页签在架构上无法强制官方订阅**：它的两个存储是
  `configText`→`~/.claude/settings.json`（用户层，**最低**）和
  `envText`→进程环境（输给 settings 文件）。都够不着项目级之上。
  可能的修法：给它也发一份只含三个空连接键的 `--settings` 叠加
- **虚拟档 `OfficialDirect` 同上**：只塞进程环境，被项目配置击穿
- **session-sync 空转**：`pnpm tauri dev` 日志里 reconciliation 每 ~10 秒一次、
  每次都报 `updated=1~2`。空闲状态不该一直有更新，要么真在无谓写库、要么计数是假的
- **claude_code 的 model_provider 绑定成了孤儿**（`bf9e0b4b` 带出来的）。被删的那处
  `<ClaudeConfigFields>` 是传了 `providers=` 的，所以那页**曾经**能把 Claude 绑到一个
  model provider 上。删掉之后：没有界面能看到或解绑，但
  `inferClaudeAuthMode`（`acp-agent-settings.tsx:841`）仍会因 `model_provider_id != null`
  判成 `model_provider`，`apply_model_provider_env`（`acp.rs:8865`）仍会在启动时把
  provider 的 `api_url`/`api_key` 注进运行时环境——**设得上、取不下的隐形配置源**。
  本机实测不可达：`agent_setting` 全部 14 行 `model_provider_id` 都是 NULL，
  `model_provider` 表空，所以**不是合并阻断项**。要么把 claude 那条分支一并删掉，
  要么给它留一个能解绑的控件。另：provider 走的是进程环境，本来就输给项目配置
- **加固开关不再强写**（同上批的行为改变，不是纯死代码清理）。
  `materializeClaudeHardeningFlags` 的注释写着"不管用户有没有碰开关"，
  每次保存都会把两个 `CLAUDE_CODE_*` 键写进用户自己的 `~/.claude/settings.json`。
  现在不写了。本机无影响（这两个键早就在 agent 的 env 行里），但要记着
