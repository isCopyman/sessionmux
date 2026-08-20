# 前后端模型体检 RFC（2026-08-21）

- 来源：r-modelaudit 只读审计（sonnet5），领导抽验后落库。
- 动机：近期 bug 复盘显示根因多为"模型缺概念"而非手滑（O13/O15/O31/O34）。
- 用法：深层重构的手术清单与顺序依据。**动 schema 的项需用户过目后才动手。**
- 领导抽验记录：五处根 folder 推导重复（rg 实证 4 文件 5 处）、
  collection_service::canonical_root_folder 确实不过滤 deleted_at 且对 Chat
  硬报错（与另四处行为分叉）、serde 双约定实证（collaboration.rs 35 处
  rename_all vs conversation.rs 4 处 / folder.rs 2 处）。抽验通过。

## 六切面清单（病症 → 修复级别 → 风险 → 裁决）

### ① Room / Session 双时间线底座（唯一"两套基础设施"级分裂）

- Session 侧 MessageTurn 有 usage/model/completed_at/多态 blocks
  （message.rs:208-231, 79-167）；Room 侧 RoomTimelineEvent 只有纯文本
  subject/body（collaboration.rs:608-625），无 usage/model/phase。
- 私信有 embedded_turn_ref 投影进会话转录面（collaboration.rs:262 +
  message-list-view.tsx applyCollaborationTimelineProjection）；Room 完全
  没有这条管线。
- 一致性机制两套：私信走 revision 乐观计数器；Room 的 RoomChanged 无版本
  号，靠 updated_at + last_read_event_id 游标。
- 级别：动 schema/迁移。风险：高。
- **裁决：最后做，先冻结范围**。UI 面统一由转录面骨架先行（进行中）；
  数据面统一等 ②③④⑥ 落地、⑤ 命名约定统一后再立项，否则要把前面的
  决定重新打包一遍。

### ② human 身份（借用 + 旁路修补的三层结构）

- collaboration_event.source_conversation_id NOT NULL，human 借房主 id；
  O13 已把守卫与三处读侧改为 authorship test（已落地）。
- 残留：post_room 的 bump_revision(source.id) 在 human 发帖时给房主
  revision 空转 +1——**已排查，确认无副作用，不需要修**（r-modelaudit
  二挖 + 领导抽验）：revision 从不参与计数（只是 SELECT 搭车字段），
  四个计数子查询全钉 `COALESCE(e.visibility,'direct')='direct'` 把 Room
  帖挡在外；COLLABORATION_CHANGED 的全部 4 个前端监听者逐个核过，最坏
  只是多一次内容不变的重拉（feed/timeline/queued-mailbox 判断都走
  direct-only 投影，不会误开页签或错亮徽标）。
- 一等公民化 = source 可空 + 独立 author 标识列，需重审所有 JOIN
  conversation 求 source 快照的路径。
- 级别：一等公民化动 schema。风险：现状中（靠"记得查 author_kind"约定
  维持，非类型保证）。
- **裁决：与 ③ 同批、先行半步；schema 动刀前须用户过目。**

### ③ 回复义务状态机（两个独立可变列共担一个状态）

- obligation_state 三态（None/AwaitingReply/Resolved），"作废"却在
  delivery.state（dismissed/failed）里——每条查询都要记得
  `AND d.state <> 'dismissed' AND <> 'failed'`，靠约定不靠 schema。
- 写入语义两套：human 帖由"是否 @ 人"推导（rooms-page.tsx:576），agent
  帖显式声明 expects_reply（session_collaboration.rs）。
- N/M 已回进度在数据层天然精确（N 条独立 delivery 行），只缺聚合读出
  （O31 后半，已排队）。
- 催办层无按义务类型建模，只有粗粒度"有无待处理摘要"（与合并 digest
  语义自洽，非独立 bug）。
- 级别：只动代码（不变量加固 + N/M 聚合 + 写入规则统一是产品决策）。
  风险：中（模式易被未来查询复制错）。
- **裁决：N/M 聚合与不变量加固可先行；写入规则统一并入 ② 批。**

### ④ 根 folder 推导五处实现、一处真分叉

- `COALESCE(f.parent_id, f.id)` 在 collaboration_room_service（×1）、
  folder_service（×2）、两个迁移（各 ×1）重复；collection_service::
  canonical_root_folder 走 SeaORM、不过滤 deleted_at、对 Chat 硬报错
  ——两个边界条件行为真不一样。
- 级别：只动代码（抽共享 resolver，迁移文件是历史快照不回改）。
  风险：中（尚无实证 bug，维护面已摆开）。
- **裁决：独立小刀，随时可做，排入近期批次。**

### ⑤ 序列化双约定（camelCase / snake_case 并存）

- collaboration.rs 全 camelCase（35 处 rename_all）；conversation.rs /
  folder.rs 基本裸 snake_case——types.ts 两族字段风格并存。字段级无漂移
  实例；CollaborationRoomSummary 四个计数在 TS 侧过度标了 `?:`（类型比
  契约宽松，非运行时 bug）。
- 级别：只动代码但改动面宽。风险：低（长期债）。
- **裁决：采纳"赶在新增字段前统一"，但补一个领导追加的前置项：先盘点
  哪些模型被持久化为 JSON**（tab 持久化、配置块等）——那些字段改名会
  破坏已存数据，要么排除要么带迁移。盘点完才准开工。

### ⑥ 工作台/页签归属（全部只动代码，已在执行）

- 跨台去重只查本台（workbench-session-tabs.ts:17-61）；
  ConversationWorkbenchRef 反查模型已存在（folder.rs:76-82）未被复用；
  collaboration_room.workbench_id 无任何 UPDATE 路径（换台 API 缺失
  实证）。
- **裁决：与报告建议一致，已派编队并行：先 Room 换台 API（纯后端）→
  跨台唯一化（tab-store + 各打开入口）→ 工作状态三层可见。**

## 手术顺序（采纳报告建议 + 领导修订）

1. ⑤ 序列化统一（前置：JSON 持久化盘点）——独立、赶在任何新增字段前。
2. ④ 推导收敛——独立小刀，随时插单。
3. ⑥ 工作台批——已在执行（O33/O34）。
4. ③ 先行件（N/M 聚合、不变量加固）——不依赖 ②。
5. ② human 一等公民化 + ③ 写入规则统一——同批，schema 动刀等用户过目。
6. ① 双时间线数据面统一——最后，范围冻结后立项；UI 面由转录面骨架先行。
