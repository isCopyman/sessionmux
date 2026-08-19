# 搜索体系四问调研报告（2026-08-20 夜，r-search 只读调研）

> 回答用户 2026-08-20 早的四个问题（HANDOFF-2026-08-20-release8 §P2）。
> 所有断言带 file:line 证据；本机没装 `ctx`，涉及 ctx 内部实现的结论标注"未验证"。

## 核心发现（一句话版）

1. **全局搜索（Ctrl+K）和会话中心的会话搜索底层是同一套接口**（标题 LIKE +
   正文外包 ctx），Ctrl+K 里的 agent 筛选和正文搜索是在复刻会话中心的活。
2. **正文搜索在这台机器上是坏的**：codeg 自己不存消息正文、不建全文索引，正文
   搜索整个 shell 出去调外部 `ctx` CLI（`which ctx` 找不到 → 界面显示"暂时无法
   搜索会话正文"）。
3. **Room 在整个 App 里哪儿都搜不到**，Rooms 页连筛选框都没有。
4. **subagent 消息和 tool 调用内容 codeg 的搜索完全覆盖不到**（结构性的，不是
   bug）。

## 1. 全局搜索现在到底搜什么

入口 Ctrl+K，组件 `src/components/conversations/search-command-dialog.tsx`，由
`workspace-chrome-controller.tsx:20,182` 常驻挂载；侧栏搜索按钮共享同一 open 状态
（`search-dialog-context.tsx`）。

两个 tab（`search-command-dialog.tsx:38,263-292`）：

- **会话 tab**：并行两路（`:115-159`）——`listAllConversations`（标题匹配）+
  `searchSessionContent`（正文匹配，≥2 字符触发，带 snippet）。另有一排 agent
  筛选 chip（`:306-335`），与会话中心筛选能力重复。
- **文件 tab**：`useFileTree` + `rankFileMatches`（`src/lib/file-search-match.ts`），
  纯文件名/路径模糊匹配（`FileSearchCandidate` 只有 lowerName/lowerPath），
  **不搜文件内容**。

点会话结果直接 `openTab()` 进工作台（`:187-197`），绕过会话中心。

**建议（领导采纳，见文末决策）**：会话不从 Ctrl+K 撤掉——Ctrl+K 的心智模型是
"直接跳到任何东西"（VSCode/Slack/Linear 同款），撤掉反而打破直觉。该改的是
**去重**：Ctrl+K 会话 tab 退回"轻量按标题跳转"（去掉 agent 筛选 chip、去掉正文
搜索分支），深度搜索引导去会话中心。两边各司其职。

## 2. 会话中心能不能搜 Room

**完全不能，且 Room 全 App 无处可搜（Rooms 页连筛选框都没有）。**

- 会话中心取数只走 `listAllConversations`/`searchSessionContent`，都查
  `conversation::Entity`；`ConversationKind`（`db/entities/conversation.rs:38-47`）
  只有 Regular/Chat/Loop/Delegate，**没有 Room**。
- Room 是独立模型（`models/collaboration.rs:546`）+ 独立表（collaboration_room
  等），结构性不在 conversation 表。
- `src/components/rooms/rooms-page.tsx` 全文无 search/filter 输入。

**方案（领导采纳）**：Room 混进会话中心搜索结果，类型徽标区分（会话 vs 群聊）。
改动面：room service 加按标题过滤的 list 函数、commands/web handler 暴露、
api.ts 封装、conversation-manage-dialog.tsx 合并 rows（需 kind 判别；批量归档/
加入工作台等操作对 Room 不适用要禁用；点 Room 结果跳 Room 而非 openTab）。
改动量不小，不是加字段就完事。

## 3. 会话中心搜索的实现机制

前端 300ms 防抖（`conversation-manage-dialog.tsx:848-955`），并行两路：

- **标题** → `conversation_service::list_all`（`conversation_service.rs:1098-1177`），
  :1154-1158 `Title.contains(s)` = SQL `LIKE '%s%'`。**非 FTS5**，只查标题列，
  无 Title 索引（前导通配符也用不上），量大线性扫描——几百到低千级会话没问题，
  往上会慢。
- **正文** → `session_search::search_session_content_core`
  （`commands/session_search.rs:137-232`）：`which::which("ctx")` 找外部二进制
  （:130-135），装了就跑 `ctx search <query> --limit 200 --backend lexical
  --refresh off --json --quiet`（15s 超时），拿 `(agent_type, external_id)` 回
  join codeg 的 list_all 结果；没装返回 `available:false` → 界面
  `contentSearchUnavailable`。**本机实测未装。**
- codeg 的 SQLite **不存消息正文**：预览面板也是每次 `spawn_blocking` 现场重新
  解析磁盘会话文件（`commands/conversations.rs:1355`）。架构 = "元数据进 SQLite，
  正文搜索外包 ctx，自己无 FTS 层"。

## 4. 与 deja / ctx 的定位差异 + subagent/tool 可见性

- **deja**（`deja --help` 实测）：跨 17 家 harness 的"coding agent 持久记忆"，
  语义+词法双通道，`--role user|assistant|tool|files|command|edit`（tool 输出是
  一等公民），有 hook-prompt 主动召回。独立于任何 UI 的跨项目记忆系统。
- **ctx**（本机未装，按调用参数反查 = ctxrs/ctx，"Git blame, but for agent
  sessions"）：把 ~/.claude、~/.codex 等目录的会话解析成自己的 sessions/messages/
  tool calls 记录。**与 codeg 自家 parser 高度重叠**；codeg 把"搜索"这步整个甩给
  它。（基于项目自述+参数匹配推断，未本机验证。）
- **subagent 可见性**：
  - Codex subagent：独立 conversation 行但标 `harness_internal: true`
    （`parsers/codex.rs:120,339`），`list_all` 无条件过滤（`conversation_service.rs:1120`），
    连 ctx 命中 join 都会被丢（`session_search.rs:221`，有单测断言必须隐藏）。
  - Claude Code subagent：sidechain 只被用来累加 token 用量到父 turn
    （`claude.rs:2265`），内容根本不进 `turns`——UI 层面不存在。
  - 普通 tool 调用：磁盘有、渲染有，但搜索不碰（标题只查 Title 列，正文外包）。
  - Room 消息：独立表，两条搜索路径都不碰。

## 对照表：什么搜索找什么

| | codeg 全局搜索(Ctrl+K) | codeg 会话中心 | deja | ctx |
|---|---|---|---|---|
| 会话标题 | 能（Title LIKE） | 能（同一接口） | 能（内容检索间接命中） | 能（自述，未验证） |
| 用户消息 | 依赖外部 ctx；本机未装=不能 | 同左 | 能（--role user） | 能（自述，未验证） |
| 助手消息 | 同上 | 同左 | 能（--role assistant） | 能（自述，未验证） |
| subagent 消息 | 不能（Claude:UI 不存在；Codex:被过滤） | 不能 | 不确定 | 不确定 |
| tool 调用 | 不能 | 不能 | 能（--role tool） | 可能能（自述，未验证） |
| 文件内容 | 不能（文件 tab 只搜名/路径） | 无此功能 | 部分（查会话 touch 过哪些文件） | 不能 |
| Room 消息 | 不能 | 不能 | 不能 | 不能 |

## 领导决策（2026-08-20 夜，用户授权"细节和模糊意图领导决断"）

1. **Room 进会话中心搜索**：实施（用户倾向明确）。等 w-filter（过滤减负）合并后
   派工，避免同文件冲突。
2. **Ctrl+K 去重**：实施轻量版——会话 tab 退回纯标题跳转（去 agent chip、去正文
   分支），加"去会话中心深度搜索"引导入口。可逆，用户不满意一个 revert 就回来。
3. **自建 FTS 替代 ctx 外包**：**不做**，架构级决策留给用户拍板。但现状"正文搜索
   依赖未安装的外部工具、坏了只有一行小字提示"要在给用户的报告里点名。
4. subagent/tool 搜索覆盖：现状按设计（隐藏 harness 内部会话），不改；与 deja 的
   分工清晰（深挖历史用 deja），不重复建设。
