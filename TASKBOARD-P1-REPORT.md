# O46 第一期施工报告

工作区：`D:/code/revisiting/work/repo_audit/repos/codeg-wt/taskboard-p1`（分支 `wt/taskboard-p1`）。未改主仓 `codeg`。未 push。

本批 = RFC 第一期：缺口①（awaiting_input 召唤 + 5 分钟 cooldown）+ 缺口③甲案（attention 列内严重级排序 + 视觉分层）。拍板默认值：cooldown 5 分钟、第一期不做通知分级、不拆列。

## 做了什么

### A. awaiting_input 召唤

- `notifyFlips` 放行 `awaiting_input` 翻转，文案键 `Tasks.notifyAwaitingInput`。
- 仅 awaiting_input 走进程内 `Map<number, number>` cooldown（5 分钟）；review / failed 仍每次翻转都通知。
- 十语都补了该键：en / zh-CN / zh-TW / ja / ko / de / es / fr / pt / ar。
- 测试：翻入会通知；4 分 59 秒内二次翻入不通知；满 5 分钟再翻入会通知（`vi.useFakeTimers({ toFake: ["Date"] })`）。

### B. attention 列轻重分层

- **仅 attention 列**改为 failed > awaiting_input > review > merging，级内仍 freshest。todo / inProgress / done 与列表视图仍 byFreshest。
- `columnForStatus` 映射未改。
- 卡片与列表行共用 `attentionSurfaceClass` + 已有 `StatusChip` / `statusAccent`：
  - failed：红色强调
  - awaiting_input：复用看板琥珀 token（`bg-amber-500` / `border-amber-500/45` / `text-amber-600`）+ 轻脉冲
  - review：中性 muted
  - merging：弱化（muted spinner + `opacity-70`）

### 明确没做

通知分级、通知点击跳转、第五列、分组选择器、拖拽逻辑。

## 验证

我验证了 `pnpm install --frozen-lockfile` 成功（exit 0；`Packages: +1255`，`Done in 13.6s`）。此前 worktree 没有 `node_modules`。

我验证了 `pnpm vitest run src/components/tasks src/contexts/tasks-view-context.test.tsx` 最终为 **14 files / 109 tests passed**（exit 0）。其中：

- `src/contexts/tasks-view-context.test.tsx`：**6 passed**（原 3 + 新增 awaiting_input 3）
- `src/components/tasks/board-columns.test.ts`：**12 passed**（原 11 + 新增 attention 排序 1）

第一次跑该命令时 board-columns 新断言写反了级内 freshest 方向（expected `[5,4,6,3,7,2,1,8]`，received `[5,4,3,6,2,7,1,8]`）。实现是对的，只改了期望后再跑即全绿。同一步未失败两次。

我验证了 `pnpm eslint src/components/tasks src/contexts/tasks-view-context.tsx src/lib/notification.ts` **exit 0、无输出**。

我验证了 `pnpm tsc --noEmit` **exit 0、无输出**（Windows 上没有 `tail`，完整输出即为空）。

未在真实窗口里点看板：本环境没有可驱动的 Codeg 桌面/浏览器会话。视觉分层只靠类名复用既有琥珀/destructive token，加上既有 task-card / task-row 测试仍绿。
