use crate::{
    control_ax::{AxElement, AxSnapshot},
    control_observation::ObservationHeader,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::io;

#[derive(Debug, Clone, Serialize)]
struct RefSample {
    section: &'static str,
    observation_id: String,
    #[serde(rename = "ref")]
    ref_id: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

pub(super) fn collect_ref_samples(
    accessibility: Option<&AxSnapshot>,
    windows: Option<&Value>,
    limit: usize,
) -> io::Result<Value> {
    let mut samples = Vec::<RefSample>::new();
    if let Some(snapshot) = accessibility {
        if let Some(observation) = snapshot.observation.as_ref() {
            for window in &snapshot.windows {
                if samples.len() >= limit {
                    break;
                }
                if let Some(ref_id) = window.ref_id.as_ref() {
                    samples.push(RefSample {
                        section: "accessibility",
                        observation_id: observation.observation_id.clone(),
                        ref_id: ref_id.clone(),
                        kind: "ax-window".to_owned(),
                        name: window.title.clone(),
                    });
                }
                collect_ax_element_ref_samples(observation, &window.elements, limit, &mut samples);
            }
        }
    }
    collect_window_ref_samples(windows, limit, &mut samples)?;
    let count = samples.len();
    let sample = serde_json::to_value(samples)
        .map_err(|err| io::Error::other(format!("observe refs 序列化失败: {err}")))?;
    Ok(json!({"count": count, "sample": sample}))
}

fn collect_ax_element_ref_samples(
    observation: &ObservationHeader,
    elements: &[AxElement],
    limit: usize,
    samples: &mut Vec<RefSample>,
) {
    for element in elements {
        if samples.len() >= limit {
            return;
        }
        if let Some(ref_id) = element.ref_id.as_ref() {
            samples.push(RefSample {
                section: "accessibility",
                observation_id: observation.observation_id.clone(),
                ref_id: ref_id.clone(),
                kind: "ax-element".to_owned(),
                name: element.name.clone(),
            });
        }
        collect_ax_element_ref_samples(observation, &element.children, limit, samples);
    }
}

fn collect_window_ref_samples(
    windows: Option<&Value>,
    limit: usize,
    samples: &mut Vec<RefSample>,
) -> io::Result<()> {
    let Some(windows) = windows else {
        return Ok(());
    };
    let observation_id = windows
        .get("observation")
        .and_then(|observation| observation.get("observation_id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(observation_id) = observation_id else {
        return Ok(());
    };
    let Some(items) = windows.get("items").and_then(Value::as_array) else {
        return Ok(());
    };
    for item in items {
        if samples.len() >= limit {
            break;
        }
        if let Some(ref_id) = item.get("ref").and_then(Value::as_str) {
            samples.push(RefSample {
                section: "windows",
                observation_id: observation_id.clone(),
                ref_id: ref_id.to_owned(),
                kind: "window".to_owned(),
                name: item.get("title").and_then(Value::as_str).map(str::to_owned),
            });
        }
    }
    Ok(())
}

pub(super) fn selector_count(
    primary: Option<&ObservationHeader>,
    window: Option<&ObservationHeader>,
    accessibility: Option<&AxSnapshot>,
) -> usize {
    let mut count = primary
        .map(|observation| observation.selector_count)
        .unwrap_or(0);
    if let Some(window) = window {
        if primary.map(|observation| observation.observation_id.as_str())
            != Some(window.observation_id.as_str())
        {
            count = count.saturating_add(window.selector_count);
        }
    }
    if let Some(ax_observation) = accessibility.and_then(|snapshot| snapshot.observation.as_ref()) {
        if primary.map(|observation| observation.observation_id.as_str())
            != Some(ax_observation.observation_id.as_str())
        {
            count = count.saturating_add(ax_observation.selector_count);
        }
    }
    count
}
