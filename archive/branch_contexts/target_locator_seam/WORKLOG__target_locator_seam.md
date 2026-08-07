## [2026-07-31 01:30:00] [Session ID: current] 任务名称: target locator seam (LATER_PLANS item 3)

### 任务内容

把 selector_rect_from_ax_rect 从 control_ax/tree.rs 和 control_window.rs 字面
重复(各 7 行完全相同)统一到 control_ax/types.rs。

### 完成过程

- types.rs 加 SelectorRect import + pub(crate) fn selector_rect_from_ax_rect
- tree.rs 删除本地副本,通过 use super::types::* 引用
- window.rs 删除本地副本,加 use crate::control_ax::selector_rect_from_ax_rect
- control_ax.rs 的 `use self::tree::{...}` 移除 selector_rect_from_ax_rect
  (改由 pub use self::types::* 提供给 caller)

### 验证

- `cargo build --all-targets` exit 0
- `cargo test --bin rdog` 652 passed; 0 failed; 1 ignored
- 净 -2 行 (21 删除 / 19 插入)

### 总结感悟

- 当 helper 在 2 个文件完全相同时,放回数据类型的"老家"是最自然的位置
  (AxRect 在 types.rs,所以 selector_rect_from_ax_rect 也放 types.rs)
- 不需要引入新模块 — Ponytail 视角下,7 行 function 不值得独立文件
- `pub use self::types::*` 在 control_ax.rs 已经存在,新加的 helper 自动
  通过这条 re-export 暴露给外部 caller,无需额外 pub use
- "4-way target resolution" 听上去大,实际只是字面重复才值得动;语义不同的
  部分(AxTarget element-level vs WindowCommandTarget window-level)统一是
  premature complexity,留给真正的需求出现时再考虑
