# rustdog control automation

本上下文定义 rustdog 操作录制与回放领域中的 canonical terms。实现规格、协议字段和任务状态不属于本文件。

## Language

**Recorder**:
运行在被控主机上,观察并记录 human operations 的组件。
_Avoid_: Controller-side recorder, macro hook

**Recording Session**:
一次具有明确开始和结束边界的操作录制过程。
_Avoid_: Capture job, macro session

**Recording Journal**:
Recording Session 产生的 canonical、append-only 操作记录,也是生成其他录制产物的唯一来源。
_Avoid_: Raw script, temporary event dump

**Replay Script**:
从 Recording Journal 派生、可由 rdog control 执行的有限步骤序列。
_Avoid_: Recording source, raw capture

**Semantic Promotion**:
Replay compiler根据Recording Journal中的target、ownership、capability和freshness事实,把physical operation编译为可重新定位的语义动作。
_Avoid_: Best-effort semantic guess, confidence-based promotion

**Guarded Coordinate Fallback**:
没有录制语义身份时,经过Participating Window、geometry、display、point/path和verification门禁后生成的`os-logical`坐标动作。
_Avoid_: Silent coordinate downgrade, raw coordinate replay

**Participating Window**:
Recording Session 中成为操作目标,或被用户主动移动、缩放的窗口。只有这类窗口属于回放环境恢复范围。
_Avoid_: All desktop windows, unrelated window

**Window Geometry Precondition**:
Participating Window 在回放动作开始前必须满足的位置、大小、display 和窗口状态约束。
_Avoid_: Desktop layout snapshot, global window reset

**Recording Bundle**:
Recording Session 完成后导出的自描述产物集合,包含 Recording Journal、Replay Script、manifest 和必要 evidence。
_Avoid_: Script file, video recording

**Ordinary Input**:
Recorder 拥有完整、明确的非安全输入证据,因而允许保存其文本语义的输入。
_Avoid_: Probably safe input, visible text

**Sensitive Input**:
由 Secure Input、secure field 或显式 secret 声明确认需要保密,其真实值不得进入任何持久化录制产物的输入。
_Avoid_: Password keystrokes, captured secret

**Unknown-Safety Input**:
Recorder 缺少足够证据判断是否安全,因此采用与 Sensitive Input 相同持久化边界的输入。
_Avoid_: Ordinary input fallback, unclassified plaintext

**Replay Parameter**:
Replay 开始前由调用方显式提供,用于补全录制期未保存或无法可靠重建的输入值。
_Avoid_: Embedded secret, template variable, stored credential

**Post-action Evidence**:
状态变更成功后,针对同一目标取得的新观察证据,用于判断显式 postcondition 是否满足。
它不是动作返回成功本身,也不包含失败后的重新定位查询。
_Avoid_: Any query after an action, recovery read

**Recovery Observation**:
`STALE_REF`、`OBSERVATION_EXPIRED` 或目标丢失后,为恢复目标身份而执行的新观察。
它用于下一次 mutation 的重新定位,不能被当作上一次动作成功的 evidence。
_Avoid_: Post-action evidence, verification proof

**Observation Capture**:
为 Recording Session 或 Replay 捕获 AX tree 的轻量级适配器,封装了"为 observation 生成 snapshot + selectors"的语义。
它调用 ax_query 底层能力,但返回 observation 需要的完整格式。
_Avoid_: Direct ax_query call from observation layer, snapshot-only capture

**AX Snapshot Cache**:
缓存已捕获的 AX tree snapshot 和对应的资源 epoch 快照,用于避免重复 observation 注册。
它由 ObservationStore 持有,是加速层而非真相源;资源 epoch 的单一真相源仍然是 ResourceCoordinator。
支持多种 TTL policy (ImplicitObserve 5s, Progressive 300s) 满足不同场景需求。
_Avoid_: Epoch truth source, global singleton cache

**Successor Observation**:
资源 lane 完成 mutation 并提交稳定 write epoch 后,针对原目标窗口生成的新 observation。
它携带下一次 mutation 可消费的 successor target,其 ref 仍然是 observation-local。
_Avoid_: Stable global ref, old observation with a new epoch

**Canonical Mutation Path**:
Agent 执行带有 ref、observation_id 和 epoch 的状态变更时,统一使用
`@computer-act` 的结构化 mutation 路径;底层 `@type-text` 等 primitive 由该路径调用。
_Avoid_: Direct primitive as the agent workflow contract

**Explicit Postcondition**:
由调用方提供、用于判断 mutation 结果的结构化 AX 条件。
服务器不从输入 content 自动推断该条件;缺少条件时只能报告 dispatch 成功和 outcome unknown。
_Avoid_: Implicit equality with requested input

**Ref-backed Type Mutation**:
带有 observation-local ref、observation_id 和 epoch 的 `@computer-act action:"type"`。
它经过 PID resource lane,可生成 successor observation 并执行显式 postcondition。
_Avoid_: Untargeted paste, direct low-level type-text workflow

**Legacy Paste Path**:
`action:"type"` 未提供 target 时的低级 paste 路径。
它不拥有目标资源的 successor/postcondition 契约,只能作为无目标输入能力使用。
_Avoid_: Verified text mutation
