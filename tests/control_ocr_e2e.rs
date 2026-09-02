//! OCR 内容层 live 三件套 e2e (specs/rdog-ocr-content-layer-plan.md §9)。
//!
//! 门控: `RDOG_OCR_LIVE_E2E=1` 时才运行 (opt-in, 与 RDOG_LIVE_* 家族一致);
//! 未设置时静默返回, 设置后所有前置缺失一律 panic (显式失败而非跳过)。
//! `RDOG_OCR_LIVE_E2E_VIA_TERMINAL=1` 时由 Terminal.app 承载 daemon
//! (agent shell 直接 spawn 的进程没有 Screen Recording 权限归属, 见
//! control_ax_e2e 同款方案)。
//!
//! 场景: macOS 计算器 (无用户数据风险, 动作结果 OCR 可见)。
//!   1. 读: `@window-find` 拿 window_id -> 窗口裁剪截图 include_ocr -> ocr 层可定位按钮
//!   2. 定位+动作: 按 OCR 文本框坐标点击数字对 (os-logical 坐标直接喂 @click)
//!   3. 验证: fresh 窗口截图 OCR 层出现两位显示结果 (且初始层没有该框)
//!   负例: 不存在的文本定位必须失败, 从而不产生任何点击。
//!
//! live 实测沉淀的三条纪律 (2026-09-02):
//!   - 每次截图前重新激活目标窗口: 被遮挡的窗口不出现在 screen-composited
//!     截图里, OCR 框数骤降说明截到的是遮挡物;
//!   - 数字定位带 conf>=0.5 过滤 + 只在显示框下方的按钮区域找 (显示区残留值
//!     会污染同名按钮定位) + 重拍重试 (单帧识别对个别按钮有抖动);
//!   - daemon 二进制每次重编后必须按 scripts/install-signed.sh 的身份重签
//!     (identifier=rdog), 否则 TCC 授权失效, 截图管线静默失败。
//!
//! 运行前置: daemon 二进制已有 Accessibility TCC; 模型缓存经 OAR_HOME 注入
//! (spec §8: e2e spawn 点必须显式注入, 不依赖默认 ~/.oar)。

#![cfg(target_os = "macos")]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

#[path = "control_mouse_e2e/support.rs"]
#[allow(dead_code)]
mod support;

use support::{
    next_free_port, rdog_binary_path, temp_workdir, wait_until_port_is_busy, ControlSession,
};

const LIVE_OCR_E2E_ENV: &str = "RDOG_OCR_LIVE_E2E";
const LIVE_OCR_E2E_VIA_TERMINAL_ENV: &str = "RDOG_OCR_LIVE_E2E_VIA_TERMINAL";
const OAR_HOME_ENV: &str = "OAR_HOME";
const WAIT_TIMEOUT: Duration = Duration::from_secs(90);

fn live_ocr_e2e_enabled() -> bool {
    matches!(
        std::env::var(LIVE_OCR_E2E_ENV).ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

fn terminal_host_enabled() -> bool {
    matches!(
        std::env::var(LIVE_OCR_E2E_VIA_TERMINAL_ENV).ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

fn shell_quote(value: &Path) -> String {
    let value = value.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn port_is_listening(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if port_is_listening(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// 直接 spawn 的 daemon (无 Terminal 时用; 需要 rdog 二进制自身已有 TCC 授权)。
struct OcrDaemonGuard {
    child: Child,
}

impl OcrDaemonGuard {
    fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for OcrDaemonGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_ocr_daemon(binary: &Path, workdir: &Path, port: u16, oar_home: &Path) -> OcrDaemonGuard {
    // stderr 落盘: daemon 崩溃/被 TCC kill 时留有现场可查
    let log = fs::File::create(workdir.join("daemon-stderr.log"))
        .expect("daemon stderr log should create");
    OcrDaemonGuard {
        child: Command::new(binary)
            .arg("daemon")
            .env("RDOG_ZENOH__ENABLED", "false")
            .env("RDOG_OBSERVATION__DURABLE_ENABLED", "false")
            .env("RDOG_OUTBOUND__ENABLED", "false")
            .env("RDOG_INBOUND__ENABLED", "true")
            .env("RDOG_INBOUND__HOST", "127.0.0.1")
            .env("RDOG_INBOUND__PORT", port.to_string())
            .env("RDOG_INBOUND__SHELL", "/bin/sh")
            .env("RDOG_INBOUND__MODE", "control")
            .env(OAR_HOME_ENV, oar_home)
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .expect("daemon should start"),
    }
}

/// Terminal 承载的 daemon: 权限归属已授权的 Terminal.app。
struct TerminalDaemon {
    port: u16,
    log_path: PathBuf,
}

impl TerminalDaemon {
    fn startup_log(&self) -> String {
        fs::read_to_string(&self.log_path)
            .unwrap_or_else(|err| format!("无法读取 daemon log {}: {err}", self.log_path.display()))
    }

    fn stop(&self) {
        let output = Command::new("lsof")
            .args(["-ti", &format!("tcp:{0}", self.port)])
            .output();
        if let Ok(output) = output {
            for pid in String::from_utf8_lossy(&output.stdout).split_whitespace() {
                let _ = Command::new("kill").arg(pid).status();
            }
        }
    }
}

impl Drop for TerminalDaemon {
    fn drop(&mut self) {
        self.stop();
    }
}

fn start_terminal_ocr_daemon(
    binary: &Path,
    workdir: &Path,
    port: u16,
    oar_home: &Path,
) -> TerminalDaemon {
    let script_path = std::env::temp_dir().join(format!("rdog-ocr-e2e-{port}.command"));
    let log_path = std::env::temp_dir().join(format!("rdog-ocr-e2e-{port}.log"));
    let script = format!(
        "#!/bin/zsh\n\
         cd {}\n\
         export RDOG_ZENOH__ENABLED=false\n\
         export RDOG_OBSERVATION__DURABLE_ENABLED=false\n\
         export RDOG_OUTBOUND__ENABLED=false\n\
         export RDOG_INBOUND__ENABLED=true\n\
         export RDOG_INBOUND__HOST=127.0.0.1\n\
         export RDOG_INBOUND__PORT={port}\n\
         export RDOG_INBOUND__SHELL=/bin/sh\n\
         export RDOG_INBOUND__MODE=control\n\
         export OAR_HOME={}\n\
         exec {} daemon > {} 2>&1\n",
        shell_quote(workdir),
        shell_quote(oar_home),
        shell_quote(binary),
        shell_quote(&log_path)
    );
    fs::write(&script_path, script).expect("terminal daemon script should write");
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&script_path)
        .expect("terminal daemon script metadata should exist")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).expect("terminal daemon script should be executable");

    let status = Command::new("open")
        .args(["-a", "Terminal"])
        .arg(&script_path)
        .status()
        .expect("open -a Terminal should run");
    assert!(
        status.success(),
        "open -a Terminal should accept daemon command script"
    );

    TerminalDaemon { port, log_path }
}

fn quit_calculator() {
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(r#"tell application "Calculator" to quit"#)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn activate_calculator() {
    // 被遮挡的窗口不出现在 screen-composited 截图里, 每次截图前必须置前
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(r#"tell application "Calculator" to activate"#)
        .status();
}

/// 最新一张截图的 manifest (客户端把 @savefile 落到 workdir/rdog_downloads)。
fn read_latest_screenshot_manifest(workdir: &Path) -> serde_json::Value {
    let download_dir = workdir.join("rdog_downloads");
    let mut manifests = fs::read_dir(&download_dir)
        .expect("download directory should exist after screenshot")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    manifests.sort();
    let manifest_path = manifests
        .pop()
        .unwrap_or_else(|| panic!("expected at least one screenshot manifest in {download_dir:?}"));
    let manifest_text = fs::read_to_string(&manifest_path).expect("manifest should be readable");
    serde_json::from_str(&manifest_text).expect("manifest should be valid json")
}

/// 从会话输出里取最后一个 `@response {"id":<id>,...}` 的 value 对象。
fn last_response_value(output: &str, request_id: u64) -> serde_json::Value {
    let needle = format!(r#"@response {{"id":{request_id}"#);
    let line = output
        .lines()
        .rev()
        .find(|line| line.starts_with(&needle))
        .unwrap_or_else(|| panic!("output should contain response for id {request_id}:\n{output}"));
    let json_text = &line["@response ".len()..];
    let value: serde_json::Value =
        serde_json::from_str(json_text).expect("response line should be valid json");
    value["value"].clone()
}

/// @window-find 拿 Calculator 主窗口的 window_id。
fn find_calculator_window(session: &mut ControlSession) -> String {
    session.send("@window-find#700:{app:\"Calculator\",limit:5}\n");
    let output = session.wait_for_all("window-find Calculator", &[r#""id":700"#], WAIT_TIMEOUT);
    let value = last_response_value(&output, 700);
    assert!(
        value.get("code").is_none(),
        "window-find 不应报错: {value}"
    );
    // 不能取 matches[0]: 鼠标悬停在按钮上时 AX 会把 tooltip 注册成窗口且
    // 排在前面 (interactable:false), 必须选可交互的真窗口 (#102 实测)。
    let matches = value["matches"]
        .as_array()
        .expect("window-find should return matches");
    let window_id = matches
        .iter()
        .find(|m| m["state"]["interactable"] == serde_json::Value::Bool(true))
        .or_else(|| matches.first())
        .and_then(|m| m["window_id"].as_str())
        .expect("window-find should return an interactable window_id")
        .to_owned();
    assert!(!window_id.is_empty(), "window_id should not be empty");
    window_id
}

/// 发送窗口级 include_ocr 截图并返回带 ocr 层的 window manifest。
/// 每次截图前重新激活目标窗口 (遮挡窗口不会出现在 composite 里)。
fn capture_window_with_ocr(
    session: &mut ControlSession,
    workdir: &Path,
    window_id: &str,
) -> serde_json::Value {
    activate_calculator();
    session.send(&format!(
        r#"@screenshot:{{target:"window",window:{{window_id:"{window_id}"}},include_ocr:true,include_ax:false}}"#
    ));
    session.send("\n");
    session.wait_for_all(
        "window screenshot with ocr layer",
        &["window-screenshot"],
        WAIT_TIMEOUT,
    );
    let manifest = read_latest_screenshot_manifest(workdir);
    // 框数骤降说明截到的是遮挡物, 置前后重拍一次
    let box_count = manifest["ocr"]["boxes"].as_array().map_or(0, Vec::len);
    if box_count < 8 {
        activate_calculator();
        std::thread::sleep(Duration::from_millis(300));
        session.send(&format!(
            r#"@screenshot:{{target:"window",window:{{window_id:"{window_id}"}},include_ocr:true,include_ax:false}}"#
        ));
        session.send("\n");
        session.wait_for_all(
            "window screenshot retry",
            &["window-screenshot"],
            WAIT_TIMEOUT,
        );
        return read_latest_screenshot_manifest(workdir);
    }
    assert_eq!(
        manifest["schema"].as_str(),
        Some("rdog.screenshot.window.v1"),
        "窗口截图应返回 window manifest"
    );
    assert_eq!(
        manifest["ocr"]["schema"].as_str(),
        Some("rdog.ocr.v1"),
        "include_ocr 请求必须返回 rdog.ocr.v1 层 (缺层会被误读成\"没有文字\", fail-closed)"
    );
    assert_eq!(
        manifest["ocr"]["engine"].as_str(),
        Some("oar"),
        "OCR 引擎应为 oar (spec v1.1)"
    );
    manifest
}

/// 在按钮区域按全等文本定位文本框 (conf>=0.5 过滤误读框),
/// 返回框中心 os-logical 坐标。显示区残留值 (Calculator 重开恢复上次显示)
/// 会污染同名按钮定位, 因此只在最上方显示框之下找按钮。
/// 在截图 OCR 层的最顶行 (显示区) 查找以 `suffix` 结尾的文本, 返回框中心。
/// 与 locate_button 互补: fresh 点击验证要找的是显示结果, 而显示区
/// 恰是 locate_button 的排除区 (top 行), 用错函数会永远 miss (#102 根因)。
/// 用 ends_with 而非全等: Calculator 的表达式状态会跨运行持久化 (清
/// savedState 也不退), 显示区常带历史残留前缀, 本轮点击生效的证据是
/// "显示值以本轮 marker 结尾"。
fn locate_display_text(manifest: &serde_json::Value, suffix: &str) -> Option<(i32, i32)> {
    let boxes = manifest["ocr"]["boxes"].as_array()?;
    if boxes.is_empty() {
        return None;
    }
    let top = boxes
        .iter()
        .filter_map(|b| b["bbox"][1].as_i64())
        .min()?;
    let top_bottom = boxes
        .iter()
        .filter_map(|b| {
            let y = b["bbox"][1].as_i64()?;
            let h = b["bbox"][3].as_i64()?;
            (y == top).then_some(y + h)
        })
        .max()?;
    for box_ in boxes {
        let Some(box_text) = box_["text"].as_str() else {
            continue;
        };
        if !box_text.trim().ends_with(suffix) {
            continue;
        }
        let confidence = box_["confidence"].as_f64()?;
        if confidence < 0.5 {
            continue;
        }
        let bbox = box_["bbox"].as_array()?;
        let y = bbox[1].as_i64()?;
        if y > top_bottom {
            continue; // 按钮区, 不是显示区
        }
        let x = bbox[0].as_i64()?;
        let w = bbox[2].as_i64()?;
        let h = bbox[3].as_i64()?;
        let center_x = i32::try_from(x + w / 2).ok()?;
        let center_y = i32::try_from(y + h / 2).ok()?;
        return Some((center_x, center_y));
    }
    None
}

fn locate_button(manifest: &serde_json::Value, text: &str) -> Option<(i32, i32)> {
    let boxes = manifest["ocr"]["boxes"].as_array()?;
    if boxes.is_empty() {
        return None;
    }
    let top = boxes
        .iter()
        .filter_map(|b| b["bbox"][1].as_i64())
        .min()?;
    let top_bottom = boxes
        .iter()
        .filter_map(|b| {
            let y = b["bbox"][1].as_i64()?;
            let h = b["bbox"][3].as_i64()?;
            (y == top).then_some(y + h)
        })
        .max()?;
    for box_ in boxes {
        let Some(box_text) = box_["text"].as_str() else {
            continue;
        };
        if box_text.trim() != text {
            continue;
        }
        let confidence = box_["confidence"].as_f64()?;
        if confidence < 0.5 {
            continue;
        }
        let bbox = box_["bbox"].as_array()?;
        let x = bbox[0].as_i64()?;
        let y = bbox[1].as_i64()?;
        if y <= top_bottom {
            continue; // 显示区/标题区, 不是按钮
        }
        let w = bbox[2].as_i64()?;
        let h = bbox[3].as_i64()?;
        let center_x = i32::try_from(x + w / 2).ok()?;
        let center_y = i32::try_from(y + h / 2).ok()?;
        return Some((center_x, center_y));
    }
    None
}

/// 带重拍的捕获+定位: 对多个目标文本同时定位, 全部命中才成功;
/// 未全中返回 Err (调用方决定是否重试)。
fn capture_and_locate(
    session: &mut ControlSession,
    workdir: &Path,
    window_id: &str,
    targets: &[&str],
    attempts: u32,
) -> Result<(serde_json::Value, Vec<Option<(i32, i32)>>), String> {
    for _ in 0..attempts {
        let manifest = capture_window_with_ocr(session, workdir, window_id);
        let located: Vec<Option<(i32, i32)>> =
            targets.iter().map(|t| locate_button(&manifest, t)).collect();
        if located.iter().all(Option::is_some) {
            return Ok((manifest, located));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Err(format!("{attempts} 次重拍后仍未全部定位 {targets:?}"))
}

/// 从候选数字对里找一对当前都能可靠定位的按钮。
/// 单帧 OCR 对个别按钮有漏检/误读抖动 (实测 "2" 偶发误读, conf 0.12),
/// 候选列表 + 重拍让流程对单帧抖动稳健。
fn locate_digit_pair(
    session: &mut ControlSession,
    workdir: &Path,
    window_id: &str,
) -> ((i32, i32), (i32, i32), String, serde_json::Value) {
    for [first, second] in [["1", "3"], ["7", "5"], ["4", "9"]] {
        for _ in 0..2 {
            let initial = capture_window_with_ocr(session, workdir, window_id);
            if let (Some(a), Some(b)) = (
                locate_button(&initial, first),
                locate_button(&initial, second),
            ) {
                let marker = format!("{first}{second}");
                return ((a.0, a.1), (b.0, b.1), marker, initial);
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    panic!("候选数字对 (1,3)/(7,5)/(4,9) 全部无法定位; 识别质量不足以支撑按文本点击");
}

/// e2e 前置严格校验: 门控开启后任何前置缺失都 panic, 不静默跳过。
fn require_prerequisites() -> PathBuf {
    let oar_home = std::env::var_os(OAR_HOME_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{OAR_HOME_ENV} 未设置: live OCR e2e 需要显式模型缓存目录"));
    assert!(
        oar_home.join("pp-ocrv6_tiny_det.onnx").exists()
            && oar_home.join("pp-ocrv6_tiny_rec.onnx").exists(),
        "模型缓存 {} 缺少 pp-ocrv6_tiny_*.onnx: 请先完成模型预下载",
        oar_home.display()
    );
    oar_home
}

#[test]
fn live_ocr_three_piece_calculator_flow() {
    if !live_ocr_e2e_enabled() {
        // opt-in 家族惯例: 未显式开启时静默返回。
        return;
    }
    let oar_home = require_prerequisites();

    // 干净起点: 先退出既有计算器 (稍后再打开, 避免 Terminal 窗口把它盖住)
    quit_calculator();
    std::thread::sleep(Duration::from_millis(500));

    let binary = rdog_binary_path();
    let workdir = temp_workdir("ocr-live");
    let port = next_free_port();
    let _terminal_daemon = if terminal_host_enabled() {
        let daemon = start_terminal_ocr_daemon(&binary, &workdir, port, &oar_home);
        assert!(
            wait_for_port(port, WAIT_TIMEOUT),
            "Terminal daemon should listen on {port}; log:\n{}",
            daemon.startup_log()
        );
        Some(daemon)
    } else {
        let mut daemon = start_ocr_daemon(&binary, &workdir, port, &oar_home);
        assert!(
            wait_until_port_is_busy(daemon.child_mut(), port, WAIT_TIMEOUT),
            "daemon should listen on {port}"
        );
        None
    };

    // daemon 就绪后再打开计算器: open 会激活并置前 (遮挡窗口不进 OCR 层)
    Command::new("open")
        .args(["-a", "Calculator"])
        .status()
        .expect("Calculator should launch");
    std::thread::sleep(Duration::from_millis(1200));

    let mut session = ControlSession::spawn(&binary, &workdir, port);

    // ---- 1. 读: 定位窗口 + 窗口裁剪 OCR ----
    let window_id = find_calculator_window(&mut session);

    // ---- 2/3. 定位+点击+验证, 失败整轮重试 (最多 3 轮) ----
    // 单轮可能失败于: 首次点击被窗口激活吞掉 / 个别按钮单帧误读。
    // 每轮以 AC 归零开始, 保证重试轮的状态干净 (上一轮残留数字会污染对照)。
    let mut after_clicks = None;
    let mut marker_final = String::new();
    let mut one_y_final = i32::MAX;
    for attempt in 1..=3 {
        // 鼠标残留会触发按钮 tooltip, 而 AX 会把 tooltip 注册成窗口并劫持
        // window rect (实测 215x18 的 tooltip rect 让 composite 裁到 tooltip
        // 文本, 后续 OCR 全 miss)。每轮开始先把鼠标移出计算器窗口。
        session.send(r#"@mouse-move#699:{x:200,y:600,coordinate_space:"os-logical"}"#);
        session.send("\n"); // 行协议以换行收口, 漏 \n 该行永不执行 (#102 实测)
        session.wait_for_all("move mouse away", &[r#""id":699"#], Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(300)); // 等已弹出的 tooltip 消失

        // AC 归零 (带重拍定位): 清掉上一轮残留
        if let Ok((_, located)) =
            capture_and_locate(&mut session, &workdir, &window_id, &["AC"], 2)
        {
            let (ac_x, ac_y) = located[0].expect("AC located");
            session.send(&format!(
                r#"@click#600:{{x:{ac_x},y:{ac_y},button:"left",count:1,hold_ms:80,coordinate_space:"os-logical"}}"#
            ));
            session.wait_for_all("AC reset click", &[r#""id":600"#], Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(400));
        }

        let ((one_x, one_y), (two_x, two_y), marker, initial) =
            locate_digit_pair(&mut session, &workdir, &window_id);
        // Calculator 的表达式显示会跨运行持久化 (savedState/defaults 清理都
        // 不退), 残留尾巴可能恰好以 marker 结尾, 使"初始不应已含结果"的强
        // 断言误判。此时点一个扰动数字改变末尾, 让"显示以 marker 结尾"重新
        // 成为本轮点击的证据, 而不是直接 fail。
        if locate_display_text(&initial, &marker).is_some() {
            let Some((nudge_x, nudge_y)) = locate_button(&initial, "8") else {
                panic!("初始显示已以 \"{marker}\" 结尾且扰动数字 8 不可定位");
            };
            session.send(&format!(
                r#"@click#604:{{x:{nudge_x},y:{nudge_y},button:"left",count:1,hold_ms:80,coordinate_space:"os-logical"}}"#
            ));
            session.send("\n");
            session.wait_for_all(
                "nudge click to break suffix collision",
                &[r#""id":604"#],
                Duration::from_secs(30),
            );
            std::thread::sleep(Duration::from_millis(400));
        }

        // 点击前显式置前: 首次点击若被窗口激活吞掉会少一位数字 (实测)。
        activate_calculator();
        std::thread::sleep(Duration::from_millis(300));
        session.send(&format!(
            r#"@click#601:{{x:{one_x},y:{one_y},button:"left",count:1,hold_ms:80,coordinate_space:"os-logical"}}
@click#602:{{x:{two_x},y:{two_y},button:"left",count:1,hold_ms:80,coordinate_space:"os-logical"}}
@cmd#603:"sleep 0.6"
"#
        ));
        let click_output = session.wait_for_all(
            "two ocr-guided clicks",
            &[r#""id":601"#, r#""id":602"#],
            Duration::from_secs(30),
        );
        assert!(
            !click_output.contains("\"code\":"),
            "click 应答不应是错误: {click_output}"
        );

        // fresh 截图仲裁: 显示结果出现才算本轮成功 (在显示区找, 不在按钮区)
        let after = capture_window_with_ocr(&mut session, &workdir, &window_id);
        // 诊断: 打印显示区与全部 OCR 文本, 失败轮可直接看到点击后的真实显示
        let display_texts: Vec<&str> = after["ocr"]["boxes"]
            .as_array()
            .map(|boxes| {
                boxes
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        eprintln!("[ocr-e2e] 第 {attempt} 轮点击后 OCR 全文: {display_texts:?}");
        let Some((display_x, display_y)) = locate_display_text(&after, &marker) else {
            eprintln!("[ocr-e2e] 第 {attempt} 轮未出现 \"{marker}\", 重试");
            continue;
        };
        // 显示区在按钮区上方: 结果框中心 y 应小于按钮框中心 y (空间合理性)
        assert!(
            display_y < one_y,
            "显示结果框应位于按钮区上方 (display_y={display_y} vs button_y={one_y})"
        );
        let _ = display_x;
        after_clicks = Some(after);
        marker_final = marker;
        one_y_final = one_y;
        break;
    }
    let after_clicks = after_clicks
        .unwrap_or_else(|| panic!("3 轮点击重试后仍未在 ocr 层看到显示结果"));

    // ---- 负例: 不存在的文本定位失败, 不产生任何点击 ----
    assert!(
        locate_button(&after_clicks, "rdog-ocr-nonexistent-marker").is_none(),
        "不存在的文本不应被定位到 (locate-miss 路径)"
    );
    // 定位失败 => 没有可执行动作 => 无需第三次截图;
    // 同时避免连续相同 composite 触发 SCREENSHOT_STALE_FRAME 守卫。
    let _ = (marker_final, one_y_final);

    session.send("@exit\n");
    quit_calculator();
}
