mod backend;
mod parser;
mod plan;
mod report;
mod request;
mod target;

#[allow(unused_imports)]
pub use backend::{perform_mouse_plan, MouseBackend};
pub use parser::{
    parse_click_payload, parse_drag_payload, parse_mouse_button_payload, parse_mouse_move_payload,
    parse_wheel_payload,
};
#[allow(unused_imports)]
pub use plan::{
    build_click_plan, build_drag_plan, build_mouse_button_plan, build_mouse_move_plan,
    build_wheel_plan, MouseExecutionPlan, MousePlanStep,
};
#[allow(unused_imports)]
pub use report::MouseExecutionReport;
#[allow(unused_imports)]
pub use request::{
    ClickRequest, DragRequest, MouseAnchor, MouseButtonMode, MouseButtonName, MouseButtonRequest,
    MouseCoordinateSpace, MouseEndpoint, MouseMoveRequest, MousePoint, MouseRefTarget,
    MouseSelectorTarget, WheelRequest, DEFAULT_MOUSE_CLICK_HOLD_MS,
    DEFAULT_MOUSE_CLICK_INTERVAL_MS, DEFAULT_MOUSE_DRAG_DURATION_MS, DEFAULT_MOUSE_DRAG_STEPS,
};
pub use target::{
    prepare_click_request, prepare_drag_request, prepare_mouse_move_request, prepare_wheel_request,
    PreparedMouseRequest,
};

#[cfg(test)]
pub(crate) use plan::interpolate_point;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod target_tests;
