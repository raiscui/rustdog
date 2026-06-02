use super::*;

#[test]
fn parse_should_accept_computer_use_density_bench_request() {
    let request = parse_gui_bench_payload(
        r#"{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level",write_artifact:true}"#,
    )
    .unwrap();

    assert_eq!(
        request,
        GuiBenchRequest {
            suite: "computer-use-density".to_owned(),
            case_name: "xhs-left-nav-home".to_owned(),
            variant: "baseline-low-level".to_owned(),
            runner: GuiBenchRunner::Fixture,
            allow_side_effects: false,
            write_artifact: true,
        }
    );
}

#[test]
fn parse_should_accept_live_runner_only_with_explicit_fields() {
    let request = parse_gui_bench_payload(
        r#"{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"dense-web-act",runner:"live",allow_side_effects:true}"#,
    )
    .unwrap();

    assert_eq!(request.runner, GuiBenchRunner::Live);
    assert!(request.allow_side_effects);
    assert_eq!(request.variant, "dense-web-act");
}

#[test]
fn parse_should_reject_missing_or_unknown_fields() {
    assert!(parse_gui_bench_payload(r#"{suite:"computer-use-density"}"#).is_err());
    assert!(
        parse_gui_bench_payload(
            r#"{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level",mode:"live"}"#
        )
        .is_err()
    );
    assert!(parse_gui_bench_payload(
        r#"{suite:"computer-use-density",case:"xhs-left-nav-home",variant:""}"#
    )
    .is_err());
    assert!(parse_gui_bench_payload(
        r#"{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"dense-web-act",runner:"recorded"}"#
    )
    .is_err());
}

#[test]
fn response_should_report_baseline_density_gap_without_command_failure() {
    let request = fixture_request("baseline-low-level");

    let json = build_gui_bench_response_json(&request).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["kind"].as_str(), Some("gui-bench"));
    assert_eq!(value["schema"].as_str(), Some(GUI_BENCH_SCHEMA));
    assert_eq!(value["status"].as_str(), Some("complete"));
    assert_eq!(value["runner"].as_str(), Some("fixture"));
    assert_eq!(value["variant_count"].as_u64(), Some(1));
    assert_eq!(value["dense_target_passed"].as_bool(), Some(false));
    assert_eq!(value["metrics"]["backend_request_count"].as_u64(), Some(8));
    assert_eq!(
        value["thresholds"]["max_backend_request_count"].as_u64(),
        Some(2)
    );
    assert!(value["threshold_failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|failure| failure.as_str() == Some("baseline-low-level:backend_request_count")));
    assert!(value["threshold_failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|failure| failure.as_str() == Some("baseline-low-level:agent_decision_points")));
    assert_eq!(value["steps_summary"]["step_count"].as_u64(), Some(8));
    assert_eq!(
        value["steps_summary"]["mouse_fallback_commands"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(value["runs"].as_array().unwrap().len(), 1);
}

#[test]
fn response_should_report_dense_web_act_as_passing_variant() {
    let request = fixture_request("dense-web-act");

    let json = build_gui_bench_response_json(&request).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["dense_target_passed"].as_bool(), Some(true));
    assert_eq!(value["metrics"]["backend_request_count"].as_u64(), Some(1));
    assert_eq!(value["metrics"]["semantic_action_count"].as_u64(), Some(1));
    assert_eq!(
        value["metrics"]["stale_ref_recovery_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        value["steps_summary"]["semantic_action_commands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn response_should_compare_all_variants() {
    let request = fixture_request("all");

    let json = build_gui_bench_response_json(&request).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["variant"].as_str(), Some("all"));
    assert_eq!(value["variant_count"].as_u64(), Some(3));
    assert_eq!(value["dense_target_passed"].as_bool(), Some(false));
    assert!(value.get("metrics").is_none());
    assert_eq!(value["runs"].as_array().unwrap().len(), 3);
    assert!(value["threshold_failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|failure| failure.as_str() == Some("baseline-low-level:backend_request_count")));
}

#[test]
fn response_should_reject_unknown_phase_3a_fixture() {
    let mut request = fixture_request("baseline-low-level");
    request.case_name = "unknown".to_owned();

    let err = build_gui_bench_response_json(&request).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("暂不支持 case"));
}

#[test]
fn response_should_write_artifact_only_when_requested() {
    let mut request = fixture_request("dense-web-find");
    request.write_artifact = true;

    let json = build_gui_bench_response_json(&request).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let path = value["artifact"]["path"]
        .as_str()
        .expect("artifact path should exist when requested");

    assert!(path.ends_with("computer-use-density__xhs-left-nav-home__dense-web-find.json"));
    let artifact_text = std::fs::read_to_string(path).unwrap();
    let artifact: serde_json::Value = serde_json::from_str(&artifact_text).unwrap();
    assert_eq!(artifact["artifact"]["path"].as_str(), Some(path));
    assert_eq!(artifact["variant"].as_str(), Some("dense-web-find"));

    let _ = std::fs::remove_file(path);
}

#[test]
fn live_runner_should_require_side_effect_consent() {
    let mut request = fixture_request("dense-web-act");
    request.runner = GuiBenchRunner::Live;

    let err = build_gui_bench_response_json(&request).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("allow_side_effects:true"));
}

#[test]
fn live_runner_should_reject_all_variant() {
    let mut request = fixture_request("all");
    request.runner = GuiBenchRunner::Live;
    request.allow_side_effects = true;

    let err = build_gui_bench_response_json(&request).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("variant:\"all\""));
}

#[test]
fn live_runner_should_replay_dense_web_act_with_stubbed_builder() {
    let mut request = fixture_request("dense-web-act");
    request.runner = GuiBenchRunner::Live;
    request.allow_side_effects = true;

    let json = build_live_gui_bench_response_json_with(
        &request,
        |_| panic!("dense-web-act should not call web-find builder"),
        |web_act| {
            assert_eq!(web_act.find.query.text, "首页");
            assert!(web_act.verify);
            Ok(r#"{"kind":"web-act","schema":"rdog.web-act.v1","status":"complete","performed":true,"verified":true,"match_count":1,"returned_count":1}"#.to_owned())
        },
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["runner"].as_str(), Some("live"));
    assert_eq!(value["allow_side_effects"].as_bool(), Some(true));
    assert_eq!(value["status"].as_str(), Some("complete"));
    assert_eq!(value["dense_target_passed"].as_bool(), Some(true));
    assert_eq!(
        value["runs"][0]["live_replay"]["response_kind"].as_str(),
        Some("web-act")
    );
    assert_eq!(
        value["runs"][0]["live_replay"]["performed"].as_bool(),
        Some(true)
    );
    assert_eq!(
        value["runs"][0]["live_replay"]["verified"].as_bool(),
        Some(true)
    );
}

#[test]
fn live_runner_should_mark_failed_replay_without_claiming_success() {
    let mut request = fixture_request("dense-web-act");
    request.runner = GuiBenchRunner::Live;
    request.allow_side_effects = true;

    let json = build_live_gui_bench_response_json_with(
        &request,
        |_| panic!("dense-web-act should not call web-find builder"),
        |_| {
            Ok(r#"{"kind":"web-act","schema":"rdog.web-act.v1","status":"verification_failed","performed":true,"verified":false,"match_count":0,"returned_count":0,"error_code":"WEB_ACTION_VERIFICATION_FAILED"}"#.to_owned())
        },
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["runner"].as_str(), Some("live"));
    assert_eq!(value["status"].as_str(), Some("threshold_failed"));
    assert_eq!(value["dense_target_passed"].as_bool(), Some(false));
    assert!(value["threshold_failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|failure| failure.as_str() == Some("dense-web-act:live_replay")));
    assert_eq!(
        value["runs"][0]["live_replay"]["verified"].as_bool(),
        Some(false)
    );
}

fn fixture_request(variant: &str) -> GuiBenchRequest {
    GuiBenchRequest {
        suite: "computer-use-density".to_owned(),
        case_name: "xhs-left-nav-home".to_owned(),
        variant: variant.to_owned(),
        runner: GuiBenchRunner::Fixture,
        allow_side_effects: false,
        write_artifact: false,
    }
}
