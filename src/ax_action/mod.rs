//! AX action 模块：强类型 action 执行接口。
//!
//! # 架构
//! - 本模块 (mod.rs): 模块声明与 re-export
//! - `execute.rs`: 强类型函数，业务逻辑 + 平台调用
//!
//! # 入口约定
//! 协议层 (control_protocol 的 compact 行解析) 直接产出 typed request，
//! 调用方 (control_actions / control_web) 统一使用强类型函数。
//! 曾为 "RPC 边界字符串 API" 预建的动态 routing 表经 Tickets #03-#11
//! 迁移验证后无生产消费者，已删除 (决策记录见 task_plan.md 2026-08-27)。

mod execute;

pub use execute::{
    focus, perform_action, press, press_sequence, press_with_postcondition, scroll, set_value,
};
// 清除类 continue hint 文案: @ax-press 走 press(), @key 走 control_actions 直接引用。
pub(crate) use execute::CLEAR_ACTION_HINT;
