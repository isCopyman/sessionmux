# Codeg Session Workbench 文档索引

> 状态：Active draft（已进入分阶段实现）  
> 适用范围：Codeg `main` 上的 Session 组织、工作台与后续协作层设计  
> 原则：先兼容现有 Codeg，再以显式迁移逐步实现新模型。

截至 2026-08-15，Codex 原生标题同步、命名 Workbench、顶部工作台标签、工作台复制/排序、
Session Center、可选内容全文检索、单窗口拖边吸附，以及 Collection 的层级树、唯一主要归属、
批量移动与非破坏性删除已经在本地开发分支实现；Collection 拖放/排序、工作台归档/最近关闭、
系统多窗口及协作层仍按本文档分阶段开发。文档中的“最终效果”不能被误读为当前版本已经全部
具备。

## 你应该看哪一份

如果只关心最终产品效果、功能、场景和优先级，只看：

> [产品需求与使用场景](./PRODUCT-SPEC.zh-CN.md)

其他文档是给实现和维护使用的，可以不看。

## 文档分工

- [产品需求与使用场景](./PRODUCT-SPEC.zh-CN.md)：唯一的人类需求入口，只描述最终体验。
- [领域模型](./DOMAIN-MODEL.zh-CN.md)：稳定的产品概念、用户行为、对象关系和长期边界。
- [Session Workbench 主 RFC](./SESSION-WORKBENCH-RFC.zh-CN.md)：Codeg 当前实现、拟议字段、迁移步骤、里程碑和验收标准。
- [Session Center、全文检索与打开行为子 RFC](./SESSION-DISCOVERY-OPENING-RFC.zh-CN.md)：全局会话管理、ctx 可选全文检索、默认打开决策和 Focus 语义。
- [Workbench 层级、多窗口与 Session 多视图同步子 RFC](./WORKBENCH-LAYOUT-SYNC-RFC.zh-CN.md)：顶层工作台标签、物理窗口、视图/运行时边界与同步规则。
- [Session 历史能力子 RFC](./SESSION-HISTORY-CAPABILITIES-RFC.zh-CN.md)：Fork、旧消息编辑、Rewind、文件检查点及各 Harness 的能力降级。
- [Session 间通信与调用策略 RFC](./SESSION-COMMUNICATION-RFC.zh-CN.md)：稳定寻址、消息/显示/模型调用三层边界、定向发送、调用策略和 Delivery Router。
- [Session Runtime 生命周期 RFC](./SESSION-RUNTIME-LIFECYCLE-RFC.zh-CN.md)：统一 Session/ACP/Turn、PromptQueue、Mailbox、Dispatcher、崩溃恢复与未来等待点的事实和拟议语义。
- [Host 控制面与 Agent 可编程工作台 RFC](./HOST-CONTROL-SURFACE-RFC.zh-CN.md)：把现有 delegation 解释为 Session 生命周期组合预设，并统一记录 Session 启动、持续通信、Workbench/资源操控、Skill + 渐进式 MCP 控制路径及外部实现依据。
- [Delegation Removal 与共享 Host Bridge 审计](./DELEGATION-SUBSYSTEM-AUDIT.zh-CN.md)：旧三工具直接移除；逐层区分应删除的 task_id/Broker/专属 UI 与必须保留的 companion、可信身份、transport、普通 MCP 工具和可复用视觉 primitive。
- [群聊面板与 Session 协作 RFC](./GROUP-CONVERSATION-RFC.zh-CN.md)：Room 作为内容面板、Session 成员、共享/私聊边界、显式目标、上下文摄入和协作链限制。
- [AgentBus 协作子 RFC](./AGENTBUS-COLLABORATION-RFC.zh-CN.md)：Codeg 原生 Delivery Router、跨 App/主机 AgentBus、`wait` 降级与未托管边界 Adapter。
- [GitHub Issue 与需求追踪](./ISSUE-TRACKER.zh-CN.md)：已提交问题、状态、RFC 条目和实施里程碑的对应关系。
- [Atrium 公开功能审计](./ATRIUM-FEATURE-AUDIT.zh-CN.md)：竞品样本的完整功能族、Codeg 差距、可借鉴机制和明确不照搬项。

领域模型回答“产品里的概念是什么”，主 RFC 回答“如何在 Codeg 中安全实现”，Session Center
子 RFC 回答“如何找到并可预测地打开会话”，历史能力子 RFC 回答“不同 Harness 如何可靠地
分叉或恢复”，Session 通信 RFC 回答“消息到达、UI 可见和模型调用如何分开”，Host 控制面 RFC
回答“人和 Agent 如何共用同一套 Session、运行时、布局与资源操作”，群聊 RFC 回答
“共享时间线、私聊和 Session 激活如何分开”，AgentBus RFC 回答
“如何把跨边界传输变成受管理的协作能力”，delegation 专项审计回答“现有一次性子 Agent 代码
究竟怎样拆而不误伤共享 Host Bridge”。若它们
冲突，不能直接用概念模型覆盖现有代码；必须先修订 RFC，写明适配或迁移方式，再修改代码。

Atrium 审计只提供外部参考，不是需求事实源。竞品功能与产品文档冲突时，应先回到真实使用
场景判断；不能为了追平竞品而静默扩大范围。

## 持续记录规则

后续调研不默认另建一份重复的“人类版总结”，而是按内容性质写回现有文档：

- 调研改变了最终产品效果、用户路径、优先级或验收标准：同步更新产品需求；
- 调研澄清了稳定概念和对象关系：更新领域模型；
- 调研涉及协议、源码、provider 差异、安全边界或实施顺序：更新对应技术 RFC，并注明日期；
- 调研只是在核对竞品：更新对应功能审计，不自动升级成产品需求；
- 已提交 GitHub Issue 的状态发生变化：更新 Issue 追踪表。

同一结论不在多份文档中复制完整段落。产品需求保存人类需要看到的结果，技术文档保存证据与
实现约束，并通过链接关联。发现旧结论过时，应直接修正并记录调研日期，不在正文尾部无限追加
聊天式日志。

## 兼容性政策

### 1. 当前代码是实现事实源

数据库实体、Rust 模型、TypeScript 类型和运行时行为以当前代码及迁移为事实源。设计文档中
的伪字段只用于表达需求，不自动成为代码字段。

发现文档与代码不一致时，按以下顺序处理：

1. 核实代码、测试和已有迁移的真实语义；
2. 优先修改文档中的错误映射；
3. 若产品需求确实不能由现有模型表达，新增独立字段、关系表或适配层；
4. 只有在提供迁移、回退和兼容测试后，才允许改变现有字段语义；
5. 不允许为了让代码“看起来符合 RFC”而静默复用一个语义不同的字段。

### 2. 现有数据必须可无损升级

任何 Session Workbench 改动都必须保证：

- 不删除或复制原生 Harness 会话；
- 不改变 `conversation.external_id` 所代表的原生会话身份；
- 不因分类、切换 Workbench、关闭 App Window 或删除引用而删除 `conversation`；
- 不因 Collection 操作改变 `folder_id`、cwd、worktree 或权限边界；
- 旧版 `opened_tab` 和本地布局能迁移到默认 Workbench；
- 迁移失败时仍能以兼容模式启动；
- 桌面端、Web 端和服务器模式继续共享现有核心逻辑。

### 3. 产品概念与 Codeg 现有名称的映射

| 产品概念 | Codeg 当前承载 | 兼容性要求 |
|---|---|---|
| Session | `conversation` | 不新建第二套 Session 事实表 |
| 原生 Session ID | `conversation.external_id` | 保持 `(agent_type, external_id)` 的导入身份语义 |
| Harness | `conversation.agent_type` | 不另造 `harness` 字段 |
| 本地显示名 | `conversation.title`、`title_locked` | 保留源标题同步和人工锁定规则 |
| Execution Context | `folder_id`、`folder.path`、`origin_cwd`、运行时设置 | 第一阶段不新建 `execution_context_id` |
| Workbench | 已有独立 `workbench` 实体、按工作台分区的 `opened_tab`、顶部标签及复制/排序 | 继续独立演进，不复用 Folder；归档、最近关闭和系统窗口挂载仍待实现 |
| Collection | 独立 `collection` 层级与唯一 `collection_conversation` 归属 | 不复用 Folder；拖放、排序和更多快捷视图继续演进 |
| Group Conversation / 群聊 | 尚无“多 Session 群聊”实体 | 协作轨道候选；稳定 Session 投递后新增独立关系与共享事件，不能误用现有 `chat_channel` |
| Task / Issue | 已有 `work_task`，但它是编码执行流水线 | 长期 Topic 不得包装成 `work_task` |
| Agent Profile | `custom_agent` 及内置 Agent 注册 | 群聊成员仍应引用具体 `conversation` |

### 4. 禁止复用的同名或近似字段

以下字段名称相似，但语义不同，实施时不得直接复用：

- `conversation.parent_id` 当前表示 delegation 子会话关系，不是通用 Fork 谱系；
- `folder.parent_id` 当前表示 worktree Folder 指向原始根 Folder，不是 Collection 嵌套；
- `folder_link` 是多目录工作区的符号链接及路径授权记录，不是 Session 快捷方式；
- `ConversationKind::Chat` 与 `FolderKind::Chat` 表示 folderless chat 的内部归类，
  不是多 Session 群聊；
- `chat_channel` 表示 Telegram 等外部消息渠道配置，不是多 Session 群聊；
- `chat_channel_thread_binding` 表示外部聊天线程到一个 Conversation 的绑定，不是群聊成员关系；
- `work_task` 是带 worktree、队列、审查和合并状态机的编码任务，不是 Topic、Collection
  或普通研究 Issue；
- `TAB_ORIGIN` 是一次页面加载的消息回显抑制标识，不是逻辑 Workbench ID 或物理窗口 ID；
- `workspace:tab-groups:v1` 现作为默认 Workbench 的兼容布局键；其他命名 Workbench 使用独立键，
  但这些本地布局键仍不代表系统窗口挂载或完整跨设备 Workbench 状态。

如果以后需要通用 Fork 谱系、群聊成员关系或 Decision，优先新增独立关系表，避免改变上述
字段已经被运行时和测试依赖的语义。

## 修改流程

每次实现前应完成：

1. 在领域模型中确认需求属于 Session、Collection、Workbench、Execution Context 还是群聊；
2. 在 RFC 中列出现有代码落点和拟议变更；
3. 对数据库变更给出迁移、回滚和旧数据兼容策略；
4. 对前端状态变更区分服务器共享状态与设备本地状态；
5. 增加回归测试后再更新文档状态。

文档不是要求 Codeg 一次实现全部概念。未进入当前里程碑的对象保持“概念保留、实现暂缓”，
不得为了完整数据模型提前侵入稳定代码。
