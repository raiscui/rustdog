---
title: "分片到达的控制行会被读超时清空缓冲而静默丢弃"
date: 2026-11-26
last_updated: 2026-11-26
module: control
component: control-line-read
problem_type: logic_error
severity: high
status: active
tags:
  - line-protocol
  - read-timeout
  - partial-message
  - pty
  - silent-data-loss
verified_by:
  - "python pty.fork 纯 kernel 对照: 写 0x03 到 pty master, 远端 trap 收到 INT (pty 与信号语义本身正常)"
  - "直连 daemon TCP, 帧与 '\\n' 同一 chunk: 远端输出 ^CREMOTE_INT + @pty-exit 130"
  - "同一帧与 '\\n' 分两次 sendall(间隔 30ms): 修复前丢帧, 修复后连跑 3/3 成功"
  - "CLI/daemon 双侧 strace 对拍: daemon 收到 98 字节 @pty-stdin, 但 pty master fd 只写出过 \\4 没写出 \\3"
  - "cargo test --test control_pty: 9 passed / 0 failed (control_pty_ctrl_c_... 断言零改动变绿)"
  - "cargo test --bin rdog: 1114 passed / 0 failed / 2 ignored"
root_cause: "read_line_limited 每次调用开头 buf.clear(); 对端把一条消息拆成「正文」与「结尾换行」两次写时, 两次读之间只要撞上读超时, read_until 就以 Err(WouldBlock) 返回, 已攒的半个行被下一次调用清掉 —— 整条消息静默丢失, 流仍对齐且不报错"
resolution_type: "读行状态收进 LineReadState(半行缓冲 + 丢弃标记 + 已丢弃字节数), 半行跨读超时保留、下一轮续读拼接; 超限判定按内容长度(排除 CRLF 的 \\r), 丢弃阶段的状态与计数一并跨调用保存"
related_solutions:
  - docs/solutions/conventions/env-deterministic-ci-red-vs-local-green.md
  - docs/solutions/test-failures/tty-term-dumb-environment-deterministic-failure.md
  - docs/solutions/logic-errors/gnu-coreutils-kill-neg-pid-argument-ambiguity.md
---

# 分片到达的控制行会被读超时清空缓冲而静默丢弃

## Problem

line-control 协议按行收发(`@verb` / `{json}` 一条一行),读取入口是
`src/control_line_read.rs::read_line_limited`,由 **TCP 传输**、**控制端 stdin** 和 **shell listener** 共用。

> WebSocket 传输**不走**这个读行器:`ControlTransport::read_message` 把它分派给
> `WebSocketTextTransport::read_message`,后者用 `WebSocketFrame::read_from` 读帧。
> 本文的诊断与修复只覆盖上面三条路径。

旧实现每次调用开头 `buf.clear()`,隐含假设「一次调用就能读完整行」。但两侧的写法都不满足这个假设:

- **读者侧**:PTY 桥接把传输读超时设成 25ms(`src/shell.rs::bridge_attached_pty_session_inner`、
  `src/pty_control.rs` 的 tty 循环),`read_until` 在「正文已到、换行未到」时被超时打断,返回 `Err(WouldBlock)`;
- **写者侧**:`control_transport::write_message` 是 `write_all(message)` + `write_all("\n")` **两次**系统调用,
  于是「帧」和「结尾换行」经常落在两个 TCP chunk 里 —— **分片是常态而不是异常**。

两者一撞:半个行留在 `buf` 里,函数带着 `Err` 返回;下一轮调用开头把它清掉,这条消息就永久消失。
流仍然对齐(下一条消息照常解析),不报错、不打日志 —— 上层看到的现象只是「对端没反应」。

第一次命中的现场是 PTY 的 `^C`:带 0x03 的 `@pty-stdin` 帧被吞,远端子进程收不到 SIGINT,
`control_pty_ctrl_c_should_exit_remote_program_and_return_to_control_prompt` 稳定失败(RAI-415)。

## Symptoms

- 一条控制指令「发了但没效果」,daemon 日志里没有任何异常;
- 同样的命令单独重试经常就成功了(取决于两次写之间有没有撞上读超时);
- 日志层面与「信号丢失」「远端程序不响应」完全同形,靠日志无法区分。

## What Didn't Work

- **先怀疑 pty 语义**:远端 pty `stty -a` 显示 `isig icanon`,helper 就是该 pty 的 foreground pgrp
  (`pid=pgid=tpgid=sid`),用 python `pty.fork()` 写 0x03 到 master 也能正常触发 trap ——
  pty 与信号链本身没问题;把它当产品语义缺陷去改 pty lane 会白改一场。
- **只看一侧日志**:CLI 侧看不出丢帧,daemon 侧也没有「收到但没处理」的日志,两侧各自看都「一切正常」。
- **加大超时/重试**:超时窗口变大只改变概率,不改变语义 —— 分片仍然会被丢。

## Verified Root Cause

`strace` 双侧对拍把范围从「pty lane 的产品语义」缩到「读取层的缓冲语义」:

```
CLI   (fd3): sendto(3, "@pty-stdin {...\"data\":\"Aw==\"}", 98) = 98
             sendto(3, "\n", 1) = 1                    # 两次写
daemon(fd7): recvfrom(7, "@pty-stdin {...}", 98) = 98
             recvfrom(7, "\n", 1) = 1                   # 两次读, 中间若干次 EAGAIN
daemon pty master(fd9): write(9, "\4", 1)           # 只写出过 ^D, 从没写出 ^C
```

帧确实到了 daemon 的 socket、也确实被 `recvfrom` 读进了缓冲,但 pty master 上没有对应的写出。
注意它**没有走到解析**:旧实现里 `read_line_limited` 以 `Err` 返回,`TcpLineTransport::read_message` 直接把错误
上抛,`PtyStdinFrame::parse_wire_message` 从未被调用 —— 帧是在「收进缓冲」与「交给解析」之间被丢的,
这正好指向读行器的缓冲语义,而不是协议解析或 pty 转发。
最小复现(直连 daemon,`@pty` 后把帧与 `"\n"` 分两次 `sendall`,间隔 30ms):

```
split=False  REMOTE_INT=True   exit=True    # 同一 chunk, 稳定成功
split=True   REMOTE_INT=False  exit=False   # 分片 + 超时, 帧被吞
split=True   REMOTE_INT=True   exit=True    # 分片但偶发成功 (取决于超时落在哪)
```

## Solution

把「半行缓冲 + 丢弃标记 + 已丢弃字节数」收进调用方持有的 `LineReadState`,让它们跨调用存活:

- 半行在 `read_until` 因超时/中断返回错误时**保留**,下一次调用续读拼接;
- 只在「整行交出 / 超限丢弃完成 / 干净 EOF」时清空;
- 超限判定按**内容长度**(排除行尾 `\n` 与 CRLF 的 `\r`);进入时先判一次,超限直接进丢弃模式;
- 丢弃阶段用显式 `discarding` + `discarded_bytes`,并在 `read_until` **带着已读字节返回错误**时
  把 `scratch.len()` 同样计入 —— 否则 `consumed_bytes` 会严重偏小(实测 4097 字节的坏行报成 1537);

回归测试都在 `src/control_line_read.rs` 的 tests 模块,用可脚本化的 `ChunkedReader` 在任意位置插 `WouldBlock`:

- `partial_line_survives_read_timeout_between_chunks` —— 分片消息必须拼成完整行;
- `crlf_split_after_cr_is_not_rejected_at_the_limit` —— 切在 CRLF 的 `\r` 之后不得改变判定;
- `retained_partial_line_cannot_grow_past_the_limit_across_timeouts` —— 跨超时保留的半行仍受 limit 约束,
  且 `consumed_bytes` 精确等于整条坏行字节数(`8 * 512 + 1`)。

## Why This Matters

**「分片是常态」不是风格建议,而是写者实现决定的事实。** 只要 `write_message` 分两次写,
任何「入口清缓冲 + 可中断读」的读取实现都会丢消息,而且丢得完全静默 —— 它不会让流错位,
后续消息照常成功,现场看起来像「偶发的业务无响应」。

同样值得记住的是**丢在哪一层**决定了排查方向:这一例丢在「收进缓冲」与「解析」之间,所以协议解析和
pty 转发都是干净的;若只验证到「帧发了」和「远端没反应」,任何一层看起来都可能是嫌疑人。

诊断上最值钱的动作是**双侧系统调用对拍**:单看任意一侧都说不清「消息没发」还是「消息丢了」;
同时抓 CLI 与 daemon 的 `sendto` / `recvfrom` / `write`,才能看到「收到了但没写出」这个中间状态。

## Prevention

- 任何「累积到分隔符」的读取实现,只要读者可能中途返回错误(超时、`WouldBlock`、`EINTR`),
  就必须把已累积数据放在调用方持有的状态里,**清空只能发生在一条完整记录交出之后**;
- 错误路径必须保留已完成的部分进度 —— 与本仓库 `durable-observation`、`control-observation`
  里「失败不推进状态」是同一条纪律的另一面;
- 验证分片行为时,用可脚本化的假 reader 在任意位置插 `WouldBlock`,比调时序、调超时可靠得多。
