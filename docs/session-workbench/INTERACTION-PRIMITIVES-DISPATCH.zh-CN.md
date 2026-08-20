# 交互原语批 先行件派工规格（包 K / 包 L）

> 用户拍板 2026-08-21：体验问题优先。两件与宪章①骨架**并行**（不同文件不撞车）。
> 裁决出处：`docs/session-workbench/DOGFOODING-LOG-2026-08-20.zh-CN.md` 的 O33 / O34 条目。
> **本文件与任何消息冲突时以本文件为准。**
>
> 基线：`codex/session-message-v1` 最新（已含 fork/rewind 第三轮 `ca922f4b`）。
> **永不 push，永不动 main，不跑 `pnpm tauri build`。**

## 0. 共同硬规矩

与 `CHARTER1-PHASE1-DISPATCH.zh-CN.md` §0 **完全相同**，先去读那一节（10 条）。
要点复述：建完 worktree 立刻 `cd`；**绝不在协调者树**（`codeg-wt/charter1`）内写；
**禁 `git add -A`**；前端门禁前先 `pnpm install --frozen-lockfile`；门禁跑全；
失败两次停手上报；**发现规格错直接说**；报告带 `file:line` 与命令原文输出。

**本批与骨架批的不撞车边界**（务必遵守）：

- 骨架批（包 I / J）动 `src/components/message/**` 与新建 `src/components/transcript/**`。
- **本批不许碰这两处。** 你们动的是 `src-tauri/**`（K）与
  `src/lib/workbench-session-tabs.ts` + 打开路径（L）。
- 有交叉需求先在 Room 问，别自己扩面。

## 包 K：Room 换工作台后端 API（O33 先行件）

分支 `wt/o33-room-workbench-api`，目录 `codeg-wt/o33-room-workbench-api`。

### 背景（协调者已核实）

`collaboration_room.workbench_id` **只在建群时写死，之后无任何重指派路径**：

- Host Control 现有 room 动作只有 `room.create / add_member / delete / list /
  list_workbench / post`（`commands/host_control_room.rs`），**没有换台**。
- Web 端点 `/collaboration_room_*` 有 create/list/get/add_members/remove_member/
  rename/add_path，**没有换台**（`web/router.rs:1356-1384` 一带）。
- 现有校验样板可参考 `db/service/collaboration_room_service.rs:59 require_workbench`
  （换台必须复用它校验目标台存在）与 `:247 room_workbench_id`。

O33 的产品目标是"Room 行与会话行右键能力**相通**"（用户原话："不是要完全一致，
而是相通的功能要都有"）。会话行已有"移动到工作台"，Room 行缺后端能力。

### 交付

**只做后端能力，不做菜单 UI**（菜单对齐是后续件，等这个 API 落地）。

1. **service 层**：`collaboration_room_service` 加换台函数。必须：
   - 复用 `require_workbench` 校验目标工作台存在；
   - 目标台与当前台相同时是幂等 no-op（不是错误）；
   - Room 已删除/非 active 时明确拒绝；
   - 想清楚**成员与 Room 的关系是否受影响**——如果 Room 换台会让某些成员会话
     "不在同一台"，这是**判断题，写进汇报让协调者拍板**，不要自己改成员语义。
2. **双模式端点**：Host Control 动作（`room.` 命名空间，与既有对齐）+ Web 端点
   （`/collaboration_room_*` 命名，与既有对齐）。两侧都要。
3. **测试**：换台成功落库；目标台不存在 → 拒绝；同台幂等；非 active Room → 拒绝。

### 边界

- **不改前端**（右键菜单是后续件）。
- 不碰 `src/components/message/**`、`src/components/transcript/**`（骨架批地盘）。
- 不动 Room 成员语义（除非协调者拍板）。

### 门禁

```bash
cd src-tauri
CARGO_TARGET_DIR=<你自己的独立目录> cargo fmt
CARGO_TARGET_DIR=... cargo clippy --all-targets --features test-utils -- -D warnings
CARGO_TARGET_DIR=... cargo clippy --no-default-features --bin codeg-server --lib -- -D warnings
CARGO_TARGET_DIR=... cargo test --no-default-features --bin codeg-server --lib
```
**必须用独立 `CARGO_TARGET_DIR`**（简报要求；且并行 worktree 各建一个 target 会撑爆盘，
上一轮真出过 `os error 112`）。动了 TS 才跑前端四条。

---

## 包 L：会话跨台唯一化 + 打开即聚焦（O34 前两条裁决）

分支 `wt/o34-session-uniqueness`，目录 `codeg-wt/o34-session-uniqueness`。

### 裁决原文（**只做前两条，第三条不做**）

1. **跨台唯一，一个会话一个家。** 理由（领导原话）：会话是有状态的活流不是文档，
   双开只带来"我在哪个副本里打字"的混乱和双份渲染。
2. **打开即聚焦**：任何入口（侧边栏 / 会话中心 / 群聊提及跳转）打开**已开**会话
   = 跨台跳转聚焦既有页签；**未开** = 当前台新开页签；新窗口只走显式动作。
3. ~~工作状态三层可见~~ —— **本包不做**，用户明确"先做前两条"。

### 现状（协调者已核实）

`src/lib/workbench-session-tabs.ts`（**193 行**，配 **242 行**测试）的
`appendConversationTabs`（`:25-44` 一带）用 `conversationIdsInTabs(existing)`
**只在本台去重**，跨台可重复打开同一会话；无"聚焦已开页签"的跨台跳转。

### 交付

1. **跨台唯一化**：同一 `conversation_id` 全局只能存在于一个工作台的页签中。
   注意 `appendConversationTabs` 现在**只看 `existing`（本台）**，跨台唯一需要
   更大的输入面——**如何拿到"全台页签"是设计判断，先在汇报里说明你的方案**
   （纯前端聚合？还是需要后端支持？若需要后端，**停手先问**，别自己开 Rust 战线）。
2. **打开即聚焦**：梳理**所有**打开路径（侧边栏 / 会话中心 / 群聊提及跳转），
   已开 → 跨台跳转并聚焦；未开 → 当前台新开。**打开路径清单必须列进汇报**，
   漏一条就是漏一个入口。
3. **迁移/兜底**：现存数据里可能已有同一会话开在多台（旧行为的产物）。
   **遇到重复时的处置是判断题**——保留哪个、怎么合、要不要提示用户，
   写进汇报让协调者拍板，**不要默默删用户的页签**。
4. 测试：既有 242 行不得删不得放松；新增覆盖跨台去重与聚焦跳转。

### 边界

- 不碰 `src/components/message/**`、`src/components/transcript/**`（骨架批地盘）。
- **不做第三条裁决**（工作状态可见性）。
- 若发现必须动 Rust 才能做到跨台唯一，**停手在 Room 报告**，不要自己开战线。

### 门禁

`pnpm install --frozen-lockfile` → `pnpm eslint .` → `pnpm tsc --noEmit` →
`pnpm vitest run` → `pnpm build`。纯前端不动 Rust。

---

## 汇报格式（两包相同）

Room 回帖 `expects_reply=false`：改动文件清单 / 对外 API 或行为契约 /
**判断题与你的建议**（K 的成员语义、L 的跨台数据来源与重复兜底）/ 没做的事 /
分支 + commit / **门禁输出摘要**（数字真实，协调者会重跑）。
