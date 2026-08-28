//! AX query 模块：无状态的 AX 捕获核心。
//!
//! # 架构
//! - 本模块 (mod.rs): 模块声明与 re-export
//! - `capture.rs`: 全局/窗口/子树捕获入口, target 物化, snapshot 内查找
//!
//! # 纯度契约
//! 本模块是 ax_action / control_observation / control_web / computer_act /
//! screenshot 等消费方共同依赖的稳定底层, 必须保持无状态、零副作用:
//! - 禁止 import control_observation (observation 注册与 ref 解析在 verb 层)
//! - 禁止 import control_protocol (协议解析在 verb 层)
//! - 不持有任何 cache / static 状态 (cache 由 control_ax 的 observation 缓存承担,
//!   其 epoch 真相源在 observation store / control_resource_lane)
//! - 共享类型继续使用 control_ax::types (单一真相源, 与 ax_action 同一约定)
//!
//! 依赖方向恒为: verb 层 (control_ax / control_actions / ...) -> ax_query。

mod capture;

pub use capture::{
    ax_snapshot_status_error, ax_window_id_from_backend_id, capture_ax_find_snapshot,
    capture_current_ax_subtree, capture_current_ax_window_snapshot, capture_default_ax_snapshot,
    capture_semantic_target_snapshot, collect_ax_role_values, current_ax_platform,
    find_ax_element_by_id, materialize_app_window_target, materialize_app_window_target_with,
};
