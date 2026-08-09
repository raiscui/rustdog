use super::*;
use crate::control_ax::{AxMode, AxRect, AxWindow};

#[test]
fn parse_observe_payload_should_apply_mode_defaults_and_overrides() {
    let default_request = parse_observe_payload("").unwrap();
    assert_eq!(default_request.mode, ObserveMode::Hybrid);
    assert!(default_request.include_screenshot);
    assert!(default_request.include_ax);
    assert!(default_request.include_windows);

    let request = parse_observe_payload(
        r#"{mode:"window",target:{app:"System Settings",window_title_contains:"储存"},limit:5,include_refs:false}"#,
    )
    .unwrap();
    assert_eq!(request.mode, ObserveMode::Window);
    assert!(!request.include_screenshot);
    assert!(!request.include_ax);
    assert!(request.include_windows);
    assert_eq!(request.limit, 5);
    assert!(!request.include_refs);
    assert_eq!(
        request
            .target
            .as_ref()
            .and_then(|target| target.app.as_deref()),
        Some("System Settings")
    );

    let skeleton_request = parse_observe_payload(r#"{mode:"ax",ax_mode:"skeleton"}"#).unwrap();
    assert_eq!(skeleton_request.mode, ObserveMode::Ax);
    assert_eq!(skeleton_request.ax_mode, AxMode::Windows);
}

#[test]
fn parse_observe_payload_should_reject_unknowns_and_duplicate_fields() {
    assert!(parse_observe_payload(r#"{mode:"desktop"}"#).is_err());
    assert!(parse_observe_payload(r#"{mode:"ax",mode:"window"}"#).is_err());
    assert!(parse_observe_payload(r#"{target:{}}"#).is_err());
    assert!(parse_observe_payload(r#"{unknown:true}"#).is_err());
}

#[test]
fn parse_observe_payload_should_accept_display_scope_selectors() {
    let request = parse_observe_payload(r#"{scope:{display:{id:"d2"}}}"#).unwrap();
    assert_eq!(
        request
            .display_scope
            .as_ref()
            .map(|scope| scope.display.to_value()),
        Some(serde_json::json!({"id": "d2"}))
    );

    let request = parse_observe_payload(
        r#"{mode:"hybrid",scope:{display:{window_ref:"@e4",observation_id:"obs-1"}}}"#,
    )
    .unwrap();
    assert_eq!(
        request
            .display_scope
            .as_ref()
            .map(|scope| scope.display.to_value()),
        Some(serde_json::json!({"window_ref": "@e4", "observation_id": "obs-1"}))
    );
}

#[test]
fn parse_observe_payload_should_reject_display_scope_legacy_shapes() {
    assert!(parse_observe_payload(r#"{display_id:"d2"}"#).is_err());
    assert!(parse_observe_payload(r#"{scope:{display:{ref:"@d2"}}}"#).is_err());
    assert!(parse_observe_payload(r#"{scope:{display:{window_ref:"@e4"}}}"#).is_err());
}

#[test]
fn render_observe_response_should_keep_section_ref_scope() {
    let request = ObserveRequest {
        mode: ObserveMode::Ax,
        include_screenshot: false,
        include_windows: false,
        ..ObserveRequest::for_mode(ObserveMode::Ax)
    };
    let snapshot = AxSnapshot::complete("macos", vec![fake_ax_window()], false)
        .with_observation("@observe ax")
        .unwrap();
    let produced = ProducedSections {
        savefile_frames: Vec::new(),
        visual: None,
        windows: None,
        window_observation: None,
        primary_observation: snapshot.observation.clone(),
        accessibility: Some(snapshot),
        display_scope_resolution: None,
    };

    let response = render_observe_response(Some(9), &request, produced).unwrap();
    assert!(response.savefile_frames.is_empty());
    let payload = response
        .response_line
        .strip_prefix("@response ")
        .expect("response should have prefix");
    let value: Value = serde_json::from_str(payload).unwrap();
    assert_eq!(value["id"], 9);
    assert_eq!(value["value"]["kind"], "observe");
    assert_eq!(value["value"]["schema"], OBSERVE_SCHEMA);
    assert_eq!(value["value"]["mode"], "ax");
    assert_eq!(
        value["value"]["primary_observation_source"],
        "accessibility"
    );
    assert_eq!(
        value["value"]["refs"]["sample"][0]["section"],
        "accessibility"
    );
    assert_eq!(
        value["value"]["refs"]["sample"][0]["observation_id"],
        value["value"]["observation"]["observation_id"]
    );
    assert_eq!(value["value"]["visual"]["status"], "not_requested");
}

#[test]
fn build_observe_bundle_should_expose_value_without_response_line_parsing() {
    let request = ObserveRequest {
        mode: ObserveMode::Ax,
        include_screenshot: false,
        include_windows: false,
        ..ObserveRequest::for_mode(ObserveMode::Ax)
    };
    let snapshot = AxSnapshot::complete("macos", vec![fake_ax_window()], false)
        .with_observation("@observe ax")
        .unwrap();
    let produced = ProducedSections {
        savefile_frames: Vec::new(),
        visual: None,
        windows: None,
        window_observation: None,
        primary_observation: snapshot.observation.clone(),
        accessibility: Some(snapshot),
        display_scope_resolution: None,
    };

    let bundle = build_observe_bundle_from_sections(&request, produced).unwrap();

    assert!(bundle.savefile_frames.is_empty());
    assert_eq!(bundle.value["kind"], "observe");
    assert_eq!(bundle.value["schema"], OBSERVE_SCHEMA);
    assert_eq!(bundle.value["mode"], "ax");
    assert_eq!(bundle.value["primary_observation_source"], "accessibility");
    assert_eq!(bundle.value["visual"]["status"], "not_requested");
}

#[test]
fn select_primary_observation_should_record_visual_when_it_is_the_only_section() {
    let request = ObserveRequest::for_mode(ObserveMode::Visual);
    let observation = select_primary_observation(&request, None, None)
        .unwrap()
        .expect("visual observe should record a primary observation");

    assert_eq!(observation.scope, "observe.visual");
    assert_eq!(observation.source_command, "@observe visual");
    assert_eq!(observation.root.schema, OBSERVE_SCHEMA);
    assert_eq!(observation.root.coordinate_space, "os-logical");
}

fn fake_ax_window() -> AxWindow {
    AxWindow {
        id: "pid:1/window:0".to_owned(),
        ref_id: None,
        pid: 1,
        process_name: "System Settings".to_owned(),
        title: Some("储存空间".to_owned()),
        role: "AXWindow".to_owned(),
        subrole: None,
        rect: Some(AxRect {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        }),
        focused: Some(true),
        elements: vec![AxElement {
            id: "pid:1/window:0/path:0".to_owned(),
            ref_id: None,
            role: "AXButton".to_owned(),
            subrole: None,
            name: Some("储存空间".to_owned()),
            value: None,
            value_redacted: false,
            description: None,
            rect: None,
            enabled: Some(true),
            actions: vec!["AXPress".to_owned()],
            ax_path: vec![0],
            children: Vec::new(),
        }],
    }
}

#[test]
fn parse_observe_payload_should_accept_compact_form() {
    use crate::control_observation::observe::ObserveTarget;

    // Mode-only compact: "@observe:window" should set mode, no target.
    let mode_only = parse_observe_payload("window").unwrap();
    assert_eq!(mode_only.mode, ObserveMode::Window);
    assert!(mode_only.target.is_none());

    // Target-only compact: "@observe:app:Calculator" should set target, default mode (Hybrid).
    let target_only = parse_observe_payload("app:Calculator").unwrap();
    assert_eq!(target_only.mode, ObserveMode::Hybrid);
    assert_eq!(
        target_only.target.as_ref().and_then(|t| t.app.as_deref()),
        Some("Calculator")
    );

    // Both: "@observe:app:Calculator,window" should set both.
    let both = parse_observe_payload("app:Calculator,window").unwrap();
    assert_eq!(both.mode, ObserveMode::Window);
    assert_eq!(
        both.target.as_ref().and_then(|t| t.app.as_deref()),
        Some("Calculator")
    );

    // ax mode compact.
    let ax_mode = parse_observe_payload("ax").unwrap();
    assert_eq!(ax_mode.mode, ObserveMode::Ax);
    assert!(ax_mode.target.is_none());

    // ax + target.
    let ax_target = parse_observe_payload("app:Settings,ax").unwrap();
    assert_eq!(ax_target.mode, ObserveMode::Ax);
    assert_eq!(
        ax_target.target.as_ref().and_then(|t| t.app.as_deref()),
        Some("Settings")
    );

    // Empty trailing mode should default to Hybrid, not error.
    let empty_mode = parse_observe_payload("app:Calculator,").unwrap();
    assert_eq!(empty_mode.mode, ObserveMode::Hybrid);
    assert_eq!(
        empty_mode.target.as_ref().and_then(|t| t.app.as_deref()),
        Some("Calculator")
    );
}

#[test]
fn parse_observe_payload_should_reject_non_ascii_app_in_compact_form() {
    let err = parse_observe_payload("app:计算器")
        .expect_err("non-ASCII app name must fail at parser layer");
    let msg = err.to_string();
    assert!(msg.contains("ASCII"), "error must mention ASCII: {msg}");
    // After format substitution the input echoes in `received: 计算器`.
    assert!(msg.contains("计算器"), "error must echo input name: {msg}");
    // Should also reject in target-only form.
    assert!(parse_observe_payload("app:計算機,window").is_err());
}

#[test]
fn parse_observe_payload_should_reject_too_many_compact_fields() {
    assert!(parse_observe_payload("app:Calculator,window,ax").is_err());
    assert!(parse_observe_payload("window,ax,hybrid").is_err());
}

#[test]
fn parse_observe_payload_should_reject_unknown_mode_in_compact_form() {
    let err = parse_observe_payload("desktop").expect_err("unknown mode must fail");
    assert!(err.to_string().contains("desktop"));
}

// --- epoch (feature/observe-epoch-stale-reject) ---

#[test]
fn render_observe_response_should_expose_epoch_at_top_level() {
    // epoch 字段应该出现在响应顶层,等于 primary observation 的 created_at_unix_ms.
    // 客户端用这个 epoch 跟 @computer-act 一起回传, daemon 验证动作是否 stale.
    let request = ObserveRequest {
        mode: ObserveMode::Ax,
        include_screenshot: false,
        include_windows: false,
        ..ObserveRequest::for_mode(ObserveMode::Ax)
    };
    let snapshot = AxSnapshot::complete("macos", vec![fake_ax_window()], false)
        .with_observation("@observe ax")
        .unwrap();
    let primary_observation = snapshot
        .observation
        .clone()
        .expect("snapshot must carry observation header");
    let produced = ProducedSections {
        savefile_frames: Vec::new(),
        visual: None,
        windows: None,
        window_observation: None,
        primary_observation: Some(primary_observation.clone()),
        accessibility: Some(snapshot),
        display_scope_resolution: None,
    };

    let bundle = build_observe_bundle_from_sections(&request, produced).unwrap();
    let epoch = bundle.value["epoch"]
        .as_u64()
        .expect("epoch should be u64 at top level");
    assert_eq!(
        epoch, primary_observation.created_at_unix_ms,
        "epoch 必须等于 primary observation 的 created_at_unix_ms"
    );
    // epoch 与 observed_at_unix_ms 应该类同 (同时间产生), 允许差 1ms 内
    let observed_at = bundle.value["observed_at_unix_ms"]
        .as_u64()
        .expect("observed_at_unix_ms should be u64");
    assert!(
        observed_at.abs_diff(epoch) <= 1,
        "epoch ({epoch}) 应该与 observed_at_unix_ms ({observed_at}) 一致"
    );
}

#[test]
fn build_observe_response_should_expose_epoch_even_without_primary_observation() {
    // 没有 primary observation 时, epoch 退化到 observed_at_unix_ms, 客户端拿到
    // 一个非零 epoch, 仍可参与 round-trip (后续 compare 注定不匹配, fast-reject).
    let request = ObserveRequest {
        mode: ObserveMode::Ax,
        include_screenshot: false,
        include_windows: false,
        ..ObserveRequest::for_mode(ObserveMode::Ax)
    };
    let produced = ProducedSections {
        savefile_frames: Vec::new(),
        visual: None,
        windows: None,
        window_observation: None,
        primary_observation: None,
        accessibility: None,
        display_scope_resolution: None,
    };

    let bundle = build_observe_bundle_from_sections(&request, produced).unwrap();
    let epoch = bundle.value["epoch"]
        .as_u64()
        .expect("epoch should fallback to observed_at_unix_ms");
    let observed_at = bundle.value["observed_at_unix_ms"]
        .as_u64()
        .expect("observed_at_unix_ms should be u64");
    assert_eq!(
        epoch, observed_at,
        "无 primary observation 时, epoch 应该等于 observed_at_unix_ms"
    );
}
