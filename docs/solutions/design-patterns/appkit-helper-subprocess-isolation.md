---
title: "AppKit helper UI 的子进程隔离 (daemon 内非主线程 NSPanel 会 abort 整个进程)"
date: 2026-09-02
last_updated: 2026-09-02
module: daemon
component: overlay
problem_type: architecture_pattern
severity: high
status: active
tags:
  - macos
  - appkit
  - subprocess-isolation
  - overlay
  - main-thread
verified_by:
  - "2026-09-02 rustdog daemon 日志: `fatal runtime error: Rust cannot catch foreign exceptions, aborting` (非主线程 NSPanel 触发 ObjC 异常)"
  - "子进程架构修复后实证: 点击+截图流程中 daemon 存活, overlay 子进程独立绘制 (rustdog #102)"
---

# AppKit helper UI 的子进程隔离 (daemon 内非主线程 NSPanel 会 abort 整个进程)

## Context

Rust daemon (主线程是 tokio runtime) 需要在屏幕上绘制可视化辅助 UI
(如自动化动作的可视化反馈层)。AppKit 严格要求 UI 操作在**进程主线程**,
而 daemon 的主线程已被 runtime 占用。在辅助线程直接创建 NSPanel/NSWindow:
AppKit 抛出 Objective-C 异常 (NSInternalInconsistencyException 类),
Rust 的 panic 机制**无法捕获 C++ 异常**,进程直接:

```text
fatal runtime error: Rust cannot catch foreign exceptions, aborting
```

整个 daemon (包括所有与此 UI 无关的控制通道) 一起死亡。
实测: rustdog overlay 可视化层首版, 点击动作触发 flash 面板创建即 abort
(rustdog #102, 2026-09-02)。

即使用 `MainThreadMarker::new_unchecked()` 在辅助线程"假装"主线程,
也只是把 abort 换成随机时点的同类异常, 不是修复。

## Guidance

**AppKit helper UI 拆为同二进制的子进程**, 进程边界隔离一切 AppKit 崩溃:

1. 子命令形态: daemon 二进制自带 `overlay` 子命令 (或独立 helper bin),
   子进程的 `main()` 天然运行在它自己的主线程, AppKit 使用完全合法。
2. 事件通道: daemon 持有子进程的 stdin 管道, 按行写 JSON 事件
   (`{"color":"green","ttl_ms":1200,"rects":[[x,y,w,h],...]}`)。
3. 生命周期: 子进程读到 stdin EOF (daemon 退出/被杀) 即自动退出,
   无需显式管理; daemon 侧写失败 (子进程已死) 则静默重生或放弃,
   可视化是尽力而为的旁路功能, 不影响控制主流程。
4. **主线程事件循环**: 子进程主线程需要驱动 AppKit 事务提交。
   不跑 NSApp run() 时, 用"手动 runloop"模式: 每轮消化事件队列后调用
   `NSRunLoop::mainRunLoop().runMode_beforeDate(NSDefaultRunLoopMode,
   now+30ms)`。**没有这一步, 窗口只会在 AppKit 内注册而不会提交到窗口
   服务器**: CGWindowList 可见但 bounds 0x0、onscreen=false、屏幕不可见
   (实测, objc2-app-kit 0.3.2)。
5. 线程边界内的合法用法: 子进程主线程 `MainThreadMarker::new()` 正常
   返回 Some (无需 unchecked); NSPanel 创建用 `mtm.alloc::<T>()`
   (objc2 0.6, alloc 是 MainThreadMarker 的泛型方法, 不是类静态方法)。

## Evidence

- abort 实证: rustdog daemon 日志 `fatal runtime error: Rust cannot catch
  foreign exceptions, aborting`, 时点与 flash 面板创建精确吻合
  (rustdog #102, 2026-09-02)。
- 修复后实证: 同一 daemon 进程内多次点击 + 截图, 子进程独立存活绘制,
  daemon 无异常 (rustdog #102 comment, 2026-09-02)。
- 注册 vs 可见分离的实证: 修复主 runloop 缺失前, CGWindowList 显示面板
  bounds 0x0/onscreen=false; 补 `runMode_beforeDate` 后注册为正确
  bounds/layer 3 (可见性仍有遗留问题时, 至少注册路径已正确)。

## Why This Matters

这类 abort 杀死的是**整个 daemon**——所有无关的控制通道、在途任务、
其他会话一起陪葬。且触发点 (可视化旁路功能) 与损害范围 (控制主流程)
完全不成比例。Rust 侧 `catch_unwind` 无法防御 (foreign exception 不走
Rust unwinding), 唯一防御是进程隔离。

## When to Apply

- 长驻 Rust 服务进程需要任何 AppKit UI (overlay、通知面板、托盘、
  截图预览)。
- 任何"后台线程创建 NSWindow/NSView/NSPanel"的设计评审。
- macOS 上多线程 GUI 相关的 crash 排查 (症状: foreign exceptions abort,
  无 Rust 崩溃报告)。

## When Not to Apply

- 进程本身就是 GUI 应用 (主线程跑 NSApp run 是标准形态, 不涉及隔离)。
- 跨平台 GUI 框架 (egui/winit/tauri) 自行管理线程模型的场景。
- Linux X11/Wayland (X11 无此主线程强约束, 但 Wayland 有各自的合成器约束)。

## Examples

rustdog `src/control_overlay.rs`: daemon 侧 `submit()` 写 JSON 行;
`Command::Overlay` 子命令入口 `run_overlay_main()` 在子进程主线程
创建 NSPanel 并以 `runMode_beforeDate` 驱动手动 runloop。
完整设计与 abort 实证见 rustdog #102 comment (2026-09-02)。

## Related

- rustdog `tests/control_ocr_e2e.rs` (VIA_TERMINAL 模式: TCC 权限归属链
  的 Terminal 承载方案, 与本方案的子进程边界正交)
- spec: `specs/rdog-ocr-content-layer-plan.md` (overlay 所在的 OCR 内容层)
