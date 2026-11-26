---
title: "zenoh serial listener 的惰性通告 — 不开设备也会进可达 locator 集合"
date: 2026-11-26
last_updated: 2026-11-26
module: zenoh
component: daemon-startup / listen_endpoints / locator 通告
problem_type: convention
severity: medium
status: active
tags:
  - zenoh
  - serial
  - locator
  - daemon-startup
  - windows
verified_by:
  - "rais-razer (Windows 11) 实测 2026-11-26: serial/COM99 与 serial/NOSUCHPORT 都让 daemon 正常 ready, 日志写 `Zenoh can be reached at: serial/COM99`, 端口照常 LISTENING (RAI-383 评论)"
  - "Linux 云机对拍 (RAI-383, 隔离 HOME): 同一配置修前 `serial/NOSUCHPORT` 与 `serial//dev/…` 都出现在通告集合, 修后都消失并各留一条四要素 warn"
  - "socat pty 冒充可用串口 (Linux 云机): `serial//…/rai383-tty-a` 正常进入通告集合、无 warn"
  - "源码: zenoh-link-serial 1.8.0 src/unicast.rs:321-372 (new_listener 只登记 + to_locator), :409-437 (accept_read_task 才 ZSerial::new), :400-405 (get_locators 无条件回读)"
  - "源码: zenoh 1.8.0 src/net/runtime/orchestrator.rs:525-533 (绑定失败 warn + Err), :537-543 (print_locators 无条件打印)"
  - "源码: serialport 4.9.0 src/windows/com.rs:62 (COM 必须以 share_mode 0 打开; 默认共享模式会误判可用串口, RAI-383 评审修复)"
root_cause: "serial listener 是惰性的: zenoh-link-serial 的 new_listener 不打开设备, 只把 endpoint 记进 listeners 表并由 get_locators() 无条件回读; 打开失败发生在后台 accept 循环, 走不到启动期「绑定失败即返回 Err」的分支"
resolution_type: "通告前分类 (src/zenoh_runtime/listen_plan.rs): tcp/udp/tls/unixpipe 保持致命, serial 先探测设备, 不可用即从 locator 集合剔除并打四要素 warn; 模板默认注释掉 serial 端点"
---

# zenoh serial listener 的惰性通告 — 不开设备也会进可达 locator 集合

## Problem

`rdog daemon` 的 `[zenoh].listen_endpoints` 里带一条打不开的 `serial/...` 时,
daemon **正常 ready**, 日志照旧打印 `Zenoh can be reached at: serial/COM99`
(Windows 11 真机实测, RAI-383)。后果是双向的:

- 这条地址**进不了任何链路, 却被对外宣称可达** —— 对端按 locator 拨号必然失败;
- 启动日志里**没有任何「这个串口其实不存在」的信号**, 现场只能靠猜。

误导来自源码阅读的正确但不完整的推论: `orchestrator.rs:525-533` 确实是
「listener 绑定失败 → `warn!` + `return Err`」, 于是很容易得出「串口不存在就
exit(1)、此时不回应 scout」。实测否定了它 —— serial 根本不走这条路。

## 可核断言

1. **serial 的 `new_listener` 不打开设备**: `zenoh-link-serial` 1.8.0
   `src/unicast.rs:321-372` 只做三件事 —— 算 `path` / `baud_rate`, 起一个后台
   `accept_read_task`, `listeners.insert(path, listener)`, 返回
   `endpoint.to_locator()`。函数内没有任何设备打开动作。
2. **locator 集合无条件取自登记表**: 同文件 `:400-405` 的 `get_locators()` 直接
   把 `listeners` 里每个 endpoint 转 locator; `orchestrator.rs:537-543` 的
   `print_locators()` 把这份结果逐条 `info!("Zenoh can be reached at: {}", …)`。
   也就是说 **「可达」= 「登记过」, 与设备是否存在无关**。
3. **真正的打开发生在后台且失败会被重试吞掉**: 同文件 `:409-437` 的
   `accept_read_task` 里才 `ZSerial::new(...)`; 失败只是让循环 `sleep` 后重试,
   不冒泡到启动错误路径。
4. **因此 tcp/udp 与 serial 的失败语义不同**: tcp/udp 的 `add_listener` 会因
   bind 失败返回 `Err` (`orchestrator.rs:525-533`) → runtime 不启动 (致命);
   serial 永远 `Ok`, 只污染通告集合。判据看代码路径, 不看表象。

rdog 侧的落地: `src/zenoh_runtime/listen_plan.rs` 的 `plan_listen_endpoints()`
(`:36`) 在交给 zenoh 之前分类 —— 必选入口 (tcp/udp/tls/unixpipe) 原样保留,
serial 走 `probe_serial_device()` (`:67`) 探测; 被剔除的条目由
`src/zenoh_control.rs:196-205` 打四要素 warn 后继续启动。

## Guidance

1. **任何 transport 的「可到达」声明, 先查它的 `new_listener` 是否真的获取了资源**;
   拿到 locator 不等于拿到链路。
2. **可选 transport 的通告必须前置可用性探测**, 不能交给 zenoh 的惰性路径:
   实际生效集合与对外通告集合必须是同一份数据 (rdog 的做法是把 plan 结果同时用于
   `open_router_session_tls` 与 ready 日志)。
3. **探测按 transport 语义做, 不按表象做**: unix 用字符设备判定; windows 必须按
   zenoh 底下 `serialport` 的打开语义做 —— 读写 + `share_mode(0)`
   (`serialport-4.9.0/src/windows/com.rs:62`)。通信设备不接受默认的
   `FILE_SHARE_READ|WRITE|DELETE`, 用默认共享模式试开会把一个**本来可用**的串口判成
   不可用 (RAI-383 评审抓到的真实缺陷), 那比不探测更糟。
4. **降级要带四要素**: endpoint / 原因 / 影响 / 改法。只写「不可用」等于没写。
5. **模板默认不带可选 transport**: 用户显式启用才加, 避免「默认配置在多数机器上
   产生一条假地址或一条噪声 warn」。

## Evidence

- rais-razer (Windows 11): `serial/COM99`(不存在) 与 `serial/NOSUCHPORT`(无 baudrate)
  两次实测都正常 ready, 日志 `Zenoh can be reached at: serial/COM99`, 端口 LISTENING;
- Linux 云机 (RAI-383) 同配置对拍: 修前 `serial/NOSUCHPORT` +
  `serial//dev/rdog-rai383-no-such-port#baudrate=115200` **都**出现在通告集合且无 warn;
  修后两者都被剔除、各一条四要素 warn, `tcp` 入口 `@ping` 回执 `{"ok":true,"pong":true}`;
- 回归对照: `socat` 造的 pty (`/dev/pts/0`, 字符设备) 作为串口时, endpoint 正常进入
  通告集合、无 warn —— 证明探测不是「一律剔除 serial」;
- 原始日志: `captain-remote-p2p/handoffs/rai383/receipts/`。

## Why This Matters

这条契约被误读会同时产生三个错误方向: 把「假地址被通告」当成「对端网络问题」去调
发现逻辑 (RAI-368 的方向), 按 `orchestrator.rs` 的错误分支写「启动即退出」的兜底
(本票最初的提案), 或者加长 scouting timeout 掩盖 (只会让失败更慢)。正确修法在
**通告前**这一层, 不在错误分支里。

## When to Apply

- 改 `[zenoh].listen_endpoints`、daemon 启动策略、locator 通告或 ready 日志时;
- 排查「日志说有地址, 对端却连不上」的现场, 且该 transport 是 serial (或其他惰性
  transport) 时;
- 给 rdog 增加新的可选 listen transport 时(照同一模式: 探测 → 剔除 → 四要素 warn)。

## When Not to Apply

- tcp / udp / tls / unixpipe: 这些的绑定失败本来就致命, 不需要额外探测;
- 运行期热插拔场景: 本模式只在启动时探测一次, 设备中途消失仍需按 zenoh 的后台重试
  语义理解, 不在本约定覆盖范围;
- 多播 / 发现层顺序问题 (见 `zenoh-hello-locator-priority`), 两件事互不替代。

## Related

- `src/zenoh_runtime/listen_plan.rs` (本约定的实现与 4 条单测)
- `docs/solutions/best-practices/zenoh-hello-locator-priority.md` (另一条 locator 语义坑:
  看见了 router 不等于连得上)
- `docs/solutions/conventions/zenoh-pub-sub-declaration-propagation.md` (同族「zenoh 语义
  不是直觉语义」的约定)
- `specs/zenoh-unixpipe-fast-path-plan.md` (本地 fast path listen endpoint 的组入口径)
