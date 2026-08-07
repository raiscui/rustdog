# 任务计划: target locator seam (LATER_PLANS item 3 / candidate #4)

## 目标

去掉 `selector_rect_from_ax_rect` 在 `control_ax/tree.rs` 和 `control_window.rs`
里的字面重复,统一到 `control_ax/types.rs`(AxRect 老家)。

## 启动背景

- 由 control_ax 加深 refactor LATER_PLANS item 3 / candidate #4 触发
- 上游 HEAD: `e5d117a` (verb dispatcher helper 化)
- scope A 决定:只去重字面重复,不做 4-way target resolver 大重构

## 阶段

- [ ] 阶段 1: 建旁线上下文 + task_plan 落盘
- [ ] 阶段 2: 单 commit — selector_rect_from_ax_rect 提到 control_ax/types.rs

## 关键决策 (grilling 锁定)

| # | 决策 | 结果 |
|---|---|---|
| Q1 | candidate #4 scope | A — 只去重 selector_rect_from_ax_rect 字面重复 |

## 不做的事

- 不抽 TargetLocator 共享 struct (AxTarget 与 WindowCommandTarget 语义不同)
- 不建 4-way target resolver 模块 (AxTarget 是 element-level,WindowCommandTarget
  是 window-level,语义不重合)
- 不动其他 parser helper (parse_compact_atom / split_object_field 已在
  control_protocol/parsers,control_ax 和 control_window 都通过 pub use 引用,
  没有真正的 duplicate)

## 当前结构 (commit 前)

```
src/control_ax/tree.rs:184       pub(crate) fn selector_rect_from_ax_rect(...)
src/control_window.rs:1213       fn selector_rect_from_ax_rect(...)  // 完全相同 7 行
```

## 目标结构 (commit 后)

```
src/control_ax/types.rs          pub(crate) fn selector_rect_from_ax_rect(...)
src/control_ax/tree.rs           (删除本地副本,改用 super::types::)
src/control_window.rs            (删除本地副本,改用 crate::control_ax::)
```

## 验证

| Commit | 验证 |
|---|---|
| 1 | `cargo build --all-targets` + `cargo test --bin rdog` |

## 状态

**当前: 阶段 1 完成, 阶段 2 待开始。**
