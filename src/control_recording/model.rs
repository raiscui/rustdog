use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTarget {
    pub pid: u32,
    pub hwnd: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureSpec {
    pub targets: Vec<CaptureTarget>,
    pub semantic: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowIdentity {
    pub pid: u32,
    pub hwnd: u64,
    pub process_start: u64,
    pub generation: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayGeometry {
    pub id: u64,
    pub rect: NativeRect,
    pub primary: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureEvent {
    pub sequence: u64,
    pub monotonic_ns: u64,
    pub input: CapturedInput,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CapturedInput {
    MouseDown {
        window: WindowIdentity,
        x: i32,
        y: i32,
        button: u8,
        injected: bool,
    },
    MouseUp {
        window: WindowIdentity,
        x: i32,
        y: i32,
        button: u8,
        injected: bool,
    },
    MouseMove {
        window: WindowIdentity,
        x: i32,
        y: i32,
        injected: bool,
    },
    Scroll {
        window: WindowIdentity,
        x: i32,
        y: i32,
        delta_x: i32,
        delta_y: i32,
        injected: bool,
    },
    DisplayContext {
        generation: u64,
        displays: Vec<DisplayGeometry>,
    },
    WindowContext {
        window: WindowIdentity,
        rect: NativeRect,
        dpi: u32,
        focused: bool,
    },
    Redaction {
        window: Option<WindowIdentity>,
        segment: u64,
        active: bool,
    },
    SemanticCandidate {
        for_sequence: u64,
        window: WindowIdentity,
        observed_ns: u64,
        control_type: u32,
        bounds: NativeRect,
    },
    SemanticMissing {
        for_sequence: u64,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureStop {
    pub last_sequence: Option<u64>,
}
