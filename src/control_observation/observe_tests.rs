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
