use super::*;
use crate::control_bootstrap::BootstrapMode;
use crate::control_frames::SaveFileFrame;
use crate::control_mouse::MouseEndpoint;
use crate::control_protocol::{KeyMode, KeyRequest, PasteRequestKind};
use crate::{control_actions::ActionExecutionResult, control_protocol::ControlCommand};
use std::{
    fs,
    net::{Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Default)]
struct FakeExecutor {
    commands: Arc<Mutex<Vec<ControlCommand>>>,
}

impl ControlActionExecutor for FakeExecutor {
    fn execute(&self, command: &ControlCommand, _shell: &str) -> io::Result<ActionExecutionResult> {
        self.commands
            .lock()
            .expect("commands lock should work")
            .push(command.clone());

        let stdout = match command {
            ControlCommand::Key(request) => format!("KEY:{}\n", request.key).into_bytes(),
            ControlCommand::Paste(request) => match &request.kind {
                PasteRequestKind::GlobalHotkey => b"PASTE:global-hotkey\n".to_vec(),
                PasteRequestKind::LegacyTextInjection(payload) => {
                    format!("PASTE:{payload}\n").into_bytes()
                }
            },
            ControlCommand::Script(payload) => format!("SCRIPT:{payload}\n").into_bytes(),
            ControlCommand::Ping => b"PONG\n".to_vec(),
            ControlCommand::Screenshot(request) => {
                format!("SCREENSHOT:{}\n", request.quality).into_bytes()
            }
            ControlCommand::Observe(request) => {
                format!("OBSERVE:{}\n", request.mode.as_str()).into_bytes()
            }
            ControlCommand::SaveFile(frame) => {
                format!("SAVEFILE:{}\n", frame.filename).into_bytes()
            }
            ControlCommand::PtyOpen(request) => format!("PTY_OPEN:{}\n", request.cmd).into_bytes(),
            ControlCommand::PtyClose(request) => {
                format!("PTY_CLOSE:{}\n", request.session_id).into_bytes()
            }
            ControlCommand::PtyDetach(request) => {
                format!("PTY_DETACH:{}\n", request.session_id).into_bytes()
            }
            ControlCommand::PtyAttach(request) => format!(
                "PTY_ATTACH:{}:{}x{}\n",
                request.session_id, request.cols, request.rows
            )
            .into_bytes(),
            ControlCommand::MouseMove(request) => format!(
                "MOUSE_MOVE:{}:{}\n",
                request.x.unwrap_or(0),
                request.y.unwrap_or(0)
            )
            .into_bytes(),
            ControlCommand::MouseButton(request) => {
                format!("MOUSE_BUTTON:{}\n", request.button.as_protocol_str()).into_bytes()
            }
            ControlCommand::Click(request) => format!(
                "CLICK:{}:{}\n",
                request.x.unwrap_or(0),
                request.y.unwrap_or(0)
            )
            .into_bytes(),
            ControlCommand::Drag(request) => {
                let from_x = match &request.from {
                    MouseEndpoint::Coordinate(point) => point.x,
                    _ => 0,
                };
                let to_x = match &request.to {
                    MouseEndpoint::Coordinate(point) => point.x,
                    _ => 0,
                };
                format!("DRAG:{from_x}:{to_x}\n").into_bytes()
            }
            ControlCommand::Wheel(request) => {
                format!("WHEEL:{}:{}\n", request.delta_x, request.delta_y).into_bytes()
            }
            ControlCommand::AxTree(request) => {
                format!("AX_TREE:{}:{}\n", request.depth, request.max_elements).into_bytes()
            }
            ControlCommand::AxFind(request) => format!("AX_FIND:{}\n", request.limit).into_bytes(),
            ControlCommand::AxGet(request) => format!(
                "AX_GET:{}\n",
                request.target.id.as_deref().unwrap_or("semantic")
            )
            .into_bytes(),
            ControlCommand::AxFocus(request) => format!(
                "AX_FOCUS:{}\n",
                request.window_id.as_deref().unwrap_or("target")
            )
            .into_bytes(),
            ControlCommand::AxScroll(request) => format!(
                "AX_SCROLL:{}:{}\n",
                request.direction.as_str(),
                request.pages
            )
            .into_bytes(),
            ControlCommand::AxAction(request) => format!(
                "AX_ACTION:{}:{}\n",
                request.action.protocol_str(),
                request.target.id.as_deref().unwrap_or("semantic")
            )
            .into_bytes(),
            ControlCommand::AxPress(request) => format!(
                "AX_PRESS:{}\n",
                request.target.id.as_deref().unwrap_or("semantic")
            )
            .into_bytes(),
            ControlCommand::AxSetValue(request) => format!(
                "AX_SET_VALUE:{}:{}\n",
                request.mode.as_str(),
                request.target.id.as_deref().unwrap_or("semantic")
            )
            .into_bytes(),
            ControlCommand::TypeText(request) => format!(
                "TYPE_TEXT:{}:{}\n",
                request.mode.as_str(),
                request.target.id.as_deref().unwrap_or("semantic")
            )
            .into_bytes(),
            ControlCommand::WindowFind(request) => {
                format!("WINDOW_FIND:{}\n", request.limit).into_bytes()
            }
            ControlCommand::WindowActivate(request) => format!(
                "WINDOW_ACTIVATE:{}\n",
                request
                    .target
                    .window_id
                    .as_deref()
                    .unwrap_or("query-target")
            )
            .into_bytes(),
            ControlCommand::WindowClose(request) => format!(
                "WINDOW_CLOSE:{}:{}\n",
                request.strategy.as_str(),
                request
                    .target
                    .window_id
                    .as_deref()
                    .unwrap_or("query-target")
            )
            .into_bytes(),
            ControlCommand::WindowResize(request) => format!(
                "WINDOW_RESIZE:{}x{}\n",
                request.size.width, request.size.height
            )
            .into_bytes(),
            ControlCommand::WebFind(request) => {
                format!("WEB_FIND:{}\n", request.limit).into_bytes()
            }
            ControlCommand::WebAct(request) => {
                format!("WEB_ACT:{}:{}\n", request.action.as_str(), request.verify).into_bytes()
            }
            ControlCommand::GuiBench(request) => format!(
                "GUI_BENCH:{}:{}:{}:{}:{}:{}\n",
                request.suite,
                request.case_name,
                request.variant,
                request.runner.as_str(),
                request.allow_side_effects,
                request.write_artifact
            )
            .into_bytes(),
            ControlCommand::Bootstrap(request) => {
                let mode = match request.mode {
                    BootstrapMode::Basic => "basic",
                    BootstrapMode::Gui => "gui",
                };
                format!("BOOTSTRAP:{mode}:{}\n", request.include_trace).into_bytes()
            }
            ControlCommand::Flow(request) => {
                format!("FLOW:{}:{}\n", request.schema, request.steps.len()).into_bytes()
            }
            ControlCommand::Capabilities => b"CAPABILITIES\n".to_vec(),
            ControlCommand::SelectorGet(request) => {
                format!("SELECTOR_GET:{}\n", request.selector_id).into_bytes()
            }
            ControlCommand::SelectorResolve(request) => {
                format!("SELECTOR_RESOLVE:{}\n", request.selector_id).into_bytes()
            }
            ControlCommand::SelectorRefind(request) => {
                format!("SELECTOR_REFIND:{}\n", request.selector_id).into_bytes()
            }
        };

        Ok(ActionExecutionResult {
            exit_code: 0,
            stdout,
            stderr: Vec::new(),
            response_value_json: None,
        })
    }
}

fn connected_pair() -> (TcpStream, TcpStream) {
    let listener = bind_test_listener();
    let port = listener
        .local_addr()
        .expect("listener should expose local addr")
        .port();

    let client =
        TcpStream::connect(("127.0.0.1", port)).expect("client should connect to test listener");
    let (server, _) = listener.accept().expect("server should accept test client");
    (client, server)
}

fn bind_test_listener() -> TcpListener {
    #[cfg(windows)]
    {
        const PROVIDER_INIT_ERROR: i32 = 10106;

        for _ in 0..8 {
            match TcpListener::bind(("127.0.0.1", 0)) {
                Ok(listener) => return listener,
                Err(err) if err.raw_os_error() == Some(PROVIDER_INIT_ERROR) => {
                    thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(err) => panic!("ephemeral listener should bind: {err:?}"),
            }
        }

        panic!("ephemeral listener should bind: Windows socket provider kept failing with 10106");
    }

    #[cfg(not(windows))]
    {
        TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral listener should bind")
    }
}

fn latest_response(output: &str) -> Option<serde_json::Value> {
    output.lines().rev().find_map(|line| {
        let json_text = line.strip_prefix("@response ")?;
        serde_json::from_str(json_text).ok()
    })
}

#[cfg(unix)]
fn temp_shell_wrapper(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("rdog-shell-wrapper-{name}-{}", std::process::id()));
    fs::write(
        &path,
        "#!/bin/sh\nif [ \"$1\" = \"-c\" ]; then\n  printf '%s' \"$2\"\nelse\n  printf 'unexpected-args'\nfi\n",
    )
    .expect("should write wrapper shell");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)
            .expect("wrapper metadata should exist")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("should mark wrapper executable");
    }

    path
}

fn cleanup_temp_path(path: &Path) {
    let _ = fs::remove_file(path);
}

fn temp_directory(name: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_millis();
    std::env::temp_dir().join(format!("rdog-shell-{name}-{millis}-{}", std::process::id()))
}

fn control_test_shell() -> &'static str {
    #[cfg(windows)]
    {
        "powershell.exe"
    }

    #[cfg(not(windows))]
    {
        "/bin/sh"
    }
}

fn escaped_literal_shell_case(_name: &str) -> (String, Vec<u8>, String, Option<PathBuf>) {
    #[cfg(windows)]
    {
        (
            "cmd.exe".to_owned(),
            b"@@echo ESCAPED_OK\n".to_vec(),
            "ESCAPED_OK".to_owned(),
            None,
        )
    }

    #[cfg(not(windows))]
    {
        let wrapper = temp_shell_wrapper(_name);
        (
            wrapper.to_string_lossy().to_string(),
            b"@@printf '@%s' ok\n".to_vec(),
            "@printf '@%s' ok".to_owned(),
            Some(wrapper),
        )
    }
}

#[test]
fn control_receiver_should_route_built_in_commands_to_executor() {
    let (mut client, server) = connected_pair();
    let executor = FakeExecutor::default();
    let recorded = Arc::clone(&executor.commands);
    let shell = control_test_shell();

    let worker =
        thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

    client
        .write_all(br#"@key:"F11""#)
        .expect("should write control line");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read control response");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    assert!(output.contains(r#"@response "KEY:F11\n""#));
    assert_eq!(
        recorded
            .lock()
            .expect("commands lock should work")
            .as_slice(),
        &[ControlCommand::Key(KeyRequest::legacy(
            "F11",
            200,
            KeyMode::PressRelease,
        ))]
    );
}

#[test]
fn control_receiver_should_wrap_success_response_with_request_id() {
    let (mut client, server) = connected_pair();
    let executor = FakeExecutor::default();
    let shell = control_test_shell();
    let worker =
        thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

    client
        .write_all(br#"@key#42:"F11""#)
        .expect("should write control line with request id");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read response with request id");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    assert!(output.contains(r#"@response {"id":42,"value":"KEY:F11\n"}"#));
}

#[test]
fn control_receiver_should_execute_basic_bootstrap_preflight() {
    let (mut client, server) = connected_pair();
    let executor = SystemControlActionExecutor::default();
    let shell = control_test_shell();
    let worker =
        thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

    client
        .write_all(br#"@bootstrap#601"#)
        .expect("should write bootstrap request");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read bootstrap response");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    let response = latest_response(&output).expect("bootstrap should return @response JSON");
    assert_eq!(response["id"].as_u64(), Some(601));
    let value = &response["value"];
    assert_eq!(value["kind"].as_str(), Some("bootstrap"));
    assert_eq!(value["schema"].as_str(), Some("rdog.bootstrap.v1"));
    assert_eq!(value["mode"].as_str(), Some("basic"));
    assert_eq!(value["liveness"]["reply"].as_str(), Some("pong"));
    assert_eq!(
        value["capabilities"]["schema"].as_str(),
        Some("rdog.capabilities.v1")
    );
    assert_eq!(
        value["observation"]["status"].as_str(),
        Some("not_requested")
    );
    assert_eq!(value["frames"]["savefile_count"].as_u64(), Some(0));
}

#[test]
fn control_receiver_should_execute_gui_bootstrap_window_probe() {
    let (mut client, server) = connected_pair();
    let executor = SystemControlActionExecutor::default();
    let shell = control_test_shell();
    let worker =
        thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

    client
        .write_all(br#"@bootstrap#602:{mode:"gui",observe:{mode:"window"},include_trace:false}"#)
        .expect("should write gui bootstrap request");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read gui bootstrap response");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    let response = latest_response(&output).expect("bootstrap should return @response JSON");
    assert_eq!(response["id"].as_u64(), Some(602));
    let value = &response["value"];
    assert_eq!(value["kind"].as_str(), Some("bootstrap"));
    assert_eq!(value["mode"].as_str(), Some("gui"));
    assert_eq!(value["observation"]["kind"].as_str(), Some("observe"));
    assert_eq!(value["observation"]["mode"].as_str(), Some("window"));
    assert_eq!(
        value["lanes"]["windows"]["status"].as_str(),
        Some("skipped")
    );
    assert_eq!(value["frames"]["savefile_count"].as_u64(), Some(0));
    assert!(value["trace"].is_null());
}

#[test]
fn control_receiver_should_execute_gui_bench_with_real_fixture_runner() {
    let (mut client, server) = connected_pair();
    let executor = SystemControlActionExecutor::default();
    let shell = control_test_shell();
    let worker =
        thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

    client
        .write_all(
            br#"@gui-bench#501:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level"}"#,
        )
        .expect("should write gui-bench request");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read gui-bench response");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    let response = latest_response(&output).expect("gui-bench should return @response JSON");
    assert_eq!(response["id"].as_u64(), Some(501));
    let value = &response["value"];
    assert_eq!(value["kind"].as_str(), Some("gui-bench"));
    assert_eq!(value["schema"].as_str(), Some("rdog.gui-bench.v1"));
    assert_eq!(value["status"].as_str(), Some("complete"));
    assert_eq!(value["runner"].as_str(), Some("fixture"));
    assert_eq!(value["dense_target_passed"].as_bool(), Some(false));
    assert_eq!(value["metrics"]["backend_request_count"].as_u64(), Some(8));
    assert!(value["threshold_failures"]
        .as_array()
        .expect("threshold_failures should be array")
        .iter()
        .any(|failure| failure.as_str() == Some("baseline-low-level:backend_request_count")));
}

#[test]
fn control_receiver_should_execute_gui_bench_all_variants() {
    let (mut client, server) = connected_pair();
    let executor = SystemControlActionExecutor::default();
    let shell = control_test_shell();
    let worker =
        thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

    client
        .write_all(
            br#"@gui-bench#502:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all"}"#,
        )
        .expect("should write gui-bench all request");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read gui-bench all response");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    let response = latest_response(&output).expect("gui-bench should return @response JSON");
    assert_eq!(response["id"].as_u64(), Some(502));
    let value = &response["value"];
    assert_eq!(value["variant"].as_str(), Some("all"));
    assert_eq!(value["variant_count"].as_u64(), Some(3));
    assert_eq!(value["runs"].as_array().unwrap().len(), 3);
    assert!(value["threshold_failures"]
        .as_array()
        .expect("threshold_failures should be array")
        .iter()
        .any(|failure| failure.as_str() == Some("baseline-low-level:backend_request_count")));
}

#[test]
fn control_receiver_should_write_gui_bench_artifact_for_ci_collection() {
    let expected_path =
        Path::new("target/rdog-bench/computer-use-density__xhs-left-nav-home__all.json");
    let _ = fs::remove_file(expected_path);

    let (mut client, server) = connected_pair();
    let executor = SystemControlActionExecutor::default();
    let shell = control_test_shell();
    let worker =
        thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

    client
        .write_all(
            br#"@gui-bench#503:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all",write_artifact:true}"#,
        )
        .expect("should write gui-bench artifact request");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read gui-bench artifact response");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    let response = latest_response(&output).expect("gui-bench should return @response JSON");
    assert_eq!(response["id"].as_u64(), Some(503));

    let value = &response["value"];
    let artifact_path = value["artifact"]["path"]
        .as_str()
        .expect("artifact path should be present");
    assert_eq!(artifact_path, expected_path.display().to_string());

    let artifact_text =
        fs::read_to_string(artifact_path).expect("gui-bench artifact should be written");
    let artifact: serde_json::Value =
        serde_json::from_str(&artifact_text).expect("artifact should be valid JSON");

    assert_eq!(artifact["schema"].as_str(), Some("rdog.gui-bench.v1"));
    assert_eq!(artifact["runner"].as_str(), Some("fixture"));
    assert_eq!(artifact["variant"].as_str(), Some("all"));
    assert_eq!(artifact["variant_count"].as_u64(), Some(3));
    assert_eq!(artifact["artifact"]["path"].as_str(), Some(artifact_path));
    assert_eq!(artifact["runs"].as_array().unwrap().len(), 3);
    assert!(artifact["threshold_failures"]
        .as_array()
        .expect("threshold_failures should be array")
        .iter()
        .any(|failure| failure.as_str() == Some("baseline-low-level:backend_request_count")));

    let _ = fs::remove_file(artifact_path);
}

#[test]
fn control_receiver_should_execute_savefile_request_and_report_saved_path() {
    let (mut client, server) = connected_pair();
    let save_dir = temp_directory("receiver-savefile");
    let executor = SystemControlActionExecutor::with_savefile_base_dir(save_dir.clone());
    let shell = control_test_shell();
    let worker =
        thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

    client
        .write_all(
            br#"@savefile#7:{filename:"shot.jpg",mime:"image/jpeg",encoding:"base64",data:"QUJD"}"#,
        )
        .expect("should write savefile request");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read savefile response");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    let saved_path = save_dir.join("shot.jpg");
    assert_eq!(
        fs::read(&saved_path).expect("saved file should exist"),
        b"ABC"
    );
    assert!(output.contains(r#""id":7"#));
    assert!(output.contains("saved file:"));

    let _ = fs::remove_dir_all(save_dir);
}

#[test]
fn receive_control_result_frames_should_save_file_before_final_response() {
    let (client, mut server) = connected_pair();
    let save_dir = temp_directory("savefile");
    let save_frame = SaveFileFrame {
        request_id: Some(7),
        filename: "shot.jpg".to_owned(),
        mime: "image/jpeg".to_owned(),
        encoding: "base64".to_owned(),
        data: "QUJD".to_owned(),
        quality: Some(75),
        width: Some(100),
        height: Some(60),
    };

    let worker = thread::spawn(move || {
        let mut transport =
            ControlTransport::from_tcp_stream(client).expect("transport should wrap tcp stream");
        let mut output = Vec::new();
        receive_control_result_frames(
            &mut transport,
            &mut output,
            &save_dir,
            ControlResponseDisplay::Protocol,
        )
        .expect("client should consume savefile and final response");
        (
            String::from_utf8(output).expect("output should be utf-8"),
            save_dir,
        )
    });

    write_response_line(&mut server, &save_frame.to_wire_message()).expect("savefile should send");
    write_response_line(&mut server, r#"@response {"id":7,"value":0}"#)
        .expect("final response should send");
    server
        .shutdown(Shutdown::Both)
        .expect("server side should close cleanly");

    let (output, saved_dir) = worker.join().expect("worker should not panic");
    let saved_path = saved_dir.join("shot.jpg");

    assert!(output.contains("saved file:"));
    assert!(output.contains(r#"@response {"id":7,"value":0}"#));
    assert_eq!(
        fs::read(&saved_path).expect("saved file should exist"),
        b"ABC"
    );

    let _ = fs::remove_dir_all(saved_dir);
}

#[test]
fn receive_control_result_frames_should_save_multiple_savefiles_before_final_response() {
    let (client, mut server) = connected_pair();
    let save_dir = temp_directory("savefile-bundle");
    let image_frame = SaveFileFrame {
        request_id: Some(7),
        filename: "screenshot-123-virtual-desktop.jpg".to_owned(),
        mime: "image/jpeg".to_owned(),
        encoding: "base64".to_owned(),
        data: "QUJD".to_owned(),
        quality: Some(75),
        width: Some(100),
        height: Some(60),
    };
    let manifest_frame = SaveFileFrame {
        request_id: Some(7),
        filename: "screenshot-123-manifest.json".to_owned(),
        mime: "application/json".to_owned(),
        encoding: "base64".to_owned(),
        data: "eyJzY2hlbWEiOiJyZG9nLnNjcmVlbnNob3QudjEifQ==".to_owned(),
        quality: None,
        width: None,
        height: None,
    };

    let worker = thread::spawn(move || {
        let mut transport =
            ControlTransport::from_tcp_stream(client).expect("transport should wrap tcp stream");
        let mut output = Vec::new();
        receive_control_result_frames(
            &mut transport,
            &mut output,
            &save_dir,
            ControlResponseDisplay::Protocol,
        )
        .expect("client should consume screenshot bundle frames");
        (
            String::from_utf8(output).expect("output should be utf-8"),
            save_dir,
        )
    });

    write_response_line(&mut server, &image_frame.to_wire_message())
        .expect("image savefile should send");
    write_response_line(&mut server, &manifest_frame.to_wire_message())
        .expect("manifest savefile should send");
    write_response_line(
        &mut server,
        r#"@response {"id":7,"value":{"kind":"screenshot-bundle","layout":"composite","coordinate_space":"os-logical","image":"screenshot-123-virtual-desktop.jpg","manifest":"screenshot-123-manifest.json","display_count":2}}"#,
    )
    .expect("bundle response should send");
    server
        .shutdown(Shutdown::Both)
        .expect("server side should close cleanly");

    let (output, saved_dir) = worker.join().expect("worker should not panic");

    assert_eq!(output.matches("saved file:").count(), 2);
    assert!(output.contains("screenshot-bundle"));
    assert_eq!(
        fs::read(saved_dir.join("screenshot-123-virtual-desktop.jpg"))
            .expect("image file should exist"),
        b"ABC"
    );
    assert_eq!(
        fs::read_to_string(saved_dir.join("screenshot-123-manifest.json"))
            .expect("manifest file should exist"),
        r#"{"schema":"rdog.screenshot.v1"}"#
    );

    let _ = fs::remove_dir_all(saved_dir);
}

#[test]
fn control_receiver_should_escape_double_at_to_literal_shell_command() {
    let (mut client, server) = connected_pair();
    let executor = FakeExecutor::default();
    let (shell, request_line, expected_fragment, cleanup_path) =
        escaped_literal_shell_case("shell-unit");
    let worker = thread::spawn(move || {
        run_control_receiver_with_executor(server, shell.as_str(), &executor)
    });

    client
        .write_all(&request_line)
        .expect("should write escaped shell line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read shell fallback output");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    if let Some(path) = cleanup_path.as_deref() {
        cleanup_temp_path(path);
    }

    assert!(output.contains(&expected_fragment));
}

#[test]
fn control_receiver_should_report_parse_failure_without_falling_back_to_shell() {
    let (mut client, server) = connected_pair();
    let executor = FakeExecutor::default();
    let shell = control_test_shell();
    let worker =
        thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

    client
        .write_all(br#"@script:"printf a\nb""#)
        .expect("should write invalid script payload");
    client
        .write_all(b"\n")
        .expect("should finish invalid control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read parse failure response");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish");

    assert!(output.contains(r#""code":64"#));
    assert!(output.contains("首版不支持多行 payload"));
    assert!(output.contains("@response {"));
}

#[test]
fn control_receiver_should_report_executor_failure_with_return_object() {
    struct AlwaysFailingExecutor;

    impl ControlActionExecutor for AlwaysFailingExecutor {
        fn execute(
            &self,
            _command: &ControlCommand,
            _shell: &str,
        ) -> io::Result<ActionExecutionResult> {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "首版不支持的 @key 按键: hyper",
            ))
        }
    }

    let (mut client, server) = connected_pair();
    let shell = control_test_shell();
    let worker = thread::spawn(move || {
        run_control_receiver_with_executor(server, shell, &AlwaysFailingExecutor)
    });

    client
        .write_all(br#"@key:"hyper""#)
        .expect("should write unsupported key request");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read executor failure response");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish after reporting failure");

    assert!(output.contains("首版不支持的 @key 按键: hyper"));
    assert!(output.contains(r#""code":64"#));
    assert!(output.contains("@response {"));
}

#[test]
fn control_receiver_should_wrap_executor_failure_with_request_id() {
    struct AlwaysFailingExecutor;

    impl ControlActionExecutor for AlwaysFailingExecutor {
        fn execute(
            &self,
            _command: &ControlCommand,
            _shell: &str,
        ) -> io::Result<ActionExecutionResult> {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "首版不支持的 @key 按键: hyper",
            ))
        }
    }

    let (mut client, server) = connected_pair();
    let shell = control_test_shell();
    let worker = thread::spawn(move || {
        run_control_receiver_with_executor(server, shell, &AlwaysFailingExecutor)
    });

    client
        .write_all(br#"@key#42:"hyper""#)
        .expect("should write unsupported key request with id");
    client.write_all(b"\n").expect("should finish control line");
    client
        .shutdown(Shutdown::Write)
        .expect("should close write side");

    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .expect("should read executor failure response with id");

    worker
        .join()
        .expect("worker should not panic")
        .expect("control receiver should finish after reporting failure");

    assert!(output.contains(r#""id":42"#));
    assert!(output.contains(r#""code":64"#));
    assert!(output.contains("首版不支持的 @key 按键: hyper"));
}
