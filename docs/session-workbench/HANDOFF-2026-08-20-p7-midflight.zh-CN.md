# 交接单：P7 dogfooding 飞行中快照（2026-08-20 傍晚，压缩前落盘）

> 供上下文压缩后的本会话（Fable 领导）继续。先读本单 → `DOGFOODING-LOG-2026-08-20.zh-CN.md`
> → `docs/session-workbench/fork-rewind-slices/README.zh-CN.md`（协调者台账，事实源）。
> 角色铁律不变：领导只做诊断/设计/裁决/验收/串行合并/集中门禁；执行派工人。
> 用户睡前指令仍有效：子代理用 opus/sonnet；细节我决断；完成必测试；不停不问。

## 0. 主线状态

- 分支 codex/session-message-v1 @ **64abed85**，树干净，未 push。
- 今晚批次（P0-P6+Room搜索+Ctrl+K）全部合并且门禁全绿（server 2560、vitest 4695、
  build、eslint 0 error、桌面 clippy 用 `CARGO_TARGET_DIR=target-gate` 隔离跑绿）。
- **封版纪律仍有效**：用户在用 release #8（不含今晚任何改动）；只有用户说"换版"
  才 `pnpm tauri build`。
- dev 实例在跑（tauri dev + CDP 9222，next dev 3000 是复用的旧进程，别杀）。
  fork/rewind 相关分支勿删。

## 1. P7 编队现状（codeg 内，用户拍板的模式）

- **协调者**：Opus 会话"fork-rewind 协调者"（claude-opus-5[1m]+Xhigh，
  目录 fork-rewind=wt/fork-rewind worktree）。上下文已用 ~44%，第三轮后考虑接力。
- **工人**：三个 grok-4.6 会话（316/317/318，Collection 5"fork-rewind 编队"），
  协调 Room rm_3d3711fe-d8ae-4ec5-9f55-1d45aba7b2b2。
- **第一轮已收口并由我合并主线**（4479d3a4）：三份审计（A 锚点盘点/B claude fork/
  C codex fork）。核心裁决：claude 按消息 fork 走 `_meta` **不需上游**；codex 需上游；
  print-lane 静态排雷。
- **第二轮在飞**：D 锚点管道（编码，session 316，分支 wt/fork-anchor-pipeline，
  切片 2 硬前置）/ E RFC 二轮更正（317，wt/fork-rfc-round2）/ F is_reserved_turn_id
  评估（318，wt/fork-reserved-id-audit，带"不许擅改安全逻辑"刹车）。
  规格 ROUND2-SPECS.zh-CN.md。协调者收口流程：逐份机械复核→串行合并回
  wt/fork-rewind→Room 收口→**等我验收**。
- **活体探针**：批的一发已用（未中：spawn 的 claude 继承本机代理 env
  ANTHROPIC_BASE_URL=127.0.0.1:8317 导致 turn 未完成；按预算纪律未重试）。静态部分
  已把 print-lane 贯通链证完。证据 jsonl 保留在 ~/.claude/projects/...fork-lane-probe。
- **我对协调者的裁决已下**：活体一发批（已用）；第二轮拆法认；硬规矩=工人绝不在
  协调者树内写；摩擦 9 事故样本原样保留（commit 8467a2f8 在历史里）。

## 2. 在飞的 Claude Code 子代理

- **w-phantom**（opus，worktree wt/phantom-wake @ 64abed85）：修 O9 幽灵会话
  bug。简报要点：Room @ 空闲会话→投递路径新建会话（N mention=N 幽灵、同毫秒、
  首条消息=信封原文、MCP 连不上、session.stop 返回 no active managed runtime）；
  @ 忙碌会话正常。修复语义（我定）：空闲时唤醒目标本身，绝不新建；不行则至少
  不新建（走既有提醒机制）；投递启动的 runtime 必须进 Host Control 管理。
  要求回归测试（server --lib 面）。**回报后：验收根因链→合并→门禁。**
- **w-skilldoc 未派**（worktree wt/skill-lessons 已建 @ 64abed85）：更新协作 skills
  文档（src-tauri/experts/skills/ 下 codeg-multi-agent / codeg-host-control 等），
  把实测教训写进去：① harness 取值域两套 id（registry_id_for 给 grok-build、实际收
  grok），列出正确值域；② "initial_prompt 带 room_id"做不到（鸡生蛋），改为
  "先建会话→建 Room→分派走 Room 帖"；③ 交付/记录帖用 expects_reply=false 防徽章
  噪音；④ 工人隔离军规（自建 worktree、禁 git add -A、cd 显式、pnpm install
  --frozen-lockfile）；⑤ 等 w-phantom 结论后补 wake 语义说明。10 语言 i18n 不涉及
  （skill 是英文/中文文档，按现有文件语言写）。

## 3. 哨兵与监控手法（压缩后照抄）

- 分支哨兵（最可靠）：轮询 `git rev-parse wt/fork-*` 变化，run_in_background。
  当前有一个在飞盯 D/E/F+协调者（batkw9r9o）。
- CDP 操作纪律：只用 DOM 查出的 CSS 坐标；Radix 弹层要真实鼠标序列
  （move→100ms→press→50ms→release），首击可能被面板激活吞掉（O3）；发消息=
  点 composer→Input.insertText→找同容器"发送"按钮点击；生成中发送键变停止键，
  要等 idle。截图 c.shot() 后必须 Read 图片核实再动。屏幕坐标≠CSS（DPR 1.5）。
- 协调者被 503 打断时（O8）：发一条"上游临时故障，继续"即恢复。

## 4. 摩擦/bug 总账（细节在 DOGFOODING-LOG O1-O9 + Room 四批帖）

严重：O9 幽灵会话（修复中）；O8 503 无自动重试；工作树无隔离强制（摩擦9 事故）。
待查：O3 弹层首击丢失；O4 worktree 分支识别延迟（git_branch 写空?）；
**新增 O10（未入日志）：dev 与 release 双实例同库但事件不互通**——用户在 release
侧看不到 dev 侧新建的 Room/会话，要重启/切工作台才刷新。记入日志待办。
流程类已修：eslint 三处产物目录 ignore、门禁独立 target、测试全量水分教训。

## 5. 压缩后第一步

1. 查 w-phantom 是否已回报（git -C ../codeg-wt/phantom-wake log）；验收→合并→门禁。
2. 派 w-skilldoc（简报见 §2）。
3. 查 batkw9r9o 哨兵输出，第二轮有活动就走验收流程（协调者收口在 Room 等我）。
4. 把 O10 补进 DOGFOODING-LOG。
5. 空闲时向用户汇报进展（大白话）。
