//! AX query 模块：无状态的 AX 捕获核心。
//!
//! # 架构
//! - 本模块 (mod.rs): 模块声明与 re-export
//! - `capture.rs`: 全局/窗口/子树捕获入口, scoped 分发, target 物化, snapshot 内查找
//!
//! # 纯度契约
//! 本模块是 ax_action / control_observation / control_web / computer_act /
//! screenshot 等消费方共同依赖的稳定底层, 必须保持无状态、零副作用:
//! - 禁止 import control_observation (observation 注册与 ref 解析在 verb 层)
//! - 禁止 import control_protocol (协议解析在 verb 层)
//! - 禁止消费 verb 层请求类型 (如 query.rs 的 AxFindRequest);
//!   `capture_scoped_snapshot` 只吃调用方已解析好的 window_id,
//!   observation ref -> backend id 的解析由 verb 层完成;
//!   app 名 -> window_id 的物化属于 Window API 域 (非 observation),
//!   由本模块的 `materialize_app_window_target` 提供
//! - 不持有任何 cache / static 状态 (cache 由 control_ax 的 observation 缓存承担,
//!   其 epoch 真相源在 observation store / control_resource_lane)
//!
//! # 依赖方向
//! verb 层 (control_ax / control_actions / ...) 编排本模块; 本模块反向依赖:
//! - control_ax 共享内核 (types / AxBackend trait / platform_* 平台分发)
//! - control_resource_lane (窗口捕获时嵌入 capture-start epoch 快照)
//! - control_window (app 名物化为 canonical window id)
//! 该组内核依赖是 ADR-0008 Amendment 2 认可的共享模式, 不构成编排回路。

mod capture;

pub use capture::{
    ax_snapshot_status_error, ax_window_id_from_backend_id, capture_current_ax_subtree,
    capture_current_ax_window_snapshot, capture_default_ax_snapshot, capture_scoped_snapshot,
    capture_semantic_target_snapshot, collect_ax_role_values, current_ax_platform,
    find_ax_element_by_id, materialize_app_window_target, materialize_app_window_target_with,
};
