# 发现 C：codex-acp 1.4.0 的 ACP fork 声明 vs 内部 thread/fork

> 只读调研。不改代码。信包里的代码，不信 SURVEY，不信 registry 注释。
> 未起 codex / codex-acp 进程，无活体探测。

## 0. 实际审的版本（必填）

| 项 | 值 |
|---|---|
| 审的包 | `@agentclientprotocol/codex-acp@1.4.0`（= codeg `registry.rs` 钉的版本） |
| 怎么拿到 | **联网 `npm pack`，不解进全局、不 `npm install -g`** |
| 落地目录 | `C:\Users\63036\AppData\Local\Temp\codex-acp-140-audit\`（worktree **外面**） |
| 发布产物 | `package/package.json` `version: "1.4.0"`；主证据 `package/dist/index.js`（bundled，带 `// src/...` 段注释） |
| 依赖钉 | `"@openai/codex": "^0.147.0"`、`"@agentclientprotocol/sdk": "^1.3.0"` |
| GitHub 对照 | tag `v1.4.0` / commit `97d260e`（2026-08-16，release 说明只有 AIR `#403`）。**只用来解码 tarball 里被抹掉的 generated TS 类型**，不当独立事实源 |
| 本机全局 | `@agentclientprotocol/codex-acp@1.1.9`（`C:\Users\63036\AppData\Roaming\npm\node_modules\...`）。**只做对照，不当 1.4.0 结论** |
| 活体 | **没有**起进程 |

取包命令与原文输出：

```text
npm view @agentclientprotocol/codex-acp@1.4.0 version name dist.tarball dist.integrity engines --json
{
  "version": "1.4.0",
  "name": "@agentclientprotocol/codex-acp",
  "dist.tarball": "https://registry.npmjs.org/@agentclientprotocol/codex-acp/-/codex-acp-1.4.0.tgz",
  "dist.integrity": "sha512-Fo12tKFerUZltUgM8PxBiUBVxrYrIMK24zlOmQabnJtysPHrAwNq+CXMA0AMepPPEP3RlfDXOQPtjBGhv+Rd4w=="
}

npm pack @agentclientprotocol/codex-acp@1.4.0
npm notice 📦  @agentclientprotocol/codex-acp@1.4.0
npm notice filename: agentclientprotocol-codex-acp-1.4.0.tgz
npm notice shasum: f7c4364a9c5a6e2836e847cd9ac474b807c2e8ca
npm notice integrity: sha512-Fo12tKFerUZlt[...]QPtjBGhv+Rd4w==
agentclientprotocol-codex-acp-1.4.0.tgz
```

然后 `tar -xzf` 得到 `package/`。下面所有「包内路径」默认相对这个目录。

---

## 1. 对外声明 ACP `sessionCapabilities.fork` 吗？

**不声明。形状 = 缺席。** SDK 认识 `session/fork`，适配器的 `initialize` 不广告它，handler 表也不注册它。

### 1.1 `initialize` 实际返回的 sessionCapabilities

`dist/index.js:29673-29703`（`// src/CodexAcpServer.ts`）：

```js
async initialize(_params) {
  // ...
  return {
    protocolVersion: PROTOCOL_VERSION,
    agentInfo: { name: package_default.name, title: "Codex", version: package_default.version },
    agentCapabilities: {
      auth: { logout: {} },
      providers: {},
      loadSession: true,
      promptCapabilities: { embeddedContext: true, image: true },
      sessionCapabilities: {
        resume: {},
        list: {},
        close: {},
        delete: {},
        additionalDirectories: {}
      },
      mcpCapabilities: { acp: false, http: true, sse: false }
    },
    // ...
  };
}
```

五个键：`resume` / `list` / `close` / `delete` / `additionalDirectories`。**没有 `fork`。**

同文件 `_meta` 广告的是 `steering`、`goal`、JetBrains AIR（`sessionFailure` + `agentFileChangeReport`），也不是 ACP fork。

### 1.2 ACP `session/fork` 没有 handler

`dist/index.js:32759`（`// src/index.ts`）注册表（已展开）是：

`initialize`, `session.new`, `session.load`, `session.list`, `session.delete`, `session.resume`, `session.close`, `session.setMode`, `session.setConfigOption`, `authenticate`, `logout`, `providers.*`, `session.prompt`, `session.cancel`，外加若干 extMethod。

**没有 `methods.agent.session.fork`。**

### 1.3 包里出现的 `session/fork` 字串从哪来

全是 **bundled `@agentclientprotocol/sdk`**，不是适配器自己的实现：

| 位置 | 含义 |
|---|---|
| `dist/index.js:3710` | `AGENT_METHODS.session_fork: "session/fork"` |
| `:21067` | `methods.agent.session.fork` |
| `:21559` | `unstable_forkSession: requestSpec(AGENT_METHODS.session_fork, zForkSessionRequest)` |
| `:21898` | SDK legacy 方法集合含 `session_fork` |
| `:19566-19571` | `zForkSessionRequest = { sessionId, cwd, additionalDirectories?, mcpServers?, _meta? }` |

SDK schema 里 `zSessionCapabilities.fork` 是 optional（`:18725-18738`），默认 `undefined`。适配器没有把它填上。

codeg 今天发的 `session/fork`（`src-tauri/src/acp/fork.rs:19-32`，`ForkSessionRequest::new(session_id, cwd)`）打到 1.4.0 上，按这份静态注册表会走「方法未注册」。**需活体实测**才能看到具体 JSON-RPC 错误码，本包未跑。

对照（非结论）：本机全局 1.1.9 的 `initialize` 同样不广告 `fork`（`1.1.9 dist/index.js:28720-28726`，键集合相同），且 **1.1.9 dist 里搜不到 `threadFork` / `thread/fork`**。所以「不声明 ACP fork」不是 1.4.0 回归，1.4.0 新的是内部 `thread/fork`。

---

## 2. 内部怎么用 app-server `thread/fork`？

**唯一调用点**：AIR `agentFileChangeReport` 的只读审计 fork。不是用户可见的「分叉发送」。参数与 registry 注释一致，并多一个 `lastTurnId`。

### 2.1 调用点

`dist/index.js:27569-27657`（`// src/CodexAcpClient.ts`，`runAgentFileChangeReport`）：

```js
const forkPromise = this.codexClient.threadFork({
  threadId: params.sessionId,
  lastTurnId: params.turnId,
  cwd: params.workspace.cwd,
  approvalPolicy: "never",
  sandbox: "read-only",
  developerInstructions: AGENT_FILE_CHANGE_REPORT_DEVELOPER_INSTRUCTIONS,
  ephemeral: true
});
// ...
const fork = await budget.wait(forkPromise);
forkThreadId = fork.thread.id;
const turnPromise = this.codexClient.runTurn({
  threadId: forkThreadId,
  input: [{ type: "text", text: createAgentFileChangeReportPrompt(params.workspace), text_elements: [] }],
  cwd: params.workspace.cwd,
  approvalPolicy: "never",
  sandboxPolicy: { type: "readOnly", networkAccess: false },
  summary: "none",
  outputSchema: AGENT_FILE_CHANGE_REPORT_OUTPUT_SCHEMA
}, ...);
```

收尾是 `threadUnsubscribe`（`:27654-27656` / `:27669-27674`），不是 archive，也不是 rollback。

RPC 封装：`dist/index.js:32115-32116`（`// src/CodexAppServerClient.ts`）：

```js
async threadFork(params) {
  return await this.sendRequest({ method: "thread/fork", params });
}
```

全文件 `threadFork(` 调用点 **只有这一处**。

### 2.2 场景与门控

- 触发：`prompt()` 结束的 `finally`（`:31544-31551`）→ `publishAgentFileChangeReport`（`:30778`）。
- 门控：`clientSupportsAgentFileChangeReports(this.clientCapabilities)`（`:31201`）。客户端必须在 `initialize` 的 `_meta.jetbrains.air.capabilities` 里带 `"agentFileChangeReport"`，**并且** 该次 `session/prompt` 的 `_meta` 带合法 `agentFileChangeReportRequest`。codeg `connection.rs:3186-3212` 明确不广告这项——所以这条内部 fork **在 codeg 连接上不会跑**。
- developerInstructions 原文（`:26714-26718`）：只读文件变更审计，禁止改文件。

这就是 registry 注释说的 AIR `agentFileChangeReport`。注释里的三元组（`approvalPolicy: "never"` / `sandbox: "read-only"` / `ephemeral: true`）与源码一致。注释没写的是 **`lastTurnId: params.turnId`**（按 turn 截断，含该 turn）。

### 2.3 `thread/fork` 参数形状（generated 类型，GitHub `v1.4.0`）

npm tarball **不含** `src/app-server/`。下列类型来自同 tag 的 `src/app-server/v2/ThreadForkParams.ts`（`codex app-server generate-ts` 产物，对应 `@openai/codex ^0.147.0`）：

```ts
export type ThreadForkParams = {
  threadId: string,
  /** Optional last turn id to fork through, inclusive.
   *  When specified, turns after `last_turn_id` are omitted from the fork. */
  lastTurnId?: string | null,
  model?: string | null,
  modelProvider?: string | null,
  cwd?: string | null,
  approvalPolicy?: AskForApproval | null,
  sandbox?: SandboxMode | null,
  developerInstructions?: string | null,
  ephemeral?: boolean,
  threadSource?: ThreadSource | null,
  // plus config / baseInstructions / approvalsReviewer / serviceTier
};
```

文件头注释还写了「按 path fork」的第二种方式，但 **类型字段里没有 `path`**，也 **没有 `messageId`**。

`Thread`（`src/app-server/v2/Thread.ts`）把两种谱系分开：

```ts
/** Source thread id when this thread was created by forking another thread. */
forkedFromId: string | null,
/** The ID of the parent thread. This will only be set if this thread is a subagent. */
parentThreadId: string | null,
```

**对 SURVEY「thread/fork{threadId, messageId}」的纠正**：1.4.0 / Codex 0.147 类型里的按历史截断键是 **`lastTurnId`（turn 级，inclusive）**，不是 messageId。按消息 fork **未能证实**。`lastTurnId` 是否真按文档截断 → **需活体实测**。

---

## 3. `thread/rollback` 有痕迹吗？

**适配器代码：无调用、无封装。生成类型：有，但标了 DEPRECATED。**

### 3.1 dist 里唯一命中

`dist/index.js:23688`（`// src/CodexEventHandler.ts`）：

```js
var STRING_CODEX_ERROR_CATEGORIES = {
  // ...
  threadRollbackFailed: "provider_error",
  sandboxError: "provider_error",
  other: "provider_error"
};
```

这是把 Codex 错误码 `thread_rollback_failed` 映射成 AIR `provider_error`。**不是 RPC 调用。**

`CodexAppServerClient` 的方法表有 `thread/start|resume|fork|list|read|archive|unsubscribe|compact/start|goal/*`，**没有 `thread/rollback`**（`:32109-32150`）。

对 `dist/index.js` 搜 `thread/rollback`：**0 命中**（除上面 camelCase 错误码）。

### 3.2 生成类型里的形状（GitHub `v1.4.0`）

`src/app-server/v2/ThreadRollbackParams.ts`：

```ts
/**
 * DEPRECATED: `thread/rollback` will be removed soon.
 */
export type ThreadRollbackParams = {
  threadId: string,
  /** The number of turns to drop from the end of the thread. Must be >= 1.
   *  This only modifies the thread's history and does not revert local file changes. */
  numTurns: number,
};
```

`ThreadRollbackResponse` 返回带 `turns` 的 `Thread`。`Thread.ts` 注释仍把 `thread/rollback` 列为会填充 `turns` 的方法之一。

`CodexErrorInfo` 含 `"thread_rollback_failed"`。

### 3.3 对 rewind 的含义

- 适配器 **没有** 把 rollback 接到 ACP 或内部逻辑上。
- Codex 0.147 类型里 rollback **还在，但已标即将删除**，且只丢 turn、**不回文件**。
- 所以「rewind 有没有原生底座」：协议层曾有 `numTurns` 回退，**不宜当长期底座**；codeg 今天从 ACP 也摸不到它。

`numTurns` 是否仍被 0.147 app-server 接受 → **需活体实测**（本包按纪律不起进程）。

---

## 4. app-server 通道怎么开？

**每个 adapter 进程 spawn 一个 `codex app-server` 子进程；JSON-RPC 走该子进程 stdin/stdout；所有 ACP session 复用这一条连接。不是每 session 一条，也不是复用调用方已有的连接。**

`dist/index.js:22067-22086`（`// src/CodexJsonRpcConnection.ts`）：

```js
function startCodexConnection(codexPath, env) {
  const spawnEnv = env ?? process.env;
  let codex;
  if (codexPath) {
    codex = process.platform === "win32"
      ? spawn(`"${codexPath}" app-server`, { shell: true, env: spawnEnv })
      : spawn(codexPath, ["app-server"], { env: spawnEnv });
  } else {
    const bundledCodexPath = createRequire(import.meta.url).resolve("@openai/codex/bin/codex.js");
    codex = spawn(process.execPath, [bundledCodexPath, "app-server"], { env: spawnEnv });
  }
  attachLogs(codex);
  const reader = createJSONRPCReader(codex.stdout);
  const writer = createJSONRPCWriter(codex.stdin);
  let connection = rpc.createMessageConnection(reader, writer);
  connection.listen();
  // ...
  return { connection, process: codex };
}
```

启动（`:32731` / `:32747-32750`，`// src/index.ts`）：

```js
const codexConnection = startCodexConnection(codexPath);
function createAgent(connection) {
  const appServerClient = new CodexAppServerClient(codexConnection.connection);
  const codexClient = new CodexAcpClient(appServerClient, config2, modelProvider);
  return new CodexAcpServer(connection, codexClient, ...);
}
```

`startCodexConnection` 在 `startAcpServer()` 里 **调用一次**。ACP client 连的是 adapter 的 stdin/stdout；adapter 再连子进程。codeg **看不到** 这条 app-server 连接，除非 adapter 用 ACP 方法转发出去。

README 原话（`package/README.md:7-8`）：

> `codex-acp` is a stdio ACP agent server. It starts the Codex App Server, translates ACP requests into Codex operations, and maps Codex events back into the client.

对 codeg 复刻成本：spawn 配方已经写在适配器里（`CODEX_PATH` 或 bundled `@openai/codex/bin/codex.js` + `app-server` + vscode-jsonrpc）。再开第二条通道 = **第二个 Codex 进程 + 第二份 auth 状态**，和现有 ACP 会话并行。adapter 已经占着第一条。

---

## 5. codeg 的 codex parser 认识 fork 产物到什么程度？

**认识的是 subagent 分叉，目的是「藏起来 / 不出统计」，不是「支持 fork」。用户向 `forkedFromId` 谱系完全没接。`fork_turns` 没有从 `session_meta` 读过。**

### 5.1 认什么

`is_forked_thread_header`（`src-tauri/src/parsers/codex.rs:1824-1845`）：

```rust
/// Whether a transcript's opening record declares it a forked thread
/// (`session_meta.parent_thread_id`) — codex 0.147's sub-agent shape, where the
/// child is a rollout of its own and `fork_turns` copies the parent's history
/// into its head.
fn is_forked_thread_header(value: &serde_json::Value) -> bool {
    value.get("type").and_then(|t| t.as_str()) == Some("session_meta")
        && value
            .pointer("/payload/parent_thread_id")
            .is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty()))
}
```

唯一消费者：`parse_codex_subagent_stats`（`:1907-1914`）。命中则 **整份 stats 返回 `None`**，避免把父历史里的 tool call 算到孩子头上。单测 `forked_child_transcript_yields_no_stats`（`:7066-7103`）钉的就是这个。

`codex_session_is_harness_internal`（`:335-360`）三条任一为真即内部会话：

1. `thread_source == "subagent"`
2. 非空 `parent_thread_id`
3. `source.subagent.thread_spawn` 存在

list 和 detail 都写 `harness_internal`（`:120`、`:2280`）。前端 `app-workspace-store.ts:304` 直接把 `harness_internal` 行从侧栏拿掉。

这和 Codex `Thread.parentThreadId` 注释（「only set if this thread is a subagent」）对齐。

### 5.2 不认什么

| 缺口 | 证据 |
|---|---|
| **不读 `fork_turns` 字段** | 全仓库只有注释里的概念句（`:1826`）和 `spawn_agent` 测试夹具 `"fork_turns":"all"`（`:6753`）。没有 `payload.fork_turns` 解析。SPEC「已经处理了 parent_thread_id + fork_turns」里，**后半句不成立**。 |
| **不把 `forked_from_id` 当谱系** | 单测明确：`forked_from_id` + `thread_source: "user"` **不是** harness_internal（`:4311-4317`）。解析路径没有任何 `forked_from_id` 读取。用户向 `thread/fork` 产物会当普通会话出现，没有 parent 链接。 |
| **不暴露 `parent_thread_id` 为会话父 id** | 只用来打内部标记 / 拒统计。 |
| **没有原生消息锚点** | 投影 id 是位置合成：`format!("user-{}", messages.len())`（`:2400`）、`format!("assistant-{}", ...)`（`:2429`）。`thread/fork.lastTurnId` 要用的 turn id，parser 也没往 `UnifiedMessage.id` 里塞。 |
| **不是 fork 能力** | 注释自己写了：forked thread 不出统计，是因为父历史被 replay 进孩子文件。 |

---

## 6. 裁决

**给 codex-acp 提能力。人天级写映射，日历可能到周级（等 maintainer）。**

一句话：1.4.0 **不**声明也不处理 ACP `session/fork`，所以「ACP 够用」不成立；原生入口是 adapter 已经在用的 app-server `thread/fork`（按 **`lastTurnId` turn 级** 截断，不是 messageId），而这条连接在 adapter 子进程里、ACP 客户端碰不到——与其再 spawn 第二条 app-server（第二份进程/auth，周级），不如让 adapter 把已有的 `threadFork` 接上 ACP `session/fork`（handler + `sessionCapabilities.fork`；按 turn 截断走 `_meta`，因为 ACP `zForkSessionRequest` 只有 `sessionId/cwd/additionalDirectories/mcpServers/_meta`）。

三选一对照：

| 选项 | 为什么选 / 不选 |
|---|---|
| ACP 够用 | 否。`initialize` 无 `fork`，无 `session/fork` handler。codeg `fork.rs` 打过去按静态代码会方法未注册。 |
| 必须开 app-server 第二通道 | 能立刻打到 `lastTurnId`，但重复 adapter 已经 spawn 的那条通道。值得当「上游不接 PR」的退路，不是第一入口。 |
| **给 codex-acp 提能力** | **选这个。** adapter 已有 `CodexAppServerClient.threadFork` 和 AIR 示范调用。缺的是对外 ACP 面。 |

必须分开钉死、不能混用的两句话：

1. **对外声明**：不声明 ACP fork。
2. **内部实现**：用 `thread/fork` 做 AIR 只读审计，codeg 不广告 AIR 所以这条不会跑。

另外三条判断题上交（本包不拍板）：

- **按消息 fork 在 1.4.0 类型里不存在**（无 `messageId`）。SURVEY / desktop-cc-gui 的 `thread/fork{messageId}` 与这份 generated schema 对不上。最近的原生能力是 **按 turn**。
- **`thread/rollback` 已 DEPRECATED**，且 adapter 没封装；rewind 不宜押在它上面。
- 用户向 fork 的磁盘标记是 `forkedFromId`，codeg parser 现在当普通会话，不建谱系。

---

## 7. 需活体实测（本包按纪律未做，请协调者决定）

1. 向 1.4.0 发 ACP `session/fork`，确认 JSON-RPC 错误码（推断 method-not-found）。
2. `thread/fork` + `lastTurnId` 是否真按「含该 turn、之后丢弃」截断。
3. 用户向 fork 落盘的 `session_meta` 是 `forked_from_id` 还是别的键。
4. 0.147 app-server 是否仍接受已 deprecated 的 `thread/rollback`。
5. 未写入类型的 `messageId` 会不会被 app-server 默默接受（静态代码看不到）。
