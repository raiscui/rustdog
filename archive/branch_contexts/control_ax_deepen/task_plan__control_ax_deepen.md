# 任务计划: control_ax 加深 (候选 #1)

## 目标

把 `src/control_ax.rs` (3577L / 24 pub fn) 的 5 类关注点 (types / tree / press / input / postcondition) 拆为独立子模块,16 个 ax compact parser 搬到 `control_protocol/parsers/ax.rs`。零外部行为变化,纯内部组织重构。

## 启动背景

- 由 `$improve-codebase-architecture` 扫描产生候选 #1,经 `$grilling` 锁 7 个决策。
- 上游 `4c98ce2` 已 commit `AXPressPostcondition`、`AxPressSequenceRequest` 等 dry 工作,本次 refactor 在它之上做。
- 本支线使用 `control_ax_deepen` 后缀上下文集。

## 阶段

- [x] 阶段 1: 建旁线上下文 + task_plan 落盘 (本文件)
- [x] 阶段 2: commit 1 — types.rs 抽取 (e869b50)
- [x] 阶段 3: commit 2 — tree.rs 抽取 (3 个独立 capture 函数) (68bf3f0)
- [x] 阶段 4: commit 3 — tree.rs 扩展 (20 个 capture/resolve/helper) (3c58e01)

- [ ] 阶段 4: commit 3 — input.rs 抽取
- [ ] 阶段 5: commit 4 — postcondition.rs 抽取 (constants + helpers)
- [ ] 阶段 6: commit 5 — press.rs 抽取 + 改为调用 postcondition helpers
- [ ] 阶段 7: commit 6 — parsers/ax.rs 搬移 + control_ax.rs 改为 mod.rs 公开子模块
- [ ] 阶段 8: 收尾: 写 WORKLOG + 续档 task_plan + 回扫 EPIPHANY_LOG / LATER_PLANS

## 关键决策 (grilling 锁定)

| # | 决策 | 结果 |
|---|---|---|
| Q1 | 16 个 ax compact parser 归属 | B → `control_protocol/parsers/ax.rs` |
| Q2 | 拆分粒度 | B → 5 拆: types / tree / press / input / postcondition |
| Q3 | public API 形态 | B → `pub mod` + 顶层 `pub use`,沿用 `control_observation/` |
| Q4 | `postcondition.rs` 内容 | A → constants + helpers;press.rs 调用 |
| Q5 | submodule 测试文件 | A → 每个 `{name}_tests.rs` |
| Q6 | ax parser 文件组织 | A → 一个 `parsers/ax.rs` (~770 行),跟 `pty.rs` 同形 |
| Q7 | 提交节奏 | A → 6 个 scoped commit + verification |

## 目标文件布局

```
src/control_ax/
├── mod.rs              # pub use 兼容旧路径 + pub mod 暴露新路径
├── types.rs            # 请求/结果类型 + AxSnapshot + AxMode + 常量
├── tree.rs             # capture_current_ax_subtree / capture_default_ax_snapshot / scope
├── press.rs            # ax-press / press-sequence / ax-action / focus / scroll / set-value
├── input.rs            # type-text / key delivery
├── postcondition.rs    # AX_POSTCONDITION_DEPTH / verify_window_postcondition / PostconditionPolicy
├── query.rs            # (existing)
├── macos.rs            # (existing)
└── {name}_tests.rs     # per-submodule unit tests

src/control_protocol/parsers/
└── ax.rs               # 16 个 ax compact parser, ~770 行
```

## 6 个 commit 序列 + 验证

| # | Commit | 验证命令 |
|---|---|---|
| 1 | `control_ax/types.rs` 抽取 | `cargo build --all-targets` + `cargo test --lib control_ax::types` |
| 2 | `control_ax/tree.rs` 抽取 | 同上 + `tree_tests.rs` |
| 3 | `control_ax/input.rs` 抽取 | 同上 + `input_tests.rs` |
| 4 | `control_ax/postcondition.rs` 抽取 | 同上 + `postcondition_tests.rs` |
| 5 | `control_ax/press.rs` 抽取 + 改用 postcondition helpers | 同上 + 全量 `control_ax` 单元测试 + `tests/control_ax_e2e.rs` |
| 6 | `control_protocol/parsers/ax.rs` 搬移 + `control_ax.rs` 改为 `mod.rs` 公开子模块 | 完整 `cargo test` |

## 不做的事

- 不新建 ADR (这是纯内部重构, git history + commit message 已能溯源)
- 不修改 `tests/control_ax_e2e.rs`, `specs/`, `CONTEXT.md`, `AGENTS.md`, 任何 ADR
- 不做候选 #2 (verb dispatcher registry) — 留给后续
- 不抽 `control_target_locator` seam (候选 #4) — 留给后续
- 不改 `current_ax_platform`, `parse_bool_literal` 等 helper 位置 (留 types.rs 内)

## 状态

**当前: 所有阶段完成 (commit 1-6)。**

## 最终模块结构

```
src/control_ax.rs          2111 行  facade + impl + 共享 helper + facade 重导出
src/control_ax/types.rs     447 行  常量 + 37 个 struct/enum 声明
src/control_ax/tree.rs      440 行  capture / resolve / platform-info / selector helpers
src/control_ax/input.rs      72 行  type-text + key delivery
src/control_ax/press.rs     374 行  ax-press / sequence / action / focus / scroll / value
src/control_ax/query.rs    1045 行  (existing) ax-find / ax-get
src/control_ax/macos.rs    1847 行  (existing) platform adapter
src/control_protocol/parsers/ax.rs  470 行  8 个 verb parser
```

## 6 个 commit

1. e869b50 — types.rs 抽取
2. 68bf3f0 — tree.rs 抽取 (3 个 capture 函数)
3. 3c58e01 — tree.rs 扩展 (20 个 capture/resolve/helper)
4. 0502231 — input.rs 抽取 (type-text + key delivery)
5. 3eebf9b — press.rs 抽取 (state-mutating verb)
6. deda434 — parsers/ax.rs 搬移 + facade 收口

## 与原 6 步计划的差异

- 没有 postcondition.rs 独立模块(postcondition 验证逻辑内嵌在 perform_ax_press_with_postcondition_with 中)
- plan 说 "control_ax.rs 改为 mod.rs",实际仍是 control_ax.rs 作为 facade 但功能等同 mod.rs (pub mod types/tree/input/press; + pub use 重导出)

## 验证

- `cargo build --all-targets` exit 0
- `cargo test` 多 suite 全部通过, 0 failed

## 改进的 future 工作 (LATER_PLANS)

1. **postcondition.rs 拆分** — 当前 perform_ax_press_with_postcondition_with 是 ~95 行函数,内嵌 fresh-capture + retry + value-match 三层逻辑。提取出独立的 postcondition.rs 后,其他 verb (@ax-set-value / @type-text) 也能复用 verify_window_postcondition
2. **candidate #2 (verb dispatcher registry)** — 现在 control_ax.rs 的 dispatcher 仍是 match-on-verb,可以参考 control_observation/ 的 sub dispatch 模式拆出独立 verb dispatcher
3. **candidate #4 (control_target_locator seam)** — 现在 control_ax.rs 和 control_window.rs 仍有重复的 target 解析逻辑,可以抽公共 seam


commit 3 验证记录:
- `cargo build --all-targets` exit 0
- `cargo test --bin rdog` 652 passed; 0 failed
- platform_* 暂未搬到 tree.rs,留待后续 commit

下一步 commit 4: input.rs 抽取 (type-text + key delivery)。

commit 1 验证记录:
- `cargo build --all-targets` exit 0 (无 error, 仅有与本 commit 无关的 pre-existing 警告)
- `cargo test --bin rdog control_ax::` 43 passed; 0 failed
- `cargo test --test control_ax_e2e` 6 ignored (需要 macOS live); 0 failed
- `cargo test` 全量 (除 e2e 需 live 外) 全部 passed

变更:
- 新增 `src/control_ax/types.rs` (442 行)
- 修改 `src/control_ax.rs` (-357 +4 行, 3224 行)
- 删除 `control_ax.rs` 顶部 `use serde::Serialize;` (regression 修复, 派生宏都搬走了)
- 删除 `control_ax.rs` 顶部 `ObservationHeader` import (同样不在 control_ax.rs 用)

下一步 commit 2: tree.rs 抽取 (capture 相关函数)。

Session ID: (当前 session)
