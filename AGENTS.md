# rustdog 仓库知识索引

这个文件只负责给仓库里的长期知识载体建索引。
更具体的执行规范,以外层注入的 AGENTS 指令和会话中的高优先级消息为准。

## 何时优先阅读

- 修改 `src/control_protocol.rs`、`src/shell.rs`、`tests/control_lanes.rs`、`tests/control_mode.rs` 前,先看 line-control 协议规格。
- 修改 `src/pty_control.rs`、`src/control_transport.rs`、`src/zenoh_control.rs` 或 `rdog control --pty` 行为前,先看 PTY control 规划。
- 需要理解当前协议为什么会同时保留显式协议请求和裸 shell 行时,先看项目经验沉淀。

## 长期文件索引

- `specs/control-line-protocol.md`
  - 主题: 当前 line-control 协议正式规格
  - 用途: 定义 `@response`、可选 request id、`@cmd#id` 与裸 shell 行共存边界
  - 何时阅读: 修改控制协议、响应格式、request id 或 shell/控制分流逻辑前

- `specs/pty-control-plan.md`
  - 主题: `@pty` / `@pty-close` 远程 PTY 会话规格
  - 用途: 固定 `rdog control TARGET --pty -- COMMAND ...`、PTY frame、透明输入、out-of-band close、Zenoh session channel 映射与验证边界
  - 何时阅读: 修改远程 PTY、control transport streaming、Zenoh session channel、`--pty-close` 或 TTY 交互行为前

- `EXPERIENCE.md`
  - 主题: 本项目已经验证过的协议设计经验与边界
  - 用途: 帮助后续改协议时避免把 `@exit`、显式协议请求和裸 shell 行再次混淆
  - 何时阅读: 做协议演进、回顾历史判断口径、沉淀经验时

- `.codex/skills/self-learning.zenoh-duplicate-name-local-guard/SKILL.md`
  - 主题: Zenoh 同机重复 `daemon_name` / `service_name` 只靠 liveliness 检查会漏掉启动竞争窗口
  - 用途: 固定“本地 PID/lock guard + 网络 liveliness 双层约束”的修复模式
  - 何时阅读: 遇到 Zenoh 同机重复实例偶发并存、duplicate-name 测试偶发通过,或准备实现逻辑身份唯一性保护之前

- `.codex/skills/self-learning.zenoh-fifo-recv-timeout-timeout-not-closed/SKILL.md`
  - 主题: Zenoh FIFO `recv_timeout()` 的 `Ok(None)` 是 timeout,不是 subscriber closed
  - 用途: 固定 active PTY / session bridge 短轮询时如何区分 timeout、idle 回收和 terminal lifecycle frame
  - 何时阅读: 遇到 Zenoh PTY output 已产生但 client 报 subscriber closed / transport lost,或修改 `src/zenoh_control.rs` 里的 `recv_timeout()` loop 前

- `archive/manifests/ARCHIVE_MANIFEST__2026-04-06_continuous-learning.md`
  - 主题: 2026-04-06 持续学习批次的支线上下文归档说明
  - 用途: 说明哪些旧支线六文件已完成检索总结并迁入 `archive/branch_contexts/`
  - 何时阅读: 需要追溯旧支线文件为什么被归档、或想快速定位归档后的支线目录时

- `archive/manifests/ARCHIVE_MANIFEST__2026-05-05_zenoh_bare_shell.md`
  - 主题: 2026-05-05 Zenoh 裸 shell 实现前的默认六文件续档与支线归档说明
  - 用途: 说明旧默认 `task_plan.md` / `notes.md` / `WORKLOG.md` 续档位置,以及 `control_zenoh_default` 支线上下文归档位置
  - 何时阅读: 需要追溯 `rdog control mac.lab` 短入口、Zenoh 裸 shell 能力统一前置上下文,或查找 2026-05-05 前默认工作记录时

- `archive/manifests/ARCHIVE_MANIFEST__2026-05-07_continuous-learning.md`
  - 主题: 2026-05-07 持续学习批次的默认六文件续档说明
  - 用途: 说明 `task_plan.md` / `ERRORFIX.md` 超过 1000 行后的续档位置,以及本轮 Zenoh PTY 生命周期经验沉淀范围
  - 何时阅读: 需要追溯 2026-05-07 前的长任务计划、错误修复记录,或确认为何当前默认 `task_plan.md` / `ERRORFIX.md` 变短时

- `archive/manifests/ARCHIVE_MANIFEST__2026-05-11_rdog_rename_continuation.md`
  - 主题: 2026-05-11 rdog 更名任务的默认 `task_plan.md` 续档说明
  - 用途: 说明 `task_plan.md` 超过 1000 行后的归档位置,以及本轮 `rcat/rustcat` -> `rdog/rustdog` 更名、legacy 兼容和权限主体经验沉淀范围
  - 何时阅读: 需要追溯 rdog 更名任务的长计划、验证证据、legacy 兼容决策,或确认为何当前默认 `task_plan.md` 变短时

- `archive/manifests/ARCHIVE_MANIFEST__2026-05-12_rdog_control_skill_worklog.md`
  - 主题: 2026-05-12 创建 `rdog-control` 全局 skill 后的 `WORKLOG.md` 续档说明
  - 用途: 说明旧 `WORKLOG.md` 超过 1000 行后的归档位置,以及本轮 skill 创建、验证和硬件/单片机表述边界沉淀范围
  - 何时阅读: 需要追溯 `rdog-control` skill 为什么创建、旧 WORKLOG 为什么被续档,或查找 2026-05-12 前默认工作记录时

- `/Users/cuiluming/.codex/skills/rdog-control/SKILL.md`
  - 主题: code agent 使用 `rdog daemon` / `rdog control` 控制局域网主机、硬件桥接机和单片机场景的全局 skill
  - 用途: 提供 target-name / `--entry-point` / line-control / PTY / screenshot / savefile / Zenoh session channel 的可复用操作指南
  - 何时阅读: 需要让 Codex 或其他 code agent 使用 `rdog control <target-name>` 控制主机、桌面、硬件桥接机或单片机场景之前

- `specs/zenoh-control-plane-plan.md`
  - 主题: `rustdog` 的 canonical Zenoh router/serial control-plane 规划
  - 用途: 固定 daemon 内嵌 router、control client、native `transport_serial`、autodiscovery 默认接入 + `--entry-point` fallback、identity/keyexpr、CLI/config、runtime 边界与验证矩阵
  - 何时阅读: 规划或实现 Zenoh router/serial transport、daemon/control 互联,或准备把 control plane 从 TCP 扩到 Zenoh 之前

- `specs/zenoh-peer-peer-lan-profile.md`
  - 主题: 历史性的同 LAN `daemon = peer`、`control = peer` 规格草案
  - 用途: 保留旧 peer/peer 语义、peer discovery 行为、entry point fallback、唯一性约束与 CLI/config 的历史记录; 新实现不以此为主路径
  - 何时阅读: 仅在回顾旧 peer/peer 设计、做迁移对照或排查历史行为时

- `specs/zenoh-sdk-integration-playbook.md`
  - 主题: 其他 app 使用 Zenoh SDK 对接 `rdog` daemon 的操作手册
  - 用途: 固定当前 service/member keyexpr、query payload / reply contract、discovery / resolve / retry 策略,辅导编程智能体实现对接
  - 何时阅读: 需要让第三方 app 或编程智能体直接通过 Zenoh SDK 对接 `rdog` daemon 之前

- `specs/code-agent-rdog-control-usage.md`
  - 主题: code agent 使用 `rdog daemon` / `rdog control` 在局域网或可达远程网络上操控和协调主机的操作指南
  - 用途: 固定真实 CLI 入口、target-name 寻址、line-control/PTY/截图/按键能力矩阵、Zenoh session channel 模型、安全权限边界和 smoke 命令
  - 何时阅读: 需要让 code agent 通过 `rdog control <target-name>` 控制不同主机、编排多主机任务、或解释 `rdog control` 相比 SSH 的独特性之前

- `specs/zenoh-sdk-agent-prompts.md`
  - 主题: 给编程智能体直接使用的 Rust / Unity Zenoh 对接实现提示模板
  - 用途: 提供可复制 prompt,指导智能体用 Zenoh Rust SDK 或 `mhama/zenoh-unity-plugin` 对接 `rdog` daemon
  - 何时阅读: 需要把对接手册进一步变成“智能体可直接执行的实现提示”之前

- `specs/zenoh-unity-querier-wrapper-design.md`
  - 主题: Unity 使用 `mhama/zenoh-unity-plugin` 对接 `rdog` daemon 的最小 query/reply wrapper 设计草图
  - 用途: 说明当前插件已有 wrapper 能力、缺失的 query/reply 封装层,以及给智能体的最小封装建议
  - 何时阅读: 需要让 Unity 侧真正开始实现 `rdog` control query/reply 对接前

- `specs/zenoh-screenshot-control-plan.md`
  - 主题: `@screenshot` 远程截图能力的控制面规划
  - 用途: 固定“截图请求继续走显式 control plane,默认 all-display composite JPEG + manifest JSON 通过 `@savefile` 返回,再以 `@response ...screenshot-bundle...` 收口; macOS 用 `sck-rs` 主路径 + `xcap` fallback,其他平台走 `xcap`”这一轮方案
  - 何时阅读: 准备实现或评审 `@screenshot`、截图结果响应格式、截图后端选择或 screenshot control 测试方案前

- `specs/rdog-multi-display-screenshot-coordinate-plan.md`
  - 主题: 多显示器 `@screenshot` bundle 和截图坐标/OS 鼠标坐标契约
  - 用途: 固定 `display:"all"`、`layout:"composite"`、`coordinate_space:"os-logical"`、virtual desktop JPEG、manifest JSON、`display:"primary"` 兼容入口、gap/rotation 和 Screen Recording 权限边界
  - 何时阅读: 准备实现或评审多显示器截图、manifest schema、后续 `@click` / `@drag` 坐标换算、或排查 screenshot 坐标偏移前

- `specs/rdog-mouse-control-coordinate-plan.md`
  - 主题: `@mouse-move` / `@mouse-button` / `@click` / `@drag` / `@wheel` 鼠标控制方案
  - 用途: 固定鼠标控制必须复用 screenshot manifest 的 `os-logical` 坐标语义,以及 press/release、click、drag、wheel 的协议字段、错误边界和验证矩阵
  - 何时阅读: 准备实现或评审鼠标移动、点击、拖拽、滚轮、button press/release,或排查多显示器鼠标坐标偏移前

- `specs/bidirectional-control-plane-plan.md`
  - 主题: 控制面从单向 request/reply 升级为真正双向 control peer 的规划
  - 用途: 固定“daemon 和 control 双方都能主动发送 `@key` / `@script` / `@savefile` 等显式控制指令,Zenoh 不再停留在单向 query/reply”这一轮新的架构方向
  - 何时阅读: 准备继续实现 `@savefile`、`@screenshot`、daemon 主动下发控制指令,或评审 TCP/WebSocket/Zenoh 控制模型是否仍然单向时

- `specs/control-frame-refactor-plan.md`
  - 主题: control core / outbound frame / session 抽象的分阶段重构计划
  - 用途: 固定从 `String @response` 返回模型演进到 `ControlExecutionOutcome + ControlFrame + ControlPeerSession` 的实施顺序,以及 TCP/WebSocket 先行、Zenoh 后续迁移的验证路径
  - 何时阅读: 准备开始写双向控制代码、评估先改哪层抽象、或 review `@savefile`/双向 frame 改造顺序时
