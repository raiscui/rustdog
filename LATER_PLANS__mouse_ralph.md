## [2026-05-14 12:01:41] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 后续计划: 拆分 control_mouse 大文件

### 背景
- `src/control_mouse.rs` 当前 1409 行,超过项目对静态语言文件尽量不超过 1000 行的偏好。
- 当前任务已经通过行为验证,不宜在 Ralph 收口阶段再做大范围模块拆分。

### 建议
- 后续单独拆成 `control_mouse/parser.rs`、`control_mouse/plan.rs`、`control_mouse/execution.rs`、`control_mouse/tests.rs` 或等价结构。
- 拆分时必须保持现有 parser tests、plan tests、failure recovery tests 全绿。
