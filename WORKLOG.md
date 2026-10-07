# WORKLOG: rustdog

> 本文件由前档 `WORKLOG_2026-09-02.md` (1125 行) 续档而来。前档含 Wayfinder #95 全程、
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
