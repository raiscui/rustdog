---
title: Windows 录制自注入 marker 位宽实测
date: 2026-11-05
last_updated: 2026-11-05
module: control_recording
component: windows-hooks
problem_type: logic_error
severity: high
status: active
tags:
  - windows
  - recording
  - input-injection
  - read-debugging
  - read-change
verified_by:
  - Windows 11 授权交互桌面上的前后对照
  - 本机定向 nextest 与 Windows 交叉编译
root_cause: 本次环境中 hook 观察到的 extra info 丢失了原 marker 的高 32 位, 与完整值比较不相等
resolution_type: 用 u32 唯一定义 marker 并在 API 边界加宽
---

# Windows 录制自注入 marker 位宽实测

## Problem

Windows 录制应过滤 rdog 自己注入的鼠标事件, 同时保留其他来源的输入. 在本次 Windows 11 环境中, 原候选把自身的 move, down 和 up 写入了收到的 Journal.

## Symptoms

- 原候选通过任务独占 daemon 点击受控 fixture. 实际客户端收到的失败样本含该点击的鼠标事件, 文件入口见下方 Verification.
- 独立低级鼠标 hook 在该环境中读到原 marker 的低 32 位, 未观察到高 32 位. 探针的指针宽度为 8 字节, `MSLLHOOKSTRUCT` 对应结构为 32 字节, extra 字段偏移为 24 字节.
- `injected=true` 不证明输入来自 rdog. 外部 `SendInput` 也可以有该标志.

## What Didn't Work

原实现用超过 32 位的 marker, 并在 hook 中与实际读取值做完整相等比较. 静态单元测试只比较内存中的同一个常量, 没有覆盖实际 Windows 输入通路中的值变化.

## Verified Root Cause

`src/control_mouse/win_backend.rs` 和 `src/control_actions.rs` 在输入 API 边界写入 `src/control_recording/mod.rs` 的 `SELF_EVENT_MARKER`. `src/control_recording/windows/hooks.rs` 用完整值判断是否过滤. 本次主机的独立 hook 观察到高位丢失, 因而原值无法相等. 证据不能确定高位在哪个系统组件丢失, 也不能推断所有 Windows 版本或其他 API 的行为.

## Solution

`SELF_EVENT_MARKER` 在 `src/control_recording/mod.rs` 唯一定义为 `u32`, 各 API 边界沿用已有的 `usize` 加宽. hook 继续比较完整值, 不把任意高位不同的输入当作自身事件. marker 是过滤线索, 不是认证凭据.

## Why This Works

修复版在该主机的独立 hook 上读回了完整 marker. 同一受控操作的 fixture 见证了候选点击, 但修复版 Journal 没有该点击. 区分位置的外部零 marker `SendInput` 点击, 拖动中的 move, up 和 wheel 仍被记录.

## Verification

- 原始 before/after Journal, fixture witness, 完整 target argv 和独立审计保存在 `local://windows-recording-razer-live-20261105/`. 入口为 `report.json`, 细节见 `task_plan__windows_recording_impl.md` 的 2026-11-05 真机记录.
- 同目录 `marker-probe.ps1` 与 `external-input-fixed.ps1` 是本次真机探针和回归驱动. 它们绑定本次临时目录和窗口身份, 重用前须在明确授权的独占桌面重新建立目标及身份证据. 不直接对共享桌面运行.
- `cargo nextest run --no-default-features --bin rdog -E 'test(control_recording::) | test(control_mouse::win_backend::)' --no-fail-fast --test-threads 1` 本轮 49 passed.
- `cargo xwin build --target x86_64-pc-windows-msvc --no-default-features --bin rdog` 和 `cargo xwin test --target x86_64-pc-windows-msvc --no-default-features --bin rdog --no-run` 均 exit 0. 辅助 debug-map 解析诊断仍存在, 不作为干净调试产物的证明.

## Prevention

修改发送端, hook 或 marker 位宽时, 在授权的真实 Windows 交互环境比较 hook 实际 extra info, fixture 接收和客户端 Journal. 分开测试候选自身注入与外部零 marker 输入. 不用纯常量比较, `injected` 标志或交叉编译代替该对照.

本次没有验证键盘自注入, 其他 Windows 环境或完整录制矩阵. 这些未覆盖项不属于本结论.

## Related

- `src/control_recording/mod.rs` 是 marker 的唯一数值定义.
- `specs/rdog-windows-recording-plan.md` 定义录制范围和真机验收矩阵.
- `task_plan__windows_recording_impl.md` 保留原失败, 修复版和资源清理证据.
