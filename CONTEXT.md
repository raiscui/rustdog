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
