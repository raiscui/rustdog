use std::{collections::{BTreeMap, BTreeSet}, fs::File, io::{self, IsTerminal, Read}, path::{Path, PathBuf}};
use serde::Deserialize;
use super::{replay_normalize::{Action, Segment}, replay_source::{read_source, ReplayFact, ReplaySource}};
use crate::{control_frames::ControlFrame, zenoh_control::RecordingZenohBridge};

pub(crate) struct ReplayOptions {
    pub inspect: bool,
    pub plan_path: Option<PathBuf>,
    pub namespace: Option<String>,
    pub target_name: Option<String>,
    pub entry_point: Vec<String>,
    pub paths: Vec<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: String,
    sources: Vec<Scope>,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    sha256: String,
    first_seq: u64,
    last_seq: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    sha256: String,
    journal_seq: u64,
    intent: String,
    parameter: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    #[serde(deserialize_with = "unique_map")]
    bindings: BTreeMap<String, Binding>,
    #[serde(default, deserialize_with = "unique_map")]
    pointer_bindings: BTreeMap<String, PointerBinding>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    pid: u32,
    root_hwnd: u64,
    target: crate::control_flow::FlowTextTarget,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerBinding {
    pid: u32,
    root_hwnd: u64,
    points: Vec<crate::control_flow::FlowMousePoint>,
    verify: crate::control_flow::FlowMouseVerification,
}

fn unique_map<'de, D, T>(deserializer: D) -> Result<BTreeMap<String, T>, D::Error>
where D: serde::Deserializer<'de>, T: Deserialize<'de> {
    struct Visitor<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
        type Value = BTreeMap<String, T>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("对象") }
        fn visit_map<M: serde::de::MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((key, value)) = access.next_entry()? {
                if result.insert(key, value).is_some() { return Err(serde::de::Error::custom("重复绑定")); }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Visitor(std::marker::PhantomData))
}

fn invalid() -> String { "录制、计划或运行参数无效".to_owned() }
fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path).map_err(|_| invalid())?.take(limit + 1).read_to_end(&mut bytes).map_err(|_| invalid())?;
    if bytes.len() as u64 > limit { return Err(invalid()); }
    Ok(bytes)
}

fn negotiate_replay_capabilities(
    mut send: impl FnMut(&str) -> io::Result<Vec<ControlFrame>>,
) -> Result<(), String> {
    let unsupported = || "目标未声明受保护回放契约, 动作未派发".to_owned();
    let frames = send("@capabilities").map_err(|_| unsupported())?;
    let [ControlFrame::ResponseLine(line)] = frames.as_slice() else { return Err(unsupported()); };
    let report: serde_json::Value = line.strip_prefix("@response ")
        .and_then(|payload| serde_json::from_str(payload).ok()).ok_or_else(unsupported)?;
    let replay = report.pointer("/capabilities/replay").ok_or_else(unsupported)?;
    if report.get("kind").and_then(|value| value.as_str()) != Some("capabilities")
        || report.get("schema").and_then(|value| value.as_str()) != Some(crate::control_capabilities::CAPABILITIES_SCHEMA)
        || report.pointer("/platform/os").and_then(|value| value.as_str()) != Some("windows")
        || replay.get("status").and_then(|value| value.as_str()) != Some("available")
        || replay.get("flow_contract").and_then(|value| value.as_str()) != Some(crate::control_capabilities::REPLAY_FLOW_CONTRACT)
        || ["protected_parameter", "identity_projection", "guarded_pointer"].iter()
            .any(|field| replay.get(*field).and_then(|value| value.as_bool()) != Some(true))
    { return Err(unsupported()); }
    Ok(())
}

fn fresh_window(bridge: &mut RecordingZenohBridge, pid: u32, root_hwnd: u64) -> Result<(String, String, crate::control_flow::FlowWindowRect, Option<u64>), String> {
    let frames = bridge.send_recording_line(&format!("@window-find:{{pid:{pid},limit:256,include_state:true,include_recipes:false,identity_only:true}}"))
        .map_err(|_| "目标观察失败, 动作未派发".to_owned())?;
    let [ControlFrame::ResponseLine(line)] = frames.as_slice() else { return Err("目标观察失败, 动作未派发".to_owned()); };
    let response: serde_json::Value = line.strip_prefix("@response ")
        .and_then(|text| serde_json::from_str(text).ok())
        .ok_or_else(|| "目标观察失败, 动作未派发".to_owned())?;
    if response.get("status").and_then(|v| v.as_str()) != Some("complete")
        || response.get("platform").and_then(|v| v.as_str()) != Some("windows")
        || response.get("match_count").and_then(|v| v.as_u64()).is_none()
        || response.get("match_count") != response.get("returned_count")
    { return Err("目标观察失败, 动作未派发".to_owned()); }
    let id = format!("pid:{pid}/hwnd:0x{root_hwnd:X}");
    let matches = response.get("matches").and_then(|v| v.as_array()).ok_or_else(invalid)?;
    if response.get("returned_count").and_then(|v| v.as_u64()) != Some(matches.len() as u64) {
        return Err("目标观察失败, 动作未派发".to_owned());
    }
    let mut matching = matches.iter().filter(|candidate| candidate.get("window_id").and_then(|v| v.as_str()) == Some(&id));
    let candidate = matching.next().ok_or_else(|| "目标窗口不存在, 动作未派发".to_owned())?;
    if matching.next().is_some() || candidate.pointer("/app/frontmost").and_then(|v| v.as_bool()) != Some(true) {
        return Err("目标窗口不明确或不在前台, 动作未派发".to_owned());
    }
    let observation = response.pointer("/observation/observation_id").and_then(|v| v.as_str()).ok_or_else(invalid)?;
    let reference = candidate.get("ref").and_then(|v| v.as_str()).ok_or_else(invalid)?;
    if candidate.get("coordinate_space").and_then(|v| v.as_str()) != Some("os-logical") { return Err(invalid()); }
    let rect = serde_json::from_value(candidate.get("rect").cloned().ok_or_else(invalid)?).map_err(|_| invalid())?;
    Ok((observation.to_owned(), reference.to_owned(), rect, candidate.get("focused_hwnd").and_then(|value| value.as_u64())))
}

fn pointer_action(segment: &Segment, binding: &PointerBinding) -> Result<crate::control_flow::FlowMouseAction, String> {
    use crate::control_flow::FlowMouseAction;
    match (&segment.action, binding.points.as_slice()) {
        (Action::Move { .. }, [point]) if point.offset_ns == 0 => Ok(FlowMouseAction::Move { point: point.clone() }),
        (Action::Click { button, hold_ns, .. }, [point]) if point.offset_ns == 0 =>
            Ok(FlowMouseAction::Click { point: point.clone(), button: *button, hold_ns: *hold_ns }),
        (Action::Drag { button, points: recorded, .. }, points)
            if recorded.len() == points.len() && recorded.iter().zip(points).all(|(a, b)| a.offset_ns == b.offset_ns) =>
            Ok(FlowMouseAction::Drag { points: points.to_vec(), button: *button }),
        (Action::Wheel { delta_x, delta_y, .. }, [point]) if point.offset_ns == 0 =>
            Ok(FlowMouseAction::Wheel { point: point.clone(), delta_x: *delta_x, delta_y: *delta_y }),
        _ => Err(invalid()),
    }
}

fn expected_intent(segment: &Segment) -> Option<&'static str> {
    match &segment.action {
        Action::UnknownInput { .. } => Some("confirmed_text"),
        Action::Move { .. } => Some("position_pointer"),
        Action::Click { .. } => Some("confirmed_click"),
        Action::Drag { .. } => Some("confirmed_drag"),
        Action::Wheel { .. } => Some("confirmed_wheel"),
        Action::Unresolved { .. } => None,
    }
}

fn focus_already_satisfied(intent: &str, verify: &crate::control_flow::FlowMouseVerification, focused: Option<u64>) -> bool {
    matches!((intent, verify), ("focus_control", crate::control_flow::FlowMouseVerification::Focus { control_hwnd }) if focused == Some(*control_hwnd))
}

pub(crate) fn run(options: ReplayOptions) -> Result<(), String> {
    let ReplayOptions { inspect, plan_path, namespace, target_name, entry_point, paths } = options;
    let mut sources = BTreeMap::new();
    for path in paths {
        let source = read_source(&path).map_err(|_| invalid())?;
        if sources.insert(source.sha256.clone(), (path, source)).is_some() { return Err(invalid()); }
    }
    if inspect {
        if plan_path.is_some() { return Err(invalid()); }
        let values: Vec<&ReplaySource> = sources.values().map(|(_, source)| source).collect();
        println!("{}", serde_json::to_string(&values).map_err(|_| invalid())?);
        return Ok(());
    }
    let plan_path = plan_path.ok_or_else(invalid)?;
    let plan: Plan = serde_json::from_slice(&read_limited(&plan_path, 1_048_576)?).map_err(|_| invalid())?;
    if plan.schema != "rdog.replay-plan.v1" || plan.sources.is_empty() || plan.steps.is_empty() || plan.sources.len() != sources.len() { return Err(invalid()); }
    let mut scopes = BTreeMap::new();
    for scope in &plan.sources {
        if scope.first_seq > scope.last_seq || !sources.contains_key(&scope.sha256)
            || scopes.insert(scope.sha256.as_str(), scope).is_some() { return Err(invalid()); }
    }
    let mut required = BTreeMap::new();
    for (digest, (_, source)) in &sources {
        let scope = scopes.get(digest.as_str()).ok_or_else(invalid)?;
        let contains_seq = |needle| source.facts.iter().any(|fact| match fact {
            ReplayFact::Capture { journal_seq, .. } | ReplayFact::Mark { journal_seq, .. } => *journal_seq == needle,
        });
        if !contains_seq(scope.first_seq) || !contains_seq(scope.last_seq) { return Err(invalid()); }
        for segment in &source.segments {
            let overlaps = segment.source_seqs.iter().any(|seq| (scope.first_seq..=scope.last_seq).contains(seq));
            if !overlaps { continue; }
            if segment.source_seqs.iter().any(|seq| !(scope.first_seq..=scope.last_seq).contains(seq))
                || expected_intent(segment).is_none() { return Err("来源范围包含未解析或不完整动作".to_owned()); }
            required.insert((digest.as_str(), segment.source_seqs[0]), segment);
        }
    }
    let mut seen = BTreeSet::new();
    let mut text_parameters = BTreeSet::new();
    let mut pointer_parameters = BTreeSet::new();
    let mut last_seq = BTreeMap::new();
    for step in &plan.steps {
        let key = (step.sha256.as_str(), step.journal_seq);
        let segment = required.get(&key).ok_or_else(invalid)?;
        if !(expected_intent(segment) == Some(step.intent.as_str())
            || matches!(segment.action, Action::Click { .. }) && step.intent == "focus_control")
            || step.parameter.is_empty() || !seen.insert(key)
            || last_seq.get(step.sha256.as_str()).is_some_and(|last| *last >= step.journal_seq) { return Err(invalid()); }
        last_seq.insert(step.sha256.as_str(), *segment.source_seqs.last().ok_or_else(invalid)?);
        let parameters = if step.intent == "confirmed_text" { &mut text_parameters } else { &mut pointer_parameters };
        if !parameters.insert(step.parameter.as_str()) { return Err(invalid()); }
    }
    if seen.len() != required.len() || !text_parameters.is_disjoint(&pointer_parameters) { return Err(invalid()); }
    if io::stdin().is_terminal() { return Err("运行参数必须通过受保护的标准输入管道提供".to_owned()); }
    let mut raw = Vec::new();
    io::stdin().take(1_048_577).read_to_end(&mut raw).map_err(|_| invalid())?;
    if raw.len() > 1_048_576 { return Err(invalid()); }
    let runtime: Runtime = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    if runtime.bindings.len() != text_parameters.len() || runtime.pointer_bindings.len() != pointer_parameters.len()
        || !text_parameters.iter().all(|key| runtime.bindings.contains_key(*key))
        || !pointer_parameters.iter().all(|key| runtime.pointer_bindings.contains_key(*key))
        || runtime.bindings.values().any(|binding| binding.value.contains('\0')
            || binding.value.len() > 262_144 || binding.pid == 0 || binding.pid > i32::MAX as u32
            || binding.root_hwnd == 0 || !binding.target.valid())
        || runtime.pointer_bindings.values().any(|binding| binding.pid == 0 || binding.pid > i32::MAX as u32 || binding.root_hwnd == 0)
    { return Err(invalid()); }
    for step in &plan.steps {
        if let Some(binding) = runtime.pointer_bindings.get(&step.parameter) {
            let segment = required.get(&(step.sha256.as_str(), step.journal_seq)).ok_or_else(invalid)?;
            if (step.intent == "focus_control") != matches!(binding.verify, crate::control_flow::FlowMouseVerification::Focus { .. }) {
                return Err("动作意图与验证不一致".to_owned());
            }
            let action = pointer_action(segment, binding)?;
            crate::control_mouse::validate_action(&action, &binding.verify)
                .map_err(|_| "动作缺少适用的目标结果验证".to_owned())?;
        }
    }
    if target_name.is_none() || entry_point.is_empty() { return Err("回放需要已配置认证和 TLS 的明确目标".to_owned()); }
    let mut bridge = RecordingZenohBridge::open(namespace, target_name, entry_point).map_err(|_| "安全连接不可用".to_owned())?;
    negotiate_replay_capabilities(|line| bridge.send_recording_line(line))?;
    let mut target_rects = BTreeMap::new();
    let targets: BTreeSet<_> = runtime.bindings.values().map(|binding| (binding.pid, binding.root_hwnd))
        .chain(runtime.pointer_bindings.values().map(|binding| (binding.pid, binding.root_hwnd))).collect();
    for (pid, root_hwnd) in targets {
        let (_, _, rect, _) = fresh_window(&mut bridge, pid, root_hwnd)?;
        target_rects.insert((pid, root_hwnd), rect);
    }
    for step in &plan.steps {
        if let Some(binding) = runtime.pointer_bindings.get(&step.parameter) {
            let segment = required.get(&(step.sha256.as_str(), step.journal_seq)).ok_or_else(invalid)?;
            let action = pointer_action(segment, binding)?;
            let rect = target_rects.get(&(binding.pid, binding.root_hwnd)).ok_or_else(invalid)?;
            crate::control_mouse::validate_points_in_rect(&action, rect)
                .map_err(|_| "目标点不在当前窗口内, 动作未派发".to_owned())?;
        }
    }
    let mut completed = 0usize;
    let outcome = (|| -> Result<(), String> {
    for step in &plan.steps {
        let (path, _) = sources.get(&step.sha256).ok_or_else(invalid)?;
        if read_source(path).map_err(|_| invalid())?.sha256 != step.sha256 { return Err("录制源已变化".to_owned()); }
        let payload = if let Some(binding) = runtime.bindings.get(&step.parameter) {
            let (observation_id, window_ref, _, _) = fresh_window(&mut bridge, binding.pid, binding.root_hwnd)?;
            serde_json::json!({"schema":"rdog.flow.v1","steps":[{"TextParameter":{"parameter":step.parameter,"pid":binding.pid,"root_hwnd":binding.root_hwnd,"target":binding.target,"observation_id":observation_id,"window_ref":window_ref}}],"bindings":{step.parameter.clone():binding.value}})
        } else {
            let binding = runtime.pointer_bindings.get(&step.parameter).ok_or_else(invalid)?;
            let segment = required.get(&(step.sha256.as_str(), step.journal_seq)).ok_or_else(invalid)?;
            let (observation_id, window_ref, rect, focused_hwnd) = fresh_window(&mut bridge, binding.pid, binding.root_hwnd)?;
            if focus_already_satisfied(&step.intent, &binding.verify, focused_hwnd) {
                println!("{}", serde_json::json!({"source_sha256":step.sha256,"journal_seq":step.journal_seq,"source_seqs":segment.source_seqs,"status":"already_satisfied","dispatch":"not_dispatched"}));
                completed += 1;
                continue;
            }
            let guarded = crate::control_flow::FlowGuardedMouseStep {
                pid: binding.pid, root_hwnd: binding.root_hwnd, observation_id, window_ref, rect,
                action: pointer_action(segment, binding)?, verify: binding.verify.clone(),
            };
            serde_json::json!({"schema":"rdog.flow.v1","steps":[{"GuardedMouse":guarded}]})
        };
        let frames = bridge.send_recording_line(&format!("@flow:{payload}")).map_err(|_| "动作结果未知, 不要重试".to_owned())?;
        let [ControlFrame::ResponseLine(line)] = frames.as_slice() else { return Err("动作结果未知, 不要重试".to_owned()); };
        let response: serde_json::Value = line.strip_prefix("@response ").and_then(|text| serde_json::from_str(text).ok()).ok_or_else(|| "动作结果未知, 不要重试".to_owned())?;
        if response.get("code").and_then(|v| v.as_i64()) == Some(403)
            || response.pointer("/value/failed_step/message").and_then(|v| v.as_str()) == Some("rejected_before_dispatch")
        {
            return Err("动作未派发, 目标或连接校验失败".to_owned());
        }
        if response.pointer("/value/status").and_then(|v| v.as_str()) != Some("ok") || response.pointer("/value/completed_steps").and_then(|v| v.as_u64()) != Some(1) { return Err("动作结果未知, 不要重试".to_owned()); }
        let segment = required.get(&(step.sha256.as_str(), step.journal_seq)).ok_or_else(invalid)?;
        println!("{}", serde_json::json!({"source_sha256":step.sha256,"journal_seq":step.journal_seq,"source_seqs":segment.source_seqs,"status":"verified"}));
        completed += 1;
    }
    Ok(())
    })();
    outcome.map_err(|error| if completed == 0 { error } else { format!("partial: 已完成部分步骤; {error}") })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_flow::{FlowMousePoint, FlowMouseVerification};
    use crate::control_recording::{model::WindowIdentity, replay_normalize::Point};

    #[test]
    fn compiles_drag_against_fresh_points_with_original_timing() {
        let window = WindowIdentity { pid: 1, hwnd: 2, process_start: 3, generation: 4 };
        let segment = Segment { source_seqs: vec![4, 5, 6], action: Action::Drag {
            window, button: 1, duration_ns: 10,
            points: vec![Point { x: 1, y: 2, offset_ns: 0 }, Point { x: 2, y: 3, offset_ns: 10 }],
        } };
        let mut binding = PointerBinding { pid: 7, root_hwnd: 8,
            points: vec![FlowMousePoint { x: 101, y: 102, offset_ns: 0 }, FlowMousePoint { x: 105, y: 106, offset_ns: 10 }],
            verify: FlowMouseVerification::Selection { control_hwnd: 9, start: 0, end: 2 },
        };
        assert!(matches!(pointer_action(&segment, &binding).unwrap(),
            crate::control_flow::FlowMouseAction::Drag { points, .. } if points[0].x == 101 && points[1].offset_ns == 10));
        binding.points[1].offset_ns = 11;
        assert!(pointer_action(&segment, &binding).is_err());
    }

    #[test]
    fn rejects_duplicate_runtime_bindings_without_echo() {
        let raw = r#"{"bindings":{"p":{"pid":1,"root_hwnd":2,"target":{"kind":"focused_control","control_type":50030},"value":"secret-canary"},"p":{"pid":1,"root_hwnd":2,"target":{"kind":"focused_control","control_type":50030},"value":"secret-canary"}}}"#;
        let error = serde_json::from_str::<Runtime>(raw).err().expect("必须拒绝重复绑定");
        assert!(!error.to_string().contains("secret-canary"));
    }

    #[test]
    fn focus_goal_is_idempotent_only_for_the_explicit_current_control() {
        let verify = FlowMouseVerification::Focus { control_hwnd: 9 };
        assert!(focus_already_satisfied("focus_control", &verify, Some(9)));
        assert!(!focus_already_satisfied("focus_control", &verify, Some(10)));
        assert!(!focus_already_satisfied("focus_control", &verify, None));
        assert!(!focus_already_satisfied("confirmed_click", &verify, Some(9)));
    }

    #[test]
    fn old_capability_report_sends_no_binding_or_mutation() {
        let mut sent = Vec::new();
        let result = negotiate_replay_capabilities(|line| {
            sent.push(line.to_owned());
            Ok(vec![ControlFrame::ResponseLine(
                r#"@response {"kind":"capabilities","schema":"rdog.capabilities.v1","platform":{"os":"windows"},"capabilities":{}}"#.into(),
            )])
        });
        assert!(result.is_err());
        assert_eq!(sent, ["@capabilities"]);
    }

    #[test]
    fn replay_capability_requires_every_protection() {
        let report = serde_json::json!({
            "kind":"capabilities", "schema":"rdog.capabilities.v1", "platform":{"os":"windows"},
            "capabilities":{"replay":{"status":"available","flow_contract":crate::control_capabilities::REPLAY_FLOW_CONTRACT,
                "protected_parameter":true,"identity_projection":true,"guarded_pointer":true}}
        });
        for field in ["protected_parameter", "identity_projection", "guarded_pointer"] {
            let mut changed = report.clone();
            changed["capabilities"]["replay"][field] = false.into();
            let response = format!("@response {changed}");
            assert!(negotiate_replay_capabilities(|_| Ok(vec![ControlFrame::ResponseLine(response.clone())])).is_err());
        }
        let response = format!("@response {report}");
        assert!(negotiate_replay_capabilities(|_| Ok(vec![ControlFrame::ResponseLine(response.clone())])).is_ok());
    }
}
