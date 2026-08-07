# Participating Window与geometry compiler笔记

## [2026-07-23 17:27:53] [Session ID: omx-1784512435044-92wxat] 笔记: 既有契约与第一决策边界

## 来源

### `CONTEXT.md`

Canonical definition:

> "Recording Session 中成为操作目标,或被用户主动移动、缩放的窗口。只有这类窗口属于回放环境恢复范围。"

因此"可见"、"frontmost"或"遮挡目标窗口"本身都不构成participation。

### `specs/rdog-recording-journal-model.md`

直接约束:

> "Participating Window 首次出现时,`window_snapshot` 保存:"
>
> "runtime locator hints。"
>
> "durable selector 或 selector 构造事实。"
>
> "outer rect,单位 `os-logical`。"
>
> "display key、window state 和 observation provenance。"
>
> "participating reason,例如 target、move 或 resize。"

同一规格要求move/resize/state变化追加完整snapshot,并禁止把runtime window id当永久target。

### `specs/rdog-recording-session-lifecycle.md`

直接约束:

> "Participating Window 的 geometry precondition 使用显式 `@window-resize`,包含 origin、outer size、display guard 和 fresh verification。"
>
> "App clamp 或 geometry verify failure 必须显式失败,不能假装窗口已恢复。"

### `specs/rdog-recording-semantic-promotion-policy.md`

- Geometry restore发生在semantic re-find之前。
- Geometry freshness不能替代semantic target freshness。
- Coordinate fallback要求Participating Window identity、focus、rect、display和verifier均fresh。

### 当前源码静态证据

CodeGraph和定点源码阅读确认:

- `WindowResizeRequest`已有`target`、`size`、`origin`、optional display `guard`和`verify.tolerance_px`。
- `WindowResizeSize`只支持`unit:os-logical`与`box:outer`。
- `WindowResizeOrigin`支持`keep`或绝对point。
- macOS `resize`先用fresh `@window-find`语义解析单一window,再执行unhide/unminimize/activate/raise恢复步骤。
- Backend设置size和optional position后重新读取outer rect,区分`WINDOW_RESIZE_CLAMPED`、`WINDOW_RESIZE_VERIFY_FAILED`和`WINDOW_RESIZE_GUARD_FAILED`。
- Display guard在after rect上验证,不是盲信请求坐标。

这些是静态事实,本轮尚未运行live resize,也不需要用live side effect回答participation产品决策。

## 第一决策的两个方向

### 推荐: 事件驱动纳入

Window仅在首次发生target-bound recorded operation时成为Participating Window:

- 内容click、text、shortcut、scroll或drag明确归属该window。
- 显式window activate/focus操作明确归属该window。
- Human主动move或resize该window。

仅仅可见、frontmost、被snapshot观察到、遮挡其他window或属于同一app,都不纳入。

优点是严格符合glossary,不会把整个desktop变成恢复目标,也不会为无关窗口生成`@window-resize`。

### 备选: Session启动时纳入frontmost/visible windows

优点是更容易获得first action之前的geometry。缺点是过度恢复,可能移动human没有操作的窗口,与map的out-of-scope边界冲突。

## 后续依赖

Human确认participation trigger后,下一轮才能决定:

- 首次snapshot是在动作前还是动作后冻结。
- move/resize如何区分initial precondition和录制中intentional geometry action。
- transient child surface是否并入owner window。
- missing/ambiguous window与display topology drift如何处理。

## [2026-07-23 21:34:31] [Session ID: omx-1784512435044-92wxat] 笔记: Human确认事件驱动participation

## 已确认规则

- Window只有在target-bound recorded operation、显式activate/focus,或human主动move/resize首次发生时才成为Participating Window。
- 仅可见、frontmost、被snapshot观察、遮挡其他窗口或属于同一app,不会触发participation。
- Session启动不预纳入全部可见窗口,Replay也不恢复整个desktop。

## 下一决策的两个方向

### 推荐: 冻结首个参与动作之前的fresh geometry

- Canonical initial `window_snapshot`必须代表首个参与动作执行前的窗口状态。
- Recorder可以观察尚未participate的窗口并维护短期geometry cache,但观察本身不会把窗口纳入Participating Window。
- 首次动作只负责将对应窗口纳入,并引用动作前最后一份满足freshness要求的geometry observation。
- 如果没有可证明属于动作前且足够fresh的snapshot,不得拿动作后的rect冒充precondition。具体是记录gap、令该窗口不可编译,还是终止录制,留给后续失败策略决策。

这个方向可以正确处理点击全屏、点击缩放控件、拖动标题栏等会立刻改变窗口geometry的首个动作。

### 备选: 在首个参与动作后读取geometry

实现更简单,但首个动作可能已经改变窗口位置、大小或state。这样编译出的precondition会恢复到动作后的状态,无法重现动作发生前的环境。

## [2026-07-23 21:54:58] [Session ID: omx-1784512435044-92wxat] 笔记: Human确认pre-action initial geometry

## 已确认规则

- Initial `window_snapshot`表示首个参与动作发生前的窗口outer rect、display和state。
- Recorder可以为未participate窗口维护短期observation cache,但不得因此扩大Participating Window集合。
- 首次参与时必须引用动作前的fresh observation。动作后的snapshot只能描述动作结果或后续状态,不能回填initial precondition。
- 缺少fresh pre-action observation时不得伪造。最终是拒绝该window的编译还是令整个Recording失败,在失败策略轮次决定。

## 下一决策的两个方向

### 推荐: Precondition与intentional geometry action严格分离

- 每个Participating Window只有一份initial geometry precondition,来自首次参与动作前。
- Human主动move/resize是需要重放的intentional action,无论它是否也是该window的首次participation trigger。
- 如果move/resize是首次参与事件,先引用拖动或缩放开始前的initial snapshot,再记录操作稳定结束后的完整snapshot作为action result。
- 一次持续拖动或缩放产生的高频通知只收敛为一个动作。中间rect可留作诊断证据,但不逐条编译为`@window-resize`。
- 编译时先用initial snapshot生成geometry precondition。时间线上每个已收敛的move/resize再生成一个显式`@window-resize`,并使用动作后的完整outer rect做fresh verification。

### 备选: 只保留最终geometry

把录制期间所有move/resize折叠进一个最终precondition,脚本更短,但会改变后续操作发生时的窗口布局和坐标上下文。它无法忠实表达human主动调整窗口后再继续操作的时间顺序。

## [2026-07-23 23:43:04] [Session ID: omx-1784512435044-92wxat] 笔记: Human确认move/resize action边界

## 已确认规则

- 每个Participating Window只有一份initial geometry precondition。
- Human主动move/resize是独立intentional action,不能折叠进最终precondition。
- 高频geometry通知按一次连续手势收敛,只把稳定后的完整snapshot编译为一个`@window-resize`。
- 如果move/resize触发首次participation,动作前snapshot是initial precondition,动作后snapshot是该action的verification目标。

## 下一决策的两个方向

### 推荐: Transient child surface并入owner window

- Attached sheet、popover、menu和tooltip不获得独立Participating Window identity,也不生成geometry precondition或`@window-resize`。
- 对这些surface的操作仍归属于owner Participating Window,并通过fresh semantic target重新找到对应控件。
- 录制器保存child surface的role、owner关系和semantic locator证据,用于按时间线验证它确实已重新出现。
- 只有能够脱离owner、具备稳定top-level identity,并且可被human独立移动或缩放的窗口,才作为单独Participating Window处理。

### 备选: 每个AXWindow或surface都独立参与

形式上统一,但menu、popover和sheet通常没有稳定的top-level geometry生命周期。为它们生成`@window-resize`既难以执行,也会把短暂UI误建模为回放环境前置条件。

## [2026-07-24 08:48:23] [Session ID: omx-1784512435044-92wxat] 笔记: Human确认transient surface归属

## 已确认规则

- Attached sheet、popover、menu和tooltip并入owner Participating Window。
- 它们不获得独立window identity,不生成geometry precondition或`@window-resize`。
- 对child surface的动作仍需fresh semantic re-find和owner关系验证。
- 可脱离owner、具有稳定top-level identity且可独立move/resize的窗口按普通Participating Window处理。

## 现有locator静态事实

- `@window-find` query支持`app`、`app_contains`、`bundle_id`、`pid`、`title`和`title_contains`。
- `@window-resize.target.query`复用同一查询,并要求唯一命中。多命中返回`WINDOW_AMBIGUOUS`,不会自动选择第一个。
- Journal已规定PID和`pid:<pid>/window:<index>`只能作为runtime hint。
- Observation ref只在对应observation内有效,不能成为Replay Bundle的永久target。

## 下一决策的两个方向

### 推荐: 复用现有exact/unique query,无法唯一解析就fail closed

编译层级:

1. macOS优先使用exact `bundle_id`;其他平台使用可用的exact app identity。
2. 有稳定title时叠加exact `title`。
3. 只有Journal保存了title pattern的构造事实,并且Replay preflight fresh query唯一命中时,才允许`title_contains`。
4. Title不稳定时可以使用exact app identity单独查询,但仍必须在eligible top-level windows中唯一命中。
5. `pid`、runtime `window_id`和observation ref只用于录制期关联或诊断,不写成持久Replay target。

结果契约:

- 0个命中为missing window,Replay preflight失败。
- 多个命中为ambiguous window,Replay preflight失败。
- 不允许回退到frontmost window、同app第一个window或旧坐标。

这个首版不新增document URL、workspace restore或自定义窗口指纹协议。未来若现有query不足,应单开能力演进ticket,而不是在Replay compiler里偷偷猜测。

### 备选: 自动评分并选择最相似窗口

可以容忍title变化,但会引入隐式猜测。只要同一app打开多个相似文档窗口,错误选择就可能让后续geometry restore和输入操作落到错误内容上。

## [2026-07-24 09:02:07] [Session ID: omx-1784512435044-92wxat] 笔记: Human确认exact/unique window locator

## 已确认规则

- Durable Replay target使用现有exact app identity和可选稳定title查询。
- `title_contains`必须有Journal中的构造事实,并在fresh preflight唯一命中。
- 0个或多个匹配都使Replay preflight失败,不做相似度猜测。
- PID、runtime window id和observation ref仅用于录制期关联、fresh observation或诊断。

## 现有window state静态事实

- `@window-resize`默认可以执行`unhide_app`、`unminimize_window`、`activate_app`、`raise_window`和必要的`switch_to_window_space`。
- 每个恢复步骤必须进入`steps[]`,不能隐藏桌面状态变化。
- 恢复步骤失败或后端只能给出`limited`时,resize不能假成功。
- `@window-find` state区分`minimized`、`app_hidden`、`current_space`、`fullscreen_space`和`interactable`。
- 现有规格明确要求: fullscreen或跨Space自动化无法可靠证明时返回`limited`。

## 下一决策的两个方向

### 推荐: Recorded precondition只支持normal windowed,现场恢复可使用现有显式步骤

- Initial snapshot必须表示normal windowed窗口。若录制要求窗口最初处于hidden、minimized或fullscreen state,首版判为unsupported,因为现有协议无法忠实重建这些起始状态。
- 如果录制时是normal windowed,但回放现场窗口恰好hidden、minimized或位于其他普通Space,允许`@window-resize`执行现有恢复步骤。
- 恢复后必须fresh验证窗口未hidden、未minimized、已位于当前Space,并且rect/display guard满足要求。
- 普通跨Space恢复只有返回`ok`并通过fresh verification才算成功。`limited`、无法切换或无法证明都使Replay失败。
- Fullscreen Space首版不做隐式退出、进入或迁移。录制或回放任一侧涉及无法可靠恢复的fullscreen state时fail closed。

这一区分保留现有恢复能力,同时不声称已经具备window state replay协议。

### 备选: 自动规范化所有窗口状态

无论录制起始状态如何都先unhide、unminimize、退出fullscreen再resize。这样可能让脚本继续运行,但已经改变了录制前置条件,也会吞掉human原本想录制的activate/restore操作。

## [2026-07-24 09:10:43] [Session ID: omx-1784512435044-92wxat] 笔记: Human确认normal windowed state边界

## 已确认规则

- 首版只编译normal windowed initial precondition。
- Replay现场可对hidden、minimized和普通cross-Space窗口执行现有显式恢复步骤。
- 所有恢复必须进入trace并通过fresh verification;`limited`不算成功。
- Fullscreen Space和无法可靠证明的state transition均fail closed。

## 下一决策的既有约束与推荐

`specs/rdog-recording-session-lifecycle.md`已经规定:

> "App clamp 或 geometry verify failure 必须显式失败,不能假装窗口已恢复。"

`@window-resize`默认`verify.tolerance_px`为2 logical px:

- 精确命中为`ok`。
- 各轴偏差在显式容差内可为`ok_with_delta`。
- App最小尺寸、Dock、屏幕边界或系统约束造成的实际clamp返回`WINDOW_RESIZE_CLAMPED`。
- 超出容差且没有可识别clamp证据返回`WINDOW_RESIZE_VERIFY_FAILED`。

### 推荐: 只有容差内结果可继续

- `ok`和容差内`ok_with_delta`通过geometry precondition。
- `WINDOW_RESIZE_CLAMPED`、`WINDOW_RESIZE_VERIFY_FAILED`和display guard失败都使Replay在任何内容动作前终止。
- 不提供"有限成功后继续"模式,因为这会使后续坐标和窗口内布局偏离录制环境。

### 备选: Clamp后继续semantic-only动作

对纯AX动作可能偶尔可用,但会让同一脚本的geometry契约随动作类型变化,并可能在后续coordinate fallback时才暴露偏差。首版不建议引入这种分支。

## [2026-07-24 12:33:08] [Session ID: omx-1784512435044-92wxat] 笔记: Human确认clamp严格失败

## 已确认规则

- 只有`ok`和显式容差内`ok_with_delta`通过geometry precondition。
- Clamp、超容差verify failure和display guard failure都在内容动作前终止Replay。
- 不根据后续动作是否semantic-only改变geometry成功标准。

## 现有display identity静态事实

- Journal保存完整`display_topology`,每个display带recording-scoped key,以及可用的`display_id`、name、stable key、rect、scale和rotation observed hints。
- `display_id`当前只有session stability,不能作为跨录制永久identity。
- Display scope resolver支持当前会话的`id`、`name_contains`、`contains_point`和window-based selector。
- Resolver是display scope唯一真相源,负责歧义和错误,成功后返回当前session的resolved `display_id`与`os_rect`。

## 下一决策的两个方向

### 推荐: 不做topology映射,referenced display必须精确匹配

- Replay preflight先用device-stable key匹配display;stable key不可用时,只允许用唯一name/primary候选继续核验,不能仅凭enumeration `d1`/`d2`匹配。
- 每个Participating Window引用的display都必须在当前环境唯一解析。
- 当前display的全局`os_rect`、scale factor和rotation必须与录制snapshot精确一致。未被任何Participating Window或坐标动作引用的display变化不阻断Replay。
- 匹配成功后,runner使用当前session解析出的`display_id`生成或绑定`@window-resize.guard.display`,不把recorded `display_id`持久化。
- 不做按比例缩放、相对坐标迁移、主副屏替换或nearest-display fallback。
- 录制期间如果referenced display topology发生变化,首版不尝试编译硬件变化,Recording Bundle标记为不可Replay。

任何missing、ambiguous、rect/scale/rotation drift都在移动窗口之前使Replay preflight失败。

### 备选: 自动映射到最相似display

可以让脚本在不同显示器环境中运行,但窗口outer rect、全局`os-logical`坐标和mouse fallback都需要重新变换。它会把本ticket扩大为完整的跨拓扑坐标迁移系统。

## [2026-07-25 01:07:01] [Session ID: omx-1784512435044-92wxat] 笔记: Human确认display topology精确匹配

## 已确认规则

- 首版不做display比例缩放、相对迁移、主副屏替换或nearest-display fallback。
- 每个referenced display必须唯一解析,并精确匹配全局`os_rect`、scale factor和rotation。
- Recorded `display_id`只有session stability。Replay使用当前session fresh解析出的display identity。
- Missing、ambiguous、display drift或录制期间referenced topology变化均fail closed。

## 既有execution order约束

`specs/rdog-recording-semantic-promotion-policy.md`已经固定单个动作的顺序:

1. 解析Participating Window。
2. 应用Window Geometry Precondition。
3. Fresh验证window identity、focus、rect和display。
4. 重新解析semantic target,或确认从未存在semantic identity。
5. 选择semantic或guarded coordinate lane。
6. 执行动作并采集fresh verification。

Geometry恢复不能替代semantic target freshness。每个可执行动作仍必须有fresh post-action verifier。

## 最后一项决策的两个方向

### 推荐: 全局只读preflight + geometry mutation + action timeline

1. Compiler只读取frozen Journal,验证initial snapshot、provenance、participation和unsupported状态,生成确定性的`rdog.flow.v1`与Replay requirements。
2. Replay runtime先做全局只读preflight:解析所有referenced displays和Participating Windows,检查能力与权限。任一失败时不产生window side effect。
3. Geometry phase按window首次participation的`journal_seq`稳定排序。每个窗口执行显式`@window-resize`,包含absolute origin、outer size、`os-logical`、fresh resolved display guard和`verify.tolerance_px:2`。
4. 每次resize立即检查现有structured result。全部完成后再fresh读取所有Participating Window,复核identity、normal windowed state、rect和display。多个窗口不要求同时focused。
5. Action phase在每个动作前fresh验证其owner window与focus,再按既有policy bounded re-find semantic target或刷新coordinate gates。不得用geometry verification替代target re-find。
6. 录制期间收敛出的intentional move/resize按原`journal_seq`出现在action timeline,并继续使用同样的display guard与fresh rect verification。
7. 第一个失败立即终止。禁止retry、coordinate降级、继续后续step或自动desktop rollback。
8. Failure report保留阶段、`window_key`或action `journal_seq`、现有底层错误码、expected与observed事实。不为Recorder复制一套window错误码。

不自动rollback的理由是rollback本身会产生新的未录制desktop side effect,而且失败时未必能可靠恢复已经改变的窗口。Trace必须诚实记录已经完成的步骤。

### 备选: 边执行边发现问题并best-effort继续

实现更短,但可能先移动一部分窗口,随后才发现另一个窗口或display无法解析。继续执行会让Replay结果既不一致也难以审计。
