use serde::Serialize;
use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};

pub const CAPABILITIES_SCHEMA: &str = "rdog.capabilities.v1";

/// 生成当前 daemon 可直接返回给 control peer 的能力报告。
///
/// 这份报告是 Phase 4 的单一真相源:
/// - 协议层 `@capabilities` 直接返回它
/// - 后续 `rdog doctor` 也应该复用同一份模型
/// - GUI agent recipe 只消费结构化字段,不再靠平台名字猜能力
pub fn current_capabilities_report_json() -> io::Result<String> {
    let report = build_capabilities_report(current_probe_snapshot());
    serde_json::to_string(&report).map_err(|err| io::Error::other(err.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CapabilitiesReport {
    kind: &'static str,
    schema: &'static str,
    status: &'static str,
    observed_at_unix_ms: u64,
    platform: PlatformDescriptor,
    capabilities: CapabilitySet,
    gui_agent_recipe: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PlatformDescriptor {
    os: String,
    family: String,
    arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CapabilitySet {
    line_control: CapabilityEntry,
    shell_command: CapabilityEntry,
    savefile_receiver: CapabilityEntry,
    pty: CapabilityEntry,
    zenoh_session_channel: CapabilityEntry,
    screenshot: CapabilityEntry,
    keyboard_input: CapabilityEntry,
    mouse_input: CapabilityEntry,
    accessibility: CapabilityEntry,
    window_control: CapabilityEntry,
    type_text: CapabilityEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CapabilityEntry {
    status: CapabilityStatus,
    backend: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<u16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    permissions: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    failure_hints: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notes: Vec<&'static str>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CapabilityStatus {
    Available,
    PermissionDenied,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapabilityProbeSnapshot {
    os_kind: CapabilityOs,
    platform: PlatformDescriptor,
    macos_accessibility: PermissionProbe,
    macos_screen_recording: PermissionProbe,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum CapabilityOs {
    Macos,
    Windows,
    Linux,
    Other,
}

// ------------------------------------------------------------
// 这些 probe 结果会被不同 target_os 的 cfg 分支构造。
// 在单一平台编译时,部分分支天然只会出现在测试或其他平台构建里。
// ------------------------------------------------------------
#[allow(dead_code)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum PermissionProbe {
    Granted,
    Denied,
    NotApplicable,
    Unknown,
}

fn build_capabilities_report(snapshot: CapabilityProbeSnapshot) -> CapabilitiesReport {
    let capabilities = CapabilitySet {
        line_control: available(
            "line-control",
            vec!["explicit @command parser and bare one-shot shell lines are available"],
        ),
        shell_command: available(
            "process-shell",
            vec!["@cmd, @script, and bare shell lines execute as one-shot commands"],
        ),
        savefile_receiver: available(
            "control-frame-savefile",
            vec!["@savefile frames can be delivered before the final @response"],
        ),
        pty: pty_capability(&snapshot),
        zenoh_session_channel: available(
            "zenoh-session-channel",
            vec!["rich control should use to-daemon/to-control session channels"],
        ),
        screenshot: screenshot_capability(&snapshot),
        keyboard_input: keyboard_input_capability(&snapshot),
        mouse_input: mouse_input_capability(&snapshot),
        accessibility: accessibility_capability(&snapshot),
        window_control: window_control_capability(&snapshot),
        type_text: type_text_capability(&snapshot),
    };

    let status = if report_has_permission_denied(&capabilities) {
        "degraded"
    } else {
        "complete"
    };

    CapabilitiesReport {
        kind: "capabilities",
        schema: CAPABILITIES_SCHEMA,
        status,
        observed_at_unix_ms: now_unix_ms(),
        platform: snapshot.platform,
        capabilities,
        gui_agent_recipe: vec![
            "@capabilities",
            "observe",
            "locate",
            "activate_or_focus",
            "semantic_action",
            "verify",
            "fallback_recipe",
        ],
    }
}

fn report_has_permission_denied(capabilities: &CapabilitySet) -> bool {
    [
        &capabilities.screenshot,
        &capabilities.keyboard_input,
        &capabilities.mouse_input,
        &capabilities.accessibility,
        &capabilities.window_control,
        &capabilities.type_text,
    ]
    .iter()
    .any(|entry| entry.status == CapabilityStatus::PermissionDenied)
}

fn pty_capability(snapshot: &CapabilityProbeSnapshot) -> CapabilityEntry {
    if snapshot.platform.family == "unix" {
        return available(
            "portable-pty",
            vec!["@pty, @pty-detach, @pty-attach, and @pty-close are supported"],
        );
    }

    unsupported(
        "portable-pty",
        vec!["@pty runtime is currently implemented for Unix hosts"],
    )
}

fn screenshot_capability(snapshot: &CapabilityProbeSnapshot) -> CapabilityEntry {
    match snapshot.os_kind {
        CapabilityOs::Macos => gated_by_permission(
            "sck-rs-then-xcap",
            snapshot.macos_screen_recording,
            vec!["macos.screen-recording"],
            vec!["macOS must grant Screen Recording permission to the actual rdog daemon process"],
            vec!["default screenshot returns composite JPEG plus manifest JSON"],
        ),
        CapabilityOs::Windows => available(
            "xcap-windows",
            vec!["runtime capture can still fail if the desktop session is unavailable"],
        ),
        CapabilityOs::Linux => available(
            "xcap-linux",
            vec!["runtime capture depends on the active display server and session permissions"],
        ),
        CapabilityOs::Other => unknown(
            "xcap",
            vec!["platform backend is compiled through xcap but has no preflight probe"],
        ),
    }
}

fn keyboard_input_capability(snapshot: &CapabilityProbeSnapshot) -> CapabilityEntry {
    match snapshot.os_kind {
        CapabilityOs::Macos => gated_by_permission(
            "enigo-global-input-simulation",
            snapshot.macos_accessibility,
            vec!["macos.accessibility"],
            vec!["macOS must grant Accessibility permission to the actual rdog daemon process"],
            vec!["@key should be treated as hotkey/function/navigation input"],
        ),
        CapabilityOs::Windows => unknown(
            "enigo-windows-input",
            vec!["Windows UIPI is target-window dependent; failures should map to code 77"],
        )
        .with_permissions(vec!["windows.uipi"]),
        CapabilityOs::Linux => unknown(
            "enigo-linux-input",
            vec!["Linux input injection depends on the display backend and session policy"],
        )
        .with_permissions(vec!["linux.display-input-policy"]),
        CapabilityOs::Other => unknown(
            "enigo",
            vec!["input backend has no platform-specific preflight probe"],
        ),
    }
}

fn mouse_input_capability(snapshot: &CapabilityProbeSnapshot) -> CapabilityEntry {
    match snapshot.os_kind {
        CapabilityOs::Macos => gated_by_permission(
            "enigo-global-pointer-simulation",
            snapshot.macos_accessibility,
            vec!["macos.accessibility"],
            vec!["macOS must grant Accessibility permission before mouse move/click/drag/wheel"],
            vec!["positioned mouse actions must use screenshot manifest os-logical coordinates"],
        ),
        CapabilityOs::Windows => unknown(
            "enigo-windows-pointer",
            vec!["Windows UIPI can block pointer input into elevated target windows with code 77"],
        )
        .with_permissions(vec!["windows.uipi"]),
        CapabilityOs::Linux => unknown(
            "enigo-linux-pointer",
            vec!["negative os-logical multi-display coordinates are not promised on Linux"],
        )
        .with_permissions(vec!["linux.display-input-policy"]),
        CapabilityOs::Other => unknown(
            "enigo",
            vec!["pointer backend has no platform-specific preflight probe"],
        ),
    }
}

fn accessibility_capability(snapshot: &CapabilityProbeSnapshot) -> CapabilityEntry {
    match snapshot.os_kind {
        CapabilityOs::Macos => gated_by_permission(
            "macos-accessibility",
            snapshot.macos_accessibility,
            vec!["macos.accessibility"],
            vec!["AX tree, AX actions, AX focus, and window control require Accessibility"],
            vec!["@screenshot include_ax can degrade AX metadata when ax_required is false"],
        ),
        _ => unsupported(
            "macos-accessibility",
            vec!["AX semantic UI control is currently implemented only on macOS"],
        ),
    }
}

fn window_control_capability(snapshot: &CapabilityProbeSnapshot) -> CapabilityEntry {
    match snapshot.os_kind {
        CapabilityOs::Macos => gated_by_permission(
            "macos-accessibility-window-state",
            snapshot.macos_accessibility,
            vec!["macos.accessibility"],
            vec![
                "@window-find, @window-activate, and graceful @window-close read or operate AX state",
            ],
            vec!["use @window-find before interacting with hidden/minimized/occluded windows"],
        ),
        _ => unsupported(
            "macos-accessibility-window-state",
            vec!["structured window control is currently implemented only on macOS"],
        ),
    }
}

fn type_text_capability(snapshot: &CapabilityProbeSnapshot) -> CapabilityEntry {
    match snapshot.os_kind {
        CapabilityOs::Macos => gated_by_permission(
            "macos-axvalue-targeted-keyboard-clipboard",
            snapshot.macos_accessibility,
            vec!["macos.accessibility"],
            vec!["AXValue and targeted-keyboard text delivery need Accessibility"],
            vec!["clipboard mode is opt-in and should report clipboard restore status"],
        ),
        _ => unsupported(
            "macos-type-text",
            vec!["type-text modes are currently implemented only on macOS"],
        ),
    }
}

fn gated_by_permission(
    backend: &'static str,
    probe: PermissionProbe,
    permissions: Vec<&'static str>,
    failure_hints: Vec<&'static str>,
    notes: Vec<&'static str>,
) -> CapabilityEntry {
    match probe {
        PermissionProbe::Granted => CapabilityEntry {
            status: CapabilityStatus::Available,
            backend,
            error_code: None,
            permissions,
            failure_hints: Vec::new(),
            notes,
        },
        PermissionProbe::Denied => CapabilityEntry {
            status: CapabilityStatus::PermissionDenied,
            backend,
            error_code: Some(77),
            permissions,
            failure_hints,
            notes,
        },
        PermissionProbe::NotApplicable => unsupported(backend, notes),
        PermissionProbe::Unknown => unknown(backend, notes).with_permissions(permissions),
    }
}

fn available(backend: &'static str, notes: Vec<&'static str>) -> CapabilityEntry {
    CapabilityEntry {
        status: CapabilityStatus::Available,
        backend,
        error_code: None,
        permissions: Vec::new(),
        failure_hints: Vec::new(),
        notes,
    }
}

fn unsupported(backend: &'static str, notes: Vec<&'static str>) -> CapabilityEntry {
    CapabilityEntry {
        status: CapabilityStatus::Unsupported,
        backend,
        error_code: Some(78),
        permissions: Vec::new(),
        failure_hints: Vec::new(),
        notes,
    }
}

fn unknown(backend: &'static str, notes: Vec<&'static str>) -> CapabilityEntry {
    CapabilityEntry {
        status: CapabilityStatus::Unknown,
        backend,
        error_code: None,
        permissions: Vec::new(),
        failure_hints: Vec::new(),
        notes,
    }
}

impl CapabilityEntry {
    fn with_permissions(mut self, permissions: Vec<&'static str>) -> Self {
        self.permissions = permissions;
        self
    }
}

fn current_probe_snapshot() -> CapabilityProbeSnapshot {
    CapabilityProbeSnapshot {
        os_kind: current_os_kind(),
        platform: PlatformDescriptor {
            os: std::env::consts::OS.to_owned(),
            family: current_family().to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
        },
        macos_accessibility: probe_macos_accessibility_permission(),
        macos_screen_recording: probe_macos_screen_recording_permission(),
    }
}

fn current_os_kind() -> CapabilityOs {
    if cfg!(target_os = "macos") {
        CapabilityOs::Macos
    } else if cfg!(target_os = "windows") {
        CapabilityOs::Windows
    } else if cfg!(target_os = "linux") {
        CapabilityOs::Linux
    } else {
        CapabilityOs::Other
    }
}

fn current_family() -> &'static str {
    if cfg!(target_family = "unix") {
        "unix"
    } else if cfg!(target_family = "windows") {
        "windows"
    } else {
        "other"
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(target_os = "macos")]
fn probe_macos_accessibility_permission() -> PermissionProbe {
    if unsafe { ax_is_process_trusted() != 0 } {
        PermissionProbe::Granted
    } else {
        PermissionProbe::Denied
    }
}

#[cfg(not(target_os = "macos"))]
fn probe_macos_accessibility_permission() -> PermissionProbe {
    PermissionProbe::NotApplicable
}

#[cfg(target_os = "macos")]
fn probe_macos_screen_recording_permission() -> PermissionProbe {
    if unsafe { cg_preflight_screen_capture_access() } {
        PermissionProbe::Granted
    } else {
        PermissionProbe::Denied
    }
}

#[cfg(not(target_os = "macos"))]
fn probe_macos_screen_recording_permission() -> PermissionProbe {
    PermissionProbe::NotApplicable
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    #[link_name = "AXIsProcessTrusted"]
    fn ax_is_process_trusted() -> u8;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    #[link_name = "CGPreflightScreenCaptureAccess"]
    fn cg_preflight_screen_capture_access() -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn report_should_distinguish_permission_denied_and_unsupported() {
        let report = build_capabilities_report(CapabilityProbeSnapshot {
            os_kind: CapabilityOs::Macos,
            platform: PlatformDescriptor {
                os: "macos".to_owned(),
                family: "unix".to_owned(),
                arch: "aarch64".to_owned(),
            },
            macos_accessibility: PermissionProbe::Denied,
            macos_screen_recording: PermissionProbe::Denied,
        });

        assert_eq!(
            report.capabilities.accessibility.status,
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(report.capabilities.accessibility.error_code, Some(77));
        assert_eq!(
            report.capabilities.screenshot.status,
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(report.capabilities.screenshot.error_code, Some(77));

        let unsupported_report = build_capabilities_report(CapabilityProbeSnapshot {
            os_kind: CapabilityOs::Windows,
            platform: PlatformDescriptor {
                os: "windows".to_owned(),
                family: "windows".to_owned(),
                arch: "x86_64".to_owned(),
            },
            macos_accessibility: PermissionProbe::NotApplicable,
            macos_screen_recording: PermissionProbe::NotApplicable,
        });

        assert_eq!(
            unsupported_report.capabilities.accessibility.status,
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            unsupported_report.capabilities.accessibility.error_code,
            Some(78)
        );
        assert_eq!(
            unsupported_report.capabilities.pty.status,
            CapabilityStatus::Unsupported
        );
    }

    #[test]
    fn report_json_should_expose_gui_recipe_and_schema() {
        let report = build_capabilities_report(CapabilityProbeSnapshot {
            os_kind: CapabilityOs::Linux,
            platform: PlatformDescriptor {
                os: "linux".to_owned(),
                family: "unix".to_owned(),
                arch: "x86_64".to_owned(),
            },
            macos_accessibility: PermissionProbe::NotApplicable,
            macos_screen_recording: PermissionProbe::NotApplicable,
        });
        let json = serde_json::to_string(&report).expect("report should serialize");
        let value: Value = serde_json::from_str(&json).expect("report should be valid json");

        assert_eq!(value["kind"], "capabilities");
        assert_eq!(value["schema"], CAPABILITIES_SCHEMA);
        assert_eq!(value["capabilities"]["pty"]["status"], "available");
        assert_eq!(value["gui_agent_recipe"][0], "@capabilities");
        assert_eq!(
            value["gui_agent_recipe"][4], "semantic_action",
            "recipe should make semantic action the primary act step"
        );
    }

    #[test]
    fn unknown_permission_probe_should_remain_structured() {
        let report = build_capabilities_report(CapabilityProbeSnapshot {
            os_kind: CapabilityOs::Macos,
            platform: PlatformDescriptor {
                os: "macos".to_owned(),
                family: "unix".to_owned(),
                arch: "aarch64".to_owned(),
            },
            macos_accessibility: PermissionProbe::Unknown,
            macos_screen_recording: PermissionProbe::Granted,
        });

        assert_eq!(
            report.capabilities.keyboard_input.status,
            CapabilityStatus::Unknown
        );
        assert_eq!(
            report.capabilities.keyboard_input.permissions,
            vec!["macos.accessibility"]
        );
        assert_eq!(
            report.capabilities.screenshot.status,
            CapabilityStatus::Available
        );
    }
}
