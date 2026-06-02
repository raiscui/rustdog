## [2026-05-27 13:10:30] [Session ID: codex-native-20260527-111938] 任务名称: 验证 rdog 截图旧帧问题

### 任务内容
- 按用户要求执行“重启 daemon 后重新截图”的最小验证。
- 判断错误截图是否由文件名保存问题导致,还是由截图后端/daemon 状态导致。

### 完成过程
- 终止旧 `target/debug/rdog daemon -c ./rdog_macos.toml` 进程。
- 旧进程启动时间为 2026-05-26 09:59:47,属于 long-running daemon。
- 重新启动 `./target/debug/rdog daemon -c ./rdog_macos.toml`。
- 新建 `rdog control mac.lab` 会话,执行 `@ping#1`、`@capabilities#2`、`@screenshot#3:{include_ax:false}`。
- 获取新截图 `rdog_downloads/screenshot-1779858489073-virtual-desktop.jpg`。

### 验证
- `@ping#1`: 返回 `pong`。
- `@capabilities#2`: screenshot / accessibility / window_control / mouse_input 均为 `available`。
- 旧 stale 截图 `screenshot-1779854799905-virtual-desktop.jpg` 的 MD5: `52b603b1e9cb78e60563100e051c4add`。
- 新截图 `screenshot-1779858489073-virtual-desktop.jpg` 的 MD5: `d65f16c702e88518374dbb6cc40eee34`。
- 新截图视觉内容已经是当前屏幕: 左侧显示小红书页面和左侧导航,右侧显示当前工作区。

### 总结感悟
- 本次错误不是普通文件名复用或打开错文件。
- `screenshot_id` 和落盘路径都是新生成的,但旧 daemon 的 `sck-rs` 截图链路返回了 stale frame。
- 后续 GUI 自动化在使用 `@screenshot` 作为视觉真相源之前,应先做 freshness guard: 检查新截图 hash 是否变化,或者用当前页面关键视觉/AX/window 状态交叉验证。

## [2026-05-27 14:22:00] [Session ID: codex-native-20260527-133652] 任务名称: 给 @screenshot 增加 freshness / stale guard

### 任务内容
- 给 composite `@screenshot` 生产路径增加 daemon 进程级 freshness guard。
- 当连续两次捕获到完全相同的显示器布局和像素指纹时,返回结构化错误并在 `@savefile` 之前终止。
- 同步 `@observe include_screenshot` 复用的 screenshot bundle 路径,避免 observe 继续吃旧视觉证据。

### 完成过程
- 在 `src/screenshot.rs` 增加 `CompositeCaptureFingerprint` / `DisplayCaptureFingerprint`。
- fingerprint 覆盖 display id、backend、`os_rect`、native capture size 和 RGBA 像素 FNV-1a hash。
- 生产路径使用进程级 `LAST_COMPOSITE_FINGERPRINT` 保存上一帧。
- 单元测试路径改成可注入 freshness checker,避免并行测试互相污染全局 cache。
- 控制层新增结构化 `Other` error JSON 转发测试,确认 stale 错误不会被双重转义。
- 更新 `specs/control-line-protocol.md`、`specs/rdog-multi-display-screenshot-coordinate-plan.md`、`specs/code-agent-rdog-control-usage.md` 和 `rdog-control` protocol reference。

### 验证
- `cargo fmt`: 通过。
- `cargo test --package rustdog --bin rdog -- screenshot::tests --quiet`: 19 passed。
- `cargo test --package rustdog --bin rdog -- control_core::tests::explicit_request_should_forward_structured_other_error_json --exact --quiet`: 1 passed。
- `cargo test --package rustdog --bin rdog --no-run --quiet`: 通过。
- `cargo test --package rustdog --test control_lanes --no-run --quiet`: 通过。
- `cargo test --package rustdog --test zenoh_router_client --no-run --quiet`: 通过。
- `git diff --check`: 通过。
- live smoke: 重启新二进制 daemon 后,`@ping#1` 返回 `pong`; 连续两次真实 `@screenshot` 都成功,说明当前真实屏幕两帧像素不同,正常路径没有被误杀。
- stale 触发本身由单测固定: 第二次完全相同 composite frame 在 AX capture 前返回 `SCREENSHOT_STALE_FRAME`。

### 总结感悟
- 本次 guard 的目标是阻止“看似新文件、实际旧像素”的视觉证据继续流入后续 GUI 自动化。
- 错误 payload 带 `guard_policy:"reject-consecutive-identical-composite-fingerprint"`、backend、rect、size 和 pixel hash,后续可以据此分析是 `sck-rs` / ScreenCaptureKit 状态、daemon 生命周期,还是显示器捕获路径导致旧帧。
- 当前 live daemon 已恢复在 tmux session `rdog-maclab-daemon`,实际进程为 `./target/debug/rdog daemon -c ./rdog_macos.toml`。

## [2026-06-01 15:01:15] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 任务名称: 小红书“首页”点击真实 e2e 闭环

### 任务内容
- 使用 `rdog control mac.lab` 做真实 GUI e2e。
- 点击小红书左侧导航“首页”。
- 按用户指定口径验证:只有点击后瀑布流截图内容发生变化,才算成功。

### 完成过程
- 先用现有 daemon 探测,发现 `@ping` 可达但 `@bootstrap` 返回 `不支持的控制指令类型: bootstrap`。
- 构建当前工作区二进制并重启 `rdog_macos.toml` daemon 到 tmux session `rdog-maclab-daemon`。
- 新 daemon 下 `@bootstrap#2` 成功,保存 `screenshot-1780296670220-virtual-desktop.jpg`。
- 通过 `@cmd#3` 打开 `https://www.xiaohongshu.com/explore`,保存点击前准备截图 `screenshot-1780296730337-virtual-desktop.jpg`。
- `@web-find target:{browser:"active"}` 因 4 个 Chrome 窗口返回 `BROWSER_WINDOW_AMBIGUOUS`。
- `@window-find#7` 找到小红书窗口 `pid:96405/window:3`,`@window-activate#8` 返回 ok。
- 两次坐标点击曾打开笔记详情页,因此按截图证据回滚成功口径。
- 用 `@key#16:"Escape"` 关闭详情层,以 `screenshot-1780297129609-virtual-desktop.jpg` 作为真正 before。
- 最终执行 `@click#19:{x:48,y:255,button:"left",count:1}`,等待后保存 after `screenshot-1780297167647-virtual-desktop.jpg`。

### 验证
- `@click#19`: `status:"ok"`,坐标来源为 `coordinate_fallback`,点位 `(48,255)`。
- before crop `/tmp/xhs-feed-before.jpg`: MD5 `4408ed9a9198015c32179adcc573c9e3`。
- after crop `/tmp/xhs-feed-after.jpg`: MD5 `c05f018130432dbd3cff27188f5c4a1d`。
- `imgdiff --threshold 0.1 --diff-image`: 输出 `Images are different`,不同像素 `304233`。
- 人眼复核: before 是狗/Claude Code/海岸等瀑布流; after 是“做一个独立站要多少钱”、水彩鹿、PI agent 等新瀑布流。

### 总结感悟
- 真实 GUI e2e 的成功证据必须落在用户关心的页面状态上,不能停在 action response。
- `@bootstrap` 对起手提速有效,但 `@web-find` 当前 active browser target 在多 Chrome 窗口下不够。
- 下一步产品化最好补 `@web-find` / `@gui-probe` 的 window-scoped target,减少从截图坐标 fallback 绕路。

## [2026-06-01 15:05:34] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 任务名称: bootstrap 提交和小红书 e2e 证据复核

### 任务内容
- 复核 bootstrap 主题提交是否已经落在独立 commit。
- 复核小红书“首页”点击 e2e 是否仍有可重复检查的截图差异证据。

### 完成过程
- 查看 `git log -1 --oneline`,确认当前 HEAD 为 `4d2dd7a Add read-only bootstrap preflight`。
- 对 bootstrap 核心文件执行 scoped `git status --short`,没有发现这些文件相对 HEAD 还有未提交变化。
- 对 `__xhs_home_click` 支线记录执行 `git diff --check`,确认 Markdown 记录没有 whitespace 问题。
- 重新运行 `imgdiff` 对 before/after feed crop 做差异验证。

### 总结感悟
- 这次复核把两件事分清了:bootstrap commit 是代码和文档主题提交,e2e 记录属于后续真实测试证据。
- `imgdiff` 在本场景返回 exit code 1 是预期信号,意思是两张图片不同。真正要看的是输出中的 different pixels 数量。

## [2026-06-01 15:11:42] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 任务名称: bootstrap commit 临时 worktree 验证

### 任务内容
- 在不受当前混合工作区影响的情况下复核 `4d2dd7a Add read-only bootstrap preflight`。

### 完成过程
- 使用 `git worktree add --detach` 从 `4d2dd7a` 创建临时 worktree。
- 在临时 worktree 内执行格式检查、bootstrap parser/core/zenoh 关键测试和 `rdog` bin 全量单测。
- 验证完成后删除临时 worktree。

### 验证
- `cargo fmt --check`: 通过。
- `control_protocol::tests::bootstrap`: 7 passed。
- `control_core::tests::parse_error_should_preserve_bootstrap_cached_policy_structure`: 1 passed。
- `zenoh_control::tests::legacy_queryable_should_reject_bootstrap_requests`: 1 passed。
- `cargo test --package rustdog --bin rdog --quiet`: 276 passed,0 failed。

### 总结感悟
- 验证提交本身时,临时 worktree 比当前混合工作区更干净。
- 这让“bootstrap 已单独提交并通过测试”和“当前工作区仍有其它未提交主题”可以同时成立,不会互相污染。

## [2026-06-02 13:34:05] [Session ID: codex-native-20260602-xhs-home-click-live] 任务名称: 小红书“首页”点击 live GUI 闭环

### 任务内容
- 使用 `rdog control mac.lab` 点击小红书页面左侧导航“首页”。
- 优先使用 window-scoped `@web-find` + semantic `AXPress`,不走默认坐标点击。
- 按用户指定口径,用点击前后瀑布流截图变化证明成功。

### 完成过程
- `@bootstrap#2` 确认 screenshot / accessibility / window_control / mouse_input 可用。
- `@window-find#3` 找到小红书 Chrome 窗口 `pid:96405/window:0`。
- `@web-find#4` 在该窗口的 `AXWebArea` 中找到唯一 `AXLink.description:"首页"`。
- `@window-activate#5` 激活并 raise 小红书窗口。
- 保存 before screenshot `rdog_downloads/screenshot-1780378381102-virtual-desktop.jpg`。
- 对 page-owned AX id 执行 `@ax-action#7 action:"AXPress"`,返回 `performed:true,status:"ok"`。
- 等待 2 秒后保存 after screenshot `rdog_downloads/screenshot-1780378384620-virtual-desktop.jpg`。
- 裁剪主瀑布流区域并运行 `imgdiff`。

### 验证
- before crop: `target/rdog-live-e2e/xhs-home/before_20260602_feed.jpg`,MD5 `2006ff12040554092a020a472723fd34`。
- after crop: `target/rdog-live-e2e/xhs-home/after_20260602_feed.jpg`,MD5 `eeae5a7c55d8509401e01dfb0a6801f0`。
- diff image: `target/rdog-live-e2e/xhs-home/diff_20260602_feed.png`。
- `imgdiff --threshold 0.1 --diff-image`: 不同像素 `564124`。
- 人眼复核: before 和 after 的瀑布流卡片内容不同,after 已进入新的推荐流状态。

### 总结感悟
- window-scoped `@web-find` 已经把这类任务从坐标 fallback 拉回到语义 AX 主路径。
- 对小红书这类 feed-changing 页面,动作成功和任务成功仍是两件事;最终证据必须是 feed 区域截图变化。
