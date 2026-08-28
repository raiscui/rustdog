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
为 observation 注册而做的 AX 富化语义, as-built 形态是 `AxSnapshot::with_observation(source_command)`:
为 snapshot 分配 ref_id、生成 refs 与 durable selector drafts, 并经 control_observation 落注册。
底层纯捕获来自无状态模块 ax_query; 富化与注册属于 verb 层职责。
_Avoid_: Direct ax_query capture without observation registration, raw snapshot as evidence identity

**AX Snapshot Cache**:
缓存已捕获的 AX tree snapshot 和捕获时刻的资源 epoch 快照,用于避免重复 observation 注册。
它是加速层而非真相源: 读取时向 observation store / resource lane 校验当前 epoch,
epoch 不一致即 cache miss (stale_observation_cache)。
资源 epoch 的单一真相源是 ObservationStore / control_resource_lane, 缓存内 epoch 只是捕获时快照。
as-built 无 TTL policy, 失效判定纯靠 epoch 变化 (TTL 型复用由 computer-act 的 implicit observe 缓存另行承担)。
_Avoid_: Epoch truth source, time-based expiry replacing epoch validation

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
