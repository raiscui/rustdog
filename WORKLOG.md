# WORKLOG: rustdog

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

## [2026-11-24] RAI-178: P2-A ACL 与租约可否定实验（仅本地）

- 显式注入单调时钟, 验证租约截止边界、可续租、云不可达不可续租、目标绑定、查询 ACL 和旧实例 fence；5/5 通过。
- 提交: c4fea5f44912a1c0b1fb6cdb9ef3c63229d701b8
- 用 18 个实际 iroh 回环进程验证可注册/续租/查询、同名竞争、旧实例注销拒绝、握手后准入拒绝、现实短租约与云失联回收。保持正式 CLI/config/协议/锁不变；既有回归通过。临时契约见 [rdog-crossnet-p2-a-probe](specs/rdog-crossnet-p2-a-probe.md)，生产凭证、政策和撤销顺序维持 HOLD。
- 提交: a81553f32c2ee0341154240b64747c04dd4f1bce

## [2026-11-24] RAI-168: 正式 CLI iroh 只读闭环（仅本地）

- 同一个默认 OCR release 二进制启动只允许回环的 rendezvous（内嵌 HTTPS relay 和 QAD）、daemon 和 control。按名探活经 iroh 直连或 relay 均返回 `id:42/pong:true`; 另一临时身份在两条路径完成握手后被 daemon 明确拒绝, ping handler 均未进入。
- 复用既有 parser、ping handler、`ControlPeerSession` 和扩展后的 `ControlTransport`。旧 TCP、WebSocket、Zenoh 的针对性集成测试、Linux unixpipe 单元和 CLI 冒烟通过。临时契约见 [rdog-crossnet-p1-readonly](specs/rdog-crossnet-p1-readonly.md); 未接入 bulk、其他业务命令或生产授权。
- 提交: 15f3ce1f032a65be4638b328fe83ad789a77e648

## [2026-11-23] RAI-137: iroh 回环准入探针（仅本地）

- 用 RAI-131 实编的 iroh 1.4.1 在仅回环的网络命名空间验证三个连接。两次获准连接都收到 `@ping#42` 的结构化 pong; 不在测试 allowlist 的临时身份完成 QUIC 握手后被明确关闭, ping handler 未进入。
- 只添加手工测试探针, 没有改动根锁、默认特性、生产命令或授权策略。探针没有调用生产 ping handler, 不代表官方 CLI 已接入 iroh。
- 提交: 34909253345e4af4841efd4866df55dbb7418b68

## [2026-11-23] RAI-131: Rust 1.95 根包默认构建（仅本地）

- 从 RAI-128 私有基线建立候选, 根包保留默认 OCR, 精确 pin `iroh` 与 `iroh-relay` 为 `=1.4.1`, 后者启用 `server`。Linux x86_64 在空目标目录用 Rust 1.95.0 进行 `cargo build --release --locked --bin rdog` 成功, 编译事件确认 OCR 和两条 iroh 依赖均参与实际构建。
- 同一 release 二进制在仅 loopback 的网络命名空间完成既有、带认证的 Zenoh `@ping#42`, 返回原 request ID 和 `pong: true`。没有执行 shell 指令或改动控制行为, 未验证 OCR 推理或跨网 P2P。
- 提交: 77215d8cd21fdb38b5117eadd8823257c5e4c84f

## [2026-11-23] RAI-128: fixed-echo 原型恢复（仅本地）

- 从 Mac 仓库仍可读取的 Git 快照 `50d55eba98fecef92f08354464aeb55a14b8a5d7` 恢复四个原型源码/测试文件。逐文件核对模式、大小、Git blob 和 SHA-256。原临时工作树已不存在, 不将快照视为对其他未提交文件的完整保全。
- 保留独立原型锁文件与 `iroh` / `iroh-relay` 的 `=1.4.1` pin。Linux x86_64 用 Rust 1.91.0 验证 release 构建、两条写失败回归、既有本地 echo 脚本和独立首流隔离场景。未改动正式控制面, 未运行跨网验证。
- 提交: 16df4c8975fa478363157ec1623727f032b61857

## [2026-11-23] RAI-128: iroh 原型本地构建基线

- 从 `5588fcb75bdb77735e4e43af19234a82b822735d` 在隔离分支导入公开原型 `eaaf5a677021f1d737d58597b4696e37532c42b2`。保留两侧历史, 无冲突; 导入时原型子树与上游完全一致。
- 提交: b2768fd31020c342a3d8b39cf44ebd61d0ab7ace
- 原型 `Cargo.toml` 精确 pin `iroh` 和 `iroh-relay` 到 `=1.4.1`, 原型 `Cargo.lock` 未变。Linux x86_64 干净 `cargo +1.91.0 build --release --locked` 通过。主线根目录同命令在默认特性下被锁定的 `oar-ocr` 0.9.2 依赖族的 Rust 1.95 要求拦截。主线清单和锁文件未变, 尚无合并依赖图, 未取得原工作树的修复。
- 提交: be1a9b9e80960cf97dd230f7239f7bc9ffb11e97

## 2026-11-25 RAI-183: PTY 桥接错误后回收当前会话

共享 PTY 桥接在不可恢复的传输读写错误后请求回收; runtime 只处理仍归该控制端的会话。正常退出、显式 detach 和重附语义保持, Iroh 正式入口未开放 PTY。
隔离 Linux 定向用例 7/7, Rust 1.95 默认特性 release 构建通过。既有 PTY 套件 5/8, 三条红项均在等价基线复现; 未将全套标为通过。
- 提交: 15aadeb34fac8f5ba9bb6512798900332cb70f98

## 2026-11-25 RAI-179 U1: Iroh 会话准入契约

冻结 H1=B、H2=A、H3=C 及角色、拒绝和终态边界。默认入口保持 ping-only；隔离入口验证与运行证据留给后续单元。
- 提交: 1d1d5c65ab980c24cacf1b7a5080bf32ebcb4907

## 2026-11-25 RAI-179 U2: 隔离准入模型

测试编译内的签发、续租、本地策略、撤销及固定 PTY 夹具通过 4 项定向验证；正常 Iroh 入口仍只接受 ping。此单元未证明 Iroh 传输上的 PTY 生命周期。
- 提交: 0df1f9de885a38778076da0f6242376944cb442f

## 2026-11-25 RAI-179 U3: 固定夹具的隔离生命周期

真实回环 Iroh 测试入口只运行固定无害 PTY 夹具，验证身份拒绝、重附与 stale owner、租约到期、graceful/emergency 终态及回收；本地真实 TTY 另验恢复。默认生产入口仍拒非 ping。
- 提交: a2bd9b3b23bfd6de846a52bc0bb923191f3c9bb9

## 2026-11-25 RAI-179: 隔离验收评审补修

测试入口定时回收已 detach 夹具，确认终态后清理 live 状态，持久写入不等同于现存终止，实例内按 UUID 去重。四条定向红绿均复核；默认 release 仍拒非 ping。
- 提交: ee9b2fc603dd559c7c85ebb6c5229dcb83109683

## 2026-11-25 RAI-179: 隔离终态授权与撤销时钟

终态重复 Close 仍按已认证 peer 和最后 attachment 授权；emergency 在持久化成功后读时钟。两项定向红绿和隔离生命周期通过。
- 提交: 1df17acd868e92da01f0da05e216919ae103fef8

## 2026-11-25 RAI-305 U1: Iroh 跨网 profile 地址放开

profile 新增 `routable`(缺省 false)与 `bind`; `network`/`relay_allowed` 是唯一地址策略, 目录、endpoint、relay-QAD、advertise 与注册校验都走它。缺省 profile 仍是 P1 的 IPv4 回环门禁与原错误文案; routable 下拒绝未指定/组播/广播地址。4 项定向用例从红转绿(基线红是 `routable daemon profile parses: invalid iroh JSON profile`), 16 项 control_iroh 既有用例保持绿。
- 提交: b5205ba

## 2026-11-25 RAI-305 U2-a: 准入语义搬出测试构建

`Admission`/`SignedGrant`/`Policy`/撤销语义从 `#[cfg(test)]` 的 `admission_tests.rs` 搬进生产模块 `src/control_iroh/admission.rs`, 生产与测试共用同一份实现; 夹具与 Lane 驱动留在测试侧 (`fixtures.rs`/`lifecycle_tests.rs`)。新增 `sweep`: 同一锁与同一时钟下处理到期、已落盘的 emergency 标记与未确认终帧的重发。
搬移后 RAI-179 隔离证据重跑通过: 18 项单测/驱动用例全绿; 隔离 Iroh 生命周期矩阵 graceful 3006/3000ms 报 `lease_expired`、emergency 256/12000ms 报 `lease_revoked`, 授权前零进程、重附与跨主体不误关均保持; 真实本地 TTY 全部标志恢复。
- 提交: 52ab6a1

## 2026-11-25 RAI-305 U2/U3/U4: iroh 承载附着式 PTY 会话

生产接线: daemon profile 给出 `policy_file` 时启用准入 PTY 通道(`serve_admitted`), 缺省仍 ping-only + 回环; 通道只服务 ping 与 PTY 帧, 其余 control 行按 `LANE_REFUSED` 拒绝, bulk 保持关闭。`route`/`network`/`relay_allowed` 是地址唯一策略, `Route::Auto` 提供直连不可得时经 relay 的交互回退并记录实际路径。
租约: 客户端按 TTL/3 续租, 服务侧 `lease_sweeper` + `Admission::sweep` 在同一锁与同一时钟下处理到期、已落盘 emergency 标记与未确认终帧; detach 后无后续请求也会到期回收。
关闭授权(Rai-298 契约重实现): 按主体判定, 同主体跨连接可关自己的会话, 异主体 `iroh_subject_denied` 且目标继续运行; 旧通道维持按 id 关闭并已在规格写明"可达即全权"。
证据: 隔离回环矩阵 8/8(`tests/manual/iroh_pty_lane.py`, 含准入前零进程、stdin 回显、跨连接关闭、异主拒绝、detach 到期回收、graceful/emergency、真 TTY); 19 项 iroh 单测; 搬迁后 RAI-179 隔离夹具矩阵重跑通过。`control_pty` 2 项与 `control_tty` 1 项为 util-linux `script` 2.39.3 的 BrokenPipe 环境红, 与候选无关(基线复核见回执)。
- 提交: 187c2e5

## 2026-11-25 RAI-305 U6: 跨网验收与回退修正

非回环验收改成三主机拓扑(`tests/manual/iroh_pty_routable.sh`): 客户 10.41.0.1 / 中转 10.41.0.2-10.42.0.1(目录 + relay, 开转发) / daemon 10.42.0.2, 客户能到 relay 与目录但直连 daemon 被黑洞。11 项全绿: 真实握手、长会话 stdin 回显与 exit、同主体跨连接关闭、异主拒绝且目标存活、detach 到期回收、graceful/emergency、relay-only 可达、直连被黑洞时 `direct` 必失败而 `auto` 落到 `iroh_path=relay`(实测 auto 11.1s)。回环 8 项、iroh 单测 19 项、profile 测试 5 项同步复跑通过。
U3 行为基线: 同一矩阵里 `direct` 在直连被阻断时失败、`auto` 成功并记录 `iroh_path=relay`, 构成红/绿对照。
发现的真实缺陷与修法: 同一 endpoint 先试 direct 后, 远端状态会持续重试已知路径, relay 候选拿不到新的 QAD 应答; 改为 `candidate_endpoint` 按候选建 endpoint, 并给 relay 候选独立的 `RELAY_CONNECT` 预算。
环境红: `control_pty`/`control_tty` 的 3 个用例在 `b5205ba` 与候选顶点红集完全相同(util-linux `script` BrokenPipe), 与候选无关。
- 提交: bc2eb93

## 2026-11-25 RAI-305 复核处置: 续租与终止

非作者只读复核给出 2 条 high, 均经代码核对成立并已修:
- 续租帧被桥写进远端 PTY 的 stdin, `Admission::renew` 在会话期间永不执行, 会话到 TTL 被 sweep 杀掉。红基线: 2s 租约下终端出现三份 `{"op":"renew",...}`、客户端报 `remote PTY closed before natural exit: lease_expired`(`receipts/u7/baseline-red.log`, sha256 a1ae1e37…)。修法: lane 侧 `LaneIo` 在桥接层截获 `Handshake::Renew` 并处理, 同处拒绝指向异主体会话的 close。绿: `receipts/u7/green-loopback.log`(sha256 4844d3f4…)、三主机 `receipts/u7/routable.log` 12/12 含 `renewal: a session outlived its lease deadline`。
- 回收只杀直接子进程, 无进程组信号、无 SIGKILL 升级。修法: `signal_pty_group` 按 `setsid` 会话首进程发组信号 + `escalate_pty_group_kill` 在 `PTY_KILL_GRACE` 后升级。绿: `revocation: grandchild 93065 died with the session`(回环)、`grandchild 93337`(三主机)。
另修: 一条租约只持有一个 PTY(`Admission::session_of`, 否则 detach 后同租约再开会让第一条永久无回收路径)、grant 容忍 `CLOCK_SKEW`、`MIN_SESSION_TTL` 下限(夹具租约 300ms 同步改为 1000ms, 断言未削弱)。规格已同步并指回 `LaneIo` / `signal_pty_group` / `escalate_pty_group_kill` / `CLOCK_SKEW` / `MIN_SESSION_TTL` / `PTY_KILL_GRACE`。
未修(记录在报告): 3s 写超时按断链处理、租约表不裁剪、attach 的 200ms 到期窗口、`iroh is only supported by the ping one-shot CLI` 的陈旧注释。
- 提交: 8b99436

## 2026-11-25 RAI-305 验收补口

按复核指出的缺口补 attach 用例: `--pty-detach` 之后由第三个连接 `--pty-attach` 重新附着、回显并关闭。回环 11/11(`receipts/u7/loopback-attach.log`)、三主机 13/13(`receipts/u7/routable.log`)含 `attach: ... attached again and closed from a third connection`。同时把 `control_invocation.rs` 里"iroh 只支持 ping 一次性 CLI"的陈旧提示改成实际语义(不是删除检查, 只改文案)。
- 提交: c2d882f

## 2026-11-25 RAI-305 第二轮复核处置

修后 diff 的第二轮只读复核判定三项修复(续租截获、进程组终止、一条租约一个 PTY + TTL 下限 + 时钟容忍)全部成立, 另给四条: 当场修三条 ——
- 宽限期在一半路径上是 0: `kill_pty_child` 传空 session id 使 `pty_session_registered("")` 恒假, SIGHUP 后立刻 SIGKILL。改为 `Option<String>`, 没有会话 id 时走满 `PTY_KILL_GRACE` 再探测组, sweep / `@pty-close` / 断链三条路径同一套升级语义, 规格同步。
- 同主体但指向另一会话的 `@pty-close` 被当字面量敲进当前 PTY 的 stdin: `LaneIo` 记住本 lane 的会话 id, 不匹配的一律丢弃。
- 租约冲突复用 lane 能力拒绝的 reason: 改为独立 `LEASE_BUSY`(`iroh_lease_session_conflict`), 规格错误表加一行。
未修(确认为这批之前就存在): 3s 写超时超时不 reset 的半帧风险, 以及慢链上可能被误判断链。
回执: 回环 11/11(`receipts/u7/loopback-final.log`)、三主机 13/13(`receipts/u7/routable-final.log`)、iroh 单测 19/19。
- 提交: 77f7b42

## 2026-11-25 RAI-305 U7: 半帧安全与写预算

- 半帧: 新增 `FrameGuard`(归一化写帧路径)与 `FRAME_BROKEN`。任何一次写失败/超时都把该流标记不可用, 之后写直接被拒, lane 结束并用同一 reason 关闭会话(`abort_broken_frame`)。红基线 `receipts/u7/u7-half-frame-red.log`(旧行为下第二次写确实进了残留半帧的流, 断言 `a frame written onto a stream that holds a partial frame must be refused` 失败); 绿: `cargo test --bin rdog control_iroh` 23/23。
- 写预算: profile 新增 `frame_timeout_ms`, 两侧校验, `MIN_FRAME_TIMEOUT` 1s / `MAX_FRAME_TIMEOUT` 120s / 缺省 3s。红基线 `receipts/u7/u7-frame-budget-red.log`(字段与校验都不存在); 绿: 两条 profile 用例。
- 三主机实测(如实记录, 与预期相反): 链路消失 2s(`u7-outage-2s.log`)、6s(`u7-outage-6s.log`)、6s+12s 预算(`u7-outage-6s-budget12s.log`)、20s 冻结读端+64MB(`u7-stall-reader.log`) 全部以连接层 `connection lost` / `LastOpenPath` 结束, 均未出现 `iroh_frame_broken`。结论: 路径消失由连接活性决定, 写预算不改变结果; 写预算只管"路径在、对端不读"的写挂起, 该规则的真实网络证据尚未构造出来, 确定性证据是传输层单测。
- 提交: d7c5dd2

## 2026-11-25 RAI-305 U8: 操作者撤销路径

- CLI: `rdog policy --config <daemon profile> <list|admit|revoke|emergency>`(`src/input.rs` + `control_iroh::run_policy`)。list 输出 id + SHA-256 指纹前缀 + 登记时间 + 备注 + revoked/terminate 标记; admit/revoke/emergency 都走 `save_policy`(临时文件 → 替换 → 父目录 fsync), 写失败与持久性未确认分别非零退出并报原因。
- 策略文件新增 `parties`(备注 + 登记时间), 准入判定仍只读 allow/revoked/terminate 三个集合。
- 验收(回环 13/13 `receipts/u8/loopback.log`, 三主机 15/15 `receipts/u8/routable.log`): `policy list` 显示指纹与备注; `policy revoke` 返回 0 后新会话 `iroh_target_denied`、现存会话仍活; `policy emergency` 现存会话被终止且终帧 `lease_revoked`; 重启 daemon 后仍拒新。
- 规格新增"操作者路径"一节(命令、落盘语义、两种失败、重启语义)。
- 提交: 9f30ce3

## 2026-11-25 RAI-305 第三轮复核处置(U7/U8)

复核给 6 条, 全部处置:
- 作废检查在写锁之外(并发写者可绕): `FrameGuard::ensure_intact`, 拿到流锁后再查一次。
- `abort_broken_frame` 对第一次失败不可达(会话其实以 `control_disconnect` 收尾): lane 写失败改带哨兵, 会话真的以 `iroh_frame_broken` 关闭。**正向实测**: 1s 预算 + 20s 冻结读端 + 16MB 积压 → `stall: ... frame_broken=True`, daemon 记录 `iroh lane aborted reason=iroh_frame_broken session=…`(`receipts/u7/u7-stall-budget1s.log`)。
- `policy admit` 对已撤销主体谎报成功: 现在明确报"仍被撤销, 先解封"。
- `run_policy` 读-改-写无锁(并发可覆盖刚落盘的 revoked): 加 `<policy>.lock` 上的 `flock` 排他锁。
- `frame_timeout_ms` 只覆盖 lane: 写进规格(目录跳仍 3s); `@pty-detach` 按本 lane 会话收紧。
- 重启用例使 `path=direct` 断言失效: 改读第一个 daemon 的日志。
复跑: 回环 13/13(`receipts/u8/loopback-final.log`)、三主机 15/15(`receipts/u8/routable-final.log`)、iroh 单测 23/23、shell 单测 46/46。
- 提交: 132c66b

## 2026-11-25 RAI-305 公网 1:1 NAT 的 relay 通告地址(RUST-3 真机反馈)

真机跨网实测暴露的缺口: 1:1 NAT 机型上公网地址(EIP)只存在于 NAT 设备, 本机网卡绑不住
(`[Errno 99] Cannot assign requested address`); 而 registry 用绑定地址拼 relay URL, 对端
因此拿到一个到不了的地址, 注册被 `iroh_registration_denied` 拒 —— 候选在真机起不来。

处理:
- `RendezvousConfig` 新增 `advertise`(缺省空), 只改写给对端的 relay URL, 不改绑定地址;
  `bind` / `relay_http` / `relay_https` / `qad` 仍绑网卡地址。就绪日志同时打印绑定与通告值。
- 规格 `specs/rdog-iroh-admission-contract.md` 写明 `advertise` 有意不过 `network()`:
  它不是本机地址, 按绑定地址规则校验会把唯一可用的配置方式判死; 进 URL 后仍受 `RelayUrl`
  解析与对端 `relay_allowed` 约束。
- 新增隔离证据 `tests/manual/iroh_pty_advertise.sh` → `iroh_pty_lane.py --advertise`:
  三网络栈模拟 1:1 NAT(peer 10.60.0.2 / nat 10.60.0.1+10.50.0.254 / relay 10.50.0.2, 四个端口
  DNAT + SNAT), 三段: 绑私网通告公网 → daemon relay home 为通告值且注册+ping 通过; 不写
  `advertise` → `iroh_registration_denied` 且非零退出; 通告一个没人持有的地址 → 从未注册。

复跑(同一二进制):
- `advertise` 三段全过 `receipts/u10/advertise.log`
- 回环 13/13 `receipts/u10/loopback.log`
- 三主机合成 15/15 `receipts/u10/routable.log`
- iroh 单测 23/23, shell 单测 46/46
- 载体守卫 `check-carrier-assertions.py` OK
- 提交: b57d780

## 2026-11-26 RAI-305 真机换网恢复后的两条运维事实

RUST-3 真机第二轮(家宽 -> 手机热点)补的两条, 写进规格与交付文档, 避免被当 bug 查:

- 客户端 `bind` 必须跟本机当前地址走: `bind_addr` 把它直接交给 `network` 绑, 换网后已在跑的会话
  不受影响(走已建连接), 但新建连接用旧地址会 `EADDRNOTAVAIL`, CLI 表面是 `Failed to bind sockets`。
  实测 192.168.50.143 -> 172.20.10.2 沿用旧值报该错, 改新值立刻 `iroh_path=relay` + `pong:true`。
- relay 主机运行库: glibc 2.39 构建在 glibc 2.38 宿主上 `GLIBC_2.39 not found`(容器 `rust:1-slim-bookworm`
  构建可解); `LD_LIBRARY_PATH` 只能带 GUI 那几个库, 混进 `ld-linux`/`libc`/`libm`/`libpthread`/`libgcc_s`
  会直接 `Floating point exception (core dumped)`。

同步到交付文档: `real3/README.md` 的客户端参数节与 relay 部署节、`profiles/README.md`(模板旁注意事项)。
守卫 `check-carrier-assertions.py` OK。
- 提交: 72b4220

## 2026-11-26 RAI-305 bind 通配化与注册契约放宽(Rais 批准)

动机: 客户端必须填本机 IP 才能用, 换网就得改配置。Rais 追问后批准同时放宽注册契约。

- `bind_addr` 在 `routable` 下把"省略"与 `0.0.0.0:0` 都解析成通配绑定 `0.0.0.0:0`, daemon 与 client
  共用; `bindable` 让绑定面接受未指定地址。通配 + 固定端口仍拒; loopback profile 仍回环。
- `register` 改为枚举端点实际接口地址(`data.addr().ip_addrs()`), 通配绑定下逐接口展开, 不出现
  `0.0.0.0` 字面值; `connect_route` 把**全部** IP 候选一起拨, 不再只用第一个。
- `Registry::request` 的 `Register` 分支: 去掉 `addrs.len() == 2` 与 `ip_addrs().count() == 1`,
  改为 `>= 1` 个 IP 候选 + 恰好 1 个等于配置值的 relay; 逐 IP 过 `network` 且端口非 0。保留
  `owners[name] == Some(peer)` 与 `addr.id == peer`(一个名字一个 owner)。去重无需代码:
  `EndpointAddr.addrs` 是 `BTreeSet`。
- 新增 `src/control_iroh/registry_tests.rs`: 多候选注册+回读, 以及 0 候选/未指定地址/relay 不匹配/
  非 owner 四个负例都落在 `iroh_registration_denied`。

复跑(同一二进制): 回环 13/13(`receipts/u11/loopback.log`)、routable 17/17 显式(`routable.log`)、
routable 17/17 daemon 通配(`routable-wildcard.log`, netns 双接口)、advertise 3/3(`advertise.log`)、
iroh+shell 单测 74 通过 / 2 ignored(netns 专用)、载体守卫 OK。
- 提交: 8ad4aa6

## 2026-11-26 RAI-305 routable 下不回环候选(RUST-3 收口要求)

- `candidate_ok`(src/control_iroh.rs): `network` + 端口非 0, 且 `routable` 下拒回环 —— 对端拨
  `127.0.0.1` 只会打到它自己, 既浪费一次拨号又制造误导性的前半段失败。
- daemon 侧 `register` 用它过滤通告候选; registry 的 `Register` 校验同样用它, 收到回环候选
  按 `iroh_registration_denied` 拒; 非 `routable`(回环 profile)不变。
- `Register` 成功后新增一行 `iroh directory registered name=… candidates=…`, 让通告的候选集
  可观测, harness 在 routable 下断言该行不含 `127.0.0.1`。
- 新增 registry 用例: routable 收到回环候选被拒 / 回环 profile 仍接受回环 socket。

复跑(`receipts/u12/`, 同一二进制): 回环 13/13(`loopback.log`)、routable 显式 18/18(`routable.log`,
`candidates=10.42.0.2:36558`)、routable daemon 通配 18/18(`routable-wildcard.log`, netns 双接口
仍只通告 `10.42.0.2:58186`)、advertise 3/3(`advertise.log`)、control_iroh 29 passed / 2 ignored、
shell 46 passed、两个载体守卫 OK。
- 提交: a897b79

## 2026-11-26 RAI-413 把 iroh 跨网 lane 落到 main（rebased 到 b227c63）

落地方式：lane 的历史在 `capy/rai305-iroh-crossnet-lane`（备份顶点 `ef6360c`）；PR 分支 `rai412` 以当前
`main`(`b227c63`) 为基，用两提交承载同一棵树——工作提交 + 本记账提交，避免 55 个历史 commit 逐个与
`WORKLOG.md` 冲突。`tests/**`（含 #138 的 `tests/support/rdog_binary.rs`）保持 main 原样，本 lane 只新增
`tests/manual/**`。
- 提交: fc17ebd
  - Windows 编译门(同轮补): `a43cbca` —— `OpenOptionsExt::mode` 与 `libc::flock` 的 `#[cfg(unix)]` 门控

## 2026-11-26 RAI-413 #141 评审六条处置

评审给 6 条(5 medium / 1 low), 逐条对照 diff 后全部成立并已修:
- **重附会话仍按原租约到期**: 附上后没有把会话从 detach 掉的租约转移走, sweep 仍按旧租约截止杀死它。新增
  `Admission::attach_session`(清旧租约 pty + 记到新租约), 附分支先转移再桥接; 单测
  `an_attached_session_survives_the_lease_that_detached_it` + harness 在旧租约截止后再写一行验证仍通。
- **输出积压饿死续租**: `bridge_attached_pty_session_inner` 无界排空输出队列, 读侧拿不到 `@lease-renew`。
  改为每轮最多 64 帧后必定读一次(桥接本就用 25ms 轮询读超时, 代价可忽略)。
- **过期连接仍持生命周期权**: attach/close/detach 只查主体不查本连接的租约。三个分支都加 `require_active`。
- **策略准入无法让新客户端进来**: 连接层用静态 ping allowlist 拦, `rdog policy admit` 白写。有 policy 时
  准入 lane 只按 policy 判, 无 policy(ping-only lane)仍用 allowlist。
- **并发写可接在半帧之后**: `FrameGuard` 在释放 stream 锁之后才标 broken。改为在锁内 `mark_broken()`。
- **相对 policy 路径必报持久性失败**: `Path::parent()` 对 `policy.json` 返回空串 → `File::open("")` 失败。
  空父目录归一为 `.`。

复跑: `--bin rdog` 1144 passed / 0 failed / 4 ignored; `control_iroh` 30 passed / 2 ignored; 回环 13/13(含新增
的重附跨原截止用例); `control_pty` 5 passed / 3 failed 与基线逐项相同; `control_tty` 0/1 与基线相同。
- 提交: f9543b7

## 2026-11-26 RAI-413 #141 第二轮评审两条处置

- **附上第二个会话会孤立第一个**: 一个租约若已持有一个仍然存在的会话, 再附上另一个会话会让前者失去唯一回收者
  (sweep 只走租约指向的会话)。`attach_session` 增加 `held_alive` 判据: 租约已持有别的活会话即拒(映射 LEASE_BUSY);
  持有的会话已不存在则不阻塞。新增单测 `a_lease_cannot_take_a_second_session_and_orphan_the_first`。
- **未登记对端会白占 task slot**: 上一轮把连接门放宽成"有 policy 就放行", 结果任何能拨通的对端都会拿到
  `MAX_SESSIONS` 名额并在 `serve_admitted` 里等满 `HANDSHAKE_TIMEOUT`。改为**在分配 task 之前**问 policy
  (`Admission::party_permitted` + `identity_admitted`), 未知对端立即拒。

复跑: `--bin rdog` 1145 passed / 0 failed / 4 ignored; `control_iroh` 31 passed / 2 ignored; 回环 13/13; 两个守卫 OK。
- 提交: 9a32601

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

## 2026-11-26 RAI-413 #141 第三轮评审一条 + 重定基到 03ff36e

- **被拒的重复附上会把目标会话留在"已附"状态**: attach 分支先 `attach_active_pty_session`(运行时会标记 attached
  并装帧发送器) 才做准入检查, 拒绝时 `AttachedPtySession` 被直接丢弃、没 disconnect, 目标会话便挂在一个已消失的
  控制会话上(输出线程下一次 send 失败即停, 后续 attach 报 already attached)。改为**先查后占**: 用
  `session_of(lease)` + `pty_session_registered` 预检(与 open 分支同形), 再 claim; 防御性地在 `attach_session`
  拒绝分支补 `session.disconnect()`。
- 分支重定基到 `main`(`03ff36e`, #137 落地后)。`tests/**`(含 #138 的 `tests/support/rdog_binary.rs`)保持 main 原样。

重定基后复跑: `--bin rdog` 1145 passed / 0 failed / 4 ignored; `control_iroh` 31 passed / 2 ignored;
回环 13/13; `control_pty` 8 passed / 1 failed(`ctrl_c`) vs 基线 7 passed / 2 failed(`ctrl_c`+`string_shorthand`);
`control_tty` 本分支 2 passed / 0 failed, 与基线同(基线此前已转绿)。
- 提交: 2b30395

## 2026-11-26 RAI-413 #141 第四轮评审一条(low)

被拒的重复附上原用 `session.disconnect()` 释放占位, 但 Disconnect 是**关闭**语义(运行时 Killer 该子进程),
而目标是别的租约的会话 —— 拒绝不该毁掉它。改为 `session.detach("attach_refused")`: 清 owner、会话继续存活。
该分支当前不可达(预检与 `attach_session` 判据同源, 租约无删除路径), 属防御性修正。

复跑: `--bin rdog` 1145 passed / 0 failed / 4 ignored; `control_iroh` 31 passed / 2 ignored; 回环 13/13。
- 提交: 0993ff6

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

## [2026-11-26] 控制行分片到达时被读超时丢弃, PTY 的 ^C 因此失效 (RAI-415)

- 根因: `src/control_line_read.rs::read_line_limited` 入口 `buf.clear()`; `read_until` 在
  「正文已到、换行未到」时被读超时打断返回 WouldBlock, 下一轮调用把半行清掉 → 整条消息静默丢失。
- 触发面: `transport.write_message` 是 `write_all(message)` + `write_all("\n")` 两次写, 25ms 读超时的
  PTY 桥接最容易在两次读之间撞超时。表现: `@pty-stdin`(含 ^C) 帧被吞, 远端收不到 SIGINT。
- 定位链: 远端 pty termios 正常 → helper 即前台 pgrp → python `pty.fork` 纯 kernel 对照正常 →
  直连 daemon TCP 发同一帧正常 → strace 双侧发现 daemon 收到帧却只写出过 `\4` 没写出 `\3`。
- 改法: 入口不清 buf, `total` 从 `buf.len()` 起算, 只在整行交出/超限丢弃/干净 EOF 时清空。
- 评审修复 e884f71: 状态收进 `LineReadState`(半行 + 丢弃标记 + 已丢弃字节数), 修 CRLF 边界
  (尾部 `\r` 不计入内容长度) 与丢弃计数跨读超时丢失(4097 报成 1537); 三个调用方共用同一状态。
- 评审修复 6a009ae: 跨调用保留的半行会绕过 8 MiB 上限(错误分支在长度检查前返回);
  改成进入时先判 limit、丢弃状态用 buf 留 `limit+1` 字节跨调用传递(截断而非清空),
  否则下一轮会忘记状态把剩余数据当成新行。回归测试覆盖 8 块超限分片。
- 回执: `control_pty` 9 passed(断言零改动) / 全量单测 1110 passed / control_tty·shell_pty·control_lanes·
  control_websocket·connect_modes 全绿; 分片复现命令修复后 3/3 成功。
- 提交: 49ea862 (评审修复 6a009ae / e884f71; 均为 rebase 到当前 main 后的工作提交)

## 2026-11-26 RAI-368 Zenoh Windows scout 接口与 loopback 告警

- Windows 的默认 Scout 使用本机非回环单播 IPv4 列表；显式 `zenoh.scout_interfaces` 优先，空列表不写 Zenoh 接口键。Unix 无显式设置时继续用 `auto`。
- 提交: 8133573

- Zenoh 的 TCP/UDP listener 全为回环时给出告警，仍允许本机使用。与 Scout 接口改动可独立回退。
- 提交: 413182d

本轮 Linux 验证：构建通过，config 单测 46/46，session 单测 6/6；未跑真机或网络 e2e。

## 2026-11-26 RAI-385 Windows 本机自连:端口简写映射、回环会话关闭、缺失的 fast path

实测钉死根因(不是推断): zenoh 模式的 daemon 强制 `inbound.enabled=false`,配置里的
`tcp/...` 端口只跑 zenoh link 层 —— 原始 TCP 握手成功, 发 line-control 字节被 zenoh
丢弃链路, 客户端读 EOF, 于是报「在收到 UI script 结果前就关闭了」。另一条更早的坏路:
只写端口时 host 被解析成 `0.0.0.0`, 拿 listen 地址当 connect 目标, Windows 上 os error 10049。

- `connect_host`: 未指定地址 (0.0.0.0 / ::) 统一映射回环, 端口简写与 host+port 共用出口。
- `src/local_daemon_registry.rs`: daemon 路由就绪后登记本机控制 lane (zenoh + 回环
  connect endpoint), liveness 用 lock 上的 OS advisory lock; unix 仍独占 unixpipe。
- `resolve_local_control_target` / `promote_loopback_tcp`: `self` 与 `control <回环端口>`
  按登记解析协议; 显式 `--transport tcp` 不改写。
- 对端一帧未回就断开时给出可行动错误 + `RDOG_LOG_LEVEL=debug` 的 `control link:` 回执。

验证: `cargo test --bin rdog` 三轮全量并行各 1116 passed / 0 failed / 2 ignored;
Linux 同构现场复现 + 修复后三条本机路径都回 `@response`(回执见
handoffs/rai385/receipts/, reports/RAI-385.md); Windows 真机回执由总控代跑。
`tests/control_lanes` 15/15 卡在 `rdog_binary_path` 的 target 布局(RAI-396 只修了
recording_e2e), 属既有测试资产缺陷, 冻结基线同款红; `tests/zenoh_unixpipe_fast_path`
是 `cfg(target_os = "macos")`, Linux 上 0 用例不是回归。
- 提交: 268e50f

## 2026-11-26 RAI-385 评审回合:四条发现全部为真并修复

Capy review 给出的 4 条(2 high / 2 medium)复核后全部成立,均属会伤到目标平台的缺陷:

- **bind/connect 语义混用**: `host_from_opts` 被 `Command::Listen` 共用, 我的回环映射会让
  `rdog listen 5555` 只绑 127.0.0.1。拆成 `host_from_opts`(bind, 保持 0.0.0.0) 与
  `connect_hosts_from_opts`(connect, 未指定地址 -> 回环)。
- **Windows 强制范围锁**: `File::try_lock` 在 Windows 是 LockFileEx, 客户端读被锁范围会失败,
  活着的 daemon 会被判残留并删登记 —— 直接废掉本票能力。liveness 改为只看锁争用。
- **残留撤销竞态**: 改为在独占锁下重读比对后才删; guard drop 不再删 lock 路径。
- **端口匹配丢弃方案**: 只认 `tcp/` + 回环 host, 避免 udp lane 抢走同端口号的 TCP 请求。

复验: `cargo test --bin rdog` 三轮 1117 passed / 0 failed / 2 ignored; Linux 三条本机路径
重新跑过仍全部 `@response`。`tests/control_lanes` 的 15 红在干净基线上逐字同款(回执
`handoffs/rai385/receipts/04-baseline-control-lanes-red.txt`), 属既有夹具缺陷。
- 提交: bb94451

## 2026-11-26 RAI-385 第三轮评审 (1 low) 修复:登记锁与客户端探测的启动竞态

客户端 `live_local_daemons` 的 liveness 探测会短暂持 `<ns>__<name>.lock` 共享锁;若 daemon
正在启动并 `try_lock`,会拿到 WouldBlock 并被当成"已有活跃 daemon",登记静默缺失到下次重启。
`register_local_daemon` 改为退避重试(5 次 x 80ms),新增用例
`registration_should_retry_past_a_short_liveness_probe` 锁住该路径。
- 提交: 03bf1e8

## 2026-11-26 RAI-385 收尾:跨机验收通过 + 凭证同源坑入册

总控用同一分支 88ef40e 在 Mac 重建客户端后, 跨机 zenoh 两条命令 (显式 entry-point / 纯发现)
都拿到 @response, 四条验收全部成立。中途那次失败证伪了"跨网坏了"的猜测: 隔离 profile 的
daemon + 主张量 profile 的客户端 = 凭证不同源, zenoh 在 OpenSyn 阶段回 Close(GENERIC),
表象像网络问题。该门槛写进 docs/solutions/conventions/e2e-isolated-home-credentials.md
Guidance 第 0 条 (不只 e2e, 人工跨机验收同样适用)。
- 提交: 6b4893f
