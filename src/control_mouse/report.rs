use super::request::{
    ClickRequest, DragRequest, MouseButtonMode, MouseButtonName, MouseButtonRequest,
    MouseCoordinateSpace, MousePoint, WheelRequest,
};
use serde_json::json;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseReleaseRecovery {
    NotNeeded,
    ReleaseSucceeded,
    ReleaseFailed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseAction {
    Move,
    Button,
    Click,
    Drag,
    Wheel,
}

impl MouseAction {
    fn as_protocol_str(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Button => "button",
            Self::Click => "click",
            Self::Drag => "drag",
            Self::Wheel => "wheel",
        }
    }
}

/// 鼠标动作成功后的结构化证据。
///
/// 这里直接生成 JSON value 字符串,上层只负责按 request id 包一层。
/// 这样可以避免把 mouse metadata 当普通 stdout 再做一次 JSON 转义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseExecutionReport {
    pub action: MouseAction,
    pub coordinate_space: Option<MouseCoordinateSpace>,
    pub backend: &'static str,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub dx: Option<i32>,
    pub dy: Option<i32>,
    pub from: Option<MousePoint>,
    pub to: Option<MousePoint>,
    pub button: Option<MouseButtonName>,
    pub mode: Option<MouseButtonMode>,
    pub count: Option<u8>,
    pub hold_ms: Option<u64>,
    pub interval_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub steps: Option<u16>,
    pub delta_x: Option<i32>,
    pub delta_y: Option<i32>,
    pub wheel_order: Vec<&'static str>,
    pub released: Option<bool>,
    pub release_recovery: MouseReleaseRecovery,
}

impl MouseExecutionReport {
    pub fn to_value_json(&self) -> String {
        let mut value = json!({
            "kind": "mouse",
            "action": self.action.as_protocol_str(),
            "backend": self.backend,
            "status": "ok",
        });

        if let Some(coordinate_space) = self.coordinate_space {
            value["coordinate_space"] = json!(coordinate_space.as_protocol_str());
        }
        if let Some(x) = self.x {
            value["x"] = json!(x);
        }
        if let Some(y) = self.y {
            value["y"] = json!(y);
        }
        if let Some(dx) = self.dx {
            value["dx"] = json!(dx);
        }
        if let Some(dy) = self.dy {
            value["dy"] = json!(dy);
        }
        if let Some(from) = self.from {
            value["from"] = json!({ "x": from.x, "y": from.y });
        }
        if let Some(to) = self.to {
            value["to"] = json!({ "x": to.x, "y": to.y });
        }
        if let Some(button) = self.button {
            value["button"] = json!(button.as_protocol_str());
        }
        if let Some(mode) = self.mode {
            value["mode"] = json!(mode.as_protocol_str());
        }
        if let Some(count) = self.count {
            value["count"] = json!(count);
        }
        if let Some(hold_ms) = self.hold_ms {
            value["hold_ms"] = json!(hold_ms);
        }
        if let Some(interval_ms) = self.interval_ms {
            value["interval_ms"] = json!(interval_ms);
        }
        if let Some(duration_ms) = self.duration_ms {
            value["duration_ms"] = json!(duration_ms);
        }
        if let Some(steps) = self.steps {
            value["steps"] = json!(steps);
        }
        if let Some(delta_x) = self.delta_x {
            value["delta_x"] = json!(delta_x);
        }
        if let Some(delta_y) = self.delta_y {
            value["delta_y"] = json!(delta_y);
        }
        if !self.wheel_order.is_empty() {
            value["wheel_order"] = json!(self.wheel_order);
        }
        if let Some(released) = self.released {
            value["released"] = json!(released);
        }
        if self.release_recovery != MouseReleaseRecovery::NotNeeded {
            value["release_recovery"] = json!(match self.release_recovery {
                MouseReleaseRecovery::NotNeeded => "not-needed",
                MouseReleaseRecovery::ReleaseSucceeded => "release-succeeded",
                MouseReleaseRecovery::ReleaseFailed => "release-failed",
            });
        }

        serde_json::to_string(&value).expect("mouse report should serialize")
    }
}

pub(crate) fn base_report(
    action: MouseAction,
    coordinate_space: Option<MouseCoordinateSpace>,
    backend: &'static str,
) -> MouseExecutionReport {
    MouseExecutionReport {
        action,
        coordinate_space,
        backend,
        x: None,
        y: None,
        dx: None,
        dy: None,
        from: None,
        to: None,
        button: None,
        mode: None,
        count: None,
        hold_ms: None,
        interval_ms: None,
        duration_ms: None,
        steps: None,
        delta_x: None,
        delta_y: None,
        wheel_order: Vec::new(),
        released: None,
        release_recovery: MouseReleaseRecovery::NotNeeded,
    }
}

impl MouseExecutionReport {
    pub(crate) fn with_point_fields(
        mut self,
        x: Option<i32>,
        y: Option<i32>,
        dx: Option<i32>,
        dy: Option<i32>,
    ) -> Self {
        self.x = x;
        self.y = y;
        self.dx = dx;
        self.dy = dy;
        self
    }

    pub(crate) fn with_button_fields(mut self, request: &MouseButtonRequest) -> Self {
        self.button = Some(request.button);
        self.mode = Some(request.mode);
        self.hold_ms = Some(request.hold_ms);
        self.released = (request.mode == MouseButtonMode::Click).then_some(true);
        self
    }

    pub(crate) fn with_click_fields(mut self, request: &ClickRequest, point: MousePoint) -> Self {
        self.x = Some(point.x);
        self.y = Some(point.y);
        self.button = Some(request.button);
        self.count = Some(request.count);
        self.hold_ms = Some(request.hold_ms);
        self.interval_ms = Some(request.interval_ms);
        self.released = Some(true);
        self
    }

    pub(crate) fn with_drag_fields(
        mut self,
        request: &DragRequest,
        from: MousePoint,
        to: MousePoint,
    ) -> Self {
        self.from = Some(from);
        self.to = Some(to);
        self.button = Some(request.button);
        self.duration_ms = Some(request.duration_ms);
        self.steps = Some(request.steps);
        self.released = Some(true);
        self
    }

    pub(crate) fn with_wheel_fields(
        mut self,
        request: &WheelRequest,
        point: Option<MousePoint>,
        wheel_order: Vec<&'static str>,
    ) -> Self {
        if let Some(point) = point {
            self.x = Some(point.x);
            self.y = Some(point.y);
        }
        self.delta_x = Some(request.delta_x);
        self.delta_y = Some(request.delta_y);
        self.wheel_order = wheel_order;
        self
    }
}
