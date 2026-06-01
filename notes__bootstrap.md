## [2026-05-29 23:54:42] [Session ID: codex-20260529-bootstrap-plan] 笔记: bootstrap 产品化规划依据

## 来源

### 来源1: 当前协议和执行入口

- `src/control_protocol.rs`
  - `ControlCommand` 当前已有 `Ping`、`GuiBench`、`Capabilities`、`Observe`,还没有 `Bootstrap` / `GuiProbe`。
  - `parse_control_line` 已经把 `@capabilities`、`@observe`、`@gui-bench` 接入同一 line-control parser。
- `src/control_core.rs`
  - `Ping`、`Capabilities`、`Observe` 当前由 core 直接处理。
  - 这说明 bootstrap 也应该在 core 聚合,不进入 action executor。
- `src/control_observation/observe.rs`
  - `@observe` 已经保证 savefile frame 先发,最终 response line 后发。
  - 这是 bootstrap 复用观察 bundle 的关键边界。

### 来源2: 现有规格和 skill 经验

- `.codex/skills/rdog-control/references/control-workflow.md`
  - 当前 fast bootstrap 是一个 session 中批量发三条 read-only line-control 请求。
- `specs/rdog-computer-use-density-plan.md`
  - 已经提出 `@gui-probe` read-only high-density observation。
  - 已经要求 dense commands 内部处理 liveness / capability,并暴露 `capability_policy`。
  - `@gui-bench` 默认 fixture runner 只读,live replay 必须显式 `runner:"live",allow_side_effects:true`。

## 综合发现

### 推荐设计

- 第一阶段做通用只读 `@bootstrap`,把 liveness、capabilities 和可选 `@observe` 合并为一个响应。
- 第二阶段做任务型 `@gui-probe`,复用 bootstrap + `@web-find` 既有定位能力。
- 不推荐把 liveness/capabilities 直接塞进 `@observe`,因为这会把 observation facade 变成 handshake facade。

### 关键风险

- `@bootstrap` 需要和 Zenoh session bootstrap 术语区分清楚。
- bootstrap 必须复用 `@observe` / `@capabilities` 的单一真相源,不能复制 producer。
- 真实 GUI side effect 继续必须显式 opt-in,不能进入默认 fixture runner。

## [2026-05-30 11:34:00] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 笔记: ralplan consensus 决议

## 来源

### 来源1: Planner / Architect / Critic

- Planner verdict: APPROVE WITH REQUIRED PLAN REVISIONS。
- Architect verdict: ITERATE。
- Critic verdict: APPROVE。

## 综合发现

### 最终硬决策

- `@bootstrap` 作为一等 read-only line-control preflight command。
- 第一版不实现 `@gui-probe`,只保留 deferred follow-up。
- 第一版只接受 `capability_policy:"fresh"`,传 `cached` 返回 `BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED`。
- 所有 `@bootstrap` 都走 Zenoh session channel,包括 `mode:"basic"`。
- observe reusable API 采用 typed-first bundle 外壳,bootstrap 不解析已经渲染好的 `@response`。
- 默认 fixture runner 继续只读,任何 live GUI side effect 仍必须显式 opt-in。
