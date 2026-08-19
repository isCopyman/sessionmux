# 交接单：release #8 交付 + 新一批待办（2026-08-20 早）

> 本会话（Fable 领导）完成：qoder 门禁补考、四个批次收编、release #7/#8 两版交付。
> 用户日用 release #8 中。先读本单 → 再读 `MAINTAINABILITY-PROGRAM-2026-08-19.zh-CN.md`
> §5（事实源）→ 再动代码。旧单 `HANDOFF-2026-08-20-quota-close.zh-CN.md` 已过期。

## 0. 角色边界（用户铁律，不许越界）

- 你是**领导者/设计者/规划者**：只做诊断、设计、裁决、验收、串行合并、集中门禁。
  一切执行派工人：`subagent_type: "worker-k3"`（Kimi K3）。fable 绝不当子代理。
- **思考强度分配（2026-08-20 用户拍板，已写入 ~/.claude/CLAUDE.md）**：写代码默认
  high；跨模块/事务/状态机升 xhigh；max 不是"极少用"——用户自己日常开 max，值得等的
  复杂设计就用；真正机械活（i18n 批量替换、格式化）才降 low/medium。
- 派工简报必带：worktree 正斜杠命令钉基线、开场 gitStatus 过期警告、**禁止编译/运行/
  跑测试/杀进程**、证据必须 file:line、防复活黑名单（旧委派工作流 delegate_to_agent/
  ACP replay/timer autoPaused UI/旧 CollaborationUnreadBadge/list shim/
  outbound_awaiting_summary）、**禁止向主仓写任何文件**。
- 对用户说大白话。永不 push origin。不杀用户的 cargo/tauri/node 进程（自己起的调查
  实例可清，认 target\debug 路径）。
- **release 封版纪律（2026-08-20 用户拍板）**：用户说"换版"才构建 release；用户在用
  release 期间禁止跑 `pnpm tauri build`（Windows 锁运行中 exe）。验证只走 dev 侧。

## 1. 当前快照（现跑命令核实）

- 主分支 `codex/session-message-v1` @ **7ed8ef87**，工作树干净，未 push。
- **用户手里是 release #8**（2026-08-20 早构建，`src-tauri/target/release/codeg.exe`），
  含：Collection 新建会话入口+草稿显示、侧栏三件套（G5-5/G5-11/G5-12）、i18n 清扫、
  qoder、上游三小件、协作手册。
- 门禁基线（9f4d7951 时点全绿）：server 2543 过/0 挂/1 忽略、clippy×3、vitest 362
  文件 4600 用例全绿（修复后）、build 成功。
- 在飞工人：无。残留 worktree：sidebar-ux（已合并完可清，wt/sidebar-ux 全并入主线）。
  其余 worktree（acp-survey/archive-mail 等）是更早批次的残留，均可清。
- 我起的 dev 调查实例已清理（CDP 9222 已关，target\debug 进程归零）。

## 2. 本会话已交付（全部合并+门禁绿）

1. **qoder 批门禁补考**：唯一红测是测试的 Unix 假设（Windows dirs::home_dir() 走
   USERPROFILE 不认 HOME pin），修复 c1b85b53。
2. **playbook**（e0b78898）：协作工具手册，16 MCP 工具+22 Host Control 动作全部
   源码实证。
3. **i18n 清扫**（defee2ca）：前任 9 语言半成品收编+补 44 行英文残留+扫描报告
   （docs/session-workbench/I18N-SWEEP-2026-08-20.zh-CN.md）。
4. **Collection 新建会话**（cb086355 系列）：菜单入口（右键+悬停）+ 草稿立即显示
   在 Collection 下（斜体弱化，点击激活）。
5. **侧栏三件套**（5df48f97+9f4d7951）：G5-5 建群入口显眼化、G5-11 欠回复徽章、
   G5-12 Room 进多选（混选批量操作，跨 root 交集修复）。

## 3. 待办队列（用户 2026-08-20 早亲口布置，按序）

### P0：IME bug（GitHub issue #518，用户明确要求修+测试）

**现象**：Windows + 中文 IME，输 `@美` 能搜到文件，继续输 `术` 变 `@美术` 就空。
ASCII（`@docs`）正常。
**issue 里的方向**（报告者分析）：TipTap mention 的 `allow: !editor.view.composing` +
只读 `nodeBefore` 文本——IME 组合中第二字符变成 `美shu` 或分裂成没有 `@` 的新文本节点。
**派工要点**：先读 TipTap mention 插件的触发逻辑（src/components/chat/composer/ 下），
复现条件写进测试（模拟 compositionstart/compositionend），修复后必须带测试。
**effort 建议**：xhigh（涉及编辑器内部状态机）。

### P1：@ 搜索性能调研+缓存（用户明确质疑）

**用户原话**："@的时候每次都会重新触发搜索，难道本地的 claude code 和 codex 他们的
@ 实现都是这样的？这个非常消耗性能很不合理，是不是应该有什么缓存机制？即加载上次的
缓存，然后同时也刷新？建议研究一下他们的代码。包括会话中心、待回复面板也是。"
**派工**：explore-k3 只读调研——① Claude Code CLI 和 Codex CLI 源码里 @ 补全的缓存/
索引机制（增量刷新？debounce？frecency？）；② 我们现状（@ 面板、会话中心列表、
待回复取数）每次是否全量重查；③ 产出方案（缓存+后台刷新的具体设计）。
**注意**：这是调研先行，方案出来给用户过目再实施。

### P2：搜索体系盘点与整合（用户 2026-08-20 早连珠提问，全部待调研回答）

用户原话（四个子问题，一个都别丢）：

1. **全局搜索覆盖范围存疑**："现在的搜索功能，覆盖会话和文件，感觉非常奇怪，会话
   已经被我们挪到单独的会话中心了，不应该出现在这里了是不是？"——调查：全局搜索
   （Ctrl+K？侧栏搜索？）现在到底搜什么，会话条目是否该撤出全局搜索（既然有会话
   中心），向用户报告现状+建议。
2. **会话中心无法搜 Room？**："会话中心是不是无法搜索 room，要不要把 room 也当做
   session 可以被搜索？room 多了之后感觉搜索也很有必要啊，放在会话中心里？还是怎么
   说？"——先确认现状（会话中心搜索是否含 Room），再出方案：Room 进会话中心搜索结果
   （类型标记区分），还是单独入口。用户倾向明显是"Room 应该可搜"。
3. **会话中心搜索的实现机制**："会话中心里的搜索功能是怎么搜索"——读代码回答：
   SQL LIKE？FTS？搜标题还是搜内容？
4. **与 deja/ctx 的定位差异 + subagent/tool 可见性**："和 deja 以及 ctx 的区别是啥。
   对 subagent 以及 tool 都能搜索吗"——deja=用户的 deja-search skill（搜历史 AI 编码
   会话）；ctx=用户队列里的"调研 ctx 和 ccas 同类项目"（会话搜索工具）。调查：会话
   中心搜索能否搜到 subagent 消息、tool 调用内容；与 deja/ctx 的能力边界对比，给用户
   一张"什么搜索找什么"的对照表。

**执行建议**：这是纯调研（explore-k3，只读），产出一份中文报告给用户过目，**不直接
实施**。与 P1（@ 补全性能）是不同问题：P1 是 @ 面板的文件/会话候选搜索，本项是会话
内容的搜索体系。

### P3：待回复面板定位（我的建议，用户未最终拍板但方向认可）

**背景**：用户在会话里质疑"待回复是什么鬼，这个不就应该直接列在会话中心里的一个
过滤吗，为啥单独有一个？是不是没必要？"
**我的建议（待用户确认）**：侧栏"待回复"独立行撤掉，改成**会话中心入口按钮上的琥珀
徽章**（数字+点击直接进入 needs_reply 过滤视图）——既不重复又保留一眼可见。
**另一个用户质疑待答**："既然有 agent 没回话，系统为啥没有提醒？"——现状是 Room/
私信有未读标记但无主动 push，这是既有设计（协作场景待回复是常态），实施时向用户
解释清楚或评估是否加提醒。

### P4：会话中心过滤条件拥挤（用户明确要求优化）

**用户原话**："会话中心里各种过滤条件现在非常拥挤，建议优化一下"
**方向**：过滤收进过滤器面板/快捷标签页，常用过滤（未读/待回复）做成 Tab。
先出设计稿给用户看再实施。

### P5：两个已拍板的小项

- **G5-11 徽章聚合 Room 欠回复**：用户已说"都补一下"=**做**。后端 unread_overview
  只数 visibility='direct'（collaboration_service.rs:2181），Room 侧 host 聚合硬编码
  "0"（collaboration_room_service.rs:394）。需后端补 Room 欠回复聚合进 unread_overview，
  前端徽章自动跟上。⚠️ 注意与 P2 的关系：如果 P2 改成徽章方案，这个聚合就是给徽章供数。
- **visibleCollectionSessionIds 死代码**：用户已说"都补一下"=**删**。collection-tree
  已迁到 visibleCollectionItemKeys，旧函数只剩自测试引用（w-sidebar-ux-2 报告确认）。
  删函数+其测试。⚠️ 删除类改动注意 memory 里记的坑：rg 裸名调用+pub(crate) 收紧。

### P6：打磨件（编排自己跑，不派工）

- 全仓 eslint/prettier 扫尾（一条命令）
- dev 任务栏图标角标（icon-dev.png + set_icon）

### 冻结不动

fork/rewind、workflow 功能、hooks 注入。

## 4. 门禁套餐（照抄；输出重定向+逐段 echo EXIT:$?，只认显式标记）

```bash
cd src-tauri && cargo fmt        # 工人惯性不带格式，合并后必跑
cargo clippy --all-targets --features test-utils -- -D warnings
cargo clippy --no-default-features --bin codeg-server --lib -- -D warnings
cargo clippy --no-default-features --bin codeg-mcp -- -D warnings
cargo test --no-default-features --bin codeg-server --lib   # 预期 ≥2543
cd .. && pnpm vitest run && pnpm build                      # 预期 362 文件/4600 用例
```

## 5. 环境雷区（新旧全部有效）

1. 开场 gitStatus 快照是过期的——任何状态断言现跑命令。
2. 后台命令 cwd 漂移：一律绝对路径。
3. 桌面 `cargo test --features test-utils` 二进制启动即崩 0xc0000139（老问题）——
   记"编译成功+运行 BLOCKED"，绝不假绿。
4. CDP 验收：Radix 菜单要真实鼠标事件；探针脚本一律用 Write 工具写（bash heredoc
   吃 `\\n` 转义）；dev 实例启动命令见旧单 §8.4。
5. **K3 工人可能用 fixup+autosquash rebase**——提交号会全变，验收前以分支当前
   `git log` 为准，别拿旧 hash 核对（本会话踩过：误判工人没修 readonly）。
6. **K3 工人可能不写 cd 直接跑脚本**——本会话有工人把 i18n 键误写进主仓（已自愈），
   合并前必须核主仓 `git status` 干净。
7. 合并冲突高发点：collection-tree.tsx（多批次都碰）、i18n 十语言 json。
   解法先例：renderDraftSession 与 bulkSelectionMenuItems 并存、useTabActions 解构
   取并集。

## 6. 给用户的第一句话素材

- IME bug 修复进展；
- @ 搜索缓存调研结论（Claude Code/Codex 到底怎么做的）；
- 搜索体系四问的回答（全局搜索范围/Room 可搜/实现机制/deja+ctx 对照表）；
- 待回复徽章方案 + 会话中心过滤优化设计稿（给用户过目）。
