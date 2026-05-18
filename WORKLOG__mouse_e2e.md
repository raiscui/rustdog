## [2026-05-14 12:33:20] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: macOS Apple 菜单鼠标 E2E

### 任务内容
- 创建一个 macOS-only 的 `rdog control` 鼠标 E2E 测试。
- 测试动作包括移动到左上角 Apple 图标、点击 Apple 菜单、再点击“关于本机”。
- 测试默认 ignored,避免常规 CI 或普通 `cargo test` 真实移动鼠标。

### 完成过程
- 新增 `tests/control_mouse_e2e.rs`。
- 测试启动本地 `rdog daemon` 的 TCP control lane。
- 先执行 `@screenshot#30`,读取保存到 `rdog_downloads` 的 screenshot manifest。
- 从 primary display 的 `os_rect` 推导 Apple 菜单和“关于本机”的 `os-logical` 坐标。
- 发送 `@mouse-move` 和 `@click` 序列,最后执行 `@screenshot#37` 作为视觉证据。

### 总结感悟
- 真实 GUI E2E 应该独立成测试文件,不要继续膨胀已有 control lane 集成测试。
- 鼠标坐标继续以 screenshot manifest 为唯一真相源,不在测试里新增第二套坐标解释。

## [2026-05-14 12:52:35] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: Apple 菜单第二次点击动态定位修正

### 任务内容
- 修正 macOS 鼠标 E2E 的“关于本机”点击方式。
- 从菜单打开后的截图差异里推导菜单面板和第一项点击坐标。
- 增加最终截图视觉断言,避免只看协议 response 的假阳性。

### 完成过程
- 将初始截图、菜单截图、最终截图都作为测试内证据读取。
- 保持 screenshot manifest 的 `os-logical` 作为唯一坐标真相源。
- 真实 ignored E2E 复跑通过,输出 Apple 图标坐标 `(14,12)` 和“关于本机”坐标 `(112,36)`。

### 总结感悟
- GUI E2E 不能只验证控制协议成功,必须验证可见 UI 状态。
- 固定菜单项偏移容易随系统状态和菜单实际布局漂移,用截图证据推导坐标更稳。

## [2026-05-16 23:55:45] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: About This Mac 鼠标 E2E 硬验收收口

### 任务内容
- 完成 `tests/control_mouse_e2e.rs` 的真实 macOS 鼠标 E2E 验证.
- 成功标准固定为: rdog 通过真实 mouse control 打开独立 About This Mac 窗口,并在该窗口 crop OCR 或 AX 文本中读到 `MacBook Air` 和 `15.7.5`.
- 不再把 `24G624` 作为本轮未点击版本号流程的强断言.

### 完成过程
- 修正 Apple 菜单第一项点击点: x 轴取 Apple 菜单面板真实宽度上限内的中心,y 轴固定为 primary display 顶部加菜单栏高度再加第一行中心偏移.
- 将独立 About 窗口检测宽度阈值从 320 降到 280,避免真实窗口 `319px` 宽时被差 1 像素误判.
- 将内容断言改为独立窗口 crop OCR 和 AX 文本双路径;当前 AX 文本不稳定时,crop OCR 仍能提供来自独立窗口区域的证据.
- 真实 ignored E2E 最终通过,输出 `about_this_mac=(110,54)` 和独立窗口 crop OCR 文本.

### 总结感悟
- GUI E2E 的内容证据必须先绑定到独立窗口区域,再做 OCR/AX 文本断言;整屏 OCR 很容易吃到终端或聊天内容.
- Apple 菜单打开后的差分图会包含菜单栏高亮,不要把这个高亮当作下拉面板顶边.
- 对真实系统窗口尺寸的阈值要留合理余量,不能用恰好卡边的像素数做产品成功标准.
