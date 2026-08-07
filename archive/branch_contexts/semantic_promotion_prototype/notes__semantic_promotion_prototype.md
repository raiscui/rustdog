# 语义提升与坐标 fallback prototype 笔记

## [2026-07-23 17:48:00] [Session ID: omx-1784512435044-92wxat] 笔记: 既有协议边界与 prototype 假设

## 来源

### `specs/rdog-observation-scoped-refmap-plan.md`

直接约束:

> "observation 内的 ref 只对这次 observation 负责."
>
> "durable selector 只负责跨 observation 的恢复."
>
> "mouse 坐标是物理 fallback,不是默认主路径."

来源: lines 22-32。

Selector re-find 只允许在单候选且证据充分时自动继续。多候选、低可信、rect 缺失或 stale 都必须 no-action。Mouse selector target 默认也是 handoff,只有显式 `auto_refind:true`、typed decision 为 rebound且 fresh rect 可解析时才允许执行。

来源: lines 259-275、312-348。

### `specs/rdog-display-aware-control-chain-plan.md`

直接约束:

> "focus 是动作前置条件,必须通过 fresh state 验证。"
>
> "screenshot 裁剪只改变 image space,鼠标继续使用全局 `os-logical`。"
>
> "`status:\"ok\"` 只表示动作提交成功;完整成功还需要 verification。"

来源: lines 35-43。

Mouse action的 performed report不是业务成功。动作后必须重新采集目标窗口的AX或visual evidence。

来源: lines 194-202。

### `specs/rdog-computer-use-density-plan.md`

动态Web页面的已实现语义路径是:

- fresh AX snapshot。
- 解析明确 browser window与当前 `AXWebArea`。
- 唯一 actionable match。
- `AXPress`。
- stale-like失败只在同一daemon request内做一次bounded re-find。
- fresh subtree/full snapshot verification。

Ambiguous match或缺少 `AXPress` 时不动作,也不自动mouse fallback。

来源: lines 176-205。

### `specs/rdog-recording-journal-model.md`

直接约束:

> "`physical` 保存 capture backend 已交付的输入事实。Journal writer 不做语义合并或 Replay-oriented coalescing。"
>
> "candidate 可以保存 durable selector,不能保存 observation ref。"
>
> "candidate 不表示 Replay compiler 已选择该动作。"

来源: lines 171-229。

因此 prototype input必须同时保留physical evidence与零到多个semantic candidates,compiler decision只能作为派生输出。

### `specs/rdog-recording-redaction-parameter-model.md`

- Ordinary committed text确认后才允许literal `TypeText`。
- Sensitive/unknown或ordinary semantic commit不可验证时使用Replay Parameter。
- Shortcut必须明确是非文本动作且位于redaction外。
- IME/dead-key intermediate过程不生成Replay step。

来源: lines 67-102。

### `specs/rdog-non-mouse-semantic-control-plan.md`

当前五类动作的semantic lane:

- click/button/menu: `@web-act`或`@ax-action`。
- text: planned typed `TypeText`,runtime复用`@type-text`/AXValue。
- shortcut/function/navigation: window-targeted或pid-targeted `@key`。
- scroll: `@ax-scroll`。
- drag:没有通用semantic drag,复杂drag保留为mouse fallback。

既有agent流程明确写成semantic优先,只有无法语义化且允许干扰时才进入 `@click/@drag/@wheel`。

来源: lines 244-262。

### 当前源码结构证据

CodeGraph确认:

- `execute_ax_action`、`execute_type_text`、`execute_web_act`都由 `src/control_actions.rs` 路由到现有control backend。
- `ObservationRefEntry`只保存 `ref_id`、`backend_id`、`kind`,并归属于内存中的 `StoredObservation.refs`。
- `MouseDisplayGuard`只携带 `DisplaySelector`,通过 `as_scope()`复用display scope resolver。

这支持"prototype只做compiler/gate logic,不重写action backend"。

## 当前主假设

自动semantic promotion必须同时满足:

1. 唯一target candidate。
2. Durable selector或可构造selector的事实完整。
3. Window ownership一致。
4. Action capability匹配,例如 `AXPress`/settable AXValue/AX scroll。
5. Dynamic页面在执行时fresh re-find成功。
6. 存在fresh post-action verifier。

不满足semantic条件时,只有以下条件全部成立才允许guarded coordinate fallback:

1. 动作本身适合物理重放,例如free-space click、wheel或canvas drag。
2. Participating Window geometry precondition可恢复并验证。
3. Fresh window rect、display topology与action point/path仍一致。
4. Window ownership与display guard通过。
5. 存在fresh AX或visual post-action verifier。

否则compiler应拒绝生成可执行step,而不是静默猜测。

## 最强备选解释

只要先用 `@window-resize` 恢复录制时outer rect,就尽量直接重放physical `os-logical`坐标。这可能对no-AX/canvas和高度自绘UI更忠实,也避免selector churn。

该方案的风险是dynamic layout、content scroll、display topology变化和遮挡会让绝对坐标指向不同目标。Window geometry相同不能证明content geometry相同。

## 可证伪条件

- 如果五类代表场景中,semantic candidate经常多义或stale,而geometry恢复后的guarded坐标持续唯一且fresh,主假设需要放宽semantic优先。
- 如果no-AX场景无法获得fresh window/display/visual verifier,坐标备选也必须被拒绝,不能因为"只有坐标可用"就假装安全。
- 如果dynamic Web页面的一次bounded re-find仍多义,必须输出 `needs_disambiguation`,不能自动落到旧坐标。

## Prototype 输入与量化口径

每个scenario保存:

- action kind。
- physical event数量。
- semantic candidate数量与action capability。
- observation age/TTL和selector re-find结果。
- window ownership、focus、geometry、display guard状态。
- required/present verifier数量。
- compiler decision: semantic、parameterized-semantic、guarded-coordinate或reject。
- 生成的command shape与gate trace。

不生成无证据的浮点confidence。量化使用candidate count、age/TTL、required/present guard和verifier计数,以及全suite各decision数量。

## Tooling 说明

当前工具清单没有暴露Context7 MCP。Prototype仅使用Python标准库,不依赖外部library/API;实现仍会保持单文件、小规模和throwaway边界。

## [2026-07-23 16:39:29] [Session ID: omx-1784512435044-92wxat] 笔记: stale semantic target 最小反例

### 现象

- 原12个scenario全部匹配expected decision,但它们没有覆盖"旧语义target在执行期not found,同时坐标guard仍fresh"。
- 将`web-click-dynamic-refind`复制为负向fixture,仅把`execution_refind`改为`not_found`,并把expected decision设为`reject`。
- 实际命令以exit code 1失败,关键输出是: `expected reject, got guarded-coordinate`。

### 已验证结论

- 失败轮次确实经过`decide_click -> coordinate_gate`。
- 它使用同一个scenario里的旧physical point和fresh window/display guard生成`@click`。
- 这会覆盖"stale semantic target不得自动回退旧坐标"的不变量;仅有geometry/display freshness不能证明旧point仍指向原语义target。

### 修正边界

- Coordinate lane必须额外证明不存在capture-time semantic identity: candidate count为0、durable selector为空、captured ref为空。
- Coordinate evidence本身也必须满足`observation_age_ms <= observation_ttl_ms`。
- Text lane补齐candidate ownership与`AXValue`/`TypeText` capability检查。
- Shortcut lane补齐owned durable window selector与`KeyDelivery` capability检查。
- 不新增生产协议,只让throwaway prototype与它声明的policy一致。

## [2026-07-23 16:47:52] [Session ID: omx-1784512435044-92wxat] 笔记: 最终动态证据与远端资产

### 动态验证

- Python语法和fixture JSON语法通过。
- 13个常驻scenario全部匹配expected decision。
- Decision分布: semantic 5、parameterized-semantic 1、guarded-coordinate 3、reject 4。
- Edge覆盖: ambiguity 1、dynamic page 3、no-AX 4、stale target 4。
- 9个可执行decision全部有fresh verifier。
- persisted observation ref为0,prevented unsafe fallback为3。
- 4个临时负向fixture全部得到reject: stale semantic click、unowned text、缺window selector shortcut、缺parameter id。

### 远端证据

- Branch: `prototype/recording-semantic-promotion`。
- Commit: `c0d2e0158df2d8bac4d37ce34dcdc7a66276b994`。
- `git ls-remote`返回同一SHA。
- Ticket comment: https://github.com/raiscui/rustdog/issues/6#issuecomment-5056296153 。
- Ticket复查状态为open,符合HITL prototype边界。

### 待human verdict

Ambiguous或stale semantic target是否永不自动回退旧坐标;只有semantic candidate为零,且window geometry、display guard、point/path和post-action verifier全部fresh时,才允许coordinate fallback。

## [2026-07-23 16:56:30] [Session ID: omx-1784512435044-92wxat] 笔记: Human verdict与正式文档归属

### Human verdict

- Human在唯一policy问题后回复"继续"。
- 按当前待办语境,该回复确认prototype推荐的fail-closed promotion policy,允许进入resolution阶段。

### 文档边界

- `specs/rdog-recording-journal-model.md`明确把Replay compiler选择、合并、等待和retry排除在Journal schema之外。
- `specs/rdog-observation-scoped-refmap-plan.md`固定运行时observation ref、durable selector和semantic re-find,不负责Recording Journal到Replay Script的编译决策。
- `specs/rdog-mouse-control-coordinate-plan.md`固定control命令坐标语义,不负责Recorder何时能选择坐标lane。
- 因此正式真相源采用新文件`specs/rdog-recording-semantic-promotion-policy.md`,只定义semantic promotion与guarded coordinate fallback选择。

### 必要同步

- `CONTEXT.md`: 增加Semantic Promotion和Guarded Coordinate Fallback canonical terms。
- `AGENTS.md`: 为新长期规格建立索引。
- `specs/rdog-recording-journal-model.md`: 增加compiler handoff指针,并把non-goal收窄为本policy之外的merge/wait/retry。
- 不更新README,因为本轮没有新增可运行的生产命令或用户行为。

## [2026-07-23 17:07:55] [Session ID: omx-1784512435044-92wxat] 笔记: Resolution与frontier最终证据

- 正式规格: `specs/rdog-recording-semantic-promotion-policy.md`。
- Main commit: `3de8cd631c9a307910829f42f914f09923596f4d`。
- Resolution comment: https://github.com/raiscui/rustdog/issues/6#issuecomment-5056501081 。
- Ticket state: `CLOSED`,state reason: `COMPLETED`。
- Map回读确认decision title与policy pointer均存在。
- Native frontier query确认两张fully unblocked、unassigned ticket:Participating Window geometry编译与Recording Bundle schema。
- Deterministic compiler仍被Participating Window geometry编译阻塞;preflight与验收矩阵继续保持blocked。
- 现有fog尚未成熟:multi-display drift等待geometry决策,Composite等待compiler prototype,evidence retention等待Bundle样本。
