use crate::control_frames::{ControlExecutionOutcome, ControlFrame};
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

/// 组合 observation producer 与 response renderer。
///
/// `@observe` 的 savefile frame 必须先于最终 response line 发出,
/// 这里保留统一出口,避免各 section 自己决定 frame 顺序。
pub fn build_observe_outcome(
    request_id: Option<u64>,
    request: &ObserveRequest,
) -> io::Result<ControlExecutionOutcome> {
    let produced = producer::produce_observe_sections(request_id, request)?;
    let response = response::render_observe_response(request_id, request, produced)?;
    let mut outbound_frames = response.savefile_frames;
    outbound_frames.push(ControlFrame::ResponseLine(response.response_line));
    Ok(ControlExecutionOutcome { outbound_frames })
}

#[cfg(test)]
use producer::{select_primary_observation, ProducedSections};
#[cfg(test)]
use response::render_observe_response;

#[cfg(test)]
use crate::control_ax::{AxElement, AxSnapshot};
#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
#[path = "observe_tests.rs"]
mod tests;
