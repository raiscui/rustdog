# 任务计划: postcondition 逻辑拆分 (LATER_PLANS item 1)

## 目标

把 `src/control_ax/press.rs` 内嵌在 `perform_ax_press_with_postcondition_with` 中的
postcondition 验证 helper 拆到独立 `src/control_ax/postcondition.rs` 子模块。

## 启动背景

- 由 control_ax 加深 refactor (commit 1-6) 的 LATER_PLANS item 1 触发
- 上游 HEAD: `deda434` (control_ax 加深 6/6 commit 完成的 HEAD)
- 仅做 scope A:5 个 helper 函数搬到 postcondition.rs,retry loop 编排留在 press.rs
- `AX_POSTCONDITION_DEPTH` / `AX_POSTCONDITION_MAX_ELEMENTS` 常量留在 types.rs (被 tree.rs 和 control_ax.rs 测试引用)

## 阶段

- [x] 阶段 1: 建旁线上下文 + task_plan 落盘
- [x] 阶段 2: 单 commit 撤销 — scope C YAGNI 关闭

## 关键决策 (grilling 锁定)

| # | 决策 | 结果 |
|---|---|---|
| Q1 | postcondition.rs scope | A — 只搬 5 helper,retry loop 留在 press.rs |

## 目标文件布局

```
src/control_ax/
├── mod.rs (facade)
├── types.rs (已有,常量继续留在 types.rs)
├── tree.rs (已有)
├── input.rs (已有)
├── press.rs (修改 — 删除 5 helper, 加 use super::postcondition::*)
├── postcondition.rs (新增 — 5 helper)
├── query.rs (已有)
└── macos.rs (已有)
```

## 单 commit 内容

| Commit | 内容 | 验证 |
|---|---|---|
| 1 | postcondition.rs 新建 (5 helper),press.rs 删除 helper + 加 use | `cargo build --all-targets` + `cargo test --bin rdog` |

## 搬到 postcondition.rs 的 5 个 helper

| 函数 | 当前 visibility | postcondition.rs visibility |
|---|---|---|
| `normalize_ax_verification_value` | pub(crate) | pub(crate) |
| `collect_ax_values_by_role` | pub(crate) | pub(crate) |
| `observe_current_ax_values_with` | pub(crate) | pub(crate) |
| `build_ax_press_postcondition_report` | pub(crate) | pub(crate) |
| `perform_ax_press_with_postcondition_with` (留 press.rs) | pub(crate) | (不搬) |

## 不做的事

- 不抽出 `verify_and_retry_postcondition` 通用原语 (Q1=A)
- 不把 postcondition 接到 @ax-set-value / @type-text (留 future work)
- 不改常量位置 (AX_POSTCONDITION_DEPTH / MAX_ELEMENTS 留在 types.rs)
- 不动其他 verb 模块 (ax.rs / query.rs / tree.rs 等)

## 状态

**当前: 阶段 2 完成 (grilling 决定 C, YAGNI 撤销)。**

## 决策结论

grilling Q1 选 **C**:不接入 postcondition 到 @ax-set-value / @type-text。

## 理由

- 当前唯一使用方是 @ax-press,postcondition 的语义对其他 verb 不通用
- 在真正有第二个 caller 之前,扩展协议是 speculative 复杂度
- Ponytail YAGNI 适用

## 关闭

LATER_PLANS item 4 关闭。等真正需求出现时再开新任务。

## 关联

- item 1 (postcondition 抽取) 已在 commit d3ea584 完成
- item 2 (verb dispatcher helper 化) 已在 commit e5d117a 完成
- item 3 (target locator seam) 已在 commit 9973fae 完成
- item 4 (本任务) YAGNI 撤销

整个 LATER_PLANS 列表(由 control_ax 加深 refactor 提炼的 4 项后续工作)全部关闭。

Session ID: (当前 session)
