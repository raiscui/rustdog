---
title: macOS live GUI 自动化测试的环境门禁 (显示器休眠/权限归属/焦点吞没)
date: 2026-09-02
last_updated: 2026-09-02
module: control
component: gui-testing
problem_type: best_practice
severity: high
status: active
tags:
  - macos
  - live-e2e
  - screen-capture
  - display-sleep
  - window-occlusion
  - ocr
verified_by:
  - "2026-09-02 OCR live e2e 全链实证: 全屏截图亮度均值≈0 期间 sck/xcap 双超时+NSPanel onscreen=false, caffeinate 唤醒后恢复 (rustdog #102/#103)"
  - "计算器三件套闭环一次通过 (显示值 13) + 两次独立 \"7\" 点击精确命中 (screencapture 真值显示 77)"
  - "多组失败-恢复对照: README 旧帧/全黑屏 vs 唤醒后正常截图, 同机同二进制"
related_solutions:
  - docs/solutions/best-practices/macos-tcc-stable-codesign-identity.md
  - docs/solutions/design-patterns/appkit-helper-subprocess-isolation.md
---

# macOS live GUI 自动化测试的环境门禁 (显示器休眠/权限归属/焦点吞没)

## Context

live GUI e2e (真实截图 + 真实点击 + fresh 证据仲裁) 的失败模式与代码 bug 高度相似:
截图后端双双超时、窗口截图返回数分钟前的旧帧、屏幕上新建的窗口不可见、
点击后目标毫无反应。2026-09-02 的 OCR live e2e 调试 (rustdog #102/#103) 实证:
其中一整类"间歇失败"的根因是**显示器休眠/锁屏**,另一类是 **TCC 权限归属链**,
第三类是**窗口遮挡与焦点竞争**。三者都不是被测代码的 bug。

适用版本:macOS (display sleep / TCC / AppKit compositing 机制),任何依赖
screen-capture + 合成事件点击的 live GUI 测试形态。

## Guidance

按测试生命周期的三个阶段设门禁, 每条都有当日实证:

### 1. 测试前: 显示器与权限门禁 (缺一即 fail fast)

- **显示器休眠检查 (第一前置)**: 全屏 `screencapture -x` 后计算亮度均值,
  接近 0 即显示器休眠。休眠状态下: sck-rs/xcap 双后端 5s 超时、AppKit 新窗口
  `onscreen=false` 永不合成、screencapture 返回全黑——所有下游"失败"都是假的。
  唤醒: `caffeinate -u -t 5`; 保持: `caffeinate -dis -t <测试时长>` 后台运行。
- **锁屏检查**: 唤醒后截屏内容可能是锁屏壁纸 (需要用户解锁)。锁屏下窗口点击
  无法到达桌面会话。
- **权限归属链**: 由 agent shell / cargo test 直接 spawn 的进程, 其 Screen
  Recording 归属不继承用户终端的授权。两条路: 测试前置授权的二进制
  (DR 稳定身份, 见 related solution), 或由已授权宿主 (如 Terminal) 承载
  被测进程 (`open -a Terminal <.command>` 模式)。
- 环境不满足必须显式 fail (panic 带缺失项说明), 不得静默跳过。

### 2. 测试中: 截图与点击的防御

- **每次截图前重新 activate 目标窗口**: 被遮挡的窗口不会出现在
  screen-composited 截图里; OCR 层框数骤降 = 大概率截到遮挡物, 置前重拍。
- **断言只用同帧 manifest**: 跨截图轮次复用坐标是无效的——被测窗口位置会
  级联变化 (macOS 每次启动级联偏移), 且部分 app 重开后恢复上次 UI 状态
  (Calculator 重开恢复上次显示值, 残留数字会污染同名按钮定位)。
- **点击吞没重试**: 目标窗口非最前时首次点击被激活吞掉 (不产生输入)。
  点击前显式 activate + 点击后验证状态变化, 未变则整轮重试
  (每轮先恢复干净状态, 防止部分生效的残留污染对照)。
- **单帧识别抖动**: 小按钮的 OCR 单帧漏检/误读是常态 (实测个别按钮 conf
  低至 0.12 且文字错), 重拍 + conf 下限过滤 + 候选集合回退。

### 3. 断言仲裁: 拿"系统工具真值"对照

怀疑被测流程返回假数据时, 用系统级工具 (screencapture) 同刻截同一区域做
真值对照: 两者内容分叉即为复现证据。判定显示器状态用全屏截图亮度均值,
一分钟内可证伪/证实, 避免在代码里空转排查。

## Evidence

- 2026-09-02 OCR live e2e (rustdog #102/#103, 计算器场景):
  - 显示器休眠期: 全屏截图亮度均值 ≈0 (纯黑), sck-rs 与 xcap 双双 5s 超时,
    新建 NSPanel `onscreen=false` 不可见; `caffeinate -u` 唤醒后亮度恢复 141,
    同一二进制截图正常。
  - 权限归属: re-signed 二进制 (DR identifier=rdog) 从 agent shell spawn 仍
    Connection refused/截屏失败, 改 Terminal 承载后错误显式化为 code 77,
    授权 Terminal 后通过。
  - 点击精度: 两次独立 "7" 按钮点击均精确命中 (screencapture 真值显示 "77")。
  - 单帧抖动: "2" 按钮单帧误读为 "8" (conf 0.12, 真值 8 的 conf 1.00),
    重拍 + conf>=0.5 过滤 + 候选对回退后闭环通过。
- 复现入口: rustdog `tests/control_ocr_e2e.rs`
  (`RDOG_OCR_LIVE_E2E=1 [+ _VIA_TERMINAL=1] cargo test --release --test control_ocr_e2e`)。

## Why This Matters

三类环境因素的症状 (超时/旧帧/不合成/点击无效) 与真实代码 bug 几乎不可区分,
且随显示器状态在"通过/超时/旧帧/全黑"间漂移, 极易把排查引向错误方向
(当晚曾在"坐标偏移 45px"的错误假设上消耗多轮)。前置门禁把环境变量
从排查变量变成常量。

## When to Apply

- macOS 上任何依赖屏幕捕获、合成输入事件、窗口枚举的 live e2e / 真机冒烟。
- 评测 OCR/视觉模型在真实屏幕内容上的表现 (休眠屏会让召回悬崖式崩塌)。
- agent 驱动的无人值守 GUI 自动化 (无人唤醒显示器)。

## When Not to Apply

- 单元测试、模拟 backend 的测试 (无真实屏幕交互)。
- Linux X11/Wayland 与 Windows (机制不同, 需各自的环境门禁方案)。
- CI headless runner (本方案依赖真实 GUI 会话; headless 用 fake backend)。

## Related

- `docs/solutions/best-practices/macos-tcc-stable-codesign-identity.md`
  (TCC 身份稳定: 重签解决跨构建保留; 本文档的权限归属链是其补充维度)
- `docs/solutions/design-patterns/appkit-helper-subprocess-isolation.md`
  (daemon 内 AppKit helper 的子进程隔离; 本文档的 via-terminal 归属链
  与其 spawn 模式衔接)
- rustdog `tests/control_ocr_e2e.rs` (本门禁的可执行实现)
