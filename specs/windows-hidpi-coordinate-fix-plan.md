# Windows HiDPI 坐标链路修复方案

> 状态：**提案**（待拍板 3 项）
> 来源：2026-10-05 Windows 真机远程验证，HiDPI 屏上 `@click` 连续点空三次

## 目标

修复 Windows 高 DPI / 缩放 / 多屏场景下 `@click`、`@mouse-move`、`@drag`
落点偏移的问题，使 rdog 的坐标契约（`os-logical`）在 Windows 上真正成立。

这份文档是**根因分析 + 修复方案**，不是施工说明。

---

## 现象

2026-10-05 在 `rais-razer.local`（Windows 11 / 4K 屏 / 200% 系统缩放）远程验证时：

- `@screenshot` 返回 3840×2160 的图
- 同一个 manifest 声明 `coordinate_space: "os-logical"` 与
  `image_coordinate_space: "virtual-logical-pixels"`
- 但**没有给出逻辑虚拟桌面尺寸，也没有给出 scale factor**

agent 只能自行假设"3840 ÷ 2 = 1920 逻辑宽"来换算，**连续三次点空**，
每次点空后再截图核对、再重算，浪费约 20 分钟。

同一天 daemon 日志里反复出现：

```
info: get_hi_dpi_scale_factor failed: Process not DPI aware
```

---

## 根因（三层，逐层源码实证）

### 根因 1：daemon 进程在 Windows 上不是 DPI-aware

全仓搜索 `SetProcessDpiAwarenessContext` / `DPI_AWARE` / `dpiAware`：
**零命中** —— rdog 从未声明 DPI 感知。

后果（Windows 对 DPI-unaware 进程的既定行为）：

- `GetSystemMetrics` 系列返回**虚拟化后的逻辑值**而非物理值
- `GetDpiForMonitor` / xcap 的 `scale_factor()` 拿不到真实缩放
  （今天日志的 `get_hi_dpi_scale_factor failed` 就是这个失败的现场）

即：**rdog 在 Windows 上关于"这块屏多大"的所有认知都是错的**，
在 200% 缩放下会以为屏是 1920×1080 逻辑尺寸而实际物理 3840×2160。

### 根因 2：enigo 把逻辑尺寸当作归一化分母

`enigo-0.6.1/src/win/win_impl.rs:162-176`：

```rust
let (w, h) = self.main_display()?;        // DPI-unaware 下 = 逻辑尺寸
let w = w as i64 - 1;
let x = (x as i64 * 65535 + w / 2 * x.signum()) / w;   // 按逻辑尺寸归一化
(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, x as i32, y as i32)
```

rdog 的鼠标 backend（`src/control_mouse/backend.rs:16-19`）把协议里的
`os-logical` 坐标**原样透传**给 enigo，全程没有换算层。

在**单屏 + 统一缩放**下，rdog 传逻辑坐标、enigo 按逻辑尺寸归一化、
Windows 也按逻辑解释 —— 三者恰好对齐，坐标是对的。
**这解释了为什么 macOS 和早期 Windows 验证都没暴露问题**：
不是逻辑正确，是**误差恰好为零**。

### 根因 3：多屏 + 混合 DPI 下必然错

两个叠加问题：

1. `main_display()` **只取主屏**。enigo 自己的代码里留着未做的 TODO：
   ```rust
   // TODO: Check if we should use MOUSEEVENTF_VIRTUALDESK too
   ```
   没加 `MOUSEEVENTF_VIRTUALDESK`，坐标只在**主屏范围内**有效 ——
   副屏上的点会落回主屏。

2. Windows 允许**每屏不同缩放**（主屏 200% + 副屏 100% 是常见配置）。
   DPI-unaware 进程只有一个"系统 DPI"概念，无法表达"这块屏 scale 是 2，
   那块是 1"。此时无论传什么坐标，落点都是错的。

---

## 为什么现有文档没兜住

`specs/rdog-multi-display-screenshot-coordinate-plan.md` 立过
"manifest 是截图坐标与鼠标坐标之间的单一真相源"这条正确原则，
`specs/rdog-mouse-control-coordinate-plan.md` 也要求动作显式声明
`coordinate_space:"os-logical"`。

**原则是对的，但缺了让真相源可用的数据**：manifest 声明了两个坐标空间的
**名字**，却没给两者之间的**换算参数**。声明 `os-logical` 而不给逻辑屏尺寸，
等于只说了单位、没给标尺 —— agent 只能猜。

---

## 修复方案（三层，建议全部做）

### 第 1 层：让进程 DPI-aware（根因修复）

daemon 启动最早处（任何窗口创建之前）调用：

```rust
// Windows: 让整个进程进入 per-monitor aware v2
SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
```

放在 `main()` 的第一行附近，早于 rdoc/xcap/enigo 任何初始化。

**收益**：

- `GetSystemMetrics` 返回真实物理像素
- xcap 的 `scale_factor()` 返回真实缩放
- 多屏各自的 scale 可被枚举
- 为第 2、3 层提供正确地基

**代价**：需要给 `windows-sys` 增补 feature
（`Win32_UI_HiDpi`），并在 manifest 里声明（若走 exe 内嵌 manifest 而非 API 调用）。
**风险**：进程 DPI 语义一变，所有既有坐标相关代码都要复核（这正是本方案第 2 层的意义）。

> 注：macOS 侧不需要对应处理 —— AppKit 的 backingScaleFactor 一直是正确的，
> 今天 macOS 没出问题不是运气。

### 第 2 层：manifest 补齐换算参数（让 agent 不猜）

`screenshot` manifest 增加：

```json
{
  "coordinate_space": "os-logical",
  "image_coordinate_space": "virtual-logical-pixels",
  "virtual_bounds": { "x":0, "y":0, "width":3840, "height":2160 },
  "os_logical_bounds": { "x":0, "y":0, "width":1920, "height":1080 },
  "displays": [
    { "id":"65537", "os_rect":{...}, "scale_factor":2.0,
      "os_logical_rect":{...} }
  ]
}
```

**规则**：`os_logical_rect = os_rect / 该屏 scale_factor`。
agent 拿到这两个数就能精确换算，不必猜。

同时 `@click` 的响应回显**实际落点**，让点偏可被立即发现而非静默：

```json
{ "x":1093, "y":213, "resolved_os_point":{...}, "display_id":"65537",
  "applied_scale_factor":2.0 }
```

### 第 3 层：Windows 鼠标后端绕开 enigo 的归一化（多屏正确性）

enigo 的归一化方案在多屏 + 混合 DPI 下无解（`main_display()` 只取主屏、
不认 `MOUSEEVENTF_VIRTUALDESK`）。**这是 enigo 层面的缺陷，rdog 无法通过配置绕过。**

Windows 上直接用 Win32 原生：

- `SetCursorPos(x, y)` —— 绝对物理坐标，**无归一化、无主屏假设**
- `mouse_event` 或 `SendInput` 发按键/滚轮（相对量，不涉及坐标空间）

macOS/Linux 继续走 enigo（`CGEvent` / `XTest` 的坐标语义本来就正确）。

**收益**：多屏、混合 DPI、副屏点击全部正确，且不再依赖 enigo 的隐式假设。

**代价**：Windows 侧多一份后端代码（约 60-80 行），与现有 `MouseBackend` trait
对接即可，不影响测试注入结构。

---

## 待拍板

### 决策 1：第 3 层做不做

| 方案 | 覆盖 | 代价 |
| --- | --- | --- |
| A. 只做 1+2 层 | 单屏统一缩放正确 | **多屏仍错**（副屏点落主屏） |
| **B. 三层全做**（推荐） | 含多屏与混合 DPI | Windows 侧多一个后端 |

**推荐 B**。今天那台机器是单屏所以 A 看着够用，但"主屏 200% + 副屏 100%"
在真实办公环境很常见，而这类失败的现场是"点到了别的地方"—— 静默且危险。

### 决策 2：Windows 后端是替换还是叠加

| 方案 | 说明 |
| --- | --- |
| A. 替换（推荐） | Windows 全面走原生 `SetCursorPos`/`mouse_event`，enigo 只服务 macOS/Linux |
| B. 叠加 | Windows 保留 enigo 作为可选后端，供极端场景回退 |

**推荐 A**。留一条"坐标可能不对"的回退路径，等于留一个静默出错的口子。

### 决策 3：DPI awareness 的设置方式

| 方案 | 说明 |
| --- | --- |
| **A. 代码调 API**（推荐） | `SetProcessDpiAwarenessContext`，启动最早处调用，跨打包方式可靠 |
| B. exe manifest 声明 | 更"正统"，但要求改构建链，且不同打包方式要分别处理 |

**推荐 A**：与 electron-builder/其它打包方式解耦，且容易在单测里验证调用时机。

---

## 验证方法（每层独立可验）

| 层 | 验证方式 | 通过判据 |
| --- | --- | --- |
| 1 | Windows 上看 daemon 日志 | `get_hi_dpi_scale_factor failed` 消失；`scale_factor()` 返回 2.0 |
| 2 | 对比 manifest 的 `os_logical_bounds` 与实际逻辑屏 | 数值一致（可用 `GetSystemMetrics` 交叉验证） |
| 3 | 在副屏四角各点一次 | `@click` 响应里的 `resolved_os_point` 与视觉落点一致 |

回归测试：新增 Windows 平台测试，用多屏坐标样本覆盖
"主屏 / 副屏 / 跨屏边界 / 混合缩放"四类。

---

## 影响面

- `src/control_mouse/backend.rs` —— 新增 Windows 原生后端
- `src/screenshot.rs` —— manifest 增加 `os_logical_bounds` 与每屏 `scale_factor`
- `src/control_display_scope.rs` —— scale 数据源
- daemon 启动路径 —— 插入 DPI awareness 调用
- `Cargo.toml` —— `windows-sys` 增补 `Win32_UI_HiDpi`

**不影响** macOS/Linux 的既有行为（它们本就没有根因 1/3）。

---

## 相关

- [rdog 鼠标控制坐标规格](rdog-mouse-control-coordinate-plan.md) —— 现行 `os-logical` 契约来源
- [多屏截图与坐标规格](rdog-multi-display-screenshot-coordinate-plan.md) —— manifest 真相源原则
- [显示感知控制链规格](rdog-display-aware-control-chain-plan.md) —— display scope 与 guard
- [远程被控端实战反馈](../docs/solutions/best-practices/remote-control-blocked-command-pitfalls.md)
  —— P1-1 即本问题
