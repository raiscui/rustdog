use serde_json::Value;

const XHS_LEFT_NAV_HOME_BASELINE: &str =
    include_str!("fixtures/computer_use_density/xhs_left_nav_home_baseline.json");

#[test]
fn xhs_left_nav_home_baseline_density_metrics_are_consistent() {
    let case = load_case(XHS_LEFT_NAV_HOME_BASELINE);
    assert_eq!(
        case["schema"].as_str(),
        Some("rdog.computer-use-density.bench.v1")
    );
    assert_eq!(case["suite"].as_str(), Some("computer-use-density"));
    assert_eq!(case["case"].as_str(), Some("xhs-left-nav-home"));

    let dense_target = &case["dense_target"];
    assert_eq!(dense_target["max_backend_request_count"].as_u64(), Some(2));
    assert_eq!(dense_target["max_agent_decision_points"].as_u64(), Some(1));
    assert_eq!(
        dense_target["required_scope"].as_str(),
        Some("active_web_area")
    );
    assert_eq!(dense_target["required_action"].as_str(), Some("AXPress"));

    let variant = variant_by_name(&case, "baseline-low-level");
    assert_eq!(variant["variant"].as_str(), Some("baseline-low-level"));

    let steps = variant["steps"]
        .as_array()
        .expect("baseline variant should contain steps");
    let metrics = &variant["metrics"];

    assert_eq!(
        metrics["backend_request_count"].as_u64(),
        Some(steps.len() as u64),
        "backend request count must track the low-level command sequence"
    );
    let control_frame_count = steps
        .iter()
        .map(|step| {
            step["expected_frame_count"]
                .as_u64()
                .expect("each step should record expected frame count")
        })
        .sum::<u64>();
    assert_eq!(
        metrics["control_frame_count"].as_u64(),
        Some(control_frame_count),
        "control frame count must account for multi-frame commands such as @screenshot"
    );

    let decision_points = steps
        .iter()
        .filter(|step| step["agent_decision_after"].as_bool() == Some(true))
        .count() as u64;
    assert_eq!(
        metrics["agent_decision_points"].as_u64(),
        Some(decision_points),
        "agent decision points must be derived from the recorded steps"
    );

    assert!(
        metrics["backend_request_count"].as_u64().unwrap()
            > dense_target["max_backend_request_count"].as_u64().unwrap(),
        "baseline must make the task-density gap visible"
    );
    assert!(
        metrics["agent_decision_points"].as_u64().unwrap()
            > dense_target["max_agent_decision_points"].as_u64().unwrap(),
        "baseline must make agent orchestration cost visible"
    );
}

#[test]
fn xhs_left_nav_home_baseline_uses_web_area_and_no_wrong_ax_target_shape() {
    let case = load_case(XHS_LEFT_NAV_HOME_BASELINE);
    let variant = variant_by_name(&case, "baseline-low-level");
    let steps = variant["steps"]
        .as_array()
        .expect("baseline variant should contain steps");
    let commands = steps
        .iter()
        .map(|step| {
            step["command"]
                .as_str()
                .expect("each step should contain a command")
        })
        .collect::<Vec<_>>();

    assert!(
        commands.iter().all(|command| command.starts_with('@')),
        "baseline commands must be line-control requests"
    );
    assert!(
        commands
            .iter()
            .all(|command| !command.contains("target:{window_id")),
        "`@ax-get` targets must use target.id, not target.window_id"
    );
    let ax_get_count = commands
        .iter()
        .filter(|command| command.starts_with("@ax-get"))
        .count();
    assert!(
        ax_get_count >= 3,
        "baseline should make repeated AX drill-down cost visible"
    );
    assert!(
        steps.iter().any(|step| {
            step["purpose"]
                .as_str()
                .is_some_and(|purpose| purpose.contains("AXWebArea"))
        }),
        "baseline should explicitly record the AXWebArea discovery step"
    );
    assert!(
        commands
            .iter()
            .any(|command| command.contains("path:0.0.0.0.1.0.0.0")),
        "baseline should record the known Chrome AXWebArea path shape"
    );
    assert!(
        commands
            .iter()
            .any(|command| command.contains("@ax-action") && command.contains("AXPress")),
        "baseline should prefer semantic AXPress over mouse fallback"
    );
    assert_eq!(
        variant["metrics"]["mouse_fallback_count"].as_u64(),
        Some(0),
        "phase-0 safe baseline should not bake in coordinate fallback as success"
    );
}

#[test]
fn xhs_left_nav_home_dense_variants_show_expected_request_density() {
    let case = load_case(XHS_LEFT_NAV_HOME_BASELINE);
    let dense_target = &case["dense_target"];
    let variants = case["variants"]
        .as_array()
        .expect("fixture should contain variants");
    assert_eq!(
        variants.len(),
        3,
        "bench runner all mode expects 3 variants"
    );

    let web_find = variant_by_name(&case, "dense-web-find");
    assert_eq!(
        web_find["metrics"]["backend_request_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        web_find["metrics"]["agent_decision_points"].as_u64(),
        Some(0)
    );
    assert_eq!(
        web_find["metrics"]["semantic_action_count"].as_u64(),
        Some(0)
    );
    assert!(
        web_find["metrics"]["backend_request_count"]
            .as_u64()
            .unwrap()
            <= dense_target["max_backend_request_count"].as_u64().unwrap()
    );
    assert!(variant_commands(web_find)
        .iter()
        .all(|command| command.starts_with("@web-find")));

    let web_act = variant_by_name(&case, "dense-web-act");
    assert_eq!(
        web_act["metrics"]["backend_request_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        web_act["metrics"]["semantic_action_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        web_act["metrics"]["stale_ref_recovery_count"].as_u64(),
        Some(1)
    );
    assert_eq!(web_act["metrics"]["mouse_fallback_count"].as_u64(), Some(0));
    assert!(variant_commands(web_act)
        .iter()
        .all(|command| command.starts_with("@web-act") && command.contains(r#"action:"press""#)));
}

fn load_case(text: &str) -> Value {
    serde_json::from_str(text).expect("computer-use density fixture should be valid JSON")
}

fn variant_by_name<'a>(case: &'a Value, variant_name: &str) -> &'a Value {
    case["variants"]
        .as_array()
        .and_then(|variants| {
            variants
                .iter()
                .find(|variant| variant["variant"].as_str() == Some(variant_name))
        })
        .expect("fixture should contain requested variant")
}

fn variant_commands(variant: &Value) -> Vec<&str> {
    variant["steps"]
        .as_array()
        .expect("variant should contain steps")
        .iter()
        .map(|step| {
            step["command"]
                .as_str()
                .expect("each step should contain command")
        })
        .collect()
}
