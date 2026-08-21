# TODO 作战板

> 这是领导会话的**待办清单**（没有 todo 工具，所以清单落在文件里，好处是跨会话不丢、
> 用户随时能翻）。规则：每次醒来先读这里，收尾最后一件事更新这里。
> 只记"现在"，做完的移进 DOGFOODING-LOG 台账，不在这里堆历史。
>
> 状态记号：`[ ]` 待办 / `[~]` 进行中 / `[x]` 本批已完成待归档 / `[?]` 等用户拍板

最后更新：2026-08-21 14:05（档后端合并、UI 与 MCP 两路派出后）

## 进行中（派出去了，等回收）

- [~] **O59-B** — 档的**前端**：组合框 chip（裸档名 + 下拉第一行写控件名）+ 设置页目录
      （grok via cli-delegate，worktree `codeg-wt/claude-profile-ui`）
- [~] **O59-C** — 档对 **agent 可见**：`list_profiles` 工具 + `create_work_task` /
      `create_automation` 加 `profile` / `model`（worktree `codeg-wt/profile-mcp`）
- [~] 主仓 Rust 权威门禁（档后端合并后）：FMT/CLIPPY 已 0，桌面 test 跑着

## 待办（我自己排的，不需要用户点头）

- [ ] 实机走查 O51/O56/O57/O61（建群→拉人→加入群聊全链路，CDP）
- [ ] 看板 RFC 按 O46 调研结论修订：**一级容器＝项目 Folder**（不是 Collection/Room/工作台），
      列沿用现成 `WorkTaskStatus` 四列；并更正"看板尚不存在"的旧表述——**它已经存在**
- [ ] 档第二期（等第一期落稳）：`claude_profile` → `launch_profile` 改名 + 加 `agentType`，
      按 `agent_root_slots` 查表支持 Codex 等（RFC §5.6 已写清成本）

## 等用户拍板（不许自己动）

- [?] **GitHub**：fork（必须公开）还是自建私库？现在 origin 是上游主仓，一次没推过
- [?] **Claude 档 RFC 剩余项**：凭据存哪（已按"托管档目录内 0600"落地，待追认）
- [?] **看板 RFC A–F**：一期已按建议值施工，其余等确认
- [?] **O47 建议 C**：后端重启后滞留的队列催办要不要禁止自动重放
- [?] **schema 手术批**：human 一等公民化等（模型体检 RFC ②③①）
- [?] **旧 worktree 处置**：sessionmux-session-timer、agent-a31e34…
- [?] **产品改名**

## 本批已完成（下次收尾时归档进台账）

- [x] O8 瞬时失败不杀连接 + Retry ｜ O48 群帖逐步披露 ｜ O51 先建群后拉人
- [x] O52 会话中心最近活动排序 ｜ O53 群聊每条消息带时间
- [x] O55 配置 chip 通用值带名字 → **用户否决，已改**：chip 恢复裸值，
      控件名移进下拉第一行（占位更省）
- [x] O56 共享 SessionPicker + membership 服务（顺带修 hydration button 套 button）
- [x] O57 会话面板「加入群聊」｜ O61 侧栏批量「加入群聊 ▾」（内含新建）
- [x] O58 多选可发现性：右键菜单「选择」+ 顺带教会 Ctrl/⌘+点击
- [x] O54 分类树文件夹右键「新建群聊」（带文件夹作用域，已合并 + 修了一个源码锚点脆测试）
- [x] O59-A 档后端：`CLAUDE_CONFIG_DIR` 注入、`__codeg_profile__` 会话绑定、
      停写用户 `~/.claude/settings.json`（零 schema 改动）
- [x] O62 看板 To-do 拖拽在"全部项目"下静默失效 → 说清原因
- [x] O60 会话中心筛选行冒出竖滚动条（`overflow-x-auto` 的 CSS 陷阱）
- [x] O47 复查二挖：不是空转，是协同税定价过高（唤醒分级降级不做）
- [x] O46 看板组织维度调研（grok 只读，已存档并更正其中一条过时结论）
- [x] 知识体系：README 索引 + LESSONS 教训库 + 本清单，CLAUDE.md/AGENTS.md 指路
- [x] 委派通道换成 cli-delegate（grok-bridge 的 runs 注册表实测不可靠）
