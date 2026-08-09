## [2026-07-30 22:30:00] [Session ID: current] 任务名称: control_ax 加深 — commit 1 (types.rs 抽取)

### 任务内容

- 在 src/control_ax/ 下新建 types.rs 子模块,装 12 个常量 + 37 个 struct/enum 声明。
- 修改 src/control_ax.rs:删除重复声明,加 `pub mod types;` 和 `pub use self::types::*;` 重导出。
- 清理 regression:`use serde::Serialize;` 和 `ObservationHeader` import 这两个在 control_ax.rs 顶层已无用途。

### 完成过程

- 用 Python 脚本从 control_ax.rs 按精确行范围提取常量 + 声明块,写到 types.rs。
- 同步删除 control_ax.rs 中对应行,顶部插入模块声明 + use 重导出。
- 编译发现 types.rs 缺 `ObservationHeader` 和 `WindowActionReport` 两个外部类型 import;补 `use crate::{control_observation::ObservationHeader, control_window::WindowActionReport};`。
- 第二次编译发现 control_ax.rs 顶层 `use serde::Serialize;` 变 unused(派生宏都搬走了),删除。
- 同样删除 control_ax.rs 顶层 `ObservationHeader` import(struct 搬走了,control_ax.rs 内不再用)。

### 验证

- `cargo build --all-targets` exit 0 (与本 commit 无关的 pre-existing 警告 7 条保留)
- `cargo test --bin rdog control_ax::` 43 passed; 0 failed
- `cargo test --test control_ax_e2e` 6 ignored (需要 macOS live); 0 failed
- 全量 `cargo test` exit 0 (除 live e2e 外全部通过)

### 总结感悟

- "先小步 refactor,再大刀阔斧" 有效:types.rs 抽出后 control_ax.rs 仍有 3224 行,但文件结构已建立"types 在子模块"的事实。后续 commit 可以无脑搬 impl 块和函数。
- 不要为了"全套拆分"一次到位。impl 块跨文件引用同 crate 的 struct 是合法的;先搬数据、再搬实现可以分阶段。
- `pub use self::types::*;` 的双向兼容:外部 caller 路径不变 + 内部 impl 块能找到 struct 名。
- 每次"搬"都先做精确行范围提取 (Python 脚本) + 编译验证 + 测试验证的三步,才下手。

## [2026-07-30 23:00:00] [Session ID: current] 任务名称: control_ax 加深 — commit 2 (tree.rs 初始 3 个 capture 函数)

### 任务内容

- 在 src/control_ax/ 下新建 tree.rs 子模块,装 3 个独立 capture / platform 函数。
- 修改 src/control_ax.rs:删除对应函数,加 `pub mod tree;` 和显式 `pub use`。

### 完成过程

- 写了 tree.rs,装 current_ax_platform + capture_default_ax_snapshot + capture_current_ax_subtree。
- capture_current_ax_subtree 调用 control_ax.rs 的 `platform_capture_current_subtree`(commit 2 已经把后者标 pub(crate))。
- tree.rs 通过 `super::AxBackend` 引用 AxBackend trait(留在 control_ax.rs)。
- control_ax.rs 顶部加 `pub mod tree;` 和 `pub use self::tree::{capture_current_ax_subtree, capture_default_ax_snapshot, current_ax_platform};`。

### 验证

- `cargo build --all-targets` exit 0
- `cargo test --bin rdog control_ax::` 43 passed; 0 failed

### 总结感悟

- 起步 commit 故意小(只 3 个函数 ~25 行),后续逐步扩展。小步确保每次编译/测试绿,可以早期发现问题。
- "独立函数"判定:函数体只引用 super::* 或 std::* 或 sibling pub item,可立刻搬。需要内部 helper 的不进本次。

## [2026-07-30 23:30:00] [Session ID: current] 任务名称: control_ax 加深 — commit 3 (tree.rs 扩展 20 个函数)

### 任务内容

- tree.rs 扩展到 20 个函数:capture / resolve / selector helpers / error mapping
- 调整 visibility:`invalid_data` / `invalid_input` / `to_invalid_input` 改 `pub(crate)` 供 tree.rs 调用
- `mod query;` 改 `pub mod query;` 让 tree.rs 通过 `super::query` 访问 AxFindRequest 等
- 所有 `fn platform_*` 改 `pub(crate)` 供 tree.rs 通过 super 调用

### 完成过程

- 用 Python 脚本提取 20 个函数 + 调整 control_ax.rs 的 pub use/use 列表。
- 编译发现 `capture_current_ax_window_snapshot` 被 control_web 导入,所以需要保持 `pub` 而非 `pub(crate)`。
- 编译发现 `pub(crate) pub(crate)` 双重修饰(我两次都改成 pub(crate) 了),清理掉。
- 编译发现 `capture_current_ax_window_snapshot` 在 control_ax.rs 同时被 `pub use` 和 `use` 引用 → 冲突。删除 `use` 中的引用,保留 `pub use`。
- 编译发现 `platform_*` 只有第一个定义被改 pub(crate),第二个 cfg-pair 还是 private。统一 regex 重写。

### 验证

- `cargo build --all-targets` exit 0
- `cargo test --bin rdog` 652 passed; 0 failed; 1 ignored

### 总结感悟

- visibility 反复横跳是这次 refactor 最大的痛点。规则:`fn` 默认 child 不可见,`pub(crate)` 同 crate 都可见,`pub` 跨 crate 可见。
- 跨模块 visibility 方向不对称:parent 不能访问 child 的 private(必须 pub+),但 child 不能访问 parent 的 private(必须 pub+)。
- 一开始没把"哪些函数被外部 caller 用"摸清楚,导致多次返工调整 visibility。下次先扫一遍 caller 矩阵。

## [2026-07-30 23:50:00] [Session ID: current] 任务名称: control_ax 加深 — commit 4 (input.rs: type-text + key delivery)

### 任务内容

- 新建 src/control_ax/input.rs,装 perform_default_key_delivery + perform_default_type_text + 2 个 remap helper。
- control_ax.rs 加 `pub mod input;` 和 `pub use`。

### 完成过程

- tree.rs 已建立模式,input.rs 沿用相同的 header (super::types::* + super::helpers)。
- perform_default_key_delivery 用 KeyDelivery 枚举需要从 control_protocol 导入。
- perform_default_type_text 用 SystemAxBackend.type_text() 需要 AxBackend trait 导入。

### 验证

- `cargo build --all-targets` exit 0
- `cargo test --bin rdog` 652 passed; 0 failed; 1 ignored

## [2026-07-31 00:00:00] [Session ID: current] 任务名称: control_ax 加深 — commit 5 (press.rs: state-mutating verb)

### 任务内容

- 新建 src/control_ax/press.rs,装 7 个公开 perform + 8 个 helper
- control_ax.rs 加 `pub mod press;` 和 `pub use`
- control_ax.rs 的 mod tests 加 `use crate::control_ax::press::{...}` 用于直接调用 `pub(crate)` helper

### 完成过程

- press.rs 需要 tree.rs 的 capture helper (`capture_current_ax_window_snapshot` 等),通过 `use super::tree::{...};` 引入。
- 测试块在 control_ax.rs 内部,需要直接调用 `perform_ax_press_with_postcondition_with` 等 helper。这些是 `pub(crate)`,在 sibling module 中可见,但需显式 `use` 才能引用。

### 验证

- `cargo build --all-targets` exit 0
- `cargo test --bin rdog` 652 passed; 0 failed; 1 ignored

## [2026-07-31 00:15:00] [Session ID: current] 任务名称: control_ax 加深 — commit 6 (parsers/ax.rs + facade 收口)

### 任务内容

- 新建 src/control_protocol/parsers/ax.rs,装 8 个 verb parser (parse_ax_tree_payload 等)
- 17 个 parser helper (parse_ax_target / assign_once / key_mode_as_str 等) 留在 control_ax.rs (被 query.rs / control_observation / screenshot/tests 多处共享)
- control_ax.rs 加 `pub use crate::control_protocol::parsers::ax::{...};` 保持外部 caller 路径不变
- `mod parsers;` (control_protocol.rs) 改 `pub mod parsers;`
- `mod ax;` (parsers.rs) 改 `pub mod ax;`
- AxTarget::validate 改 `pub(crate)`

### 完成过程

- brace matching 算法必须跳过字符串字面量和注释内的 `{` `}`,否则会被 `{field_name}` 这种插值误导,导致 parse_ax_press_payload 等长函数 brace 计数错乱,WARN "brace not found in 600 lines"。
- 加了 strip_strings_and_comments 函数处理块注释、行注释、doc comment、字符串字面量、raw string。
- pub(crate) visibility 用 Python regex 修改时遇到 escape 问题:`r'fn assign_once\\('` 的 `\\` 多了一层转义,导致 findall 返回空。改用简单的 `content.replace('fn assign_once<', 'pub(crate) fn assign_once<', 1)` 字符串替换。

### 验证

- `cargo build --all-targets` exit 0
- `cargo test` 全 suite 通过 (652 + 3 + 2 + 15 + 1 + 8 + 1 + 2 + 1 + 26 + 12 等),0 failed

### 总结感悟 (整个 6 commit 周期)

- "PONYTAIL full" 在 refactor 任务里其实更应该是 "PANICTAIL" — 步步小跑,每步必须编译绿
- 6 commit vs 1 commit:每 commit 是一次回滚点,如果中途出错只丢一部分工作
- visibility 规则需要事先写清楚一张"哪些 caller 用什么"的表,而不是边搬边查
- Python 脚本 brace matching 必须 strip 字符串,这是 Rust 源码处理的常见陷阱
