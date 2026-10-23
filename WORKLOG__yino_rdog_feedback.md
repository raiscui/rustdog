
## [2026-10-23 20:52:30] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 取证与首轮优化

已完成: 检索 yino 根上下文与 archive 共 577 份 Markdown, 30 文件/65 行命中, 归纳 8 类需求与缺陷, 来源见 notes, 对账及分阶段方案见 specs/yino-rdog-usage-improvement-plan.md. GitHub issue 已只读检查, #121/#122/#123 与 #106 继续沿用, 不重复立项.

红灯: nextest run 04bd0dda 的 PowerShell builder 测试失败; run 0cb65c0a 的真实 sh exit0+stderr 测试失败, 实际 error_code=shell_exit_nonzero. 说明 progress 默认未关闭及错误分类不实, 不证明 Windows 原现场机制已完全复现. 本机 pwsh 子进程启动两次分别在 20s/35s 超时, 均已由 subprocess 超时回收, 不再反复重试.

实现: 初始化收敛到现有 build_shell_command, 删除同步入口的单独前缀; PowerShell 源头关闭 progress, Windows 保留 UTF-8 初始化; exit0+stderr 分类改 shell_stderr, 仍保留 ok:false 和诊断. 新增 Windows 条件集成用例, 本机未运行. canonical skill 修正 CDP 描述冲突并补能力/会话/超时/cleanup 纪律.

验证进行中: 相关 shell/task/flow nextest 已启动. 载体断言守卫和记账守卫通过; 全仓 fmt --check 失败, HEAD 与当前 control_actions.rs 均仅同两处旧格式差异, tests.rs 均零格式差异. 不扩大为全仓格式整理. 本轮未操作共享 Windows/daemon/GUI, 未新增依赖或环境变量.

范围校准: 关闭本轮确定的输出契约缺口, 未引入辅助工程. 队列诊断/后台子孙取消/截图矩阵属于下一阶段, 未冒充实现. 重大风险与既有目标归属/超时未知契约一致, 无需另建 EPIPHANY.

## [2026-10-23 20:58:57] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 必要回归结果

命令: `cargo +1.95.0 nextest run --package rustdog --bin rdog -E 'test(control_actions::shell_lane_tests::) | test(control_actions::tests::shell_) | test(control_actions::tests::execute_control_command_should_run_script_via_shell) | test(control_flow::tests::shell_lane_) | test(task_control::tests::) | test(control_core::tests::control_line_should_execute_literal_shell_line)' --test-threads 1 --no-fail-fast`.

实际结果: test profile 编译成功 (7m24s), run 2cb5af31-a25d-4fcb-aa2f-e203e3947bf6, 35/35 通过 (11.326s), 1037 未选中. 新增两项红灯回归转绿. 没有全仓/Windows 动态通过声明. 源文件变更后的额外 cargo check 正在检查依赖.

补充交叉检索: 同一 577 文件新增宽关键词搜索, 21 条候选, 保存 7 条编码/连接相关原文, 其余为业务控制面/数字误命中等. CP936/CLIXML 有重复环境反馈, 但 SSH 侧错误不归为 rdog 自身缺陷.

范围校准: 本轮选定实现与相关回归已完成; 后续仅收口编译和文档守卫, 不继续扩展能力建设.

## [2026-10-23 21:02:14] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 最终校准

已完成本轮检索和 8 类反馈对账, 选定修复通过 35 项回归. 必要编译由 nextest test profile 成功提供. 额外 cargo check 因重新检查全依赖而主动终止, exit143, 不称通过; 精确核对 PID/cwd 后停止本轮 check 及唯一子进程, 未动其他 cargo/服务. 本轮没有新增编译警告或测试失败.

载体断言、记账、git diff --check 通过. 全仓 fmt --check 为未通过, 已确定受影响源文件的两处格式差异与 HEAD 相同; 新测试格式零差异. Windows 真机/Windows 条件测试和全仓测试未运行, 不称全部平台通过.

延期项已写 LATER_PLANS__yino_rdog_feedback.md. 无新的架构灾难结论需要 EPIPHANY; 原风险已由规格和延期项承接. 用户数据/共享 daemon/应用现场/网络配置均未更改. 当前改动未提交或推送.

## [2026-10-23 21:41:02] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 续接红灯与隔离现场

旧实现 nextest run6921fe9a-31cb-4ffa-968a-ab4252651f86: 两项失败. ping 缺 control.active_requests; 后台取消后本测试子PID98559仍活, 测试已精确清理, 同名无关进程保护断言保留. 静态仅child.kill与动态残留相符, 确认受管子树取消缺陷. 闪窗现象由用户提供, 静态发现task/flow/裸shell/taskkill缺隐藏设置, 动态Windows仍待验证.

只读Windows基线: rais-razer.local, rdog PID9996, Session1, C:/Users/vdcoo/yino-test/rdog.exe, 5555监听. 不修改/重启该实例. 后续只运行本任务Windows test binary及唯一临时目录/进程, 不启动新的共享daemon或应用.

按用户新授权委派GPT6 Luna: windows_process拥有进程辅助/task取消/相关测试; diagnostics_docs拥有现有规格与skill同步. 主Agent负责队列遥测接线、集成测试与最终验收.

队列设计: 复用Zenoh默认FIFO, 真实callback入站记等待, worker出队解除; 统一执行入口记录active, Ping自身不计. 仅查询/连接建立走querycallback快速路径, 业务仍FIFO. TCP/WebSocket内核缓冲、网络中未到达请求及PTY输入不冒充已观测队列, 不建设新调度器. Windows交叉测试构建使用--no-default-features排除与本次无关OCR引擎, 该配置证据只覆盖控制面.

## [2026-10-23 22:01:58] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] Windows 标志对照与审查修正

隔离SSH进程的ProcessStartInfo对照 (无共享daemon调用): CreateNoWindow=false时自有子PID64792的GetConsoleWindow=72220848; true时自有子PID67220返回0. 两者exit0/stderr空且已退出. 这是平台标志的动态证据, 不是新rdog二进制路径的验收.

Luna队列只读审查发现并已修正: 局部格式处理漏掉mod control_diagnostics声明,已恢复; bridge过滤原始PTY stdin帧; session-open创建期间持身份集合锁,失败撤销占位,防提前ACK; 测试添加预注册cancel token及Drop signal/join, readiness改成本测试唯一marker而非全局active计数,防并行测试串扰. 保留callback同步初始化的实际边界, 由动态测试验证不把静态检查当通过.

Windows进程代码已落地,正在复核测试的cfg门和清理身份. 新版本未替换共享daemon, Windows交叉构建仍在依赖阶段. 范围未扩展, 没有新增凭据、Job框架或调度器.

## [2026-10-23 22:46:25] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] Windows 编译阻塞分类与修正

Windows no-default-features 首次测试编译在 src/control_ocr.rs:567 失败: 既有 feature-off 用例 unwrap_err 额外要求成功值 Vec<RawOcrRegion>:Debug. 本轮选择的控制面隔离构建配置暴露了该既有测试缺口. 已仅将错误值提取改为 err().expect(...), 保持 OCR_ENGINE_UNAVAILABLE 原断言和 OCR 产品逻辑不变, 属必要验证的局部解阻. 默认 feature 开启的 macOS 构建不会编译该测试分支, 控制面源码未变化.

编译同时报告 8 条 Windows unused import/variable 警告, 位于原有平台分支 (control_actions/open_app, control_ax, listener, pty_control, target_resolve, zenoh_runtime/session). 对应导入/分支未由本轮修改, 记录为既有告警, 不扩展为跨平台清理. 尚不宣称新 Windows 测试通过.

Windows 重编译 session55549 已启动, 依赖已缓存; macOS 回归 session9104 已进入源码编译.

## [2026-10-23 22:58:28] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 本地39/40与关闭计数修复

Native runbb1f881f-6ccf-4064-8cbc-b0b2ece11a56: 39 passed / 1 failed. 已通过: 进程树取消且无关进程存活, 取消失败可重试, 取消等待不持全局registry锁, 繁忙query worker时独立Ping和排队请求保留. 失败: 接收端drop后queued仍为1.

上一静态假设不成立: flume sender仍持有缓冲项, receiver单独drop不立即析构Queued守卫. 已实现ChannelReceiver: Drop先关闭alive标记, 再在同一REQUESTS锁下按channel id清除queued; 注册在该锁内检查alive,发送仍在锁外走原默认FIFO. 不随断连清除已经执行的active请求,避免把仍在发生的副作用藏掉. 原断言保留,补关闭后发送不新增计数. 该问题由本轮引入,必须修复验证,没有放宽测试.

已停止已知旧实现的Windows编译(精确本任务PID/cwd,exit143),依赖缓存保留. 当前Native重跑session45765, Windows新构建另排队. Windows还额外验证真正observer session-open/bridge-ping/close路径,该补充仅cfg(windows)测试代码.

## [2026-10-23 23:24:29] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 续接最终验收与现场保护

- Native 最新相关 nextest 40/40 通过, 7.739s, 1037 未选中. 编译覆盖当前源; 队列关闭失败用例转绿, 保留原断言.
- Windows 交叉测试构建: `cargo +1.95.0 xwin test --package rustdog --bin rdog --no-default-features --no-run --target x86_64-pc-windows-msvc --jobs 1`, exit0. PE x64 44,542,464B, SHA256 `8a82341f202e7a5430dbdc5142dd9056825b38cd696119280941ae9f980435fb`, 远端逐字节哈希一致且真实载入成功.
- Windows 11项控制面用例全部通过: builder/task/flow三处GetConsoleWindow=0, progress抑制+中文UTF8+真实错误保留, stderr分类, 目标listener随取消关闭且无关listener存活, 取消错误可重试, 取消等待不阻塞registry查询, 队列关闭清理, 繁忙时legacy Ping和真实observer session-open→bridge Ping→close. 每个远端用例核对inventory且确认running1test/1passed, 防0测试假绿.
- 额外feature-off用例首次50s外部限先于READY_TIMEOUT到期, 属harness限额错误; 改外部限为75s覆盖源码既有60s预算, 原断言未放宽. 第二次程序60.01s/SSH61.658s后通过. 这不代表OCR识别通过. feature-off worker不发布ENGINE_READY的独立延迟已去重移交 #127, 未扩大修复.
- Windows测试exe进程0, owner nonce核实后删除独占目录, removed=true. 共享PID9996/Session1/C:/Users/vdcoo/yino-test/rdog.exe与5555监听前后一致, `jd`保护字段diff为空/exit0. 未重启、替换、改配置或操作共享应用. 161份源码/清单指纹核对未变.

原始日志/清理回执/指纹: `/var/folders/58/3f9_69ts3bx4slnb8tgl572m0000gn/T/rdog-control-verify-gf1jl80g/`. 该目录保留本轮证据, Windows远端临时目录已清理.

限制: Windows构建仍有33条既有平台/feature-off unused告警; 宿主debug map后处理报错但cargo返回0、PE有效且真机12项定向运行通过, 调试符号未验收、具体原因未定位. 未跑完整测试集/完整OCR模型推理/真实可见GUI与PTY; 全仓旧格式差异不宣称通过. 未使用Job Object, 父shell自然退出后已脱离的后代不在取消保证内. 未提交/推送或替换正式daemon, 当前共享实例需后续维护更新才会获得新行为.

范围校准: 用户要求的队列诊断、受管子树取消、默认隐藏控制台及必要隔离验证已完成. 新增OCR调查已停止于记录与开票, 没有成为本次前置或扩展修复. 无新的架构灾难结论需另建EPIPHANY, 已有风险由规格/延期票承接.

## [2026-10-23 23:27:17] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 最终守卫及issue回执

载体断言、记账、git diff --check最终均通过. 证据manifest断言确认Native40/40、Windows12/12、远端清理、共享daemon保护jd和161源码指纹一致. OCR延期issue已成功创建并回读确认: https://github.com/raiscui/rustdog/issues/127 (OPEN); 已确认origin与PUBLIC仓库raiscui/rustdog一致,发布正文不含内部主机/路径或凭据. 不因开票标为修复.
