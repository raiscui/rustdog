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
- [ ] 启动独立 plan:方向 B(直接 UDS 控制面,10~50x 提速),作为 unixpipe 体验确认后的 follow-up

### 已有 flake 待处理

- [ ] `zenoh_router_client` 测试集多测试并发 ~4% flake,失败用例不固定。
  - 已记录到 EPIPHANY_LOG。
  - 排查方向: 给 `resolve_target` 的 liveliness get 加 retry,或 test helper 给每个 test 独立 namespace。
  - 不属于本轮范围,留作 follow-up。

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
- [ ] 启动独立 plan:方向 B(直接 UDS 控制面,10~50x 提速),作为 unixpipe 体验确认后的 follow-up
- [ ] `zenoh_router_client` 测试集 ~4% 多测试并发 flake 排查(已记 EPIPHANY_LOG,不在本轮范围)
- [ ] 路径上限 100 vs 95 字节口径同步:`src/zenoh_runtime.rs` 注释写 ≤ 95 字节(macOS sun_path 104 - `_downlink` 9),但 `src/config.rs::UNIXPIPE_SOCKET_PATH_MAX_BYTES` 实际常量是 100,`specs/zenoh-unixpipe-fast-path-plan.md` 也写 100。两种都通过测试但口径不一致,需要后续对齐

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

## [2026-06-25 14:07:20] [Session ID: native-hook-20260625-135331] 后续建议: local-default unixpipe daemon 实现收口

### 背景
- 已生成计划 `.omx/plans/rdog-local-default-unixpipe-daemon-plan.md`。
- 当前问题是 `rdog control @screenshot` 在本机多个 `$TMPDIR/rdog-*.pipe_uplink` 候选存在时无法选择默认 daemon。

### 后续事项
- [ ] 按计划实现 local-default registry + guard。
- [ ] 同步修正 `specs/zenoh-unixpipe-fast-path-plan.md` 与 `src/config.rs` 注释里残留的 "socket" / 100 字节旧口径。
- [ ] 清理或修正 `AGENTS.md` 中当前磁盘不存在的 `.codex/skills/self-learning.zenoh-duplicate-name-local-guard/SKILL.md` 索引。
- [ ] 执行计划中的 unit / e2e / live smoke 验证,确认 `rdog control @ping` 和 `rdog control @screenshot` 不再卡在 target 选择层。

## [2026-06-25 15:45:55] [Session ID: 019efd3b-9edc-7e11-9168-461c6e467d1d] 完成记录: local-default unixpipe daemon 实现已落地

### 对应旧计划
- 对应 `2026-06-25 14:07:20` 的"local-default unixpipe daemon 实现收口"。

### 完成结果
- 已实现 local-default registry + PID guard。
- 已同步 `src/config.rs` / `src/daemon.rs` / `src/zenoh_runtime.rs` / `src/main.rs`。
- 已修正 FIFO / 95 字节口径,同步模板、spec 和 `rdog-control` skill。
- 已完成 unit / e2e / live smoke,确认 `rdog control @ping` 和 `rdog control @screenshot` 不再卡在 target 选择层。

### 后续事项
- [ ] `AGENTS.md` 中 `.codex/skills/self-learning.zenoh-duplicate-name-local-guard/SKILL.md` 路径仍需单独核对。当前本轮未处理,避免和业务修复混合。
- [ ] 方向 B(直接 UDS 控制面,10~50x 提速)仍是独立 plan,不属于本轮 local-default 修复。

## [2026-06-26 13:18:00] [Session ID: 019f023a-e4c3-7f73-9d7b-9393ef3d38ff] 后续建议: rdog UI script 正式规格化

### 背景

- 本轮只做设计讨论,参考了 iced_emg 的 UI Script JSON DSL、`docs/ui_script_command.md` 和 rdog 当前 control 协议。
- 当前推荐方向是先做 CLI-side runner,复用现有 line-control frames,不要直接新增第二套 UI 协议。

### 后续事项

- [ ] 如果用户确认方向,将设计整理为 `specs/rdog-ui-script-control-plan.md`。
- [ ] 同步 `AGENTS.md` 长期知识索引,说明修改 UI script runner、control script、GUI 自动化 DSL 前应阅读该规格。
- [ ] 实现前先补 parser/runner 的 fixture tests,至少覆盖 iced-compatible `SleepMs/Move/Click/Screenshot/Exit` 和 rdog-specific `Observe/Scope/Expect`。
- [ ] 设计命名时避开 `@script`,因为它已经是 shell 执行语义。

## [2026-06-26 16:02:51] [Session ID: codex-20260626-ui-script-spec] 完成记录: rdog UI script 规格化已落地

### 对应旧计划

- 对应 `2026-06-26 13:18:00` 的"rdog UI script 正式规格化"。

### 完成结果

- 已创建 `specs/rdog-ui-script-control-plan.md`。
- 已同步 `AGENTS.md` 长期知识索引。
- 已在规格中明确 `@script` / `@cmd` 仍是 shell 执行语义,UI flow 后续应使用 CLI-side runner 或独立 `@ui-flow` 命名。

### 后续事项

- [ ] 实现前仍需补 parser/runner fixture tests,覆盖 iced-compatible `SleepMs/Move/Click/Screenshot/Exit` 和 rdog-specific `Target/Scope/Observe/Expect`。
- [ ] 如果后续要让 `WindowSize` 真正 resize 窗口,应先设计 window resize control 协议,不要借 UI script 偷渡。

## [2026-06-26 16:31:44] [Session ID: codex-20260626-ui-script-fixtures] 完成记录与后续建议: UI script fixture tests / window resize

### 对应旧计划

- 对应 `2026-06-26 16:02:51` 的"实现前仍需补 parser/runner fixture tests"。
- 对应 `2026-06-26 16:02:51` 的"`WindowSize` 真正 resize 窗口应先设计 window resize control 协议"。

### 完成结果

- parser / dry-run runner fixture tests 已落地到 `src/ui_script.rs` 和 `tests/fixtures/ui_script/`。
- window resize control 协议已规划到 `specs/rdog-window-control-plan.md` 的 `@window-resize` 节。
- UI script 规格已说明 `WindowSize mode:"resize"` 未来应编译到 `@window-resize`,当前 dry-run 仍只接受 `mode:"precondition"`。

### 后续事项

- [ ] 接入正式 CLI,例如 `rdog ui-script run [TARGET] <file.json>` 或 `rdog control TARGET --ui-script <file.json>`。
- [ ] 将 dry-run compiler 接到真实 control transport,生成 trace.jsonl、artifacts 和 step report。
- [ ] 实现 `@window-resize` parser / executor / macOS AX backend / focused tests。默认行为必须恢复/激活目标窗口,请求里不需要 `activate:true`。同时覆盖 `verify.tolerance_px = 2`、`ok_with_delta`、`WINDOW_RESIZE_CLAMPED`、`WINDOW_RESIZE_NOT_SETTABLE`、`WINDOW_RESIZE_GUARD_FAILED` 和 `WINDOW_AMBIGUOUS`。
- [ ] `@window-resize` 落地后,把 UI script `WindowSize mode:"resize"` 从未来语义升级为可编译 step。该 step 直接编译到默认恢复/激活的 `@window-resize`。

## [2026-06-28 00:50:00] [Session ID: codex-20260628-goal-window-resize] 完成记录与后续建议: @window-resize 已落地,UI script 接入仍待做

### 对应旧计划

- 对应 `2026-06-26 16:31:44` 的"`@window-resize` parser / executor / macOS AX backend / focused tests"。

### 完成结果

- `@window-resize` parser / command model / executor / macOS AX backend / focused tests 已落地。
- 默认恢复/激活、canonical `target:{...}`、`target.query` 唯一命中、`verify.tolerance_px = 2`、`ok_with_delta`、`WINDOW_RESIZE_CLAMPED`、`WINDOW_RESIZE_NOT_SETTABLE`、`WINDOW_RESIZE_GUARD_FAILED` 和 resize ambiguity 边界已覆盖。

### 后续事项

- [ ] 接入正式 UI script CLI 或 `rdog control --ui-script` 时,把 `WindowSize mode:"resize"` 编译到 `@window-resize`。
- [ ] UI script runner 接真实 transport 后,为 resize step 记录 trace.jsonl、artifacts 和 per-step report。
- [ ] 有明确目标窗口时,再跑 live `@window-resize` smoke,避免在没有用户确认的情况下移动/缩放当前桌面窗口。

## [2026-06-28 13:16:00] [Session ID: codex-20260628-ui-script-window-size-resize] 完成记录与后续建议: WindowSize resize dry-run 编译已落地

### 对应旧计划

- 对应 `2026-06-28 00:50:00` 的"接入正式 UI script CLI 或 `rdog control --ui-script` 时,把 `WindowSize mode:\"resize\"` 编译到 `@window-resize`"中的 dry-run compiler 部分。

### 完成结果

- `src/ui_script.rs` 已支持 `WindowSize mode:"resize"` dry-run 编译到 `@window-resize`。
- 新增 `window_size_resize.json` fixture。
- 新测试确认生成的 line-control 文本可被真实 `parse_control_line` 解析为 `ControlCommand::WindowResize`。

### 后续事项

- [ ] 正式 `rdog ui-script run` / `rdog control --ui-script` 仍未接入,后续 runner 应复用当前 dry-run compiler。
- [ ] 真实 transport 接入后,把 `WindowSize mode:"resize"` 的 step report、`@window-resize` response 和 artifacts 写入 trace。
- [ ] 有明确目标窗口时,再跑 live `@window-resize` smoke。

## [2026-06-28 13:30:07] [Session ID: codex-20260628-finder-window-resize-live] 完成记录与后续建议: Finder live @window-resize smoke 已完成

### 对应旧计划

- 对应 `2026-06-28 13:16:00` 的"有明确目标窗口时,再跑 live `@window-resize` smoke"。

### 完成结果

- 已对 Finder `docs` 窗口执行 live `@window-resize`。
- 请求尺寸为 `1000x700`,最终 Finder/macOS clamp 到 `{x:271,y:247,width:1000,height:652}`。
- 独立 `@window-find` 已验证最终 rect 和可交互状态。

### 后续事项

- [ ] installed `/Users/cuiluming/.cargo/bin/rdog` daemon 仍可能是旧二进制。若后续希望裸 `rdog control ...` 直接支持 `@window-resize`,需要安装当前 workspace 版本并重启 daemon。
- [ ] 当前 debug daemon 运行在 tmux session `rdog-debug-daemon`。后续如果切回 release daemon,需要先停掉这个 session,避免 local-default 指向调试进程。
