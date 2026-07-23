# Recording redaction 工作日志

## [2026-07-23 17:12:00] [Session ID: omx-1784512435044-92wxat] 任务名称: 定义敏感输入脱敏与 Replay 参数模型

### 任务内容

- 完成 Wayfinder ticket `定义敏感输入脱敏与 Replay 参数模型` 的 HITL 决策与正式规格。
- 更新 `CONTEXT.md`,固定 Ordinary Input、Sensitive Input、Unknown-Safety Input 和 Replay Parameter。
- 新增 `specs/rdog-recording-redaction-parameter-model.md`。
- 同步 `AGENTS.md` 长期索引,并给 Journal/Flow 规格增加 planned-extension pointer。
- 未实现 Recorder 或 `flow_parameters` 生产代码。

### 完成过程

- 固定输入三态分类与 sensitive/unknown fail-closed 落盘规则。
- 固定 paste、快捷键、IME、ordinary unverified text 的参数化边界。
- 固定 canonical descriptor、`param-N` 分配、segment cardinality 和 Journal 单一 ownership。
- 固定 typed `TypeText`、runtime-only bindings、CLI stdin/SDK memory 输入源和禁止通用字符串插值。
- 固定 capability preflight、actual transport confidentiality gate、exact-set validation、audit、zeroize、disconnect failure 和 no-retry。
- 用 `python3` 严格解析全部 9 个完整 JSON 示例,包括 duplicate-key 检查。
- 用 `beautiful-mermaid-rs --ascii` 成功渲染 flowchart 与 sequenceDiagram。
- `git diff --check` 与 staged text-check 通过。
- 规格资产以 commit `28c21dc70338f2180a5a789ec7c10ec9eeb8b063` 推送到 `origin/main`。
- 发布 resolution comment并关闭 ticket,随后更新 Wayfinder map 的 Decisions so far。

### 交付结果

- 规格: `specs/rdog-recording-redaction-parameter-model.md`。
- Resolution: `https://github.com/raiscui/rustdog/issues/3#issuecomment-5055992990`。
- Wayfinder map: `https://github.com/raiscui/rustdog/issues/2`。
- 关闭后的默认下一 frontier: `验证语义提升与坐标 fallback 的可行性`。
- 同时解锁: `定义 Recording Bundle schema 与原子导出`。

### 总结感悟

- 输入安全分类和文本语义可信度必须分开,否则 ordinary 但不可重建的文本会被错误保存或错误归类。
- Capability 与 actual transport gate 必须发生在读取 stdin bindings 之前,这样旧 daemon 或不安全 transport 不会让 secret 提前进入 Replay 进程。
- Parameter value 的“不落盘”不等于远程传输保密。当前只有经过 identity、UID 和 mode 动态验证的 local unixpipe 能承载 sensitive/unknown binding。
- 断线后的投递状态不可证明,最简单且正确的语义就是本次录制回放失败,不自动重试。

### 延期与重大风险检查

- 本轮未创建 `LATER_PLANS__recording_redaction.md`: 后续实施和剩余决策已经由 Wayfinder map 跟踪。
- 本轮未创建 `EPIPHANY_LOG__recording_redaction.md`: 安全边界已经写入正式规格,没有脱离当前 ticket 且尚未落盘的灾难点。
