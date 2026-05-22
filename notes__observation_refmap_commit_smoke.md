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

## [2026-05-22 14:37:24] [Session ID: DECD1A1F-DE7A-4689-8762-F23D9FCF9708] 笔记: ref mouse live smoke 修复证据

## 来源

### 来源1: macOS live lane `mac.observe.lab`

- 命令:
  - `@capabilities#100`
  - `@observe#101:{mode:"ax",include_ax:true,include_refs:true,include_selectors:true,ax_required:false,ax_mode:"interactive",limit:80}`
  - `@mouse-move#102:{target:{ref:"@e18",observation_id:"obs-1779431694917-1"}}`
  - `@observe#103:{mode:"ax",include_ax:true,include_refs:true,include_selectors:true,ax_required:false,ax_mode:"interactive",limit:10}`
- 证据文件: `/tmp/rdog-observe-smoke-final-summary.json`
- 要点:
  - `screenshot`、`accessibility`、`window_control`、`mouse_input`、`zenoh_session_channel` 均为 `available`。
  - `@observe` 返回 `kind:"observe"`、`schema:"rdog.observe.v1"`、`observation_id:"obs-1779431694917-1"`。
  - 选中的 ref 是 `@e18`,名称为 `tab bar`,section 为 `accessibility`。
  - `@mouse-move` 返回 `status:"ok"`,并带有 `target_resolution.source:"observation_ref"`。
  - fresh verify observation 返回 `observation_id:"obs-1779431695476-2"`。

## 综合发现

### 现象

- ref mouse 在修复前会表现为 `Zenoh session bridge subscriber 在收到结果前关闭`。
- 同一 target 的 raw `@mouse-move {x,y,coordinate_space:"os-logical"}` 能成功。
- 同一 observation ref 的 `@ax-get` 能在同一个 `rdog control` 进程里成功。

### 结论

- 已验证结论: 修复后,AX observation ref mouse target 可以在 3 秒 Zenoh session timeout 内完成 current rect 解析和 mouse move。
- 修复点: AX ref current rect 不再为了一个 backend id 重建完整 AX snapshot,而是直接按 target id retain 当前 AX element/window 并读取 rect。

## [2026-05-22 15:29:16] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: deslop 归类

## 来源

### 来源1: ai-slop-cleaner scope scan

- 范围: 98d57a6 与 7cbc2b6 涉及的文件。
- 扫描文件:
  - src/control_ax.rs
  - src/control_ax/macos.rs
  - task_plan__observation_refmap_commit_smoke.md
  - notes__observation_refmap_commit_smoke.md
  - WORKLOG__observation_refmap_commit_smoke.md
  - ERRORFIX__observation_refmap_commit_smoke.md

## 综合发现

### Fallback-like 归类

- 本次新增 direct id / observation ref rect resolver 不是 masking fallback。它把已有 backend id 直接交给平台层读取当前 rect,失败时仍返回结构化 io::Error,没有吞错或静默默认。
- semantic AX locator 继续走 snapshot resolver,这是保留语义查询路径的兼容边界。它不是新增绕路,而是避免把 semantic selector 和已解析 ref 混在同一条快速路径里。
- 非 macOS direct rect resolver 返回 Unsupported,属于显式失败语义,不是 silent skip。
- 文档里的 mouse fallback 是产品契约用语,表示鼠标作为 AX/semantic action 之后的降级 lane。它通过 live smoke 证据锁住 target_resolution.source=observation_ref,不是未验证 fallback。
- macOS 旧有 fallback_visible_windows / snapshot_optional_ax_error / clipboard restore skipped 等命中属于历史代码中的外部 API 兼容或权限/状态保留语义。本轮未新增这些路径,也没有发现需要在本次收尾里改动的 masking fallback。

### 结论

- 当前 deslop 复核不需要代码改动。
- 本轮应只记录归类结果,避免为了清理术语而破坏已经验证的 observation-ref mouse fallback 合同。
