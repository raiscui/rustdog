use super::*;
use crate::{
    control_gui_bench::GuiBenchRequest,
    control_observation::{
        SelectorGetRequest, SelectorRefindPolicy, SelectorRefindRequest, SelectorRefindSource,
        SelectorResolveRequest,
    },
    control_web::{
        WebActRequest, WebFindBrowserTarget, WebFindQuery, WebFindRequest, WebFindTarget,
    },
};

#[test]
fn parse_should_support_web_find_command() {
    assert_eq!(
        parse_control_line(
            r#"@web-find#401:{target:{browser:"active"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(401),
            command: ControlCommand::WebFind(WebFindRequest {
                target: WebFindTarget {
                    browser: WebFindBrowserTarget::Active,
                    app: None,
                    window_id: None,
                    window_ref: None,
                    observation_id: None,
                    window_title_contains: None,
                },
                query: WebFindQuery {
                    text: "首页".to_owned(),
                },
                display_scope: None,
                roles: vec!["AXLink".to_owned(), "AXButton".to_owned()],
                limit: 10,
                depth: 8,
                max_elements: 2000,
                include_values: true,
            }),
        })
    );

    assert!(parse_control_line(r#"@web-find:{match:{}}"#).is_err());
    assert!(parse_control_line(r#"@web-find:{match:{text:"首页"},roles:[]}"#).is_err());
    assert!(
        parse_control_line(r#"@web-find:{target:{browser:"background"},match:{text:"首页"}}"#)
            .is_err()
    );

    let window_scoped = parse_control_line(
        r#"@web-find#403:{target:{window_id:"pid:96405/window:3"},match:{text:"首页"}}"#,
    )
    .unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::WebFind(request),
        ..
    }) = window_scoped
    else {
        panic!("expected @web-find control request");
    };
    assert_eq!(
        request.target.window_id.as_deref(),
        Some("pid:96405/window:3")
    );

    let ref_scoped = parse_control_line(
        r#"@web-find#404:{target:{window_ref:"@e1",observation_id:"obs-123"},match:{text:"首页"}}"#,
    )
    .unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::WebFind(request),
        ..
    }) = ref_scoped
    else {
        panic!("expected @web-find control request");
    };
    assert_eq!(request.target.window_ref.as_deref(), Some("@e1"));
    assert_eq!(request.target.observation_id.as_deref(), Some("obs-123"));

    assert!(
        parse_control_line(r#"@web-find:{target:{window_ref:"@e1"},match:{text:"首页"}}"#).is_err()
    );
    assert!(
        parse_control_line(
            r#"@web-find:{target:{window_id:"pid:1/window:0",window_ref:"@e1",observation_id:"obs-1"},match:{text:"首页"}}"#
        )
        .is_err()
    );
}

#[test]
fn parse_should_support_web_act_command() {
    let parsed = parse_control_line(
        r#"@web-act#402:{target:{browser:"active"},match:{text:"首页"},action:"press",verify:true,roles:["AXLink"],limit:1}"#,
    )
    .unwrap();
    let ControlParseResult::Control(ControlRequest {
        request_id: Some(402),
        command:
            ControlCommand::WebAct(WebActRequest {
                find,
                action,
                verify,
            }),
    }) = parsed
    else {
        panic!("expected @web-act control request");
    };

    assert_eq!(
        find,
        WebFindRequest {
            target: WebFindTarget {
                browser: WebFindBrowserTarget::Active,
                app: None,
                window_id: None,
                window_ref: None,
                observation_id: None,
                window_title_contains: None,
            },
            query: WebFindQuery {
                text: "首页".to_owned(),
            },
            display_scope: None,
            roles: vec!["AXLink".to_owned()],
            limit: 1,
            depth: 8,
            max_elements: 2000,
            include_values: true,
        }
    );
    assert_eq!(action.as_str(), "press");
    assert!(verify);

    assert!(parse_control_line(r#"@web-act:{match:{text:"首页"},verify:true}"#).is_err());
    assert!(parse_control_line(r#"@web-act:{match:{text:"首页"},action:"click"}"#).is_err());

    let ref_scoped = parse_control_line(
        r#"@web-act#405:{target:{window_ref:"@e1",observation_id:"obs-123"},match:{text:"首页"},action:"press"}"#,
    )
    .unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::WebAct(WebActRequest { find: ref_find, .. }),
        ..
    }) = ref_scoped
    else {
        panic!("expected @web-act control request");
    };
    assert_eq!(ref_find.target.window_ref.as_deref(), Some("@e1"));
    assert_eq!(ref_find.target.observation_id.as_deref(), Some("obs-123"));
}

#[test]
fn parse_should_support_gui_bench_command() {
    assert_eq!(
        parse_control_line(
            r#"@gui-bench#501:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level"}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(501),
            command: ControlCommand::GuiBench(GuiBenchRequest {
                suite: "computer-use-density".to_owned(),
                case_name: "xhs-left-nav-home".to_owned(),
                variant: "baseline-low-level".to_owned(),
                runner: crate::control_gui_bench::GuiBenchRunner::Fixture,
                allow_side_effects: false,
                write_artifact: false,
            }),
        })
    );
    assert_eq!(
        parse_control_line(
            r#"@gui-bench:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all",write_artifact:true}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::GuiBench(GuiBenchRequest {
                suite: "computer-use-density".to_owned(),
                case_name: "xhs-left-nav-home".to_owned(),
                variant: "all".to_owned(),
                runner: crate::control_gui_bench::GuiBenchRunner::Fixture,
                allow_side_effects: false,
                write_artifact: true,
            }),
        })
    );

    assert!(parse_control_line(r#"@gui-bench:{suite:"computer-use-density"}"#).is_err());
    assert!(
        parse_control_line(
            r#"@gui-bench:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level",mode:"live"}"#
        )
        .is_err()
    );
}

#[test]
fn parse_should_support_selector_commands() {
    assert_eq!(
        parse_control_line(r#"@selector-get#301:{selector_id:"sel-v1-abc",include_history:true}"#)
            .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(301),
            command: ControlCommand::SelectorGet(SelectorGetRequest {
                selector_id: "sel-v1-abc".to_owned(),
                include_history: true,
            }),
        })
    );
    assert_eq!(
        parse_control_line(
            r#"@selector-resolve#302:{selector_id:"sel-v1-abc",limit:5,dry_run:true,include_explanations:false}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(302),
            command: ControlCommand::SelectorResolve(SelectorResolveRequest {
                selector_id: "sel-v1-abc".to_owned(),
                limit: 5,
                dry_run: true,
                include_explanations: false,
            }),
        })
    );
    assert_eq!(
        parse_control_line(
            r#"@selector-refind#303:{selector_id:"sel-v1-abc",limit:7,policy:"manual",min_confidence:0.75,include_explanations:false,include_history:true,source:{observation_id:"obs-old",ref:"@e8"}}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(303),
            command: ControlCommand::SelectorRefind(SelectorRefindRequest {
                selector_id: "sel-v1-abc".to_owned(),
                limit: 7,
                policy: SelectorRefindPolicy::Manual,
                min_confidence_milli: 750,
                include_explanations: false,
                include_history: true,
                source: Some(SelectorRefindSource {
                    observation_id: "obs-old".to_owned(),
                    ref_id: "@e8".to_owned(),
                }),
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@selector-refind:{selector_id:"sel-v1-abc"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::SelectorRefind(SelectorRefindRequest {
                selector_id: "sel-v1-abc".to_owned(),
                limit: crate::control_observation::refind::DEFAULT_REFIND_LIMIT,
                policy: SelectorRefindPolicy::Safe,
                min_confidence_milli:
                    crate::control_observation::refind::DEFAULT_REFIND_MIN_CONFIDENCE_MILLI,
                include_explanations: true,
                include_history: false,
                source: None,
            }),
        })
    );
    assert!(
        parse_control_line(r#"@selector-refind:{selector_id:"sel",min_confidence:1.01}"#).is_err()
    );
    assert!(parse_control_line(r#"@selector-refind:{selector_id:"sel",policy:"auto"}"#).is_err());
    assert!(parse_control_line(
        r#"@selector-refind:{selector_id:"sel",source:{observation_id:"obs"}}"#
    )
    .is_err());
}

#[test]
fn parse_should_reject_invalid_mouse_payloads() {
    assert!(parse_control_line(r#"@mouse-move:{x:1,y:2,dx:1,dy:2}"#).is_err());
    assert!(parse_control_line(r#"@mouse-move:{dx:1,coordinate_space:"relative"}"#).is_err());
    assert!(parse_control_line(r#"@mouse-button:{button:"side",mode:"press"}"#).is_err());
    assert!(parse_control_line(r#"@mouse-button:{button:"left",mode:"hold"}"#).is_err());
    assert!(parse_control_line(r#"@click:{x:1,y:2,count:0}"#).is_err());
    assert!(parse_control_line(r#"@click:{x:1,y:2,coordinate_space:"native"}"#).is_err());
    assert!(parse_control_line(r#"@click:{target:{ref:"@e1"}}"#).is_err());
    assert!(
        parse_control_line(r#"@click:{x:1,y:2,target:{ref:"@e1",observation_id:"obs-1"}}"#)
            .is_err()
    );
    assert!(parse_control_line(
        r#"@click:{target:{ref:"@e1",selector_id:"sel-v1",observation_id:"obs-1"}}"#
    )
    .is_err());
    assert!(
        parse_control_line(r#"@click:{target:{selector_id:"sel-v1",observation_id:"obs-1"}}"#)
            .is_err()
    );
    assert!(parse_control_line(r#"@drag:{from:{x:1,y:2},to:{x:3,y:4},steps:0}"#).is_err());
    assert!(parse_control_line(r#"@drag:{from:{x:1},to:{x:3,y:4}}"#).is_err());
    assert!(parse_control_line(r#"@wheel:{delta_y:0}"#).is_err());
    assert!(parse_control_line(r#"@wheel:{x:1,delta_y:-3}"#).is_err());
    assert!(parse_control_line(r#"@wheel:{delta_y:-3,unknown:1}"#).is_err());
    assert!(
        parse_control_line(r#"@wheel:{x:1,y:2,delta_y:-3,coordinate_space:"relative"}"#).is_err()
    );
}
