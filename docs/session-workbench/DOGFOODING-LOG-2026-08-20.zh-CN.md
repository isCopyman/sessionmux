# Dogfooding 日志：用 codeg 开发 codeg（2026-08-20 起）

> P7 阶段的活文档。领导以人类身份通过 CDP 操作 dev 实例，用 codeg 自身的持久化
> 多 agent 协作开发 fork/rewind。**每条不便之处都记在这里**，按"现象 → 影响 →
> 改进方向"三段式，攒够一批评审后立项。

## 环境

- dev 实例：tauri dev + CDP 9222，主线 @ 今晚批次收官后（含 Room 搜索/过滤重构/
  IME 修复/@ 性能包）。复用了已在跑的 next dev（3000）。
- 首个课题：fork/rewind 第一切片（简报 `FORK-REWIND-TASK1-BRIEF.zh-CN.md`，
  在 wt/fork-rewind worktree）。
- 主编码会话：Claude Code + claude-opus-5[1m] + Xhigh + Bypass Permissions，
  工作目录 fork-rewind（分支 wt/fork-rewind）。
- 协作模型（用户指定）：Grok Build + Grok 4.6 —— 作评审位，等主编码者出活后拉群。

## 观察记录

### O1. 好评：打开文件夹有应用内路径输入（Ctrl+O）

现象：`Ctrl+O` 文件夹对话框自带"输入目录路径..."文本框 + 目录浏览，原生选择器
只是快捷方式（workspace-folder-dialog.tsx:93 注释明说）。
影响：无头/自动化/远程场景全可用；这是 server 模式兼容性带来的正外部性。
方向：保持。

### O2. 两个"工作文件夹"控件同名不同性

现象：会话头部的文件夹面包屑和 composer 下方的文件夹切换器，title 都是
"工作文件夹: X"。头部那个是静态标签（代码注释："never a popover trigger"），
底部才是真选择器（aria-haspopup=dialog）。
影响：肉眼难分；自动化按 title 找控件必踩坑（本次踩了 3 轮）。对用户来说，
点头部面包屑无反馈也算轻微困惑。
方向：头部面包屑去掉按钮语义（改 span）或 title 改成"所在文件夹"；给底部
选择器加稳定 data-testid。

### O3. 弹层触发在自动化点击下时灵时不灵

现象：composer 下方文件夹选择器，同一坐标的 CDP 真实鼠标点击第一次无反应
（aria-expanded 保持 false），键盘 focus+Enter 也无反应，改成"先点面板任意处
激活 → 再点触发器（带 100ms 间隔的 move/press/release）"才打开。
影响：普通用户大概率无感（真实鼠标有自然间隔），但说明触发器对事件序列敏感，
可能与面板激活状态有关；自动化测试会 flaky。
方向：排查 pane 激活逻辑是否吞第一次点击（focus-within 切换时）；给 E2E 留
"点击两次"惯例或修根因。

### O4. worktree 分支识别有延迟窗口

现象：切到 fork-rewind（git worktree，.git 是文件）后分支 chip 短暂显示
"无分支"，数秒后变 wt/fork-rewind。
影响：轻微；但如果用户在窗口期发消息，会话记录的 git_branch 会不会写空？待查。
方向：确认 branch 探测的异步时序，或首次探测完成前禁用发送位的 branch 记录。

### O5. 会话中心徽章/Room 搜索/过滤重构/Ctrl+K 手递手：实测全过

（属于今晚批次验收而非摩擦，记录在案：徽章=私信+Room 之和、点徽章直达
needs_reply 分段、筛选弹层 7 facet+Escape 逐层退、列表混排 Room 带"群聊"徽标、
Ctrl+K 无 agent chips、"在会话中心搜索 {query}"手递手预填成功。）

## 待办/跟踪

- [ ] 主编码会话完成第一切片 → 验收 diff → 拉 Grok 评审群
- [ ] O4 的 git_branch 写空疑问
- [ ] IME #518 真实输入法验证（单测已覆盖状态机；真 IME 需要人工敲一次中文，
      留给用户早上顺手敲一次）

### O6. 门禁与 dev 实例共用 target 目录会打架

现象：dev 实例运行时跑 `cargo clippy --all-targets --features test-utils`，
tauri-build 复制 sidecar（binaries/codeg-mcp-*.exe）时 PermissionDenied——
运行中进程锁着文件。
影响：dev 长驻 + 门禁并行的工作流（正是 dogfooding 形态）必撞。
方向：门禁惯例改用 `CARGO_TARGET_DIR=target-gate`（已实践）；或文档里写明。

### O7. 协调者体验（正面居多）

- Host Control 建会话/Collection/Room 一条龙顺畅；协调者还能自查自纠
  （标题错字、自己标题被截断）。
- 协调者主动做了"编队卫生"（协作者进独立 Collection）和"省回合"设计
  （建会话不带首 prompt，分派走 Room 帖）——工具面表达力够用。
- 待观察：Room 帖能否可靠唤醒 grok-4.6 协作者干活（@ 唤醒语义）、工人完工
  回报的闭环。

### O8. API 503 打断回合后没有自动重试

现象：协调者回合中途遇 "API Error: 503 No available accounts"（上游临时故障），
回合直接终止，底部红条提示"usually temporary — try again"，但系统不自动重试，
会话就地闲置；需要人工发一条"继续"才恢复。
影响：无人值守的多 agent 编队里，一次瞬时 503 就能让协调链停摆（本次停了约
10 分钟才被哨兵发现）。
方向：对 5xx/临时性错误加有限次自动重试（指数退避）；或至少把"上次回合因临时
错误中断"变成显式可恢复状态（一键续跑 + 计入待回复徽章）。

### O9.（严重）Room @ 投递克隆幽灵会话 + session.stop 管不到

协调者第五批摩擦实锤（Room event 7a30351f，证据链完整）：@ 空闲会话时投递路径
**创建新会话**而非唤醒目标——幽灵数量精确等于 mention 数、同毫秒诞生、首条 user
消息=Room 信封原文；幽灵误认身份、MCP 连接失败、单个烧 455k token（一次性，
非持续泄漏——协调者用 message_count 自纠了"持续烧钱"的误判）。@ 忙碌会话无此
现象（猜测走了"closed Session is started"分支）。session.stop 对其返回
"no active managed runtime"。事故样本 4 个会话按指令保留未删。
影响：多 agent 协作核心路径的正确性 + 真金白银的 token。
方向：已立项派修（w-phantom），根因锁定投递唤醒分支。
