## [2026-07-31 01:00:00] [Session ID: current] 任务名称: verb dispatcher 简化 (LATER_PLANS item 2)

### 任务内容

在 control_core.rs 中加 `outcome_or_error` 和 `structured_or_error` helper,
消除 7 个直接派发 arm 的样板代码。

### 完成过程

- 用 Python 脚本一次性写 2 个 helper + 替换 7 个 match arm。
- Python 脚本里用 multi-line string replace,匹配原始代码块的精确文本。

### 验证

- `cargo build --all-targets` exit 0
- `cargo test --bin rdog` 652 passed; 0 failed; 1 ignored

### 总结感悟

- "消除样板" 比 "拆模块" 更直接:helper 比 trait 注册表便宜,先做 helper 再说
- 2 个 helper 覆盖 7 个 arm,平均每 arm 节约 4 行 (1277 -> 1276 行总长,但 11 个
  arm 加起来从 ~70 行降到 ~40 行)
- PtyClose/PtyDetach 的 3-way match 没简化,因为它们有自己的 protocol error code
  (64 PTY_NOT_FOUND),不属于通用错误 fallback
- executor fallthrough (cancel registry + 响应包装) 没动,涉及 ADR-0005 ticket 03
  和 cancel self-target bug fix 注释,改动风险高
