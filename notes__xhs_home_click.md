## [2026-05-27 12:12:30] [Session ID: codex-native-20260527-111938] 笔记: 小红书“首页”点击任务复盘

## 现象

- 用户要求点击左侧显示器浏览器内小红书 website 的“首页”。
- `@ax-get#8` 曾在 Chrome `AXWebArea` 深层捕捉到 `AXLink.description:"首页"`。
- `@ax-action#9` 对 fresh ref 执行 `AXPress` 时返回 `STALE_REF`。
- 我随后按 AX rect 中心点 `(74,298)` 执行了 `@click#10`,返回 `status:"ok"`。
- 点击后打开的验证截图显示左屏是 GitHub / Finder 叠层,不是当前小红书页面。用户明确指出这是老旧内容。

## 断裂点

### 1. 我把 AX 目标证据和视觉真相源混在了一起

- AX 证据证明某个 Chrome 窗口历史上/当前 AX snapshot 中存在“小红书 - 你的生活兴趣社区”标题和 `AXLink.description:"首页"`。
- 但视觉截图证明左侧显示器当前可见内容不是小红书页面。
- 我没有在点击前先用截图确认“小红书窗口在左屏可见且没有被 GitHub/Finder 遮挡”。

### 2. `@window-find` 已经暴露了风险,但我没有及时停下

- `@window-find#4` 返回 Chrome 小红书窗口 `frontmost:false`, `occluded:true`, `interactable:false`。
- 正确做法应该是激活后立刻截图验证窗口是否真的可见、是否还是用户说的“左边显示浏览器”。
- 我只看到 `@window-activate#5 status:"ok"`,就继续深读 AX,没有把“视觉当前态”作为门槛。

### 3. 坐标 fallback 的前提不成立

- 坐标 fallback 只能在“目标窗口可见且没有遮挡”的前提下使用。
- 这次 fallback 使用的是 AX rect 中心点 `(74,298)`,但没有先证明该 rect 对应的当前屏幕像素仍然是小红书“首页”。
- 后续截图显示同一坐标区域落在 GitHub/Finder 可见画面附近,所以不能 claim 点击成功。

### 4. 耗时变长的直接原因

- 我围绕 Chrome AX ref/path 易失问题做了多轮尝试: path id, observation ref, selector re-find, fresh ref。
- 这些尝试在技术上解释了为什么 `AXPress` 失败,但没有解决用户实际目标: 当前左屏可见页面上的“首页”。
- 根本流程问题是: 我把“找到一个小红书 AX 节点”当成主线,而不是先固定“当前左屏视觉真相源”。

## 结论

- `@click#10 status:"ok"` 只证明 rdog 发出了鼠标点击,不证明任务完成。
- 本轮任务应回滚为“未完成,已发现目标窗口/视觉真相源不一致”。
- 后续如果继续执行,必须先用 fresh screenshot 识别左屏当前浏览器窗口和真实页面,必要时先关闭/移开遮挡窗口,再点击。

## [2026-06-01 15:01:15] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 笔记: 首页点击真实 e2e 闭环经验

## 来源

### 来源1: live `rdog control mac.lab`

- `@ping#1`: target 可达。
- `@bootstrap#2`: 新 daemon 返回 `rdog.bootstrap.v1`,screenshot / accessibility / mouse / window lane 可用。
- `@web-find#5` 和 `@web-find#9`: 均返回 `BROWSER_WINDOW_AMBIGUOUS`,原因是当前有 4 个 Chrome browser 窗口且没有唯一 focused browser window。
- `@window-find#7`: 找到小红书窗口 `pid:96405/window:3`,标题 `小红书 - 你的生活兴趣社区 - Google Chrome - Rais`。
- `@window-activate#8`: 对小红书窗口返回 `status:"ok"`。
- `@click#10` / `@click#13`: 坐标点击返回 ok,但截图显示打开了笔记详情页,不能算成功。
- `@key#16:"Escape"`: 关闭详情层,恢复到瀑布流。
- `@click#19:{x:48,y:255,...}`: 点击首页区域后,截图显示瀑布流内容刷新。

## 综合发现

### 成功口径

- `@click status:"ok"` 是动作证据,不是完成证据。
- 小红书“首页”点击必须用 before/after feed screenshot 验证。
- 本轮有效 before/after:
  - before: `rdog_downloads/screenshot-1780297129609-virtual-desktop.jpg`。
  - after: `rdog_downloads/screenshot-1780297167647-virtual-desktop.jpg`。
  - crop: `/tmp/xhs-feed-before.jpg` vs `/tmp/xhs-feed-after.jpg`。
  - `imgdiff`: different pixels `304233`。

### 方法经验

- `@bootstrap` 把起手探测从 `@ping + @capabilities + @observe` 收成一次请求,速度和证据密度都更好。
- 多 Chrome 窗口时,当前 `@web-find target:{browser:"active"}` 不够,会被 active browser ambiguity 卡住。
- 在 `@web-find` target schema 没有 `window_id` 之前,最快路径是:
  1. `@bootstrap` 获取当前视觉/AX/窗口证据。
  2. `@window-find` 确认小红书窗口存在且可交互。
  3. 必要时 `@window-activate`。
  4. 用 fresh screenshot 裁剪左侧导航,选定坐标 fallback。
  5. 点击后必须截图并对 feed 区域做 diff。

### 后续产品化入口

- `@gui-probe` 或 `@web-find` 增强时,优先补 `target.window_id` / `target.window_ref`。
- `@gui-probe` 可以把 “window-find -> scoped web find -> before screenshot -> action -> after screenshot diff” 做成任务级只读/可选动作流程。
