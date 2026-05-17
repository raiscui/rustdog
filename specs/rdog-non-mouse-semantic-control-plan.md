# rdog 非鼠标语义控制协议方案

## 目标

让 `rdog` 的 GUI 控制默认优先使用:

- AX observation
- AX semantic action
- settable `AXValue`
- 显式 window activation

而不是默认退化成真实鼠标操作。

这份规格当前已覆盖两批能力:

- `@ax-action`
- `@ax-set-value`
- `@type-text`
- `@key delivery`
- `@ax-focus`
- `@ax-scroll`

## 设计原则

1. 鼠标是显式 fallback,不是默认路径。
2. `@window-activate` 仍是唯一允许显式改变桌面可见状态的窗口恢复入口。
3. `os-logical` 仍是唯一坐标语义,本协议不引入第二套坐标解释。
4. 权限失败是一等结果:
   - Accessibility 权限不足: code `77`
   - 当前平台不支持: code `78`
5. 不默认使用剪贴板 fallback。

## 已落地范围

### `@ax-action`

示例:

```text
@ax-action#10:{target:{id:"pid:123/window:0/path:7.3"},action:"AXPress"}
@ax-action#11:{target:{id:"pid:123/window:0/path:9.1"},action:"AXShowMenu"}
```

当前 allowlist:

- `AXPress`
- `AXOpen`
- `AXConfirm`
- `AXCancel`
- `AXShowMenu`
- `AXScrollToVisible`

语义:

- parser 只接受 allowlist 内动作。
- 后端执行前还会检查目标元素当前暴露的 `actions[]` 是否真的包含该动作。
- `@ax-press` 继续保留,内部等价于 `@ax-action action:"AXPress"`。

返回:

```json
{"kind":"ax-action","action":"AXShowMenu","backend":"macos-accessibility","target_id":"pid:123/window:0/path:9.1","performed":true,"status":"ok"}
```

### `@ax-set-value`

示例:

```text
@ax-set-value#20:{target:{id:"pid:123/window:0/path:8.2"},value:"hello",mode:"replace"}
@ax-set-value#21:{target:{id:"pid:123/window:0/path:8.2"},value:" world",mode:"append"}
```

语义:

- 写入前必须检查 `AXUIElementIsAttributeSettable(AXValue)`。
- 不可写时返回错误,不自动 fallback 到键盘或剪贴板。
- `mode:"append"` 只有在当前 `AXValue` 可读时才允许执行。
- 如果当前值不可读,请求必须失败,不能静默退化成 replace。

返回:

```json
{"kind":"ax-set-value","backend":"macos-accessibility","target_id":"pid:123/window:0/path:8.2","mode":"append","performed":true,"status":"ok","settable":true,"old_value_redacted":true,"new_value_redacted":true}
```

### `@type-text`

示例:

```text
@type-text#30:{target:{id:"pid:123/window:0/path:8.2"},text:"hello",mode:"ax-value"}
@type-text#31:{target:{id:"pid:123/window:0/path:8.2"},text:"hello",mode:"auto"}
```

当前阶段语义:

- 支持 `mode:"ax-value"`、`mode:"targeted-keyboard"`、`mode:"clipboard"` 和 `mode:"auto"`。
- `@type-text` 的职责是普通文本输入,不是快捷键触发。
- `mode:"auto"` 按下面的梯子尝试:
  1. AXValue replace
  2. targeted keyboard
  3. clipboard,但只有 `allow_clipboard:true` 时才允许
- `mode:"clipboard"` 必须显式 `allow_clipboard:true`。
- clipboard 路径只会在目标写入期间临时借用系统剪贴板,并且只在剪贴板仍保持 rdog 写入内容时恢复旧值。
- clipboard 路径不是无热键方案。
  当前 macOS 实现仍通过剪贴板 + 定向 paste key 完成,所以只作为显式 fallback。
- response 里应额外暴露:
  - `clipboard_restore_policy:"restore-if-unchanged"`
  - `clipboard_restored:true|false`
  - `clipboard_restore_skipped_reason`
- `mode:"targeted-keyboard"` 仍可能受焦点、输入法、app 自己的键盘处理逻辑影响。
  它是“定向文本输入”路径,不是功能热键路径。
- response 里的 `delivered_via` 必须说真话:
  - `ax-value`
  - `targeted-keyboard`
  - `clipboard`

返回:

```json
{"kind":"type-text","backend":"macos-accessibility","target_id":"pid:123/window:0/path:8.2","mode":"ax-value","delivered_via":"ax-value","performed":true,"status":"ok","used_clipboard":false}
```

targeted keyboard / clipboard 示例:

```text
@type-text#32:{target:{id:"pid:123/window:0/path:8.2"},text:"hello",mode:"targeted-keyboard"}
@type-text#33:{target:{id:"pid:123/window:0/path:8.2"},text:"hello",mode:"clipboard",allow_clipboard:true}
```

clipboard response 示例:

```json
{"kind":"type-text","backend":"macos-clipboard+cg-event-post-to-pid","target_id":"pid:123/window:0/path:8.2","mode":"clipboard","delivered_via":"clipboard","performed":true,"status":"ok","used_clipboard":true,"clipboard_restore_policy":"restore-if-unchanged","clipboard_restored":true}
```

### `@paste`

示例:

```text
@paste
@paste#34
```

当前阶段语义:

- 裸 `@paste` 不带 payload,不带 target。
- 它表示“对当前远端前台焦点执行系统粘贴”。
- macOS 使用 `Cmd+V`;Windows / Linux 使用 `Ctrl+V`。
- response 必须暴露:
  - `delivery:"global-hotkey"`
  - `delivered_via:"cmd-v"` 或 `delivered_via:"ctrl-v"`
  - `used_hotkey:true`
  - `requires_focus:true`
- 它不是稳定普通文本输入接口。
- 旧 `@paste:"text"` 只作为 legacy text injection 兼容层保留,新 agent 应优先使用 `@type-text mode:"ax-value"` 或 `@ax-set-value`。

返回:

```json
{"kind":"paste","delivery":"global-hotkey","delivered_via":"cmd-v","used_hotkey":true,"used_keyboard":true,"requires_focus":true,"performed":true,"status":"ok"}
```

### `@key delivery`

示例:

```text
@key#40:{key:"Return",delivery:"pid-targeted",pid:556}
@key#41:{key:"Cmd+W",delivery:"window-targeted",window_id:"pid:556/window:0"}
@key#42:{key:"F11",delivery:"global"}
```

语义:

- `@key` 的主职责是:
  - 快捷键
  - 功能键
  - 导航键
  - 在特定 app / window 焦点下触发功能
- `@key` 不应被当作通用普通文本输入接口。
  如果目标是稳定写入文本,优先使用 `@ax-set-value` 或 `@type-text`。
- 旧字符串 payload 和旧 object payload 继续兼容,仍可走 legacy 成功响应。
- 只要 object payload 显式带 `delivery` / `pid` / `window_id`,成功响应就切到结构化 `kind:"key"` report。
- `delivery:"pid-targeted"` 需要 `pid`。
- `delivery:"window-targeted"` 需要 `window_id`。
- `delivery:"global"` 不能再带 `pid` / `window_id`。
- macOS 当前真实后端:
  - `global`: 仍走本地输入模拟
  - `pid-targeted` / `window-targeted`: 走 `CGEventPostToPid`

返回:

```json
{"kind":"key","backend":"macos-cg-event-post-to-pid","key":"Cmd+W","mode":"press_release","delivery":"window-targeted","target_pid":556,"window_id":"pid:556/window:0","performed":true,"status":"ok"}
```

### `@ax-focus`

示例:

```text
@ax-focus#50:{target:{id:"pid:123/window:0/path:8.2"}}
@ax-focus#51:{window_id:"pid:123/window:0",activate:true}
```

语义:

- `target` 和 `window_id` 二选一。
- 默认 `activate:false`。
- 只有显式 `activate:true` 时,执行层才允许先复用 `@window-activate` 做窗口恢复。
- `activate:true` 仍不代表隐式 mouse fallback。

返回:

```json
{"kind":"ax-focus","backend":"macos-accessibility","target_id":"pid:123/window:0/path:8.2","activated":false,"performed":true,"status":"ok"}
```

### `@ax-scroll`

示例:

```text
@ax-scroll#60:{target:{id:"pid:123/window:0/path:10.1"},direction:"down",pages:2}
```

语义:

- 当前命令名仍叫 `@ax-scroll`,macOS 主路径会在目标窗口内查找同方向的 `AXScrollBar`,再写入它的 `AXValue`。
- 不会偷偷退化成全局 wheel。
- response 必须回报 `delivered_via:"ax-scrollbar-value"`。
  `line_steps` 在这条语义路径上为 `0`,避免把 AXValue 比例写入伪装成系统 line wheel。

返回:

```json
{"kind":"ax-scroll","backend":"macos-accessibility","target_id":"pid:123/window:0/path:10.1","direction":"down","pages":2,"line_steps":0,"delivered_via":"ax-scrollbar-value","performed":true,"status":"ok"}
```

## Agent 决策流

```mermaid
flowchart TD
    Start[需要 GUI 操作] --> Observe[@screenshot include_ax 或 @ax-tree]
    Observe --> Find[@ax-find 或 @window-find]
    Find --> Ready{窗口可交互?}
    Ready -->|否| Activate[@window-activate]
    Activate --> FindAgain[@ax-find 或 @ax-tree]
    Ready -->|是| Choose{目标类型}
    FindAgain --> Choose
    Choose -->|按钮/菜单| Action[@ax-action 或 @ax-press]
    Choose -->|文本输入| Value[@ax-set-value 或 @type-text]
    Choose -->|快捷键/功能触发| Key[@key]
    Choose -->|无法语义化| Fallback{允许干扰?}
    Fallback -->|否| Limited[返回 limited]
    Fallback -->|是| Mouse[@click/@drag/@wheel]
```

## 非目标范围

当前仍然没有落地:

- 非 macOS 的 targeted keyboard / AX focus / AX scroll / clipboard 后端
- `app` / `bundle_id` 级别的 `@key` / `@type-text` 定向投递
- 更强的 page-level AX scroll action fallback
