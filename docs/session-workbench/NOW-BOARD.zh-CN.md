# 作战板（当前在飞的活，每次唤醒先读后更）

> 规则：领导会话每次醒来第一件事读这里，收尾最后一件事更新这里。
> 只记"现在"，历史进台账/程序日志。更新时间戳手写。

最后更新：2026-08-21 白天（O56/O57 已合并；O39 启动档 RFC 待拍板）

## 在飞（等结果）

| 线程 | 状态 | 下一步 |
| --- | --- | --- |
| o46-research（看板按项目/群聊/Collection 组织，grok） | running | 回来→修订 TASKBOARD RFC 二期 |
| O54 分类树文件夹菜单「新建群聊」 | 待派工（等 O56 合并——**已合并，可派**） | 用统一 SessionPicker 做 |

## 已合并待实机走查（dev 重建后 CDP）

- O51 建群流程（单人建群→面板→拉人按钮/空群引导）
- O52 会话中心最近活动排序
- O53 聚合消息左轨时间
- O46 一期（awaiting_input 通知 + attention 分层——通知要等真实任务翻转才能看到）
- O8 红条 Retry（要等真实瞬时失败或手工注入）

## 等用户拍板（不许自己动）

- GitHub：fork（公开）还是自建私库
- O47 建议 C：后端重启不自动重放滞留队列项（A 已降级不做，B 在看板 RFC）
- TASKBOARD RFC 待拍板 A–F 逐项确认（一期已按建议值施工）
- schema 手术批（human 一等公民等，模型体检 RFC ②③①）
- 旧 worktree 处置：sessionmux-session-timer、agent-a31e34…、fork-rewind/o8-audit 空壳
- 唤醒分级/协同税进一步治理方向

## 记账

- 六单 grok 派工：5 完成已合并（taskboard-p1/o39-research/room-create/o8-retry/room-read）+1 在跑（o46-research）
- 领导亲改已合并：O52 排序、O53 时间轨、O47 复查（含 2 条漏网催办补暂停）
- Trellis 借鉴两件套已落地：任务书骨架=LESSONS L11；教训回灌惯例=LESSONS 本身

## 2026-08-21 白天增补

已合并（门禁绿）：O52 排序 / O53 消息时间 / O55 配置 chip 命名 / O56 共享 SessionPicker
+ membership 服务（顺带修掉 hydration button 套 button，带回归钉）/ O57 会话面板
「加入群聊」。O8 / O48 / O51 早批已合并并跑过 Rust 全套。

新等拍板：**`CLAUDE-PROFILE-RFC-2026-08-21`**（Claude 启动配置档）——机制已核实
（`CLAUDE_CONFIG_DIR` per-spawn，适配器与 SDK 都认），拍板点 A–F，其中 A（停写用户
原生配置文件，顺带解上游 #520 一类污染）与 B（会话绑定需要一个可空列）是前提。
