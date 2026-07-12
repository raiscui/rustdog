# rustdog 仓库知识索引

rustdog 首先被LLM agent 智能体使用，其次被人类使用。要以agent使用（AI原生）为根本。

这个文件只负责给仓库里的长期知识载体建索引。
更具体的执行规范,以外层注入的 AGENTS 指令和会话中的高优先级消息为准。

## 何时优先阅读

- 修改 `src/control_protocol.rs`、`src/shell.rs`、`tests/control_lanes.rs`、`tests/control_mode.rs` 前,先看 line-control 协议规格。
- 修改 `src/pty_control.rs`、`src/control_transport.rs`、`src/zenoh_control.rs` 或 `rdog control --pty` 行为前,先看 PTY control 规划。
- 需要理解当前协议为什么会同时保留显式协议请求和裸 shell 行时,先看项目经验沉淀。
- 修改 UI script runner、GUI 自动化 JSON DSL、control-flow 脚本编译或后续 `@ui-flow` 前,先看 UI script 控制计划。

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
  - 用途: 帮助后续改协议时避免把 `@exit`、显式协议请求和裸 shell 行再次混淆;沉淀跨平台修复、Zenoh peer/peer、log 路径等隐性契约教训
  - 何时阅读: 做协议演进、回顾历史判断口径、沉淀经验时;改 daemon 启动日志 / log target 路径 / e2e polling 假设前

- `.codex/skills/rdog-control/SKILL.md`
  - 主题: rdog-control skill,覆盖 Zenoh target-name、line-control、PTY、AX、鼠标和硬件桥接,以及 `rdog ax-diff` 结构化 AX JSON diff
  - 用途: agent-agnostic skill 入口,同时服务 Codex / Claude / GPT / openai-compatible / MCP / 人类。让 `rdog control` 使用约定和协议文档一起版本化
  - 何时阅读: 改 rdog control 相关协议、示例、README、PTY、GUI 控制、ax-diff 子命令,或调整 agent 适用范围前

- `.codex/skills/rdog-control/references/cookbook-web-content.md`
  - 主题: 浏览器当前激活页面内的 Web 内容 AX 操作 cookbook,evidence 模式从"截图前后 diff"切到"AX JSON 结构化 diff"
  - 用途: 固定 `AXWebArea` 优先的网页内容搜索、按钮/链接匹配、`AXPress` 优先、浏览器 chrome 隔离策略,以及用 `rdog ax-diff` 做 action 前/后比对的标准化流程
  - 何时阅读: 用户要检查、列举或点击浏览器当前网页内控件,或要在 AX JSON 层做 action 前/后 evidence 对比时
- `.codex/skills/rdog-control/examples/`
  - 主题: rdog ax-diff 的小红书首页 before/after AX snapshot fixture
  - 用途: 给 cookbook 的 AX JSON diff 章节和 CI smoke 脚本一个可直接复用的最小例子
  - 何时阅读: 需要给 agent / 测试脚本演示 `rdog ax-diff` 输入/输出格式时

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

- `archive/manifests/ARCHIVE_MANIFEST__2026-05-25_rdog_control_notes.md`
  - 主题: 2026-05-25 `$rdog-control` live GUI 点击任务触发默认 `notes.md` 超过 1000 行后的续档说明
  - 用途: 说明旧 `notes.md` 归档位置,以及 Chrome 小红书“首页”按钮坐标 fallback live smoke 的经验沉淀范围
  - 何时阅读: 需要追溯本轮 `rdog control mac.lab` GUI live smoke、`notes.md` 为什么变短,或排查 `@window-activate` session bridge 提前关闭线索时

- `archive/manifests/ARCHIVE_MANIFEST__2026-06-01_computer_use_density_task_plan.md`
  - 主题: 2026-06-01 computer-use density 支线 `task_plan__computer_use_density.md` 续档说明
  - 用途: 说明旧支线 task plan 超过 1000 行后的归档位置,以及 `@web-find target.window_id` / `target.window_ref` 产品化经验沉淀范围
  - 何时阅读: 需要追溯 computer-use density Phase 3F 之后的长计划、window-scoped Web target 验证证据,或确认为何当前支线 task plan 变短时

- `archive/manifests/ARCHIVE_MANIFEST__2026-06-20_worklog_rollover.md`
  - 主题: 2026-06-20 默认 `WORKLOG.md` 超过 1000 行后的续档说明
  - 用途: 说明 2026-05-12 到 2026-06-20 默认 WORKLOG 归档位置,以及 one-shot / Zenoh / GUI control 相关工作记录摘要
  - 何时阅读: 需要追溯 2026-06-20 前默认 WORKLOG 记录,或确认为何当前默认 `WORKLOG.md` 变短时

- `archive/manifests/ARCHIVE_MANIFEST__2026-06-20_branch_context_cleanup.md`
  - 主题: 2026-06-20 根目录旧支线六文件完整整理说明
  - 用途: 说明 23 个旧支线组、90 个支线六文件归档到 `archive/branch_contexts/<suffix>/` 的映射和摘要
  - 何时阅读: 需要追溯 `agent_desktop_review`、`observation_refmap_*`、`computer_use_density`、`xhs_*`、`rdog_*` 等旧支线上下文,或确认根目录为什么只保留默认六文件入口时

- `archive/manifests/ARCHIVE_MANIFEST__2026-06-25_task_plan_rollover_local_default_unixpipe.md`
  - 主题: 2026-06-25 默认 `task_plan.md` 超过 1000 行后的续档说明
  - 用途: 说明 `$oh-my-codex:plan` local-default unixpipe daemon 方案前旧 `task_plan.md` 的归档位置和续档原因
  - 何时阅读: 需要追溯 2026-06-25 local-default unixpipe 规划前的默认任务计划,或确认为何当前默认 `task_plan.md` 重新变短时

- `specs/zenoh-unixpipe-fast-path-plan.md`
  - 主题: rdog control macOS / Linux 本机 fast path(Zenoh `transport_unixpipe`)规划
  - 用途: 固定"同机 daemon + control 自动走 Zenoh unixpipe FIFO,失败透明 fallback 到 UDP/TCP"的契约,以及 FIFO base 路径推导、local-default registry、daemon/client 行为边界、错误处理、验收标准;包含 2026-06-21 加的 `self` / 空 target 入口设计和 2026-06-25 加的 local-default 默认 daemon 规则
  - 何时阅读: 修改 `src/zenoh_runtime.rs` / `src/zenoh_control.rs` / `src/config.rs` / `src/main.rs` 的 unixpipe 相关逻辑、`Cargo.toml` 的 `transport_unixpipe` feature、`rdog_macos.toml` / `rdog_linux.toml` 模板,或排查"同机 ping 慢" / "FIFO 创建失败" / "远端 fallback 是否生效" / "`rdog control self` 找不到 daemon"前

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

- `specs/rdog-computer-use-density-plan.md`
  - 主题: computer-use 类 GUI/Web 任务的高密度 primitive 与 bench suite 规划
  - 用途: 固定 `@gui-probe`、`@web-find`、`@web-act`、`@gui-act`、`@gui-bench` 的演进方向,以及用 `backend_request_count` / `agent_decision_points` 等指标衡量任务密度
  - 何时阅读: 准备减少 agent 手动串联 `@ping` / `@capabilities` / `@window-find` / `@ax-get` 等低级请求,实现 Web/GUI 高密度任务 primitive 或 bench baseline 前

- `specs/rdog-ui-script-control-plan.md`
  - 主题: UI script / control-flow JSON DSL 规划
  - 用途: 定义 iced_emg-compatible JSON DSL 如何适配 rdog control frames,包括 CLI-side runner、step 映射、trace/artifacts、安全策略和后续 daemon-side `@ui-flow` 边界
  - 何时阅读: 修改 UI script runner、control script、GUI 自动化 DSL、`@ui-flow`、脚本验证、坐标回放或语义动作编排前

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

- `specs/rdog-display-scope-control-plan.md`
  - 主题: 多显示器 display scope resolver 与 action guard 控制方案
  - 用途: 固定请求侧唯一使用 `scope:{display:{...}}` / `guard:{display:{...}}`,支持 `id`、`name_contains`、`contains_point`、`window_id`、`window_ref + observation_id` resolver,并明确 `display_id` 只作为 resolved identity 返回
  - 何时阅读: 修改 `src/control_display_scope.rs`、`@observe`、`@window-find`、`@ax-find`、`@web-find`、mouse display guard、`@bootstrap` nested observe scope,或排查多显示器目标过滤和误点保护前

- `specs/rdog-display-aware-control-chain-plan.md`
  - 主题: display scope、window identity、focus verification、targeted AX/scoped visual与post-action evidence的完整控制链
  - 用途: 固定 `@window-activate guard/verify`、`@ax-find.window`、single-display observe artifact和fresh reobserve验收口径
  - 何时阅读: 实现或评审多显示器GUI动作链、目标窗口定向采集、动作后验证,或排查双屏误控/错误成功报告前

- `specs/rdog-mouse-control-coordinate-plan.md`
  - 主题: `@mouse-move` / `@mouse-button` / `@click` / `@drag` / `@wheel` 鼠标控制方案
  - 用途: 固定鼠标控制必须复用 screenshot manifest 的 `os-logical` 坐标语义,以及 press/release、click、drag、wheel 的协议字段、错误边界和验证矩阵
  - 何时阅读: 准备实现或评审鼠标移动、点击、拖拽、滚轮、button press/release,或排查多显示器鼠标坐标偏移前

- `specs/rdog-ax-screenshot-manifest-control-plan.md`
  - 主题: `@screenshot` manifest 集成 macOS AX 窗口/UI 元素结构,以及 `@ax-tree` / `@ax-press` AX control 方案
  - 用途: 固定 `include_ax`、`ax_required`、`rdog.ax.v1` manifest schema、Accessibility 权限降级语义、AXPress target locator 和错误映射
  - 何时阅读: 准备实现或评审 AX screenshot manifest、`@ax-tree`、`@ax-press`、Accessibility 权限提示,或排查 AX 元素坐标/定位歧义前

- `specs/rdog-observation-scoped-refmap-plan.md`
  - 主题: observation-scoped refmap、durable selector、semantic re-find、`@observe` 与 mouse ref 化的长期路线图
  - 用途: 固定 GUI observation 的短期 ref / 长期 selector / 重启恢复 / 语义重找 / 统一观察入口 / mouse fallback 当前契约和完整演进线,避免只做最小可用版后丢失后续目标
  - 何时阅读: 修改 `src/control_observation*`、`src/control_mouse*`、`@observe`、selector 恢复、mouse ref target,或更新 `rdog-control` skill / README / control protocol 文档前

- `specs/rdog-non-mouse-semantic-control-plan.md`
  - 主题: `@ax-action` / `@ax-set-value` / `@type-text` / `@key delivery` / `@ax-focus` / `@ax-scroll` 的非鼠标语义控制协议
  - 用途: 固定“非鼠标优先”协议能力,明确 `@ax-press` 兼容映射、AXValue 写入边界、`targeted-keyboard` / clipboard opt-in、`@key` 定向投递,以及 `@ax-focus activate:true` 复用 `@window-activate` 的边界
  - 何时阅读: 准备实现或评审非鼠标 GUI 控制、更新 `rdog-control` skill,或判断某个交互是否该先走 AX/value 而不是鼠标前

- `specs/rdog-window-control-plan.md`
  - 主题: `@window-find` / `@window-activate` / `@window-close` / `@window-resize` 的窗口状态、生命周期和窗口尺寸控制方案
  - 用途: 固定截图不可见窗口的 agent 工作流、window state schema、graceful/terminate/kill 关闭边界、hidden/minimized/occluded/cross-space 的诚实状态语义,以及 `WindowSize` 应如何通过默认恢复/激活目标窗口的 `@window-resize` 进入 control plane
  - 何时阅读: 准备实现或评审窗口发现、窗口激活、窗口关闭、窗口 resize、被遮挡窗口交互,或更新 `rdog-control` skill / UI script 的窗口控制指引前

- `specs/bidirectional-control-plane-plan.md`
  - 主题: 控制面从单向 request/reply 升级为真正双向 control peer 的规划
  - 用途: 固定“daemon 和 control 双方都能主动发送 `@key` / `@script` / `@savefile` 等显式控制指令,Zenoh 不再停留在单向 query/reply”这一轮新的架构方向
  - 何时阅读: 准备继续实现 `@savefile`、`@screenshot`、daemon 主动下发控制指令,或评审 TCP/WebSocket/Zenoh 控制模型是否仍然单向时

- `specs/control-frame-refactor-plan.md`
  - 主题: control core / outbound frame / session 抽象的分阶段重构计划
  - 用途: 固定从 `String @response` 返回模型演进到 `ControlExecutionOutcome + ControlFrame + ControlPeerSession` 的实施顺序,以及 TCP/WebSocket 先行、Zenoh 后续迁移的验证路径
  - 何时阅读: 准备开始写双向控制代码、评估先改哪层抽象、或 review `@savefile`/双向 frame 改造顺序时
