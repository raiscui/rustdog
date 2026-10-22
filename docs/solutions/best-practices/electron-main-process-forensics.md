---
title: "Electron 主进程取证: 渲染层 CDP 看不见主进程时的双通道定位法"
date: 2026-10-22
last_updated: 2026-10-22
module: control plane
component: cdp-lane
problem_type: best_practice
severity: medium
status: active
tags:
  - electron
  - cdp
  - ipc
  - remote-ops
  - debugging
  - agent-ux
verified_by:
  - "2026-10-11 Windows 11 真机跨机调试 (rdog control 远程拉起 Electron 应用): --inspect 主进程取证 + 双通道实验各复现一轮"
  - "2026-10-22 复核: rdog 侧 CDP lane 代码存在 (src/control_cdp.rs), skill recipe 见 .codex/skills/rdog-control/SKILL.md CDP Lane 节"
  - "证据缺口: 被调 Electron 应用与版本未在本仓记录; 结论依赖 Node inspector 与 Electron ipcMain 内部结构, 跨大版本需重新核对"
related_solutions:
  - docs/solutions/best-practices/remote-control-blocked-command-pitfalls.md
related_skills:
  - rdog-control
---

# Electron 主进程取证: 渲染层 CDP 看不见主进程时的双通道定位法

## Context

用 rdog 的 CDP lane 远程调一个 Electron 应用时, 最容易踩的坑是**取证面选错**:
渲染进程的 DevTools 协议 (CDP) 只能看到渲染层, **看不到主进程的 JS 状态**。
于是当"renderer 发了消息, main 没反应"这类问题出现时, agent 只能反复在渲染层
黑盒猜测 —— 改代码、加日志、猜 IPC 通道名, 三轮下来仍在对猜。

本文固定两件可复用的东西:

1. 主进程怎么取到证面 (`--inspect` + 不过滤 target 类型 + `ipcMain` 两个挂载点)
2. 怎么用**双通道实验**一次把"发送链断"与"handler 逻辑错"分开

适用版本: Electron 应用, 主进程可用 `--inspect` 打开 Node inspector;
rdog 侧走 `specs/cdp-remote-lane-spec.md` 描述的 CDP lane (daemon 内置 lane 或
CDP-over-@cmd 兜底, 见 skill)。

## Guidance

### 一、主进程取证面 = `--inspect`, 不是渲染层 CDP

拉起被调应用时给主进程加 inspector 端口:

```bash
# 主进程开 Node inspector (端口与渲染层 9222 分开, 避免混淆)
<app.exe> --inspect=9230
```

Node inspector 暴露的是主进程自己的 JS 上下文, 里面的变量 (`ipcMain`、
模块级状态、业务单例) 都能直接读。

**取 target 时不要按 `type` 过滤**: 主进程 inspector 的 target 类型不是 `page`
(node target)。用 `Where type -eq 'page'` 过滤会**取到空集合**, 然后被误读成
"主进程没开 inspector" / "没有挂载 handler"。正确做法是不过滤 type, 或按
`webSocketDebuggerUrl` 存在与否取。

### 二、`ipcMain` 有两个挂载点, 查清单必须两处都看

Electron 主进程的 IPC 注册分成两套, 存放在**不同**的字段里:

| 注册方式 | 存放位置 | 数据结构 |
| --- | --- | --- |
| `ipcMain.on(channel, handler)` | `ipcMain._events[channel]` | 事件字典 |
| `ipcMain.handle(channel, handler)` | `ipcMain._invokeHandlers` | `Map` |

只查 `_events` 会漏掉全部 `handle()` 型通道 (渲染层用 `invoke()` 发的就是这一类),
表现为"清单里根本没有这个通道", 很容易被误判成"发送方没发"。

### 三、handler 可以直接手动调用 —— 一步验证「handler → 状态 → 消费面」全链

拿到 handler 引用后, 可以在主进程上下文里直接调用它, 读它写进的状态:

```javascript
// 事件型
ipcMain._events[channel]({ sender: null }, payload)
// invoke 型
ipcMain._invokeHandlers.get(channel)({ sender: null }, ...args)
```

这一步把"handler 逻辑本身对不对"从黑盒猜测变成一次直接观察: 调完立刻读
handler 写入的状态字段, 就能判断消费面是否被正确更新, 不必等真实消息往返。

### 四、双通道实验: 一次分开"发送链断"与"handler 错"

当症状是"renderer 发了, main 好像没收到"时, 同时用两个通道:

1. **渲染层通道** (CDP 9222): 从 renderer 侧真的发一次消息, 触发问题路径
2. **主进程通道** (inspector 9230): 手动调用消费 handler, 直接读状态

判据:

| inspector 侧表现 | 结论 |
| --- | --- |
| 行为正确 (状态按预期更新) | 失效点在**发送链** (renderer → main 的 IPC 投递) |
| 行为错误 (状态不更新 / 抛错) | 失效点在 **handler 逻辑** |

**单侧取证永远是在对猜**: 同一现象可以由发送链、handler、通道名错误、
时序竞争中任意一个引起, 单看一侧都解释得通。2026-10-11 的现场就是
"busy 疑云"连续三轮黑盒猜测无果, 双通道一次定谳。

## Evidence

- 静态: rdog CDP lane 实现存在 (`src/control_cdp.rs`); skill 已有形态 A/B 两条
  通路 (`@cdp-targets` / `@cdp-eval` / `@cdp`) 与 CDP-over-@cmd 兜底配方
- 静态: `ipcMain._events` (on) 与 `ipcMain._invokeHandlers` (handle) 两处挂载点
  分离, 是 Electron 主进程 IPC 的既有结构, 与渲染层是否可见无关
- 动态 (2026-10-11): `--inspect=9230` 后按 `type='page'` 过滤取到空集合;
  不过滤即取到主进程 target
- 动态 (2026-10-11): 手动调用消费 handler 后状态按预期更新, 判定失效点在发送链
- 可复跑 (在可加 `--inspect` 的 Electron 环境中):

```bash
# 1. 起主进程 inspector
<app.exe> --inspect=9230
# 2. 取 target (注意: 不要按 type 过滤)
curl -s http://127.0.0.1:9230/json/list
# 3. 在主进程上下文读挂载清单
ipcMain._events; ipcMain._invokeHandlers
```

## Why This Matters

- 猜错的代价是**时间**: 2026-10-11 现场三轮黑盒猜测未果, 而双通道实验一次定谳。
- 错取证面会制造**假结论**: "主进程里没有这个 handler" 往往只是查错了字段
  (`_events` vs `_invokeHandlers`) 或过滤掉了 node target, 不是应用没注册。
- 这条方法对 rdog 自身有特殊价值: rdog 的 CDP lane 就是给 agent 用的,
  agent 第一次远程调 Electron 应用时几乎必然撞上这个坑。

## When to Apply

- 远程调试 Electron 应用, 症状涉及主进程状态、IPC、后台任务
- 怀疑"消息发出去了但对方没反应"
- 需要读主进程的业务状态 (单例、模块级变量) 而渲染层拿不到

## When Not to Apply

- 纯渲染层问题 (DOM 渲染、页面内 JS 状态): 直接用渲染层 CDP, 不必开 inspector
- 非 Electron 应用: 无 `ipcMain`, 第三、四节不适用 (但"双通道分清发送链与消费端"
  这个思路仍可迁移)
- 主进程不允许开 `--inspect` (安全策略): 只能退回渲染层取证 + 应用侧日志

## Related

- `.codex/skills/rdog-control/SKILL.md` — CDP Lane 节 (形态 A/B + 本篇要点摘要)
- `specs/cdp-remote-lane-spec.md` — rdog CDP lane 规格
- `docs/solutions/best-practices/remote-control-blocked-command-pitfalls.md` —
  同一场跨机实战的其他沉淀 (坐标空间 / 超时 / 带空格路径)
