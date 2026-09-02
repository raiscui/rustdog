use super::*;

mod bootstrap;
mod cancel_seq;
mod computer_act;
mod flow;
mod open_app;
mod wait;
mod web_gui;

use crate::{
    control_ax::{
        AxActionName, AxActionRequest, AxMode, AxSetValueRequest, AxTarget, AxTreeScope,
        AxValueSetMode, TypeTextMode, TypeTextRequest,
    },
    control_display_scope::{DisplayScope, DisplaySelector},
    control_mouse::{
        MouseAnchor, MouseButtonMode, MouseButtonName, MouseCoordinateSpace, MouseEndpoint,
        MousePoint, MouseRefTarget, MouseSelectorTarget, DEFAULT_MOUSE_CLICK_HOLD_MS,
        DEFAULT_MOUSE_CLICK_INTERVAL_MS,
    },
    control_observation::{
        observe::{ObserveMode, ObserveTarget},
        ObserveRequest, SelectorRefindPolicy,
    },
    control_window::{
        WindowCloseStrategy, WindowCommandTarget, WindowQuery, WindowResizeBox, WindowResizeOrigin,
        WindowResizeRequest, WindowResizeSize, WindowResizeUnit, WindowResizeVerify,
        WindowSelectPolicy,
    },
};

#[test]
fn parse_should_route_plain_shell_lines_to_literal() {
    assert_eq!(
        parse_control_line("echo hi").unwrap(),
        ControlParseResult::LiteralShellLine("echo hi".to_owned())
    );
}

#[test]
fn parse_should_unescape_double_at_to_literal_shell_line() {
    assert_eq!(
        parse_control_line("@@echo hi").unwrap(),
        ControlParseResult::LiteralShellLine("@echo hi".to_owned())
    );
}

#[test]
fn parse_should_support_key_paste_script_cmd_and_screenshot() {
    assert_eq!(
        parse_control_line(r#"@key:"F11""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Key(KeyRequest::legacy(
                "F11",
                DEFAULT_KEY_HOLD_MS,
                KeyMode::PressRelease,
            )),
        })
    );
    assert_eq!(
        parse_control_line(r#"@paste:"hello""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Paste(PasteRequest::legacy_text("hello")),
        })
    );
    assert_eq!(
        parse_control_line("@paste").unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Paste(PasteRequest::hotkey()),
        })
    );
    assert_eq!(
        parse_control_line("@capabilities").unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Capabilities,
        })
    );
    assert_eq!(
        parse_control_line(r#"@paste#12"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(12),
            command: ControlCommand::Paste(PasteRequest::hotkey()),
        })
    );
    assert_eq!(
        parse_control_line(r#"@script:"echo hi""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Script("echo hi".to_owned()),
        })
    );
    assert_eq!(
        parse_control_line(r#"@cmd:"echo hi""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Script("echo hi".to_owned()),
        })
    );
    assert_eq!(
        parse_control_line(
            r#"@savefile:{filename:"shot.jpg",mime:"image/jpeg",encoding:"base64",data:"QUJD"}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::SaveFile(SaveFileFrame {
                request_id: None,
                filename: "shot.jpg".to_owned(),
                mime: "image/jpeg".to_owned(),
                encoding: "base64".to_owned(),
                data: "QUJD".to_owned(),
                quality: None,
                width: None,
                height: None,
            }),
        })
    );
    assert_eq!(
        parse_control_line("@screenshot").unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Screenshot(ScreenshotRequest::default()),
        })
    );
    assert_eq!(
        parse_control_line(
            r#"@screenshot:{target:"display",display:"primary",format:"jpeg",quality:80}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Screenshot(ScreenshotRequest {
                display: ScreenshotDisplaySelector::Primary,
                layout: ScreenshotLayout::Single,
                quality: 80,
                ..ScreenshotRequest::default()
            }),
        })
    );
}

#[test]
fn parse_should_accept_raw_single_line_cmd_and_reject_ambiguous_payloads() {
    assert_eq!(
        parse_control_line("@cmd#42:printf READY").unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(42),
            command: ControlCommand::Script("printf READY".to_owned()),
        })
    );
    assert!(parse_control_line(r#"@cmd:{exec:"printf READY"}"#).is_err());
    assert!(parse_control_line("@cmd:\necho READY").is_err());
    assert!(parse_control_line("@cmd:printf READY\nprintf AGAIN").is_err());
    assert!(parse_control_line("@cmd:echo READY\n").is_ok());
}

#[test]
fn parse_should_support_screenshot_display_layout_and_coordinate_space() {
    assert_eq!(
            parse_control_line(
                r#"@screenshot#7:{target:"display",display:"all",layout:"composite",coordinate_space:"os-logical",format:"jpeg",quality:80}"#
            )
            .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(7),
                command: ControlCommand::Screenshot(ScreenshotRequest {
                    target: ScreenshotTarget::Display,
                    display: ScreenshotDisplaySelector::All,
                    layout: ScreenshotLayout::Composite,
                    coordinate_space: ScreenshotCoordinateSpace::OsLogical,
                    quality: 80,
                    ..ScreenshotRequest::default()
                }),
            })
        );

    assert_eq!(
        parse_control_line(r#"@screenshot:{display:"primary"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Screenshot(ScreenshotRequest {
                display: ScreenshotDisplaySelector::Primary,
                layout: ScreenshotLayout::Single,
                ..ScreenshotRequest::default()
            }),
        })
    );
}

#[test]
fn parse_should_support_mouse_requests() {
    assert_eq!(
        parse_control_line(r#"@mouse-move#1:{x:1,y:2,coordinate_space:"os-logical"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(1),
            command: ControlCommand::MouseMove(MouseMoveRequest {
                x: Some(1),
                y: Some(2),
                dx: None,
                dy: None,
                target: None,
                guard: None,
                coordinate_space: MouseCoordinateSpace::OsLogical,
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@mouse-move#2:{dx:1,dy:-2,coordinate_space:"relative"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(2),
            command: ControlCommand::MouseMove(MouseMoveRequest {
                x: None,
                y: None,
                dx: Some(1),
                dy: Some(-2),
                target: None,
                guard: None,
                coordinate_space: MouseCoordinateSpace::Relative,
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@mouse-button#3:{button:"left",mode:"press"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(3),
            command: ControlCommand::MouseButton(MouseButtonRequest {
                button: MouseButtonName::Left,
                mode: MouseButtonMode::Press,
                hold_ms: DEFAULT_MOUSE_CLICK_HOLD_MS,
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@click#4:{x:1,y:2}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(4),
            command: ControlCommand::Click(ClickRequest {
                x: Some(1),
                y: Some(2),
                target: None,
                guard: None,
                button: MouseButtonName::Left,
                count: 1,
                hold_ms: DEFAULT_MOUSE_CLICK_HOLD_MS,
                interval_ms: DEFAULT_MOUSE_CLICK_INTERVAL_MS,
                coordinate_space: MouseCoordinateSpace::OsLogical,
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@drag#5:{from:{x:1,y:2},to:{x:3,y:4}}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(5),
            command: ControlCommand::Drag(DragRequest {
                from: MouseEndpoint::Coordinate(MousePoint { x: 1, y: 2 }),
                to: MouseEndpoint::Coordinate(MousePoint { x: 3, y: 4 }),
                guard: None,
                button: MouseButtonName::Left,
                duration_ms: crate::control_mouse::DEFAULT_MOUSE_DRAG_DURATION_MS,
                steps: crate::control_mouse::DEFAULT_MOUSE_DRAG_STEPS,
                coordinate_space: MouseCoordinateSpace::OsLogical,
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@wheel#6:{delta_y:-3}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(6),
            command: ControlCommand::Wheel(WheelRequest {
                x: None,
                y: None,
                target: None,
                guard: None,
                delta_x: 0,
                delta_y: -3,
                coordinate_space: MouseCoordinateSpace::OsLogical,
            }),
        })
    );
}

#[test]
fn parse_should_support_mouse_ref_and_selector_targets() {
    assert_eq!(
        parse_control_line(r#"@click#7:{target:{ref:"@e1",observation_id:"obs-1"},button:"left"}"#)
            .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(7),
            command: ControlCommand::Click(ClickRequest {
                x: None,
                y: None,
                target: Some(MouseEndpoint::ObservationRef(MouseRefTarget {
                    observation_id: "obs-1".to_owned(),
                    ref_id: "@e1".to_owned(),
                    anchor: MouseAnchor::Center,
                })),
                guard: None,
                button: MouseButtonName::Left,
                count: 1,
                hold_ms: DEFAULT_MOUSE_CLICK_HOLD_MS,
                interval_ms: DEFAULT_MOUSE_CLICK_INTERVAL_MS,
                coordinate_space: MouseCoordinateSpace::OsLogical,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@mouse-move#8:{target:{ref:"@e2",observation_id:"obs-1"}}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(8),
            command: ControlCommand::MouseMove(MouseMoveRequest {
                x: None,
                y: None,
                dx: None,
                dy: None,
                guard: None,
                target: Some(MouseEndpoint::ObservationRef(MouseRefTarget {
                    observation_id: "obs-1".to_owned(),
                    ref_id: "@e2".to_owned(),
                    anchor: MouseAnchor::Center,
                })),
                coordinate_space: MouseCoordinateSpace::OsLogical,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@wheel#9:{target:{selector_id:"sel-v1-main"},delta_y:-3}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(9),
            command: ControlCommand::Wheel(WheelRequest {
                x: None,
                y: None,
                target: Some(MouseEndpoint::Selector(MouseSelectorTarget {
                    selector_id: "sel-v1-main".to_owned(),
                    auto_refind: false,
                    policy: SelectorRefindPolicy::Safe,
                    min_confidence_milli: 900,
                    anchor: MouseAnchor::Center,
                })),
                guard: None,
                delta_x: 0,
                delta_y: -3,
                coordinate_space: MouseCoordinateSpace::OsLogical,
            }),
        })
    );
}

#[test]
fn parse_should_support_mouse_display_guard_on_targeted_commands() {
    for line in [
        r#"@mouse-move:{x:1,y:2,guard:{display:{id:"d2"}}}"#,
        r#"@click:{x:1,y:2,guard:{display:{name_contains:"DELL"}}}"#,
        r#"@drag:{from:{x:1,y:2},to:{x:3,y:4},guard:{display:{contains_point:{x:1,y:2}}}}"#,
        r#"@wheel:{x:1,y:2,delta_y:-3,guard:{display:{window_id:"pid:1/window:0"}}}"#,
    ] {
        let parsed = parse_control_line(line).unwrap();
        match parsed {
            ControlParseResult::Control(ControlRequest {
                command: ControlCommand::MouseMove(request),
                ..
            }) => assert!(request.guard.is_some()),
            ControlParseResult::Control(ControlRequest {
                command: ControlCommand::Click(request),
                ..
            }) => assert!(request.guard.is_some()),
            ControlParseResult::Control(ControlRequest {
                command: ControlCommand::Drag(request),
                ..
            }) => assert!(request.guard.is_some()),
            ControlParseResult::Control(ControlRequest {
                command: ControlCommand::Wheel(request),
                ..
            }) => assert!(request.guard.is_some()),
            other => panic!("expected guarded mouse command, got {other:?}"),
        }
    }

    let err = parse_control_line(r#"@mouse-button:{button:"left",guard:{display:{id:"d2"}}}"#)
        .unwrap_err();
    assert!(err.to_string().contains("@mouse-button 不支持 guard"));
}

#[test]
fn parse_should_support_screenshot_ax_fields() {
    assert_eq!(
        parse_control_line(r#"@screenshot:{include_ax:true,ax_required:true}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Screenshot(ScreenshotRequest {
                include_ax: true,
                ax_required: true,
                ..ScreenshotRequest::default()
            }),
        })
    );

    assert_eq!(
        parse_control_line(
            r#"@screenshot:{ax_depth:4,ax_max_elements:1000,ax_include_values:false}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Screenshot(ScreenshotRequest {
                ax_depth: 4,
                ax_max_elements: 1000,
                ax_include_values: false,
                ..ScreenshotRequest::default()
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@screenshot:{include_ax:true,ax_mode:"windows"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Screenshot(ScreenshotRequest {
                include_ax: true,
                ax_mode: AxMode::Windows,
                ax_depth: crate::control_ax::AX_WINDOWS_DEPTH,
                ax_max_elements: crate::control_ax::AX_WINDOWS_MAX_ELEMENTS,
                ax_include_values: crate::control_ax::AX_WINDOWS_INCLUDE_VALUES,
                ..ScreenshotRequest::default()
            }),
        })
    );
}

#[test]
fn parse_should_support_screenshot_include_ocr_field() {
    assert_eq!(
        parse_control_line(r#"@screenshot:{include_ocr:true}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Screenshot(ScreenshotRequest {
                include_ocr: true,
                ..ScreenshotRequest::default()
            }),
        })
    );

    // WeChat 等 no-AX 场景主入口: 窗口截图 + OCR, 显式关掉 AX 层
    assert_eq!(
        parse_control_line(
            r#"@screenshot:{target:"window",window:{window_id:"pid:123/window:0"},include_ocr:true,include_ax:false}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Screenshot(ScreenshotRequest {
                target: ScreenshotTarget::Window,
                window: Some(ScreenshotWindowTarget {
                    window_id: Some("pid:123/window:0".to_owned()),
                    ref_id: None,
                    observation_id: None,
                }),
                include_ocr: true,
                ..ScreenshotRequest::default()
            }),
        })
    );

    // 字段重复必须显式报错, 与 include_ax 行为一致
    let err = parse_control_line(r#"@screenshot:{include_ocr:true,include_ocr:false}"#)
        .unwrap_err();
    assert!(err.to_string().contains("`include_ocr` 字段重复"));
}

#[test]
fn parse_should_support_observe_command() {
    assert_eq!(
        parse_control_line("@observe").unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Observe(ObserveRequest::default()),
        })
    );
    assert_eq!(
        parse_control_line(r#"@observe#21:{mode:"window"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(21),
            command: ControlCommand::Observe(ObserveRequest {
                mode: ObserveMode::Window,
                include_screenshot: false,
                include_ax: false,
                include_windows: true,
                ..ObserveRequest::default()
            }),
        })
    );
    assert_eq!(
        parse_control_line(
            r#"@observe:{mode:"hybrid",target:{app:"System Settings",window_title_contains:"储存"},include_screenshot:true,include_ax:true,include_windows:true,ax_required:true,include_manifest:false,limit:5}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Observe(ObserveRequest {
                mode: ObserveMode::Hybrid,
                target: Some(ObserveTarget {
                    app: Some("System Settings".to_owned()),
                    bundle_id: None,
                    window_title: None,
                    window_title_contains: Some("储存".to_owned()),
                }),
                include_screenshot: true,
                include_ax: true,
                ax_required: true,
                include_windows: true,
                include_manifest: false,
                limit: 5,
                ..ObserveRequest::default()
            }),
        })
    );
}

#[test]
fn parse_should_support_ax_tree_and_ax_commands() {
    assert_eq!(
        parse_control_line(r#"@ax-tree#1:{scope:"windows",depth:4,max_elements:1000}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(1),
            command: ControlCommand::AxTree(AxTreeRequest {
                scope: AxTreeScope::Windows,
                app_menu_app: None,
                depth: 4,
                max_elements: 1000,
                include_values: DEFAULT_AX_INCLUDE_VALUES,
                observation_id: None,
                epoch: None,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@ax-tree#4:{mode:"interactive"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(4),
            command: ControlCommand::AxTree(AxTreeRequest {
                scope: AxTreeScope::Windows,
                app_menu_app: None,
                depth: crate::control_ax::AX_INTERACTIVE_DEPTH,
                max_elements: crate::control_ax::AX_INTERACTIVE_MAX_ELEMENTS,
                include_values: crate::control_ax::AX_INTERACTIVE_INCLUDE_VALUES,
                observation_id: None,
                epoch: None,
            }),
        })
    );

    assert!(matches!(
        parse_control_line(r#"@ax-find#5:{role:"AXButton",name_contains:"取消"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(5),
            command: ControlCommand::AxFind(_),
        })
    ));

    assert!(matches!(
        parse_control_line(r#"@ax-get#6:{target:{id:"pid:1/window:0/path:0"},depth:2}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(6),
            command: ControlCommand::AxGet(_),
        })
    ));

    assert_eq!(
        parse_control_line(
            r#"@ax-action#7:{target:{id:"pid:1/window:0/path:0"},action:"AXShowMenu"}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(7),
            command: ControlCommand::AxAction(AxActionRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
                action: AxActionName::ShowMenu,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@ax-press#2:{target:{id:"pid:1/window:0/path:0"}}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(2),
            command: ControlCommand::AxPress(AxPressRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
                postcondition: None,
            }),
        })
    );

    assert_eq!(
            parse_control_line(
                r#"@ax-press#3:{target:{process:"System Information",window_title:"关于本机",role:"AXButton",description:"关闭按钮"}}"#
            )
            .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(3),
                command: ControlCommand::AxPress(AxPressRequest {
                    target: AxTarget {
                        process: Some("System Information".to_owned()),
                        window_title: Some("关于本机".to_owned()),
                        role: Some("AXButton".to_owned()),
                        description: Some("关闭按钮".to_owned()),
                        ..AxTarget::default()
                    },
                    postcondition: None,
                }),
            })
        );

    assert_eq!(
        parse_control_line(
            r#"@ax-set-value#8:{target:{id:"pid:1/window:0/path:0"},value:"hello",mode:"append"}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(8),
            command: ControlCommand::AxSetValue(AxSetValueRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
                value: "hello".to_owned(),
                mode: AxValueSetMode::Append,
            }),
        })
    );

    assert_eq!(
            parse_control_line(
                r#"@type-text#9:{target:{id:"pid:1/window:0/path:0"},text:"hello",mode:"ax-value",allow_clipboard:false}"#
            )
            .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(9),
                command: ControlCommand::TypeText(TypeTextRequest {
                    target: AxTarget {
                        id: Some("pid:1/window:0/path:0".to_owned()),
                        ..AxTarget::default()
                    },
                    text: "hello".to_owned(),
                    mode: TypeTextMode::AxValue,
                    allow_clipboard: false,
                }),
            })
        );
}

#[test]
fn parse_should_support_compact_app_scoped_ax_commands() {
    // canonical skill 暴露的是通用 app selector,不是 Calculator 专用语法.
    // 三种命令必须共享同一套 shell-safe 窗口归属合同.
    for command in [
        "@ax-find:app:Calculator,AXStaticText",
        "@ax-press:app:Calculator,1",
        "@ax-press-sequence:app:Calculator,1,加,2,等于",
    ] {
        assert!(
            parse_control_line(command).is_ok(),
            "canonical compact AX command should parse: {command}"
        );
    }
}

#[test]
fn parse_should_support_window_commands() {
    assert_eq!(
        parse_control_line(r#"@window-find#201:{app:"Terminal",title_contains:"rdog",limit:5}"#)
            .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(201),
            command: ControlCommand::WindowFind(WindowFindRequest {
                query: WindowQuery {
                    app: Some("Terminal".to_owned()),
                    title_contains: Some("rdog".to_owned()),
                    ..WindowQuery::default()
                },
                display_scope: None,
                limit: 5,
                include_state: true,
                include_recipes: true,
            }),
        })
    );

    assert_eq!(
            parse_control_line(
                r#"@window-activate:{window_id:"pid:1/window:0",recipe:"to_interact",allow_ambiguous:false,select:"frontmost"}"#
            )
            .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::WindowActivate(WindowActivateRequest {
                    target: WindowCommandTarget {
                        window_id: Some("pid:1/window:0".to_owned()),
                        ..WindowCommandTarget::default()
                    },
                    recipe: Some("to_interact".to_owned()),
                    steps: Vec::new(),
                    allow_ambiguous: false,
                    select: Some(WindowSelectPolicy::Frontmost),
                    guard: None,
                    verify: crate::control_window::WindowActivateVerify::default(),
                }),
            })
        );

    assert_eq!(
        parse_control_line(r#"@window-close:{window_id:"pid:1/window:0",strategy:"terminate"}"#)
            .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::WindowClose(WindowCloseRequest {
                target: WindowCommandTarget {
                    window_id: Some("pid:1/window:0".to_owned()),
                    ..WindowCommandTarget::default()
                },
                strategy: WindowCloseStrategy::Terminate,
                allow_ambiguous: false,
                select: None,
            }),
        })
    );

    assert_eq!(
        parse_control_line(
            r#"@window-resize#202:{target:{query:{app_contains:"Chrome",title_contains:"Docs"}},size:{width:1200,height:800,unit:"os-logical",box:"outer"},origin:"keep",guard:{display:{id:"d2"}},verify:true}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(202),
            command: ControlCommand::WindowResize(WindowResizeRequest {
                target: WindowCommandTarget {
                    query: WindowQuery {
                        app_contains: Some("Chrome".to_owned()),
                        title_contains: Some("Docs".to_owned()),
                        ..WindowQuery::default()
                    },
                    ..WindowCommandTarget::default()
                },
                size: WindowResizeSize {
                    width: 1200,
                    height: 800,
                    unit: WindowResizeUnit::OsLogical,
                    box_model: WindowResizeBox::Outer,
                },
                origin: WindowResizeOrigin::Keep,
                guard: Some(DisplayScope {
                    display: DisplaySelector::Id("d2".to_owned()),
                }),
                verify: WindowResizeVerify { tolerance_px: 2 },
            }),
        })
    );
}

#[test]
fn parse_should_support_single_positional_window_find_app() {
    assert_eq!(
        parse_control_line("@window-find:Terminal").unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::WindowFind(WindowFindRequest {
                query: WindowQuery {
                    app: Some("Terminal".to_owned()),
                    ..WindowQuery::default()
                },
                display_scope: None,
                limit: 20,
                include_state: true,
                include_recipes: true,
            }),
        })
    );
}

#[test]
fn parse_should_support_pty_open_and_close_requests() {
    assert_eq!(
        parse_control_line(r#"@pty:"codex""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyOpen(PtyOpenRequest {
                cmd: "codex".to_owned(),
                args: vec![],
                cols: 80,
                rows: 24,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@pty:"codex resume 019e02de-8814-72a2-ab0c-b06263cc0fba""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyOpen(PtyOpenRequest {
                cmd: "codex".to_owned(),
                args: vec![
                    "resume".to_owned(),
                    "019e02de-8814-72a2-ab0c-b06263cc0fba".to_owned()
                ],
                cols: 80,
                rows: 24,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@pty:"/bin/sh -c 'printf hello world'""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyOpen(PtyOpenRequest {
                cmd: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), "printf hello world".to_owned()],
                cols: 80,
                rows: 24,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@pty:"/tmp/my\ helper --name \"fast mode\"""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyOpen(PtyOpenRequest {
                cmd: "/tmp/my helper".to_owned(),
                args: vec!["--name".to_owned(), "fast mode".to_owned()],
                cols: 80,
                rows: 24,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@pty:{cmd:"codex",args:["--profile","fast"],cols:120,rows:40}"#)
            .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyOpen(PtyOpenRequest {
                cmd: "codex".to_owned(),
                args: vec!["--profile".to_owned(), "fast".to_owned()],
                cols: 120,
                rows: 40,
            }),
        })
    );

    assert_eq!(
        parse_control_line(
            r#"@pty:{cmd:"codex",argv:["codex","--profile","fast"],cols:120,rows:40}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyOpen(PtyOpenRequest {
                cmd: "codex".to_owned(),
                args: vec!["--profile".to_owned(), "fast".to_owned()],
                cols: 120,
                rows: 40,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@pty-close:{session_id:"session-1"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyClose(PtyCloseRequest {
                session_id: "session-1".to_owned(),
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@pty-detach:{session_id:"session-1"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyDetach(PtyDetachRequest {
                session_id: "session-1".to_owned(),
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@pty-attach:"session-1""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyAttach(PtyAttachRequest {
                session_id: "session-1".to_owned(),
                cols: 80,
                rows: 24,
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@pty-attach:{session_id:"session-1",cols:120,rows:40}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::PtyAttach(PtyAttachRequest {
                session_id: "session-1".to_owned(),
                cols: 120,
                rows: 40,
            }),
        })
    );
}

#[test]
fn parse_should_support_ping() {
    assert_eq!(
        parse_control_line("@ping").unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Ping,
        })
    );
}

#[test]
fn parse_should_support_optional_request_ids() {
    assert_eq!(
        parse_control_line(r#"@ping#42"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(42),
            command: ControlCommand::Ping,
        })
    );
    assert_eq!(
        parse_control_line(r#"@capabilities#11"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(11),
            command: ControlCommand::Capabilities,
        })
    );
    assert_eq!(
        parse_control_line(r#"@key#7:"F11""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(7),
            command: ControlCommand::Key(KeyRequest::legacy(
                "F11",
                DEFAULT_KEY_HOLD_MS,
                KeyMode::PressRelease,
            )),
        })
    );
    assert_eq!(
        parse_control_line(r#"@pty#9:"codex""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(9),
            command: ControlCommand::PtyOpen(PtyOpenRequest {
                cmd: "codex".to_owned(),
                args: vec![],
                cols: 80,
                rows: 24,
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@cmd#42:"printf READY""#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(42),
            command: ControlCommand::Script("printf READY".to_owned()),
        })
    );
    assert_eq!(
        parse_control_line(
            r#"@savefile#9:{filename:"shot.jpg",mime:"image/jpeg",encoding:"base64",data:"QUJD"}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(9),
            command: ControlCommand::SaveFile(SaveFileFrame {
                request_id: None,
                filename: "shot.jpg".to_owned(),
                mime: "image/jpeg".to_owned(),
                encoding: "base64".to_owned(),
                data: "QUJD".to_owned(),
                quality: None,
                width: None,
                height: None,
            }),
        })
    );
    assert_eq!(
        parse_control_line(r#"@screenshot#12"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(12),
            command: ControlCommand::Screenshot(ScreenshotRequest::default()),
        })
    );
}

#[test]
fn parse_should_support_key_object_payloads() {
    assert_eq!(
        parse_control_line(r#"@key#7:{key:"right-option",hold_ms:200,mode:"press_release"}"#)
            .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(7),
            command: ControlCommand::Key(KeyRequest::legacy(
                "right-option",
                200,
                KeyMode::PressRelease,
            )),
        })
    );

    assert_eq!(
        parse_control_line(r#"@key:{key:"right-option"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Key(KeyRequest::legacy(
                "right-option",
                DEFAULT_KEY_HOLD_MS,
                KeyMode::PressRelease,
            )),
        })
    );

    assert_eq!(
        parse_control_line(r#"@key#8:{key:"Return",delivery:"pid-targeted",pid:556}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(8),
            command: ControlCommand::Key(KeyRequest {
                key: "Return".to_owned(),
                hold_ms: DEFAULT_KEY_HOLD_MS,
                mode: KeyMode::PressRelease,
                delivery: KeyDelivery::PidTargeted,
                pid: Some(556),
                window_id: None,
                response_mode: KeyResponseMode::Structured,
            }),
        })
    );

    assert_eq!(
        parse_control_line(
            r#"@key:{key:"Cmd+W",delivery:"window-targeted",window_id:"pid:556/window:0"}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Key(KeyRequest {
                key: "Cmd+W".to_owned(),
                hold_ms: DEFAULT_KEY_HOLD_MS,
                mode: KeyMode::PressRelease,
                delivery: KeyDelivery::WindowTargeted,
                pid: None,
                window_id: Some("pid:556/window:0".to_owned()),
                response_mode: KeyResponseMode::Structured,
            }),
        })
    );
}

#[test]
fn parse_should_support_compact_bare_key_payloads() {
    for (line, key) in [
        ("@key:Cmd+T", "Cmd+T"),
        ("@key#7:Return", "Return"),
        ("@key:Esc", "Esc"),
    ] {
        let request_id = (line == "@key#7:Return").then_some(7);
        assert_eq!(
            parse_control_line(line).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id,
                command: ControlCommand::Key(KeyRequest::legacy(
                    key,
                    DEFAULT_KEY_HOLD_MS,
                    KeyMode::PressRelease,
                )),
            })
        );
    }

    // 带空白的key名称必须继续使用quoted或object语法,避免裸payload边界歧义。
    assert!(parse_control_line("@key:Page Down").is_err());
}

#[test]
fn parse_compact_window_selector_should_reject_non_ascii_app_name() -> io::Result<()> {
    use crate::control_protocol::parsers::parse_compact_window_selector;
    // ponytail: ASCII gate; macOS Launch Services 0-matches Chinese app names.
    let chinese_error = parse_compact_window_selector("@ax-find", "app:计算器")
        .expect_err("non-ASCII app name must be rejected at parser layer");
    let msg = chinese_error.to_string();
    assert!(msg.contains("ASCII"), "error must mention ASCII: {msg}");
    assert!(msg.contains("app:计算器"), "error must echo input: {msg}");
    // ASCII app name still parses correctly.
    let ascii = parse_compact_window_selector("@ax-find", "app:Calculator")?;
    match ascii {
        crate::control_protocol::parsers::CompactWindowSelector::App(app) => {
            assert_eq!(app, "Calculator");
        }
        other => panic!("expected App variant, got {other:?}"),
    }
    Ok(())
}

#[test]
fn parse_compact_window_selector_should_reject_mixed_ascii_app_name() -> io::Result<()> {
    use crate::control_protocol::parsers::parse_compact_window_selector;
    // Mixed ASCII + non-ASCII (e.g. Japanese kanji mixed with ASCII) is also non-ASCII.
    let err = parse_compact_window_selector("@ax-find", "app:電卓App")
        .expect_err("mixed script app name must be rejected");
    assert!(err.to_string().contains("ASCII"));
    Ok(())
}

#[test]
fn compact_should_reject_window_suffix_with_actionable_hint() {
    // 模型误用 `@window:N` 后缀时, 必须显式报错提示正确语法, 不能静默 0 匹配。
    let err = parse_control_line(r#"@ax-find:app:Calculator,AXButton@window:0"#)
        .expect_err("compact @window: suffix must be rejected");
    let msg = err.to_string();
    assert!(msg.contains("@window:"), "error must echo suffix: {msg}");
    assert!(
        msg.contains("app:APP,ROLE"),
        "error must suggest correct compact syntax: {msg}"
    );

    // 同样的误用出现在 @ax-press 的 description 位置也要被拒绝。
    let err = parse_control_line(r#"@ax-press:app:Calculator,1@window:0"#)
        .expect_err("compact @window: suffix must be rejected for press too");
    assert!(err.to_string().contains("@window:"));
}

#[test]
fn compact_should_route_named_prefix_fields() {
    // role: 前缀路由到角色槽位 (模型把对象语法字段名带进 compact)。
    let result = parse_control_line(r#"@ax-find:app:Calculator,role:AXButton"#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::AxFind(request),
        ..
    }) = result
    else {
        panic!("应解析为 AxFind");
    };
    assert_eq!(request.query.role.as_deref(), Some("AXButton"));
    assert_eq!(
        request.window.as_ref().and_then(|w| w.app.as_deref()),
        Some("Calculator")
    );

    // description: 前缀路由到 @ax-press 的按钮描述。
    let result = parse_control_line(r#"@ax-press:app:Calculator,description:加"#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::AxPress(request),
        ..
    }) = result
    else {
        panic!("应解析为 AxPress");
    };
    assert_eq!(request.target.description.as_deref(), Some("加"));
    assert_eq!(request.target.app.as_deref(), Some("Calculator"));
}

#[test]
fn compact_should_accept_trailing_named_options() {
    // 模型把对象选项追加到 compact 尾部: include_values/limit/depth/max_elements/mode。
    let result =
        parse_control_line(r#"@ax-find:app:Calculator,AXStaticText,include_values:true,limit:10"#)
            .unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::AxFind(request),
        ..
    }) = result
    else {
        panic!("应解析为 AxFind");
    };
    assert_eq!(request.query.role.as_deref(), Some("AXStaticText"));
    assert!(request.tree.include_values);
    assert_eq!(request.limit, 10);

    // depth / max_elements / mode 也生效。
    let result = parse_control_line(
        r#"@ax-find:app:Calculator,AXButton,depth:6,max_elements:100,mode:full"#,
    )
    .unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::AxFind(request),
        ..
    }) = result
    else {
        panic!("应解析为 AxFind");
    };
    assert_eq!(request.tree.depth, 6);
    assert_eq!(request.tree.max_elements, 100);
}

#[test]
fn compact_should_reject_unknown_prefix_with_prefix_list() {
    // 未知前缀必须报错并列出合法前缀, 不能静默 0 匹配。
    let err = parse_control_line(r#"@ax-find:app:Calculator,roll:AXButton"#)
        .expect_err("unknown prefix must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("未知字段前缀"),
        "error must mention prefix: {msg}"
    );
    assert!(
        msg.contains("role"),
        "error must list legal prefixes: {msg}"
    );
}

#[test]
fn compact_should_reject_conflicting_positional_and_named() {
    // 位置字段与命名字段同时给同一槽位 -> 报错 (模型二选一)。
    let err = parse_control_line(r#"@ax-find:app:Calculator,AXButton,role:AXStaticText"#)
        .expect_err("conflicting role must be rejected");
    assert!(
        err.to_string().contains("冲突"),
        "error must mention conflict: {err}"
    );
}

#[test]
fn object_syntax_should_accept_top_level_app() {
    // 对象语法顶层 app 归一化为 window 选择器 (模型把 compact 思维带进对象)。
    let result = parse_control_line(r#"@ax-find:{app:"Calculator",role:"AXStaticText"}"#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::AxFind(request),
        ..
    }) = result
    else {
        panic!("应解析为 AxFind");
    };
    assert_eq!(request.query.role.as_deref(), Some("AXStaticText"));
    assert_eq!(
        request.window.as_ref().and_then(|w| w.app.as_deref()),
        Some("Calculator")
    );

    // @ax-press 对象语法顶层字段归一化到 target。
    let result = parse_control_line(r#"@ax-press:{app:"Calculator",description:"加"}"#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::AxPress(request),
        ..
    }) = result
    else {
        panic!("应解析为 AxPress");
    };
    assert_eq!(request.target.app.as_deref(), Some("Calculator"));
    assert_eq!(request.target.description.as_deref(), Some("加"));
}

#[test]
fn guarded_press_should_accept_named_fields() {
    // guarded press 5 字段支持命名写法, 与位置式等价。
    let result = parse_control_line(
        r#"@ax-press:app:Calculator,description:删除,role:AXStaticText,expected_value:0,max_attempts:3"#,
    )
    .unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::AxPress(request),
        ..
    }) = result
    else {
        panic!("应解析为 AxPress");
    };
    assert_eq!(request.target.description.as_deref(), Some("删除"));
    let postcondition = request
        .postcondition
        .expect("guarded fields must set postcondition");
    assert_eq!(postcondition.role, "AXStaticText");
    assert_eq!(postcondition.expected_value, "0");
    assert_eq!(postcondition.max_attempts, 3);
}

#[test]
fn press_sequence_should_accept_named_description_fields() {
    // @ax-press-sequence 支持 description: 前缀追加 (重复出现合法)。
    let result = parse_control_line(
        r#"@ax-press-sequence:app:Calculator,description:8,description:加,description:等于"#,
    )
    .unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::AxPressSequence(request),
        ..
    }) = result
    else {
        panic!("应解析为 AxPressSequence");
    };
    let descriptions = request
        .targets
        .iter()
        .map(|target| target.description.as_deref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(descriptions, ["8", "加", "等于"]);
}

#[test]
fn window_find_should_accept_space_separated_and_compact_payloads() {
    // 空格分隔参数 (模型把 shell 习惯带进协议): @window-find app:Terminal
    let result = parse_control_line("@window-find app:Terminal").unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::WindowFind(request),
        ..
    }) = result
    else {
        panic!("应解析为 WindowFind");
    };
    assert_eq!(request.query.app.as_deref(), Some("Terminal"));

    // 空格 + JSON 参数也归一化。
    let result = parse_control_line(r#"@window-find {"app":"Terminal"}"#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::WindowFind(request),
        ..
    }) = result
    else {
        panic!("应解析为 WindowFind");
    };
    assert_eq!(request.query.app.as_deref(), Some("Terminal"));

    // compact 冒号写法 + 带引号值。
    let result = parse_control_line(r#"@window-find:app:"Terminal""#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::WindowFind(request),
        ..
    }) = result
    else {
        panic!("应解析为 WindowFind");
    };
    assert_eq!(request.query.app.as_deref(), Some("Terminal"));

    // pid compact。
    let result = parse_control_line("@window-find:pid:123/window:0").unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::WindowFind(request),
        ..
    }) = result
    else {
        panic!("应解析为 WindowFind");
    };
    assert_eq!(request.query.pid, Some(123));
}

#[test]
fn bare_window_find_should_return_all_windows() {
    // 裸 @window-find (无参数) 返回全部窗口查询, 模型无需知道参数写法。
    let result = parse_control_line("@window-find").unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::WindowFind(request),
        ..
    }) = result
    else {
        panic!("裸 @window-find 应解析为 WindowFind");
    };
    assert!(request.query.app.is_none(), "空 query = 匹配全部窗口");
    assert!(request.query.pid.is_none());
    assert!(request.query.title.is_none());
    assert_eq!(request.limit, 20);
}

#[test]
fn key_should_accept_space_separated_payload() {
    // 空格参数兼容对 @key 同样生效: @key 1 等价 @key:1。
    let result = parse_control_line("@key 1").unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::Key(request),
        ..
    }) = result
    else {
        panic!("应解析为 Key");
    };
    assert_eq!(request.key, "1");
}

#[test]
fn parse_should_reject_unknown_or_empty_or_multiline_payloads_or_bad_request_ids() {
    assert!(parse_control_line(r#"@unknown:"x""#).is_err());
    assert!(parse_control_line(r#"@key:"""#).is_err());
    assert!(parse_control_line("@script:\"printf a\\nb\"").is_err());
    assert!(parse_control_line(r#"@ping#:"x""#).is_err());
    assert!(parse_control_line(r#"@ping#abc"#).is_err());
    assert!(parse_control_line(r#"@ping#42:"x""#).is_err());
    assert!(parse_control_line(r#"@key:{hold_ms:200}"#).is_err());
    assert!(parse_control_line(r#"@key:{key:"x",hold_ms:"200"}"#).is_err());
    assert!(parse_control_line(r#"@key:{key:"x",mode:"tap"}"#).is_err());
    assert!(parse_control_line(r#"@key:{key:"x",unknown:1}"#).is_err());
    assert!(parse_control_line(r#"@capabilities:{foo:"bar"}"#).is_err());
    assert!(parse_control_line(r#"@pty:"""#).is_err());
    assert!(parse_control_line(r#"@pty:{cmd:"codex",args:[""]}"#).is_err());
    assert!(parse_control_line(r#"@pty:{cmd:"codex",args:["--a"],argv:["codex","--a"]}"#).is_err());
    assert!(parse_control_line(r#"@pty:{cmd:"codex",argv:["other","--a"]}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{quality:0}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{quality:101}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{format:"png"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{display:"secondary"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{layout:"separate"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{coordinate_space:"native"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{display:"all",layout:"single"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{display:"primary",layout:"composite"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{display:"all",display:"primary"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{layout:"composite",layout:"single"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{quality:75,quality:80}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{include_ax:true,include_ax:false}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{include_ax:"true"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{ax_depth:0}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{ax_max_elements:0}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{ax_mode:"small"}"#).is_err());
    assert!(parse_control_line(r#"@screenshot:{mode:"windows"}"#).is_err());
    assert!(parse_control_line(r#"@observe:{mode:"desktop"}"#).is_err());
    assert!(parse_control_line(r#"@observe:{mode:"ax",mode:"window"}"#).is_err());
    assert!(parse_control_line(r#"@observe:{limit:0}"#).is_err());
    assert!(parse_control_line(r#"@observe:{unknown:true}"#).is_err());
    assert!(parse_control_line(r#"@ax-tree:{depth:0}"#).is_err());
    assert!(parse_control_line(r#"@ax-tree:{max_elements:0}"#).is_err());
    assert!(parse_control_line(r#"@ax-find:{limit:0,role:"AXButton"}"#).is_err());
    assert!(parse_control_line(r#"@ax-find:{}"#).is_err());
    assert!(parse_control_line(r#"@ax-get:{target:{}}"#).is_err());
    assert!(parse_control_line(r#"@ax-press:{target:{}}"#).is_err());
}

#[test]
fn record_status_without_payload_is_accepted() {
    // Issue #23 E2E smoke: the CLI sends `@record-status` (no colon) and
    // the parser must accept it. Previously the parser rejected it with
    // "无效控制指令".
    let result = parse_control_line("@record-status").expect("parse should succeed");
    match result {
        ControlParseResult::Control(req) => {
            assert!(matches!(
                req.command,
                ControlCommand::Record(RecordRequest::Status)
            ));
        }
        other => panic!("expected Control, got {other:?}"),
    }
}

#[test]
fn parse_should_handle_single_char_operator_keys() {
    // 运算符单字符(+,*,/,=)是 AX press 的主要候选,必须能被 @key 解析
    for (line, expected) in [
        (r#"@key:"+""#, "+"),
        (r#"@key:{key:"+"}"#, "+"),
        (r#"@key:"*""#, "*"),
        (r#"@key:"/""#, "/"),
        (r#"@key:"=""#, "="),
        (r#"@key:{key:"-"}"#, "-"),
    ] {
        let result = parse_control_line(line).unwrap();
        let ControlParseResult::Control(ControlRequest {
            command: ControlCommand::Key(request),
            ..
        }) = result
        else {
            panic!("@{line} 应解析为 Key 控制指令");
        };
        assert_eq!(request.key, expected, "line={line}");
    }
}

#[test]
fn record_stop_and_cancel_without_payload_are_accepted() {
    // Symmetric handling for `@record-stop` and `@record-cancel` so the
    // CLI can drop the `{}` suffix without breaking.
    let stop = parse_control_line("@record-stop").expect("parse should succeed");
    assert!(matches!(
        stop,
        ControlParseResult::Control(ControlRequest {
            command: ControlCommand::Record(RecordRequest::Stop),
            ..
        })
    ));
    let cancel = parse_control_line("@record-cancel").expect("parse should succeed");
    assert!(matches!(
        cancel,
        ControlParseResult::Control(ControlRequest {
            command: ControlCommand::Record(RecordRequest::Cancel),
            ..
        })
    ));
}

#[test]
fn key_object_should_accept_modifiers_field() {
    // OpenAI 风格 modifiers 数组归一化为组合键字符串。
    let result = parse_control_line(r#"@key:{key:"k",modifiers:["Cmd"]}"#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::Key(request),
        ..
    }) = result
    else {
        panic!("应解析为 Key");
    };
    assert_eq!(request.key, "Cmd+k");

    // 多修饰符 + 单字符串形式。
    let result = parse_control_line(r#"@key:{key:"t",modifiers:"Cmd+Shift"}"#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::Key(request),
        ..
    }) = result
    else {
        panic!("应解析为 Key");
    };
    assert_eq!(request.key, "Cmd+Shift+t");

    // 空数组 = 无修饰符, key 原样。
    let result = parse_control_line(r#"@key:{key:"1",modifiers:[]}"#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::Key(request),
        ..
    }) = result
    else {
        panic!("应解析为 Key");
    };
    assert_eq!(request.key, "1");
}

#[test]
fn screenshot_window_target_should_accept_stable_window_locators() {
    let direct = parse_control_line(r#"@screenshot:{window:{window_id:"pid:1/window:0"}}"#)
        .expect("nested window id should parse");
    assert_eq!(
        direct,
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Screenshot(ScreenshotRequest {
                target: ScreenshotTarget::Window,
                window: Some(ScreenshotWindowTarget {
                    window_id: Some("pid:1/window:0".to_owned()),
                    ref_id: None,
                    observation_id: None,
                }),
                ..ScreenshotRequest::default()
            }),
        })
    );

    let observed = parse_control_line(
        r#"@screenshot#12:{target:"window",window:{ref:"@e3",observation_id:"obs-7"}}"#,
    )
    .expect("fresh observation window locator should parse");
    assert_eq!(
        observed,
        ControlParseResult::Control(ControlRequest {
            request_id: Some(12),
            command: ControlCommand::Screenshot(ScreenshotRequest {
                target: ScreenshotTarget::Window,
                window: Some(ScreenshotWindowTarget {
                    window_id: None,
                    ref_id: Some("@e3".to_owned()),
                    observation_id: Some("obs-7".to_owned()),
                }),
                ..ScreenshotRequest::default()
            }),
        })
    );

    let top_level = parse_control_line(r#"@screenshot:{window_id:"pid:2/window:1"}"#)
        .expect("top-level window id should normalize");
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::Screenshot(request),
        ..
    }) = top_level
    else {
        panic!("应解析为 Screenshot");
    };
    assert_eq!(request.target, ScreenshotTarget::Window);
    assert_eq!(
        request.window.and_then(|window| window.window_id),
        Some("pid:2/window:1".to_owned())
    );

    let err = parse_control_line(r#"@screenshot:{target:"window",ref:"@e3"}"#)
        .expect_err("window ref must be nested and paired with observation id");
    assert!(err.to_string().contains("只能写在 window 对象中"));
}

#[test]
fn ax_target_should_accept_app_field() {
    // AxTarget struct 有 app 字段, 但 parse_ax_target 漏了解析 (LLM 兼容缺口)。
    let result = parse_control_line(
        r#"@type-text:{target:{app:"Terminal",role:"AXTextArea"},text:"echo hi",mode:"ax-value"}"#,
    )
    .unwrap();
    let ControlParseResult::Control(ControlRequest {
        command: ControlCommand::TypeText(request),
        ..
    }) = result
    else {
        panic!("应解析为 TypeText");
    };
    assert_eq!(request.target.app.as_deref(), Some("Terminal"));
    assert_eq!(request.target.role.as_deref(), Some("AXTextArea"));
}
