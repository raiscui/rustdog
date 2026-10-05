# CDP 远程通道规格

> 状态：**决策已定**（2026-10-06 用户确认全部采纳推荐方案）
> 来源：2026-10-05 远程验证 Electron 自动更新链时，CDP 只能手工搓一次性脚本

## 目标

让 rdog 控制面具备 **CDP（Chrome DevTools Protocol）** 通道，从而在**不介入被控端桌面**的前提下：

- 读取与操作 **Electron 应用的 renderer**（DOM、JS 状态、console、截图）
- 用 CDP 驱动**浏览器**做网页自动化，补现有 AX lane 的盲区

这份文档是 CDP lane 的需求与决策记录，不是施工说明。

---

## 需求来源（真实场景）

2026-10-05 远程验证一条 Electron 应用的自动更新链时，需要确认 renderer 状态 ——
读 DOM 确认更新横幅文案、点「立即重启更新」按钮、截图取证。

没有 CDP 通道时的实际做法：往被控端扔一个一次性 `.mjs` 脚本，它自己
`fetch('http://127.0.0.1:9223/json')` 拿 target、`new WebSocket()` 连
debugger、`Runtime.evaluate` 执行表达式、打印结果。**这个脚本用完即弃**：

- 每次姿势不同（读什么、点哪里）都要重写重传
- 拿不到结构化响应，只能靠 stdout 人工解析
- 换一台机器要重写一遍（macOS 那次验证时写的脚本在 Windows 上不能用）

结论：**CDP 是控制面缺失的一块**。Electron/Chromium 应用的可观测性与
可操作性，目前完全依赖 agent 手搓一次性脚本。

---

## 定位：与现有 web lane 互补，不是替代

`control_web`（`@web-find` / `@web-act`）走 **AX tree** 路线
（角色 `AXLink` / `AXButton` / `AXMenuButton` / `AXGroup`，按进程名找浏览器窗口）。
CDP 走 **DOM + JS 上下文**路线。

| 维度 | 现有 web lane（AX） | 拟建 CDP lane |
| --- | --- | --- |
| 证据源 | 平台可访问性树 | DOM + JS 运行时 |
| Windows 可用性 | 实测 `platform: unsupported` | Chromium 协议跨平台一致 |
| canvas / shadow DOM | 不可见 | 可见 |
| 读 JS 运行时状态 | 不支持 | `Runtime.evaluate` 任意表达式 |
| console / 网络 | 不支持 | `Runtime.consoleAPICalled` / `Network.*` |
| 依赖平台辅助功能授权 | 是（macOS TCC 敏感） | 否 |
| 语义级动作（"点登录按钮"） | 强 | 需 CSS selector |
| Electron 应用 | 部分 AX 树不完整 | 原生支持 |

**结论**：两条 lane 并存。AX 保留（语义动作 + 零 CDP 端口的浏览器），
CDP 补上（跨平台 + DOM 级 + JS 可观测 + Electron 调试）。

**Windows 上 CDP 的价值反而最高** —— 那边 AX lane 整体 unsupported（2026-10-05 实测），
CDP 是唯一能拿到 DOM 的路。

---

## 两个使用面

### 面 A：Electron 远程调试桥接

远程连上任意机器上的 Electron 应用，直接读写其 renderer：

- 读 renderer DOM（确认 UI 状态、横幅文案、错误提示）
- 注入 JS 执行（调应用暴露的调试接口、取 store 快照）
- 拿 console 输出与未捕获异常
- renderer 截图（比 OS 级窗口截图精确，不受窗口遮挡与缩放影响）

前置条件：目标 Electron 以 `--remote-debugging-port=<port>` 启动。
**谁来启动**是待决项之一。

### 面 B：浏览器 CDP 控制

用 CDP 驱动 Chrome / Edge / Arc 等做网页自动化，补 AX lane 盲区
（canvas 渲染内容、shadow DOM、单页应用的 JS 状态）。

---

## 决策记录（2026-10-06 已定）


> **结论：四项全部采纳推荐方案**（A/B/C 见各节标注），本文其余部分按此为准。
### 决策 1：CDP client 放哪一端

| 方案 | 形态 | 评价 |
| --- | --- | --- |
| **A. daemon 内置 client**（推荐） | 被控端 daemon 连本机 CDP，控制端发语义化 CDP 命令 | 与 click/screenshot 现有形态一致；权限可管；控制端无需 WS 栈 |
| B. 端口穿透 | daemon 把 CDP 端口经 rdog 通道转发，控制端自跑 CDP client | 实现快，但控制端要带 WS，且每个 Chromium 应用要单独配端口 |

**推荐 A**。B 看似省事，实则把 CDP 的巨大安全面直接暴露到网络，
且无法复用 daemon 已有的权限与传输层。

### 决策 2：verb 粒度

| 方案 | 示例 | 评价 |
| --- | --- | --- |
| A. 细粒度语义 verb | `@cdp-eval` / `@cdp-query` / `@cdp-console` | 好用，但 CDP 方法上百，语义 verb 会长到不可维护 |
| **B. 透传 + 常用封装**（推荐） | `@cdp:{method:"Runtime.evaluate",params:{...}}` + `@cdp-eval` / `@cdp-targets` 等薄封装 | 通用能力不封死，常用动作不必手写 JSON |
| C. 全语义 | 每个 CDP 方法一个 verb | 维护不可持续 |

**推荐 B**。

### 决策 3：安全模型

CDP = 被控端任意代码执行能力：**debug 端口一旦可达，连接者即拥有该进程全权**。
是否有额外的连接方校验不改变这个结论（任何能在被控端本地发起连接的组件
都能满足），不能把它当缓解措施。

最低要求：

- **默认关闭**，需配置显式开启（不能"有端口就能连"）
- 开启后 CDP 仅在 daemon 本机回环可达，不经网络暴露
- 沿用 rdog 既有 capability 授权面（`@capabilities` 应能看到该项）

### 决策 4：Electron 调试端口由谁开

| 方案 | 评价 |
| --- | --- |
| A. 用户自己带 flag 启动 | 零侵入，但"要能调试"变成使用者的隐性负担 |
| B. rdog 提供启动包装 | 一等公民，但 `@open-app` 目前非 macOS 是 `platform_unsupported`（`control_actions.rs:552`） |
| C. 两者都支持 | 推荐：默认用户自起，`@open-app` 支持时提供开箱路径 |

---

## 实现约束

1. **项目当前无 WebSocket 客户端依赖**。CDP 必须 WS（`GET /json` 只拿 target
   列表，方法调用全在 WS 上）。需新增 `tungstenite`（同步、纯 Rust、与现有
   `std::process` 风格一致）或 `tokio-tungstenite`（若走异步 runtime，
   需评估现有架构）。
2. **端口发现**：`GET /json/list` 返回 target 列表，需过滤 `type=="page"`；
   Electron 还要区分主窗口与 DevTools 窗口。
3. **证据纪律对齐**：CDP 的 `Runtime.evaluate` 同样是"截图类证据" ——
   结果可能过期。SKILL 里现有的 fresh read 纪律应延伸适用。

---

## 需求

### CDP client SHALL 在 daemon 侧经本机 WebSocket 连接

- 指定 debug 端口但未指定 target 时，daemon 查询 `GET /json/list` 并选出
  `type=="page"` 的 target，响应报告 target 的 id / title / url
- 端口连不上时返回结构化错误（**在请求期限内**），daemon 控制 worker 保持可用

### CDP lane SHALL 默认关闭，需显式开启

- 关闭时收到 CDP 请求 → 返回 `permission_denied` 类结构化错误
- `@capabilities` 报告该 lane 为禁用
- 开启时 daemon 只连回环地址，不监听也不转发 CDP 端口

### CDP lane SHALL 同时提供通用透传与常用薄封装

- 携带合法 CDP `method` 的请求 → daemon 转发给目标并返回 CDP 结果
- 求值请求 → 发出 `Runtime.evaluate`，返回值与异常文本一并返回

### CDP 响应 SHALL 是结构化的

- 响应含 target id、CDP method、成功/失败状态
- 失败时含 CDP `error` 原文，而非格式化字符串

---

## 进入施工前仍需定

以下不属上述四项决策，是实现细节待确认：


1. 同步 WS 栈选型：`tungstenite` vs `tokio-tungstenite`
2. 首批 verb 清单：`@cdp-targets` / `@cdp-eval` / `@cdp-query` / `@cdp-console` /
   `@cdp-screenshot` / `@cdp-raw` 是否够用
3. console 事件形态：一次性拉取 vs 长连接流式（建议先做一次性拉取）

---

## 相关

- [远程被控端实战反馈](remote-control-blocked-command-pitfalls.md) —— 同一场作业暴露的
  控制面问题，含 Windows window lane 整体不可用的实测
- [文件传输通道规格](file-transfer-control-spec.md)
