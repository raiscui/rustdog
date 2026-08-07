# 任务计划: 定义 Participating Window 与 geometry precondition 编译

## [2026-07-23 17:24:39] [Session ID: omx-1784512435044-92wxat] [任务启动]: Wayfinder grilling ticket

### 目标

形成可直接供deterministic Replay compiler使用的正式规格,定义哪些窗口属于Participating Window、录制哪些geometry事实、如何编译为现有`@window-resize`与验证步骤,以及环境不一致时何时fail closed。

### 两个方向

1. 不惜代价方案:定义完整window matching、跨display相对映射、Space/全屏/最小化恢复、遮挡处理和拓扑迁移策略。覆盖更广,但容易提前吞并replay preflight与multi-display fog。
2. 首版严格方案:只恢复已明确关联的Participating Window,复用现有`@window-resize`,要求window/display identity与fresh verification完整;无法精确恢复时reject,不重排无关窗口。

当前推荐方向2,符合此前"不要过度设计"与fail-closed边界。最终规则由human逐项确认,不由agent代答。

### 阶段

- [x] 阶段 1: 重新验证frontier并claim ticket
- [x] 阶段 2: 回读domain、ticket、window/display/Journal/semantic policy与真实源码边界
- [x] 阶段 3: 一次一个问题完成Participating Window和geometry compiler grilling
- [x] 阶段 4: 写正式resolution asset,同步索引与交叉引用
- [x] 阶段 5: 验证、commit、push、resolution、close与map更新

### 约束

- 本ticket只做规格决策,不实现Recorder生产代码。
- 只恢复Participating Window,绝不重排整个desktop或无关窗口。
- 必须复用现有control lane,没有证据前不新增window命令。
- Geometry freshness不能替代semantic target freshness。
- Display topology drift是否支持映射,必须由human显式决定。
- 每轮只问一个具体决策问题。

### 停止条件

- Participating Window纳入/排除规则明确。
- Canonical geometry snapshot与编译顺序明确。
- `@window-resize`、activate/focus、display guard和post-restore verification边界明确。
- Missing window、ambiguous match、display drift和unsupported state均有fail-closed结果。
- Human verdict写入正式规格并完成Wayfinder resolution。

### 当前状态

**阶段 1 进行中**: 先重新查询live frontier,确认ticket仍open、unassigned、fully unblocked后claim。

## [2026-07-23 17:25:37] [Session ID: omx-1784512435044-92wxat] [阶段完成]: Ticket已claim

- [x] Live GraphQL确认ticket为open、unassigned,唯一blocker`验证语义提升与坐标 fallback 的可行性`已closed。
- [x] GitHub assignee已设置为`raiscui`。
- [x] Ticket类型为HITL grilling;agent不得代替human回答geometry policy。
- [x] 阶段 1 完成。
- [x] 下一步回读grilling/domain规则、正式spec和现有window实现,建立第一轮决策边界。

**阶段 2 进行中**: 先确认既有能力和缺口,再提出第一个human问题。

## [2026-07-23 17:27:53] [Session ID: omx-1784512435044-92wxat] [阶段进展]: Participation trigger边界已收敛

- [x] 回读grilling、domain-modeling和rdog-control skill。
- [x] 回读Journal、lifecycle、Semantic Promotion、window、display和mouse相关规格。
- [x] CodeGraph核对`WindowResizeRequest`、`execute_window_resize`与macOS resize backend。
- [x] 确认现有`@window-resize`已覆盖outer/os-logical size、optional origin、恢复、clamp、display guard和fresh rect verification。
- [x] 确认首个未决产品问题是Participating Window纳入触发条件,而不是新增resize命令。
- [x] 向human一次只确认"事件驱动纳入"规则。

**阶段 3 第一轮**: 等待human决定仅target-bound operation/move/resize纳入,还是session启动时预纳入可见窗口。

## [2026-07-23 21:34:31] [Session ID: omx-1784512435044-92wxat] [Human确认]: Participating Window采用事件驱动纳入

- [x] Human确认仅在target-bound operation、显式activate/focus或主动move/resize首次发生时纳入窗口。
- [x] 仅可见、frontmost、被snapshot观察、遮挡其他窗口或属于同一app,均不构成participation。
- [x] 不在session启动时预纳入或恢复全部可见窗口。
- [x] 下一步确认首次`window_snapshot`的冻结时点: 首个参与动作前或动作后。

**阶段 3 第二轮**: 等待human决定canonical initial geometry是否必须来自fresh pre-action observation。

## [2026-07-23 21:54:58] [Session ID: omx-1784512435044-92wxat] [Human确认]: Initial geometry冻结在首个参与动作前

- [x] Human确认canonical initial `window_snapshot`必须来自首个参与动作前的fresh observation。
- [x] 尚未participate的窗口可以被短期观察和缓存,但观察本身不触发participation。
- [x] 禁止用动作后的rect或state冒充geometry precondition。
- [x] 下一步确认human move/resize是否始终编译为独立intentional geometry action。

**阶段 3 第三轮**: 等待human决定move/resize的precondition与action分界和连续事件收敛规则。

## [2026-07-23 23:43:04] [Session ID: omx-1784512435044-92wxat] [Human确认]: Move/resize作为独立intentional action

- [x] Human确认每个Participating Window只有一份pre-action initial geometry precondition。
- [x] Human主动move/resize按时间线编译为独立`@window-resize` action。
- [x] 连续move/resize通知收敛为一个动作,以稳定后的完整snapshot作为结果和verification目标。
- [x] 下一步确认sheet、popover、menu等transient child surface是否并入owner window。

**阶段 3 第四轮**: 等待human决定transient surface的participation identity与geometry恢复边界。

## [2026-07-24 08:48:23] [Session ID: omx-1784512435044-92wxat] [Human确认]: Transient child surface并入owner window

- [x] Human确认attached sheet、popover、menu和tooltip不获得独立Participating Window identity。
- [x] Transient surface不生成geometry precondition或`@window-resize`,操作归属于owner window。
- [x] 只有具备稳定top-level identity且可独立移动或缩放的窗口才单独参与。
- [x] 下一步确认durable window locator层级与missing/ambiguous match边界。

**阶段 3 第五轮**: 等待human决定首版是否只使用现有exact/unique window query并fail closed。

## [2026-07-24 09:02:07] [Session ID: omx-1784512435044-92wxat] [Human确认]: Durable locator复用现有exact/unique query

- [x] Human确认Replay target优先exact app identity,稳定时叠加exact title。
- [x] `title_contains`仅在有构造事实且fresh preflight唯一命中时允许。
- [x] Missing或ambiguous window均fail closed,不回退frontmost、同app首个窗口或旧坐标。
- [x] PID、runtime window id与observation ref不成为持久Replay target。
- [x] 下一步确认hidden、minimized、fullscreen和cross-Space的首版恢复边界。

**阶段 3 第六轮**: 等待human决定只接受normal windowed recorded precondition,并将无法证明的state recovery视为Replay失败。

## [2026-07-24 09:10:43] [Session ID: omx-1784512435044-92wxat] [Human确认]: Recorded precondition只支持normal windowed

- [x] Human确认hidden、minimized或fullscreen recorded initial state在首版为unsupported。
- [x] Replay现场的hidden、minimized或普通cross-Space状态可由现有`@window-resize`恢复,但必须返回`ok`并fresh验证。
- [x] Fullscreen Space不做隐式进入、退出或迁移;`limited`或不可证明均fail closed。
- [x] 下一步确认任何超出显式容差的clamp均使Replay失败。

**阶段 3 第七轮**: 等待human确认沿用现有lifecycle的clamp/verify failure契约。

## [2026-07-24 12:33:08] [Session ID: omx-1784512435044-92wxat] [Human确认]: 超容差clamp与verify failure终止Replay

- [x] Human确认只有`ok`和显式容差内`ok_with_delta`可以继续。
- [x] `WINDOW_RESIZE_CLAMPED`、`WINDOW_RESIZE_VERIFY_FAILED`和display guard失败均在内容动作前终止Replay。
- [x] 不提供geometry有限成功后继续semantic-only动作的分支。
- [x] 下一步确认display topology drift首版不映射,只接受referenced display geometry精确匹配。

**阶段 3 第八轮**: 等待human决定display identity、topology equality与fail-closed边界。

## [2026-07-25 01:07:01] [Session ID: omx-1784512435044-92wxat] [Human确认]: Referenced display必须精确匹配

- [x] Human以连续"继续"确认首版不做display topology自动映射。
- [x] Referenced display必须唯一解析,且全局`os_rect`、scale factor与rotation精确匹配。
- [x] Runtime使用当前session解析出的display identity,不持久化recorded `d1`/`d2`。
- [x] Missing、ambiguous或topology drift均在任何window side effect前fail closed。
- [x] 下一步确认最终compiler sequence、双阶段verification和first-error abort契约。

**阶段 3 第九轮**: 等待human确认最终编译与执行顺序。确认后进入正式resolution asset编写。

## [2026-07-25 13:07:25] [Session ID: omx-1784512435044-92wxat] [Human确认]: 最终compiler sequence与失败契约

- [x] Human确认全局只读preflight、geometry mutation、action timeline三阶段顺序。
- [x] Human确认每次resize即时验证,geometry phase完成后再做全窗口fresh复核。
- [x] Human确认动作前仍需fresh owner/focus与target re-find,动作后仍需fresh verifier。
- [x] Human确认first-error abort、no retry、no downgrade、no automatic desktop rollback。
- [x] 阶段 3全部HITL决策完成。
- [x] 编写正式resolution asset与Mermaid流程图/时序图。
- [x] 同步AGENTS知识索引和相关Recording规格交叉引用。
- [x] 验证Markdown、Mermaid、diff与scoped commit。
- [x] Push main,评论并关闭ticket,更新Wayfinder map与fog。

**阶段 4 进行中**: 只编辑本ticket正式文档,不纳入支线上下文文件或工作区其他任务改动。

## [2026-07-25 13:07:25] [Session ID: omx-1784512435044-92wxat] [阶段进展]: Resolution asset已落盘并通过Mermaid解析

- [x] 新增`specs/rdog-recording-window-geometry-policy.md`,覆盖全部Human verdict。
- [x] 文档包含decision flow与execution sequence两张Mermaid图。
- [x] `beautiful-mermaid-rs --ascii`成功解析两个Mermaid block。
- [x] 新规格共345行,未超过项目文档规模边界。
- [x] 同步AGENTS长期知识索引和lifecycle、Journal、Semantic Promotion交叉引用。
- [x] 审查scoped diff、Markdown引用和规格内部一致性。
- [x] 完成scoped commit、push和GitHub resolution。

**阶段 4 审查中**: 重点验证没有新增Recorder专用window command,且所有fail-closed门禁与既有规格一致。

## [2026-07-25 13:07:25] [Session ID: omx-1784512435044-92wxat] [阶段完成]: 正式规格静态审查通过

- [x] Markdown围栏、必需章节和本地文档引用检查通过。
- [x] Scoped `git diff --check`通过。
- [x] 两张Mermaid图均通过`beautiful-mermaid-rs`实际解析。
- [x] 远端`origin/main`与本地HEAD ahead/behind均为0。
- [x] 仓库无submodule指针需要同步。
- [x] 阶段 4完成。
- [x] 只暂存正式文档文件并审查cached diff。
- [x] Commit并push main。
- [x] 发布resolution comment、关闭ticket并更新Wayfinder map。

**阶段 5 进行中**: 执行scoped delivery,保持工作区其他任务改动不变。

## [2026-07-25 13:16:04] [Session ID: omx-1784512435044-92wxat] [任务完成]: Geometry policy已发布并关闭ticket

- [x] Scoped commit `6973dfa3c9bc5edc9528c51448e0f7d6d9a60599`已创建。
- [x] Commit只包含AGENTS索引、3处Recording交叉引用和新geometry policy,未混入其他工作区改动。
- [x] Commit已push到`origin/main`,GitHub API远端SHA与本地HEAD完全一致。
- [x] Resolution comment已发布到`定义 Participating Window 与 geometry precondition 编译`。
- [x] Ticket已`CLOSED/COMPLETED`。
- [x] Wayfinder map已追加context pointer并清除旧display topology fog。
- [x] 阶段 5完成,本ticket全部停止条件满足。

**最终状态**: 完成。生产Recorder实现仍属于Wayfinder destination之后的实施阶段,本ticket没有实现或声称实现生产代码。
