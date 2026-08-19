# 交接单：可维护性收敛计划 · 额度耗尽收官（2026-08-20 凌晨）

> 本会话（Fable 领导）跑完了通宵批次，额度见底时用户令：停工人、写交接。
> 先读本单 → 再读 `MAINTAINABILITY-PROGRAM-2026-08-19.zh-CN.md` §5（事实源，
> 本会话追加了 ~15 条决策/关账记录）→ 再动代码。

## 0. 角色边界（用户铁律，不许越界）

- 你是**领导者/设计者/规划者**：只做诊断、设计、裁决、验收、串行合并、集中门禁。
  一切执行（写代码/写文档/调研）派工人：`subagent_type: "general-purpose"` +
  `model: "sonnet"`（本会话全程用这个组合；fable 绝不当子代理）。
- 派工简报必带：worktree 正斜杠命令钉基线（`git -C <主仓> worktree add
  ../codeg-wt/<名> -b wt/<名> codex/session-message-v1`）、开场 gitStatus 过期
  警告、**禁止编译/运行/跑测试/杀进程**、证据必须 file:line、防复活黑名单
  （旧委派工作流/ACP replay/timer autoPaused/旧 CollaborationUnreadBadge/list
  shim/outbound_awaiting_summary）、**禁止向主仓写任何文件（含探针）**（有工人写过
  `.scratch_persistence_test` 到主仓根，已删）。
- 对用户说大白话，代码引用 `文件:行号`。永不 push origin。不杀用户的
  cargo/tauri/node 进程（自己起的调查实例可清，认路径 target\debug + [DEV] 标题）。
- **提速模式（用户嫌慢后定的现行操作法）**：排队项尽量并行派工；交货后攒 2-3 批
  一起合并、共享一轮门禁；真机验证只在发版前做一次总的。

## 1. 当前快照（交棒时现跑命令核实）

- `codex/session-message-v1` @ **9be2c4a0**，工作树干净，未 push。
- **⚠️ 接班第一件事：qoder 批（44936f7b 合并 + 9be2c4a0 fmt）是本会话唯一
  「已合并、门禁未跑」的批次**——额度断在门禁前。先跑 §7 全套门禁；红了按错误
  修（工人报告质量高、静态自检过，预期小错为主），实在不行 revert 这两笔。
  绿了才能打 release #6。
- 门禁最近一次全绿基线（qoder 合并前，d8ffafcf 时点）：server 测试 **2515 过/1 忽略**、
  clippy ×3 零警告、vitest **359 文件**全过、build 过。qoder 合并后这些数字会涨
  （parser 自带 ~20 测试）。
- 桌面 `cargo test --features test-utils` 测试二进制启动即崩 0xc0000139（老问题）
  ——记"编译成功+运行 BLOCKED"，绝不假绿。

## 2. 本会话已交付且门禁全绿的（细节全在台账 §5）

- **Tier-1 稳定性三件套**：① 卡"回复中"修复（上游 1e3e5a10 逐字节重放+17 回归
  测试，bb689da1）；② codex 标题 DB 单一事实源（同步进列表读路径+改名锁+频道
  传播摘挂收敛，95300618；免迁移，title_locked 早已有）；③ ACP 绑定两洞修复
  （bind_external_id 抢锁事务、先绑后宣、冲突拆路由、header durability；审计报告
  ACP-BINDING-AUDIT-2026-08-19.zh-CN.md）。
- **体验批**：Room 附加路径第一批（collaboration_room_path 表+多 root 扫描共享
  50k 预算+10s deadline+「…」菜单管理对话框，CDP 真机端到端验证过）；窗格临时
  放大（视图层标记不动布局树）；删定时器/移成员确认弹窗；成员数按钮开面板。
- **上游三小件**（六道门禁一把绿）：grok 真实 token 用量（context/usage 分离）、
  侧栏右键"添加到会话"、@ 面板锚定 composer+同宽+加载态（tabOrder/房间三组/key
  tiebreaker/3s 降级四项既有行为全保留，工人逐行号复核过）。
- **上游消化进度：qoder 批门禁过后即全部消化完毕**（批 1 直拿 11 提交、标题链、
  绑定审计+修复、三小件、qoder 均落地；881a2d06 版本号跳过）。
- 换库已执行：release 现在用 dev 全量数据（旧库备份在
  `C:/Users/63036/AppData/Roaming/app.codeg/backup-release-db-2026-08-20-0016/`）。

## 3. release 状态

- 用户手里是 **release #5**（03:13 构建，113MB，`src-tauri/target/release/codeg.exe`）
  ：含 Tier-1 + 体验批全部。
- **三小件批和 qoder 批还没进任何 release** → qoder 门禁绿后
  `pnpm tauri build --no-bundle` 打 **#6** 通知用户换用（输出重定向+EXIT 标记老规矩）。

## 4. 中断现场（额度墙时刻）

- **w-i18n-sweep**：被停时留了**未提交半成品**在 `../codeg-wt/i18n-sweep`
  （ar/de/es/fr/ja/ko 六个语言文件已改，pt 和扫描报告没做完）。处置二选一：
  ① 进 worktree 审阅这六个文件的翻译质量，补 pt+报告后收编；② 整个丢弃
  （`git -C ../codeg-wt/i18n-sweep checkout .` 后 remove worktree）重派。任务背景：
  wakeHint 8 语言、Room 成员管理键组 7 语言是已知欠账，另有全量占位扫描。
- **w-sidebar-ux / w-playbook**：派了但一行没写就被停，worktree 已清，**需原样
  重派**。简报要点在台账（sidebar-ux = G5-11 谁欠我回复全局入口 + G5-5 建群入口
  显眼化 + G5-12 Room 进多选；playbook = 给 agent 看的协作手册，逐工具从
  codeg-mcp tool_schema 实证，含"什么时候 @human"纪律）。

## 5. 待办队列（按序，含已裁决项，别翻案）

1. **qoder 门禁 →（红则修）→ release #6 → 通知用户**（§1）。
2. i18n 半成品处置（§4）。
3. 重派 sidebar-ux、playbook（§4）。
4. 附加路径第二批（可缓）：截断原因标记、Session 侧对等、菜单打磨。
5. 全仓 eslint/prettier 扫尾（编排自己跑，一条命令）+ dev 任务栏图标角标
   （icon-dev.png + set_icon，调查结论可做）。
6. **等用户点头**：指令文件体检扫描范围——已提议"默认只扫工作区、用户级目录做
   显式开关"，用户未回。
7. 冻结不动：fork/rewind（研究已归档，attribute-fork 问题立案待 RFC）、
   workflow 功能、hooks 注入。

## 6. 本会话新增裁决（已落台账，不许重新裁决）

- **@ 扫描超时 10 秒放后端** walk 循环内（与 50k 硬顶并列第二跳出条件），前端
  3s 降级保留——前端超时不取消底层调用治不了根因（用户建议+侦察确认）。
- **全局目录（~/.claude 等）不自动入 @ 范围**——Room 是共享空间，隐私语境不同；
  想要走手动加路径同一入口（用户委托后定案）。
- **paseo"有附加路径功能"系用户记忆混淆**：其 additionalDirectories 是 Claude
  Agent SDK 沙箱权限字段，UI 零命中（编排亲验）。
- frecency/前缀索引/fs 监听/完整取消令牌链**不做**（50k 量级用不上）。
- update_external_id 保留 pub(crate) 仅供"内建 agent 死 id 纠偏"一个调用方
  （过 bind_external_id 拆分逻辑会造幽灵行——注释写死了理由，别"顺手清理"）。
- DbError::Conflict（CAS 可重试）与上游同名变体语义相反——绑定类冲突用
  ExternalIdTaken，永远别混。

## 7. 门禁套餐（照抄；输出重定向到 scratchpad 文件+逐段 `echo EXIT:$?`，完事 grep 文件——任务级退出码会被收尾命令污染，**只认显式标记**）

```bash
cd src-tauri && cargo fmt        # 工人惯性不带格式，合并后必跑
cargo clippy --all-targets --features test-utils -- -D warnings
cargo clippy --no-default-features --bin codeg-server --lib -- -D warnings
cargo clippy --no-default-features --bin codeg-mcp -- -D warnings
cargo test --no-default-features --bin codeg-server --lib   # 预期 ≥2515
cd .. && pnpm vitest run && pnpm build                      # 涉前端才需要；vitest 闪红先复跑
```

## 8. 环境雷区（本会话新踩的，加上旧单 §6 仍然全部有效）

1. **限额墙**：工人撞限额是"出发即死、零产出"，可能留下空 worktree——清掉原简报
   重派即可，别当丢活。限额重置时间看报错信息。
2. **CDP 验收**：Radix 菜单必须真实鼠标事件（Input.dispatchMouseEvent 三连），
   合成 click 打不开；页面有十几个 aria-haspopup 按钮，按几何+title 定位（房间
   "…"=「更多操作」y≈92），别拿"最后一个"；**探针脚本一律用 Write 工具写**，
   bash heredoc 会吃 `\\n` 转义把真换行塞进正则。
3. **后台命令 cwd 漂移**：前台 `cd` 会改会话 cwd，后台命令里的相对 `cd` 会因此
   失败且 `&&` 链静默断——一律绝对路径。
4. dev 实例验收流程：`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"
   pnpm tauri dev --no-watch --config '{"build":{"beforeDevCommand":""}}'`（后台）+
   curl 轮询 9222 就绪；用完 TaskStop + 杀 target\debug 孤儿（认路径认 [DEV] 题）。
   `pnpm dev`（next，3000）常驻别动。验收脚本库在
   `.artifacts/desktop-validation/maintainability-program-2026-08-19/`（_cdp.mjs 是
   连接助手，c.errors 是数组）。
5. 工人交货没报告只有 idle 心跳时：先查分支有没有提交（可能只是报告没送到），
   SendMessage 催补报告，别急着当失败重派。

## 9. 接班后给用户的第一句话素材

- qoder 门禁结果 + release #6 是否就绪；
- 三个被停工人的重派计划；
- 指令文件体检范围还等他点头（§5.6）。
