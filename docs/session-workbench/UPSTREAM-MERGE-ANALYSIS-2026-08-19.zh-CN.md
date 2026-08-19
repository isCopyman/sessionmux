# 上游合并前分析（G11 预研）— 2026-08-19

范围：`ea5177ea`（merge-base）→ `origin/main@0935e1eb`（tag v0.26.2），共 37 提交
（10 merge 跳过，27 实质）。调研：w-upstream-recon（只读）；编排抽验三个支柱论断
（ai-elements 零分叉、grok 1.0.5、DbError::Conflict 撞名）均属实。

**摘要**：12 个可直拿（A），10 个需适配但不矛盾（B，其中 ACP 绑定两修复接近
"靶子已不存在"），5 个组成"codex 标题同步链"与我方 f56ec5e6 正面撞设计——
**待用户拍板**。`01ee6b84` 把 grok bump 到 1.0.5，命中我方冻结的先导任务。
qoder 为第 14 家 harness，同构接入可完整拿。

## A 类·可直拿（12）

| 提交 | 内容 | 依据 |
|---|---|---|
| 47dde4bc a6e8f837 ba8bd97e 02cb17bb 756186d5 | markdown 数学/货币误判修复五连 | 全在 `src/components/ai-elements/`，我方 merge-base 以来零改动（diff 为空，编排亲验）。ba8bd97e 新增 4 个纯 devDependency 只服务新测试 |
| ad42a0aa | Windows 路径分隔符被 CommonMark 吞掉的修复 | 同落 ai-elements/，零分叉；Windows 主力用户价值大，**建议第一批合** |
| 3299d333 364350c8 428df4f8 | 状态栏快捷操作菜单三连 | status-bar.tsx + 新文件均零分叉 |
| 0ad0c826 | Office 预览跳过点开头隐藏文件 | workspace-context.tsx / language-detect.ts 零分叉 |
| 01ee6b84 | ACP bump：hermes 0.20.4 / codebuddy 2.137.1 / kimi-code 0.37.2 / **grok 1.0.5** / deepseek-acp 0.5.0 | registry.rs 我方仅 13 行分叉；**grok 1.0.4→1.0.5 命中冻结先导任务**，上游 commit message 亲测 1.0.5 initialize 宣告不变（sessionCapabilities list/resume/close，无新增 fork 广告——x.ai/rewind 私扩展本就不在 initialize 里） |
| 881a2d06 | release 0.26.2 版本号 | 纯版本字段，跟不跟是产品节奏，可跳过 |

## B 类·需适配（10）

- **58999771** grok 真实 token 用量（此前显示 0）：parsers/grok.rs 我方改 312 行且
  区间重叠（parse_updates/GrokTurnMeta），逐 hunk 手工重放。
- **c36d7b04 + 0935e1eb** qoder 接入（第 14 家）：parser 新文件（840→1801 行）零冲突；
  models/agent.rs 我方仅 5 行分叉可直套；麻烦在外围接线 8-10 触点
  （companion.rs 我方重写 3448 行、tool_schema.json 重写 283 行内找位手补，
  含硬编码 "13"→"14"）。机械无设计冲突。
- **9d716876 + 79061f64** 侧栏右键"添加到会话"：sidebar-conversation-card.tsx 两边
  hunk 挨着（100-700 行区间），手捋菜单顺序；session-attachment-events.ts 纯增量直套。
- **51174e15 + 2fd356a2** @ 面板归属 composer + 同宽/加载态：suggestion-popup.tsx、
  rich-composer.tsx 上游改动时我方零分叉，但 **w-room-file-mention 正在这两个文件上
  加 room 文件@**——落地定序：我方 fix 先落，上游后合，冲突编排解（已定）。
  chat-input.tsx / message-input.tsx 有小冲突（我方 22/9 行）。
- **522ab922 + fbb0ca06** ACP "turn 落错会话"/"孤立历史"修复：**不许顺手合**。
  上游靶子函数 `bind_external_id` 在我方已不存在（被 send_prompt_linked /
  update_external_id / ConversationLinked 机制取代）；上游新增 DbError::Conflict
  与我方独立加过的同名变体**语义相反**（我方=CAS 竞争可重读重试，上游=唯一键冲突
  必须放弃）。正确做法：立独立深挖任务，核实我方重写后的绑定逻辑是否仍有同类洞，
  照我方机制重修，不搬上游补丁。
- **1e3e5a10** 会话卡"responding"修复：acp-connections-context.tsx 两边 hunk 在
  4100-5900 行状态机区大量重叠，逐段对照读后重放。

## 待用户拍板：codex 标题同步链（5951c61b → e1d20c88 → 56d51e4d → 3e8148c5 → c416920f）

上游整条链解决"codex 会话标题没同步进侧栏/工作区列表"；我方 f56ec5e6 已用
`read_codex_session_index_titles` / `session_index_titles`（parsers/codex.rs）独立
解决过极相近症状。两套方案函数名不同、路径不同，直接合大概率产生冗余/互相打架的
标题判定。**选项**：① 照单全收上游链、撤我方 f56ec5e6；② 保留我方方案，只挑
上游 c416920f 的"标题同步挪出读路径"纯性能修复（大概率两边都想要）；③ 全部不合，
观察症状是否仍在。编排倾向 ②（保留自有方案+吸收性能点），但按红线交用户定。

## 合并策略（编排采纳工人建议）

**不做一次性 merge origin/main**——lifecycle.rs（我方 3280 行分叉）、manager.rs
（2629）、acp-connections-context.tsx（热区重叠）、commands/acp.rs（1117）、
commands/conversations.rs（897）冲突 hunk 量级没法安全 review，自动合有吞修复风险。
分三批：
1. **A 类 12 个直接 cherry-pick**（markdown 链按序、Windows 路径优先）；
2. **B 类独立项逐个 cherry-pick + 手核**（qoder 新文件先落再补接线；grok token；
   侧栏右键；@ 面板两连等 room 文件@ 落地后合）；
3. **ACP 绑定两修复 → 独立深挖任务；标题同步链 → 等用户拍板**。

预计冲突文件（强→弱）：acp/lifecycle.rs、acp/manager.rs、acp-connections-context.tsx、
commands/acp.rs、commands/conversations.rs、conversation_service.rs、parsers/codex.rs、
parsers/grok.rs、db/error.rs（变体重名）、sidebar-conversation-card.tsx、
chat-input.tsx、message-input.tsx。
