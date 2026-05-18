
## [2026-05-14 14:42:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: About 窗口版本号无需点击

### 现象
- 用户确认版本号和 build 号已经显示在 About 界面上,无需点击版本号文本。
- 当前 `tests/control_mouse_e2e.rs` 在首次打开 About 窗口后仍执行 `derive_about_version_click_target` 和第二轮 `@click`。

### 假设
- 主假设: 当前测试的失败方向来自旧的交互假设,应改成打开窗口后直接验证窗口文本。
- 备选解释: AX 读取脚本只抓取了窗口一层 static text,遗漏了实际 UI 子树里可见的 build 号。

### 验证计划
- 移除二次点击版本号流程。
- 将内容断言集中在第一次打开的独立窗口上,要求同一窗口 AX 文本包含 `MacBook Air`、`15.7.5`、`24G624`。
- 将 AX 文本读取改为递归读取 `entire contents of w` 的 value/name/description,提高覆盖率。
- 重新运行格式化、编译、默认 ignored 测试和真实 ignored E2E。

## [2026-05-14 14:57:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: build 号点击语义校正

### 现象
- 用户确认 `24G624` 是点击版本号后才会读到,未点击没读到不算失败。
- 当前测试仍残留 `EXPECTED_ABOUT_BUILD` 强断言,会把正常状态误判为失败。

### 结论
- 本轮 E2E 不再验证 build 号。
- 如果未来要覆盖 build 号,应该作为另一个显式测试分支: 先验证打开 About 窗口,再通过 rdog 点击版本号 static text 中心,然后断言 `24G624`。

## [2026-05-16 23:55:45] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: About 窗口证据收口

### 关键证据
- 通过截图差分定位到独立 About 窗口区域后, crop OCR 读到了 `MacBook Air` 和 `macOS Sequoia 15.7.5`.
- 同一轮 AX 文本只返回 `PROCESS:System Information`,说明当前环境下 AX 递归文本不能作为唯一成功证据.
- 最终通过轮次的点击点是 `about_this_mac=(110,54)`,菜单面板图像区域是 `LogicalRect { x: 0, y: 160, width: 221, height: 303 }`.

### 结论
- 当前 E2E 的硬证据链是: screenshot manifest 的 `os-logical` 坐标 -> Apple 菜单打开截图 -> 菜单第一项点击 -> 独立窗口区域 -> crop OCR/AX 文本组合断言.
- `24G624` 是未来可以单独补的版本号展开测试,不属于本轮基础 About 窗口打开验收.

## [2026-05-17 00:12:35] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: diff 窗口检测漏判

### 现象
- 真实 live E2E 失败提示没有 large new window region。
- 失败轮次 final screenshot 中可以直接看到独立“关于本机”窗口。
- changed candidates 被拆成多个小区域,例如 `152x86`、`516x14`、`33x484`、`37x464`,没有一个单独 component 同时满足窗口宽高阈值。

### 假设与验证
- 主假设: 点击流程已成功,失败来自 diff component 对半透明窗口/文字/边缘的分块方式。
- 动态证据: final screenshot 已包含独立 About 窗口,且窗口在 primary display 上可见。
- 备选解释“点击没命中”被截图推翻。

### 结论
- diff-based large component 不适合作为唯一窗口定位真相源。
- 下一步应优先使用 AX 窗口几何定位 About 窗口 crop,继续用 screenshot manifest 的 `virtual_bounds` 把 OS logical rect 转成 composite image rect。
- 截图 diff 保留为 fallback 与失败诊断信息。
