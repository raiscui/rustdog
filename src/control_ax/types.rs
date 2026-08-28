//! control_ax 模块的纯数据定义。
//!
//! 本文件只放常量、结构体、枚举。所有 impl 块、函数和 submodule
//! (`query` / `macos`) 留在 control_ax.rs 顶层。
//!
//! 通过 control_ax.rs 顶层的 `pub use self::types::*;` 重导出,
//! 外部 caller 无需切换 import 路径,内部 impl 块也仍能找到这些类型。
//!
//! 等后续 commit 把 `tree / press / input / postcondition` 拆出独立子模块时,
//! 与 verb 对应的 impl 块再随之搬走。

use serde::Serialize;

use crate::{
    control_observation::ObservationHeader, control_resource_lane::ResourceEpochCapture,
    control_window::WindowActionReport,
};

// ---- was lines 22-40 ----
pub const AX_SCHEMA: &str = "rdog.ax.v1";
pub const AX_WINDOWS_DEPTH: u8 = 1;
pub const AX_WINDOWS_MAX_ELEMENTS: u16 = 80;
pub const AX_WINDOWS_INCLUDE_VALUES: bool = false;
pub const AX_INTERACTIVE_DEPTH: u8 = 2;
pub const AX_INTERACTIVE_MAX_ELEMENTS: u16 = 200;
pub const AX_INTERACTIVE_INCLUDE_VALUES: bool = false;
pub const DEFAULT_AX_DEPTH: u8 = 4;
pub const DEFAULT_AX_MAX_ELEMENTS: u16 = 1000;

/// 通用 guarded press fresh 观察所使用的 AX 树深度。
///
/// Calculator 等原生 macOS 应用的 AXStaticText result value 节点常位于
/// depth >= 5;若使用默认 4 会把 postcondition 证据截断,verified 永远为 false。
pub const AX_POSTCONDITION_DEPTH: u8 = 8;

/// 与 `AX_POSTCONDITION_DEPTH` 配对的元素上限,确保深值节点不被过早截断。
pub const AX_POSTCONDITION_MAX_ELEMENTS: u16 = 5_000;
pub const DEFAULT_AX_INCLUDE_VALUES: bool = true;

// ---- was lines 42-47 ----
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AxMode {
    Windows,
    Interactive,
    Full,
}

// ---- was lines 71-76 ----
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AxModePreset {
    pub depth: u8,
    pub max_elements: u16,
    pub include_values: bool,
}

// ---- was lines 78-84 ----
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxTreeRequest {
    pub scope: AxTreeScope,
    /// `AppMenu` root 的精确应用选择器。
    ///
    /// 这不是结果过滤条件。平台后端必须据此缩小 AX capture 范围,
    /// 防止无关应用先耗尽 `max_elements` 后遗漏目标菜单。
    pub app_menu_app: Option<String>,
    pub depth: u8,
    pub max_elements: u16,
    pub include_values: bool,
    /// 只读缓存查询使用的 observation 身份。缺省时保持 live capture 语义。
    pub observation_id: Option<String>,
    /// 缓存查询要求的观察 epoch。必须与 observation_id 成对出现。
    pub epoch: Option<u64>,
}

// ---- was lines 97-100 ----
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AxTreeScope {
    Windows,
    /// 应用菜单栏不是任何普通窗口的子树。
    /// 这个 root 让菜单元素保留自己的 AX 生命周期和 target id 语义。
    AppMenu,
}

// ---- was lines 102-106 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct AxPressRequest {
    pub target: AxTarget,
    pub postcondition: Option<AxPressPostcondition>,
}

// ---- was lines 108-113 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct AxPressPostcondition {
    pub role: String,
    pub expected_value: String,
    pub max_attempts: usize,
}

// ---- was lines 115-118 ----
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxPressSequenceRequest {
    pub targets: Vec<AxTarget>,
}

// ---- was lines 120-124 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct AxActionRequest {
    pub target: AxTarget,
    pub action: AxActionName,
}

// ---- was lines 126-134 ----
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum AxActionName {
    Press,
    Open,
    Confirm,
    Cancel,
    ShowMenu,
    ScrollToVisible,
}

// ---- was lines 153-158 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct AxSetValueRequest {
    pub target: AxTarget,
    pub value: String,
    pub mode: AxValueSetMode,
}

// ---- was lines 160-165 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct AxFocusRequest {
    pub target: Option<AxTarget>,
    pub window_id: Option<String>,
    pub activate: bool,
}

// ---- was lines 167-172 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct AxScrollRequest {
    pub target: AxTarget,
    pub direction: AxScrollDirection,
    pub pages: u16,
}

// ---- was lines 174-180 ----
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum AxScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

// ---- was lines 193-197 ----
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum AxValueSetMode {
    Replace,
    Append,
}

// ---- was lines 208-214 ----
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeTextRequest {
    pub target: AxTarget,
    pub text: String,
    pub mode: TypeTextMode,
    pub allow_clipboard: bool,
}

// ---- was lines 216-222 ----
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TypeTextMode {
    Auto,
    AxValue,
    TargetedKeyboard,
    Clipboard,
}

// ---- was lines 235-239 ----
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardRestoreStatus {
    pub restored: bool,
    pub skipped_reason: Option<&'static str>,
}

// ---- was lines 257-270 ----
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
pub struct AxTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    pub ref_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---- was lines 345-351 ----
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub struct AxRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

// ---- was lines 353-366 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxSnapshot {
    pub schema: &'static str,
    pub platform: String,
    pub capture_status: String,
    pub permission_status: String,
    pub coordinate_space: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<ObservationHeader>,
    #[serde(skip)]
    pub(crate) resource_epoch_capture: Option<ResourceEpochCapture>,
    pub window_count: usize,
    pub element_count: usize,
    pub truncated: bool,
    pub windows: Vec<AxWindow>,
}

// ---- was lines 501-518 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxWindow {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    pub ref_id: Option<String>,
    pub pid: i32,
    pub process_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<AxRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    pub elements: Vec<AxElement>,
}

// ---- was lines 526-549 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxElement {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    pub ref_id: Option<String>,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub value_redacted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<AxRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub actions: Vec<String>,
    pub ax_path: Vec<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<AxElement>,
}

// ---- was lines 551-555 ----
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxCapturedSubtree {
    pub element: AxElement,
    pub truncated: bool,
}

// ---- was lines 575-581 ----
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxResolvedTargetRect {
    pub target_id: String,
    pub target_type: &'static str,
    pub window_id: Option<String>,
    pub rect: Option<AxRect>,
}

// ---- was lines 712-721 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxActionReport {
    pub kind: &'static str,
    pub action: String,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    // 2026-08-04 (LLM 兼容): 响应自包含按钮描述, runner/agent 无需
    // 依赖 @ax-find 映射即可恢复 timeline label (先按后查场景)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    // 2026-08-04 (清除类操作 hint): 清除类按钮被按下后, 给 agent 一句
    // 通用引导, 防止"清除子目标完成 -> 流程断裂"的系统性迷失。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub performed: bool,
    pub status: &'static str,
}

// ---- was lines 741-751 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxPressPostconditionStepReport {
    pub index: usize,
    pub performed: bool,
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub observed_values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---- was lines 753-767 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxPressPostconditionReport {
    pub kind: &'static str,
    pub action: &'static str,
    pub performed: bool,
    pub verified: bool,
    pub status: &'static str,
    pub role: String,
    pub expected_value: String,
    pub attempt_count: usize,
    pub max_attempts: usize,
    pub steps: Vec<AxPressPostconditionStepReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---- was lines 776-786 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxPressSequenceStepReport {
    pub index: usize,
    pub description: String,
    pub performed: bool,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---- was lines 812-824 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxPressSequenceReport {
    pub kind: &'static str,
    pub action: &'static str,
    pub performed: bool,
    pub status: &'static str,
    pub step_count: usize,
    pub steps: Vec<AxPressSequenceStepReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---- was lines 833-842 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxPerformedActionReport {
    pub kind: &'static str,
    pub action: String,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub performed: bool,
    pub status: &'static str,
}

// ---- was lines 866-878 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxSetValueReport {
    pub kind: &'static str,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub mode: &'static str,
    pub performed: bool,
    pub status: &'static str,
    pub settable: bool,
    pub old_value_redacted: bool,
    pub new_value_redacted: bool,
}

// ---- was lines 907-924 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeTextReport {
    pub kind: &'static str,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub mode: &'static str,
    pub delivered_via: &'static str,
    pub performed: bool,
    pub status: &'static str,
    pub used_clipboard: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipboard_restore_policy: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipboard_restored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipboard_restore_skipped_reason: Option<&'static str>,
}

// ---- was lines 992-1005 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyDeliveryReport {
    pub kind: &'static str,
    pub backend: String,
    pub key: String,
    pub mode: &'static str,
    pub delivery: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    // 2026-08-04 (清除类按键 hint): @key 的清除类按键 (escape/backspace/delete)
    // 模拟成功后带通用引导, 防止"清除子目标完成 -> 流程断裂"。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub performed: bool,
    pub status: &'static str,
}

// ---- was lines 1033-1048 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxFocusReport {
    pub kind: &'static str,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    pub activated: bool,
    pub performed: bool,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation: Option<WindowActionReport>,
}

// ---- was lines 1113-1125 ----
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxScrollReport {
    pub kind: &'static str,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub direction: &'static str,
    pub pages: u16,
    pub line_steps: i32,
    pub delivered_via: &'static str,
    pub performed: bool,
    pub status: &'static str,
}

// ---- was lines 1170-1172 ----

#[derive(Debug, Copy, Clone, Default)]
pub struct SystemAxBackend;
