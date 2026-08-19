# 交接单：可维护性收敛计划 · 领导会话换棒（2026-08-19 傍晚）

> **【17:1x 更新：交接未发生】** Fable 额度恢复、原领导会话回归继续。本单保留作
> 当时点快照；此后的进展以 MAINTAINABILITY-PROGRAM §5 进度日志为准（dev 实例已
> 复活、T2 已关账、子代理舰队已按用户令换 Opus 5）。

> 前一任领导（Fable）因主会话模型额度耗尽交接。**交棒时状态已刷新到 54cd5cac**
> （w-archive 批次全部合并+门禁全绿，见 §1 更新）。先读本单 → 再读
> `MAINTAINABILITY-PROGRAM-2026-08-19.zh-CN.md`（事实源）→ 再动代码。

## 0. 你是谁、你的职责边界（用户亲口定过的，不许越界）

- 用户对你的原话：**"你是领导者设计者规划者"**、"你自己干活太贵了"。你只干四件事：
  设计/裁决、审稿验收、串行合并、跑门禁。所有写代码/写文档/跑调查的执行一律开
  worker-k3（Kimi K3，~/.claude/agents/worker-k3.md，model 钉 k3[1m]）。读代码可以
  自己读（审稿是领导职责），写代码不许自己动手（合并时解冲突、修一行级集成小错除外）。
- 探索类用 explore-k3（只读审计），执行类用 worker-k3（可写）。
- **主会话不是 Fable 时（现在就不是），子代理省略 model 也行，但惯例是显式
  `subagent_type: "worker-k3"`**。额度告急时可将 k3[1m] 临时降 k3-256k（子代理实测
  很少超 256k），额度恢复即改回——备忘在用户全局 CLAUDE.md。
- 沟通风格：对用户用大白话+具体场景（用户明确投诉过架构黑话），别堆代号缩写，
  引用代码用 `文件:行号`。

## 1. 当前快照（交棒时现跑命令核实过）

- 主分支 `codex/session-message-v1` @ **54cd5cac**，工作区干净，**未 push**（永远
  不 push 到 origin；fork remote 是 sessionmux，也别 push，除非用户明确说）。
- 门禁最近一次全绿（54cd5cac 时点）：server lib 测试 **2464 过/0 挂/1 忽略**；
  三条 clippy 全过；vitest 348 文件/4350 用例全过（CPU 争抢下会闪失败，红了先复跑）；
  pnpm build 成功。
- 已合并的工人批次（全在链上，门禁绿色）：w-playbooks（2950aaa8）、
  w-goal-resume（8e9be7d4）、w-g9-hostctl（7cd2b12e）、w-automation（79993c0b）、
  w-archive（e672b386+54cd5cac，含任务 1-8 全部）、w-acp-survey（c4fb1820+abe7f5c8，
  含 desktop-cc-gui 案例研究 §8）。
- 桌面模式 `cargo test --features test-utils` 的测试二进制启动即崩（0xc0000139，
  根因未查明）——按手册记"编译成功+运行 BLOCKED"，绝不假绿。
- 后台老任务：全仓 eslint 还在跑（task be4q7oxmn，已知 28 个存量老问题零新增；
  结果出来后收敛动作=全仓 prettier 一次性格式化，见 §4 的 G4 小批次）。别再起第二份。
- 在飞的工人：见 §3。

## 2. 今天已交付落库的（全在 3a96c5b7 链上，门禁绿色）

- `3c86beae` 义务提示只统计 state='embedded' 的投递（欠账提示不再把"还在路上的信"
  复述一遍）。
- `a2679456` 三模式 clippy 存量清零（13+1 条）。
- `8e9be7d4` Goal 卡片"继续"按钮（codex 限定，把目标文本作 /goal 提示重排队）+
  Room 命名空间 16 个孤儿 i18n 键删除（10 语言各 160 行）。
- `7cd2b12e` G9：host 可调 session 模型/思考力度（session.get/set_selectors，语义键
  pin 胜过前端 localStorage 模板——**逃生门口径：host 主导、不加，记录在案**）。
- `79993c0b` 自动化新动作 queue_prompt（到点给 session 排提示，队列中间档）+
  conversation.created_by 列（user/agent/automation，两个新 migration
  m20260819_000001/000002）。
- `c4fb1820` ACP 生态调研入库（docs/session-workbench/ACP-ECOSYSTEM-ADAPTER-SURVEY-
  2026-08-19.zh-CN.md；含 steer 支持矩阵 §3：wire 面真支持的只有 claude/codex）。
- `2950aaa8` 两份剧本：lead-executor-split 加 Fleet hygiene 节 + 新 shadow-auditor.md。

## 3. 在飞的工人（名字可继续 SendMessage 唤起，带上下文）

| 工人 | 状态 | 位置 |
|---|---|---|
| **t2-capture** | 待命等 3000 复活：复采 Escape 三连截（脚本 `_current-esc.mjs` 已备好在 .artifacts/desktop-validation/maintainability-program-2026-08-19/） | 无 worktree |

已收工（全部合并落库、门禁绿色、可复用）：w-playbooks、w-goal-resume、
w-g9-hostctl、w-automation、w-archive、w-acp-survey、gate-finisher。

## 4. 待办队列（按序，含已做裁决，别重新裁决）

1. **【等用户授权，问过没答】重启挂死的 next dev（pid 63392，端口 3000）**。
   证据包（t2-capture 16:09 报送）：3000 LISTENING 但 HTTP 永久无响应 40+ 分钟，
   codeg 本体（pid 65788）和 9222 都健康。**杀 63392 → `pnpm tauri dev` 重拉**。
   授权到手后先杀再起，然后叫 t2-capture 补拍 Escape 取证收尾 T2。
2. ~~**w-archive 整批合并**~~ **已完成**：e672b386+54cd5cac 已合并，门禁全绿
   （2464 过/0 挂）。任务 1-8 全部落库。
3. **来源过滤 UI（G13-c 前端）**：地基 created_by 已合并。派工：侧栏/会话中心加
   来源筛选（只看用户开的/隐藏 agent 开的），10 语言 i18n。
4. **G4 小批次**（等 w-archive 落地后派，同一片文件）：两个孤儿函数
   （outbound_awaiting_summary / latest_mailbox_info_at）、CollaborationDeliveryState
   补 as_str、催办重复 wake 去重、mail_box/scope 命名对齐、G4-4 三处"名不副实"注释。
   同批带上全仓 prettier 收敛（等 eslint 结果出来后做）。
5. **第二轮孤儿 i18n 键**：约 147 候选（Collaboration.* 48 个大头）。w-goal-resume
   的审计脚本在 `codeg-wt/goal-resume/scripts/orphan-i18n-audit.cjs`（未跟踪）。
   关键坑：动态拼接 `t(\`ns.${var}\`)` 会吞字面量键，命名空间在 JS 模板字符串里
   时直接保留该空间。**建议主会话先亲自把命名空间分布摸清再派工**（我正在做的
   事，未完成——步骤：①`rg useTranslations\\(\\\"` 全量名单；②动态 t() 热点文件
   acp-agent-settings.tsx(5 处)/quick-actions.tsx(4 处)/appearance-settings.tsx(2 处)
   的前缀摸出；③凡有动态命中的命名空间整个标记"保留"；④其余交 worker 删）。
6. **【用户新需求，未派工】跨 harness 的用户级配置文件管理调研**（用户原话：切换
   多个 harness 时 claude.md/agents.md 等容易不一致；也承认不能强求一致，比如
   claude.md 里的 subagent 用法对其他 harness 无效）。派一个调研工：各家 harness
   的用户级/项目级指令文件约定盘点（claude CLAUDE.md、codex AGENTS.md、gemini
   GEMINI.md、cursor .cursorrules 等）× codeg 现有注入机制（experts/skills、MCP
   配置注入、prompt 模板）× 可行的"写一份多处生效"方案（symlink？codeg 生成
   各格式？），产出建议不做实现。
7. **【永远最后做】G11 上游合并**：origin/main（xintaofei/codeg）自 ea5177ea 起
   27 个提交。worktree 试合 → 预期文本冲突在 connection.rs/conversation_service/
   parsers/i18n/message-input；**遇设计冲突（不是文本冲突）立刻停下写分析等用户
   拍板**——用户原话"必须思考且经过我的同意才能合并"。上游基线勘察在 worktree
   `codeg-upstream-main`（detached 84274701）。

## 5. 用户拍板过的"以后再说"栏（别主动翻案）

- **G10 fork/rewind 实施**：整体暂缓（"这个再说吧，估计比较复杂"）。四步方案
  （底座/claude _meta 私约/codex app-server/重放降级）和 desktop-cc-gui 调研结果
  都在库里等重启日。
- **workflow 功能**：整体暂缓含 RFC。
- **hooks 注入**：维持凌晨"不引入"定案。steer 矩阵证明跨家中途必达协议层短期
  无解（claude/codex 两家私约可用、不支持的家硬塞会产无主回合 #934）；现有阶梯
  （排队保底+high 自动 steer+显式 send_interrupt）是当前最优。可推动的低成本项：
  给 kimi 上游 issue #2370 站台。
- **ACP schema 0.11.7→1.6.0 升级评估**：ACP 调研 §7.5 建议单独立项，未排。

## 6. 环境雷区（全是今天踩过的，别再踩）

1. **会话开场注入的 git status 快照是过期的**——任何工作区状态断言必须现跑命令。
   K3 工人已因此虚构过一次核验（已写入 memory：k3-worker-verification-discipline）。
   派工话术里务必带这句预防针。
2. **管道输出过滤会吃结果**：`| tail` 截管道会丢失败退出码；`pnpm vitest run` 的
   stderr 噪音会把 grep 窗口挤出关键行。**门禁一律输出重定向到文件、逐阶段显式
   `echo EXIT:$?`、完事再 grep 文件**（今天的正确姿势见 §1 各数字的出处）。
3. **主工作区任何 rust 源文件改动会触发 `cargo tauri dev` 重编译重启 dev 实例**
   ——watcher 触发会连带把前端热更链搞挂（今天 3000 就是这么挂死的）。rust 施工
   窗口内 T2 截图类活一律让 t2 待命。
4. **不许杀任何 cargo/tauri/node dev 进程**（用户红线；3000 那次是唯一例外申请）。
5. **isolation:worktree 的基线是陈旧 ref**——派工第一句必须是
   `git worktree add ... -b wt/xxx codex/session-message-v1` 显式钉基线。
6. **K3 不报完工就催**（memory 记过：K3 工人完工不交稿要催）；催稿用 SendMessage
   按名字唤起即可。
7. vitest 在 CPU 争抢下闪失败——红了先单测复跑再定性。
8. 合并工人分支时**预期**的冲突点：i18n 十个 json（多个前端批次都碰）、
   collaboration_service.rs / collaboration_room_service.rs（a2679456 lint 清账动过）、
   models/prompt_queue.rs（Default derive vs 枚举加成员）。解法先例：注释取新文案、
   derive 保留 Default（枚举身上已有 #[default] 标注）。

## 7. 合并后的标准门禁套餐（照抄）

```bash
cd src-tauri && cargo fmt
cargo clippy --all-targets --features test-utils -- -D warnings   # desktop
cargo clippy --no-default-features --bin codeg-server --lib -- -D warnings
cargo clippy --no-default-features --bin codeg-mcp -- -D warnings
cargo test --no-default-features --bin codeg-server --lib          # 预期 ≥2453 全绿
cd .. && pnpm vitest run && pnpm build                             # 涉前端才需要
```

每步输出重定向到文件、echo 退出码。全绿后提交，progress log 记进 MAINTAINABILITY-
PROGRAM §5 并单独提交一笔 docs commit。

## 8. 用户等待中的回话（接班后第一时间处理）

- 3000 重启授权（§4.1，已问过，等"可以"）。
- G9 逃生门口径（§2：host 主导不加逃生门）——用户无异议即可，不用主动再问。
- steer/hooks 意见已完整回复过（维持阶梯、不上 hooks），用户未表态反对，视为接受。
