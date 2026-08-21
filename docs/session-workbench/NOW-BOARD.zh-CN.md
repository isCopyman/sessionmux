# TODO 作战板

> 这是领导会话的**待办清单**（没有 todo 工具，所以清单落在文件里，好处是跨会话不丢、
> 用户随时能翻）。规则：每次醒来先读这里，收尾最后一件事更新这里。
> 只记"现在"，做完的移进 DOGFOODING-LOG 台账，不在这里堆历史。
>
> 状态记号：`[ ]` 待办 / `[~]` 进行中 / `[x]` 本批已完成待归档 / `[?]` 等用户拍板

最后更新：2026-08-21 15:40（配置模型定稿：只留「跟随 CLI」和「codeg 档」）

## 进行中（派出去了，等回收）

- [~] **O69-A** — 档吃下整份 settings.json + 从现成 settings.json 单向导入 +
      把 agent 全局连接配置迁进一个档 + 退休 `official-direct`
      （codex `gpt-5.6-sol` `xhigh`，lane `.cli-delegate/worktrees/profile-own`）
- [~] dev 应用重启中（我用 CDP 改主窗口地址栏把它弄挂了，见 L11）

## 待办（我自己排的，不需要用户点头）

- [ ] **O69-B**（等 O69-A 契约）：档编辑器接 `settingsJson` 折叠编辑区 +
      `+` 菜单加「从现有配置导入」；Claude 的全局连接块整块删掉
- [ ] **冷启动白屏**：Tauri 窗口先弹出来、前端还在 turbopack 编译（实测 compile 13.5s
      时白屏 16 秒），用户以为坏了。修法：窗口先挂骨架/加载态，别给空白 webview
- [ ] 实机走查：O51/O56/O57/O61 建群全链路 + 今天的 O67 看板三件 + O68 配置层级
- [ ] `model_provider` 绑定仍会把连接键写回 `env_json`（`acp.rs:10670` 一带）——
      O69-A 若确认，下一批堵掉；否则迁移完还会被重新污染
- [ ] 档第二期：`claude_profile` → `launch_profile` 改名 + 加 `agentType`，
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

- [x] **O66-A** 档拥有连接 env + 防御清扫（订阅档不再被全局 token 打穿）
- [x] **O66-C / O68** 配置层级自明：CLI 全局设置改成「跟随默认」页签的正文；
      虚拟档走后端 `isVirtual`；档的 kind 选择器删掉
- [x] **O67** 看板三件：分组维度、attention 列内分层、卡片活动点
- [x] **配置模型定稿** `CONFIG-MODEL-2026-08-21`：只留两种概念，逐条记了删哪层为什么删
- [x] `envVarsScope` 文案**说反了**（写成 overlay 赢配置文件，实际是文件赢）——十语改正
- [x] Claude 的 env 覆盖层折叠（它和 settings.json 的 env 块是同一批键）
- [x] 磁盘卫生：合并完的 8 棵 worktree 全删（含 11G 的 claude-profile），23 个 wt/* 分支清掉
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
