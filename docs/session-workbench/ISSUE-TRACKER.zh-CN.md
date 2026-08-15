# Session Workbench GitHub Issue 与需求追踪

> 状态：Draft  
> 最近核对：2026-08-15  
> 范围：`xintaofei/codeg` 中由 `isCopyman` 提交、与本 RFC 直接相关的 Issue。

本文只做需求和实施映射。GitHub Issue 是讨论与上游状态事实源；RFC 是本地设计事实源。Issue
状态变化后应更新本文，不能把本页状态长期当作实时 GitHub 数据。

## 1. 当前 Issue

| Issue | 当前状态 | 对应需求 | RFC 位置 |
|---|---|---|---|
| [#403 Claude `/rename` 自定义名称](https://github.com/xintaofei/codeg/issues/403) | Closed | Claude 标题保真 | 主 RFC `SYNC-001`、Milestone 0 回归 |
| [#404 已导入会话不更新 `updated_at`](https://github.com/xintaofei/codeg/issues/404) | Closed | 活动时间与排序 | 主 RFC `SYNC-002`、Milestone 0 回归 |
| [#456 新窗口与拖拽分屏/吸附](https://github.com/xintaofei/codeg/issues/456) | Open | App Window、边缘 drop zone | 主 RFC `WIN-006`、`LAYOUT-001..004` |
| [#457 Codex 未读取真实会话名称](https://github.com/xintaofei/codeg/issues/457) | Open | Codex `session_index.jsonl` 标题 | 主 RFC `SYNC-001` |
| [#458 自动同步已导入的本地会话](https://github.com/xintaofei/codeg/issues/458) | Open | 文件监听、增量刷新 | 主 RFC `SYNC-002..005` |
| [#459 Collection / 主题树](https://github.com/xintaofei/codeg/issues/459) | Open | 语义分类；本地设计采用唯一主要归属 | 主 RFC `COL-001..004` |
| [#460 多套命名工作台](https://github.com/xintaofei/codeg/issues/460) | Open | Workbench 保存、切换和恢复 | 主 RFC `WIN-001..005` |
| [#461 持久 Session Team/Chatroom](https://github.com/xintaofei/codeg/issues/461) | Open | 后置：先做多选发送/转发/比较；再做以 Session 为成员的独立群聊面板，不先增加 Team | 群聊 RFC、AgentBus RFC、主 RFC 协作章节 |
| [#464 LaTeX `.tex` 高亮与编译/PDF 预览](https://github.com/xintaofei/codeg/issues/464) | Closed | 已由需求提出者关闭，不列入当前路线 | 外部编辑器边界 |
| [#465 独立 Mermaid 文件预览](https://github.com/xintaofei/codeg/issues/465) | Closed | 已由需求提出者关闭，不列入当前路线 | 外部编辑器边界 |

## 2. 需求聚类

### A. Session 可靠性

- #403：Claude 人工标题；
- #404：外部继续后更新时间；
- #457：Codex Desktop/CLI 的真实标题；
- #458：外部 Session 的持续增量同步。

这组是其他功能的基础。Collection、Window 和 AgentBus 绑定都依赖稳定 Conversation ID、标题和
活动状态。Closed 只表示上游 Issue 已关闭，实施时仍需保留回归测试，避免后续解析器修改重新
引入问题。

### B. 工作现场与窗口交互

- #456：拖边吸附、拆到物理窗口；
- #460：保存多套逻辑 Workbench，并恢复 Session、页面和布局。

两者相关但不相同：#460 先解决逻辑工作现场，#456 的物理新窗口可以在其后复用 Workbench
身份。拖边吸附本身独立、风险较低，可以先交付。

### C. 长期 Session 组织

- #459：Collection、主题树、未分类和快捷视图。实现时同时补齐当前 Workbench/全部 Session 的
  常驻 Scope、点击 Collection 后的范围标签，以及运行中、等待处理、Harness 和状态筛选。列表视图只是查询，
  不改变 Collection 归属、cwd、Workbench 或 Runtime。Issue 原提案中的多归属不作为第一版默认行为；
  当前裁决为唯一主要位置，未来若有真实需求再增加显式快捷方式。

Collection 不得复用 Folder 或 Work Task，也不改变 cwd。

### D. 多 Agent 协作

- #461：原提案名为 Team/Chatroom；本地产品先做多选发送、转发、比较和投递状态，再增加可选
  群聊面板。群聊把共享可见、实际激活和模型上下文分开，成员直接引用已有 Session，不先增加
  Team；
- [群聊面板与 Session 协作 RFC](./GROUP-CONVERSATION-RFC.zh-CN.md)：规定群聊/私聊、`@`、后台
  成员、上下文游标和 Agent 连续交接的边界；
- [AgentBus 协作子 RFC](./AGENTBUS-COLLABORATION-RFC.zh-CN.md)：把跨 App/主机 mailbox、
  project/role 管理和受管 Harness turn 投递补充进 #461 的范围。

#461 原始提案主要描述 Room 交互；AgentBus 子 RFC进一步解决当前真实使用中的绑定不可见、
Terminal 管理成本、远程通信和确定性投递问题。若向上游提交，应作为 #461 的补充设计或独立
Issue，不应悄悄扩大 #461 而失去评审边界。

### E. 富文档与资源预览

- #464、#465 已关闭，不再把 LaTeX 编译/PDF 工作流和独立 Mermaid Viewer 放入当前路线；
- Codeg 保留现有文件树、快速编辑、Diff 和常用预览，新增“在 VS Code/配置的外部编辑器打开”
  作为复杂文件工作的出口；
- 若以后重新出现足够稳定的内嵌预览需求，应重新开独立 Issue，而不是在 Session Workbench 改造
  中顺便扩张成完整 IDE。

## 3. 当前优先级

优先级采用依赖图，而不是把所有工作强行排成一条直线：

```text
Session identity / sync (#403, #404, #457, #458)
            │
            ├──> Collection (#459)
            ├──> Workbench persistence (#460)
            │       └──> physical windows (#456 部分)
            └──> AgentBus binding / collaboration (#461 + sub-RFC)

Split docking (#456 部分) ── 可独立提前交付
```

建议实施批次：

1. **P0 基线可靠性**：核实 #403/#404 修复，处理 #457，建立 #458 的增量同步基础；
2. **P1 快速体验收益**：完成 #456 的拖边吸附，不等待完整多窗口；
3. **P2 逻辑 Workbench**：实现 #460，保存多套分屏与 Session 组合；
4. **P3 Collection**：实现 #459；如果侧栏重构更适合提前，也可与 P2 并行；
5. **P4 大量会话、物理多窗口与资源 Pane**：Session Center、可选全文检索、完成 #456 剩余部分，
   实现 Session 跟随、Workbench 图钉固定和“在外部编辑器打开”；#464/#465 不再列入路线；
6. **C0-C3 可选协作轨道**：AgentBus 只读总览/消息 UI、Session 绑定、多选发送、转发、比较和
   受管投递，可在稳定 Session 映射后并行；
7. **C4 持久群聊**：稳定 Session 投递和多视图同步后，实施 #461 的群聊面板、共享消息、显式
   目标和链式协作保护，仍不增加 Team 层。

## 4. 防止需求遗漏的更新规则

每次发现新痛点时，先判断它属于：

- Session identity/sync；
- Collection/搜索；
- Workbench/Window/Layout；
- Harness 能力保真；
- 群聊/AgentBus；
- 资源页；
- 富文档预览；
- Task/Automation；
- 非目标或未来研究。

能映射到现有 Issue 时补充本页和 RFC 条目；不能映射时先写问题与验收条件，再决定是否开
Issue。不要只在聊天历史、截图或个人记忆中保留需求。
