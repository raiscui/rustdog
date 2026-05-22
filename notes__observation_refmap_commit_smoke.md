## [2026-05-22 12:05:43] [Session ID: 0bb1198d-77aa-4dd1-bf4d-65b82e83c8ea] 笔记: observation refmap 文档跟进

## 来源

### 来源1: 当前代码

- 路径:
  - `src/control_observation/observe/request.rs`
  - `src/control_observation/observe/response.rs`
  - `src/control_observation/observe/refs.rs`
  - `src/control_observation/durable.rs`
  - `src/control_mouse/target.rs`
- 要点:
  - `@observe` 当前 schema 是 `rdog.observe.v1`,默认 `mode:"hybrid"`。
  - `@observe.target` 支持 `app` / `process` / `process_name`、`bundle_id`、`window_title` / `title`、`window_title_contains` / `title_contains`。
  - `refs.sample[]` 当前是 compact 摘要,字段为 `section`、`observation_id`、`ref`、`kind`、可选 `name`。
  - durable observation state 使用 `meta.json`、`index.json`、`observations.jsonl`、`selectors.jsonl`、`ref_cache.jsonl`。
  - mouse ref target 在动作前重新解析当前 AX/window rect;坐标 target 标记 `coordinate_fallback`; selector target 默认 no-action。

### 来源2: 当前文档

- 路径:
  - `.codex/skills/rdog-control/SKILL.md`
  - `.codex/skills/rdog-control/references/protocol.md`
  - `.codex/skills/rdog-control/references/control-workflow.md`
  - `README.md`
  - `specs/control-line-protocol.md`
  - `specs/code-agent-rdog-control-usage.md`
  - `specs/rdog-observation-scoped-refmap-plan.md`
- 要点:
  - 多数文档已有 observation/ref/selector 主题,但 README 和 `control-line-protocol.md` 对 `@observe`、selector command、mouse ref target 的入口呈现还不够一等。
  - observation roadmap 里 P1 durable state 仍有未来式表达,需要补当前 JSON/JSONL 实现状态。
  - skill 需要增加一条明确 live evidence chain,方便后续真实 GUI smoke 对齐。

## 综合发现

- 文档应统一表达为: `@observe` 只读观察,短期 `@eN` 只在当前 observation 内有效,selector 是跨 observation 的 stable 恢复线索。
- `@selector-refind` 的 `fresh_target` 不是动作成功,必须先执行 `verify_hint`,再显式发送 side-effect 命令。
- mouse 是 fallback lane。ref fallback 成功要看 `target_resolution.source:"observation_ref"`; raw coordinate 成功要看 `target_resolution.source:"coordinate_fallback"`。
- live smoke 的证据链应固定为 `@capabilities -> @observe -> ref target mouse fallback -> fresh verify`。
