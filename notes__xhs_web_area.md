## [2026-07-24 12:46:33] [Session ID: omx-1784789038072-clve0o] 笔记: Xiaohongshu AXWebArea 稳定红灯

## 来源

### 来源1: 当前 live daemon 和 Chrome

- 探针命令:
  - `rtk proxy python3 target/rdog-debug/xhs-web-area/probe_xhs_web_area.py --window-id 'pid:63270/window:0' --iterations 3 --delay-seconds 1 --timeout-seconds 60 --output-dir target/rdog-debug/xhs-web-area/probe-20260724-124542`
- 窗口所有权:
  - left display 上两扇 Chrome 窗口中,`pid:63270/window:0` 为唯一 `focused:true` Chrome AXWindow。
  - 激活后新建标签导航到 `xiaohongshu.com/explore`,窗口标题为 `小红书 - 你的生活兴趣社区 - Google Chrome - Rais`。
- 三轮对比:
  - targeted observe:3/3 `status:"complete"`,3/3 `truncated:true`,每轮返回4个 shallow AX element。
  - targeted `@ax-find role:"AXWebArea"`:3/3 `match_count:1`,3/3 `truncated:false`,window id 均为同一 `pid:63270/window:0`。
  - targeted `@web-find text:"首页"`:3/3 `match_count:0`,3/3 `error_code:"AX_WEB_AREA_NOT_FOUND"`。
  - trace 稳定停在 `capture-ax-snapshot:complete -> target-browser-window:ok -> find-ax-web-area:not_found`。
- 耗时:
  - targeted observe:0.564到1.157秒。
  - `@ax-find`:0.638到1.102秒。
  - `@web-find`:4.440到5.921秒。
- 探针退出码为1,这是对精确 bug pattern 的 RED 断言,不是脚本崩溃。

### 来源2: 历史 Xiaohongshu 回归证据

- `archive/branch_contexts/computer_use_density/ERRORFIX__computer_use_density.md` 记录2026-05-29的同类 false negative:浅层窗口快照可找到 WebArea,但深层首页链接未进入匹配树。
- 当时的修复是在已找到 WebArea 但页面匹配为0时刷新 WebArea 子树;live trace 为 `refresh-web-area-subtree:ok`。
- `archive/branch_contexts/xhs_home_click/WORKLOG__xhs_home_click.md` 记录2026-06-02 window-scoped `@web-find` 曾找到唯一 `AXLink.description:"首页"`,后续 `AXPress` 返回 `performed:true`。
- 历史修复处理的是 "WebArea已存在,页面匹配为0";当前失败发生得更早,是 "当前 snapshot 里找不到 WebArea"。

## 排序假设

1. 主假设:`@web-find` 使用的全局浅快照在目标窗口 WebArea 进入树之前已截断。
   - 预测:调用链会先 capture 全局 snapshot,后选 target AXWindow;定向从 target window root 刷新子树后会找到 WebArea。
   - 推翻证据:如果 `@web-find` 的 snapshot 已完整包含目标窗口 WebArea,但特殊过滤仍丢弃它,则截断不是根因。
2. 备选假设:`find-ax-web-area` 只遍历 target window 快照中的已展开节点,且没有在 WebArea 缺失时刷新 window subtree。
   - 预测:历史 `refresh-web-area-subtree` 仅在 WebArea 已定位后触发;不会处理当前更早的 blocker。
   - 推翻证据:如果已有 WebArea-missing 时的 window subtree refresh 且本轮确实执行,该假设不成立。
3. 备选假设:target window inventory id 和 AX snapshot window id 的归属错位。
   - 预测:`target-browser-window:ok` 选中的 snapshot AXWindow 不是 `pid:63270/window:0`,或它的 title/rect 与 live targeted AX 结果不一致。
   - 当前反证:三轮 trace 和 `@ax-find` 都指向同一 window id,所以优先级低于假设1和2。
4. 备选假设:页面加载时序过早,Chrome 当时没有暴露 WebArea。
   - 推翻证据:每轮紧邻的 targeted `@ax-find` 都找到同一 WebArea,而且三轮跨越20余秒。
   - 当前结论:这一解释已被当前动态证据推翻,不再作为主要方向。

## 下一步验证

- 用 CodeGraph 追踪 `@web-find` 到 `find-ax-web-area` 的调用链,确认 snapshot 预算、target window 过滤和 subtree refresh 的先后顺序。
- 找到公共 seam 后,用 "浅层 target window 不含 WebArea,定向 window subtree 含 WebArea 和首页" 构造 RED 测试。

## [2026-07-24 13:05:00] [Session ID: omx-1784789038072-clve0o] 笔记: 静态路径与根因结论

### 静态证据

- `build_default_web_find_response_json` 对所有 target 都先调用 `capture_default_ax_snapshot(&request.tree_request())`。
- `resolve_web_matches` 在 snapshot 已建好之后才调用 `select_target_window`,所以 `target.window_id` 没有参与 capture root 选择。
- 当 selected window 的 shallow elements 中没有 WebArea,`resolve_web_matches` 立即返回 `WebAreaNotFound`;既有 `refresh_web_area_matches` 只在 WebArea 已存在且 page match为0时执行。
- `@ax-find window_id` 使用 `capture_ax_find_snapshot_with`,对显式窗口直接调用 `capture_current_ax_window_snapshot`,而不是全局 snapshot。
- macOS 全局 snapshot 共享一个 `BuildState.element_count`;`state.element_count >= request.max_elements` 时会停止后续窗口/子树展开并标记 `truncated:true`。

### 已验证结论

- 失败轮里怀疑的路径确实发生:trace 和源码一致,是 global capture -> target window -> find WebArea -> early return。
- 两条对照路径操作的是同一 fresh 主窗口,window id、title、rect、focused 都一致。
- 全局路径的 snapshot `ref_count:2013,truncated:true`,但目标窗口子树没有 WebArea;紧邻 targeted 路径 `truncated:false,match_count:1`。
- 因此,当前 3/3 精确失败的已验证根因是:显式 window-scoped `@web-find` 没有把 target 下推到 AX capture,而是先使用共享全局预算快照;全局截断使正确窗口的 WebArea 子树未被展开。

### 2500 预算实验的口径回滚

- 数分钟后重用原 `window_id` 的2500预算实验选中了同字符串但几何不同的 transient Chrome AXWindow。
- 该实验违反 fresh window id 门禁,不能用于解释最初3/3失败。
- 它只表明 `pid/window_index` 是短期 identity,过期后可能重绑到其他 AXWindow;正式修复不会把这一 stale 结果当成根因。

### 回归 seam

- 用户可观察公共行为是 `build_default_web_find_response_json(request)` 在显式 `target.window_id` 下的 response。
- 测试使用 capture dependency injection 跑完整 response path:全局 fixture为truncated且缺 WebArea,targeted window fixture包含 WebArea和首页。
- RED 必须证明当前生产路径选了 global capture;绿灯必须证明显式 target 选择 targeted window capture 并返回首页。
- 这个 seam 同时保留 active-browser 无显式窗口时的 global capture 不变量,不会破坏多窗口消歧逻辑。

## [2026-07-25 01:30:24] [Session ID: omx-1784789038072-clve0o] 笔记: active two-stage capture最终验证

### 新对照证据

- Bonsai v5 active response的window为正确focused Chrome,但global snapshot缺WebArea。
- Codex direct紧邻显式window exact find为1,显式web-act为`performed:true`,`verified:true`。
- 因此选错窗口、locator错误和页面未加载解释均被同页对照推翻。

### 实现

- active请求先对global snapshot调用既有`select_target_window`,只提取唯一fresh backend id。
- 随后调用`capture_current_ax_window_snapshot`;最终response在targeted snapshot上重新执行完整target/display/match门禁。
- ambiguous/not-found保留global结果;targeted IO失败回退global。
- `@key` parser接受非空、无空白bare payload。quoted/object语法保持兼容。

### 验证

- 精确RED:active window capture 0次,bare key parse失败。
- GREEN:2项精确测试通过;control_web/control_protocol共100项通过;fmt通过;cargo check 0 errors。
- live artifact:`target/rdog-debug/xhs-web-area/active-two-stage-live-20260724-160000`。
- live active find为`complete/1`,active act为performed/verified,bare key与reset通过。
- 下游Pi v6 5/5 scored成功,证明修复覆盖真实多轮agent路径。
