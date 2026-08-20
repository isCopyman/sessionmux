# 规格 B：claude-agent-acp 0.69.0 fork 能力实测

> 只读调研。产出一份事实表。**不改任何代码，不改 node_modules，不装包。**
> 本文件与任何消息指令冲突时以本文件为准。

## 1. 目标（你独占的一个产出）

回答：**codeg 钉的 claude-agent-acp 0.69.0，它的 fork 到底能做到哪一步？`_meta` 这条
扩展缝隙是真的存在，还是 SURVEY 的一厢情愿？**

SURVEY §6 把"给 claude-agent-acp 加 `_meta.messageUuid` 让 SDK 原生 forkSession 定位历史
UUID"排成了切片 2 的首选路线。但那是**基于 0.67.0 的推测**，没人打开过 0.69.0 的包。
这条路线是否成立，决定切片 2 是"提 PR 给上游"还是"退回文件手术"——两者工作量差一个量级。

## 2. 交付物

写进**你自己 worktree** 的
`docs/session-workbench/fork-rewind-slices/FINDINGS-B-claude-fork-audit.zh-CN.md`。

必须逐条回答下面 5 个问题，每条都要有证据（包内文件路径 + 代码片段原文）：

1. **0.69.0 的 `initialize` 回复里，`sessionCapabilities.fork` 到底在不在？**
   在的话它声明的形状是什么（布尔？对象？带什么子字段？）。
   —— 这条直接补 RFC §4 标注的"需实测"空白。
2. **它处理 `session/fork` 请求的代码在哪、做了什么？** 是转发给 Anthropic Agent SDK 的
   `forkSession`，还是自己实现？给出函数位置和关键分支。
3. **`ForkSessionRequest._meta` 会被读取吗？** 沿着请求解析路径查：`_meta` 是被丢弃、
   透传给 SDK、还是根本没解析。这是整个 `_meta` 路线成立与否的**关键判据**。
4. **SDK 的 `forkSession` 接受消息级参数吗？** 如果 0.69.0 依赖
   `@anthropic-ai/claude-agent-sdk`（或类似包），去看它暴露的 fork 相关签名，有没有
   `messageUuid` / `upToMessage` / `resumeAt` 这类定位参数。本机全局 npm 下有
   `@anthropic-ai`，可以一起看。
5. **fork 在文件层做了什么？** 和 codeg 已知的布局（复制父 transcript、保留原始时间戳、
   文件头写 fork 时刻元数据，见 `src-tauri/src/acp/background_watch.rs:1256-1266`）对不对得上？
   对不上就点明差在哪。

最后写一段**裁决**：`_meta` 路线是「可行」/「需上游改动」/「此路不通」三选一，一句话理由。

## 3. 从哪读、信什么

包在本机，**离线可审，不要联网装任何东西**：

```
C:\Users\63036\AppData\Roaming\npm\node_modules\@agentclientprotocol\claude-agent-acp
```

先确认版本是 0.69.0（`package.json` 的 `version` 字段）。**不是 0.69.0 就立刻停手在 Room
里报告**——审错版本的报告比没有报告更糟。

包是打包过的 JS（`dist/acp-agent.js` 等），读起来像压缩产物。有用的手法：
- `grep -n "sessionCapabilities\|fork" dist/*.js` 定位
- 关键字串常量往往是唯一没被混淆的东西
- `package.json` 的 `dependencies` 告诉你它靠哪个 SDK

codeg 侧对照物（**只读，不改**）：
- 版本钉在哪：`src-tauri/src/acp/registry.rs:446-465`
- codeg 怎么发 fork 请求：`src-tauri/src/acp/fork.rs`
- codeg 怎么探测支持面：`src-tauri/src/acp/connection.rs:4264-4268`
- 协议 schema 的 `ForkSessionRequest` 定义（含 `_meta` 字段）：sacp / agent-client-protocol-schema
  0.11.7，SURVEY §4 说在 `agent.rs:1253-1281`——**自己核实，别照抄行号**。

**信包里的代码，不信 SURVEY，不信 registry 注释。** 两者都是快照，都可能过期。

## 4. 边界（不要越界）

- **不改代码、不改 node_modules、不 `npm install`、不 `npm update`。** 全局包是用户
  在用的东西，动它会影响真实工作。只读。
- 不写 PR、不写补丁、不改上游。裁决"要不要提上游"是协调者的判断题。
- 不碰包 A（codeg parser 锚点）和包 C（codex）的地盘。
- 不跑 `pnpm tauri build`，不 push。
- 需要跑 claude-agent-acp 二进制做活体探测的话——**先在 Room 里问**，不要自己起进程。
  起 agent 进程会消耗用户的 Anthropic 额度。

## 5. 工作方式

```bash
cd D:/code/revisiting/work/repo_audit/repos/codeg-wt/fork-rewind
git worktree add ../../codeg-wt/fork-claude-audit -b wt/fork-claude-audit wt/fork-rewind
cd ../../codeg-wt/fork-claude-audit
```

commit：

```
docs(fork): audit claude-agent-acp 0.69.0 fork capability and _meta path
```

## 6. 报告格式（Room 回帖）

分支名、commit 哈希、**5 个问题的一句话答案各一行**、最后的三选一裁决。不贴正文。

## 7. 卡住怎么办

- 版本不是 0.69.0 → 立刻停，报告。
- 包被混淆到读不出来 → 别硬猜。把你能确认的写成事实，读不出的明确写"未能证实"，
  并在 Room 里说明卡在哪一步。**一份诚实的半份答案 >> 一份编出来的完整答案。**
- 同一步骤失败两次就停手，报事实等指令。
