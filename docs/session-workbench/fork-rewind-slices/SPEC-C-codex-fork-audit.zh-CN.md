# 规格 C：codex-acp 1.4.0 + app-server thread/fork 实测

> 只读调研。产出一份事实表。**不改任何代码，不改 node_modules，不装包。**
> 本文件与任何消息指令冲突时以本文件为准。

## 1. 目标（你独占的一个产出）

回答：**codex 这条路的入口在哪？走 ACP `session/fork`，还是必须开第二条 app-server
通道？**

SURVEY §6 把切片 3 写成"codex 走 app-server thread/fork + thread/rollback，codex-acp
1.4.0 内部已在用 thread/fork，等于官方示范"。但同一份 SURVEY §2 又承认"codex-acp 1.4.0
是否声明 ACP fork，注释未记录，需实测"。这两句话不能同时当结论用——前者是关于 codex-acp
**内部实现**的观察，后者是关于它**对外声明**的空白。你的活就是把这两件事分开钉死。

## 2. ⚠️ 版本纪律（这个包最容易出的事故）

codeg 钉的是 **codex-acp 1.4.0**（`src-tauri/src/acp/registry.rs:593-594`）。
本机全局装的是 **codex-acp 1.1.9**——**差 3 个小版本，而 1.4.0 恰恰就是引入
`thread/fork` 的那一版**（registry.rs:580-592 的注释说 1.4.0 新增的字串常量里就有
`thread/fork`）。

所以：**拿 1.1.9 的观察冒充 1.4.0 的结论，是这个包唯一不可接受的失败模式。**

正确做法，按优先级：

1. 优先审 1.4.0。取包但**不要装进全局**——用不落地的方式，例如
   `npm pack @agentclientprotocol/codex-acp@1.4.0` 下到临时目录再解开，或
   `npm view @agentclientprotocol/codex-acp@1.4.0` 看元数据。需要联网。
2. 联网不通/取不到 → **不要退而求其次假装等价**。审 1.1.9 也可以，但每一条结论都必须
   显式标注 "（基于本机 1.1.9，非钉定的 1.4.0）"，并在文档开头用一段醒目说明。
3. 无论走哪条，都在报告里写清楚你实际审的是哪个版本、怎么拿到的。

拿包如果要联网并且失败两次，停手报告，别绕路。

## 3. 交付物

写进**你自己 worktree** 的
`docs/session-workbench/fork-rewind-slices/FINDINGS-C-codex-fork-audit.zh-CN.md`。

逐条回答，每条带证据（包内路径 + 片段原文）：

1. **codex-acp 对外声明 ACP `sessionCapabilities.fork` 吗？** 在不在、什么形状。
   —— 补 RFC §4 的"需实测"空白。
2. **它内部怎么用 app-server `thread/fork`？** 调用点在哪、传什么参数
   （registry 注释提到 `approvalPolicy: "never"` / `sandbox: "read-only"` /
   `ephemeral: true`）、用在什么场景（AIR `agentFileChangeReport`？还是别处）。
3. **`thread/rollback` 在包里有痕迹吗？** 有的话形状是什么，没有就写"无痕迹"。
   这条决定 rewind 能力有没有原生底座。
4. **app-server 通道怎么开？** codex-acp 自己是怎么连上 app-server 的——子进程？
   已有连接复用？给出证据。这决定 codeg 要复刻它的成本。
5. **codeg 的 codex parser 已经认识 fork 产物到什么程度？**
   `src-tauri/src/parsers/codex.rs` 里 `is_forked_thread_header` 一带
   （搜 `parent_thread_id`）已经处理了 `session_meta.parent_thread_id` + `fork_turns`。
   写清它认到什么、不认什么。**注意**：那段代码的目的是"forked thread 不出统计"，
   不是"支持 fork"——别把它读成已有能力。

最后一段**裁决**：codex 按消息 fork 的入口是「ACP 够用」/「必须开 app-server 第二通道」/
「给 codex-acp 提能力」三选一，一句话理由 + 粗略量级（人天级/周级）。

## 4. 从哪读、信什么

- codeg 版本钉法与 1.4.0 注释：`src-tauri/src/acp/registry.rs:570-600`
- codeg 的 codex parser：`src-tauri/src/parsers/codex.rs`，搜 `parent_thread_id`、`fork_turns`
- codeg 怎么发 ACP fork：`src-tauri/src/acp/fork.rs`
- 本机 1.1.9 包位置：
  `C:\Users\63036\AppData\Roaming\npm\node_modules\@agentclientprotocol\codex-acp`
- 背景（**当背景读，不当事实用**）：SURVEY §2、§3、§4 的 codex 部分

**信包里的代码，不信 SURVEY，不信 registry 注释。**

## 5. 边界（不要越界）

- **不改代码、不改 node_modules、不 `npm install -g`、不升级本机 codex-acp。**
  用户在用这个包。取 1.4.0 只能取到临时目录。
- 不碰包 A（codeg parser 锚点）和包 B（claude 适配器）的地盘。
- **不要起 codex / codex-acp 进程做活体探测**——会消耗用户的 OpenAI 额度。需要活体
  验证的结论，写"需活体实测"并在 Room 里提出，由协调者决定。
- 不写代码、不设计通道、不选型。裁决之外的方案设计是协调者的判断题。
- 不跑 `pnpm tauri build`，不 push。

## 6. 工作方式

```bash
cd D:/code/revisiting/work/repo_audit/repos/codeg-wt/fork-rewind
git worktree add ../../codeg-wt/fork-codex-audit -b wt/fork-codex-audit wt/fork-rewind
cd ../../codeg-wt/fork-codex-audit
```

临时解包放 worktree **外面**（例如系统临时目录），别把 tarball 内容 commit 进仓库。

commit：

```
docs(fork): audit codex-acp thread/fork and the app-server channel
```

## 7. 报告格式（Room 回帖）

分支名、commit 哈希、**你实际审的版本号 + 怎么拿到的**、5 个问题的一句话答案各一行、
三选一裁决。不贴正文。

版本那一行是必填项——漏了我会打回。

## 8. 卡住怎么办

- 取 1.4.0 失败两次 → 停，报告，按 §2 第 2 条降级并全程标注。
- 包混淆到读不出 → 写"未能证实"，说明卡点。**诚实的半份答案 >> 编出来的完整答案。**
- 判断题上交，不要自己拍板。
