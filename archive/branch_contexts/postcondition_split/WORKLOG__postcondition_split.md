## [2026-07-31 00:30:00] [Session ID: current] 任务名称: postcondition 拆分 (LATER_PLANS item 1)

### 任务内容

把 press.rs 中 4 个 postcondition helper 搬到独立 `src/control_ax/postcondition.rs` 子模块。

### 完成过程

- 用 Python 脚本按精确行范围提取 4 个 helper 到 postcondition.rs。
- press.rs 删除 helper + 加 `use super::postcondition::*;`。
- control_ax.rs 加 `pub mod postcondition;`。
- control_ax.rs 内部 mod tests 更新 import:从 `crate::control_ax::press::` 改为
  `crate::control_ax::postcondition::` (因为 helper 搬走了)。

### 验证

- `cargo build --all-targets` exit 0
- `cargo test --bin rdog` 652 passed; 0 failed; 1 ignored

### 总结感悟

- 当 helper 从一个模块搬到 sibling 模块时,同一 crate 内的测试代码需要更新 import 路径
  (`crate::control_ax::press::` -> `crate::control_ax::postcondition::`)。
- 单 commit 内紧凑的"搬 + 调引用"模式有效:整 commit < 80 行 diff,review 友好。
- LATER_PLANS item 1 第一步(抽取)完成。下一步可选:
  - 抽出 `verify_and_retry_postcondition` 通用原语
  - 接入 @ax-set-value / @type-text
