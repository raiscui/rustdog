use super::*;

mod bootstrap;
mod flow;
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
                depth: 4,
                max_elements: 1000,
                include_values: DEFAULT_AX_INCLUDE_VALUES,
            }),
        })
    );

    assert_eq!(
        parse_control_line(r#"@ax-tree#4:{mode:"interactive"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(4),
            command: ControlCommand::AxTree(AxTreeRequest {
                scope: AxTreeScope::Windows,
                depth: crate::control_ax::AX_INTERACTIVE_DEPTH,
                max_elements: crate::control_ax::AX_INTERACTIVE_MAX_ELEMENTS,
                include_values: crate::control_ax::AX_INTERACTIVE_INCLUDE_VALUES,
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
