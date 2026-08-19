# Room/Session 附加路径与 @ 补全扫描超时设计侦察

> 状态：2026-08-20 只读调研。codeg 源码只读（工作分支 `codex/session-message-v1`，隔离
> worktree `wt/at-completion-recon`）+ `claude-code`/`codex`/`codex-current`/`opencode`/`paseo`
> 五个参考仓只读，无联网查证。
> 立项记录：`MAINTAINABILITY-PROGRAM-2026-08-19.zh-CN.md:664-670`（用户 2026-08-19 深夜提出，
> 排在 room 文件 @ 修复批之后、与 fork/rewind RFC 同批评审；同文件 744-761 记录了本次要
> 扩展的护栏当初为何存在的真实故障，本报告第 1.2/3.6 节会引用）。

**结论先行**：我方 @ 面板今天已经有一套能撑住"多来源合并"的架构——`buildReferenceGroups`
把文件/智能体/会话/提交四组结果拼成固定顺序的分组，room 再在此基础上叠一层"3 秒超时降级"
——缺的只是"单 root"这一个假设：`useFileTree`、`list_workspace_files`、
`CollaborationRoomDetail` 从上到下都只认一条路径。三个参考仓里最值得抄的不是某个花哨算法，
而是 Codex file-search 一个朴素的架构选择：**一次 `WalkBuilder` 吃下所有 root，统一预算、
统一去抖、统一取消**，而不是"每个 root 各跑一份、结果拼起来"；反而 Claude Code/opencode
引以为豪的高级机制（bitmap 前缀索引、fff 原生模糊引擎、frecency）在我方 50k 条/root 的量级
下都是屠龙之技，不建议这批引进。全局目录（`~/.claude`、`~/.codex` 之类）**不建议自动纳入**
@ 范围——这不是性能问题而是"room 是多 agent 共享的"这个我方特有的隐私语境所致；建议让它们
和普通目录一样，走用户手动"添加路径"这一个入口，不做特殊化。批次排序建议：**Room 的附加
路径数据模型/命令 + 前端多 root 改造（含 10 秒后端 deadline）先做一批**，"…"菜单 UI 细节和
Session 侧的对等能力排第二批，frecency/预扫缓存/取消令牌等算法级优化本批都不做。

## 0. 侦察方法与置信度分级

本报告 codeg 现状部分先由一个只读子代理跑通全量扫描（原计划用 K3，当时 K3 子代理额度/访问
报错，实际用 Sonnet 5 顶替执行），再由我本人对会直接影响设计决策的文件逐一重新 `Read`/`Grep`
核实：护栏函数（`folders.rs` 的 `list_workspace_files`/`walk_workspace_files`/
`get_file_tree`）、room mention 的 3 秒降级全文件、room/conversation 的 SeaORM 实体与三份
迁移文件、skill 目录规格表全部 match 分支、DTO 结构、既有命令命名惯例、`list_all_conversations`
的无上限查询、`git_log` 的默认值。凡下文标"本人核实"的引用都对应本次会话里真实执行过的
`Read`/`Grep`/`Bash` 调用；其余（尤其三个参考仓的细节）来自子代理报告，其严谨程度体现在
报告里大量出现的"NOT FOUND"标注和自我保留（例如 codex-current 的 sparse checkout 缺哪些
文件、只能靠 `git show` 单独取）。

**仓库状态是移动靶，不是快照**：本次调查期间 `codeg` 主仓 HEAD 至少变化四次——会话开场
快照 `a2fef815`（**已确认过期，不代表任何本报告引用的当前状态**）→ 建 worktree 时
`850b4e5c` → codeg 现状子代理交稿时 `3551283289a6649b2e6b37d2fd14414091eb5059` → 本人最后
一次核对时 `9bc1398b341655f1e6d5cc360926baf317c3fc7c`（`docs: log stuck-responding batch
closeout and analysis correction`）。期间 `conversation_service.rs::list_all` 的行号已经
从子代理报告的 666-745 漂移到本人核实时的 816-895——同一份代码，十几分钟内行号漂移
150 行，直接证明有其他工人在并行改同一批文件（团队名单里能看到 `w-file-scan-guard`、
`w-room-file-mention` 两个高度相关的名字，见第 4 节风险提示）。**下文所有 codeg 行号仅在
最后核对点（`9bc1398b`）成立**，落地前必须重新 grep 确认，不能假设行号仍精确对应；本报告
的价值在于"机制存在、大致在哪、怎么改"，不是行号快照。

**参考仓可信度不是均质的**，分三档看待：
- `claude-code`：仓库自己的 `CLAUDE_README.md:1-11` 声明这是"从
  `@anthropic-ai/claude-code@2.1.88` 的 sourcemap 反推、经 AI 做结构补全"的非官方复原，不是
  Anthropic 官方源码；`dist/cli.js` 真实构建产物在本仓不存在（`package.json:19` 的 build
  脚本指向它，但 Glob `dist/**` 零结果）。宏观架构结论（"@ 会并入 `~/.claude` 下的 markdown
  配置目录""skills 和 agents 的覆盖优先级相反"）可信度较高——这类结论依赖控制流形状和标识符
  含义，反推/补全大概率保真；但任何精确数值常量（`MAX_SUGGESTIONS = 15`、各种超时的具体
  毫秒数）只作为"业内合理值的参考"，不要当成必须对齐 Anthropic 官方实现的规范。
- `codex-current`：是 `blob:none` 稀疏克隆，只覆盖
  `codex-rs/{app-server,app-server-client,app-server-protocol,core}`，`tui/`、`file-search/`
  等目录在磁盘上不存在，子代理靠 `git show HEAD:<path>` 单独拉取验证，报告里逐条标注了
  "这条是拉取验证过的"还是"结构上应该没变但没逐行复核"——采信时按报告原文标注区分，不要
  笼统同等确信度。本人抽验了其中最关键的一条（`require_git(true)` 的设计注释）与 `codex`
  仓完全一致，见第 2.2 节。
- `paseo`：本人直接关键词检索（`additionalDirect*`、`mention`），覆盖了
  `packages/{server,app,cli,protocol,desktop}/src` 五个源码目录，不是逐文件通读全仓（这是
  一个相当大的多端 monorepo：app/desktop/server/cli/protocol/relay/website 七个包）。结论
  "没找到多 root @ 补全"建立在关键词命中为零之上，不是穷举读过全部源码后的否定。

## 1. 我方现状图

### 1.1 @ 面板四类来源

| 来源 | 前端入口 | 后端命令 | 实际扫哪里 | 现有防护 |
|---|---|---|---|---|
| 文件 | `useFileTree`（`src/hooks/use-file-tree.ts:42-98`，本人核实全文件）预热，`useReferenceSearch` 消费（`src/components/chat/composer/use-reference-search.ts:226-229`） | Tauri `list_workspace_files` / HTTP `POST /api/list_workspace_files`（`web/handlers/folders.rs:282-287`，本人核实两侧签名一致，均只接受单个 `path: String`） | `src-tauri/src/commands/folders.rs:4687-4804`：`ignore::WalkBuilder` 单 root 遍历（本人核实全段） | `MAX_WORKSPACE_FILE_ENTRIES=50_000` 共享预算（4380 行）+ `spawn_blocking`（4691 行）+ 静默截断（返回类型无 `truncated` 字段，本人核实） |
| 智能体 | `useAcpAgents`（`src/hooks/use-acp-agents.ts:200-206`） | Tauri `acp_list_agents` | 内存里的 `builtin_acp_agents()` + `custom_registry` 的 `RwLock` map，非磁盘扫描 | 天然有界（注册的 agent 数量），不需要额外护栏 |
| 会话 | `useReferenceSearch` 内联懒加载（`use-reference-search.ts:296-309`），窗口聚焦时清缓存重取 | Tauri `list_all_conversations` | `conversation_service::list_all`（`db/service/conversation_service.rs:816-895`，本人核实到第 891 行 `let rows = query.all(conn).await?;`——无 `.paginate()`/`.limit()`） | **无**——一次性取回全部匹配行 |
| 提交 | `useReferenceSearch` 内联懒加载，`GIT_LOG_LIMIT=100`（`use-reference-search.ts:37`） | Tauri `git_log` | `folders.rs:5530-5625`：拼参数调用 `git log -N` 子进程（本人核实第 5556 行 `let limit_str = format!("-{}", limit.unwrap_or(100));`） | 客户端约定传 100，但**服务端不夹紧 `limit` 参数本身**（调用方传更大的值会直接透传给 `git log`），子进程调用也无超时包裹 |

前端渲染层再统一加一道 `MAX_PER_GROUP = 50`（`use-reference-search.ts:35`，本人核实），在
`buildReferenceGroups`（92-183 行）里对四组分别做"够 50 条就截断+置 `truncated`"（文件/
提交是边扫边数的 break 循环，104-116/142-155 行；智能体/会话是先过滤再 `.slice(0,
MAX_PER_GROUP)`，121-125/132-140 行）。`suggestion-popup.tsx` 只读这个 `truncated` 布尔值
渲染"还有更多"提示（311、456-466 行，子代理报告引用，未本人复核），不再对渲染做二次截断。

### 1.2 Room 专属 @ 面板与 3 秒降级（本人核实 `room-mention-search.ts` 全文件）

Room 的 @ 面板不是直接用 1.1 的 `useReferenceSearch`，而是包一层 `buildRoomMentionSearch`
（151-176 行）：成员名单组（`buildRoomSessionGroup`，52-105 行，@all/@human 结构化伪提及 +
每个成员，`rankByTextMatch` 排序，自身不设数量上限）**永远先行且永不因后面的失败而消失**；
文件/提交组来自 1.1 的 `workspaceReferenceSearch`（root 取 `detail.rootFolderId` 解析出的
路径——`rooms-page.tsx:427-431`，本人核实，代码原有注释明确写"resolved from `rootFolderId`,
never the currently-active folder — a room is a cross-folder concept"），套一层

```ts
searchGroups = await Promise.race([
  referenceSearch(query, signal),
  new Promise<SuggestionGroup[]>((resolve) => {
    setTimeout(() => resolve([]), REFERENCE_SEARCH_TIMEOUT_MS)  // = 3000
  }),
])
```

（137、165-170 行）。**这个 race 从不取消底层调用**——代码注释原话是"The race below never
cancels the underlying call; a result that arrives after the deadline just resolves into
nothing, unused"（133-135 行）。也就是说，真正堵住的后端线程/子进程不会因为前端超时而被
释放，只是 UI 不再等它——用户看到的"卡 3 秒后至少能唤人"，后端可能仍在原地空转。这正是
第 3.6 节要补的洞：一个只在前端生效的超时，治不好"后端资源被占住"的病根，只是让用户看不到
症状。

这套降级机制不是纸上谈兵：`MAINTAINABILITY-PROGRAM-2026-08-19.zh-CN.md:744-750` 记录了
它要防的真实故障——一个 agent 创建的 room 绑定到了 folder id=1（一个叫 Thesis 的巨型目录，
`kind=regular`，没有 `.gitignore` 可剪枝），`list_workspace_files` 在 async 运行时线程上
直接跑同步 `ignore::WalkBuilder` 遍历且当时无条目上限，遍历堵死了整个 runtime，后续所有
`invoke` 排队，@ 弹层永久卡在"搜索中…"，连本该同步返回的成员组都被 `Promise.all`/`await`
拖累一起卡住。`a6f968e2`（工人 `426b7430`）合入了 `spawn_blocking` + `MAX_WORKSPACE_FILE_
ENTRIES=50_000` 修了这个根因；`room-mention-search.ts` 的 3 秒 race 是同一批里给 UI 加的
兜底。**本报告要扩展的"附加路径"，本质上是在同一个后端命令上多开几个可能同样巨大/同样慢
的 root**——这也是第 3.6 节坚持"后端也要有硬 deadline，不能只靠前端 race"的直接动机。

### 1.3 $ Skill 面板（本人核实 `SkillStorageSpec` 结构体与全部 match 分支、`custom_agent` 实体）

`$` 触发目前**仅 Codex 智能体启用**（`src/components/chat/message-input.tsx:316-326`，
`const skillAgentType = agentType === "codex" ? "codex" : null`；其余智能体走 ACP 自带的
`availableCommands`，不需要磁盘扫描）。目录解析靠 `skill_storage_spec(agent_type)`
（`src-tauri/src/commands/acp.rs:7710`，结构体 `SkillStorageSpec { kind, global_dirs:
Vec<PathBuf>, project_rel_dirs: Vec<&'static str> }` 定义于 2629-2634 行）——**一个按
`AgentType` 硬编码的 Rust `match`**，每个分支自带一份 `global_dirs`（多数在
`home_dir_or_default()` 下，部分可被该 agent 自己的环境变量覆盖，如 Codex 的 `CODEX_HOME`、
Hermes 的 `HERMES_HOME`）和 `project_rel_dirs`（相对工作区根的相对路径）。本人逐条核实了
ClaudeCode（`~/.claude/skills`，项目 `.claude/skills`）、Codex（`~/.codex/skills` [+
`.system` 子目录]、`~/.agents/skills`；项目 `.codex/skills`、`.agents/skills`）等九个
agent 分支（7710-7799 行）。`acp_list_agent_skills`（11989-12039 行）对两类目录分别探测
存在性并列出技能文件。

**这已经是我方代码库里现成的"全局目录 + 项目目录合并"范式**，只是入口条件是"你在用哪个
agent"，不是一个用户可勾选的通用开关，也**没有配置文件**能改变某个内置 agent 的扫描目录
——完全硬编码在 Rust match 里。目前唯一真正**用户可写**的"额外目录"字段是 `custom_agent`
表的 `skills_dir: Option<String>` + `skills_shared_store: bool`
（`db/entities/custom_agent.rs:22-28`，本人核实）——但那是给"用户注册的自定义 agent"用的，
不是给 room/session 用的。另有一套完全独立的第四类技能面（`~/.codeg/skills/<id>/`，
`custom_skills.rs`，symlink 进各 agent 的全局技能目录，支撑 Settings → Skills 页面）——
和本报告讨论的 composer `$` 面板是两个不同的表层，不要混为一谈。

### 1.4 Room / Session 数据模型现状（本人核实实体文件与三份迁移文件）

`conversation`（Session）表 `folder_id: i32` **NOT NULL**（`db/entities/conversation.rs:54`）
——一个 Session 永远绑定恰好一个 Folder，`folder.path: String`（`#[sea_orm(unique)]`，
`folder.rs:26`）才是真实文件系统路径。**`collaboration_room`（Room）根本不是 SeaORM
entity**——`db/entities/mod.rs` 里没有它，全靠 `collaboration_room_service.rs` 里手写
`execute_unprepared` + `try_get` 的原生 SQL 操作（本人核实 `root_folder_id` 在该文件里
至少 10 处读写点：155-274、357-466、650-690、1086/1104/1623/1816-1846 行）。它的"绑定
folder"字段是 `root_folder_id INTEGER NULL`（migration
`m20260818_000004_room_item_and_author.rs:20-31`，本人核实全文件）——**可空**，由创建
room 那条 conversation 的 folder 一次性回填（`COALESCE(f.parent_id, f.id)`，worktree
文件夹回填到其项目根），之后就是普通字段。`CollaborationRoomSummary`/
`CollaborationRoomDetail`（`models/collaboration.rs:535-577`，本人核实）都以
`root_folder_id: Option<i32>` + `#[serde(default)]` 的 camelCase 形式暴露给前端
（`rootFolderId?: number | null`，`src/lib/types.ts:1434/1473/1488`）。

迁移文件命名惯例（本人 fresh 核对 `db/migration/mod.rs` 尾部，当前 HEAD 下最新一条是
`m20260819_000002_prompt_queue_automation_source`）：`m<YYYYMMDD>_<六位序号>_<蛇形描述>.rs`，
一迁移一文件，在 `Migrator::migrations()` 里按文件名日期顺序显式注册（`up`/`down` 混用
SeaORM schema builder 和裸 SQL，SQLite 改 FK 行为这类操作走"整表重建"的裸 SQL，如
`m20260818_000008`）。Room 相关命令遵循统一的 `<name>_core(conn, emitter, ...)`（纯业务
逻辑，运行时无关）+ `#[cfg(feature = "tauri-runtime")] #[tauri::command] <name>(..., db,
app)`（薄封装，调 `_core` 并传 `EventEmitter::Tauri(app)`）配对——本人核实
`collaboration_room_rename_core`/`collaboration_room_rename`
（`commands/collaboration.rs:1431-1440` 与 1596-1603 行）、
`collaboration_room_add_members_core`/`collaboration_room_add_members`
（1410-1418 与 1569-1575 行）两组，均额外在 `web/handlers/collaboration.rs` +
`web/router.rs` 里挂一份 HTTP 版本，前端经 `src/lib/api.ts:2239-2260` 的
`addCollaborationRoomMembers`/`renameCollaborationRoom` 调用 `getTransport().call(...)`。

Room 的"…"菜单（`rooms-page.tsx:679-710`，本人核实）目前只有三项：Rename（693-696 行）、
Add member（697-700 行）、分隔线后 Delete（702-708 行），用的是标准 shadcn `DropdownMenu`。

### 1.5 现状小结

一句话画出现状数据流向：`composer 输入 @` → `useReferenceSearch`（单 `defaultPath`）→
四路并发（`useFileTree` 预热的文件 / `useAcpAgents` 预热的智能体 / 懒加载会话 / 懒加载
提交）→ `buildReferenceGroups` 逐组截 50 → `suggestion-popup.tsx` 渲染。Room 在外面再套
一层"成员组常驻 + workspace 组 3 秒超时降级（不取消底层调用）"。这条链路上，**"单 root"
这个假设写死在三个层级**：`useFileTree` 的 `folderPath: string | undefined`（本人核实
全文件，`use-file-tree.ts:19,42`，内部状态是标量 `allFiles`/`loadedForPathRef: string |
null`，不是按 root 分桶的 map）、`list_workspace_files(path: String)` 的单参数签名、以及
`CollaborationRoomDetail` 只有一个 `root_folder_id`。第 3 节逐一说明怎么在不破坏现有
分组/降级架构的前提下，把"单 root"换成"N 个 root"。

## 2. 对标清单

### 2.1 Claude Code（可信度见第 0 节；本仓是 sourcemap 反推 + AI 结构补全，非官方源码）

@ 文件补全的索引来源是两路合并（`fileSuggestions.ts:523-570` 的 `getPathsForSuggestions`）：
`getProjectFiles()`（git 仓库走 `git ls-files --recurse-submodules`，非 git 仓库走
`ripgrep --files` 兜底）+ `getClaudeConfigFiles(cwd)`——后者才是关键：它遍历
`CLAUDE_CONFIG_DIRECTORIES = ['commands','agents','output-styles','skills','workflows',
...]`（`markdownConfigLoader.ts:29-36`），每个子目录都从**三个物理位置**加载 markdown：
托管/策略目录、`~/.claude/<subdir>`（`getClaudeConfigHomeDir()`，可被
`CLAUDE_CONFIG_DIR` 环境变量覆盖）、以及从 cwd 向上走到 git 根的逐级项目目录。两路结果
`[...projectFiles, ...configFiles]` **合并进同一个 `FileIndex`**——也就是说 Claude Code
的 @ 文件补全**确实会把全局配置目录里的 markdown 文件和项目文件混进同一个结果列表**，
不是分成两个面板。性能防护相当具体：`AbortSignal.timeout(10_000)` 包住整条取路径链路，
`git ls-files` 5s / 后台未跟踪文件 10s 各自超时，`MAX_SUGGESTIONS=15` 结果上限，模糊引擎
（`native-ts/file-index`，号称是 Rust `nucleo` 的纯 TS 移植）有 `MAX_QUERY_LEN=64`、
`TOP_LEVEL_CACHE_LIMIT=100`、每 ~4ms 让出一次事件循环的分片构建、以及一个 O(1) 位图前缀
过滤器（声称对常见查询能提前排除 89%-90%+ 的候选）。`respectGitignore` 是一个真实的
settings.json 键（默认 `true`），另有 `fileSuggestion.command` 允许整条替换为外部命令。

**skills/commands 和 agents 的覆盖优先级方向相反**，这是本轮调研里最具体、最值得记住的
一条反直觉发现：skills/commands 的合并顺序是"managed → user(全局) → project"后**用
`Array.prototype.find()` 取第一个匹配**（`loadSkillsDir.ts:717-723`，`commands.ts:688-698`）
——同名时**全局赢**；agents 却是把各来源按 `[builtIn, plugin, user(全局), project, flag,
managed]` 顺序逐个 `Map.set()`（`loadAgentsDir.ts:193-221`）——**后写覆盖先写**，同名时
**项目赢**。两者用的是完全不同的合并语义，不是同一套代码复用出来的两个结果。

### 2.2 Codex CLI（本人抽验了 `require_git` 这条注释，与子代理报告逐字一致）

模糊文件搜索是独立 crate `codex-file-search`（`codex-rs/file-search/`），核心是
`ignore::WalkBuilder` 遍历 + `nucleo`（Helix 编辑器同款模糊匹配库，git 依赖固定 rev）打分，
**增量流式**而非一次性收集：walker 线程把发现的路径塞进 `nucleo::Injector`，matcher 线程
并发消费并通过回调把带 `walk_complete: bool` 的部分快照推给调用方——有专门的测试
`session_streams_updates_before_walk_complete` 断言"遍历没结束就已经有结果流出"。TUI 弹层
再截到 `MAX_POPUP_ROWS = 8`；app-server RPC 层另设 `MATCH_LIMIT = 50`；标准 CLI 默认
`--limit 64`。**没有找到整体墙钟超时**（`scanned_file_count` 只是展示用，从不参与"该不该
停"的判断），Codex 依赖的是**取消而不是超时**：一条共享 `AtomicBool` 取消旗标 +
per-session 的 Drop 触发关闭旗标 + TUI 的世代计数器（新查询让旧结果的 `on_update` 直接被
丢弃）+ app-server RPC 的取消令牌（新请求复用同一个 token 就会把旧请求的旗标翻正）——
四层机制组合起来，靠"新查询自动废弃旧查询"而不是"每个查询各自设一个时限"来避免无限期占用
资源。

本人直接读取并逐字核对了这段设计注释（`file-search/src/lib.rs:399-410`）：*"The walker
uses `require_git(true)` to match git's own ignore semantics: git never reads `.gitignore`
files from directories above the repository root. Without this flag, the `ignore` crate
reads `.gitignore` files from all ancestor directories—a deliberate divergence... allowing
a broad parent ignore (e.g. `~/.gitignore` containing `*`) to silently suppress every file
in the walk."*——**这与我方 `list_workspace_files` 的选择正好相反**：`folders.rs:4715-4720`
（本人核实）的注释写的是"respect in-tree `.gitignore`/`.ignore`/`.git/info/exclude`, but
not the global gitignore or parent-directory ignores... `require_git(false)` keeps
`.gitignore` effective even outside a git repo"，代码里配的是 `.require_git(false)`。两边
都是**深思熟虑的选择**，不是谁忘了配：Codex 的顾虑是"一个宽泛的祖先 `.gitignore` 可能悄悄
吞掉整棵树"，我方的顾虑是"非 git 目录（比如用户随手加的一个笔记文件夹）也该尊重它自己的
`.gitignore`"——这条不建议改，只是在第 3.5 节讨论"附加路径可能是任意非 git 目录"时会
再引用它。多 root 支持是**库原生**能力：`create_session`/`run` 直接接受 `Vec<PathBuf>`，
一个 `WalkBuilder` 靠 `.add(root)` 吃下所有 root，喂进**同一个** `nucleo` 实例——即多个
root 的排名是在一个统一的竞争列表里产生的，不是"各 root 分别搜、再拼接排序"（app-server
在拿到结果后另外做了一次 `sort_by(cmp_by_score_desc_then_path_asc)` 兜底去抖，但底层排名
已经是统一的）。TUI 的 @ 提及本身**只用单 root**（`tui/src/file_search.rs` 永远传
`vec![self.search_dir.clone()]`），多 root 能力目前只有 app-server 的 RPC 客户端在用。
全局目录方面：文件搜索本身**不触达**任何 home 目录路径；唯一触达 home 的是一个从
`~/.codex/AGENTS.md`（`codex_home`，默认 `~/.codex`，可被 `CODEX_HOME` 覆盖）读取的
"全局用户指令"，明确拼在项目 `AGENTS.md` **之前**，且项目文档发现本身"不会走出项目根"
（`project_doc.rs:16` 原话）。

### 2.3 opencode

采用双引擎架构：Windows/纯 Node 环境走 `ripgrep + fuzzysort`（真正跑在生产环境的路径），
Bun + 非 Windows + 原生二进制存在时可选走 `@ff-labs/fff-bun`（闭源原生模糊查找引擎，评分
逻辑不在仓库源码里，"NOT FOUND"）。ripgrep 路径是**一次性后台全量扫描后常驻内存**、纯
按键触发的内存内 `fuzzysort` 过滤（不是"每次按键都重新扫盘"）——按 git 仓库与否分别设
"无限"和"硬顶 100,000 条"两档，这是三个参考仓里唯一和我方"数量硬顶"最相似的设计（只是
数字和 git/非 git 的分档方式不同）。**没有找到墙钟超时、也没有找到按键防抖**；取消机制
不完整——共享的 `search()` helper 支持 `AbortSignal`，但实际被聊天 @ 提及调用的
`searchFilesAndDirectories` 包装函数**丢弃了这个能力**，只在文件浏览器的另一个搜索框里
真正接上了取消（`session-file-browser-tab.tsx`，用 TanStack Query 的自动取消）。TUI 端有
一套独立的、手写的 frecency（`frequency / (1 + 天数)`，JSON-lines 持久化，上限 1000
条），但**只应用于非文件类建议**（agent 提及、引用别名）——文件结果本身要么信任原生引擎
自带的排序，要么纯 `fuzzysort` 分数，代码注释原话是"Files come from fff already fuzzy
ranked and filtered, it shouldn't be additionally sorted"。多 root：**单请求单 root**，
不存在"一次查询、多个目录合并结果"的路径；工作区切换是路由到不同的独立服务实例，不是
合并。全局目录方面，agents/commands/skills 的发现是**和文件搜索完全分离**的另一套机制
（`packages/core/src/config.ts:164-203`，本人直接读取核对）：从 XDG 全局配置目录
（`~/.config/opencode`）开始，逐级向上收集到项目根之间的每一个 `.opencode` 目录，最终
顺序"全局在前、越靠近打开位置的项目配置在后"，越靠后优先级越高（`config.ts:193-194`
原话："A config closer to the opened directory should win over one higher up"）——和
Claude Code 的"skills 全局赢、agents 项目赢"的分裂结论不同，opencode 在 agents/commands/
skills 三者上用的是**同一套**合并顺序（全局先、项目后，项目赢），内部更一致。

### 2.4 paseo（用户点名的产品，"待调研其形态"——结论是目前证据下没找到对应功能）

paseo 是一个和 codeg 精神上相当接近的产品（"one interface for Claude Code, Codex,
Copilot, OpenCode, and Pi agents"，自托管 daemon + 多端客户端），值得对标。本人用关键词
（`additionalDirect*`、`mention`）扫过 `packages/{server,app,cli,protocol,desktop}/src`
五个源码目录，找到两个名字上相关但**都不是**"@ 补全搜索更多目录"这个功能的东西：

1. `additionalDirectories: z.array(z.string()).optional()`
   （`packages/server/src/server/agent/providers/claude/options.ts:50`）——这是
   `@anthropic-ai/claude-agent-sdk` 自己的 `additionalDirectories` 选项的直通 schema
   声明（文件顶部注释"Claude Agent SDK Options, maintained against
   @anthropic-ai/claude-agent-sdk 0.3.220"）。这是一个**沙箱/权限**概念——"这个 Claude
   Code agent 进程本身除了 cwd 还被允许读写哪些目录"——不是"补全面板去哪里模糊搜文件"。
   本人核实全仓库范围内这个字段只出现两处：上述 schema 声明，和一个测试工具里的空数组
   默认值（`claude-config.ts:64`），`packages/app`、`packages/desktop` 两个 UI 包**零
   命中**——没有证据它被包装成任何用户可设置的界面，更像是"为了让 schema 完整而声明"的
   透传字段。
2. paseo 自己真正的 @ 文件提及实现——`file-mention-autocomplete.ts`（纯文本层：找 `@`、
   格式化引号路径、做替换）+ `use-agent-autocomplete.ts`（本人核实其 root 解析逻辑，
   `cwd` 一律来自 `draftConfig.cwd` 或 `sessions[serverId]?.agents?.get(agentId)?.cwd`）
   ——**单 root，绑定在 agent 自己的 cwd 上**，结构上和我方 `useFileTree`/`defaultPath`
   的现状完全同构，没有多 root 或"附加路径"的等价机制。

**结论**：按本次检索到的证据，用户记忆中"paseo 似乎有这个功能"，更可能是把 SDK 层的
`additionalDirectories`（agent 沙箱可读写范围）和 UI 层的 @ 补全搜索范围这两个概念记混
了——这两者在 codeg 里也是完全不同的两个子系统（前者是每个 agent CLI/SDK 自己的沙箱参数，
和 codeg 的 @ 补全无关；后者才是本报告在设计的东西），值得在设计里明确切割，不要因为名字
像就合并成一个功能去实现。如果"给 agent 本身开放沙箱外的额外可读写目录"也是一个真实
需求，那是一个独立的、值得另开一个调研的话题（涉及 codeg 自己怎么给各 agent CLI 传递
等价参数，如果它们各自都有的话），不在本报告范围内。

### 2.5 对比矩阵

| 维度 | codeg（现状） | Claude Code | Codex CLI | opencode | paseo |
|---|---|---|---|---|---|
| 遍历策略 | `ignore::WalkBuilder` 单 root，一次性收集 | git 仓库用 `git ls-files`，否则 ripgrep 兜底 | `ignore::WalkBuilder`，多 root 单实例，**增量流式** | ripgrep 一次性后台扫描，常驻内存，按键纯内存过滤 | 未知（单 root，未深挖底层扫描方式） |
| 模糊匹配 | 纯子串 `.includes()`（无评分） | 自研位图前缀过滤 + TS 移植版 `nucleo` | `nucleo`（Rust 库） | `fuzzysort`（或闭源原生引擎） | 未深挖 |
| 结果上限 | 50k 条/root（硬顶）+ 前端 50 条/组 | `MAX_SUGGESTIONS=15` | 20-64（各调用方不同） | 10-200（各调用方不同） | 未知 |
| 墙钟超时 | 前端 3s（room，不取消底层）；后端**无** | 多处 3s-10s（分段） | **无**（依赖取消而非超时） | **无** | 未知 |
| 取消机制 | 前端 `AbortSignal` 传到 `search`，**后端无** | 未深挖 | 四层：旗标+Drop+世代计数器+RPC取消令牌 | 部分：`search()`支持但聊天@提及路径**丢弃** | 未知 |
| 大目录/非 git | 50k 硬顶，`require_git(false)` | 后台防抖+超时链 | 无硬顶，`require_git(true)` | 非 git 硬顶 100k，git 无限 | 未知 |
| 全局目录入 @ | 仅 skill 面板（按 agent 类型硬编码），文件补全不涉及 | **是**，混进同一文件索引 | 否（文件搜索本身不触达；仅 AGENTS.md 单文件例外） | 否（文件搜索独立；agents/commands/skills 另一套机制会） | 未找到对应功能 |
| 多 root 合并 | 无（单 root） | 项目+全局配置目录，两路数组拼接 | 库原生支持，统一 `nucleo` 实例排名 | 无（单请求单 root） | 无（单 root） |

## 3. 设计建议

### 3.1 附加路径数据模型

Room 不是 SeaORM entity（1.4 节已确认），风格上应该继续走原生 SQL 子表，参照
`collaboration_room_member` 的既有形态，新增 `collaboration_room_path`：

```sql
CREATE TABLE IF NOT EXISTS collaboration_room_path (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id TEXT NOT NULL,
    path TEXT NOT NULL,
    alias TEXT NULL,
    added_by_conversation_id INTEGER NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (room_id) REFERENCES collaboration_room(id) ON DELETE CASCADE,
    UNIQUE (room_id, path)
);
CREATE INDEX IF NOT EXISTS idx_collaboration_room_path_room ON collaboration_room_path(room_id);
```

理由逐条：`path` 用裸字符串而不是 `folder_id` 外键——附加路径的意义恰恰是"不需要先把这个
目录『打开』成一个正式 Folder 才能引用它"，如果绕道 `folder_id`，用户体验上等于先要求他
在别处新建一个 Folder，违背这个功能本来要解决的"快速添加"诉求；`list_workspace_files`/
`git_log` 今天本来就是接收裸路径字符串，不是 `folder_id`，这条设计和现有命令的参数形态
是一致的。`alias` 对齐 `folder.alias`（`folder.rs:42-44`）的先例，路径一多就需要展示名。
`added_by_conversation_id` 对齐 room 自身的 `created_by_conversation_id` 做溯源。
`UNIQUE(room_id, path)` 防止重复添加产生重复搜索 root。迁移文件命名遵循 1.4 节确认的
惯例，落点 `src-tauri/src/db/migration/m20260820_0000XX_room_additional_path.rs`（今天
是 2026-08-20，当前最新注册迁移是 `m20260819_000002`，具体序号取决于当天是否有其它迁移
先落地——落地前重新看一眼 `mod.rs` 尾部）。

DTO 改动镜像 `member_count`/`members` 已有的"摘要用计数、详情用全量"分层
（`models/collaboration.rs:544` vs `574`）：`CollaborationRoomSummary` 加
`additional_path_count: u32`，`CollaborationRoomDetail` 加
`additional_paths: Vec<RoomAdditionalPath>`（`{ id, path, alias, created_at }`，同样
`#[serde(rename_all = "camelCase")]`）。命令严格照抄 1.4 节确认的
`<name>_core`/`<name>` 配对：`collaboration_room_add_path_core`/
`collaboration_room_add_path`、`collaboration_room_remove_path_core`/
`collaboration_room_remove_path`，落点 `commands/collaboration.rs`，同步在
`web/handlers/collaboration.rs` + `web/router.rs` 挂 HTTP 版本，`api.ts` 加
`addCollaborationRoomPath`/`removeCollaborationRoomPath`。不需要单独的 list 命令——
路径列表随 `CollaborationRoomDetail` 一起返回，跟 `members` 的现有做法一致。`_core`
里应该在写库前校验路径存在且是目录（复用现有的路径校验逻辑——本次侦察没有定位到一个
现成的通用"校验这是个真实目录"辅助函数，落地时需要先确认是否已有可复用的，没有就顺手
建一个）。

**范围声明**：本节设计只覆盖 Room。用户原话是"room/session"，但明确的落点入口（"…"菜单）
只提到 Room；Session（`conversation`）是正经 SeaORM entity，等价能力的实现方式会不同
（走 SeaORM migration + 关联表/字段，不能照抄这批的裸 SQL 迁移文件，只能照抄"表结构设计
的思路"）。建议这批只做 Room，Session 的对等能力作为下一批，避免这批范围失控。

### 3.2 后端多 root 扫描合并

两个可选实现深度：

**方案 A（本批建议）**——后端 `list_workspace_files` 保持单 root 签名完全不变，前端对
"room 的 root_folder 路径 + 每个 additional_path"分别发起独立调用，`Promise.all` 拿到后
在前端合并、重新按 3.3 节的规则统一截断排序。零后端改动，`list_workspace_files` 今天唯一
的调用方（普通 Folder 文件面板）不受任何影响；每个独立调用依然各自享受已合入的 50k 硬顶+
`spawn_blocking` 护栏，"N 个 root"变成"N 次独立的、各自被护栏罩住的调用"，不产生新的
无护栏路径。

**方案 B（本批不做，标记为路标）**——参照 Codex 的做法，让 `list_workspace_files` 原生
接受 `extra_paths: Vec<String>`，内部一个 `WalkBuilder` 靠 `.add(root)` 吃下所有 root，
统一预算、统一排序。这是"更正确"的终态（多个 root 共享同一份 50k 预算和同一次遍历，不会
出现"root 1 吃满 50k、root 2 一条没扫到"却各自看起来正常返回的情况），但要改的是**现有
每个单 root 调用方都依赖的核心护栏函数**，改动半径明显更大，且收益只有在"一个 room 挂了
足够多附加路径，扫描 root 1 就快吃满 50k 预算"这种边缘场景下才体现出来。建议只在方案 A
上线后，如果附加路径数量真的涨到这个规模，再考虑升级。

### 3.3 前端多 root 支持——`useFileTree` 原地扩展

本人读过 `use-file-tree.ts` 全文件后确认：`MAINTAINABILITY-PROGRAM-2026-08-19.zh-CN.md:
667-668` 里编排给出的初判"扩展点在 `use-file-tree` 的多源合并与去重"是对的，且比我最初
设想的"绕开这个 hook、在 `use-reference-search.ts` 里另起一套懒加载"更干净——原因是这个
hook 内部的数据获取本来就是一个普通 `useEffect` 里的 `async load()`，不是逐 root 调一次
hook，所以把"一条路径"泛化成"一组路径"并不违反 Hooks 规则，只是把 effect 内部的单次
`await listWorkspaceFiles(folderPath)` 换成对一组路径的 `Promise.all`。而且这个 hook
按其文档注释本来就是"shared by the search dialog and the composer @-mention picker"
（33-34 行）——多 root 能力做在这一层，两个消费方都能受益,不是只服务 room 一家。

具体改法：
- `UseFileTreeOptions.folderPath: string | undefined` → `folderPaths: string[]`（主
  composer 传 `[primaryPath]`，room 传 `[roomFolderPath, ...additionalPaths]`，都过滤掉
  `null`/空值后再传入）。
- 内部缓存从标量 `loadedForPathRef: string | null` 换成按 root 分桶（如
  `Map<string, FlatFileEntry[]>` + 已加载 root 的 `Set`）——只对**新增**的 root 发起
  `listWorkspaceFiles` 调用，已缓存的 root 不重新扫；某个 root 被移除（用户删掉一个附加
  路径）时，直接从合并结果里剔除对应桶，不需要重新请求其余 root。
- 对外的 `allFiles: FlatFileEntry[]` 仍是拍平后的合并结果，但 `FlatFileEntry` 需要新增
  一个字段记住"这条来自哪个 root"（目前 `relativePath` 是相对单一 root 的相对路径，多
  root 下必须知道是相对哪个 root，`suggestion/adapters.ts` 的 `fileToSuggestion(entry,
  root)` 目前把 `root` 当成外部统一传入的单一值，多 root 后要改成从 `entry` 自带的字段
  取）——这是这条改动里唯一会连带影响 `buildReferenceGroups`（102-116 行的文件过滤/适配
  循环）和 `suggestion/adapters.ts` 的地方。
- 错误处理需要跟着升级：现状"任意一次 fetch 失败就把全部 `allFiles` 清空"（`catch {
  setAllFiles([]) }`，74-75 行）在多 root 下不再合适——一个被用户手动删除、路径已经不
  存在的附加路径请求失败，不该把主 root 和其它附加路径的文件也一起清空，应该只把失败的
  那个 root 的桶置空。
- `buildReferenceGroups` 的最终 `MAX_PER_GROUP=50` 截断建议在**合并所有 root 之后**
  统一应用一次（而不是每个 root 各截 50 再拼接）——否则总渲染条数会随附加路径数量线性
  膨胀（`50 × root 数`），`truncated` 标志也应该反映"合并后确实超过 50"这一件事，而不是
  "某个 root 单独超过 50"。

`defaultPath`（普通 composer 的单 root）保持完全向后兼容——只是内部被包成一个长度为 1 的
数组，普通 composer 永远不传 `additionalPaths`，行为零变化。

### 3.4 "…"菜单入口

在 `rooms-page.tsx:692-709` 现有的 Rename/Add member/分隔线/Delete 之后，或在分隔线前，
新增一项（如"管理路径"），点击打开一个对话框，形态参照紧邻的 Add member 对话框
（`addOpen`/`setAddOpen` 状态，697 行）：列出 `detail.additionalPaths`，每行一个移除
按钮，加一个文本输入框提交新路径（提交后调 3.1 节的 `addCollaborationRoomPath`，失败——
比如路径不存在——用现有的 `toast.error` 模式提示，参照 `handleAddMembers`
的错误处理写法，556-575 行）。

**一个需要在落地前确认的开放问题，本报告不替它下结论**：这个"添加路径"输入框要不要配一个
目录选择器？桌面 Tauri 模式理论上可能有原生文件夹选择对话框插件，但 server 模式（浏览器
访问、`CODEG_STATIC_DIR` 纯静态托管）不可能弹出"选择服务器机器上某个目录"这种原生对话框
——如果两种模式要共享同一套 UI，只能走纯文本路径输入（可以配一个复用
`get_file_tree`/`list_workspace_files` 的只读目录树浏览组件做辅助选择，而不是 OS 原生
选择器）。本次侦察没有确认现有"新建 Folder"流程具体怎么解决这个跨模式问题（未在范围内
读那部分代码）——它大概率已经解决过一次，落地这个菜单项之前应该先找到那个方案直接复用，
而不是重新发明。

### 3.5 全局目录要不要入 @ 范围

**建议：不自动纳入，改为让用户把它当成一条普通的附加路径手动添加。**

支持这个结论的证据链：
- Codex（架构上和我方 `list_workspace_files` 最像的对照物，都是"裸文件系统模糊搜索"而非
  "策展过的配置发现"）明确不把这类范围扩展做进文件搜索本身——2.2 节确认过，它触达 home
  目录的唯一路径是一个高度受限、单一命名的 `AGENTS.md`，而不是一次目录级扫描。
- 我方 Room 自己的既有设计意图已经很明确：`rooms-page.tsx:424-426` 的注释把 room 的
  文件/提交搜索范围定义为"resolved from `rootFolderId`... a room is a cross-folder
  concept"——这句注释在说的是"跨 folder 没问题，但范围必须是显式解析出来的、可见的"，
  不是"隐式地伸到用户主目录下"。
- Room 是**多 Session 共享**的（`collaboration_room_member`，可能包含不同 agent 类型
  的多个 Session，甚至未来可能是不同人类协作者），这和 Claude Code 那种"单用户本地 CLI
  看自己主目录下的技能文件"是完全不同的暴露语境——把 `~/.claude`、`~/.codex`、
  `~/.agents/skills` 这类可能包含其它项目技能、个人指令文件的目录，未经用户逐 room
  显式确认就悄悄并入一个多方共享空间的 @ 候选列表，是这批功能不该承担的隐私意外。
  Claude Code 敢把全局配置目录混进 @ 索引，前提是它的整个产品语境就是单用户单进程本地
  CLI，这个前提在 codeg 的 Room 上不成立。
- 一旦"全局目录"被当成普通附加路径处理，3.1-3.4 节的全部机制原样复用，不需要为"全局"
  这个概念在数据模型或后端里做任何特殊分支——用户想要 `~/.claude/skills` 可搜索，就把
  它当一条路径加进去，跟加一个同事的项目目录没有本质区别，用户自己承担"我确实想让 room
  里的人搜到这个目录"的知情选择。

**唯一的例外，且应该保持现状不动**：composer 的 `$` 技能面板（1.3 节）继续沿用
"按 agent 类型硬编码全局+项目目录"的既有机制——它的范围本来就是"这个 agent 能执行哪些
技能"，是一个受限、策展过、单一目的的列表，和"随便搜一个文件"的信任/范围模型完全不同,
不要因为这次调研而把两者混着改。

### 3.6 扫描超时机制分层设计

用户建议的 10 秒应该加在**后端**，和现有 50k 条硬顶并列成为 `walk_workspace_files` 循环
里的第二个跳出条件，而不是只加在前端。理由是这批设计里权重最高的一条判断，值得完整说
一遍：

**条目数上限治的是"内存/结果量"，治不了"墙钟时间"。** `walk_workspace_files` 现有的
`if entries.len() >= cap { break; }`（`folders.rs:4749-4751`，本人核实）假设的失败模式
是"一个巨大但正常的目录，每个条目处理耗时大致均匀"——1.2 节引用的真实故障（Thesis 目录）
正是这种情况，条目数硬顶已经能防住。但一个真正病态的场景——比如附加路径恰好指向一个高
延迟的网络挂载盘，或者一个单个子目录下有海量条目需要先被 `ignore` 逐个匹配才能判断是否
剪枝——条目数还没到 5 万，墙钟时间可能已经不可接受，条目数上限对这种情况完全不设防。
**附加路径这个功能恰恰会放大这个风险**：用户手动填一个绝对路径字符串，不像 Folder 那样
经过"打开"这个动作的某种隐性筛选（如果有的话），指向一个慢/怪路径的概率更高。

**更关键的一点：只在前端加超时，等于没修这个问题的根。** 1.2 节已经引用过
`room-mention-search.ts` 自己代码注释里的原话——3 秒 race 从不取消底层调用，一个真正
卡住的 walk **依然占着 `spawn_blocking` 线程池里的一个线程**，这正是最初那次 Thesis
故障"耗尽运行时资源"的机制本身。前端超时只是让用户看不到症状，后端那个被占住的线程/
资源不会因为前端不再等待而被释放。所以：

- **后端**（主层）：在 `walk_workspace_files` 现有循环里加第二个跳出条件——
  `if entries.len() >= cap || Instant::now() >= deadline { break; }`——`deadline` 由
  一个新常量（如 `const WORKSPACE_SCAN_DEADLINE: Duration = Duration::from_secs(10);`，
  和 `MAX_WORKSPACE_FILE_ENTRIES` 同级）在函数入口处算出。这是和现有硬顶完全同构的
  "多加一个静默截断条件"，不是新的失败模式。建议先按常量而非按调用方参数处理——现在
  没有任何调用方需要不同的预算，等真出现了再升级成参数（比如 3.7 节可能出现的后台预扫
  任务，如果愿意给它更长的预算）。
- **是否要把这次截断标记出来**：现有截断是**完全静默**的（返回类型 `Vec
  <WorkspaceFileEntry>` 没有任何 `truncated`/`total` 字段）。考虑到"因为 50k 太多"和
  "因为这个路径异常慢，可能是配置错了"对用户的含义完全不同，且"附加路径管理"UI（3.4 节）
  恰好提供了一个可以挂"⚠ 这条路径扫描超时，结果可能不全"提示的具体位置，本报告建议
  **这次是打破"静默截断"惯例、给返回值加类型化标记的合适时机**——但这意味着改
  `list_workspace_files` 的公开返回类型（`Vec<T>` → `{ entries, truncated, reason }`
  这样的结构），会连带影响 `api.ts` 的类型签名、`use-file-tree.ts`、以及所有消费这个
  命令的调用点，是一个不小的连锁改动。**如果这批想控制改动面**，退而求其次的选项是：
  只加 `deadline` 检查本身（真正堵住漏洞），命中时只打一条 `tracing::warn!` 日志做
  可观测性，不改返回类型；等 3.4 节的路径管理 UI 真正做出来、需要消费这个信号时，再
  把类型化截断原因作为那批的一部分引入——把"API 形状改动"的时机往后放到"有 UI 会消费它"
  的那一刻，符合这个项目已经在遵循的小批量节奏。
- **前端**（既有 3 秒 race 保持不变，和新的后端 10 秒是两个不同维度的预算，不需要
  互相迁就）：room 的 3 秒是"UI 愿意等多久再降级成仅成员组"的响应性预算；后端 10 秒是
  "一次扫描最多占用多久系统资源"的资源卫生预算。3 秒 < 10 秒这个关系已经是对的——常见
  情况下前端会先放弃、降级显示，用户根本感知不到后端还在慢慢跑到 10 秒；10 秒只在"前端
  已经放弃了、但后端还是要对自己的线程池负责"这个场景下才起作用，两个数字不需要也不应该
  被强行统一成一个值。

**对标结论要如实说**：三个参考仓都没有这个形状的墙钟超时——Codex 靠取消而不是超时
（2.2 节），opencode 没有整体超时（2.3 节），Claude Code 有但是分散在多个更细粒度的
`AbortSignal.timeout`（10s 整体、5s/10s 单项）上，不是"条目数硬顶 + 墙钟硬顶"叠加的
形状。这条建议不是"抄某一家的做法"，而是基于我方自己的故障史（1.2 节）和这批新增的
"用户随手填路径"场景，独立得出的、三家参考实现里都没有先例的加固——如实这样呈现，不要
包装成"对标 Codex"。

### 3.7 值得引进的算法评估

**(a) Frecency（时近度加权排序）——本批不建议做。** opencode 是唯一有真实实现的参考仓，
但它自己的实现选择就说明了适用边界：手写的 frecency 只用在非文件类建议（agent 提及、
引用别名）上，文件结果本身要么信任原生引擎排序要么纯分数，opencode 自己都没有把 frecency
用在文件排序上；Codex 的 `fff` 引擎虽然暴露了 frecency 相关字段，但 opencode 自己的
TS 代码从未调用（`trackQuery`/`getHistoricalQuery` 全仓零调用）。对我方而言，文件组
今天连一个真正的评分匹配器都没有（纯 `.includes()` 子串），frecency 需要新的持久化层
（存哪、per-workspace 还是 per-用户、server 模式下要不要跨端同步）+ 新的埋点（在建议
被选中时记录），投入和这批"加路径+加超时"的诉求不对称。如果以后要做，应该学 opencode
先落在已经有评分排序基础的组（会话/智能体，已经在用 `rankByTextMatch`），不要从文件组
开始。

**(b) 前缀索引/位图预筛（Claude Code 的自研引擎、Codex 的 `nucleo`）——本批不建议做。**
我方文件匹配今天是最朴素的线性子串扫描，是四家里排名最低的实现，但 50k 条目/root 量级
下，JS 里做一次线性子串扫描是毫秒级开销，不是真实性能问题——花哨索引在解决一个我们还
没有的问题。**值得记录成候选**：如果 3.2 节的多 root 合并上线后，真的观察到"从多份
50k 名单里筛选变卡"的反馈，下一步应该是把文件组从子串匹配换成一个现成的评分模糊库（如
opencode 用的 `fuzzysort`，JS 生态里现成、轻量、不需要 FFI），而不是自研 Claude Code
那种位图引擎或者引入 Rust `nucleo`（我方前端是纯 TS/Web，用不了）。

**(c) 后台预扫缓存——现状可能已经部分具备，值得单独核实但不必现在做。** opencode 的
"一次性后台扫描、常驻内存、纯按键过滤"模式，和我方 `useFileTree` 的"per-root 懒加载后
缓存在 ref 里，同一 root 不重复扫"在精神上是同一件事，3.3 节的多 root 扩展已经原样
延续了这个模式，不需要额外引进。**唯一不确定、这次没验证的点**：这个缓存的生命周期是
"per-root 常驻"还是"per-组件挂载"——如果每次关闭/重开 room 的输入框都会让同一个 root
重新扫一次盘，那是一个和"附加路径"无关但性价比更高的独立优化机会，建议找机会单独确认
一下 `useFileTree` 缓存在组件卸载后是否存活，不要和这批附加路径的工作耦合。

**(d) 增量失效/文件系统监听——不建议做，也不建议近期立项。** 三家参考实现里没有一家
真正把这个做扎实：opencode 的原生引擎暴露了 `disableWatch` 但 opencode 自己的构造调用
没有明确启用；Claude Code 只是一个 5 秒节流的、git 状态触发的粗粒度重扫，不是真正的
push 式监听。对我方而言，真正的文件系统监听（比如 Rust `notify` crate）是这份清单里
工程成本最高的一项——每个被监听的 root 一个长期后台任务、跨平台监听器的各种怪癖（尤其
是附加路径可能引入的网络盘场景），外加缓存失效怎么通知前端（WebSocket 广播？还是等
下次打开 @ 面板再说？）的设计成本，相对"@ 列表最多滞后几分钟"这个问题的实际严重程度
明显不成比例。更便宜的渐进方案：现有的"窗口聚焦时清空会话/提交缓存"（
`use-reference-search.ts:281-288` 的 `window.addEventListener("focus", ...)`）**目前
没有覆盖文件树缓存**（只清 `sessionsRef`/`commitsRef`，不碰 `useFileTree` 的缓存）——
如果"文件列表滞后"真的成为一个被抱怨的问题，把同一个既有的聚焦刷新惯例扩展到文件树，
是比引入文件系统监听便宜得多的第一步，且这本身和附加路径无关，可以独立成一个更小的
issue。

**(e) 取消令牌/世代计数器——本批不建议做完整版，但记录为已知差距。** Codex 的四层取消
机制（2.2 节）是三家里最成熟的。我方前端其实已经有这条链路的雏形——`ReferenceSearch`
的 `search(query, signal)` 签名本来就带 `AbortSignal`，`use-reference-search.ts:343`
也确实检查了 `signal?.aborted` 才决定要不要采纳结果——但这只解决了"前端要不要采纳一个
迟到的结果"，**后端命令本身不接收任何取消信号**，`list_workspace_files`/`git_log`/
`list_all_conversations` 一旦被 invoke，前端放弃等待并不会让后端提前收工。这和 3.6 节
讨论的是同一个资源释放问题，但 3.6 节的 10 秒硬 deadline 已经是一个更简单、足够用的
解法——一次有硬上限的 walk 无论前端还不等，最多 10 秒后自己了结，不需要额外的取消信号
通道就能达到同样的资源回收效果，不需要在这批里再叠加 Codex 那一整套更复杂的机制。
**值得记录的差距**：如果未来附加路径数量涨到"一个 room 一次触发 5-10 个并发 root 扫描"
的规模，真正的取消传播（比如直接杀掉 `git_log` 的子进程，而不是等它自己跑满 10 秒）会
比"每个 root 各自跑到 deadline"更省资源，那时候值得重新评估要不要引入 Codex 式的
取消令牌。

## 4. 风险与依赖

- **依赖已合并的护栏，且是唯一依赖**：3.2 节的方案 A（前端对每个 root 各发一次
  `list_workspace_files`）完全依赖 `MAX_WORKSPACE_FILE_ENTRIES=50_000` +
  `spawn_blocking`（`folders.rs:4380,4691`，已合入 `a6f968e2`）继续生效——这条护栏如果
  被回滚或改动，多 root 合并会把原来的单点风险变成乘数风险（N 个 root 各自可能堵住一个
  `spawn_blocking` 线程,而不是一个）。3.6 节的 10 秒 deadline 建议直接建在同一段循环
  代码上（`walk_workspace_files`,`folders.rs:4746-4751`),是往同一个函数里加第二个
  跳出条件,不是独立子系统——落地前必须重新核对这段代码当前形态是否还和本报告描述的
  一致。
- **本报告基于的代码快照可能已经过期，且过期速度很快**：团队名单里能直接看到
  `w-file-scan-guard`、`w-room-file-mention` 两个高度相关的名字，命名强烈暗示他们可能
  正在动 `folders.rs` 的扫描护栏或 room 的文件 @ 提及本身。本报告落点最密集的三个文件
  （`folders.rs` 的 `walk_workspace_files`/`list_workspace_files`、
  `use-reference-search.ts`、`room-mention-search.ts`）在本报告完成前后很可能已经被
  这两位工人改动过——第 0 节记录的 HEAD 四次漂移（`a2fef815`→`850b4e5c`→
  `3551283289a6649b2e6b37d2fd14414091eb5059`→`9bc1398b`）就是在本次侦察短短过程中
  实测发生的，不是假设性提醒。落地前必须重新 grep 这几个文件的当前版本，不能假设本
  报告里的行号或函数形态仍然成立。
- **静态导出/跨模式约束的真实冲突点，在 3.4 节已标记为待确认，不在这里重复下结论**：
  唯一需要在这里重申的是,这不是"能不能做"的冲突（弹窗、状态都是纯客户端 React,不违反
  `output: "export"`),而是"要不要假设有原生目录选择器"这个跨 Tauri/server 两态一致性
  问题,已经在 3.4 节标记为开放问题。
- **双模式一致性成本，只在选择"类型化截断标记"那条支线时才发生**：3.6 节如果选择给
  `list_workspace_files` 加返回类型（`truncated`/`reason`),必须同步改 Tauri 命令签名
  （`folders.rs:4687-4697`)、HTTP handler 与其请求/响应结构体（`web/handlers/
  folders.rs:276-287`,本人核实这是个只做参数透传的薄封装,今天两边完全一致,加字段
  必须两边同时改,否则桌面/服务器行为分叉)、前端 `api.ts` 类型、以及每一个消费该命令
  返回值的调用点。这正是 3.6 节建议"本批只加 deadline+日志、把类型改动推迟到有 UI
  消费方再做"的直接原因,不是回避,是有意把改动面切开成两批。
- **Room-only 范围本身是一个已声明的风险**：3.1-3.4 节的数据模型/命令设计只覆盖
  Room,不覆盖普通 Session（Conversation)。如果后续要求"Session 也要能加附加路径",
  `conversation` 是正经 SeaORM entity（不像 room 是纯原生 SQL),届时的具体实现方式
  （SeaORM migration + entity 字段/关联 vs. 这批 room 侧的纯原生 SQL 子表)会不同,
  只能照搬这批的"表结构设计思路",不能直接照搬这批的迁移文件写法。
- **参考仓可信度分级请按第 0 节的标注使用,不要整体等同视之**：尤其是把 Claude Code
  checkout 里的精确数值常量当成"必须对齐 Anthropic 官方实现"的规范,或者把
  codex-current 里标注为"未逐行复核"的结论当成和"已用 `git show` 验证"的结论同等确信
  ——这两类误用都会让后续实现者对参考实现的信心錯配。

## 5. 结论

我方 @ 补全今天的真实缺口只有一个——从 `useFileTree` 到 `list_workspace_files` 到
`CollaborationRoomDetail` 全链路都假设"只有一条路径"，而不是算法或护栏本身不够；三个
参考仓里最值得抄的是 Codex file-search"一次遍历吃下所有 root、统一预算和取消"的架构
选择，而不是任何一家的花哨匹配算法（我方 50k 条/root 的量级还远用不上位图索引或
frecency 这类东西），全局目录（`~/.claude`、`~/.codex` 之类）不建议自动并入 @ 范围，
应该和普通目录一样走用户手动添加这一个入口，因为 Room 是多 Session 共享空间、隐私预期
和 Claude Code 的单用户本地 CLI 不是一回事。批次排序建议先落地 Room 的
`collaboration_room_path` 表 + 命令对 + `useFileTree` 多 root 改造 + 后端 10 秒
deadline（这四件事互相耦合、构成一个能独立验收的整体），"…"菜单 UI 细节、类型化的
截断原因标记、以及 Session 侧的对等能力放进第二批，frecency/前缀索引/文件系统监听/
完整取消令牌链路这批一律不做。
