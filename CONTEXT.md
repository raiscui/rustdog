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

**Participating Window**:
Recording Session 中成为操作目标,或被用户主动移动、缩放的窗口。只有这类窗口属于回放环境恢复范围。
_Avoid_: All desktop windows, unrelated window

**Window Geometry Precondition**:
Participating Window 在回放动作开始前必须满足的位置、大小、display 和窗口状态约束。
_Avoid_: Desktop layout snapshot, global window reset

**Recording Bundle**:
Recording Session 完成后导出的自描述产物集合,包含 Recording Journal、Replay Script、manifest 和必要 evidence。
_Avoid_: Script file, video recording
