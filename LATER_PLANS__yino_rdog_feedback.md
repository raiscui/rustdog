# yino 反馈后续事项

## [2026-10-23 20:58:57] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 分阶段验收缺口

- Windows PowerShell 动态回归: 已有 `control_actions::tests::powershell_progress_should_be_silent_but_real_errors_preserved`, 待 Windows CI/有明确归属的独占实例验证. 本机 pwsh 启动不可用; 不为验收重启共享 daemon.
- 能力声明和 busy/队列诊断: 沿 https://github.com/raiscui/rustdog/issues/121 跟进, 不以 ping 可达代替业务执行状态.
- 后台取消子孙残留: yino `WORKLOG__x1_lane_conflict.md:45` 的 Vite 残留需最小 Windows 子树复现, 查明实际归属与取消路径后再定修法; 原 task 测试通过不证明此现场解决.
- 截图旧帧/错误工作区: 不将所有历史截图问题归为缓存. #103 已关闭仍需对上述现场单独验证, 混合 DPI 沿 https://github.com/raiscui/rustdog/issues/123.
- 单引号层级沿 https://github.com/raiscui/rustdog/issues/122, 不做猜测式兼容改写. 其余 Windows resize/短等待/跨网方案见 specs/yino-rdog-usage-improvement-plan.md.

本轮只读核对远端 issues, 未开新票或修改其状态. 未复现候选与尚未开展的下一阶段均保持未完成.

## [2026-10-23 23:24:29] [Session ID: 01a168b3-33af-7c53-9c6b-0fb808d7e9dc] 本轮关闭与剩余边界

原条目中的Windows控制面动态验证已实际完成: 11/11, Native40/40. 队列诊断与受管子进程树取消已有有界实现和验证, 不再按完全未实现处理; #121远端仍open, 本轮没有擅自关闭其更宽的跨lane调度/诊断范围.

- 已移交 https://github.com/raiscui/rustdog/issues/127: feature-off OCR仍等满既有初始化预算再拒绝, 实测60.01s. 本轮仅修了该测试的编译取错方式, 未修引擎生命周期.
- Windows宿主debug-map后处理错误及33条既有告警保留; PE运行不受该次后处理错误阻断, 调试符号/告警清理并未完成.
- 共享daemon未替换. 后续应在不影响当前使用者的维护窗口构建/替换正式二进制并复核能力; 本轮产物是test-exe, 不能把它当正式daemon安装.
- 父shell退出后已脱离的子进程、混合DPI(#123)、单引号层级(#122)和完整截图矩阵仍未完成. 不因本轮控制面测试绿而销账.
