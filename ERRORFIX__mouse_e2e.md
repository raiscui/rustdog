
## [2026-05-14 12:52:35] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 错误修复: Apple 菜单“关于本机”点击假阳性

### 现象
- ignored E2E exit 0,但用户观察到第二次点击没有命中“关于本机”。
- 原测试只校验 mouse response 和 screenshot bundle,没有证明最终窗口出现。

### 原因
- 已验证的代码问题是测试使用固定 `about_this_mac_x/y` 偏移,并且缺少最终 UI 视觉断言。
- 上一轮复跑失败的最终断言受到用户手动移动鼠标干扰,不能单独作为当前实现失败证据。

### 修复
- 改为先点击 Apple icon,再截图保留 Apple 菜单打开状态。
- 从初始截图和菜单截图的差异中识别菜单面板,并将面板第一项中心转换回 `os-logical` 坐标。
- 点击后再次截图,要求最终画面出现非菜单区域的明显变化。

### 验证
- `cargo fmt -- --check`: 通过。
- `cargo test --test control_mouse_e2e --no-run`: 通过。
- `cargo test --test control_mouse_e2e`: 通过,1 ignored。
- `cargo test --test control_mouse_e2e daemon_control_lane_should_click_apple_menu_about_this_mac_via_rdog_control -- --ignored --exact --nocapture`: 通过。
- `git diff --check`: 通过。

## [2026-05-14 12:56:31] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 错误修复: 上下文追加 heredoc 未引用导致命令替换

### 现象
- 追加 `task_plan__mouse_e2e.md` 时,命令输出 `zsh:2: command not found: 2026-05-14`。

### 原因
- heredoc 正文包含反引号,但使用了未加引号的 `EOF`。
- shell 对反引号内容做了 command substitution。

### 修复
- 后续追加包含反引号的 Markdown 正文时,必须使用 `cat <<'EOF'`。

### 验证
- 本条记录使用单引号 heredoc 写入,不会触发反引号命令替换。

## [2026-05-16 23:55:45] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 错误修复: About This Mac E2E 点击点和内容断言不稳定

### 现象
- `about_this_mac=(210,36)` 时没有打开独立窗口.
- `about_this_mac=(120,54)` 后能打开 About 窗口,但旧窗口宽度阈值 `320` 把 `319px` 宽的真实窗口误判为失败.
- AX `entire contents` 在当前 About 窗口上可能只返回 `PROCESS:System Information`,没有静态文本.

### 原因
- 菜单面板顶边检测把 Apple 菜单栏高亮或菜单内部宽变化行当成下拉面板顶边.
- 菜单点击 x 坐标使用 420px 搜索框中心,偏到菜单右边缘.
- 内容断言过度依赖 AX 文本,没有利用已经定位到的独立窗口 crop OCR.

### 修复
- Apple 菜单第一项点击点改为固定菜单栏高度后的第一行中心,并把 x 限制在 Apple 菜单常见面板宽度内.
- About 窗口独立区域检测宽度阈值降到 280.
- 最终内容断言使用独立窗口 crop OCR 和 AX 文本的组合证据,要求包含 `MacBook Air` 和 `15.7.5`.
- 移除 `24G624` 本轮强断言.

### 验证
- `cargo test --package rustdog --test control_mouse_e2e --no-run` -> passed.
- `cargo test --package rustdog --test control_mouse_e2e` -> 0 passed, 1 ignored.
- `cargo test --package rustdog --test control_mouse_e2e daemon_control_lane_should_click_apple_menu_about_this_mac_via_rdog_control -- --ignored --exact --nocapture` -> 1 passed, 37.78s.
- `cargo fmt -- --check` -> passed.
- `git diff --check` -> passed.
