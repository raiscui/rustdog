## [2026-05-17 00:29:01] [Session ID: codex-20260517-non-mouse-control-research] 笔记: open-codex-computer-use 非鼠标控制调研

### 来源
- GitHub 仓库: https://github.com/iFurySt/open-codex-computer-use
- 本地浅 clone: `/tmp/open-codex-computer-use-research`
- 重点文件:
  - `README.md`
  - `docs/ARCHITECTURE.md`
  - `skills/open-computer-use/SKILL.md`
  - `skills/open-computer-use/references/usage.md`
  - `skills/open-computer-use/references/troubleshooting.md`
  - `packages/OpenComputerUseKit/Sources/OpenComputerUseKit/ToolDefinitions.swift`
  - `packages/OpenComputerUseKit/Sources/OpenComputerUseKit/ComputerUseService.swift`
  - `packages/OpenComputerUseKit/Sources/OpenComputerUseKit/InputSimulation.swift`
  - `packages/OpenComputerUseKit/Sources/OpenComputerUseKit/AccessibilitySnapshot.swift`

### 关键观察
- 该项目的公开工具面是 9 个 Computer Use tools: `list_apps`, `get_app_state`, `click`, `perform_secondary_action`, `scroll`, `drag`, `type_text`, `press_key`, `set_value`.
- 它不是纯鼠标自动化。macOS 主实现会先尝试 AX 语义动作:
  - `perform_secondary_action` 直接执行元素暴露的 AX action.
  - `click` 内部先尝试 `AXSelectedChildren`, `AXPress`, `AXConfirm`, `AXOpen`, `AXShowMenu`.
  - `set_value` 只对 `AXValue` 可设置元素写值.
  - `type_text` 先尝试对 focused settable element 追加写回 `AXValue`,再考虑键盘事件.
  - `press_key` 和 `type_text` 使用 `postToPid`,不是全局前台键盘事件.
- 坐标类动作在该项目里仍存在,但默认优先定向到目标 pid。全局 pointer fallback 需要显式环境变量 `OPEN_COMPUTER_USE_ALLOW_GLOBAL_POINTER_FALLBACKS=1`.
- `get_app_state` 会返回窗口标题、窗口 bounds、截图、AX tree、focused element、selected text 和 element index 映射。它会尝试恢复 hidden/minimized/不可见窗口,这会改变桌面状态。
- Windows runtime 使用 UI Automation + Win32 message fallback,并明确不默认 auto-launch app / SetFocus / UIA text fallback,以减少抢前台。
- Linux runtime 使用 AT-SPI2/D-Bus accessibility,但 Wayland 下截图和 coordinate fallback 是 best-effort.
- 仓库没有独立暴露 clipboard tool,也没有单独的 window-find/window-close API。窗口恢复更多藏在 `get_app_state` 内部。

### 对 rustdog 的含义
- `rdog` 现在已有 `@ax-tree`, `@ax-find`, `@ax-get`, `@ax-press`, `@window-find`, `@window-activate`, `@window-close`,以及鼠标命令。
- 要避免干扰人类,下一步不应该继续强化鼠标测试,而应该补齐 non-mouse action layer:
  - `@ax-action`: 泛化 `AXPress`,允许执行元素暴露的安全 action,如 `AXOpen`, `AXConfirm`, `AXShowMenu`, `AXCancel`.
  - `@ax-set-value`: 写 settable `AXValue`.
  - `@ax-focus`: 对文本输入或窗口设置 focused/main,但必须显式调用.
  - `@key` / `@type-text` 的 pid-targeted 或 window-targeted 模式,避免全局键盘抢焦点.
  - `@scroll` 优先 AX page action,再考虑 pid-targeted event,最后才是显式 opt-in 的全局 pointer fallback.
- `rdog` 比 open-computer-use 更适合保留显式 window tools。不要把 window 激活藏进 `@ax-tree` 或 `@screenshot`;应该继续让 agent 明确调用 `@window-activate`.
- `rdog` 应当把“非鼠标模式”做成策略字段或 profile,而不是让所有命令静默 fallback 到鼠标。

### 当前结论
- 可以借鉴 open-codex-computer-use 的核心分层: observe -> element index / target locator -> semantic action -> targeted event fallback -> explicit global pointer fallback.
- 但 `rdog` 的协议面应该更显式,因为它是远程控制工具,需要 agent 能审计每一次是否改变窗口状态或使用鼠标。
- 本轮没有运行任何真实鼠标操作。
