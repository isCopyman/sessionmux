# 教训库（跨会话必读）

> 每条教训 = 一次真实翻车换来的规则。格式：场景 → 规则 → 怎么执行。
> 新教训往下追加并编号；被推翻的划掉不删（保留证据链）。
> 本文件由仓库 CLAUDE.md 指路，所有 harness（Claude/grok/Codex）开工前应读。

## L1. 管道会吃退出码

场景：`cargo test | tail; echo EXIT:$?` 打出 `EXIT:0`，其实 `$?` 是 tail 的。已两次被骗。
规则：**判定成败只认命令自己的输出文本**（`test result:`、`Test Files N passed`），
或把每步 `>> log; echo STAGE-EXIT:$? >> log` 分开写。

## L2. 工人会虚构核验

场景：K3/子代理报告"我验证了"，实际没跑命令；开场 gitStatus 快照过期是根因之一。
规则：任务书强制"每句『我验证了』必须对应真实命令输出"；承重结论主会话必须抽验
（亲自跑一遍关键测试/读关键 diff）。"0 passed; N filtered out" 不算跑过测试（见 O51 验收）。

## L3. 改显示语义前，先读退役/排序机制的设计注释

场景：会话中心排序差点改成"排序键≠打印值"（破坏"一条递减的钟"）；background_watch
尾巴补发差点把 rewind 掉的内容钉死在新会话视图（overlay 按字节水位退役、不认会话）。
两次都是读了机制注释后自行撤销。
规则：显示层改动必须先找到并读懂对应的失效/退役/排序机制，确认新行为在该机制下成立。

## L4. Windows worktree 删除三板斧

场景：`git worktree remove` 报 "Directory not empty"，元数据删了目录还在。
规则：`git worktree remove --force` → `rm -rf` → `ls` 验证；句柄锁死的空壳重启自清。
worktree 一合并立即删（磁盘 20G/棵），详见 DISK-WORKTREE-HYGIENE。

## L5. worktree 里桌面测试 exe 载入即死（0xc0000139）

场景：worktree 内 `cargo test --features test-utils` 编译成功但 exe 载入即死
（comctl32 v6 环境怪癖；主仓已由 build.rs 修复，worktree 疑因 out/ 占位路径差异复发）。
规则：worktree 里遇到就 `--no-run` 证明编译 + 用 `--no-default-features --features
test-utils --lib` 跑 server 模式测试（room-read 工人发明，实测有效）；权威测试在主仓
门禁跑（CARGO_TARGET_DIR=target-gate）。

## L6. grok 桥 runs 注册表按 cwd 分项目

场景：换目录查 run 报 "No run found"。
规则：查进度/看报告必须回到派工时的同一 worktree 目录跑 `runs <id>` / `show <id>`。

## L7. 批量文本编辑必须带断言

场景：python 批量改文档，目标串不存在时静默不改，改动"看起来成功了"。
规则：每处替换前 `assert old in s`；改完 grep 验证至少一处新串存在。

## L8. i18n 十语是硬门槛

场景：新 UI 文案只加 en/zh 会在 eslint/运行时炸或静默英文兜底。
规则：新键必须同批补齐 10 个语言文件（en/zh-CN/zh-TW/ja/ko/es/de/fr/pt/ar），
语气跟随各文件既有翻译。

## L9. "规格表副本 + 一致性测试"模式值得推广

场景：board-columns.ts 把状态→列映射写成常量表，并用测试强制它与实现函数一致——
加状态漏归列=测试红而不是界面上静默消失。serde 24 条钉死测试同理。
规则：凡"约定靠人记住"的地方，考虑把约定写成数据 + 一条一致性测试。

## L10. 队列在进程死后会突袭复活

场景：O47 手工暂停 6 条后又进 2 条 queued，指向已取消会话，重启即自动重放。
规则：处理队列事故时按 source+时间窗全量清点（不要只清点已知的那批）；
设计层修复（重启不自动重放滞留项）见 O47 建议 C，待拍板。

## L11. 派工任务书的固定骨架

场景：每次手写铁律段容易漂移。
规则：任务书必含——工作区铁律（worktree 路径/不改主仓/不 push/显式 git add/报告
落 worktree 根）、已核实事实（file:line）、要做的事、明确不做、验证命令（带 L1 的
exit-code 纪律）、纪律段（不发明规格/失败两次停手/核验对应真实输出）。样例见
近期 *-REPORT 对应的任务书。
