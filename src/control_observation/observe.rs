use crate::control_frames::{ControlExecutionOutcome, ControlFrame};
use serde_json::Value;
use std::io;

#[path = "observe/producer.rs"]
mod producer;
#[path = "observe/refs.rs"]
mod refs;
#[path = "observe/request.rs"]
mod request;
#[path = "observe/response.rs"]
mod response;

pub use request::{parse_observe_payload, ObserveRequest};
#[cfg(test)]
pub(crate) use request::{ObserveMode, ObserveTarget};

const OBSERVE_SCHEMA: &str = "rdog.observe.v1";

/// `@observe` 的结构化产物。
///
/// 后续 `@bootstrap` 这类组合命令应该消费这个外壳:
/// - `savefile_frames` 保持先发 frame 的顺序语义
/// - `value` 是已经组装好的 observe lane,无需反解析 `@response`
#[derive(Debug, Clone, PartialEq)]
pub struct ObserveBundle {
    pub savefile_frames: Vec<ControlFrame>,
    pub value: Value,
}

pub fn build_observe_bundle(
    request_id: Option<u64>,
    request: &ObserveRequest,
) -> io::Result<ObserveBundle> {
    let produced = producer::produce_observe_sections(request_id, request)?;
    response::build_observe_bundle_from_sections(request, produced)
}

/// 组合 observation producer 与 response renderer。
///
/// `@observe` 的 savefile frame 必须先于最终 response line 发出,
/// 这里保留统一出口,避免各 section 自己决定 frame 顺序。
pub fn build_observe_outcome(
    request_id: Option<u64>,
    request: &ObserveRequest,
) -> io::Result<ControlExecutionOutcome> {
    let bundle = build_observe_bundle(request_id, request)?;
    let response_line = response::render_observe_bundle_response_line(request_id, &bundle.value)?;
    let mut outbound_frames = bundle.savefile_frames;
    outbound_frames.push(ControlFrame::ResponseLine(response_line));
    Ok(ControlExecutionOutcome { outbound_frames })
}

#[cfg(test)]
use producer::{select_primary_observation, ProducedSections};
#[cfg(test)]
use response::{build_observe_bundle_from_sections, render_observe_response};

#[cfg(test)]
use crate::control_ax::{AxElement, AxSnapshot};

#[cfg(test)]
#[path = "observe_tests.rs"]
mod tests;
