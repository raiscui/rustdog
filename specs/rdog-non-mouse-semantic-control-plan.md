# rdog 非鼠标语义控制协议方案

## 目标

让 `rdog` 的 GUI 控制默认优先使用:

- AX observation
- AX semantic action
- settable `AXValue`
- 显式 window activation

而不是默认退化成真实鼠标操作。

这份规格聚焦第一批落地能力:

- `@ax-action`
- `@ax-set-value`
- `@type-text`

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

- 第一版只支持 `mode:"ax-value"` 和 `mode:"auto"`。
- `mode:"auto"` 当前仍只走 AXValue 分支。
- `allow_clipboard` 已有协议字段,但默认必须是 `false`,当前不会自动用剪贴板。

返回:

```json
{"kind":"type-text","backend":"macos-accessibility","target_id":"pid:123/window:0/path:8.2","mode":"ax-value","delivered_via":"ax-value","performed":true,"status":"ok","used_clipboard":false}
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
    Choose -->|无法语义化| Fallback{允许干扰?}
    Fallback -->|否| Limited[返回 limited]
    Fallback -->|是| Mouse[@click/@drag/@wheel]
```

## 非目标范围

当前这版还没有落地:

- `@key delivery:"pid-targeted" | "window-targeted" | "global"`
- `@ax-focus`
- `@ax-scroll`
- 剪贴板 fallback

这些仍属于下一阶段扩展。
