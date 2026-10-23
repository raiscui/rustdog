# yino 使用反馈驱动的 rdog 优化方案

## 范围与证据

2026-10-23 检索 yino 的 canonical checkout
`/Users/cuiluming/local_doc/l_dev/my/video/pireel`: 根级六文件及同后缀上下文,
以及 `archive/` 全部 Markdown. 共扫描 577 份文件, 30 份命中,
65 行直接提到 rdog/rustdog/rustcat. 原始命中路径、行号及摘录见
`notes__yino_rdog_feedback.md`. 这些行数是检索结果, 不是独立缺陷数量.

archive 的非 Markdown 文件不作为本轮上下文正文; 未扫描所有平行 worktree,
未读取业务凭据, 未操作共享远端 daemon 或应用. 最终 owner/隔离目录清理检查及共享 daemon
保护字段前后对账已完成, 公开文档不记录内部 PID 或主机路径. 下列来源均相对于上述 yino 根目录.
历史回执是现场记录, 不替代当前版本复现. 当前源码与 GitHub issue 已只读对账.

## 需求与缺陷对账

| 需求/现象 | yino 来源 | 判断与方案 | 最后复核 |
| --- | --- | --- | --- |
| 启动/关闭已发生, 但回执 `ok:false`, CLIXML 大量输出 | `archive/branch_contexts/cat_native_size_fix/WORKLOG__cat_native_size_fix.md:251,355`; `archive/branch_contexts/arms_runtime_error_2/WORKLOG__arms_runtime_error_2.md:136` | 高频执行与回执分歧. 当前 `execute_script_with_timeout` 在 exit=0、stderr 非空时仍标 `shell_exit_nonzero`. 本轮修正错误分类, 在 PowerShell 输出源抑制 progress; 不丢弃 stderr, 不把 exit=0 直接当业务成功. 原 Windows CLIXML 现场尚未重跑. | 2026-10-23 |
| 后台输出需要稳定 UTF-8 | `WORKLOG__x1_lane_conflict.md:16`; `archive/branch_contexts/arms_runtime_error_2/task_plan__arms_runtime_error_2.md:324`; 同上 CLIXML 记录 | Windows 11 PowerShell 对照通过: progress 输出被抑制, 中文 UTF-8 与真实 `Write-Error` 保留, stderr 分类正确. `build_shell_command` 覆盖同步、task、flow、裸 shell; `@run` 仍直接执行, 不改写 args. | 2026-10-23 |
| 新 client/文档面对旧 daemon, `@run` 不被识别 | `archive/branch_contexts/dev_update_acceptance/task_plan__dev_update_acceptance.md:35`; `archive/branch_contexts/cat_native_size_fix/WORKLOG__cat_native_size_fix.md:251` | 部署能力不一致, 不是当前 parser 缺少 Run. 当前已有 `ControlCommand::Run` 和 `execute_run`. 本轮补使用指引: 能力只以目标回执为准; 不为用新命令自动更新/重启共享 daemon. 后续增强精确 verb 能力声明. | 2026-10-23 |
| 客户端超时但应用已启动, 重发可能产生重复实例 | `archive/branch_contexts/cat_native_size_fix/WORKLOG__cat_native_size_fix.md:289`; `archive/branch_contexts/dev_update_acceptance/task_plan__dev_update_acceptance.md:35` | 超时只说明调用结果未取得, 不证明未执行. 操作指引要求先核对路径/profile/PID/会话/端口. 新增的 `@ping` ingress snapshot 只能提供并发忙闲线索, 不承诺 exactly-once; 超时后仍须核验业务状态再决定是否重试. | 2026-10-23 |
| 截图是旧桌面或别的工作区 | `ERRORFIX.md:1310`; `WORKLOG__oneshot_impl.md:5`; `notes__oneshot_impl.md:9` | 至少包含目标归属不匹配; 单凭这些记录不能全部归因为缓存. #103 已关闭, 但关闭不证明上述现场已被修复. 本轮补证据边界; 后续独立做切屏/多窗口/锁屏与新帧验证. | 2026-10-23 |
| SSH Session0 看不到交互桌面窗口, 原生关闭/保存框操作失败 | `archive/branch_contexts/dev_update_acceptance/task_plan__dev_update_acceptance.md:31,35`; `archive/branch_contexts/download_receipt/WORKLOG__download_receipt.md:21` | 交互会话身份是必要前置. 记录已有成功替代: 交互桌面 daemon + fresh 窗口身份. 本轮完善指引, 不跨会话启动新服务绕过权限. | 2026-10-23 |
| 后台任务取消后 Vite 残留 | `WORKLOG__x1_lane_conflict.md:45` | `src/task_control.rs::cancel_task` 走进程树终止与根进程收割: Unix `@spawn` 隔离进程组, Windows 检查 `taskkill /F /T /PID`. Windows 11 隔离用例确认目标 listener 退出、无关 listener 仍活; 取消失败保留可重试状态且不持全局 registry 锁. 已脱离父 shell 的后代仍不保证覆盖, 端口核查仍是调用方责任. Native 相关 nextest 40/40 通过. | 2026-10-23 |
| Windows 非交互命令短暂闪出终端窗口 | 本轮需求 | `src/control_actions.rs::build_shell_command` 对 builder 创建的 Windows shell 设置 `CREATE_NO_WINDOW`. Windows 11 新 rdog test-exe 实测 builder/task/flow 三条路径 `GetConsoleWindow=0`; `@run` window 选项与 PTY 语义保留, 不隐藏命令明确启动的 GUI app. OCR 不在该控制面验收范围内. | 2026-10-23 |
| daemon 忙闲与 ingress 排队诊断 | GitHub issue #121 | `@ping` 保留既有字段并新增 `control` 快照, `queue_scope` 固定为 `zenoh-ingress`. Native 首轮 39/40 暴露 receiver close 后 queued 未清的问题; 修复后最新相关 nextest 40/40 (7.739s), 原断言保留. Windows 11 control-plane 11/11 通过, 连同独立 feature-off 用例共 12/12. 真实 SDK busy 用例中的 legacy Ping 与 observer session-open/bridgePing/close、queue close 清理均通过. 字段范围见 `specs/control-line-protocol.md`. | 2026-10-23 |
| Zenoh 名称发现失败, 但明确 TCP 或 SSH 可达 | `WORKLOG__alpha12_export_arms.md:20`; `archive/branch_contexts/cat_native_size_fix/WORKLOG__cat_native_size_fix.md:251` | 区分发现失败、旧地址与主机失联. 本轮指引允许使用已确认的传输入口, 不自动改网络/安全配置. 跨网需求继续由 #106 承接. | 2026-10-23 |

单引号消失的具体层级仍未确定, 复用 #122, 不新增猜测式 parser 修复.
HiDPI 坐标文档、同步超时、Windows window-find/activate/close、直接执行 `@run`
已由现有实现承接, 不根据旧存档重复建设. 混合 DPI 保留 #123;
Windows resize 仍需确定 per-monitor DPI 和外框尺寸语义.

## 本轮实现方案

1. 复用 `src/control_actions.rs::build_shell_command` 作为 shell 初始化唯一入口.
   PowerShell 设置 `$ProgressPreference='SilentlyContinue'`, Windows 继续使用既有
   UTF-8 初始化; cmd 保留既有 codepage 初始化; 其他 shell 文本不变.
   删除同步入口的独占前缀, 从而覆盖 `@cmd`、task、flow 和裸 shell 的现有调用方.
   用户脚本仍可显式重设 ProgressPreference. 不改 ErrorActionPreference,
   不按字符串删除 CLIXML, 不静默过滤真实错误.
2. `build_shell_failure_response_json` 按退出码区分 `shell_stderr` 与
   `shell_exit_nonzero`. 二者仍为 `ok:false`, 保留 stdout/stderr/shell/exit_code.
   此处报告的是命令执行证据, 不是应用生命周期或保存结果的最终判定.
3. 修正 canonical skill 的 CDP 描述冲突, 增补旧 daemon 能力核验、超时后查证、
   Windows 会话归属、PowerShell 输出和后台取消后的进程核查. 不创建第二份操作手册.
4. `@ping` 附加 `src/control_diagnostics.rs::snapshot` 的 scoped ingress 计数;
   独立 Ping 与 session-open callback 分支绕过业务 FIFO. session-open bridge 初始化仍同步等待,
   不承诺其时延,并发时其他连接的可用性由隔离回归确认. same-session FIFO 不变,
   不观测网络/内核缓冲或 PTY 原始 stdin 数据帧, `@key` 等协议控制请求仍可计数;
   不保存命令正文, 也不增加 reset/restart/scheduler.
5. `@spawn` 在 Unix 隔离进程组; `@task-cancel` 在 registry 锁外终止并等待收割.
   Windows 用经检查的 `taskkill /F /T /PID`;失败不写 `canceled`, 保留任务供重试.
   已脱离父 shell 的子孙进程仍在保证范围外, 不引入 Job Object.
6. Windows 的 `build_shell_command` 默认隐藏创建的非交互 shell, taskkill helper 同样隐藏.
   `@run` 保留自己的 window 选项, PTY 保留交互终端; 不隐藏命令明确启动的 GUI app.

依据 Microsoft 的 [ProgressPreference 契约](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_preference_variables?view=powershell-5.1),
SilentlyContinue 仅停止显示进度, 命令继续执行.
[Progress stream 不支持常规重定向](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_redirection?view=powershell-7.4),
所以优先控制输出源, 不建立 XML 过滤器或把 stderr 判定整体放宽.

影响面: `control_actions` 的脚本执行与 builder, `control_core`/`shell`/`task_control`/
`control_flow/process` 既有 builder 调用方, 单测与操作载体. 不新增依赖、环境变量、
配置、协议 verb 或数据迁移. `@run` 及交互 PTY 不在该 builder 契约内.

## 后续执行顺序

1. P1: screenshot freshness/owner 矩阵, 复核 #103 与 #123 边界.
   用已知窗口内容切换和 manifest 归属证明图属于目标, 不以文件 mtime 代替新帧.
2. P2: Windows resize、短等待转后台及跨网能力分别沿现有规格/issue 推进.
   短等待不能把已经解除的控制面阻塞重新引入, 跨网不成为本轮前置.

## 验收与限制

- Native 相关回归和 Windows 11 指定控制面矩阵已完成; 具体证据与限界列在下方.
- Windows test-exe 使用 `--no-default-features`, 不包含 OCR 能力; 不把控制面通过扩写为 OCR 识别通过.
- OCR feature-off 用例的 75s 限时重跑已通过,但不代表 OCR 识别验收; `ENGINE_READY` worker 未发布导致的 60s 等待作为独立延期.
- Windows 调试符号后处理未通过, 原因未定位; 33 条既有 Windows/feature-off unused 告警仍在.
- 共享 daemon 本轮未改动; 最终保护字段前后对账通过, 不在公开文档记录内部 PID.

## 本轮实际结果 (2026-10-23)

- 新增回归先红后绿; nextest run `2cb5af31-a25d-4fcb-aa2f-e203e3947bf6`
  为 35 passed, 0 failed, 1037 未选中, 测试目标已编译成功.
- 载体断言、记账与 diff whitespace 检查通过. 全仓 fmt 检查存在既有差异,
  受影响源码与 HEAD 的格式差异相同, 本轮新测试无格式差异.
- 额外 `cargo check` 的全依赖重查已主动停止 (exit143), 不计通过.
  截至该早期记录时 Windows 条件集成用例、Windows 真机及完整测试集未运行; 后续收口见下.
- 后续事项已落 `LATER_PLANS__yino_rdog_feedback.md`, 不因本轮收口标为完成.

后续控制面收口:

- Native 首轮 39/40 暴露 receiver close 后 queued 计数残留. `ChannelReceiver` 修复后,
  原失败断言及 close 后发送断言保留并通过; 最新相关 nextest 为 40/40 passed, 7.739s.
- Windows 交叉构建返回 0, 生成 PE x64 test-exe (44,542,464 bytes); 本地与远端
  SHA256 均为 `8a82341f202e7a5430dbdc5142dd9056825b38cd696119280941ae9f980435fb`.
- Windows 11 控制面 11/11 通过; 连同独立 feature-off 用例共 12/12. 覆盖 PowerShell progress/中文 UTF-8/真实 error 保留与 stderr 分类,
  builder/task/flow 三路 `GetConsoleWindow=0`, 任务树取消与无关 listener 存活、取消失败重试、
  无全局 registry 锁、queue close 清理, 以及真实 SDK busy 时 legacy Ping 与
  observer session-open/bridgePing/close. 这些结果仅覆盖控制面矩阵.
- Cargo 交叉构建状态码为 0 且 PE 可运行, 但 debug-map 后处理报告
  `cannot parse debug map ... file not recognized as valid object file`; 调试符号未通过.
  另有 33 条既有 Windows/feature-off unused 告警, 不宣称零 warning.
- OCR feature-off 原用例曾因 `unwrap_err()` 要求 `RawOcrRegion: Debug` 阻塞编译,
  已仅改为 `.err().expect(...)` 解阻. 75s 外部限时用例通过: libtest 60.01s,
  SSH 总时长 61.658s; 该用例不证明 OCR 识别能力. 现有 feature-off worker 未发布
  `ENGINE_READY`,因此等待既有 60s 超时;该行为单独延期,不改 OCR 行为.
- 最终清理核查确认本轮 test-exe 进程数为 0、隔离目录已删除;共享 daemon 保护字段
  `jd` 前后对比无差异,确认本轮未改变共享实例.
