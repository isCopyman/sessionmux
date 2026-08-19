# UI 渲染性能对比诊断：codeg vs Monet — 2026-08-19

背景：用户体感 Monet"非常丝滑"、codeg 不够。w-perf-recon（只读静态分析）产出，
编排抽验三个支柱证据（shiki 缓存 key、warm-cache 容量、monet live 降级注释）均属实。
**未做真机 profile**——各项"幅度"待 CDP Performance 采样定罪（场景清单见文末）。

## 必须先确认的前提

Monet 只发 macOS 包（README 明说 Windows 仅覆盖核心功能，系统级集成为 macOS 专属）。
若"丝滑"体感来自 macOS（WKWebView+Metal），codeg 在 Windows（WebView2+DWM）上比，
有一部分是 webview 引擎/合成器平台税，应用代码怎么改都追不平。
**待用户回答：Monet 的丝滑是在哪个系统上体验的？**

## 瓶颈候选（按收益÷难度排序）

### 1. 流式代码块 shiki 全量重分词，无 streaming 降级 ⭐ 机制已实锤
- 每次流式 delta 到达，把**当前累积的整个代码串**重新丢给 shiki 完整分词：
  `code-block.tsx:396-399` syncTokenized 依赖 `[code,...]`；缓存 key
  （`code-block.tsx:134` getTokensCacheKey）**包含 code.length**，流式追加每步
  必 cache miss，每步新起一次异步 `codeToTokens` 全量分词，且不取消上一次在飞
  请求、无"仍在流式就跳过高亮"开关。
- Monet 对照：`useStreaming.ts:56-58` 的 `live` 标志注释原文——"靠它走 plain
  降级渲染管线（否则每帧全文 shiki 重解析）"，流式期间纯文本，turn 结束才高亮一次。
- 收益高（直击"流式输出时卡"，长代码/大 diff 场景）；**小改**：加"流式中"prop
  强制走现成的 rawTokens 兜底，流结束触发一次真高亮 + AbortController 取消过期
  请求。幅度待 profile。

### 2. 会话 warm-cache 容量硬顶 8，超额 tab 切换回冷路径
- `session-warm-cache.ts:21` `capacity = 8`：只有最近 8 个会话的解析结果/虚拟化
  几何/空闲 ACP 连接保持热态；第 9 个 tab 起，切回久未碰的 tab = 重解析全史+重建
  虚拟列表，体感"有的 tab 秒开有的卡一下"。
- 收益中（重度多会话用户正是主诉人群）；小改（调大或做成设置项），但要连着看
  内存与常驻连接数代价。

### 3. 框架地板差（背景色调，非 bug）
Vue3 细粒度响应式改字段只更新对应 DOM 文本节点；React 即便 selector 精确，命中
组件仍整体重跑渲染函数+diff。codeg 的 zustand 用法没有误用（见排除清单），说明
差距更多来自框架天花板。不可修（换框架不现实），可用 #4 部分对冲。

### 4. React Compiler / useDeferredValue 未用于热路径（免费杠杆）
- babel-plugin-react-compiler 未安装使用；useDeferredValue/startTransition 全仓
  只出现在 3 个 aux-panel 文件（文件树/git 面板），消息流与 tab 切换两个主诉热路径
  零使用。收益中（compiler 需实测；useDeferredValue 对"流式期间打字卡"收益明确）；
  小-中改。

### 5. useTranslations 叶子密度（低优先级）
content-parts-renderer.tsx 12 处等，千刀万剐式背景噪音。猜测级,最后再看。

## 已排查证伪（别浪费 profile 额度）

zustand 订阅粒度（逐字段 selector+useShallow+recomputeTabs 引用复用+拖拽短路）、
消息虚拟化（virtua+完整滚动恢复）、流式合批（16ms 定时+同类 delta 合并,与 monet
RAF 同量级）、按会话时间线记忆化、Reorder 动画（TabItem 未接 FLIP）、
backdrop-blur 密度（13 处均不在滚动区）——**这几处代码质量讲究,不是嫌疑点**。

## 下一轮 CDP Performance 采样场景

1. 长代码流式（验证#1）：agent 写 200+ 行文件，看 codeToTokens 调用栈是否随
   delta 对齐出现、单次耗时是否随长度增长；
2. warm-cache 冷热对比（验证#2）：开 10+ tab，对比热/冷切换 Scripting+Layout 台阶；
3. 长会话快速滚动：耗时落 Scripting/Layout/Composite 哪层；
4. 纯文本流式基线：与场景 1 对比，坐实#1 是代码块特有；
5. 同机对照（能做则做）：同一台 Windows 机跑 Monet Windows 构建对比帧时间，
   拆开平台税/框架税/代码问题三者。

## 关键文件
codeg：`src/components/ai-elements/code-block.tsx`、`src/lib/session-warm-cache.ts`、
`src/contexts/acp-connections-context.tsx`、`src/components/message/virtualized-message-thread.tsx`
monet：`src/composables/useStreaming.ts`、`README.md`
