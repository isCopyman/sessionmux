# SessionMux / Codeg Desktop 开发、调试与验收手册

> 状态：Active（2026-08-16）
>
> 适用对象：继续维护 SessionMux 的人类与 Agent
>
> 当前产品载体：Tauri 2 Desktop；代码、进程名和配置中仍保留 `Codeg` 名称

本手册不覆盖 `codeg-server` Web 产品验收。Web 页面最多作为隔离前端问题的辅助工具，不能
替代真实 Desktop 结论。

## 1. 这份手册解决什么问题

SessionMux 的主要产品不是普通网页，而是由 Tauri、WebView2、Rust、SQLite 和不同 Harness
共同组成的桌面应用。只运行前端测试或在 Chrome 中打开页面，无法证明以下行为成立：

- Tauri IPC 与 Rust 状态正确；
- Desktop 使用了正确的 SQLite 数据目录；
- 原生 Session 能创建、恢复和继续；
- 工作台在真实 WebView 中能切换、拖放和恢复；
- 关闭、重启、原生窗口拖动和系统对话框行为正确。

因此，开发流程的目标不是“测试越多越好”，而是用最少但足够的证据回答：

> 这次改动在真实 Desktop 中解决了用户场景吗？重启后仍成立吗？有没有产生新的控制台、
> Rust 或持久化错误？

Web/浏览器模式只能用于隔离前端问题或快速预览，不能替代 Desktop 验收。

## 2. 先理解实际运行链路

```text
React / Next UI
      ↓ Tauri invoke / event
Tauri Desktop + WebView2
      ↓
Rust AppState / commands / services
      ↓
SQLite + ACP ConnectionManager + Harness CLI
```

当前仓库的关键事实：

- `pnpm tauri dev` 会执行 `pnpm tauri:before-dev`；
- `tauri:before-dev` 先准备 `codeg-mcp` sidecar，再启动 Next dev server；
- Desktop WebView 在开发时加载 `http://localhost:3000`；
- Debug Tauri 使用 `codeg-dev.db`，Release 使用 `codeg.db`；
- `CODEG_DATA_DIR` 决定 SQLite 和多数 Desktop 资产的根目录；
- `CODEG_HOME` 若与 `CODEG_DATA_DIR` 不同，附件、宠物、日志等可能与数据库分裂；
- Debug 允许与已安装的 Release 并存，但二者仍可能共享数据目录中的非数据库资产。

## 3. 四层证据，不要混为一谈

| 层级 | 能证明什么 | 不能证明什么 |
|---|---|---|
| 前端组件测试 | Store、组件、事件处理和局部交互契约 | Tauri IPC、原生窗口、真实持久化 |
| Rust 测试与编译 | 数据模型、命令、迁移和 Desktop feature 能编译 | 页面真实可用、布局视觉正确 |
| 真实 Tauri WebView | UI、Tauri IPC、Rust 后端、真实 WebView 和数据链路联通 | 窗口边框、系统拖动、锁屏等纯原生行为 |
| 原生窗口观察 | 窗口拖动、最大化、多窗口、系统对话框和最终使用体验 | 不自动证明内部状态机正确 |

验收结论必须写清自己达到哪一层。例如：

- “Vitest 通过”不是“Desktop 通过”；
- “`cargo check` 通过”不是“功能可用”；
- “Chrome 页面截图正确”不是“Tauri WebView 正确”；
- “Tauri 截图正确”也不能单独证明重启恢复。

## 4. 一轮 Desktop 开发的标准流程

### 4.1 开工前保护工作树

```powershell
git status --short
git branch --show-current
git log -3 --oneline
git remote -v
```

规则：

- 不重置、不清理、不覆盖不属于当前任务的修改；
- 先记录本批允许修改的文件；
- 当前公开 fork 使用 `sessionmux` remote；除非用户明确要求，不向上游 `origin` 推送；
- Rust 格式化优先针对单文件，避免 `cargo fmt --all` 制造全仓噪音；
- 提交时显式暂存文件，不使用会吸入截图、临时数据和他人文档的宽泛命令。

### 4.2 准备依赖与静态资产

首次运行或依赖变化后：

```powershell
pnpm install --frozen-lockfile
```

当默认 Tauri Rust 编译提示 `../out` 不存在时，先生成静态输出：

```powershell
pnpm build
```

不要把 `resource path ..\out doesn't exist` 误判成 Rust 业务代码错误。它表示 Tauri build script
在真正检查 Rust 代码前缺少前端资源。

### 4.3 为验收使用隔离数据目录

不要直接拿用户正在使用的正式数据做自动化。最安全的是建立空白 Desktop 验收目录：

```powershell
$desktopData = Join-Path (Get-Location) '.artifacts/desktop-validation/data'
New-Item -ItemType Directory -Force -Path $desktopData | Out-Null
$desktopData = (Resolve-Path -LiteralPath $desktopData).Path
$env:CODEG_DATA_DIR = $desktopData
$env:CODEG_HOME = $desktopData
```

两者指向同一绝对路径，避免数据库与附件、日志等状态分裂。Debug Desktop 会在其中使用
`codeg-dev.db`。

若必须使用真实历史验证 Session 恢复：

1. 完全停止所有正在使用源数据目录的 Codeg 进程；
2. 对整个数据目录制作静态快照，再让测试实例使用快照；
3. 不要在数据库仍打开时只复制一个 SQLite 主文件并忽略 WAL/SHM；
4. 不要让两个 Desktop/Server 实例同时对同一数据目录运行 Automation 或 Work Task 引擎。

结束验收后先停止进程，再处理临时目录。不要在进程仍运行时递归删除数据目录。

### 4.3.1 测试产物统一放在哪里

仓库根目录不再堆放 `.tmp-*.png`、一次性 `.mjs`、CDP 返回 JSON 或测试数据库。统一规则：

- 本地、可删除的完整验收产物放在 `.artifacts/desktop-validation/<功能名>/`；
- 一次性 CDP 脚本也放在同一目录，用完可连同场景目录删除；
- 只有会长期复用的脚本才进入 `scripts/`，并同时补充参数说明和错误处理；
- 只有需要进入产品文档的精选截图才放入 `docs/`，普通调试截图不提交；
- `.artifacts/`、历史根目录 `.tmp-*`、Playwright 默认输出均由 `.gitignore` 排除。

每次验收结束应保留一个简短结果文件，记录分支、commit、场景、断言、截图名和新错误；无需
保留几十张过程截图。清理前必须先确认对应 Desktop/Server 进程已经停止。

### 4.4 只跑与改动相称的自动测试

前端局部改动先跑相关测试：

```powershell
pnpm exec vitest run <相关测试文件...>
pnpm exec eslint <本批修改的前端文件...> --max-warnings=0
pnpm exec tsc --noEmit
```

涉及共享 Store、布局、路由或数据类型时再扩大到：

```powershell
pnpm test
pnpm build
```

Rust 局部改动先跑相关模块：

```powershell
Set-Location src-tauri
cargo test --features test-utils <测试过滤词>
cargo check
```

若修改了 Server、共享 Service、MCP companion 或无 Tauri 运行时路径，再补：

```powershell
cargo check --no-default-features --bin codeg-server
cargo check --no-default-features --bin codeg-mcp
```

原则：

- CSS 小改不必每次跑完整 Rust 全库；
- 数据库迁移、共享状态或消息调度不能只跑前端测试；
- 不为追求“测试很多”反复运行没有新增信息的全量测试；
- Windows 上若测试二进制在启动时报告 `STATUS_ENTRYPOINT_NOT_FOUND`，要记录为本机 DLL/
  运行环境阻塞；不能谎称测试通过，也不能把它误写成某个测试断言失败。

### 4.5 启动真实 Desktop，并打开可重复调试通道

在同一个已设置隔离数据目录的 PowerShell 中：

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
pnpm tauri dev
```

保持它在前台运行，等待 Rust、Next 和主窗口都完成启动。端口被占用时换一个未使用的本地端口。
调试结束用该终端的 `Ctrl+C` 正常停止；不要宽泛终止所有 `codeg.exe`。

另开一个 PowerShell，确认真实 WebView2 已暴露调试目标：

```powershell
Invoke-RestMethod http://127.0.0.1:9222/json/list |
  Select-Object id, title, url, webSocketDebuggerUrl
```

应选择 URL 指向当前 Desktop 页面、类型为 `page` 的目标。这里连接的是 Tauri 内部 WebView2，
不是另开的 Chrome。

### 4.6 用 CDP 检查 DOM、执行真实点击并截图

优先按可访问文本、角色或稳定属性定位元素，不要先用屏幕坐标。下面的 PowerShell + Node 示例
会连接真实 WebView2、读取页面文本、点击一个包含目标文字的按钮、收集运行时错误并截图：

```powershell
$env:CODEG_CDP_PORT = '9222'
$env:CODEG_CDP_TEXT = '会话中心'

$probe = @'
import fs from "node:fs";
import path from "node:path";

const port = process.env.CODEG_CDP_PORT ?? "9222";
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const page = targets.find((x) => x.type === "page" && /localhost:3000/.test(x.url))
  ?? targets.find((x) => x.type === "page");
if (!page) throw new Error("No WebView2 page target found");

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve, { once: true });
  ws.addEventListener("error", reject, { once: true });
});

let nextId = 0;
const pending = new Map();
const errors = [];
ws.addEventListener("message", (event) => {
  const msg = JSON.parse(event.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
  } else if (msg.method === "Runtime.exceptionThrown" || msg.method === "Log.entryAdded") {
    errors.push(msg);
  }
});
const send = (method, params = {}) => new Promise((resolve, reject) => {
  const id = ++nextId;
  pending.set(id, { resolve, reject });
  ws.send(JSON.stringify({ id, method, params }));
});

await send("Runtime.enable");
await send("Log.enable");
await send("Page.enable");
const wanted = JSON.stringify(process.env.CODEG_CDP_TEXT ?? "");
const result = await send("Runtime.evaluate", {
  expression: `(() => {
    const wanted = ${wanted};
    const nodes = [...document.querySelectorAll('button,[role="button"],[role="tab"]')];
    const target = nodes.find((node) => (node.textContent ?? '').includes(wanted));
    if (!target) return { clicked: false, text: document.body.innerText.slice(0, 4000) };
    target.click();
    return { clicked: true, label: target.textContent, url: location.href };
  })()`,
  returnByValue: true,
});
await new Promise((resolve) => setTimeout(resolve, 500));
const shot = await send("Page.captureScreenshot", { format: "png", fromSurface: true });
const output = path.join(process.env.TEMP, "sessionmux-desktop-webview.png");
fs.writeFileSync(output, Buffer.from(shot.data, "base64"));
console.log(JSON.stringify({ target: page.url, result: result.result.value, errors, output }, null, 2));
ws.close();
'@

node --input-type=module -e $probe
```

这只是最小探针。每个真实场景都应采用以下循环：

1. 读取当前 DOM/状态；
2. 找到准确的目标；
3. 触发一次交互；
4. 重新读取 DOM/状态；
5. 截图；
6. 检查 `Runtime.exceptionThrown` 和 `Log.entryAdded`；
7. 必要时完全重启 Desktop 再验证持久化结果。

当前项目 Node 运行时提供全局 `WebSocket`。若本机 Node 版本过旧，不要为了一个探针随意污染项目
依赖；可以在 Debug 窗口使用 WebView2 DevTools 手工完成同样检查。

### 4.7 什么时候才使用屏幕坐标或 Computer Use

DOM/CDP 适合：

- 点击按钮、标签和菜单；
- 检查文本、ARIA 状态、CSS 尺寸与滚动位置；
- 监听控制台错误和 Tauri 页面状态；
- 截取 WebView 内容。

只有以下原生行为才需要屏幕级操作：

- 拖动整个应用窗口；
- 最大化、最小化和系统标题栏；
- 多个物理窗口的位置关系；
- 文件选择器、权限弹窗等系统对话框；
- WebView 外部的原生边框或菜单。

即使使用屏幕操作，也应先用 CDP 确认页面处于预期状态。若 Windows 被锁定或要求 PIN，停止
验收并报告“原生 Desktop 验证被锁屏阻塞”；绝不代用户输入 PIN，也不拿 Web 模式冒充通过。

## 5. Desktop 的最小验收场景

不是每个提交都要跑完所有场景；选择与改动直接相关的部分。但涉及 Workbench、Session
缓存、布局或持久化时，至少覆盖下面的黄金路径。

### 5.1 Workbench 切换与重启恢复

准备两个命名 Workbench：

- A：打开两个真实、可恢复的持久 Session，形成 50/50 分屏；
- B：打开另一个 Session 或空布局。

验证：

1. 在 A 左侧长会话中滚动到一个可辨识文本锚点；
2. 切换 A → B → A；
3. A 的 Session、分屏方向、比例、活动 Pane 和文本锚点仍在；
4. 完全退出 Desktop，再从同一数据目录启动；
5. 回到关闭前最后活动的 Workbench，布局和阅读位置恢复。

不要只比较一个数字滚动值；虚拟列表重建后相同数字未必对应相同内容。必须同时记录可见文本锚点。

### 5.2 标签拖动与吸附分屏

验证：

1. 开始拖标签后出现浮动预览；
2. 悬停期间源标签顺序不被提前改变；
3. Pane 中央有明确的“留在当前组”预览，不出现像卡住一样的空白区；
4. 边缘吸附区足够容易命中；
5. 放下后才重算布局；
6. 左右 Pane 间可继续拖动；
7. “拖动改变布局”和“拖动调整标签顺序”互不混淆；
8. 活动 Session 在标签和内容区域有一致、可辨识的高亮。

### 5.3 Session 恢复与关闭语义

验证：

- 导入的 Session 显示真实历史而不是只显示索引；
- 标题、Harness、模型和 cwd 与持久记录一致；
- 切换离开再回来不整页重载历史；
- 关闭 Pane 只关闭视图，不删除、归档或终止 Session；
- 重新打开时使用原生 Session ID Resume，不创建一个重读全部历史的新 Session。

### 5.4 多 Harness

若改动涉及 Session 公共 UI、连接层或模型选择，至少用 Claude Code 与 Codex 各验证一个真实
Session。若改动直接涉及 Grok、Gemini/OpenCode 或自定义 ACP，再增加对应 Harness；不要用一个
Provider 的成功推断所有 Adapter 都成功。

### 5.5 Session 间通信

若改动涉及跨 Session 消息：

1. 在 Session A 选择稳定 ID 对应的 Session B；
2. 发送一条可辨识消息；
3. 检查 A 的发出状态与 B 的收到状态来自同一事件；
4. B 忙碌时消息不擅自中断当前 Turn；
5. B 收到、交给 Agent、回复分别显示不同状态；
6. 回复回到 A，并能跳转到来源 Session；
7. 完全重启 Desktop 后，消息和投递状态仍存在。

### 5.6 Collection 与 Session 树拖放

若改动涉及左侧 Collection 树，至少用真实 Desktop 验证：

1. 轻点 Session 或 Collection 仍是普通打开/展开，不会误触拖动；
2. 移动超过激活阈值后才出现浮动预览；
3. Session 可拖入同一路径下的 Collection、Unclassified 或另一个 Session 所在 Collection；
4. Session 不能跨 canonical Path，拖回当前位置不产生重复请求；
5. Collection 行顶部/中部/底部分别显示“之前/内部/之后”的稳定反馈；
6. Collection 不能移入自身、后代或另一 Path，放下失败时树结构不变；
7. 拖动时列表可自动滚动，放下或取消后高亮与浮层完全清理；
8. “新建分类”直接在触发位置创建，只有显式“移动”操作才显示目标层级选择器。

组件测试重点验证放置规则和 API 参数；指针命中、浮层、自动滚动与 Tauri WebView 兼容性必须
由真实 Desktop 场景补齐，不要用 JSDOM 原生 `dragStart/drop` 冒充指针拖放验收。

## 6. 常见失败如何判断

### `../out` 不存在

这是静态前端资源未生成。先运行 `pnpm build`，再执行默认 Tauri Rust 检查。

### sidecar 构建慢或缺失

`pnpm tauri dev` 默认会以 release 模式构建 `codeg-mcp`。第一次启动较慢是正常现象。若本次只做
不依赖 Host Control 的纯前端短循环，可以临时设置 `CODEG_SKIP_SIDECAR=1`；最终涉及 MCP、
Session 通信或 Host Control 的验收不得跳过 sidecar。

### CDP `/json/list` 没有目标

依次检查：

- 环境变量是否在启动 `pnpm tauri dev` 的同一个终端设置；
- 端口是否被占用；
- Tauri 主窗口是否已经创建；
- 查询的是不是实际设置的端口。

### 点击没有反应

先通过 CDP 读取 DOM，判断元素是否存在、是否被遮挡、是否禁用。坐标点击失败经常只是缩放、
窗口位置或遮罩变化，不等于产品逻辑失败。

### 切换场景后滚动到末尾

同时记录：Session ID、Workbench ID、Pane ID、切换前文本锚点、切换后文本锚点、重启后锚点。
只截图底部或只报告 `scrollTop` 不足以定位缓存、卸载或虚拟列表恢复问题。

### Windows 测试进程无法启动

若 `cargo test` 已完成编译，但 exe 立即以 `STATUS_ENTRYPOINT_NOT_FOUND` 退出，应记录编译成功与
运行被本机 DLL 环境阻塞这两个事实。不要删除测试，也不要把它包装成绿色结果。

### Windows 锁屏

锁屏意味着原生窗口验收阻塞。可以继续做只读代码审计或自动测试，但最终报告必须明确没有完成
真实 Desktop 交互；不能把浏览器模式结果升级成 Desktop 结论。

## 7. 提交前的证据清单

每个涉及用户可见 Desktop 行为的提交，报告至少包含：

- 分支与精确 commit；
- 本批实际修改文件；
- 使用的隔离数据目录或快照来源；
- Desktop 启动命令和对应进程；
- 真实执行的场景；
- DOM/状态断言；
- 截图绝对路径；
- WebView 控制台和 Rust 日志是否出现新错误；
- 自动测试及其结果；
- 明确的 `PASS`、`FAIL` 或 `BLOCKED`；
- 尚未验证的层级和残余风险。

推荐使用下面的交接格式：

```text
结论：PASS / FAIL / BLOCKED
分支与提交：...
Desktop 数据目录：...
场景：...
观察到的行为：...
自动测试：...
Tauri WebView：已验证 / 未验证
原生窗口：已验证 / 被锁屏等原因阻塞
截图：...
新错误：无 / ...
残余风险：...
```

提交前再次执行：

```powershell
git diff --check
git status --short
git diff -- <本批文件>
```

只显式暂存本批文件，提交后检查 commit 内容，再推送到 `sessionmux` 当前分支。

## 8. 当前还没有的基础设施

截至 2026-08-16，仓库没有一套正式的 Playwright/Tauri 端到端测试脚本。现有稳定方法是：

- Vitest/Testing Library 验证局部契约；
- Rust 测试验证数据库、命令和状态机；
- `pnpm tauri dev` 启动真实 Desktop；
- WebView2 CDP 做可重复的 DOM、IPC、控制台和截图验证；
- 必要时再做原生窗口观察。

未来可以把高频黄金路径封装为 Tauri Driver 或专用 CDP 脚本，但在该基础设施真正落库前，文档
不能声称已有自动 Desktop E2E。与其堆大量脆弱坐标脚本，不如保留少量、可审计、以真实用户
场景为中心的 Desktop 验收。

## 9. 维护依据

本手册以当前仓库配置与源码为事实源，并整理了此前真实使用过的 Codeg Desktop 调试过程。
历史过程通过本地 ctx 恢复，主要对应 Session `4b3a16bf-6d96-7bd8-b061-e402b4e3deeb` 中的
Desktop 调试分层与 WebView2 CDP 验证记录；历史记录只用于找回做法，若与当前源码冲突，以
当前 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/src/lib.rs` 和测试配置为准。

## 10. Mailbox / 多 Session 交互验收（2026-08-18）

此前只测“发得出、回得来”不够。Mailbox 改动后必须再验：通知不含正文、读信渲染成信、
往来面板是收件箱/发件箱/往来线程，以及分屏在关标签后不会留下空白栏。

### 10.1 必看的 UI 事实

- 时间线里的**系统提醒**必须有「来自邮件系统」标记，样式复用 user prompt 气泡，
  不得伪装成某个 Session 说的话。徽章只在信封 `kind=system_notify`（或无 kind 且正文
  为空）时出现；旧信封若把正文写进 prompt，前端会当成来自 Session 的信。
- Agent 调用 `read_message` 后，工具结果渲染成**信件**：主题 + 正文 + 来自 Session
  标记。不要只剩折叠的 MCP 原文，也不要在时间线再复制一张无标记的第二份正文。
- 往来是**会话内悬浮面板**，不要做成第三条分屏。触发条只占一行；点开后浮在时间线
  上，不挤掉对话。列表一行：发件人、主题 — 摘要、彩色状态。点行再展开主题+正文。
  视图至少有收件箱、发件箱、往来（按 `reply_to_event_id` 串成线程）。
- 状态颜色：未读蓝、已读未回琥珀、已回复绿、失败红、已读灰。
- 状态对人只说：未读 / 已读 / 已读未回 / 已回复。不要把「已加入 Agent 会话」当已读。
- 没有主题的旧信显示「（无主题）」，不要把整段正文当成主题标题。

### 10.2 Desktop 再测清单（debug `codeg-dev.db`，Thesis 工作台）

1. 打开「多session交流测试工作台」。若只剩 `newConversation` 而 Session C/D 在 Thesis
   目录里，把它们拖回工作台；空分栏应自动收掉。
2. 截图：往来面板收件箱、发件箱、往来三个视图。确认列表是主题行、阅读窗才是全文。
3. C 向 D `send_message`（必须有 `title` + `content`）。D 时间线出现系统提醒徽章，
   提醒里只有标题，没有全文。若当前 debug exe 早于 subject 迁移，发出的信会没有主题
   ——这是后端进程旧了，不是前端坏了；要验主题必须先停掉占用锁的 `codeg.exe` 再重编。
4. D `read_message`。时间线出现信件卡（主题+正文+来自 Session C），原卡变为 Agent 已读。
5. D 用 `reply_to_event_id` 回复。C 收件箱/往来线程看到同一串。
6. 忙碌且不能 steer 的 Session：不得被自动 cancel。
7. 关掉一个分屏里的标签后，空白栏消失。
8. 展开往来面板后再看长会话/多轮：历史 `send_message` 卡、PING/PONG 链和滚动位置还在。
   验完把面板收起来，否则阅读窗会把时间线压到只剩一小条。

每次验收把截图放到 `.artifacts/desktop-validation/mailbox-retest/`，并在
`result.md` 写：场景、截图路径、库里的 event/delivery/receipt、是否回归。

### 10.3 测试后的感悟

- Composer 的 DOM 插入不会点亮发送按钮，验收发信应走 Tauri `acp_prompt`、
  `collaboration_send` 或真人点击。
- `/model haiku` 不能和正文写在同一行，会被当成模型名。
- 工作台标签和往来快照不是同一件事：Session 改名后，往来必须显示当前名。
- 「测过一次」不能证明现在还能工作。Mailbox UI、Dispatcher 和分屏任何一批改动之后，
  都要重跑 10.2，不能只引用旧截图。
- 分屏时 DOM 里会挂着未选中标签的往来面板（`visibility: hidden`）。按坐标点「往来信件」
  可能点到隐藏那一层；只点 `visibility === "visible"` 的 banner，或先读
  `getBoundingClientRect` 再点。
- 正在跑的 debug `codeg.exe` 会锁住 `tauri-build`。只改前端时靠 Next HMR；要验
  `subject` / `system_notify` 信封必须先停进程再重编，不能把旧 exe 的空主题当成产品回归。
- `invoke_when_idle` 仍会在目标空闲时注入通知。2026-08-18 的 UI 再测里，C=290 发给
  D=291 后 D 自行 `read_message`（`agent_receipt_ref=inbox_read`）并在时间线渲染成
  prompt 样式信件卡；旧的 MAILBOX-PING → PONG → ACK 多轮链还在。
- 「已读未回」要单独测：`expects_reply=true` + `store_only`，再让目标只
  `read_message`、禁止 `send_message`。2026-08-18 用 event `ef6bccdd-…` 得到
  `obligation=awaiting_reply`、`replyReceived=false`、触发条「1 封已读未回」、
  列表琥珀标签。当时过了 10 分钟也没有系统催办：扫描曾只认
  `invoke_when_idle`，把已读的 `store_only` 信排除了；连接停在 `connecting` 还会被
  当成未接通而不投递。这两处已修。不要用立刻回完的 PING/PONG 冒充催办已测。
- 没有单独的 `session_dispatcher.rs`。队列、空闲投递和催办都走 PromptQueue。
  `store_only` 仍不单独起 Turn。`invoke_when_idle` 在目标没有工作台标签时会打开
  该 Session（走现有连接生命周期，像人点开再发）。完全没有连接时 PromptQueue
  仍不会自己 spawn ACP；要看见催办/投递，目标标签得被打开或已经连上。
