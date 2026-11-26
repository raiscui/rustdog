# WORKLOG: rustdog

## [2026-11-26] 测试夹具的 daemon 在 panic 路径被收掉 (RAI-423)

- 现象: 夹具只在正常路径 `stop_child`; 断言 panic 后子进程被 init 收养, 一轮批次留下 27 个孤儿
  `rdog daemon`, 放大后续用例的假红(一次 panic 污染整批)。
- 改法: 新增 `tests/support/child_guard.rs`: `ChildGuard` 持有 Child、`Drop` 里 kill+wait,
  `take()` 供 `wait_with_output` 用, `ChildHandle` trait 让 `stop_child(&mut impl ChildHandle)`
  同时接受裸 `Child` 与 `ChildGuard`(调用点语义不变)。
- 迁移: control_lanes / control_mode / control_pty / control_websocket / daemon_inbound /
  zenoh_router_client / connect_modes; zenoh_unixpipe 自带 Drop 的 TestDaemon 未动。
- 回执: panic 探针 → guard 那个被收、裸 Child 对照残留 1 个; 连跑三轮批次 leftover 恒为 0;
  各 target 计数与改动前一致。
- 提交: 8792422

## [2026-11-26] 控制行分片到达时被读超时丢弃, PTY 的 ^C 因此失效 (RAI-415)

- 根因: `src/control_line_read.rs::read_line_limited` 入口 `buf.clear()`; `read_until` 在
  「正文已到、换行未到」时被读超时打断返回 WouldBlock, 下一轮调用把半行清掉 → 整条消息静默丢失。
- 触发面: `transport.write_message` 是 `write_all(message)` + `write_all("\n")` 两次写, 25ms 读超时的
  PTY 桥接最容易在两次读之间撞超时。表现: `@pty-stdin`(含 ^C) 帧被吞, 远端收不到 SIGINT。
- 定位链: 远端 pty termios 正常 → helper 即前台 pgrp → python `pty.fork` 纯 kernel 对照正常 →
  直连 daemon TCP 发同一帧正常 → strace 双侧发现 daemon 收到帧却只写出过 `\4` 没写出 `\3`。
- 改法: 入口不清 buf, `total` 从 `buf.len()` 起算, 只在整行交出/超限丢弃/干净 EOF 时清空。
- 回执: `control_pty` 9 passed(断言零改动) / 全量单测 1108 passed / control_tty·shell_pty·control_lanes·
  control_websocket·connect_modes 全绿; 分片复现命令修复后 3/3 成功。
- 提交: 7067f40

## [2026-11-26] tests/ TTY 夹具改按能力探测选择 script 调用形式 (RAI-410)

- 4 处夹具写 `script -q /dev/null <rdog> control ...`(BSD/macOS 尾随命令形式), util-linux 2.39.3 只认
  `script -q -c "<CMD>" /dev/null`, 直接报 `unexpected number of arguments` 退出 -> 子进程 stdin BrokenPipe,
  这 4 个 TTY 用例在 Linux 上从未跑起来。
- 新增 `tests/support/tty_script.rs`: 探一次 `script -c` 能力并缓存, util-linux 走 `-c`(命令行 POSIX 单引号引用),
  BSD/macOS 走尾随命令; 探测失败不静默跳过。
- 顺带修 `control_pty_should_treat_control_words_as_remote_input`: 直接给 `rdog control --pty` 喂管道 stdin
  (`--pty` 需要真终端) 会永久挂到超时(实测 rc=124), 包一层伪终端后拿到 `GOT:@script:...`。
- 回执: 本机(ubuntu 24.04 / util-linux 2.39.3 / cargo 1.100) `control_pty` 8 passed/1 failed、
  `control_tty` 2 passed、`zenoh_router_client` 38 passed/0 failed/2 ignored。
- 剩余 1 个失败 (`control_pty_ctrl_c_...`) 与夹具无关: python 真 pty(不经 script)复现同一行为; 远端 pty 实测
  `isig icanon`、helper 即该 pty 的 foreground pgrp、0x03 也确实作为字节送到远端(raw 读端收到 `61 03 62`),
  只是不产生 SIGINT — 属 pty lane 产品侧缺口, 本票范围外。
- 评审修复: `control_pty_should_treat_control_words_as_remote_input` 原先写完就关 stdin,
  而 BSD/macOS 的 script 见到 stdin EOF 可能提前收掉伪终端 (control_tty 里有同类记录); 改成等远端打印
  `GOT:` 后再关写端, stdout/stderr 走收集器。
  收集线程在子进程退出后 join 再快照, 否则最后一段输出可能还没落 buffer 就断言 (评审第二轮)。
- 提交: 370317f (评审修复 6c187d7 / ee8474c)

## [2026-11-26] tests/ 下 17 处 rdog 二进制路径反推收成一处 (RAI-405)

- `tests/` 下 17 个集成测试各自复制「测试可执行文件上两级 + `/rdog`」; cargo 1.100 把测试可执行文件放到
  `target/{profile}/build/rustdog/<hash>/out/`, 这些夹具解析到不存在的路径, 一旦被跑起来就在夹具阶段 0.00s 失败。
- 新增 `tests/support/rdog_binary.rs` 作为唯一实现 (候选顺序同 RAI-396 验证过的那份: `CARGO_BIN_EXE_rdog` →
  `{profile}/rdog` → `deps/rdog` → `build/rustdog/<hash>/out/rdog` 新→旧 → 测试可执行文件目录及上级);
  17 个调用点只剩一行 include + 一行 `use` (3 个带 env override 的保留一行委托)。
- 集成测试是独立 crate, 链接不到 bin crate 的 `src/test_support.rs`, 所以公共实现只能住 `tests/` 下。
- 回执: 迁移后 9 个此前不在 CI 的用例集实测 (connect_modes 2/2、control_lanes 15/15+1 ignored、daemon_inbound、
  listen_suspend、shell_pty、control_mode、control_websocket、recording_e2e 6/6 全绿); 6 个失败全部是
  `Command::new("script")` 与本机 util-linux 2.39 的调用形式不兼容, base 对照同样失败; 缺二进制时 panic 列出全部候选;
  `cargo test --bin rdog -- --test-threads=4` 连绿 8 轮 (1107 passed × 8)。
- 提交: cbe31b5

## [2026-11-06] Persistent coding sessions

- Added authenticated coding sessions, their RPC dependency, and protocol documentation.
- 提交: 295b942

> 本文件由前档 `archive/default_history/WORKLOG_2026-09-02_ocr_content_layer_rollover.md` (1125 行) 续档而来。前档含 Wayfinder #95 全程、
> OCR 引擎横评、live e2e 调试链、#102/#103 票据记录。续档原因: 超 1000 行。
> 交接文档: /tmp/rdog-ocr-handoff-2026-09-02.md

## [2026-09-02 23:00:00] [Session ID: wayfinder-ocr-chart] 任务名称: continuous-learning 完整复盘 (会话收尾)

### 任务内容
- 完整 continuous-learning: 六文件回读、Gate/Capture、Scoped Refresh、AGENTS 索引、六文件续档

### 完成过程
- Capture 两份新 solution (frontmatter+claims 校验全过):
  - docs/solutions/best-practices/macos-live-gui-testing-environment-gates.md (休眠/归属/焦点/级联四门禁)
  - docs/solutions/design-patterns/appkit-helper-subprocess-isolation.md (AppKit 非主线程 abort -> 子进程隔离 + 手动 runloop)
- Scoped Refresh: macos-tcc-stable-codesign-identity.md 补 2026-09-02 两个新维度 (重编覆盖重签 / responsible process 归属链)
- EXPERIENCE inbox 增补: OCR 引擎横评方法论 (单日证据, 升级条件注明)
- AGENTS.md 索引两条新 solution; 六文件续档 (task_plan 1684 / WORKLOG 1125 -> 日期后缀前档 + 新档)

### 总结感悟
- 候选取舍: "坐标偏移 45px 假说被推翻"过程不单独成文 (已融入环境门禁 solution 的 Why This Matters);
  TCC 新维度走 Scoped Refresh 而非新 solution (同主题增量)
- 校验脚本是硬门禁: architecture_patterns vs architecture_pattern 一字之差即 FAIL, 枚举必须查表

## [2026-09-02 18:05:00] [Session ID: sess_68ea20f1-1318-4541-85bc-08d0ca1ddbd8] 任务名称: OCR 交接续档 - PR #104 提交 + overlay 渲染根因修复 + #103 重验尝试

### 任务内容
- 从 /tmp/rdog-ocr-handoff-2026-09-02.md 续档; 完成 git 提交 (交接待办第 1 项) + #102 overlay 渲染根因定位与修复 + #103 亮屏重验尝试 (现场污染中止)
- 覆盖: src/control_overlay.rs / Cargo.toml / 多个 issue comment; PR #104 (feat/ocr-content-layer-phase1, 5 commits)

### 完成过程
- 提交前 948 单测验证 (3 个 process_lease/control_actions 抽签族失败单跑全过, 与 OCR 改动无关); 按明确清单三 commit (实现/e2e/文档), 禁 add -A; PR #104 CI 触发正常
- overlay onscreen=false 根因链 (现象->假设->证伪): swift 同构对照排除绑定用法 -> 完整 app.run 对照**推翻生命周期主假设** -> 截图硬判据确认真实不可见 -> 7 变体属性矩阵锁定 **NSPanel+Borderless 组合被 macOS 拒绝合成** (NSWindow+Borderless / NSPanel+Titled 均正常); 修复改 NSWindow, 真机验证 onscreen=1 + 像素绿特征 (G-R +19 vs 对照 +3)
- 修复引出 CI Linux 构建红: control_overlay 的 objc2 引用无 cfg 门控 -> 全部 #[cfg(target_os)] 门控 + submit 编译期短路, push 后 CI 重跑
- #103 重验: 亮屏确认后用 AXValue 直写文本 + daemon include_ocr + screencapture 真值做交替对照; 发现实验窗在第二屏 (主屏真值无效需 -D 2), 且用户实时操作致窗口层级持续变化, 帧对照被污染 -> 全部判定作废, 如实回写票并留方法学; 顺带确认当前 window 路径 source=display-composite-crop (走 composite 守卫), 票描述的"独立 window 后端"可能已不适用
- 环境治理: 清理 60+ 历次测试遗留 debug daemon + 僵尸 overlay 子进程; 清理本轮产生的隐私截图 (/tmp/ocr99 + rdog_downloads); 旧调试 png 保留待用户处置

### 总结感悟
- 生命周期假设被 swift 完整 app.run 对照推翻是本轮关键转折: "同款代码 swift 也不可见"把搜索空间从绑定层拉回窗口属性层, 属性矩阵一次定位
- 判定"不可见/陈旧"必须三层证据齐备: CGWindowList 状态 + 屏幕像素 + 有效真值; 本轮 t2"陈旧"判定就栽在真值抓错屏上
- 用户在用的机器上做 window-level live 实验会持续被污染, 必须受控时段; 发现截到用户私人内容要立即停手并清理产物

## [2026-09-02 19:25:00] [Session ID: sess_68ea20f1-1318-4541-85bc-08d0ca1ddbd8] 任务名称: #103 受控重验 (关票) + #102 e2e 稳定化四缺陷修复

### 任务内容
- #103: 亮屏受控重验 window 截图新鲜性 -> 通过, 关票
- #102: live e2e 连跑调试, 定位并修复四个确定性缺陷 (commit 06ad1d2)

### 完成过程
- #103 静态证据先行 (screenshot.rs:109 守卫已挂 window 路径), 动态闭环: AXValue 直写交替改内容 + daemon include_ocr 判读 + 所在屏 screencapture -D 真值; daemon 帧与真值窗口区像素 0% diff = 帧新鲜实锤。全屏采样 diff 0% 的教训: 一行小字占比太低会稀释信号, 窗口区直接 diff 才是判据
- #102 逐层剥离: 显示区定位误用 locate_button (排除 top 行致 marker 永远 miss, 此前"偶尔过"是噪声框巧合) -> 鼠标悬停 tooltip 劫持 AX window:0 (215x18 rect 实证) -> Calculator 表达式跨运行持久化 (savedState/defaults 均清不掉) -> 每轮 OCR 全文诊断日志
- 迭代纪律: 每次失败先看新证据再改, eprintln 诊断日志 (点击后 OCR 全文) 是定位提速的关键投资
- 止损判断: 单帧按钮误读 + 超长残留是 e2e 深水区, 干净环境验收留待重启后, 不在脏环境无限迭代

### 总结感悟
- "测试偶尔过"的三种成因要区分: 抽签 (时序), 环境假设缺陷 (PID 复用), 逻辑巧合 (噪声框改变 top 计算) —— 各需不同修法
- AX 会把 tooltip 注册成窗口并排到 window:0: 任何按窗口枚举序定位的实现都可能被悬停 tooltip 劫持, 这是通用产品隐患
- live e2e 的"干净起点"假设必须显式保障, 依赖被测 app 自身状态重置 (quit/reopen) 在 macOS 持久化时代不可靠

## [2026-09-02 21:15:00] [Session ID: sess_68ea20f1-1318-4541-85bc-08d0ca1ddbd8] 任务名称: #102 终极根因 (click 收敛) 修复与 e2e 三连绿验收

### 任务内容
- 定位并修复 @click 跨屏落错位置的终极根因; live 三件套三连绿验收通过; #102/#103 双票关闭

### 完成过程
- 排除链: overlay 拦截 (RDOG_OVERLAY=0 对照) -> 鼠标 warp 偏移 (三点标定全精确, 期间发现 65011 daemon 已死导致一轮假数据, 推翻重来) -> 纯 click 无 move 必中 vs warp 后立即 click 必落旧位置 -> 收敛时序根因实锤
- 产品修复 Move 步后 40ms; e2e 配套 Esc 归零 (新版计算器无 AC 按钮的截图实证)/稳定候选对/几何校验/诊断日志
- 一次 git 事故与自救: commit 混入 index 里并行会话 staged 的六文件 (9 files/-2816 行), 分支未进 main, soft reset + unstage + force push 重写为干净 2 文件 commit

### 总结感悟
- "click 落旧位置"这类时序缺陷的手动复现会因往返延迟掩盖问题, 只有自动化连发才暴露——这是 e2e 不可替代的价值
- 对照实验设计: 改单一变量 (去掉 move / 拉长间隔) 比堆日志更快逼近根因
- git 纪律再+1: commit 前 git status --short 必须核对文件清单与预期一致 (这次 9 files 异常就是最后一道防线该拦的)

## [2026-10-06 12:36:00] [Session ID: rustdog-win-verify-2026-10-06] 任务名称: rdog Windows 11 真机验证 (4K/200% 坐标批次)

### 任务内容
在 rais-razer.local (Win11 x64, 4K 物理 3840x2160 / OS 逻辑 1920x1080, 200% 缩放) 上
部署新交叉编译的 rdog.exe, 逐项验证 a~f。只观测, 未改任何 rdog 实现代码。

### 完成过程
1. 部署: schtasks End+Delete RdogDaemon2 → Stop-Process rdog → 5555 释放 →
   scp 新二进制 (SHA256 A2CDE103... 两端一致) → 设 RDOG_LOG_FILE / RDOG_SHELL_LANE_TIMEOUT_MS
   → 起 daemon → 验 listen。
2. a~f 六项全部取得真实输出 (见下方结论)。
3. e 项用 PIL 在 3840x2160 图上按颜色定位 Win11 开始按钮图上中心 (1210, 2112),
   按 manifest 公式 image_to_os 手算得 os-logical (605, 1056), 点击后开始菜单打开,
   按钮进入高亮态, 37.22% 像素变化; 再点一次关闭, 终态与点击前 0.00% 差异。

### 总结感悟
- Windows 上 `rdog daemon` 的隐藏常驻 child **不能**用 `Start-Process` 起:
  进程留在 ssh 会话的 job 里, 会话一收就死 (实测三次: 38872 存活于 kill 前,
  16632 与 32508 都在 ssh 退出后 DEAD)。必须用计划任务起才活得下来 (13888 跨会话稳定)。
- User 作用域的 `[Environment]::SetEnvironmentVariable` 只写注册表, 已运行的
  PowerShell 拿不到 —— 启动 daemon 的那个 shell 必须自己再 `$env:` 赋一遍。
- 通过 ssh 传带引号/空格的控制指令, cmd.exe + PowerShell 5.1 会层层加引号改写
  (实测 `-Arg '@ping'` 变成字面量 `'@ping'`, 报"位置参数最多 2 个")。可靠做法 =
  指令写 UTF-8 无 BOM 文件 + `cmd /c "... rdog.exe control < < file"` 重定向, 字节直通。

### 结论 (a~f)
a 通过 / b 通过 / c 通过 / d 通过 / e 通过 / f 通过 —— 六项全绿。

## [2026-10-22 14:30:00] [Session ID: mvs_04f490f87a924c96ac1cb0adcbd30806] 任务名称: continuous-learning 完整复盘 (补记 10-07~10-20 缺口 + 载体漂移修复)

### 任务内容
- 用户显式调用 `$continuous-learning`, 跑完整流程: 六文件回读 → Gate/Capture →
  Scoped Refresh → 载体同步 → 双校验 → 归档 → 六文件更新 → issue 检索与报告。
- 起因: 六文件最新条目停在 2026-10-06, 但 git 已到 10-08, 工作区还留着一份
  10-11 的未提交 SKILL.md 改动 —— 说明 10-07 之后的产出**没有进账本**。

### 完成过程
1. **回读与分组**: 六文件 (task_plan 113 / notes 775 / WORKLOG 101 / LATER_PLANS 915 /
   ERRORFIX 321 / EPIPHANY_LOG 103 / EXPERIENCE 575 行) + 23 份 solution + 22 个 open
   issue + 4 个 worktree。确认本工作区无并行会话占用 (其余会话都在 pireel /
   zcode-collab / xtalk), 那份未提交 SKILL.md 是 10-11 遗留, 无人认领。
2. **发现载体漂移 (本轮最有价值的产出)**:
   - `remote-control-blocked-command-pitfalls.md` 的落地表停在 10-06, 但 10-07
     (536afd0, UTF-8) 与 10-08 (f7b94bd, `@run`) 已把 P3-2 / P4 做掉 —— 表里
     仍写着"未落地 / 全仓无该 verb"。
   - 该表 P1-1 行记"文档/skill 里的换算口诀 —— **未核实**"。本轮核实发现
     同一份单段式公式 `os_x = image_x + virtual_bounds.x` **在 5 处载体里都是错的**
     (skill `references/protocol.md` ×2、`README.md`、`specs/code-agent-rdog-control-usage.md`、
     `specs/rdog-multi-display-screenshot-coordinate-plan.md`、
     `specs/rdog-mouse-control-coordinate-plan.md`): Windows 4K@200% 时
     `virtual_bounds` 是物理像素 (与 os-logical 差 2 倍), 窗口裁剪图还少一项
     `image_rect` 平移。权威公式在 `src/control_mouse/screenshot_frame.rs:17`。
     5 处已全部改为"以 manifest 的 `transforms` 字段为准 + 权威公式"。
   - 这正是 2026-10-05 那场实战"连续三次点空、浪费约 20 分钟"的同一个坑 ——
     换了载体 (代码里有公式) 但**错的那份留在了 agent 实际会读的地方**;
     且原始设计面向 macOS (scale=1) 时旧式成立, 所以 macOS-only 的验证看不见它。
3. **Capture**: 新建 `docs/solutions/best-practices/electron-main-process-forensics.md`
   (渲染层 CDP 看不见主进程时的 `--inspect` 取证面 + 双通道定位法),
   七项门禁逐条过。
4. **Scoped Refresh**: 重写落地表 6 行 + 补 "2026-10-11 增量" 一节 (带空格路径 /
   主进程取证 / 双通道 / 附带修掉的公式缺陷)。**并撤回一处未证实的断言**:
   原 skill 写"rdog 链路会剥单引号", 但全仓 `trim_matches` 只有三处且都作用于
   双引号, one-shot 行直接来自 argv —— **rdog 源码里找不到剥单引号的代码路径**,
   该机制层级尚未定位, 改写为"现象已观察 + 机制未定位 + 解法不依赖定位"。
5. **知识卫生**: 24 份 solution 双校验全绿 (修前 2 份 frontmatter 失败:
   `problem_type: feedback` 不在枚举、`logic_error` 缺 `root_cause`/`resolution_type`;
   另裁决 cross-compile 文档 5 条 claims flag)。AGENTS.md 补 2 条遗漏索引
   (remote-control / cross-compile-windows)。
6. **归档**: `task_plan_2026-09-02.md` + `WORKLOG_2026-09-02.md` →
   `archive/default_history/` (git mv), manifest 见
   `archive/manifests/ARCHIVE_MANIFEST__2026-10-22_default_history_rollover.md`;
   核对未勾选项转出, **无缺口**。

### 总结感悟
- **"未核实"是知识库里最贵的三个字**: 落地表里 P1-1 那行挂了 16 天没人核, 核出来
  是错的 —— 而且不是一处, 是 5 处照抄传播。标着待核实的项需要一个明确的到期动作,
  否则它既不提醒也不负责。
- **错的那份往往不在代码里, 在 agent 真正会读的那个文件里**: 代码有权威公式,
  但 skill 给的是另一套 —— 载体之间没有对账机制时, 错的那个反而更容易被信。
- **载体该写"指向真相源的指针", 不该手抄公式**: 指针不会过期, 手抄的会。
  5 处已改成"读 manifest 的 `transforms` 字段"为主、公式为辅。
- **旧式在原始目标平台上成立, 才让它活这么久**: macOS scale=1 时单段式逐字正确,
  macOS-only 的验证永远看不见 HiDPI 下的 2 倍偏差 —— 与本仓
  `env-deterministic-ci-red-vs-local-green` 那族同源。
- **现场断言要先落到代码路径再落纸面**: "rdog 剥单引号"听着像产品缺陷, 但静态
  找不到对应代码; 老实写成"机制未定位", 并给不依赖定位的解法 (`@run` 结构化
  args / mklink), 比编一个机制有用得多。
- 六文件对 10-07~10-20 零记录 = 这段时间的产出只能靠 git commit 反推。**账本
  滞后比知识缺失更隐蔽**: 知识还在 git 里, 但没人知道该在哪读。

### 补记: 10-05 ~ 10-22 提交账目 (sha 互引)

2026-10-22 建「提交即记账」守卫时 (`scripts/check-bookkeeping.py`) 扫出的实际欠账:
10-05 以来 21 个 commit 里有 **14 个从未进过账本**。下表一次性补记, 作为该时段
的索引入口 —— 之后每个工作 commit 由 `WORKLOG` 条目末尾的 `- 提交: <sha>` 互引。

| 提交 | 时间 | 内容 | 知识落点 |
| --- | --- | --- | --- |
| `f6d2c92` | 10-06 00:26 | 实战反馈补 SSH 多行 PowerShell 静默失败坑 | solution `remote-control-blocked-command-pitfalls.md` P0 附带坑 |
| `e909147` | 10-06 01:09 | line-based 协议加帧长度上限, 堵无界读 | `specs/control-line-protocol.md` |
| `2a541b9` | 10-06 02:35 | CDP 远程通道最小闭环 (`@cdp-targets` / `@cdp-eval` / `@cdp`) | `specs/cdp-remote-lane-spec.md` + skill CDP Lane 节 |
| `2a1740e` | 10-06 03:17 | 双向分块文件传输 (续传 / sha256 / 路径穿越拒绝) | `specs/file-transfer-control-spec.md` |
| `73a1034` | 10-06 13:09 | 点击后端标签如实上报 + Windows 测试目标平台门 + 跨平台配方 | solution `remote-control-blocked-command-pitfalls.md` P1-3 |
| `765e0a7` | 10-06 14:06 | OCR 坐标换算进 os-logical + 点击落点回传真值 | `specs/rdog-ocr-content-layer-plan.md` |
| `82fd029` | 10-06 15:00 | Windows 测试可移植性 47→8 失败 + daemon 失败可见性 | 本仓 Windows CI 面 (`ff171de` 补 runner) |
| `35e7c73` | 10-06 15:31 | awareness 断言改枚举判据 (伪句柄运行时值 ≠ SDK 哨兵) | `specs/windows-hidpi-coordinate-fix-plan.md` |
| `b940525` | 10-06 15:59 | zenoh namespace 配置为主 + 冲突报错; json5 路径转义修 Windows usrpwd 崩溃 | `specs/zenoh-control-plane-plan.md` |
| `ff171de` | 10-06 16:17 | Windows bundle commit ACCESS_DENIED + CI 加 windows runner (全量 1009 清零) | `.github/workflows/ci.yml` |
| `9c4feba` | 10-07 23:08 | CDP lane 真机实测形态入册 (形态 A daemon 内置 / 形态 B CDP-over-@cmd) | skill `SKILL.md` CDP Lane 节 |
| `c094a92` | 10-08 01:39 | `@run` 原语入册 + 并行会话在途文档顺带入库 | `specs/rdog-task-spawn-control-plan.md` + skill |
| `b65680f` | 10-22 13:49 | 载体漂移修正 + continuous-learning 复盘 (5 处换算公式) | 本条目 + `docs/solutions/best-practices/electron-main-process-forensics.md` (新建) |
| `bdfe746` | 10-22 13:49 | 补提交流水线归档删除侧 (上一 commit `--only` 漏了删除侧) | `archive/manifests/ARCHIVE_MANIFEST__2026-10-22_default_history_rollover.md` |

已在账本中 (无需补记): `7264f6b` (DPI awareness 地基) / `510dc2d` (manifest 补
`os_logical_bounds`) / `b2c3b33` (Windows 原生鼠标后端) / `a7ecbec` (Windows 真机
实战改进落地) / `536afd0` (真机实战三修) / `f7b94bd` (`@run` P4 原语落地) —
这 6 个的 sha 已在 `task_plan.md` / `notes.md` 的 10-06 条目里出现。

**教训**: 上面 14 个里有 6 个是**功能代码** (CDP lane / 文件传输 / 帧上限 /
DPI 修正 / zenoh namespace / recording 修复), 它们当时连一行账本都没留下 ——
不是"记漏了一句", 是**根本没意识到要记**。所以守卫是必要的, 不是形式主义。

### 追加: 三条改进建议守卫化 (同日续, 提交 a884bad)

用户让把本轮报告里的三条项目级改进建议真正做掉。三条都落成**可执行守卫**,
不只写进建议清单 —— 约定正文在 `docs/agents/knowledge-carriers.md`。

1. **载体断言清单化** → `scripts/check-carrier-assertions.py`
   权威换算式从 `src/screenshot.rs` **抽取**(不硬编码), 逐轴比对载体里给出的每个
   换算式; 断言清单与文档双向对账。**顺带抓到第 6 处载体漂移**:
   skill `references/control-workflow.md:339` 的散文写法
   ("convert ... by adding virtual_bounds.x/y") —— 上一轮按公式字面量 grep 没命中。
2. **进度表加「最后复核」列** → `remote-control-blocked-command-pitfalls.md`
   的「落地情况」表 17 行逐行补日期 (每行都对当前代码核过); 优先级建议表不再
   自带表级日期(会自己过期), 改为指向唯一真相源。
3. **提交即记账** → `scripts/check-bookkeeping.py`
   范围 = 最近一个已被账本引用的 commit 之后; 每个 commit 要么被账本引用, 要么
   只改账本文件(记账提交豁免, 不自我递归)。

**两个守卫都做了反向验证**(注入回归看它是否真会红), 并据此修掉守卫自身的漏洞:
载体守卫第一版有 3 个洞 —— 指针豁免过宽(改一处、别处提过 transforms 就能免检)、
免责词表把 `not the` 误当免责标记(一行里旧公式+正确否定陈述会整行被豁免)、
整串比对必假阳性(权威串单行 `;` 分隔, 载体分两行/只写 x 轴) → 改成
**去空白后按轴做前缀匹配**。

接入: AGENTS.md 新增 "Knowledge carriers" 小节; CI 新增 `knowledge-carriers` job
(载体断言全平台必跑, 记账检查只在 push 到 main 跑)。

- 提交: a884bad

### 追加: Windows CI 红的根因修复 + Actions 复开 (同日续)

起因: 上一次 CI (2026-10-06, commit `ff171de`) 的 **windows job 是红的**
(1007 passed / 2 failed), 而**仓库级 GitHub Actions 当时是关闭的**
(`repos/raiscui/rustdog/actions/permissions` → `enabled: false`),
所以这批红没人看见, main 上这段历史一直没有 CI 把关。推送后复核时才发现。

**两个失败的根因是同一个**: Windows 上同一个路径有两种拼法。
- `execute_control_command_should_run_script_via_shell`: 并发单测里
  `figment::Jail` 把进程级 cwd 切到带 `\\?\` verbatim 前缀的临时目录,
  cmd.exe 认不了这种 cwd → 打 "UNC paths are not supported" 告警 →
  挂在"stderr 必须为空"这条断言上 (脚本本身照常执行, stdout/exit_code 都对)。
  修法: 与 unix 侧同类放行 (`is_deleted_cwd_shell_warning`) 对称, 放行这一类
  环境噪音; 判据做成**语言无关** (中文 Windows 上是中文文案, 真机实测),
  共同特征 = 告警带 verbatim 路径 + 提到 "CMD.EXE"。
- `shell_lane_should_apply_cwd_env_and_script_text`: runner 的 TEMP 是 **8.3 短名**
  (`RUNNER~1`), 而 PowerShell 的 `$PWD` 报的是**长名** (`runneradmin`);
  同一目录两种拼法。修法: 期望值改走 `shell_visible_path()` (canonicalize
  把短名解析成长名, 再去 `\\?\` 前缀), unix 侧行为不变。

**顺带捞出两个从未执行的测试**: `src/control_protocol/tests.rs` 里两个 `@run`
解析测试被**嵌套在**另一个测试函数体内 —— 嵌套的 `#[test]` fn 不被框架收集,
断言从来没跑过 (`@run` 原语的解析契约一直是没测的)。证据 = 编译告警
`function run_payload_requires_path is never used`。提到模块级后两个都通过。
修完全仓 0 个 "never used" 告警 —— 这类缺陷就这一处。

**真机实证** (rais-razer.local, Win11): scp 一个 .ps1 过去跑, 复现了
cmd.exe 在 verbatim cwd 下的告警原文 (中文 Windows 版), 并用普通 cwd 做对照
(stderr 为空) —— 证明这条告警**只**在 verbatim cwd 下出现, 放行它不会掩盖真失败。
真机无 Rust 工具链, Windows 侧最终由 CI runner 验证。

**本地环境两个坑** (与本仓无关, 但下次会再遇到):
- `rustup` 组件下载在本机被环境权限门挡住 (`~/.rustup/downloads/` 里 rename
  返回 EPERM) → 仓库 pin 了 `channel = "stable"` 而本机没装 stable,
  `cargo` 会卡在下载阶段。绕法: `cargo +1.95.0 ...` (用已装的具体版本)。
- 本机跑全量单测有 **37 个失败**, 与本次改动**无关** (stash 对照: 改动前后
  失败集完全一致, 通过数 1026 → 1032): 32 个 recording 家族是
  `PermissionMissing("input_monitoring")` (本 agent 进程没有 macOS TCC 授权),
  4 个 unixpipe 是仓库已记录的 PID 复用抽签族。

- 提交: a040124

### 追加: Actions 复开 + CI 终态 (同日续)

- **仓库级 Actions 此前是关闭的** (`enabled: false`) → 2026-10-06 之后 main 没有
  CI 把关; 那个 commit 写"Windows 全量 1009 清零" 但**它自己的 run 就是红的**。
  已开回 (`enabled: true`)。
- 触发完整矩阵: 首轮 Windows 红 6 个 (4 个 flow shell lane 30s 超时 + shell
  receiver + control_core), **只重跑失败 job 即全绿**; 判定 runner 偶发
  (flow lane 产品代码逐行未变, 10-06 那次同一份代码能跑)。
- **终态**: macOS ✅ / Ubuntu ✅ / Windows ✅ (`1013 passed; 0 failed`) /
  `knowledge carriers` ✅ (新加的载体守卫 job 生效并通过)。
- 测试总数 1009 → 1013: +2 是从嵌套里提出来的 `@run` 测试 (原先从未执行),
  +2 是本轮新增的跨平台单测。
- 纪律 (已记 LATER_PLANS): Windows job 首轮红**先重跑再归因**; 同轮产品代码
  未变 + 重跑即绿 = runner 偶发, 连续三轮红在同一组测试上才按真缺陷查。

### 追加: 记账检查改成"CI 只报告不阻断" (同日续)

用户裁定: 账本滞后是**流程债**, 不该在 push 到 main 时硬拦别人的推送。

- `scripts/check-bookkeeping.py` 新增 `--report-only`: 永远退出码 0, 但打印完整
  报告; 在 GitHub Actions 环境额外发 `::warning::` 注解 —— 漏账条目会显示在
  **提交/PR 视图**里, 而不是只埋进日志 (不阻断但看得见)。
- 默认仍是 **strict** (漏账退出码 1), 留给本地 pre-flight 用。
- CI 的 `knowledge-carriers` job 改用 `--report-only`; **载体断言检查保持硬门禁**
  (载体与代码分叉是真缺陷, 不是流程债)。
- 反向验证: 剥掉账本里所有真实 sha 制造 9 个漏记账 commit →
  strict `exit=1` (报 9 条) / report-only `exit=0` (同样报 9 条 + 9 条注解)。
- 约定正文 (`docs/agents/knowledge-carriers.md`) 与 AGENTS.md 同步写清两种模式的分工。
- 提交: a39be21

## 2026-10-24 yino反馈优化及正式daemon更新

队列诊断、Windows默认隐窗与后台任务树取消已完成并通过Native40/40、Windows12/12定向验证. 源码提交:
- 提交: 254a275e3c6d535b201801552761edbb7f7413cc

用户授权下已cargo install默认功能+稳定签名并启动mac.lab, Windows默认功能release+DirectML经原任务切换到PID63640/Session1/5555. 正式隐窗和子树取消通过, 配置/任务未变. target无占用后cargo clean回收9.9GiB, 清理后双端fresh ping通过. 详细验收与回退材料索引见WORKLOG__yino_rdog_feedback.md, 全仓/完整OCR/GUI/PTY等边界保留. 部署账本提交:
- 提交: a80d0bebedf6675816bf23f3cd7465f082124aaf

## 2026-11-26 修两处既有测试资产缺陷 (RAI-396 recording_e2e 夹具 / RAI-386 Jail cwd 竞态)

cargo 1.100 的 build-dir 布局让 `recording_e2e` 的 `rdog_binary_path()` 在夹具阶段就
退出 (1 passed; 5 failed; 0.00s), 同时 `figment::Jail` 删进程 cwd 让 shell-lane 用例
随机翻红。两条都不是业务回归, 是测试资产没跟上工具链与并行调度。

- 夹具改成先构造去重候选表再按序命中 (`target/{profile}/rdog` -> `deps/rdog` ->
  `build/*/*/out/rdog` 扫描 -> 测试可执行文件所在目录), 全部未命中时 panic 并逐条
  列出尝试过的路径。实测: 真 build-dir 布局 6/6, 人为只留 `target/debug/rdog` 的
  旧布局 6/6, 两者都没有时 panic 列出 4 条候选。
- 新增 crate 级 `src/test_support.rs` 的唯一 `cwd_test_guard()`: 36 个 `figment::Jail`
  用例与 6 个 spawn shell 且严格断言 stderr/stdout 的用例共享。Jail 自带的锁是私有
  static, 只串行化 Jail 之间, 挡不住外部的子进程继承到即将被删的 cwd。
  修前 `cargo test --bin rdog -- --test-threads=4` 6 轮红 3 次 (stderr
  `sh: 0: getcwd() failed: No such file or directory`), 修后 10 轮全绿 (1107 passed)。
- 评审加严: 夹具首选 cargo 注入的 `CARGO_BIN_EXE_rdog`, 只有它缺失时才按布局探测,
  且 build 目录扫描改为 mtime 新的在前 —— 避免选中旧 hash 产物里的 daemon。
- 顺带补记 RAI-387 的合并提交 (纯账本缺口, 该 commit 无代码改动)。
- 提交: d8cd8e8
- 提交: 1034bb7
- 提交: 176d191
- 提交: 6924b82

## 2026-11-26 ort-sys 获取可复现 + 合并门区分「未运行 / 失败」(RAI-397)

`main` 的 `test (windows-latest)` 自基线起在 Build 就红, 一次测试都没跑到。日志里的
`Os { code: 3, kind: NotFound }` **不是下载失败**: ort-sys build script 先成功下载
`+directml` 包, 然后 `build/dynamic_link.rs` 的 copy-dylibs 把 dist 里的 `DirectML.dll`
复制到 `<OUT_DIR 第 3 个祖先>` 及其 `examples/`、`deps/`, 而 `cargo build --bin rdog`
从不创建 `examples/` → `fs::copy` 目标路径不存在 → panic。Linux 的 dist 只有静态
`libonnxruntime.a` (匹配不到 `.so`), 所以这条路在 linux/macos 上从未被走到。

本地复现 (linux, 在 pyke 缓存目录里放一个 `libdummy_rai397.so` 再 `cargo clean -p ort-sys`):
panic 于 `dynamic_link.rs:45` (unix 分支, 同一循环); `mkdir -p target/debug/build/examples`
后同一构建 16.5s 成功并把 dummy 软链进 3 处。Windows 侧的第 3 个祖先是 `<profile>/`
而不是 `<profile>/build/`, 所以两个层级的目录都铺。

- 新增 `scripts/ci/prefetch-ort-dist.py`: 目标三元组→dist 行的映射**不手抄**, 从
  `Cargo.lock` 锁定的 ort-sys crate 内嵌 `build/download/dist.tsv` 读 URL+sha256
  (选行规则对齐 `resolve_dist()`: 本仓库没开任何 ort backend feature, 取首行),
  校验后按 `<cache_root>/dfbin/<target>/<sha256>/` 落盘 → build script 命中即跳过网络。
  实测: 缓存移走后 MISS→下载→PREFETCHED (linux `libonnxruntime.a`, windows
  `DirectML.dll` + `onnxruntime.lib`); 随后把 `HTTPS_PROXY` 指向不可达端口构建仍
  `Finished dev profile`, 日志里没有 ort-sys 的 `downloading from`。
- 新增 `scripts/ci/assert-tests-ran.py`: 解析 `cargo test` 日志的 `test result:` 行。
  `0 passed` 判红 (`tests did not run` 注解), 有 failed 判红。本地真实日志 1107 passed 通过。
- `.github/actions/cargo-prep/action.yml`: build / test 两个 job 共用同一份准备
  (原生依赖 / 工具链 / cargo 缓存 / ort dist 缓存 / `cargo fetch --locked` / 预取 /
  ort-sys 目录), 避免"两边环境不同"制造新的不可归因红点。
- `.github/workflows/ci.yml`: `build (os)` 与 `test (os)` 拆开 —— 同 OS 的 test 被跳过
  就是「测试未运行」, test 红才是「测试失败」; Windows 加 `find ... DirectML.dll`
  铺放断言; `pull_request.types` 显式带 `ready_for_review` (默认类型不含它, 标 ready
  的 PR 会 0 run 而门读成"没有失败")。
- `docs/agents/ci-merge-gate.md`: 必需检查表 (7 条)、触发面、红点判读表格, 以及
  ort-sys/onnxruntime 获取契约与"改 Cargo.lock 里的 ort-sys 版本 = 复核这条链"。
- 提交: 139becb
- 提交: 360c000 (build 失败摘要 + 已知红到期日规则)
- 提交: fb378c4 (按 OS 拆 needs / 测试目标编译进 build / 注解化退出 / 编码)
- 提交: 088b876 (评审第二轮: 扁平化六个显式 job / lockfile 缺口归类)。reusable workflow 会把检查名改成 `<caller> / <callee>`, 与必需表冲突; 三个 OS 的步骤文本因此是复制的, 改一处要改三处。
- 提交: 85f7df7 (评审第三轮: 覆盖说法收敛到实际范围 + 缺口进已知红表)
- 提交: 403fb9a (评审第四轮: 覆盖缺口 18 个 + 从已知红表移入 §3 披露)
- 提交: 80ddfc3 (windows 首次跑到本组测试抓到的 ConnectionReset: 12 处读响应改成 EOF/reset 皆终止)
- 提交: 00bb079 (windows 并行 spawn 偶发 ERROR_FILE_NOT_FOUND: 对 NotFound 有界重试)

## 2026-11-26 serial listen endpoint 不再当可达 locator 通告 (RAI-383)

Zenoh 对 serial 端点是惰性的 (`zenoh-link-serial::new_listener` 不打开串口, 只把
endpoint 记进 locator 集合), 于是打不开的串口也会出现在 `Zenoh can be reached at:`
里, 对端按它拨号必然失败且启动日志无提示。RAI-383 按 RUST-3 的真机实测重写后,
落地两层:

- 新增 `src/zenoh_runtime/listen_plan.rs`: 启动期分类 listen_endpoints —— 必选
  (tcp/udp/tls/unixpipe) 原样交给 Zenoh (绑定失败仍致命, 形状非法直接报错),
  可选 (serial) 先探测设备 (unix 看字符设备, windows 按 zenoh 的 exclusive 语义试开);
  不可用的 serial 记入 dropped 并由 `run_router_daemon` 打一条含 endpoint/原因/
  影响/改法的 warn, daemon 继续启动。ready 日志改为回读实际生效集合。
- 模板收窄: `rdog_win.toml` / `rdog_linux.toml` 的 serial 端点改为注释 (与
  `rdog_macos.toml` 一致), README 最小配置同步并写明该语义。
- 实测 (Linux 云机, 隔离 HOME): 修前同配置下 `serial/NOSUCHPORT` 与
  `serial//dev/rdog-rai383-no-such-port` 都出现在 `can be reached at`; 修后两者
  都被剔除并各留一条 warn, `tcp` 入口 `@ping` 回执 `pong:true`; socat pty 冒充
  可用串口时该 endpoint 正常进入通告集合。`cargo test --bin rdog` 1111 passed /
  0 failed / 2 ignored (新增 4 条 listen_plan 单测)。
- 未验: Windows 侧只落到代码与语义 (试开 `\\.\COMxx`), 本机无 Windows 环境实跑。
- 提交: 564fc4a

## 2026-11-26 RAI-383 评审修复与知识 Capture

- 评审抓到 windows 探测缺陷: `std::fs::OpenOptions` 默认共享模式与 Windows 通信设备
  不兼容 (`serialport-4.9.0/src/windows/com.rs:62` 用 `share_mode 0`), 会让可用串口在
  探测阶段被判死并永久剔除。已改成读写 + `share_mode(0)`, 与 zenoh 底层同一打开语义。
  本机无 windows 环境, 该修复的编译与 `COM255` 负例由 windows-latest job 给出证据;
  Linux 侧重跑 `cargo test --bin rdog` 1111 passed / 0 failed / 2 ignored, 两份真机
  回执 (不可用剔除 + socat pty 可用保留) 在新二进制上复跑一致。
- 知识 Capture: `docs/solutions/conventions/zenoh-serial-listener-lazy-locator.md`,
  根 `AGENTS.md` 粗索引与 capture 记录同提交更新。仓库里没有
  `docs/solutions/AGENTS.md` 细索引, 也没有 `scripts/check-solutions-index.mjs`,
  故只跑现存的两份守卫 (`check-carrier-assertions.py` / `check-bookkeeping.py`)。
- 提交: ddf32fb
- 提交: d226320
