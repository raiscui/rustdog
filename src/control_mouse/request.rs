use crate::control_observation::SelectorRefindPolicy;
use std::io;

pub const DEFAULT_MOUSE_CLICK_HOLD_MS: u64 = 80;
pub const DEFAULT_MOUSE_CLICK_INTERVAL_MS: u64 = 120;
pub const DEFAULT_MOUSE_DRAG_DURATION_MS: u64 = 450;
pub const DEFAULT_MOUSE_DRAG_STEPS: u16 = 24;
pub const MAX_MOUSE_CLICK_COUNT: u8 = 3;
pub const MAX_MOUSE_DRAG_STEPS: u16 = 120;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseCoordinateSpace {
    OsLogical,
    Relative,
}

impl MouseCoordinateSpace {
    pub fn as_protocol_str(self) -> &'static str {
        match self {
            Self::OsLogical => "os-logical",
            Self::Relative => "relative",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseButtonName {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButtonName {
    pub fn as_protocol_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
            Self::Back => "back",
            Self::Forward => "forward",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseButtonMode {
    Press,
    Release,
    Click,
}

impl MouseButtonMode {
    pub fn as_protocol_str(self) -> &'static str {
        match self {
            Self::Press => "press",
            Self::Release => "release",
            Self::Click => "click",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct MousePoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseAnchor {
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Offset { dx: i32, dy: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseRefTarget {
    pub observation_id: String,
    pub ref_id: String,
    pub anchor: MouseAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseSelectorTarget {
    pub selector_id: String,
    pub auto_refind: bool,
    pub policy: SelectorRefindPolicy,
    pub min_confidence_milli: u16,
    pub anchor: MouseAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseEndpoint {
    Coordinate(MousePoint),
    ObservationRef(MouseRefTarget),
    Selector(MouseSelectorTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseMoveRequest {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub dx: Option<i32>,
    pub dy: Option<i32>,
    pub target: Option<MouseEndpoint>,
    pub coordinate_space: MouseCoordinateSpace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseButtonRequest {
    pub button: MouseButtonName,
    pub mode: MouseButtonMode,
    pub hold_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickRequest {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub target: Option<MouseEndpoint>,
    pub button: MouseButtonName,
    pub count: u8,
    pub hold_ms: u64,
    pub interval_ms: u64,
    pub coordinate_space: MouseCoordinateSpace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragRequest {
    pub from: MouseEndpoint,
    pub to: MouseEndpoint,
    pub button: MouseButtonName,
    pub duration_ms: u64,
    pub steps: u16,
    pub coordinate_space: MouseCoordinateSpace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WheelRequest {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub target: Option<MouseEndpoint>,
    pub delta_x: i32,
    pub delta_y: i32,
    pub coordinate_space: MouseCoordinateSpace,
}

pub(crate) fn validate_mouse_move_shape(
    request: &MouseMoveRequest,
    kind: io::ErrorKind,
) -> io::Result<()> {
    if request.target.is_some() {
        if request.x.is_some()
            || request.y.is_some()
            || request.dx.is_some()
            || request.dy.is_some()
        {
            return Err(io::Error::new(
                kind,
                "@mouse-move 的 `target` 不能与 x/y/dx/dy 混用",
            ));
        }
        if request.coordinate_space != MouseCoordinateSpace::OsLogical {
            return Err(io::Error::new(
                kind,
                "@mouse-move target 只支持 coordinate_space=\"os-logical\"",
            ));
        }
        return Ok(());
    }

    match request.coordinate_space {
        MouseCoordinateSpace::OsLogical => {
            if request.x.is_none() || request.y.is_none() {
                return Err(io::Error::new(
                    kind,
                    "@mouse-move coordinate_space=\"os-logical\" 必须包含 `x` 和 `y`",
                ));
            }
            if request.dx.is_some() || request.dy.is_some() {
                return Err(io::Error::new(
                    kind,
                    "@mouse-move 不能同时包含绝对坐标和相对坐标",
                ));
            }
        }
        MouseCoordinateSpace::Relative => {
            if request.dx.is_none() || request.dy.is_none() {
                return Err(io::Error::new(
                    kind,
                    "@mouse-move coordinate_space=\"relative\" 必须包含 `dx` 和 `dy`",
                ));
            }
            if request.x.is_some() || request.y.is_some() {
                return Err(io::Error::new(
                    kind,
                    "@mouse-move 不能同时包含绝对坐标和相对坐标",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_wheel_shape(request: &WheelRequest, kind: io::ErrorKind) -> io::Result<()> {
    if request.delta_x == 0 && request.delta_y == 0 {
        return Err(io::Error::new(
            kind,
            "@wheel 的 `delta_x` 和 `delta_y` 至少要有一个非 0",
        ));
    }
    if request.target.is_some() {
        if request.x.is_some() || request.y.is_some() {
            return Err(io::Error::new(kind, "@wheel 的 `target` 不能与 x/y 混用"));
        }
        if request.coordinate_space != MouseCoordinateSpace::OsLogical {
            return Err(io::Error::new(
                kind,
                "@wheel target 只支持 coordinate_space=\"os-logical\"",
            ));
        }
        return Ok(());
    }

    match (request.x, request.y) {
        (Some(_), Some(_)) => {
            if request.coordinate_space != MouseCoordinateSpace::OsLogical {
                return Err(io::Error::new(
                    kind,
                    "@wheel 提供 x/y 时只支持 coordinate_space=\"os-logical\"",
                ));
            }
            Ok(())
        }
        (None, None) => Ok(()),
        _ => Err(io::Error::new(kind, "@wheel 的 `x` 和 `y` 必须同时提供")),
    }
}
