# 多 Agent 生态调研：openteams workflow / pi-shadow-mind / Multica

> 状态：2026-08-19 凌晨完成（G12-B2/B3 bonus 调研，网络取证由子代理执行，
> §4 裁决由编排会话撰写）。事实与出处在 §1-§3，任何实施都要先过 §4 的裁决与用户点头。

## 1. openteams（github.com/openteams-lab/openteams）

定位：Tauri 2 + Rust + React 的本地优先桌面应用，把 13 种 coding agent CLI（Claude Code、Codex、Gemini CLI、Qwen、OpenCode、Amp、Copilot、Cursor、Droid、Kimi Code、Qoder、Pi、自带 openteams-cli）拉进一个共享会话组队。文档站 doc.openteams-lab.com。migration 历史（execution_processes、task_attempts、shared_tasks 等表名）和 crate 结构与 vibe-kanban 完全同源，可判定是 vibe-kanban 的深度分叉。

### 1.1 Workflow 的形态

- 不是用户手写定义文件，也不是 UI 拖拽编排器起家：是"Lead agent 生成 JSON 计划 → 编译成 DAG → 人在卡片上审批 → 运行时按依赖波次执行"。
- 触发链：用户在会话里确认设计后，lead agent 首轮输出 `workflow_generate` 协议 → 后端给 lead 发"计划生成 prompt"（内嵌完整 JSON Schema + 图语义编译规则）→ lead 两段式输出（先 Markdown 草案，最后一个完整 JSON 对象被解析器提取）→ 校验+编译 → 聊天时间线生成 workflow 卡片，用户从卡片执行/重试。（crates/services/src/services/workflow/runtime/prompt_builders/plan_generation.rs）
- 计划格式：React Flow 兼容 JSON（WorkflowPlanJson）：{version, title, goal, agents:{lead, available[]}, globals:{interrupt_mode, default_retry(默认3,上限10), global_pause_supported}, viewport, nodes[], edges[], loops?, policies?}。节点 data：stepType(task/review/result)、agentId、title、instructions、acceptance（三级验收 required/partial/recommended）、outputs、selfCheck、verificationCommands、completionEvidence、interruptible、maxRetry、reviewScope。边只有 hard 依赖。（crates/db/src/models/workflow_types.rs、compiler/compiler.rs）
- 图语义硬规则（不过即编译失败）：ID 全图唯一；无环；必须且只能有一个 result 节点且无出边；agentId 必须在 agents.available 里；reviewScope 构成 Loop（被审 task 必须是 review 节点上游、路径不得穿过另一个 review、每个 task 最多属一个 Loop）。
- 执行：拓扑排序 + 波次并行（含同波次同 workspace 冲突检测）；步骤状态机 13 态（waiting_input/waiting_review/blocked/revising/skipped…）；审核者三类 Lead/Reviewer/User；用户可步骤级 interrupt/retry/skip/submit_input；一轮跑完进 round 的 waiting_user_acceptance，拒绝则带反馈重新生成计划（iteration）。
- UI：React Flow 节点图（WorkflowGraphBoard）+ 聊天卡片（待输入/待审核/终审/迭代反馈）。近期 hardening：prompt safety、预算上限、reviewer-loop 不变量、per-prompt 预算分配。

### 1.2 多 agent 协作组织

- 会话 = 共享工作区，人和 AI member 同一时间线；分派靠 @mention，agent 互相 @ 移交。消息四类：user / AI member / system / task。完整历史不进 prompt，落成 message.jsonl 让 agent 按需读。（docs/core-features/chat-session-overview.mdx）
- AI member 一等公民：角色+专属技能+独立 workspace+可绑不同模型；内置 160 个成员；8 个预置团队模板，团队可存 preset。"团队规则"（谁可直接对用户说话、禁 AI 闲聊）文档明说是软约束。（docs/advanced-usage/create-team.mdx）
- Team Protocol（v0.3.20 技术文档草稿）：agent 输出 JSON 数组，action 有 send(intent: request/reply/notify/blocker/confirm)、record（共享事实）、artifact、conclusion；反死循环靠 ChainDepth 传播深度限制。
- 执行隔离：并行任务跑独立 git worktree，可分别 review/merge/discard；另有 Issues（可同步 GitHub issue）作为人控工作项入口。

### 1.3 workflow 与自由群聊的关系：并存，同一会话两种模式

- Open mode（自由聊）：去中心化、@mention 互相挑战，用于脑暴，用户自己综合结论。
- Work mode（workflow）：会话不再承载自由消息流，变成任务执行入口；主时间线只剩需求确认、冲突升级、结果验收三类消息，过程细节收进日志/artifacts。文档原话：前者"强调探索与讨论"，后者"强调执行与交付"，"只看真正需要你决策的信息"。

## 2. pi-shadow-mind（linux.do t/2759945）

帖子：《GPT 写的烂代码终于找到根治手段了哈哈哈》，作者 BW_liu，2026-08-15，回复 98+。插件 github.com/liuzhengdongfortest/pi-shadow-mind（MIT，npm）。

- 机制：给 Pi CLI 起两个"shadow mind"在主 agent 干活时自动异步审计并"插话"：【code-smell-auditor】查坏味道，【delivery-alignment-auditor】核对原始需求。
- 触发：挂主 agent turn_end 事件，纯文本轮跳过；符合条件的轮次默认 1/3 概率发心跳，每个 shadow 再按自己 activation_probability 掷签，最多 2 并发。
- 隔离：每次激活起全新临时 session，继承主 agent system prompt，只看**脱敏轨迹**（去思考、工具结果压缩成摘要）；shadow 先自判相关性，不相关静默退出，有发现才调 report_to_main 插回主会话。
- 定义文件：~/.pi/agent/shadow-minds/ 下 Markdown（frontmatter: id/name/activation_probability/active_for_models/tools + prompt 正文）；给 shadow 配工具可从"只读举报"升级为并行干活（如维护文档）。Alt+S 暂停，/shadow 面板。
- 实测截图：主 agent 编辑交易项目代码时，🐙 shadow 块实时插入"routes_app.py 已成杂物抽屉，建议拆 routes_chat.py / routes_system.py"，主 agent 随后真的开拆；另有目标一致性审计报告（部分通过，列已满足/缺失项）。
- 讨论区关键教训：类似物 dsh 曾出现"子代理插话→主 agent 回复→子代理又插话"的 2-3 方循环，后修掉；连续插话影响大，设了 **300 秒插话 debuff 排队**；token 消耗是痛点（有人一天 1.95B）。作者对"这不就是 coder→reviewer 多 agent 工作流吗"的回应："你这样理解也没什么问题……一个是自动，一个是多重。"

## 3. Multica（github.com/multica-ai/multica）

把 agent 当同事管理——看板/任务队列，agent 自己 claim 任务、上报 blocker、实时更新状态；本地 daemon 自动探测已装 claude/codex/openclaw/opencode CLI；WebSocket 推流；PostgreSQL 存任务与"技能库"。与 openteams 的会话制不同，是 issue 队列制。

## 4. 编排会话裁决（2026-08-19，待用户晨间过目）

### 4.1 B2：codeg 要不要加 workflow？——**方向认可，现在不做，先立 RFC**

- 用户的直觉有依据：openteams 证明"自由群聊（Open mode）+ 编译执行面（Work mode）"可以并存互补，且 Work mode 的核心卖点——"只看真正需要你决策的信息"——正是 codeg 人机友好目标的延长线。今晚我们自己的通宵编排实质就是一个手工 workflow（拆包→并行执行→验收门→合并），需求真实存在。
- 但这是**大特性不是收敛项**：计划 JSON schema + DAG 编译器 + 13 态状态机 + 图 UI + 预算/重试 hardening，openteams 的补丁史说明尾巴很长。今晚的可维护性计划不该开这个口子。
- 若立项，codeg native 的形态**不应照抄**：codeg 已有 work_task（taskboard/worktree 编码流水线）和 automation 两个半执行面。正路是"Room 当共享看板 + work_task 引擎当执行器 + 计划对象渲染成 Room 卡片"，复用一张账本，而不是再造一条平行消息系统。这是 RFC 要回答的第一题（与既有 taskboard/automation 的归并关系）。
- **今晚可白捡的三件小事**（零代码/低代码）：① 三级验收词汇（required/partial/recommended）吸进 delegation brief 惯例（skill 层）；② openteams 的 ChainDepth 与我们的链深=4 保险丝互为独立佐证，记录之；③ worktree 隔离执行我们已是纪律，无需动作。

### 4.2 B3：pi-shadow-mind 是不是多 agent？——**是（用户直觉对），且 codeg 今天就能当 playbook 做**

- 定性：主 agent + 并行只读审计副 agent 的"影子审计"编排，作者自认与 coder→reviewer 工作流同构，差异在"自动触发"。
- codeg 映射：**插话原语我们已经有**——高优先级信件对忙碌会话的 steer 注入就是 report_to_main 的等价物；审计者可以是一个带 timer 的普通 Session，用 get_session_info 读主会话近况、用 normal/high 信件反馈。不需要任何引擎改动，写成 playbook 即可（候选名 shadow-auditor，**待用户点头再写**——用户点名的 playbook 清单里没有它）。
- codeg 缺的三件（如实记录，不建议现在补）：turn_end 钩子（今晚已裁决不上 hooks，3/13 harness 覆盖）、概率激活门、脱敏轨迹。用 timer 轮询近似 turn_end 是可行降级。
- 反向验证价值：dsh 的 2-3 方插话循环翻车 + 300s debuff，恰好印证我们已有的设计——每轮至多 steer 一次（R2 正在落地）、5 分钟催办冷却、链深保险丝。我们的保守缺省是对的。

### 4.3 Multica：issue 队列制 ≈ codeg taskboard 的远亲，无行动项。
