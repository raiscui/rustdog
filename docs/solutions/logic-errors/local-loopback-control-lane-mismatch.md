---
title: 本机自连的两种坏路径 — 绑定地址当 connect 目标、端口上说的是另一套协议
date: 2026-11-26
last_updated: 2026-11-26
module: control plane
component: control_invocation / local_daemon_registry / zenoh-proofing
problem_type: logic_error
severity: high
status: active
tags:
  - local-fast-path
  - windows
  - loopback
  - protocol-mismatch
  - zenoh
  - agent-ux
verified_by:
  - "2026-11-26 Linux 同构复现 (本仓库 target/debug/rdog, daemon 以 zenoh router 绑 tcp/127.0.0.1:17480): `--transport tcp 127.0.0.1 17480 @ping` 与 `control 17480 @ping` 都返回「一帧 control 都没回就关闭了会话」, 同 daemon 的 `--transport zenoh --namespace lab win-test.lab @ping` 返回 @response"
  - "2026-11-26 Windows 11 真机 (rais-razer, daemon 绑 tcp/0.0.0.0:17480) 报 os error 10049 / 会话提前关闭, 与上述复现同因"
  - "2026-11-26 修复后 Linux 回执: `control self @ping`、`control 17480 @ping`、`control 127.0.0.1 17480 @ping` 三条都拿到 @response, `--transport tcp` 保持 TCP 语义并给出可行动错误"
resolution_type: "已落地: 端口简写/未指定地址映射回环 (src/control_invocation.rs::connect_host), 本机 daemon 控制 lane 登记表 (src/local_daemon_registry.rs) + 回环端口按登记认领协议 (promote_loopback_tcp)"
related_solutions:
  - docs/solutions/best-practices/remote-control-blocked-command-pitfalls.md
  - docs/solutions/best-practices/zenoh-hello-locator-priority.md
related_skills:
  - rdog-control
---

# 本机自连的两种坏路径 — 绑定地址当 connect 目标、端口上说的是另一套协议

## 背景

`rdog control` 的"本机自己连自己"在 Windows 上曾有两条路都是坏的, 而它们看起来是
两个 bug, 实际是两个独立成因叠在同一个入口上:

| 调用 | 现象 | 成因 |
|---|---|---|
| `rdog control 17480 @ping` | `os error 10049` | 端口简写被解析成 `host = 0.0.0.0`, 拿 **listen 地址**当 connect 目标 |
| `rdog control 127.0.0.1 17480 @ping` | TCP 连上, 但 `control connection 在收到 UI script 结果前就关闭了` | 17480 是 daemon 的 **zenoh listen endpoint**, 上面只跑 zenoh 协议 |
| `rdog control self @ping` | `当前平台不支持 unixpipe 本机 fast path` | Windows 的 zenoh 构建没有 `transport_unixpipe`, 本机 lane 不存在 |

## 成因一: 绑定地址不是可达地址

`0.0.0.0` / `::` 只在 `bind()` 语境里有意义。当 connect 目标时:

- Linux 上碰巧落到本机回环, 所以这个 bug 在 unix 上从没被发现;
- Windows 上直接 `WSAEADDRNOTAVAIL`(os error 10049), 连"有人监听"都谈不上。

用户写 `rdog control 17480` 的语义是"这台机器上的 daemon", 与 `0.0.0.0` 无关。
所以判据是: **只要目标地址是未指定地址, 就映射回环** (`0.0.0.0 → 127.0.0.1`,
`:: → ::1`), 位置在 `src/control_invocation.rs::connect_host`, 由
`host_from_opts` 与端口简写分支统一调用 —— 单一出口, 避免只修一个入口。

## 成因二: 同一端口换了协议, 表现却是"连接被关闭"

`run_zenoh_router` 会**强制** `config.inbound.enabled = false`, 并在横幅里写
`tcp=不启动`。于是配置里 `tcp/0.0.0.0:17480` 这个端口上只有 zenoh 的 link 层:
原始 TCP 三次握手照样成功, 客户端发 `@ping\n` 后 zenoh 判定握手非法、丢弃链路,
客户端只看到 EOF。

这就是"连接能建立但会话提前关闭"的指纹 —— **不是**自连检测、不是认证、不是
UI script 前置。判别方法: 一帧都没收到就 EOF。据此 client 侧给出可行动错误
(`ControlPeer::closed_before_any_frame_message`), 而不是含糊的"结果前就关闭了"。

## 修复: 让本机 daemon 自己登记控制 lane

命名管道 (`\\.\pipe\rdog-<ns>-<name>`) 是对齐 unixpipe 语义的正解, 但它要求
rustdog 自己实现一套 Windows 命名管道传输 + 帧处理 + 认证, 与 zenoh 那条链路
平行存在 —— 体量和风险都高一个数量级。选的是"回环直连", 但落点不是 line-control
TCP (那个端口说的是 zenoh), 而是:

1. daemon 启动成功后把身份与本机落点写进
   `~/.rdog/run/local-daemon/<namespace>__<daemon_name>.json`
   (`src/local_daemon_registry.rs`), lane 形如
   `{"kind":"zenoh","connect_endpoints":["tcp/127.0.0.1:17480"]}`;
2. liveness 的真相源是 lock 文件上的 OS advisory lock, 不是 PID (进程怎么死内核都会
   释放 lock, PID 会复用);
3. 客户端 `self` / 空 target 读它 → 直接 zenoh 回环连本机 router;
4. `control <port>` 这类"只给端口"的目标, 在 host 是回环时按登记表**认领协议**
   (`promote_loopback_tcp`): 该端口属于已登记的 zenoh lane 就走 zenoh, 否则保持
   line-control TCP 老行为。显式 `--transport tcp` 不参与认领 —— 那是用户的协议选择。

平台分工写死在代码里: **unix 继续用 unixpipe FIFO** (不写这份登记, 避免两套本机
fast path 并存导致 "self 选了谁" 二义); 非 unix 用登记表。因此 unix 的
`unmanaged FIFO 拒绝`语义零变化。

## 复验方法 (无需 Windows)

本机 lane 的登记是 daemon 侧动作, 但客户端解析可以单独验证: 手工写一条登记记录,
用一个持有 exclusive lock 的进程当 owner, 然后跑三条命令。回执要求:
`control self` / `control <port>` / `control 127.0.0.1 <port>` 都出现 `@response`;
`RDOG_LOG_LEVEL=debug` 下各有 `control link: ...` 一行说明走了哪条链路。
