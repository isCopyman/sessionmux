# FINDINGS B：claude-agent-acp 0.69.0 fork 能力实测

> 只读。不改代码、不改 node_modules、不装包、不起 claude-agent-acp 进程。
> 审包版本以本机全局 `package.json` 为准；信包里的代码，不信 SURVEY / registry 注释。

## 0. 审了什么

| 项 | 值 |
|---|---|
| 适配器 | `C:\Users\63036\AppData\Roaming\npm\node_modules\@agentclientprotocol\claude-agent-acp` |
| `package.json` version | **0.69.0**（= codeg `acp/registry.rs:458-460` 钉的版本，继续） |
| 入口 | `dist/acp-agent.js` + `dist/acp-agent.d.ts`（tsc 产物，注释保留，未混淆） |
| ACP TS SDK | 同包 `node_modules/@agentclientprotocol/sdk@1.3.0` |
| Agent SDK | 同包 `node_modules/@anthropic-ai/claude-agent-sdk@0.3.232` |
| 全局 `@anthropic-ai` | `%APPDATA%\npm\node_modules\@anthropic-ai` **目录存在但为空**。实际审的是适配器钉死的 0.3.232，不是另一份全局 SDK。 |
| ACP Rust schema | cargo registry `agent-client-protocol-schema-0.11.7` `src/agent.rs:1253-1280`（SURVEY 行号复核通过） |
| 未做 | 起 agent 进程、读用户 `~/.claude/projects` 活体 jsonl、改包、写 PR |

适配器 `dependencies`：

```json
"@agentclientprotocol/sdk": "1.3.0",
"@anthropic-ai/claude-agent-sdk": "0.3.232"
```

适配器 **import 了** `query` / `listSessions` / `getSessionMessages` / `getSessionInfo` / `deleteSession`，**没有 import** SDK 的 `forkSession()` 函数（`dist/acp-agent.js:2`）。

---

## 1. `initialize` 回复里 `sessionCapabilities.fork` 在不在？什么形状？

**在。空对象 `{}`，不是布尔。** 嵌在 `agentCapabilities.sessionCapabilities` 下。

`dist/acp-agent.js:688-718`（`ClaudeAcpAgent.initialize` 的 return）：

```javascript
return {
    protocolVersion: 1,
    agentCapabilities: {
        // ...
        loadSession: true,
        sessionCapabilities: {
            additionalDirectories: {},
            close: {},
            delete: {},
            fork: {},
            list: {},
            resume: {},
        },
    },
    // ...
};
```

形状与 ACP schema 一致：`SessionForkCapabilities` 只有可选 `_meta`，**Supplying `{}` means the agent supports forking sessions**（`@agentclientprotocol/sdk@1.3.0` `dist/schema/types.gen.d.ts:1653-1761`）。

codeg 探测是 `.fork.is_some()`（`src-tauri/src/acp/connection.rs:4264-4268`）。`{}` 会让这条为 true。RFC / SURVEY 标的「0.69 需实测」空白可以闭合：**0.69.0 仍声明 fork。**

---

## 2. `session/fork` 处理代码在哪、做了什么？转发给 SDK 还是自己实现？

**处理函数：`ClaudeAcpAgent.unstable_forkSession`。不是自己拷文件，也不是调用 SDK 的 `forkSession()`。是 `query({ resume, forkSession: true, sessionId: 新 UUID })`，由 CLI 子进程做 fork。**

注册（`dist/acp-agent.js:6815-6819`）：

```javascript
const connection = acpAgent({ name: "claude-code-acp" })
    .onRequest(methods.agent.initialize, (ctx) => agent.initialize(ctx.params))
    .onRequest(methods.agent.session.new, (ctx) => agent.newSession(ctx.params))
    .onRequest(methods.agent.session.load, (ctx) => agent.loadSession(ctx.params))
    .onRequest(methods.agent.session.fork, (ctx) => agent.unstable_forkSession(ctx.params))
```

方法名常量：`AGENT_METHODS.session_fork === "session/fork"`（sdk `dist/schema/index.js:17`）。请求体先经 `zForkSessionRequest` 校验（sdk `dist/acp.js:597`）。

处理本体（`dist/acp-agent.js:756-770`）：

```javascript
async unstable_forkSession(params) {
    const response = await this.createSession({
        cwd: params.cwd,
        mcpServers: params.mcpServers ?? [],
        additionalDirectories: params.additionalDirectories,
        _meta: params._meta,
    }, {
        resume: params.sessionId,
        forkSession: true,
    });
    setTimeout(() => {
        this.sendAvailableCommandsUpdate(response.sessionId);
    }, 0);
    return response;
}
```

`createSession` 关键分支（`dist/acp-agent.js:4678-4689, 4936-4955`）：

```javascript
// We want to create a new session id unless it is resume,
// but not resume + forkSession.
let sessionId;
if (creationOpts.forkSession) {
    sessionId = randomUUID();
} else if (creationOpts.resume) {
    sessionId = creationOpts.resume;
} else {
    sessionId = randomUUID();
}
// ...
...creationOpts,          // { resume: 父 sessionId, forkSession: true }
abortController,
};
if (creationOpts?.resume === undefined || creationOpts?.forkSession) {
    options.sessionId = sessionId;   // fork 路径强制写入新 UUID
}
const q = query({ prompt: input, options });
```

到 SDK 之后变成 CLI 参数（`claude-agent-sdk@0.3.232` `sdk.mjs`，`ProcessTransport.initialize` 解构 `resume:v` 等）：

```javascript
if (E) Y.push("--continue");
if (v) Y.push(`--resume=${v}`);
// ...
if (this.options.forkSession) Y.push("--fork-session");
if (this.options.resumeSessionAt) Y.push(`--resume-session-at=${this.options.resumeSessionAt}`);
if (this.options.resumeDropsTurn !== void 0) Y.push(`--resume-drops-turn=${this.options.resumeDropsTurn}`);
if (this.options.sessionId) Y.push(`--session-id=${this.options.sessionId}`);
```

因此 0.69.0 的 `session/fork` = **整会话 head fork**：`--resume=<父 id> --fork-session --session-id=<新 UUID>`。处理函数**没有**传 `resumeSessionAt`。

---

## 3. `ForkSessionRequest._meta` 会被读取吗？

**会被解析，会整包透传到 `createSession`，fork 处理函数本身不读任何消息级键。`_meta.messageUuid` / `upToMessage` / `resumeSessionAt` 在 adapter 源码里零引用。**

协议层（schema 0.11.7 `agent.rs:1253-1280`，行号与 SURVEY 一致）：

```rust
pub struct ForkSessionRequest {
    pub session_id: SessionId,
    pub cwd: PathBuf,
    pub additional_directories: Vec<PathBuf>,
    pub mcp_servers: Vec<McpServer>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub meta: Option<Meta>,
}
```

`ForkSessionRequest::new` 把 `meta: None`（同文件 1285-1293）。codeg 今天发 fork 就走这条构造器（`src-tauri/src/acp/fork.rs:24`），**根本不带 `_meta`**。

ACP TS 校验（sdk `dist/schema/zod.gen.js:2497-2503`）：

```javascript
export const zForkSessionRequest = z.object({
    sessionId: zSessionId,
    cwd: z.string(),
    additionalDirectories: defaultOnError(vecSkipError(z.string()).optional(), () => []),
    mcpServers: defaultOnError(vecSkipError(zMcpServer).optional(), () => []),
    _meta: defaultOnError(z.record(z.string(), z.unknown()).nullish(), () => undefined),
});
```

`_meta` 是 `Record<string, unknown>`，**不会被 schema 丢掉**。

adapter 实际读了哪些键（全部在 `createSession`，`dist/acp-agent.js:4721-4765, 4940-4943`）：

| `_meta` 键 | 行为 |
|---|---|
| `systemPrompt` | 覆盖 SDK system prompt |
| `claudeCode.options` | **整包 spread 进** `query()` 的 `Options` |
| `claudeCode.emitRawSDKMessages` | 存到 Session |
| `disableBuiltInTools` | 旧缩写，等价 `tools: []` |
| `additionalRoots` | 与 ACP `additionalDirectories` 合并 |
| **`messageUuid` / `upToMessage` / `upToMessageId` / `resumeSessionAt`** | **不读** |

`_meta.claudeCode.options` 的类型是 SDK `Options`（`dist/acp-agent.d.ts:504-521` `NewSessionMeta`）。注释列出 ACP 会忽略/接管的字段是 `cwd` / `includePartialMessages` / `allowDangerouslySkipPermissions` / `permissionMode` / `canUseTool` / `executable`——**名单里没有 `resumeSessionAt`**。

展开顺序（`dist/acp-agent.js:4817-4937`）：

```javascript
const userProvidedOptions = sessionMeta?.claudeCode?.options;
const options = {
    // ...
    ...userProvidedOptions,   // resumeSessionAt 若出现会落在这里
    // ... ACP 强制覆盖 cwd/mcp/hooks/extraArgs 等
    ...creationOpts,          // { resume, forkSession: true } —— 不覆盖 resumeSessionAt
    abortController,
};
```

所以：

- 丢弃？否。
- 根本没解析？否（zod 收下，再整包交给 `createSession`）。
- 透传？**对 `claudeCode.options` 是透传（spread）**；对 SURVEY 设想的 `_meta.messageUuid` **是未读字段，等于没有**。

adapter 自己也知道消息级 fork/rewind 还没接线（`dist/acp-agent.d.ts:473-489`）：

```
Maps the ACP `messageId` we expose to clients … to the SDK message uuid that
the Agent SDK's rewind/resume APIs key on (`Query.rewindFiles` takes a
user-message uuid; `resumeSessionAt` takes an `SDKAssistantMessage.uuid`).
…
NOT READ YET — recorded now so the mapping exists if/when we wire up
fork/rewind.
```

`messageIdToUuid` 在 live consumer（`acp-agent.js:2979-2985`）和 `replaySessionHistory`（`3868-3876`）里写入，在 `createSession` 里初始化为空 Map（`5154`），**没有任何 fork/rewind 读取点**。

---

## 4. SDK 的 `forkSession` 接受消息级参数吗？

**接受，但那是另一条 API；adapter 没用它。query Options 另有 `resumeSessionAt`。**

### 4.1 独立函数 `forkSession(sessionId, { upToMessageId, title })`

`sdk.d.ts:688-712`：

```typescript
/**
 * Fork a session into a new branch with fresh UUIDs.
 * Copies transcript messages … remapping every message UUID and preserving
 * the parentUuid chain. Supports `upToMessageId` for branching from a
 * specific point … Forked sessions start without undo history …
 */
export declare function forkSession(
    _sessionId: string,
    _options?: ForkSessionOptions,
): Promise<ForkSessionResult>;

export declare type ForkSessionOptions = SessionMutationOptions & {
    /** Slice transcript up to this message UUID (inclusive). If omitted, full copy. */
    upToMessageId?: string;
    title?: string;
};
```

实现（`sdk.mjs` 函数 `RK` / `PK`）要点：

- `upToMessageId` 必须是 UUID；按 `uuid` 在非 sidechain transcript 里 **inclusive slice**。
- 每条消息 **remap 新 uuid**，重写 `parentUuid` / `logicalParentUuid` / `sessionId`。
- 每条带 `forkedFrom: { sessionId: 父, messageUuid: 原 uuid }`。
- **最后一条** timestamp 改成 `new Date().toISOString()`，前面的保留原时间戳。
- 不拷 file-history / undo。可选 `history-suppression`（`cause: "fork_inherit"`）。
- 末尾追加 `custom-title`（默认 `"… (fork)"`）。
- **没有** `queue-operation` 文件头。

这条 API **不在 adapter 的 import 列表里**，`session/fork` 走不到这里。

### 4.2 `query()` Options：`forkSession?: boolean` + `resumeSessionAt?: string`

`sdk.d.ts:1522-1526, 1836-1843`：

```typescript
/**
 * When true, resumed sessions will fork to a new session ID rather than
 * continuing the previous session. Use with `resume`.
 */
forkSession?: boolean;

/**
 * When resuming, only resume messages up to and including the message with
 * this UUID. Use with `resume`. … Accepts any chain-entry UUID — typically
 * `SDKAssistantMessage.uuid` …
 */
resumeSessionAt?: string;
```

同文件 1878-1884 注明 `resumeSessionAt` + `resumeDropsTurn` 是 **PRINT/HEADLESS 通道**（Agent SDK `query()` / ProcessTransport 属于这条）。interactive `claude --resume` 会忽略截断。adapter 用的就是 `query()`，所以若 Options 里带了 `resumeSessionAt`，理论上会被 CLI 吃掉。

**没有** `messageUuid` / `upToMessage` 这两个 Options 字段名。消息级定位的官方名字是：

- 函数 API：`upToMessageId`
- query/CLI：`resumeSessionAt` → `--resume-session-at=`

另：`Query.rewindFiles(userMessageId)` 是**文件检查点回卷**，不是会话 fork（`sdk.d.ts:2557-2567`）。

---

## 5. fork 在文件层做了什么？和 `background_watch.rs:1256-1266` 对不对得上？

**adapter 自己不写会话文件**（`acp-agent.js` 对文件系统只有 `fs.stat(cwd)`）。文件层有两条不同的实现，ACP `session/fork` 只走其中一条。

### 5.1 ACP 实际路径 = CLI `--fork-session`（未能从本包源码还原布局）

`session/fork` → `query({ resume, forkSession: true, sessionId })` → 子进程 `claude --resume=<父> --fork-session --session-id=<新>`。

jsonl 怎么拷、头上写什么、uuid 变不变，都在 **Claude CLI 二进制**里，不在 `claude-agent-acp` 的 JS、也不在 SDK 的 `forkSession()` 函数。本包 `sdk.mjs` 搜不到 `queue-operation`。

**未能证实**：`--fork-session` 产物是否仍是「文件头 queue-operation + 原文时间戳拷贝」。要钉死需要读一次真实 fork 出的 jsonl，或起进程（规格禁止后者）。

### 5.2 SDK `forkSession()` 路径（adapter 没用）——和 codeg 已知布局对不上

| | codeg `background_watch.rs:1256-1266` + 回归测试 `:1544-1576` | SDK `forkSession()` `PK()` |
|---|---|---|
| 拷贝父 transcript | 是 | 是（滤掉 sidechain / progress） |
| 原始时间戳 | **全部保留** | **最后一条改成 fork 时刻** |
| 文件头元数据 | `queue-operation` / `mode` 写在 **HEAD**，时间戳 = fork 时刻 | 无 `queue-operation`；`custom-title` 在 **末尾** |
| uuid | 测试夹具按「原 uuid 原样拷」建模 | **全部 remap**，另写 `forkedFrom.messageUuid` |
| 谁在用 | codeg 观察的 **ACP/CLI fork 产物** | 独立文件 API；**0.69.0 adapter 不调用** |

codeg 那段注释描述的是 CLI `--fork-session` 的磁盘形状，不是 SDK `forkSession()`。两者已经不是同一条路。回归测试（`:1559-1564`）明确假设：

```
queue-operation (fork 时刻戳, 文件头)
copied user/assistant (原时间戳)
new user (fork 之后)
```

这与 `PK()` 的「remap + 末条改戳 + 尾部 custom-title」冲突。在没有活体 `--fork-session` 样本前，**只能说：ACP 路径的文件层不在本包；SDK 函数路径对不上 codeg 布局。**

---

## 6. 裁决

**需上游改动。**

一句话：`_meta` 缝隙是真的（zod 解析 + 整包交给 `createSession`），但 SURVEY 猜的键 `_meta.messageUuid` 和猜的 API（adapter 调 SDK `forkSession({upToMessageId})`）在 0.69.0 **都不存在**；adapter 的 fork 只发 `--resume + --fork-session`，自己标注 `messageIdToUuid`「NOT READ YET」。要把消息级锚点接到 `--resume-session-at` 或 `forkSession({upToMessageId})`，必须改 claude-agent-acp（或等它接线）。

不是「此路不通」：schema 有 `_meta`，SDK 有消息级定位，adapter 还预埋了 ACP `messageId` → SDK uuid 表。不是「开箱可行」：今天发 `_meta.messageUuid` 会被忽略。

### 未活体验证的旁路（供协调者判断，不当成已证实可用）

若 codeg 发：

```json
{ "_meta": { "claudeCode": { "options": { "resumeSessionAt": "<SDK 链 uuid>" } } } }
```

`createSession` 会把 `resumeSessionAt` spread 进 `query()` Options，SDK 会加 `--resume-session-at=`。这是 `NewSessionMeta.claudeCode.options` 的副作用，**不是文档化的 fork 契约**，且：

1. 必须是 SDK 链 uuid，不是 ACP 暴露给客户端的 assistant `msg_…`（`messageIdForGrouping`，`acp-agent.js:6103-6113`）。
2. 从未跑过，规格禁止起进程。
3. codeg `fork.rs` 现在 `ForkSessionRequest::new`，`_meta` 为 None。

---

## 7. 对 SURVEY §6 切片 2 的纠偏（事实，不是方案）

SURVEY 原文：「给 claude-agent-acp 加 `_meta.messageUuid` 让 SDK 原生 `forkSession` 定位历史 UUID」。

实测差了三处：

1. adapter **不调用** `forkSession()`，调用 `query({ forkSession: true })`。
2. 消息级字段名是 `resumeSessionAt` / `upToMessageId`，不是 `messageUuid`。
3. ACP 客户端手里的 id 经常是 `msg_…`，SDK 要的是链 uuid——adapter 已经在攒对照表，但没读。

head fork（无锚点）在 0.69.0 **已经可用**，codeg 现在走的就是这条。
