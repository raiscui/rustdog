# yino 使用反馈驱动的 rdog 优化

## [2026-10-23 20:27:06] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 范围基线

- 用户目标: 检索 yino 上下文与 archive, 分析 rdog 使用需求和缺陷, 制定计划并优化调整.
- 本次交付: 可追溯反馈清单, 与当前实现/issue 对账, 按影响优先级实施可复现问题, 同步操作契约并验证.
- 范围外: 不修改 yino 业务代码, 不操作共享 Windows 应用/daemon, 不清理历史证据, 不展开跨网互联或 Windows AX 全量建设.
- 验收: 来源文件与行号, 问题状态及证据边界, 修改前后测试, Rust 编译及相关载体守卫.
- 停止条件: 检索范围覆盖, 分类与方案落盘, 本轮选定优化经验证, 剩余项明确移交且不冒充完成.
- 复评点: 首轮扫描 pireel 根上下文及 archive 全部 Markdown, 从真实使用反馈选择实施项, 不沿历史待办无限扩展.

## 阶段

- [x] 1. 检索上下文与 archive, 建立反馈证据清单.
- [x] 2. 对照当前实现与 issue, 收敛优先级和方案.
- [x] 3. 实施确定的优化, 补充回归验证.
- [x] 4. 编译/载体检查, 校准账本并交付.

## 当前状态

已定位 yino 为 `/Users/cuiluming/local_doc/l_dev/my/video/pireel`, 初扫 577 份 Markdown, 30 份包含 rdog/rustdog/rustcat. rustdog 初始工作树干净.

## [2026-10-23 15:30:00] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 方案与红灯证据

已完成 577 文件/30 命中文件/65 命中行的来源索引与 8 类反馈对账. 方案见 specs/yino-rdog-usage-improvement-plan.md. 新增 builder progress 测试和真实 sh exit0+stderr 测试均在旧实现失败, 后者实际得到 shell_exit_nonzero. 选定 shared builder 初始化和 stderr 分类两项, 未新增辅助工程. 当前进入实现与相关 shell/task/flow 回归. 全仓 fmt --check 报大量既有格式差异, 将对比 HEAD 并仅修本次引入的格式.

## [2026-10-23 20:58:57] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 相关回归通过

nextest run 2cb5af31-a25d-4fcb-aa2f-e203e3947bf6: 35 passed, 0 failed, 1037 未选中. 测试编译成功, shell 超时/孙进程清理、task 终态/合流、flow cwd/env/stderr/截断和新回归均通过. Windows 条件集成用例不在当前 macOS 运行范围, 保持未验证. cargo check 正在独立检查.

## [2026-10-23 21:02:14] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 本轮交付收口

检索/分析/方案/选定调整与本机必要验证已完成. nextest 的 35 项通过同时提供 test profile 编译证据. 额外 cargo check 进入全依赖重查后主动停止 (exit143), 已核对 cwd 和进程链, 仅终止本轮 check PID12046 与子进程23336; 不计作通过. 载体断言、记账及 git diff --check 通过. 全仓 fmt 未通过的既有差异有 HEAD 对照, 本轮无新增格式差异. Windows 集成、完整测试集与共享远端升级均未执行.

后续项已实际落 LATER_PLANS__yino_rdog_feedback.md, 本轮不代替后续阶段完成声明. 未提交、未推送. 不自动归档本支线: 仍保留 Windows 验证缺口及后续阶段入口.

## [2026-10-23 21:19:32] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 用户续接: 队列诊断、后台取消与 Windows 闪窗

本次交付: 沿原方案完善实际入站队列/执行中诊断, 后台取消覆盖子进程树, 默认非交互 Windows 作业隐藏控制台, 保留显式 visible 与 PTY 行为. 前置取证/回归/编译及 Windows 隔离验证纳入当前范围.

保护边界: 不更新或重启共享 daemon, 不操作他人应用, 不全局按进程名清理. Windows 仅用本任务唯一目录/配置/端口及 PID; 可达性不足时继续本地和交叉编译并如实报告动态缺口.

- [x] 5. 复核真实队列与进程创建/取消路径, 固定语义和失败实验.
- [x] 6. 实施队列诊断和跨 lane 隐藏控制台, 完善后台子树取消.
- [x] 7. 定向回归及 Windows 隔离验证, 同步规格并收口.

已知: @cmd 独占 suppress_shell_window, task/flow/裸 shell 未覆盖, taskkill 子进程未隐藏. cancel_task 只 child.kill, 未建任务进程组/Job. 以上为代码证据, 待动态实验. 初始 dirty 均为上一轮同任务产出, 保留.

## [2026-10-23 22:27:57] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 源码冻结, 动态验收进行中

队列/进程树取消/默认隐窗已完成实现与静态审查; callback session-open并发与失败占位已修, 原始PTY stdin计数排除已接线. 取消失败保留child且不持全局registry锁,已补专用回归. 首次Windows交叉依赖构建仍进行; 本地nextest等待同目录artifact锁, 不重复开构建. 独占Windows测试目录已创建: C:/Users/vdcoo/AppData/Local/Temp/rdog-control-verify-6c8f753c-29a6-4db6-a752-767b7f9e5d7f, owner nonce一致; 尚未上传/运行rdog测试程序. 验收步骤7继续保持未完成.

## [2026-10-23 23:24:29] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 本轮续接完成

用户本轮三项交付及隔离验证完成: Native40/40, Windows11项控制面11/11,额外feature-off60.01s验证通过. 共享daemon PID9996/Session1/5555 jd前后无差异, 本轮进程/远端临时目录清理确认. 后处理debug-map错误/既有告警/完整OCR与全仓回归不冒称通过, 延期见LATER_PLANS与 #127. 不扩展到OCR修复、跨网/混合DPI或共享daemon升级. 未提交、未推送.

## [2026-10-23 23:40:49] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 用户授权: 提交、正式安装更新及target清理

用户明确授权提交当前改动, cargo install本机正式二进制并更新daemon, 编译Windows正式版本并更新其daemon, 完毕后清理target节省空间. 原来的禁止替换共享daemon边界在本次具体更新目标内被新授权替代; 不覆盖无关应用和其他服务, 切换前仍需检查在途请求/后台任务/PTY并排空, 不强行中断未完成任务.

- [ ] 8. 审核归属/敏感内容/子模块并提交当前已验收改动.
- [ ] 9. 备份本机安装、配置与签名, cargo install及稳定身份签名, 本机正式daemon健康验证.
- [ ] 10. 默认功能的Windows release构建, 校验/备份/排空/受控切换与正式回归.
- [ ] 11. 保存回退材料和验收记录, 核查target占用并清理, 记录回收空间.

当前本机未发现rdog进程/launchd条目, 需继续核实默认注册状态. 旧installed binary的DR仍钉cdhash, 使用稳定签名首次切换可能需要重新授予TCC, 不能声称旧授权必定保留. target约6.1G, 当前卷剩余14GiB, 清理按用户要求置于更新验收之后.
