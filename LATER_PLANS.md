## [2026-03-21 14:43:41] [Session ID: codex-20260321-143912] 主题: 初始化后值得优先推进的后续事项

### 背景
- 本轮只完成了仓库初始化和基线建立,没有改动业务逻辑。
- 初始化过程中已经暴露出几个适合后续推进,但不应在无明确需求时直接动手的方向。

### 后续事项
- 为 `listen` 和 `connect` 的参数解析补充更完整的单元测试,不只覆盖 Unix 非法端口这一条路径。
- 评估 `src/unixshell.rs` 中 `unsafe` fd 接管逻辑的可维护性和资源释放语义,必要时做一次专门的健壮性审查。
- 补一份面向开发者的架构说明或 README 开发章节,降低下次接手时的重新理解成本。

## [2026-04-02 01:00:17] [Session ID: codex-20260402-001922] 主题: command-script-intercept 二期增强候选

### 背景
- 首版已经交付:
  - `@key`
  - `@paste`
  - `@script`
  - `listen` / `connect` / `daemon` 的显式 control-capable lanes
- 当前阻断级测试已经够用,但还有几条很值得后续补强的路线。

### 后续事项
- 细化 `@key` / `@paste` 的 unsupported-backend 失败契约测试,锁住 exit code 和 stderr 文案。
- 评估是否要把 `@paste` 从“直接 keystroke 文本注入”升级成更贴近真实 paste 的平台实现。
- 评估是否要给 README / `cmd.md` 增加轻量 help / 文档 smoke test,防止后续文档口径漂移。
- 评估是否需要新增独立的 `attach` / `client` 模式,让用户可以只用 `rcat` 自己去接入 `daemon inbound` 暴露出来的 bind shell,而不是依赖 `nc` 这类普通 TCP 客户端。

## [2026-04-02 02:57:57] [Session ID: omx-1775069552136-j9r53i] 主题: 为 `@key` 增加可观测 GUI 焦点夹具

### 背景
- 当前已经拿到两类证据:
  - 解析层 / 路由层单测通过
  - live `enigo` backend 在本机返回 `@exit 0`,且日志确认具备输入模拟权限
- 但 `@key` 和 `@paste` 的天然问题是: 它们不像 `@script` 那样自带 stdout,因此“最终 GUI 效果”还缺自动化观测夹具。

### 后续事项
- 评估是否要补一个专门的 macOS GUI smoke harness:
  - 打开可控文本输入目标
  - 触发 `@key` / `@paste`
  - 自动采集目标内容或窗口状态变化
- 如果要做,优先把它设计成可选的本机 smoke test,不要强行塞进默认 CI。

## [2026-04-02 08:27:31] [Session ID: omx-1775069552136-j9r53i] 主题: control lane 应把 `@key` 执行期错误回传给客户端

### 背景
- 当前 `@key:"right-option"` 这类“不支持的键名”会在服务端 `execute_key()` 阶段失败。
- 现象是:
  - daemon 日志里有真实错误
  - `rcat control` 本地只看到 `control connection closed`
  - 退出码仍然是 `0`

### 后续事项
- 评估是否要把 built-in control action 的执行期错误也包成协议响应返回,例如沿用 simple-command 风格返回 stderr + `@exit 64`。
- 这样用户就不用去翻 daemon 日志才能知道 `@key` 为什么失败。

## [2026-04-02 08:38:04] [Session ID: omx-1775069552136-j9r53i] 主题: 已完成清理 - control action 错误回传

### 背景
- 之前登记过“control lane 应把 `@key` 执行期错误回传给客户端”。

### 当前状态
- 本轮已实现:
  - built-in control action 执行期错误会回传 stderr 和 `@exit <code>`
  - `hyper` 这类非法键名现在能被客户端直接看到
- 该项可视为已完成,后续无需再把它当成未落地事项。

## [2026-04-06 11:20:00] [Session ID: omx-1775467041829-gy5cxx] 主题: Zenoh router/client + serial 迁移后的补强项

### 背景
- 当前主路径已经从 `peer/peer` 迁成:
  - `daemon = router`
  - `control = client`
  - 默认 autodiscovery, `--entry-point` 仅作 fallback
- 本地验证已经覆盖:
  - `cargo build`
  - `cargo test -- --test-threads=1`
  - Unix 下 router/client 集成测试
- 但仍有两类证据缺口没有在本轮完全补齐。

### 后续事项
- 在真实 ESP32 / 串口环境上跑一次 hardware smoke,确认:
  - serial endpoint 真能接入
  - daemon router 日志能观测到节点加入
  - control 侧仍能在同一网络里完成 `@ping` / `@cmd#id`
- 在 Windows 现场真正执行新的 `tests/zenoh_router_client_windows.rs` 对应 smoke,不要只停留在编译通过。
- 评估是否要为 autodiscovery / fallback 行为补更细的可观测日志,明确区分:
  - 当前是 autodiscovery 命中
  - 还是 `--entry-point` fallback 命中
## [2026-04-13 19:41:16] [Session ID: codex-20260413-zenoh-timeout] 主题: 为 Zenoh autodiscovery 拆出独立 timeout 配置

### 背景
- 当前为修复 Windows 多 locator autodiscovery 超时,在无 `--entry-point` 时新增了 manual scout + locator 排序。
- 当前实现先复用了 `request_timeout_ms` 作为 discovery 窗口,这样能在不扩 CLI/config 的前提下尽快落地修复。

### 后续事项
- 评估是否为 control / daemon 补一个独立的 `zenoh.discovery_timeout_ms`。
- 如果后续出现“请求超时想调长,但 discovery 不想一起变长”的现场,就把这项从备忘转成正式实现。

## [2026-05-01 14:24:00] [Session ID: 019de364-f2af-7432-ad6a-40552af185c8] 主题: 若后续实现远程截图,先补结果载荷设计而不是先开第二控制面

### 背景
- 本轮已判断“远程请求本地 daemon 截图”更适合走现有显式 control 协议,例如 `@screenshot`。
- 但当前 `control_core` 成功响应模型仍偏向 `0` / `stdout` / `stderr`,对截图这种二进制结果还缺一个正式 value contract。

### 后续事项
- 若进入实现阶段,优先先定 `@screenshot` 的成功响应形态:
  - 小图是否直接 base64 内嵌
  - 大图是否返回 metadata + 独立 result keyexpr / 临时文件引用
- 若确实需要旁观订阅或大结果分发,再新增 screenshot result pub/sub keyexpr,但不要把 request 入口改成订阅频道主入口。
- 如果 macOS 屏幕录制权限成为实测阻塞点,再评估是否需要单独 helper 进程; 在那之前不要先拆新 bin。

## [2026-05-05 17:59:16] [Session ID: codex-20260505-pty-design] 主题: 后续实现 `@pty` / `rcat control --pty` 远程交互会话

### 背景
- 用户需要 `codex` 这类要求真实 TTY 的程序能通过 `rcat control` 远程执行。
- 当前裸 shell 行是一次性 `Command::output()` 模型,不提供 PTY,所以裸 `codex` 会报 `stdin is not a terminal`。

### 后续事项
- 设计协议层 `@pty` 作为单一真相源,再提供 CLI 入口 `rcat control TARGET --pty -- COMMAND ...`。
- `@pty` session 期间,普通键盘输入默认全部进入 PTY stdin,不能再按 line-control 解析 `@key` / `@script`。
- 不保留 in-band escape 入口:
  - `~.` / `~?` / `~~` 这类字符序列也可能被 `codex`、shell、vim、REPL 等远端 TUI 使用
  - 第一版 PTY 输入流必须尽量字节透明,不要在用户输入中偷藏 control 命令
- 明确 `Ctrl-C`、`Ctrl-D` 等按键默认发给远端 PTY 程序,不直接退出 `rcat control`。
- 强制关闭 / detach 应走 out-of-band 控制面,例如另开一个 control 请求 `@pty-close#id` 或 `rcat control TARGET --pty-close SESSION_ID`,不要复用 PTY 输入流内的 escape。
- 非交互大模型任务仍优先建议走 `codex exec ...`,只有需要 TUI 时才进入 `@pty`。

## [2026-05-05 18:58:00] [Session ID: codex-20260505-pty-implementation] 主题: 已完成清理 - `@pty` / `rcat control --pty`

### 背景

- 之前登记过“后续实现 `@pty` / `rcat control --pty` 远程交互会话”。

### 当前状态

- 本轮已实现:
  - `@pty` / `@pty-close`
  - TCP / WebSocket / Zenoh session channel PTY
  - PTY 输入透明
  - out-of-band close
  - focused tests 和文档同步
- 该项可视为已完成,后续无需再把它当成未落地事项。

## [2026-05-11 22:08:00] [Session ID: omx-1778469026342-c6n34v] 主题: rdog 发布远端和外部包名迁移

### 背景

- 本轮已把项目源码、Cargo metadata、README 和安装脚本口径改为 `rustdog` / `rdog`。
- 已验证 `aur.archlinux.org/rustdog.git` 存在,并把 `.gitmodules` 切到新地址。
- 但 `git ls-remote git@github.com:raiscui/rustdog.git HEAD` 当前返回 `Repository not found`。

### 后续事项

- 创建或迁移 GitHub 仓库 `raiscui/rustdog`。
- 迁移 GitHub release、wiki、badge、installer 依赖的 release asset 发布面。
- 如果仍需要保留旧 `raiscui/rustcat` 或 `robiot/rustcat` 的来源说明,应在 README 中明确写成 upstream/legacy,不要混入新主路径。

## [2026-05-12 11:54:07] [Session ID: codex-app-2026-05-12-rustdog-repush] 主题: rdog GitHub 远端创建项已落地,但 release 发布面仍未迁移

### 背景

- 旧记录里有一项“创建或迁移 GitHub 仓库 `raiscui/rustdog`”。
- 本轮已经按 fresh init 状态重新创建 public 仓库并推送 `main`。

### 当前状态

- 已完成:
  - 创建 `https://github.com/raiscui/rustdog`
  - 设置 `origin = git@github.com:raiscui/rustdog.git`
  - 推送 `main`
  - 验证远端 HEAD 与本地 HEAD 一致
- 未在本轮处理:
  - GitHub release 迁移
  - wiki 迁移
  - badge / installer / 外部 release asset 依赖面迁移

### 后续事项

- 如果要正式对外发布,下一轮应检查 README badge、安装文档、release workflow 和旧仓库跳转说明。
- 不要把本轮 fresh init push 误记成完整 release 迁移。

## [2026-05-13 14:06:58] [Session ID: codex-native-unknown] 主题: macOS `@screenshot` 需要避免 desktop-only 假成功

### 背景

- 用户观察到 `@screenshot` 生成的 JPEG 只有桌面,没有可见窗口。
- 当前实现中 `sck-rs` 主路径失败后会 fallback 到 `xcap`。
- `xcap` monitor capture 路径可能拿到被 macOS 隐私权限裁剪后的桌面-only 图,但仍作为成功图片返回。

### 后续事项

- 在 macOS 截图前补 Screen Recording 权限 preflight 或窗口可见性 probe。
- 当权限不足或无法确认窗口内容可捕获时,应返回 `PermissionDenied`,不要继续保存 desktop-only 图。
- 可以考虑让 `@screenshot` response metadata 标注实际 backend,方便后续区分 `sck-rs` 主路径和 `xcap` fallback。

## [2026-05-13 18:26:13] [Session ID: omx-1778661154642-agn8qc] 后续计划: 将 ralplan 方案转为可提交 specs 文档

### 背景
- 当前多显示器截图坐标方案产物位于 `.omx/plans/`,该目录被 `.gitignore` 忽略。

### 建议后续动作
- 如果这份方案需要作为仓库长期规格,将核心内容整理到 `specs/rdog-multi-display-screenshot-coordinate-plan.md` 或合并进 `specs/zenoh-screenshot-control-plan.md`。
- 同步更新 `AGENTS.md` 长期知识索引,说明何时阅读该 specs 文件。

### 当前状态
- 本轮不执行迁移,因为用户当前请求是 `$ralplan` 指定 `.omx/plans/...` 产物,不是提交长期 specs。

## [2026-05-13 20:13:46] [Session ID: omx-1778661154642-agn8qc] 后续计划: 基于 screenshot manifest 实现鼠标点击和拖拽

### 背景
- 多显示器  已默认返回 virtual desktop JPEG + manifest JSON。
- manifest 已定义 、 和 ,可以把截图像素坐标稳定换算成 OS logical 鼠标坐标。

### 后续事项
- 实现  /  时,直接复用 manifest 的  坐标语义。
- 不要新增第二套屏幕坐标解释,也不要让 agent 只凭大图猜显示器偏移。
- 如需要单屏调试模式,可以后续追加  或 debug-only 输出,但不应改变默认 composite 契约。

## [2026-05-13 20:14:51] [Session ID: omx-1778661154642-agn8qc] 更正记录: 基于 screenshot manifest 实现鼠标点击和拖拽

### 说明
- 上一条同主题 LATER_PLANS 记录因未加引号 heredoc 丢失了反引号内字段。
- 本条为准。

### 背景
- 多显示器 `@screenshot` 已默认返回 virtual desktop JPEG + manifest JSON。
- manifest 已定义 `virtual_bounds`、`display.image_rect` 和 `display.os_rect`,可以把截图像素坐标稳定换算成 OS logical 鼠标坐标。

### 后续事项
- 实现 `@click` / `@drag` 时,直接复用 manifest 的 `os-logical` 坐标语义。
- 不要新增第二套屏幕坐标解释,也不要让 agent 只凭大图猜显示器偏移。
- 如需要单屏调试模式,可以后续追加 `layout:"per-display"` 或 debug-only 输出,但不应改变默认 composite 契约。

## [2026-05-18 17:09:25] [Session ID: codex-phase4-20260518-163845] 后续计划: `rdog doctor` 复用 `@capabilities` 模型

### 背景
- Phase 4 已先实现协议层 `@capabilities`,让远程 daemon 能返回 `rdog.capabilities.v1`。
- 本轮没有新增 `rdog doctor` CLI,避免 CLI 和 protocol 同时定义两套权限语义。

### 后续事项
- 如果继续做 `rdog doctor`,应复用 `src/control_capabilities.rs` 的 report model。
- doctor 可以增加本地 CLI 呈现、权限恢复提示和退出码,但不要重新发明 macOS Accessibility / Screen Recording、Windows UIPI、Linux backend 的状态字段。
- 后续如把 capability report 暴露给 SDK conformance fixtures,也应以 `rdog.capabilities.v1` 为唯一 schema。

## [2026-05-25 09:10:00] [Session ID: omx-1779670884813-rnokx6] 后续计划: 调查 `@window-activate` over Zenoh session bridge 提前关闭

### 背景
- 本轮 `$rdog-control` live GUI 点击任务中,`@window-activate#4:{window_id:"pid:8231/window:0"}` 单独执行和与 `@screenshot` 同 session 执行时,都返回 `Zenoh session bridge subscriber 在收到结果前关闭`。
- 同一临时 daemon 下,`@ping`、`@capabilities`、`@observe`、`@screenshot`、`@click` 均可用。

### 后续事项
- 为 `@window-activate` 增加 focused live smoke 或最小集成测试,确认 daemon 是否实际执行了 action 但没有回传 frame。
- 对比 `@click` / `@screenshot` 的 session-channel 返回路径,检查 `window_control` action 是否遗漏 terminal frame 或提前关闭 session bridge。
- 修复时不要把普通 `@click` 改成隐式激活路径。窗口激活仍应保持显式 `@window-activate`。

## [2026-05-25 10:25:20] [Session ID: omx-1779670884813-rnokx6] 完成记录: `@window-activate` over Zenoh session bridge 提前关闭已修复

### 对应旧计划
- 对应 `2026-05-25 09:10:00` 的“调查 `@window-activate` over Zenoh session bridge 提前关闭”。

### 完成结果
- 已修复 `src/zenoh_control/client_pty.rs::execute_remote_request()` 中把 Zenoh FIFO `recv_timeout()` 的 `Ok(None)` 误判为 subscriber closed 的问题。
- 已增加 `tests/zenoh_router_client.rs::control_should_wait_for_slow_session_channel_response` 防回归。
- 已用 live `@window-activate` over Zenoh session bridge 验证返回 `status:"ok"`。

### 备注
- 本文件遵守追加记录,未删除历史计划原文。后续整理 LATER_PLANS 时可将旧计划与本完成记录一起归档。

## [2026-05-26 10:18:30] [Session ID: omx-1779670884813-rnokx6] 后续计划: 调查 `@ax-find` 对 Chrome WebArea 深层 description 的 false negative

### 背景
- 小红书左侧导航按钮在 `@ax-get` 深读 `AXWebArea` 时可以捕捉到,形式为 `AXLink.description`。
- 但 `@ax-find` 使用 `description_contains:"首页"` / `"点点"` / `"直播"`,即使 `depth:10,max_elements:3000,truncated:false`,仍返回 `match_count:0`。

### 后续事项
- 检查 `@ax-find` 的搜索对象是否只遍历了某个 flattened summary,而没有覆盖 `@ax-get` 能拿到的 WebArea 深层节点。
- 增加针对 `AXLink.description` 的 focused fixture 或 live regression。
- 目标是让 agent 能直接用 `@ax-find description_contains` 定位网页内语义链接,减少必须手工深读 WebArea 的步骤。

## [2026-06-19 01:05:00] [Session ID: CURRENT_SESSION] 主题: rdog control one-shot 入口落地后续

### 背景
- 2026-06-19 已经完成 `rdog control <target> @<line>` 入口
- 落地时识别出 2 类值得后续补的硬化点

### 后续事项
- [DONE 2026-06-19] 给 Zenoh one-shot 入口补 e2e(同 02:05 合并到 3 个 `control_multi_one_shot_*` 测试,覆盖 2/3 line 顺序 + robust 3 line 烟测)
- [DONE 2026-06-19] `src/main.rs::init_logger` 走 stderr;4 个 e2e 已修(control_lanes::listen_local, control_pty::detach_attach, shell_pty::reverse_shell);Zenoh e2e 24+ 用 sh -c "exec rdog ... 2>&1" 兼容层处理;后续可把兼容层退役,统一改测试用合流 buffer

## [2026-06-19 02:05:00] [Session ID: CURRENT_SESSION] 主题: rdog control 多 line one-shot 落地后续

### 背景
- 2026-06-19 完成多 line one-shot 落地 (N=1..32,共享一条 transport)
- 与 01:05 同日条目里的 Zenoh one-shot 单 line e2e 一起,后续可一并补

### 后续事项
- [DONE 2026-06-19] 已合并到 01:05 的"已 done"条目
  - 拉一个 router fixture,启 daemon profile,跑 `rdog control <target> @ping @capabilities#1` 验证 Zenoh session bridge 共享 + 多 frame 收口 + 顺序输出
  - 顺便补单 line Zenoh e2e(01:05 登记的旧 follow-up)
- 32 上限是否合理需要观察:如果 agent 实际跑 GUI 任务需要更多 line,提到 64 或 128;如果从来用不到 30,降到 16

## [2026-06-19 05:05:00] [Session ID: CURRENT_SESSION] 主题: sh wrapper 退役 + 合流 helper 落地后续

### 背景
- 2026-06-19 已完成 `start_zenoh_daemon_with_combined_output` 落地,sh wrapper 退役
- 24+ Zenoh e2e 改用合流 buffer,不再依赖 stdout 上的 log marker

### 后续事项
- (无,本任务清理完毕)
- 历史上 LATER_PLANS 里 1 条 "sh wrapper 临时兼容层退役" 候选任务已 done

## [2026-06-20 12:10:00] [Session ID: omx-1781926953468-5fb1e6] 后续计划: 完整整理根目录旧支线六文件

### 背景
- 本轮因默认 `WORKLOG.md` 超过 1000 行, 只执行了最小安全续档。
- 工作区仍存在多个带后缀的旧支线上下文文件, 本轮没有展开完整 `continuous-learning` 清理, 避免偏离用户要求的 skill 更新任务。

### 后续动作
- 单独开一次完整 `continuous-learning` 任务, 按后缀分组读取根目录旧支线六文件。
- 判断哪些支线仍活跃, 哪些应归档到 `archive/branch_contexts/`。
- 若产生新的长期知识, 同步更新 `EXPERIENCE.md` 与 `AGENTS.md` 索引。

## [2026-06-20 18:47:00] [Session ID: omx-1781934324141-q2nzhz] 完成记录: 根目录旧支线六文件整理已执行

### 对应旧计划
- 对应 `2026-06-20 12:10:00` 的"完整整理根目录旧支线六文件"。

### 完成结果
- 已按后缀分组检索根目录支线六文件。
- 已将 23 个旧支线组、90 个文件归档到 `archive/branch_contexts/<suffix>/`。
- 已生成 manifest: `archive/manifests/ARCHIVE_MANIFEST__2026-06-20_branch_context_cleanup.md`。
- 已在 `AGENTS.md` 为新 manifest 建立索引。

### 备注
- 默认六文件仍为当前活跃入口,没有归档。
- 如果后续还要提交本轮 continuous-learning 结果,应单独做 scoped commit,不要和业务代码改动混合。

## [2026-06-20 23:55:00] [Session ID: omx-1781788115552-szl2hn] 后续建议

### rdog control macOS 本地 fast path 收尾(2026-06-21 大部分完成)

- [x] 把 `specs/zenoh-control-plane-plan.md` 补上 "Local fast path: unixpipe" 节,把 unixpipe exists-check 契约写进去
- [x] `EXPERIENCE.md` 沉淀经验
- [x] `rdog_linux.toml` 模板加同样的 `[zenoh.unixpipe]` 注释段
- [x] 把 plan 文件 `.omx/plans/zenoh-unixpipe-fast-path.md` 同步成"实际采用 exists-check"
- [x] **`rdog control self @<line>` / 空 target 入口**:2026-06-21 已实现
- [x] `.codex/skills/rdog-control/SKILL.md` 补 troubleshooting 段
### 已有 flake 待处理

- 已由 2026-06-21 17:35:00 的诊断记录收敛,本条不再重复挂 unchecked 项。

## [2026-06-21 16:30:00] [Session ID: omx-1781788115552-szl2hn] 完成记录: 2026-06-21 本机 fast path 全链路收尾

### 对应旧计划
- 对应 `2026-06-20 23:55:00` "rdog control macOS 本地 fast path 收尾"。

### 完成结果
- `rdog control self @<line>` / 空 target 入口已实现(`d3fdc9b`)
- `.codex/skills/rdog-control/SKILL.md` 的 "Local Fast Path Troubleshooting" 段已补(`ffa169d`)
- `references/control-workflow.md` 的本机 fast path 章节已加
- `EXPERIENCE.md` 沉淀 6 条本轮经验(self/empty 入口 3 条 + unixpipe 实现细节 3 条)
- `EPIPHANY_LOG.md` 沉淀 3 条本轮 EPIPHANY(FIFO 不是 socket、pre-existing flake、init_logger stdout)
- 7 个 e2e 全过(4 个 self/empty + 3 个原 unixpipe);369 unit + 26 zenoh_router_client 全过

### 后续事项
- 方向 B 已统一保留在当前未完成事项清单。
- `zenoh_router_client` flake 已由 2026-06-21 17:35:00 的诊断记录继续跟踪。
- 路径上限 100 vs 95 字节口径已经完成同步,当前源码和规格都使用 95 字节。

## [2026-06-21 17:35:00] [Session ID: omx-1781788115552-szl2hn] 完成记录: zenoh_router_client flake 排查(本轮诊断收敛,不实施修复)

### 对应旧计划
- 对应 `2026-06-20 23:55:00` 后续建议里的"`zenoh_router_client` 测试集 ~4% 多测试并发 flake 排查"。

### 完成结果
- 跑了对照实验(串行/2/4/8 threads × 5~30 次):4 threads 以下稳定 0 fail,8 threads 偶发失败。
- 捕获 2 次真实失败,锁定 2 个候选根因:
  - PTY polling timeout 紧张(900ms 窗口)
  - Zenoh 端口 race(`next_port()` drop → OS 状态窗口期)
- 50 次 8 threads 0 fail 决定本轮不实施 surgical 修复。
- 诊断结论 + 4 个候选修复方向 + 推荐顺序沉淀到 `EPIPHANY_LOG.md` 2026-06-21 17:30:00 条目。
- 用户决策(2026-06-21 17:35):维持现状,因为之前问题可能是用户操作干扰和权限问题。

### 后续事项
- [ ] 等待 flake 再次自然复现时(根据本机负载 / 后台进程 / macOS 状态变化大概率会再现)
- [ ] 复现后照 `EPIPHANY_LOG.md` 2026-06-21 17:30:00 的"推荐顺序"推进
- [ ] port guard surgical 修复(只动 `start_zenoh_daemon_with_combined_output` helper)作为首选,只改 1 处
## [2026-06-24 19:48:00] [Session ID: native-hook-20260624-193730] 后续建议: 多显示器 display scope 控制

### 背景
- 用户指出 `rdog` 当前缺少约束和指定 display 的控制。
- 现有规格已有 `@screenshot display:"all" / "primary"` 和 `os-logical` manifest 坐标契约,但缺少贯穿 `@observe`、AX/window/web find、mouse ref target 的 display scope。

### 建议后续动作
- 新增或扩展规格: 把 display 约束建模为 `ObservationScope` 的一部分。
- 在 `@bootstrap` / `@observe` response 中返回 displays summary 和稳定 `display_ref`。
- 让 `@observe`、`@window-find`、`@ax-find`、`@web-find` 支持 `scope.display` 或 `display` filter。
- 让 mouse 坐标 fallback 支持 display guard,避免坐标落到其他屏或 gap。
- 同步更新 `.codex/skills/rdog-control/SKILL.md`,让 agent 默认先选 display scope 再操作 GUI。

### 推荐优先级
1. 先做只读能力: displays summary + scoped observe。
2. 再做查询过滤: window/AX/web find 继承 display scope。
3. 最后做执行 guard: mouse/action 验证 target 是否仍在同一 display scope。

## [2026-06-28 19:44:30] [Session ID: codex-20260628-goal-ui-script-runner-1234] 后续建议: 当前保留的未完成事项

### 背景

- 2026-06-25 到 2026-06-28 的 local-default、`@window-resize`、UI script fixture、`WindowSize mode:"resize"`、installed daemon、最小 `rdog ui-script run`、trace/artifacts 和最小真实 `Expect` 已陆续落地。
- 这些完成事实已记录在 `WORKLOG.md` / `notes.md` / `task_plan.md`,不再继续作为 `LATER_PLANS.md` 的 unchecked future items。

### 后续事项

- [ ] 方向 B(直接 UDS 控制面,10~50x 提速)仍是独立性能路线,不属于当前 UI script runner 收口。
- [ ] 增加 `--compat iced-emg` 和更细的 policy flags。

## [2026-06-28 23:04:11] [Session ID: codex-20260628-plan-daemon-flow] 后续计划: task_plan 续档后的完整 continuous-learning

### 背景

- `task_plan.md` 已达到 1000 行,本轮为继续建立 `@flow` plan 已做最小安全续档。
- 旧文件已归档到 `archive/default_history/task_plan_2026-06-28_205227_before_flow_plan.md`。

### 后续事项

- [ ] 在当前 `@flow` plan 主线完成后的安全点,单独执行一次完整 continuous-learning。
- [ ] 回读本轮归档的旧 `task_plan.md` 与当前六文件,提炼 UI script / @flow / window control 相关长期经验。
- [ ] 判断是否需要同步 `EXPERIENCE.md`、`AGENTS.md`、`specs/rdog-ui-script-control-plan.md` 或新的长期索引。

## [2026-06-30 00:56:17] [Session ID: codex-20260629-ultragoal-ui-script-123] 后续建议: UI script 非 TCP live smoke

### 背景
- G004 final gate 已用 TCP control daemon 完成真实 live smoke。
- code-reviewer 和 architect 都确认当前 gate 可通过。
- architect 复审建议: 如果未来想继续压低 transport 覆盖风险,可以补 WebSocket 或 Zenoh 的 live smoke,但不要作为本轮 gate 阻塞项。

### 后续事项
- [ ] 后续新增一个 WebSocket live smoke 或 Zenoh local-default live smoke,复用同一份 `@ping + Expect` UI script。
- [ ] smoke 仍应检查 `trace.jsonl`、`summary.json`、`script.normalized.json`、`artifacts/` 和端口 / session 清理。

## [2026-07-14 11:05:00] [Session ID: omx-1783957580965-m4bn8e] 主题: rdog `@computer-act` 设计收口后续事项

### 背景
- 完成了 Mano-CUA 16 动作与 rdog 协议对齐的 15 题 grill session, 产出 6 个 ADR (`docs/adr/0001` 到 `0006`) + glossary (`docs/glossary.md`)。
- 决策见 ADR-0001~0006, 本次只记本轮不落地但值得后续推进的事项。

### 后续事项

- **LP1: 跨平台 (`@open-app` / `@wait` / `@paste` / `@click` / `@key` 在 Windows + Linux)**。当前 `@open-app` 只规划了 macOS (`@cmd "open -a ..."`), Windows 的 `start` / Linux 的 `xdg-open` 需要分别适配。Windows 上 enigo 触发 `@click / @key` 还要过 UIPI, rdog 现有 `looks_like_windows_uipi_permission_denied` 已经踩过坑。第一版只交付 macOS 路径, 其他平台让 `@open-app` 返回 `platform_unsupported` 错误码 (走 E2 envelope)。

- **LP2: Holo 3.1 / EvoCUA / GTA1 的协议适配**。`@computer-act` 当前 schema 是 Mano-CUA 闭集 (16 → 13 daemon 动作)。Holo 3.1 的 click / write / answer 3 动作理论可以直接复用 v1 schema; EvoCUA 的自由 JSON + `<think>` 段需要客户端先 parse 思路但 daemon 端不需要新 schema。GTA1 是 Qwen2.5-VL-7B base, 跟 Mano-CUA 训练目标不同, 可能需要单独的 v2 schema。建议先把 Holo 3.1 接进 `rdog.computer-act.v1` 验证 (3 动作是 v1 的真子集), 再考虑其他模型。

- **LP3: density benchmark suite**。`specs/rdog-computer-use-density-plan.md` §3 定义了 metrics 但没有可跑 benchmark。建议:
  - 准备 5-10 个 Mano-CUA 典型任务 (登录表单 / 浏览器搜索 / 文件操作 / 多步对话框)
  - 写一个 stdlib-only benchmark 脚本, 跑 `@computer-act` 跟 baseline (手写 `@observe + @click + @observe` 多步) 对比
  - 输出 `density` 字段报告, 验证 high-density 路线真实省了 round-trip
  - 用结果佐证 ADR-0001 的 meta 决策

- **LP4: Schema v2 evolution 路径**。`rdog.computer-act.v1` 当前只覆盖 GUI 动作。audio / multimodal 动作如果进来 (Mano-CUA 没, 但 Holo 3.1 audio 通道未来可能扩), schema v2 需要:
  - 加 `modality` 字段 (默认 `"gui"`)
  - 给 audio 通道单独 `verify` 语义 (screenshot 换 audio waveform diff)
  - 保持 v1 client 兼容 (新字段默认 false, 旧字段语义不变)

- **LP5: `@computer-act` 的 rate limit / quota**。第一版不加, 等 LP3 benchmark 跑出来看 hot loop 实际打几次 daemon。如果 agent loop 异常卡死时把 daemon 资源耗尽 (mouse 按住 / wait sleep / implicit_observe cache 累积), 需要:
  - daemon 端 per-target-seq 资源占用上限
  - client 端 rate limit (类似 OpenAI API 的 RPM 限制)
  - 超限时返回 `rate_limited` 错误码 (E2 envelope)

- **LP6: glossary 跟现有 rdog 术语对齐**。`docs/glossary.md` 当前只定义了 `@computer-act` 相关新术语。后续需要 cross-reference:
  - `@observe` / `observation_id` / `@flow` / `AX ref` 等已有术语在 `.codex/skills/rdog-control/SKILL.md` 和 `specs/` 里有定义, glossary 不应重复, 只做 index 链接
  - `density metrics` 字段的来源 (`rdog-computer-use-density-plan.md` §3) 需要在 glossary 里 cite
  - 新增 `error_code` 时同步更新 glossary 的 "retry strategy" 节

### 关联入口
- ADR 主入口: `docs/adr/0001-add-computer-act-meta.md`
- Grill session 主线: fast-infer 项目 `task_plan.md` (本轮 grill 收口记录会追加)
- 实施节奏建议: LP3 先做 (验证 ADR 决策), LP1/LP2 在 LP3 之后按用户需求触发

### LP-ticket-11-deferred: 真实 AX observe 集成 (Phase I ticket 21+)
- 当前 ticket 11 用 synthetic `@e{seq}` ref_id 占位;真实 AX observe (从 `@observe` 拿到 backend ref) 集成留给 Phase I。
- 集成路径:
  1. `apply_implicit_observe_to_args` 从 stub 升级为真函数: 把 args.start_box 拆掉,换成 `target.ref + target.observation_id`,让 click/hover/drag 走真实 ref 路径 (`MouseEndpoint::ObservationRef`)
  2. `ComputerActObservationCache.record_implicit` 调用真实 observe (screenshot + AX tree walk + coords→element mapping),而不是生成 synthetic ref
  3. 启动时间会显著增加 (single observe ~100-300ms),density metrics 字段 (`ticket 17`) 需要把 `implicit_observe_ms` 暴露出来

### LP-ticket-11-deferred-2: stale_fallback_to_coords 真实实现
- 当前 `ImplicitObserveOutcome::StaleFallbackToCoords` 是预留 enum variant,ticket 11 不暴露 (real observe 还没接入)。
- 真实实现路径: 当 real observe 失败 (e.g., screenshot capture 失败 / AX tree 为空),从 `StaleReObserved` 降级到 `StaleFallbackToCoords`,response 里给 `error_code: "observe_unavailable"` + `retry.strategy: "re_observe_then_retry"`。

### LP-ticket-11-deferred-3: implicit_observe 并发安全
- 当前 `OnceLock<Mutex<...>>` 走 Mutex 兜底,但 rdog dispatcher 单线程 (verified by control_computer_act/mod.rs 的 dispatch 路径),未来若 rdog 改成 async dispatcher / 多 worker,要审计 Mutex 是否变瓶颈。
- 替代方案: rdog dispatcher 单线程 → 直接用 `RefCell` 替代 `Mutex`,省一次 lock 开销;但要 audit 整个 control_actions.rs 链路确认没有跨 await 持有 cache ref。

### LP-ticket-13-deferred-1: ax_diff facade 抽取
- 当前 `verify.rs` 直接调 `crate::ax_diff::diff::compute_diff`,跟 ax_diff 内部 API 紧耦合。
- 后续重构期抽 facade: `ax_diff::run_between_snapshots(before: AxSnapshot, after: AxSnapshot) -> AxDiffSummary`,
  verify 模块只调 facade,ax_diff 内部可以换实现 (e.g. 异步 / 增量 diff) 而不破坏 verify 契约。
- 触发条件: ticket 14 (verify-always) 也需要 ax_diff,会有两个 caller;届时再抽 facade 避免重复。

### LP-ticket-13-deferred-2: smoke 脚本版本化
- ticket 12 把旧 smoke_computer_act.sh 期待 `verification: null` 改成 omit,旧契约被推翻。
- 后续每个 ticket 改 response envelope 时,旧 smoke 直接挂,需要 update in place。
- 长期方案: 拆 smoke_computer_act_v{N}.sh (N = 当前 acceptance 版本),主 smoke_computer_act.sh 指向最新版本。
  旧版本留作 regression,跟 git history 对齐。
- 短期方案: 维持现状,smoke 改动时手动同步。

### LP-ticket-13-deferred-3: verify=always 真实实现 (Phase E ticket 14)
- ticket 14 范围:
  1. `render_verification(Always)` 返回 screenshot_id + ax_tree_id + windows + ax_diff + window_state
  2. `run_always_verify` 走 control_observation::build_observe_outcome (完整 observe)
  3. density 增加 `screenshot_ms` 段
- ticket 13 已经预留 enum variant 和 render 函数骨架,ticket 14 只需在 `render_verification` 加 Always 分支 + 加 helper

### LP-ticket-13-deferred-4: invalid_verify 错误的 retry strategy (ticket 15)
- ticket 13 引入了 `error_code:"invalid_verify"`,但 ticket 15 E2 envelope 才补 `retry.strategy:"never"` (手动修复语法)
- 当前 invalid_verify 错误响应里没有 retry 字段,跟 ticket 04 时代 (其他错误码也没 retry) 一致
- ticket 15 统一处理

### LP-ticket-14-deferred-1: observation_block 字段接入 trace
- 当前 `AlwaysVerifySummary.observation_block` 字段保留但 `#[allow(dead_code)]`,ticket 14 不渲染。
- ticket 18 trace 实现时,可以把这个 full observe bundle 直接落盘 (不再重做 observe)。
- 触发条件: ticket 18 trace_savefile 实现时。

### LP-ticket-14-deferred-2: screenshot_id 提取逻辑优化
- 当前 fallback chain: visual.id → observation.observation_id。
- 后续 observe bundle 应该给 visual 段加独立 id 字段 (跟 ax_tree_id / window_observation 区分),让 screenshot_id 不再退化成 observation_id。
- 触发条件: rdog 上游 observe API 加 visual.id 后。

### LP-ticket-14-deferred-3: full observe 的并行化
- 当前 `run_always_verify` 是串行 (pre-AX → dispatch → post-observe → diff)。
- full observe ~1.3s,如果跟 dispatch 并行 (在 dispatch 启动期间就启动 observe),可以省掉一个 round-trip。
- 触发条件: dispatcher 改成 async 后 (rdog 后续 phase)。

### LP-ticket-14-deferred-4: verification.observation.windows 类型
- 当前 `windows` 字段直接从 observe bundle 的 windows 段复制 (结构为 `{status, reason, target_applied, ...}` 在 target_required 时)。
- 后续 verify=always 应该明确 windows 是数组 (`[{id, title, role, ...}]`),跟 best_effort 的 ax_diff.elements 一致。
- 触发条件: ticket 21 (e2e smoke) 验证真实 GUI 场景时,确认 windows 字段语义。

### LP-ticket-17-deferred-1: payload_bytes 实测
- 当前 `ComputerActDensity::payload_bytes` 字段占 0。
- 真实值应该在 response_value_json 序列化后算 (`response.to_string().len()` 或 `serde_json::to_vec(&payload).len()`)。
- 触发条件: ticket 21 e2e smoke 阶段补真实值,避免 response size 漂移无监控。

### LP-ticket-17-deferred-2: mouse_fallback_count / stale_ref_recovery_count / false_success_count 实测
- 当前都占 0。
- mouse_fallback_count: ticket 04 阶段 start_box → coordinate fallback 计数;等真实 ref 集成后才有意义
- stale_ref_recovery_count: ticket 11 implicit_observe stale_re_observed 路径每次 +1
- false_success_count: ok:true 但 GUI 没变化的次数 (跟 verification_passed 互补)
- 触发条件: ticket 21 e2e smoke 真实 GUI 场景补真实值。

### LP-ticket-18-deferred-1: request_id thread 到 execute_computer_act
- 当前 savefile name 用 ts_ms (无 request_id),trace-1784195533512-1784195533512.json (id 部分跟 ts 重复)。
- 后续 control_actions.rs 重构时,把 request_id 从 ControlRequest 传到 execute_computer_act,让 savefile name 含真实 request_id (跟 log / debug 工具对齐)。
- 触发条件: control_actions.rs::execute 签名重构时。

### LP-ticket-18-deferred-2: trace_savefile 自动清理
- 当前 rdog_downloads/trace-*.json 不断累积,没清理策略。
- 后续加 savefile TTL (e.g., 7 天) 或 size 上限 (e.g., 100MB),跟 @savefile 机制统一治理。
- 触发条件: rdog 上游 savefile 模块加全局 TTL/size 管理时。

### LP-ticket-18-deferred-3: implicit_observe sub-step 实测
- 当前 ax_tree_scan: ok (轻量 capture), screenshot_capture: skipped, ref_resolution: skipped。
- 等 ticket 11 LP-ticket-11-deferred-1 真实 observe 集成时, screenshot_capture 改 ok。
- 等 LP-ticket-11-deferred 真实 start_box → ref 解析集成时, ref_resolution 改 ok。
- 触发条件: Phase I ticket 21+ 集成时。

### LP-ticket-15-deferred-1: 剩余 error_code (observation_expired / target_not_found / verify_failed) 真实触发
- 当前 error_envelope.rs 11 个 error_code 都定义了 strategy/hint/default_evidence_key,
  但 mod.rs / dispatch_underlying 只有 unknown_action / invalid_args / invalid_verify /
  timeout 4 个真正触发并返回 E2 envelope。
- 待补:
  - observation_expired: 当 implicit_observe stale + re-observe 也失败时返回
    (tied to ticket 11 stale_fallback_to_coords LP, 真实 observe 集成时)
  - target_not_found: 当 click / scroll / drag 命中坐标但 AX 找不到 element 时
  - verify_failed: 当 verify=always/best_effort 且 ax_diff 显示 GUI 没变时
- 触发条件: ticket 21+ e2e 真实 GUI 场景补真实错误路径。

### LP-ticket-16-deferred-1: TimeoutWatcher 提早 stop
- 当前 caller `let _timeout_watcher = TimeoutWatcher::start(...)` 不调 stop,
  等 thread 跑满 timeout_ms 才被 join 回收 (block main thread 但 daemon 单线程可接受)。
- 后续优化: dispatch 完成后显式 stop, 避免 wasted 时间。
- 触发条件: rdog dispatcher 改成 async 后, 提早 stop 才能省 CPU。

### LP-ticket-21-deferred-1: 13 动作 smoke 拆成 13 个独立脚本
- 当前 smoke_computer_act_all.sh 把 13 个动作串成一个脚本 (~20s 总时长)。
- 后续拆成 13 个独立 smoke (每动作 1 个脚本), CI 可以选择性跑 (e.g., 只跑跟代码改动
  相关的几个)。
- 触发条件: CI 接入后 (ticket 21 acceptance "Smoke script is wired into CI / smoke collection")。

### LP-ticket-08-deferred-1: Composite sub-step trace_summary
- ticket 08 spec 要求 "trace_summary shows three dispatch entries (one per sub-step)
  for hotkey_click"。当前实现 1 entry (整 Composite 算一个 dispatch step)。
- 后续: 真正的 sub-step trace 需要扩 TraceSummary 概念 (e.g., 嵌套 step 数组)。
- 触发条件: ticket 18 trace 进一步细化时 (现在 hotkey_click 单 step 已经够用)。

### LP-ticket-22-deferred-1: 真实 GUI benchmark (e2e + density 同步测)
- 当前 benchmark 用 @wait/@key/@ping 等 fast 命令, 跨环境稳定但没真实 GUI 执行时间。
- 后续: 加 "real_gui" 模式 (用真实 Calculator / 浏览器 / 文件管理器), 把 wall-clock
  对比也包进 report。当前 benchmark 只比 round-trip COUNT, 真实 GUI benchmark 比
  wall-clock 才是 client 关心的最终指标。
- 触发条件: 真实使用场景需要 GUI benchmark 报告时 (e.g., 客户要求 SLA 证明)。

### LP-ticket-22-deferred-2: per-action breakdown
- 当前 benchmark 是 10 任务综合 win rate。
- 后续: 拆出 per-action benchmark (click_only / wait_only / hotkey_only 等),
  单独看每个动作的 density overhead。click 等简单动作可能 overhead 较小, 
  hotkey_click (Composite 3 步) overhead 较大。
- 触发条件: 客户端调优时需要 per-action 数据。

### LP-ticket-22-deferred-3: 历史对比
- 当前 benchmark 只输出当次结果, 没有历史对比。
- 后续: 每次跑 benchmark append 到历史文件, 跟历史 win rate 做对比, 退化时报警。
- 触发条件: CI 接入定期跑 benchmark 时。

### LP-ticket-20-deferred-1: 累积 dead_code warnings (跨 ticket 累积, 不阻塞)
本轮 ticket 19+20 范围 0 warning, 但跨 session 累积还有这些 (跟 ticket 19+20 无直接关系):
- `src/control_computer_act/mod.rs:68` unused import: `RetryStrategy`
- `src/control_computer_act/mod.rs:391` unused variable: `cancel`
- `src/control_computer_act/timeout.rs:20` unused import: `ComputerActErrorCode`
- `src/control_computer_act/error_envelope.rs:61-69` 7 个 `ComputerActErrorCode` variant 未构造
  (PermissionDenied / ObservationExpired / TargetNotFound / VerifyFailed /
   PlatformUnsupported / Infrastructure / Cancelled) — 等真实 GUI e2e 触发 → LP-ticket-15-deferred-1
- `TimeoutWatcher.fired()` / `.stop()` / `cancel_token` / `handle` 字段未读
  (let _timeout_watcher 模式留 API 给上层显式 stop / fired check)
- 跟 ticket 11 配合的 `resolve_or_re_observe_at` 函数未调用 — 等真实 observe 集成触发

触发条件: 下一次单独自清理 batch (一两个 `#[allow(dead_code)]` 集中处理),
不要在每个新 ticket 里分散处理。

### LP-ticket-20-deferred-2: `@flow` ControlLine 当前只消费 `code` / `ok` 字段
ticket 19+20 只暴露 `$.value.dispatched_to` / `$.value.ok` 给 `Expect` 断言路径导航,
但 `@computer-act` response 还包含 `density` / `trace_summary` / `observation_id` /
`verification` 等 11+ 个字段, 这些暂时不能让 `@flow` 直接断言。

后续可以加:
- `expect_density(path, threshold)` 断言 density.elapsed_ms_total < N
- `expect_trace_step(path, step_name, status)` 断言 trace_summary 里某 step status
- `expect_observation_fresh(path)` 断言 observation_used.freshness == "fresh"

触发条件: 用户用 `@flow` 跑真实 agent loop 时发现需要断言这些字段。

### LP-ticket-20-deferred-3: `json_pointer_lookup` 不支持 RFC 6901 完整语法
当前实现只支持 dot path + `[N]`, 不支持 `..` / `$ref` / 多重索引 / `-` 索引 (append)。
实际 rdog 用例 95% 是 `$.value.X.Y` 风格, 简化为 dot path 跟 Mano-CUA 一致。

后续如果需要更复杂路径, 直接用 `serde_json::Value::pointer()` (RFC 6901 标准)。
触发条件: 客户端跑大型 flow 想要断言嵌套数组元素 (e.g. `$.value.density[0]`)。

### LP-ticket-15-deferred-2: Phase F-1 之外的 4 个 error_code 真实触发
本轮 Phase F-1 收口了 3 个 (Cancelled / PlatformUnsupported / PermissionDenied) 走 envelope
helper, 但剩下 4 个 variant 完全没触发路径, 等真实场景:

1. **ObservationExpired**: 当 implicit_observe stale + re-observe 也失败时返回。
   依赖 ticket 11 LP-ticket-11-deferred-1 (真实 observe 集成), 当前 ticket 11 用
   synthetic `@e{seq}` ref_id 占位, 没真抓 screenshot / AX tree。
   触发条件: Phase I 真实 observe 集成 + TTL 真正过期时。

2. **TargetNotFound**: 当 click / scroll / drag 命中坐标但 AX 找不到 element 时。
   同样依赖 Phase I 真实 observe。
   触发条件: Phase I 集成后, 真实 GUI 截图里某个坐标没 element 时。

3. **VerifyFailed**: 当 verify=always/best_effort 且 ax_diff 显示 GUI 没变化时。
   当前 ticket 13 best_effort 是 placeholder (没真跑 AX diff), ticket 14 Always 等同 None。
   触发条件: Phase F-2 verify logic 真实化 (扩展 verify.rs + 新 smoke)。

4. **Infrastructure**: zenoh router down / unixpipe broken / zenoh timeout 等。
   触发条件: 需要 client 端断开测试, 或 daemon 端临时 kill zenoh session。

### LP-ticket-15-deferred-3: ticket 03 cancel registry 跨实例 bug
Phase F-1 smoke 调试时发现 `src/zenoh_control.rs:240` 每次请求都新建 `CancelRegistry::new()`,
跟 `SystemControlActionExecutor::cancel_registry` (line 78) 不是同一实例。结果:
- wait 命令 register 到 dispatcher 临时 registry
- cancel 命令 signal executor 内部 registry → 找不到 in-flight seq → 返回 unknown_target_seq
- wait 完整跑完 (无 cancel 中断)

修法 (本轮没做, 留后续):
- 把 dispatcher 的 registry 跟 executor 的 registry 合并成同一实例
- 或 zenoh_control 的 dispatch loop 共享同一 registry
- Phase F-3 / ticket 03 fixup 一起做

影响范围:
- 当前 `@cancel#seq` 对 `@wait` 实际无效 (smoke 验证)
- 这是 ticket 03 (cancellation) 的核心承诺, 属于 "做正确修复" 而非 "最小修复"

### LP-ticket-15-deferred-4: PlatformUnsupported 在 macOS 上 dead_code helper
本轮 `platform_unsupported_envelope_json` 加了 `#[allow(dead_code)]`, 因为 macOS 编译
时 `cfg(not(target_os = "macos"))` 分支被排除, helper 没有 live caller。

如果未来 macOS 上某个 action (e.g., `@screenshot` 在 visionOS 上) 也走 platform gate,
可以抽通用 `platform_unsupported_envelope_json(action: &str, target_os: &str)` 让 helper
在所有平台都有 caller, 移除 #[allow(dead_code)]。

触发条件: 跨平台 action 增加, 或当前 dead_code 累积需要 batch 清理。


### LP-ticket-15-deferred-3-RESOLVED: ticket 03 cancel registry 跨实例 bug
Phase F-3 收口, 详见 commit dda4cc2 + WORKLOG `[2026-07-17 15:30:00]` entry.

修法:
- src/control_actions.rs: SystemControlActionExecutor 加 cancel_registry() accessor
- src/zenoh_control.rs:240 + src/zenoh_control/daemon_bridge.rs:310: 都改用
  executor.cancel_registry() (跟 executor 内部 Arc 共享)

实操关键: ticket 03 bug 实际有**两个 instance** (queryable + session bridge path),
必须两处都改. 第一版修完一处 wait 仍跑满 10s, 第二处 trace 后才发现.

### LP-ticket-15-deferred-5: PermissionDenied live trigger (Phase F-3.5 候选)
Phase F-3 没实现 PermissionDenied live trigger, 因为 daemon PATH 隔离
(client 改 PATH 不影响 daemon 进程) + 需要 refactor execute_open_app 暴露
injectable open_fn (cfg(test) mock Command path).

后续方案:
- 抽 `run_open_app_command(app_name) -> io::Result<()>` helper 走 env::var("PATH") 控制
- 单测直接用 mock (cfg(test) 注入测试), live trigger 留 Linux/Windows CI
- Phase F-3.5 单独 ticket 跟进

### LP-ticket-15-deferred-6: ObservationExpired / TargetNotFound / VerifyFailed / Infrastructure
剩 4 个 variant 完全没触发路径:
- ObservationExpired: 依赖 Phase I 真实 observe 集成 + TTL 真正过期
- TargetNotFound: 依赖 Phase I 真实 observe (AX 找不到 element)
- VerifyFailed: 依赖 Phase F-2 verify logic 真实化 (当前 best_effort 是 placeholder)
- Infrastructure: 依赖 client 断开测试 (zenoh router down / pipe broken)

触发条件: 用户给具体场景再启 ticket.
