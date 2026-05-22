## [2026-05-20 07:43:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: P2 permanent selector brownfield 事实

## 来源

### 来源1: `specs/rdog-observation-scoped-refmap-plan.md`

- roadmap 明确把 P2 定义为 permanent selector:
  - 允许跨 observation 重新找到目标。
  - 用结构化 selector 表达语义锚点。
  - 不使用 opaque AI string。
- roadmap 同时明确 P3 才是 semantic re-find:
  - stale 后可尝试恢复。
  - 命中多个候选时返回候选集。
  - 低置信度不能静默替用户选择。
- roadmap 明确 P4 才是 `@observe`,P5 才是 mouse ref 化。

### 来源2: `.omx/plans/ralplan-rdog-observation-refmap-p1.md`

- P1 选择 JSONL records + compact JSON index,目标是 durable observation state。
- P1 的 `selectors.jsonl` 是 `rdog.selector.draft.v1`,属于 selector draft / hint。
- P1 明确延期:
  - P2: permanent selector 稳定 schema 和跨 observation target 输入。
  - P3: semantic re-find、candidate set、confidence ranking。
  - P4: `@observe` bundle。
  - P5: mouse ref 化。

### 来源3: `src/control_observation/selector.rs`

- 当前 selector 模型包含:
  - `SelectorKind::{AxWindow,AxElement,Window}`
  - `SelectorEnvelope`
  - `AppSelector`
  - `WindowSelector`
  - `ElementSelector`
  - `SelectorAnchor`
  - `SelectorRedaction`
- 当前 schema 常量是 `rdog.selector.draft.v1`。
- 当前 `selector_id_for(observation_id, ref_id)` 由 observation/ref 派生,因此不满足 P2 permanent selector 的跨 observation 稳定性。
- 当前 `DurableSelectorDraft` 生成阶段没有 `observation_id`,这符合 P1 单一真相源,但 P2 需要在写 durable record 时额外生成 stable selector identity。

### 来源4: `src/control_observation/durable.rs`

- 当前 durable index 保存:
  - observations
  - selectors
  - selector_id
  - observation_id
  - ref
  - kind
  - reobserve_commands
- 当前 lookup 只有 `selector_hint_for_ref(observation_id, ref_id)`。
- 当前没有按 stable selector id 查询、按 selector fingerprint 查询、或返回 selector record 详情的 API。
- 当前 `reobserve_commands_for_selector` 只是 recovery hint,不是 resolver,也不返回 candidate set。

### 来源5: `src/control_ax.rs` / `src/control_window.rs`

- AX window、AX element、window candidate 已经会生成 selector drafts。
- AX element selector 中已有 role、subrole、name、description、actions、ax_path。
- window selector 中已有 app name、bundle_id、pid_hint、title、role、rect。
- 当前 anchors 仍然为空。P2 应规划 anchor 生成规则,但不要把它和 P3 ranking 混在一起。

## 综合发现

### P2 的核心缺口

- 需要把 `selector draft` 提升为 `permanent selector schema`。
- 需要把 `selector_id` 从 observation/ref 派生改成结构化 selector fingerprint 派生,并保留旧 draft id 的兼容或迁移字段。
- 需要新增明确的 selector inspect / resolve dry-run surface,让 agent 能拿 selector 做可解释定位,但不默认执行 side-effect action。
- 需要 fixtures 锁定 AX/window selector JSON,否则后续 P3/P4 很容易把 schema 改散。

### P2 不应该做的事

- 不实现自动 semantic re-find。
- 不做 confidence ranking 自动选择。
- 不新增统一 `@observe` bundle。
- 不把 mouse command 改成 ref/selector target。
- 不让旧 `@eN` 因 permanent selector 存在而跨 daemon 重启复活。

### 推荐 P2 方向

- 采用 conservative Option A+:
  - 定义 `rdog.selector.v1` permanent selector schema。
  - 新增 stable selector id / fingerprint / versioning。
  - 新增 durable selector lookup API。
  - 新增显式 `@selector-get` 和 `@selector-resolve` dry-run command。
  - `@selector-resolve` 返回 candidate set 和原因,但默认不执行 action。
  - action target 支持 selector 的 side-effect 执行放到 P3 或 P2b,除非 strict single-candidate dry-run 已经被验证。

## [2026-05-20 19:02:20] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: P2 deslop 检查

## 来源

### 来源1: P2 触碰文件的 fallback-like 搜索

- 搜索范围:
  - `src/control_observation.rs`
  - `src/control_observation/`
  - `src/control_protocol.rs`
  - `src/control_protocol/`
  - `src/control_core.rs`
  - `src/control_actions.rs`
  - `src/control_ax.rs`
  - `src/shell/tests.rs`
  - `.codex/skills/rdog-control/`
  - `specs/code-agent-rdog-control-usage.md`
- 搜索词包括: `TODO`, `FIXME`, `hack`, `temporary`, `workaround`, `fallback`, `silent`, `swallow`, `bypass`, `临时`, `绕过`, `吞掉`。

## 综合发现

### fallback-like 分类

- `src/control_protocol.rs` 中的 legacy / compatibility 注释是已有协议兼容层说明,不是 P2 新增的 masking fallback。
- `src/control_actions.rs` 中 selector/capabilities/screenshot 不进入默认 executor 的错误是明确边界保护,不是吞错。
- skill / docs 中的 `fallback recipe` 是 GUI agent 工作流的最后手段,并且明确要求先看 capability status 和最新 manifest,不属于静默绕过。
- `@selector-resolve dry_run:false` 返回 `SELECTOR_ACTION_DEFERRED`,这是 P2 明确的显式失败,不是隐藏动作 fallback。

### deslop 决策

- 本轮不做额外结构性重构。
- `resolve_ax_selector_candidates()` 当前通过 AX find JSON response 复用现有 selector/ref 生成路径,有一点间接,但它保持单一真相源并避免复制 AX matching 逻辑。后续若 P3 要做 confidence ranking,再抽 typed resolver 会更合适。

## [2026-05-20 19:20:30] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: P2 reviewer rejection 修复记录

## 来源

### 来源1: Ralph reviewer `CHANGES_REQUESTED`

- reviewer 指出 `@selector-resolve` 对 0/多候选没有结构化 error gate。
- reviewer 指出 `matched_fields` 只是 selector 字段回显,不是候选实际解释。
- reviewer 指出 selector fixture 只有 AX element,缺 AX window 和 window。
- reviewer 指出 `include_history` 返回 `[last_seen]`,属于误导性表面。

## 综合发现

### 修复动作

- `@selector-resolve` 增加 finalize gate:
  - 0 候选返回 `SELECTOR_NOT_FOUND`。
  - 多候选返回 `AMBIGUOUS_SELECTOR`,并保留候选集。
  - backend `PermissionDenied` / `Unsupported` 映射到 selector error。
- resolver explanation 改成基于候选实际字段比较:
  - window backend 比较 app name、bundle id、window title。
  - AX backend 比较 process name、window title、role/subrole/name/description/actions。
- durable store 增加 `selector_history(selector_id, limit)`。
- `@selector-get include_history:true` 返回 durable history,不再伪造单条 `[last_seen]`。
- 新增 `ax_window_selector_v1.json` 和 `window_selector_v1.json` fixtures。

### 当前判断

- P2 仍保持 dry-run only,没有实现 action by selector。
- `@selector-resolve` 多候选现在是显式错误,不会让 agent 静默选择。
- history 表面现在有真实 durable index 支撑。
