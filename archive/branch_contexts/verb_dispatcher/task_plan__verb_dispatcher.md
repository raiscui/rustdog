# 任务计划: verb dispatcher registry 化 (LATER_PLANS item 2)

## 目标

把 `src/control_core.rs` 里 `execute_explicit_control_request` 的 11 个
直接派发 arm 简化为薄壳,通过新增 `outcome_or_error` helper 消除
每 arm 的错误 fallback 样板。

## 启动背景

- 由 control_ax 加深 refactor LATER_PLANS item 2 触发
- 上游 HEAD: `d3ea584` (postcondition 拆分)
- scope A 决定:不动 executor fallthrough,只搬直接派发 arm 的样板

## 阶段

- [ ] 阶段 1: 建旁线上下文 + task_plan 落盘
- [ ] 阶段 2: 单 commit — outcome_or_error helper 引入 + match arms 简化

## 关键决策 (grilling 锁定)

| # | 决策 | 结果 |
|---|---|---|
| Q1 | candidate #2 scope | A — 只搬直接派发 arm,fallthrough 留 control_core.rs |

## 不做的事

- 不引入统一 `dispatch(request, shell, executor, cancel)` 签名 (scope B 拒绝)
- 不引入 `trait VerbDispatcher` 注册表 (scope C 拒绝)
- 不动 executor fallthrough(影响 cancel registry 共享逻辑,风险高)
- 不动 control_core.rs 的响应渲染 helpers(render_response_*)
- 不动 control_core.rs 的 mod tests

## 当前结构 (commit 前)

`execute_explicit_control_request` 的 match 块:
- 11 个 verb 直接派发 arm: 每个 ~10-17 行 (含错误 fallback 样板)
- 1 个 executor fallthrough: ~40 行 (cancel registry + 响应包装)

总 ~150 行。

## 目标结构 (commit 后)

- 11 个 verb arm 缩为 1-2 行 (调用 `outcome_or_error(shell, executor, cancel, ...)`)
- `outcome_or_error` helper: 5-8 行,封装 "Ok(outcome) or Err -> error response" 样板
- executor fallthrough 保持不变

预计 control_core.rs 从 1277 行降到 ~1180 行 (~-100 行)。

## 验证

| Commit | 验证命令 |
|---|---|
| 1 | `cargo build --all-targets` + `cargo test --bin rdog` |

## 状态

**当前: 阶段 1 完成, 阶段 2 待开始。**
