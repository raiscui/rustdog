use super::*;
use crate::control_ax::AxRect;
use std::{
    ffi::CString,
    io,
    os::raw::{c_char, c_int, c_void},
    process::Command,
    ptr,
};

type Boolean = u8;
type CFIndex = isize;
type CFTypeID = usize;
type CFTypeRef = *const c_void;
type CFStringRef = CFTypeRef;
type CFArrayRef = CFTypeRef;
type CFDictionaryRef = CFTypeRef;
type CFNumberRef = CFTypeRef;
type CFBooleanRef = CFTypeRef;
type AXUIElementRef = CFTypeRef;
type AXValueRef = CFTypeRef;
type AXError = i32;

const UTF8: u32 = 0x0800_0100;
const CF_NUMBER_SINT32: i32 = 3;
const AX_SUCCESS: AXError = 0;
const AX_ERROR_FAILURE: AXError = -25200;
const AX_ERROR_INVALID_UI_ELEMENT: AXError = -25202;
const AX_ERROR_CANNOT_COMPLETE: AXError = -25204;
const AX_ERROR_ATTRIBUTE_UNSUPPORTED: AXError = -25205;
const AX_ERROR_ACTION_UNSUPPORTED: AXError = -25206;
const AX_ERROR_NOT_IMPLEMENTED: AXError = -25208;
const AX_ERROR_API_DISABLED: AXError = -25211;
const AX_ERROR_NO_VALUE: AXError = -25212;
const AX_VALUE_CG_POINT: u32 = 1;
const AX_VALUE_CG_SIZE: u32 = 2;
const CG_WINDOW_ON_SCREEN_ONLY: u32 = 1 << 0;
const CG_WINDOW_EXCLUDE_DESKTOP: u32 = 1 << 4;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kCFBooleanFalse: CFBooleanRef;
    static kCGWindowBounds: CFStringRef;
    static kCGWindowLayer: CFStringRef;
    static kCGWindowName: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;

    fn AXIsProcessTrusted() -> Boolean;
    fn AXUIElementCreateApplication(pid: c_int) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXValueGetTypeID() -> CFTypeID;
    fn AXValueGetValue(value: AXValueRef, value_type: u32, value_ptr: *mut c_void) -> Boolean;
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> CFTypeRef;
    fn CFBooleanGetTypeID() -> CFTypeID;
    fn CFBooleanGetValue(boolean: CFBooleanRef) -> Boolean;
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    fn CFGetTypeID(value: CFTypeRef) -> CFTypeID;
    fn CFNumberGetTypeID() -> CFTypeID;
    fn CFNumberGetValue(number: CFNumberRef, number_type: i32, value_ptr: *mut c_void) -> Boolean;
    fn CFRelease(value: CFTypeRef);
    fn CFRetain(value: CFTypeRef) -> CFTypeRef;
    fn CFStringCreateWithCString(
        alloc: CFTypeRef,
        c_str: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: CFIndex,
        encoding: u32,
    ) -> Boolean;
    fn CFStringGetTypeID() -> CFTypeID;
    fn CGRectMakeWithDictionaryRepresentation(dict: CFDictionaryRef, rect: *mut CGRect) -> bool;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
}

#[derive(Debug)]
struct CfOwned(CFTypeRef);

impl CfOwned {
    fn new(value: CFTypeRef) -> Option<Self> {
        (!value.is_null()).then_some(Self(value))
    }

    unsafe fn retain(value: CFTypeRef) -> Option<Self> {
        if value.is_null() {
            return None;
        }
        Some(Self(CFRetain(value)))
    }

    fn as_ptr(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                CFRelease(self.0);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct RunningApp {
    pid: i32,
    name: String,
    bundle_id: Option<String>,
    hidden: bool,
    frontmost: bool,
}

#[derive(Debug, Clone)]
struct VisibleWindowInfo {
    pid: i32,
    title: Option<String>,
    rect: Option<AxRect>,
}

#[derive(Debug, Clone)]
struct ResolvedWindow {
    app: RunningApp,
    window_index: usize,
    title: Option<String>,
    rect: Option<AxRect>,
    minimized: bool,
    fullscreen_space: bool,
    focused: bool,
    current_space: bool,
    occluded: bool,
}

pub(super) fn find(request: &WindowFindRequest) -> io::Result<WindowFindResponse> {
    ensure_trusted()?;

    let meta = WindowSnapshotMeta::now();
    let mut candidates = enumerate_candidates(request.include_state, request.include_recipes)?;
    candidates.retain(|candidate| request.query.matches_candidate(candidate));
    let match_count = candidates.len();
    candidates.truncate(usize::from(request.limit));

    Ok(WindowFindResponse {
        kind: "window-find",
        schema: WINDOW_SCHEMA,
        platform: "macos".to_owned(),
        status: "complete".to_owned(),
        capabilities: WindowCapabilities::complete(),
        match_count,
        returned_count: candidates.len(),
        snapshot_id: meta.snapshot_id,
        observed_at_unix_ms: meta.observed_at_unix_ms,
        matches: candidates,
    })
}

pub(super) fn activate(request: &WindowActivateRequest) -> io::Result<WindowActionReport> {
    ensure_trusted()?;

    let find_response = find(&WindowFindRequest {
        query: request.target.query.clone(),
        limit: DEFAULT_WINDOW_FIND_LIMIT,
        include_state: true,
        include_recipes: true,
    })?;
    let (window, snapshot_id, observed_at_unix_ms) = resolve_single_window(
        &find_response,
        &request.target,
        request.allow_ambiguous,
        request.select,
        None,
    )?;

    let mut steps = Vec::<WindowActionStepReport>::new();
    let recipe_steps = resolve_activation_steps(request, &window);
    let app_element = create_app_ax_element(window.app.pid)?;
    let window_ref = resolve_window_ref(window.app.pid, window.window_index)?;

    for step in recipe_steps {
        let report = match step.as_str() {
            "unhide_app" => perform_unhide_app(&window),
            "unminimize_window" => perform_unminimize_window(window_ref.as_ptr(), &window),
            "activate_app" => perform_activate_app(&window),
            "raise_window" => perform_raise_window(window_ref.as_ptr()),
            "switch_to_window_space" => WindowActionStepReport {
                step,
                status: "limited".to_owned(),
                reason: Some("Phase 1 仅提供 recipe,不保证自动切换 Space".to_owned()),
                error: None,
            },
            unknown => WindowActionStepReport {
                step: unknown.to_owned(),
                status: "failed".to_owned(),
                reason: None,
                error: Some(format!("未知 activate step: {unknown}")),
            },
        };
        if report.status == "failed" {
            let failed_step = report.step.clone();
            steps.push(report);
            return Ok(WindowActionReport {
                kind: "window-action",
                schema: WINDOW_SCHEMA,
                platform: "macos".to_owned(),
                action: "activate",
                status: "failed".to_owned(),
                window_id: Some(window_id(window.app.pid, window.window_index)),
                snapshot_id: Some(snapshot_id),
                observed_at_unix_ms: Some(observed_at_unix_ms),
                strategy: None,
                target_pid: None,
                process_scope: None,
                termination_attempted: None,
                failed_step: Some(failed_step),
                steps,
            });
        }
        steps.push(report);
    }

    // ------------------------------------------------------------
    // 再做一轮轻量探测,给调用方一个“是否已经可交互”的新鲜结果。
    // ------------------------------------------------------------
    let hidden = copy_bool_attr(app_element.as_ptr(), "AXHidden")?.unwrap_or(window.app.hidden);
    let minimized = copy_bool_attr(window_ref.as_ptr(), "AXMinimized")?.unwrap_or(false);
    if hidden || minimized {
        return Ok(WindowActionReport {
            kind: "window-action",
            schema: WINDOW_SCHEMA,
            platform: "macos".to_owned(),
            action: "activate",
            status: "limited".to_owned(),
            window_id: Some(window_id(window.app.pid, window.window_index)),
            snapshot_id: Some(snapshot_id),
            observed_at_unix_ms: Some(observed_at_unix_ms),
            strategy: None,
            target_pid: None,
            process_scope: None,
            termination_attempted: None,
            failed_step: None,
            steps,
        });
    }

    Ok(WindowActionReport {
        kind: "window-action",
        schema: WINDOW_SCHEMA,
        platform: "macos".to_owned(),
        action: "activate",
        status: "ok".to_owned(),
        window_id: Some(window_id(window.app.pid, window.window_index)),
        snapshot_id: Some(snapshot_id),
        observed_at_unix_ms: Some(observed_at_unix_ms),
        strategy: None,
        target_pid: None,
        process_scope: None,
        termination_attempted: None,
        failed_step: None,
        steps,
    })
}

pub(super) fn close(request: &WindowCloseRequest) -> io::Result<WindowActionReport> {
    ensure_trusted()?;

    let find_response = find(&WindowFindRequest {
        query: request.target.query.clone(),
        limit: DEFAULT_WINDOW_FIND_LIMIT,
        include_state: true,
        include_recipes: true,
    })?;
    let strategy = request.strategy;
    let (window, snapshot_id, observed_at_unix_ms) = resolve_single_window(
        &find_response,
        &request.target,
        request.allow_ambiguous,
        request.select,
        Some(strategy),
    )?;
    let window_id = window_id(window.app.pid, window.window_index);

    match strategy {
        WindowCloseStrategy::Graceful => {
            let window_ref = resolve_window_ref(window.app.pid, window.window_index)?;
            let step = perform_graceful_close(window_ref.as_ptr())?;
            let status = if step.status == "ok" { "ok" } else { "failed" };
            Ok(WindowActionReport {
                kind: "window-action",
                schema: WINDOW_SCHEMA,
                platform: "macos".to_owned(),
                action: "close",
                status: status.to_owned(),
                window_id: Some(window_id),
                snapshot_id: Some(snapshot_id),
                observed_at_unix_ms: Some(observed_at_unix_ms),
                strategy: Some("graceful"),
                target_pid: None,
                process_scope: None,
                termination_attempted: Some(false),
                failed_step: (status == "failed").then(|| step.step.clone()),
                steps: vec![step],
            })
        }
        WindowCloseStrategy::Terminate | WindowCloseStrategy::Kill => {
            let signal = if strategy == WindowCloseStrategy::Terminate {
                "-TERM"
            } else {
                "-KILL"
            };
            let kill_status = Command::new("kill")
                .arg(signal)
                .arg(window.app.pid.to_string())
                .status();
            let step = match kill_status {
                Ok(status) if status.success() => WindowActionStepReport {
                    step: if strategy == WindowCloseStrategy::Terminate {
                        "terminate_app".to_owned()
                    } else {
                        "kill_process".to_owned()
                    },
                    status: "ok".to_owned(),
                    reason: None,
                    error: None,
                },
                Ok(status) => WindowActionStepReport {
                    step: if strategy == WindowCloseStrategy::Terminate {
                        "terminate_app".to_owned()
                    } else {
                        "kill_process".to_owned()
                    },
                    status: "failed".to_owned(),
                    reason: None,
                    error: Some(format!("kill exited with status {status}")),
                },
                Err(err) => WindowActionStepReport {
                    step: if strategy == WindowCloseStrategy::Terminate {
                        "terminate_app".to_owned()
                    } else {
                        "kill_process".to_owned()
                    },
                    status: "failed".to_owned(),
                    reason: None,
                    error: Some(err.to_string()),
                },
            };
            let failed_step = (step.status == "failed").then(|| step.step.clone());
            Ok(WindowActionReport {
                kind: "window-action",
                schema: WINDOW_SCHEMA,
                platform: "macos".to_owned(),
                action: "close",
                status: if failed_step.is_some() {
                    "failed".to_owned()
                } else {
                    "ok".to_owned()
                },
                window_id: Some(window_id),
                snapshot_id: Some(snapshot_id),
                observed_at_unix_ms: Some(observed_at_unix_ms),
                strategy: Some(strategy.as_str()),
                target_pid: Some(window.app.pid),
                process_scope: Some("single_resolved_process"),
                termination_attempted: Some(true),
                failed_step,
                steps: vec![
                    WindowActionStepReport {
                        step: "resolve_window_id".to_owned(),
                        status: "ok".to_owned(),
                        reason: None,
                        error: None,
                    },
                    step,
                ],
            })
        }
    }
}

fn enumerate_candidates(
    include_state: bool,
    include_recipes: bool,
) -> io::Result<Vec<WindowCandidate>> {
    let apps = list_running_apps()?;
    let visible_windows = visible_windows()?;
    let mut candidates = Vec::new();

    for app in apps {
        let Some(app_element) = create_app_ax_element(app.pid).ok() else {
            continue;
        };
        let Some(ax_windows) = copy_attribute(app_element.as_ptr(), "AXWindows")? else {
            continue;
        };
        let count = unsafe { CFArrayGetCount(ax_windows.as_ptr()) };
        for window_index in 0..count {
            let window_ref = unsafe { CFArrayGetValueAtIndex(ax_windows.as_ptr(), window_index) };
            if window_ref.is_null() {
                continue;
            }
            let resolved = build_resolved_window(
                app.clone(),
                window_index as usize,
                window_ref,
                &visible_windows,
            )?;
            candidates.push(to_candidate(resolved, include_state, include_recipes));
        }
    }

    Ok(candidates)
}

fn resolve_single_window(
    find_response: &WindowFindResponse,
    target: &WindowCommandTarget,
    allow_ambiguous: bool,
    select: Option<WindowSelectPolicy>,
    strategy: Option<WindowCloseStrategy>,
) -> io::Result<(ResolvedWindow, String, u64)> {
    resolve_single_window_with_resolver(
        find_response,
        target,
        allow_ambiguous,
        select,
        strategy,
        resolve_window_id_direct,
    )
}

fn resolve_single_window_with_resolver(
    find_response: &WindowFindResponse,
    target: &WindowCommandTarget,
    allow_ambiguous: bool,
    select: Option<WindowSelectPolicy>,
    strategy: Option<WindowCloseStrategy>,
    mut resolve_window_id: impl FnMut(&str) -> io::Result<ResolvedWindow>,
) -> io::Result<(ResolvedWindow, String, u64)> {
    if let Some(window_id) = target.window_id.as_deref() {
        let resolved = resolve_window_id(window_id)?;
        return Ok((
            resolved,
            find_response.snapshot_id.clone(),
            find_response.observed_at_unix_ms,
        ));
    }

    let candidates = find_response.matches.clone();

    let candidate = match candidates.as_slice() {
        [candidate] => candidate.clone(),
        [] => {
            return Err(invalid_json_error(
                "window-not-found",
                64,
                "window target did not match any window",
            ));
        }
        many => {
            if let Some(strategy) = strategy {
                if strategy != WindowCloseStrategy::Graceful {
                    return Err(ambiguous_error("close", many, Some(strategy)));
                }
            }
            if !allow_ambiguous {
                return Err(ambiguous_error("activate", many, strategy));
            }
            select_candidate(many, select)?
        }
    };

    let resolved = resolve_window_id(&candidate.window_id)?;
    Ok((
        resolved,
        find_response.snapshot_id.clone(),
        find_response.observed_at_unix_ms,
    ))
}

fn select_candidate(
    candidates: &[WindowCandidate],
    select: Option<WindowSelectPolicy>,
) -> io::Result<WindowCandidate> {
    select_candidate_with_resolver(candidates, select, resolve_candidate_again)
}

fn select_candidate_with_resolver(
    candidates: &[WindowCandidate],
    select: Option<WindowSelectPolicy>,
    mut resolve: impl FnMut(&WindowCandidate) -> io::Result<ResolvedWindow>,
) -> io::Result<WindowCandidate> {
    match select.unwrap_or(WindowSelectPolicy::Frontmost) {
        WindowSelectPolicy::Frontmost => {
            let resolved_candidates = candidates
                .iter()
                .map(|candidate| Ok((candidate.clone(), resolve(candidate)?)))
                .collect::<io::Result<Vec<_>>>()?;
            resolved_candidates
                .iter()
                .find(|(_, resolved)| resolved.focused)
                .map(|(candidate, _)| candidate.clone())
                .or_else(|| {
                    resolved_candidates
                        .iter()
                        .find(|(_, resolved)| resolved.app.frontmost)
                        .map(|(candidate, _)| candidate.clone())
                })
                .or_else(|| candidates.first().cloned())
                .ok_or_else(|| {
                    invalid_json_error(
                        "window-not-found",
                        64,
                        "window target did not match any window",
                    )
                })
        }
        WindowSelectPolicy::First => candidates.first().cloned().ok_or_else(|| {
            invalid_json_error(
                "window-not-found",
                64,
                "window target did not match any window",
            )
        }),
    }
}

fn resolve_candidate_again(candidate: &WindowCandidate) -> io::Result<ResolvedWindow> {
    resolve_window_id_direct(&candidate.window_id)
}

fn resolve_window_id_direct(window_id: &str) -> io::Result<ResolvedWindow> {
    let parsed = parse_window_id(window_id)?;
    let apps = list_running_apps()?;
    let app = apps
        .into_iter()
        .find(|app| app.pid == parsed.pid)
        .ok_or_else(|| stale_error(window_id))?;
    let visible = visible_windows()?;
    let app_element = create_app_ax_element(app.pid)?;
    let windows =
        copy_attribute(app_element.as_ptr(), "AXWindows")?.ok_or_else(|| stale_error(window_id))?;
    let count = unsafe { CFArrayGetCount(windows.as_ptr()) };
    if parsed.window_index >= count as usize {
        return Err(stale_error(window_id));
    }
    let window_ref =
        unsafe { CFArrayGetValueAtIndex(windows.as_ptr(), parsed.window_index as CFIndex) };
    if window_ref.is_null() {
        return Err(stale_error(window_id));
    }
    build_resolved_window(app, parsed.window_index, window_ref, &visible)
}

fn resolve_activation_steps(
    request: &WindowActivateRequest,
    window: &ResolvedWindow,
) -> Vec<String> {
    if !request.steps.is_empty() {
        return request.steps.clone();
    }

    if request.recipe.as_deref() == Some("to_interact") || request.recipe.is_none() {
        let mut steps = Vec::<String>::new();
        if window.app.hidden {
            steps.push("unhide_app".to_owned());
        }
        if window.minimized {
            steps.push("unminimize_window".to_owned());
        }
        steps.push("activate_app".to_owned());
        steps.push("raise_window".to_owned());
        if !window.current_space {
            steps.push("switch_to_window_space".to_owned());
        }
        return steps;
    }

    vec!["activate_app".to_owned(), "raise_window".to_owned()]
}

fn build_resolved_window(
    app: RunningApp,
    window_index: usize,
    window_ref: AXUIElementRef,
    visible_windows: &[VisibleWindowInfo],
) -> io::Result<ResolvedWindow> {
    let title = copy_string_attr(window_ref, "AXTitle")?;
    let rect = copy_ax_rect(window_ref)?;
    let minimized = copy_bool_attr(window_ref, "AXMinimized")?.unwrap_or(false);
    let fullscreen_space = copy_bool_attr(window_ref, "AXFullScreen")?.unwrap_or(false);
    let focused = copy_bool_attr(window_ref, "AXFocused")?.unwrap_or(false);
    let visible_match = match_visible_window(visible_windows, app.pid, title.as_deref(), rect);
    let current_space = visible_match.is_some();
    let occluded = current_space && !focused && !app.frontmost;

    Ok(ResolvedWindow {
        app,
        window_index,
        title,
        rect,
        minimized,
        fullscreen_space,
        focused,
        current_space,
        occluded,
    })
}

fn to_candidate(
    window: ResolvedWindow,
    include_state: bool,
    include_recipes: bool,
) -> WindowCandidate {
    let state = include_state.then_some(WindowState {
        occluded: window.occluded,
        minimized: window.minimized,
        app_hidden: window.app.hidden,
        current_space: window.current_space,
        fullscreen_space: window.fullscreen_space,
        interactable: !window.app.hidden
            && !window.minimized
            && window.current_space
            && !window.occluded,
        confidence: if window.current_space {
            "best_effort".to_owned()
        } else {
            "limited".to_owned()
        },
    });
    let recipes = include_recipes.then_some(WindowRecipes {
        to_interact: {
            let mut steps = Vec::new();
            if window.app.hidden {
                steps.push("unhide_app");
            }
            if window.minimized {
                steps.push("unminimize_window");
            }
            steps.push("activate_app");
            steps.push("raise_window");
            if !window.current_space {
                steps.push("switch_to_window_space");
            }
            steps
        },
        to_close_gracefully: vec!["ax_close_window"],
        to_force_close: vec!["terminate_app", "kill_process"],
    });

    WindowCandidate {
        window_id: window_id(window.app.pid, window.window_index),
        locator_lifetime: "short_lived",
        app: WindowAppDescriptor {
            name: window.app.name,
            pid: window.app.pid,
            bundle_id: window.app.bundle_id,
            hidden: window.app.hidden,
            frontmost: window.app.frontmost,
        },
        title: window.title,
        rect: window.rect,
        coordinate_space: WINDOW_COORDINATE_SPACE,
        state,
        recipes,
    }
}

fn perform_unhide_app(window: &ResolvedWindow) -> WindowActionStepReport {
    if !window.app.hidden {
        return WindowActionStepReport {
            step: "unhide_app".to_owned(),
            status: "skipped".to_owned(),
            reason: Some("already_visible".to_owned()),
            error: None,
        };
    }

    let script = format!(
        "var se = Application('System Events');\
         var processes = se.applicationProcesses.whose({{unixId:{}}})();\
         if (processes.length === 0) {{\
           processes = se.applicationProcesses.whose({{name:{:?}}})();\
         }}\
         if (processes.length > 0) {{\
           processes[0].visible = true;\
         }} else {{\
           throw new Error('process_not_found');\
         }}",
        window.app.pid, window.app.name
    );

    let result = run_jxa_script(&script).or_else(|primary_err| {
        let fallback = if let Some(bundle_id) = window.app.bundle_id.as_deref() {
            format!("Application.currentApplication(); Application({bundle_id:?}).activate();")
        } else {
            format!(
                "Application.currentApplication(); Application({:?}).activate();",
                window.app.name
            )
        };
        run_jxa_script(&fallback).map_err(|fallback_err| {
            io::Error::other(format!(
                "primary unhide failed: {primary_err}; activate fallback failed: {fallback_err}"
            ))
        })
    });

    match result {
        Ok(_) => WindowActionStepReport {
            step: "unhide_app".to_owned(),
            status: "ok".to_owned(),
            reason: None,
            error: None,
        },
        Err(err) => WindowActionStepReport {
            step: "unhide_app".to_owned(),
            status: "failed".to_owned(),
            reason: None,
            error: Some(err.to_string()),
        },
    }
}

fn perform_activate_app(window: &ResolvedWindow) -> WindowActionStepReport {
    let script = if let Some(bundle_id) = window.app.bundle_id.as_deref() {
        format!("Application.currentApplication(); Application('{bundle_id}').activate();")
    } else {
        format!(
            "var se = Application('System Events'); se.applicationProcesses.byUnixId({}).frontmost = true;",
            window.app.pid
        )
    };
    match run_jxa_script(&script) {
        Ok(_) => WindowActionStepReport {
            step: "activate_app".to_owned(),
            status: "ok".to_owned(),
            reason: None,
            error: None,
        },
        Err(err) => WindowActionStepReport {
            step: "activate_app".to_owned(),
            status: "failed".to_owned(),
            reason: None,
            error: Some(err.to_string()),
        },
    }
}

fn perform_unminimize_window(
    window_ref: CFTypeRef,
    window: &ResolvedWindow,
) -> WindowActionStepReport {
    if !window.minimized {
        return WindowActionStepReport {
            step: "unminimize_window".to_owned(),
            status: "skipped".to_owned(),
            reason: Some("already_restored".to_owned()),
            error: None,
        };
    }
    let result = with_cf_string("AXMinimized", |attribute| unsafe {
        map_ax_write_error(AXUIElementSetAttributeValue(
            window_ref,
            attribute,
            kCFBooleanFalse,
        ))
    });
    match result {
        Ok(()) => WindowActionStepReport {
            step: "unminimize_window".to_owned(),
            status: "ok".to_owned(),
            reason: None,
            error: None,
        },
        Err(err) => WindowActionStepReport {
            step: "unminimize_window".to_owned(),
            status: "failed".to_owned(),
            reason: None,
            error: Some(err.to_string()),
        },
    }
}

fn perform_raise_window(window_ref: CFTypeRef) -> WindowActionStepReport {
    let result = with_cf_string("AXRaise", |action| unsafe {
        map_ax_action_error(AXUIElementPerformAction(window_ref, action))
    });
    match result {
        Ok(()) => WindowActionStepReport {
            step: "raise_window".to_owned(),
            status: "ok".to_owned(),
            reason: None,
            error: None,
        },
        Err(err) => WindowActionStepReport {
            step: "raise_window".to_owned(),
            status: "failed".to_owned(),
            reason: None,
            error: Some(err.to_string()),
        },
    }
}

fn perform_graceful_close(window_ref: CFTypeRef) -> io::Result<WindowActionStepReport> {
    if let Some(close_button) = copy_attribute(window_ref, "AXCloseButton")? {
        let result = with_cf_string("AXPress", |action| unsafe {
            map_ax_action_error(AXUIElementPerformAction(close_button.as_ptr(), action))
        });
        return Ok(match result {
            Ok(()) => WindowActionStepReport {
                step: "ax_close_window".to_owned(),
                status: "ok".to_owned(),
                reason: None,
                error: None,
            },
            Err(err) => WindowActionStepReport {
                step: "ax_close_window".to_owned(),
                status: "failed".to_owned(),
                reason: None,
                error: Some(err.to_string()),
            },
        });
    }

    Ok(WindowActionStepReport {
        step: "ax_close_window".to_owned(),
        status: "failed".to_owned(),
        reason: None,
        error: Some("目标窗口没有可用的 AXCloseButton".to_owned()),
    })
}

fn create_app_ax_element(pid: i32) -> io::Result<CfOwned> {
    unsafe { CfOwned::new(AXUIElementCreateApplication(pid)) }
        .ok_or_else(|| io::Error::other(format!("无法创建 pid={pid} 的 AX application")))
}

fn resolve_window_ref(pid: i32, window_index: usize) -> io::Result<CfOwned> {
    let app = create_app_ax_element(pid)?;
    let windows = copy_attribute(app.as_ptr(), "AXWindows")?
        .ok_or_else(|| stale_error(&window_id(pid, window_index)))?;
    let count = unsafe { CFArrayGetCount(windows.as_ptr()) };
    if window_index >= count as usize {
        return Err(stale_error(&window_id(pid, window_index)));
    }
    let window_ref = unsafe { CFArrayGetValueAtIndex(windows.as_ptr(), window_index as CFIndex) };
    unsafe { CfOwned::retain(window_ref) }.ok_or_else(|| stale_error(&window_id(pid, window_index)))
}

fn list_running_apps() -> io::Result<Vec<RunningApp>> {
    let output = run_jxa_script(
        "var se = Application('System Events');\
         var apps = se.applicationProcesses.whose({backgroundOnly:false})();\
         JSON.stringify(apps.map(function(p){\
           return {pid:p.unixId(),name:p.name(),hidden:!p.visible(),frontmost:p.frontmost()};\
         }));",
    )?;
    let values = serde_json::from_str::<Vec<serde_json::Value>>(&output).map_err(|err| {
        io::Error::other(format!(
            "解析 macOS app process JSON 失败: {err}; raw={output}"
        ))
    })?;
    let mut apps = Vec::new();
    for value in values {
        let pid = value
            .get("pid")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| io::Error::other("JXA process 缺少 pid"))? as i32;
        let name = value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let hidden = value
            .get("hidden")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let frontmost = value
            .get("frontmost")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        apps.push(RunningApp {
            pid,
            name,
            bundle_id: bundle_id_for_pid(pid),
            hidden,
            frontmost,
        });
    }
    Ok(apps)
}

fn bundle_id_for_pid(pid: i32) -> Option<String> {
    let script = format!(
        "ObjC.import('AppKit');\
         var app = $.NSRunningApplication.runningApplicationWithProcessIdentifier({pid});\
         if (!app) {{ '' }} else {{ ObjC.unwrap(app.bundleIdentifier) || '' }};"
    );
    run_jxa_script(&script)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn visible_windows() -> io::Result<Vec<VisibleWindowInfo>> {
    let mut result = Vec::<VisibleWindowInfo>::new();
    let options = CG_WINDOW_ON_SCREEN_ONLY | CG_WINDOW_EXCLUDE_DESKTOP;
    let windows = unsafe { CfOwned::new(CGWindowListCopyWindowInfo(options, 0)) }
        .ok_or_else(|| io::Error::other("无法读取 CGWindowList"))?;
    let count = unsafe { CFArrayGetCount(windows.as_ptr()) };
    for index in 0..count {
        let dict = unsafe { CFArrayGetValueAtIndex(windows.as_ptr(), index) };
        if dict.is_null() {
            continue;
        }
        let Some(pid) = dictionary_i32(dict, unsafe { kCGWindowOwnerPID }) else {
            continue;
        };
        let layer = dictionary_i32(dict, unsafe { kCGWindowLayer }).unwrap_or(0);
        if layer != 0 {
            continue;
        }
        result.push(VisibleWindowInfo {
            pid,
            title: dictionary_string(dict, unsafe { kCGWindowName }),
            rect: dictionary_rect(dict, unsafe { kCGWindowBounds }),
        });
    }
    Ok(result)
}

fn match_visible_window(
    visible_windows: &[VisibleWindowInfo],
    pid: i32,
    title: Option<&str>,
    rect: Option<AxRect>,
) -> Option<VisibleWindowInfo> {
    let has_title = title.is_some();
    let has_rect = rect.is_some();

    visible_windows
        .iter()
        .find(|item| {
            if item.pid != pid {
                return false;
            }

            let title_matches = match title {
                Some(title) => item.title.as_deref() == Some(title),
                None => false,
            };
            let rect_matches = match rect {
                Some(rect) => item.rect == Some(rect),
                None => false,
            };

            match (has_title, has_rect) {
                // --------------------------------------------------
                // 对 macOS 来说, AX 的 title 和 CGWindow 的 name 并不总是同一份真相.
                // 有些窗口会给出精确 rect, 但标题会退化成短名或局部名, 反之亦然.
                // 所以只要同 pid 下有 title 或 rect 的真实命中, 就认为当前 Space 可见.
                // --------------------------------------------------
                (true, true) => title_matches || rect_matches,
                (true, false) => title_matches,
                (false, true) => rect_matches,
                (false, false) => false,
            }
        })
        .cloned()
}

fn parse_window_id(input: &str) -> io::Result<ParsedWindowId> {
    let parts = input.split('/').collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err(stale_error(input));
    }
    let pid = parts[0]
        .strip_prefix("pid:")
        .ok_or_else(|| stale_error(input))?
        .parse::<i32>()
        .map_err(|_| stale_error(input))?;
    let window_index = parts[1]
        .strip_prefix("window:")
        .ok_or_else(|| stale_error(input))?
        .parse::<usize>()
        .map_err(|_| stale_error(input))?;
    Ok(ParsedWindowId { pid, window_index })
}

#[derive(Debug, Copy, Clone)]
struct ParsedWindowId {
    pid: i32,
    window_index: usize,
}

fn window_id(pid: i32, window_index: usize) -> String {
    format!("pid:{pid}/window:{window_index}")
}

fn run_jxa_script(script: &str) -> io::Result<String> {
    let output = Command::new("osascript")
        .arg("-l")
        .arg("JavaScript")
        .arg("-e")
        .arg(script)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "osascript 执行失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn ensure_trusted() -> io::Result<()> {
    if unsafe { AXIsProcessTrusted() } != 0 {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "macOS Accessibility 权限不足: 请给实际执行 rdog daemon/control 的进程授予辅助功能权限,授权后重启该进程",
    ))
}

fn copy_attribute(element: AXUIElementRef, attr: &str) -> io::Result<Option<CfOwned>> {
    with_cf_string(attr, |attr_ref| {
        let mut value = ptr::null();
        let error = unsafe { AXUIElementCopyAttributeValue(element, attr_ref, &mut value) };
        match error {
            AX_SUCCESS => Ok(CfOwned::new(value)),
            code if optional_ax_error(code) => Ok(None),
            AX_ERROR_API_DISABLED => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "macOS Accessibility API 当前不可用或未授权",
            )),
            code => Err(io::Error::other(format!(
                "读取 AX attribute `{attr}` 失败: AXError {code}"
            ))),
        }
    })
}

fn optional_ax_error(error: AXError) -> bool {
    matches!(
        error,
        AX_ERROR_FAILURE
            | AX_ERROR_NOT_IMPLEMENTED
            | AX_ERROR_ATTRIBUTE_UNSUPPORTED
            | AX_ERROR_NO_VALUE
            | AX_ERROR_CANNOT_COMPLETE
            | AX_ERROR_INVALID_UI_ELEMENT
    )
}

fn copy_string_attr(element: AXUIElementRef, attr: &str) -> io::Result<Option<String>> {
    let Some(value) = copy_attribute(element, attr)? else {
        return Ok(None);
    };
    Ok(cf_to_string(value.as_ptr()))
}

fn copy_bool_attr(element: AXUIElementRef, attr: &str) -> io::Result<Option<bool>> {
    let Some(value) = copy_attribute(element, attr)? else {
        return Ok(None);
    };
    Ok(cf_to_bool(value.as_ptr()))
}

fn copy_ax_rect(element: AXUIElementRef) -> io::Result<Option<AxRect>> {
    let Some(position) = copy_attribute(element, "AXPosition")? else {
        return Ok(None);
    };
    let Some(size) = copy_attribute(element, "AXSize")? else {
        return Ok(None);
    };
    let Some(point) = cf_to_point(position.as_ptr()) else {
        return Ok(None);
    };
    let Some(size) = cf_to_size(size.as_ptr()) else {
        return Ok(None);
    };
    Ok(ax_rect_from_parts(point, size))
}

fn dictionary_string(dict: CFDictionaryRef, key: CFStringRef) -> Option<String> {
    let value = unsafe { CFDictionaryGetValue(dict, key) };
    cf_to_string(value)
}

fn dictionary_i32(dict: CFDictionaryRef, key: CFStringRef) -> Option<i32> {
    let value = unsafe { CFDictionaryGetValue(dict, key) };
    cf_to_i32(value)
}

fn dictionary_rect(dict: CFDictionaryRef, key: CFStringRef) -> Option<AxRect> {
    let value = unsafe { CFDictionaryGetValue(dict, key) };
    if value.is_null() {
        return None;
    }

    let mut rect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    };
    let ok = unsafe { CGRectMakeWithDictionaryRepresentation(value, &mut rect) };
    ok.then(|| ax_rect_from_parts(rect.origin, rect.size))
        .flatten()
}

fn cf_to_string(value: CFTypeRef) -> Option<String> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
        return cf_to_i32(value)
            .map(|number| number.to_string())
            .or_else(|| cf_to_bool(value).map(|flag| flag.to_string()));
    }

    let mut buffer = vec![0i8; 4096];
    let ok =
        unsafe { CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as CFIndex, UTF8) };
    if ok == 0 {
        return None;
    }
    let bytes = buffer
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect::<Vec<_>>();
    String::from_utf8(bytes).ok()
}

fn cf_to_bool(value: CFTypeRef) -> Option<bool> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { CFBooleanGetTypeID() } {
        return None;
    }
    Some(unsafe { CFBooleanGetValue(value as CFBooleanRef) != 0 })
}

fn cf_to_i32(value: CFTypeRef) -> Option<i32> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { CFNumberGetTypeID() } {
        return None;
    }
    let mut number = 0i32;
    let ok = unsafe {
        CFNumberGetValue(
            value as CFNumberRef,
            CF_NUMBER_SINT32,
            (&mut number as *mut i32).cast(),
        )
    };
    (ok != 0).then_some(number)
}

fn cf_to_point(value: CFTypeRef) -> Option<CGPoint> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { AXValueGetTypeID() } {
        return None;
    }
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    let ok = unsafe {
        AXValueGetValue(
            value as AXValueRef,
            AX_VALUE_CG_POINT,
            (&mut point as *mut CGPoint).cast(),
        )
    };
    (ok != 0).then_some(point)
}

fn cf_to_size(value: CFTypeRef) -> Option<CGSize> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { AXValueGetTypeID() } {
        return None;
    }
    let mut size = CGSize {
        width: 0.0,
        height: 0.0,
    };
    let ok = unsafe {
        AXValueGetValue(
            value as AXValueRef,
            AX_VALUE_CG_SIZE,
            (&mut size as *mut CGSize).cast(),
        )
    };
    (ok != 0).then_some(size)
}

fn ax_rect_from_parts(point: CGPoint, size: CGSize) -> Option<AxRect> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || !size.width.is_finite()
        || !size.height.is_finite()
        || size.width < 0.0
        || size.height < 0.0
    {
        return None;
    }

    Some(AxRect {
        x: point.x.round() as i32,
        y: point.y.round() as i32,
        width: size.width.round() as u32,
        height: size.height.round() as u32,
    })
}

fn with_cf_string<T>(value: &str, f: impl FnOnce(CFStringRef) -> T) -> T {
    let c_string = CString::new(value).expect("CFString content should not contain NUL");
    let cf_string = unsafe {
        CfOwned::new(CFStringCreateWithCString(
            ptr::null(),
            c_string.as_ptr(),
            UTF8,
        ))
    }
    .expect("CFStringCreateWithCString should succeed");
    f(cf_string.as_ptr())
}

unsafe fn map_ax_action_error(error: AXError) -> io::Result<()> {
    match error {
        AX_SUCCESS => Ok(()),
        AX_ERROR_API_DISABLED => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "macOS Accessibility API 当前不可用或未授权",
        )),
        AX_ERROR_ACTION_UNSUPPORTED => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "目标窗口不支持该 AX action",
        )),
        AX_ERROR_INVALID_UI_ELEMENT | AX_ERROR_NO_VALUE => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "目标窗口 locator 已失效",
        )),
        code => Err(io::Error::other(format!(
            "执行 AX action 失败: AXError {code}"
        ))),
    }
}

unsafe fn map_ax_write_error(error: AXError) -> io::Result<()> {
    match error {
        AX_SUCCESS => Ok(()),
        AX_ERROR_API_DISABLED => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "macOS Accessibility API 当前不可用或未授权",
        )),
        AX_ERROR_ATTRIBUTE_UNSUPPORTED | AX_ERROR_ACTION_UNSUPPORTED => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "目标窗口不支持该 AX attribute 写入",
        )),
        AX_ERROR_INVALID_UI_ELEMENT | AX_ERROR_NO_VALUE => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "目标窗口 locator 已失效",
        )),
        code => Err(io::Error::other(format!(
            "写入 AX attribute 失败: AXError {code}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_window_id_should_accept_expected_shape() {
        let parsed = parse_window_id("pid:123/window:2").unwrap();
        assert_eq!(parsed.pid, 123);
        assert_eq!(parsed.window_index, 2);
        assert!(parse_window_id("pid:bad/window:2").is_err());
        assert!(parse_window_id("pid:1/window:bad").is_err());
    }

    #[test]
    fn step_selection_should_reflect_window_state() {
        let window = ResolvedWindow {
            app: RunningApp {
                pid: 1,
                name: "Terminal".to_owned(),
                bundle_id: Some("com.apple.Terminal".to_owned()),
                hidden: true,
                frontmost: false,
            },
            window_index: 0,
            title: Some("rdog".to_owned()),
            rect: None,
            minimized: true,
            fullscreen_space: false,
            focused: false,
            current_space: false,
            occluded: false,
        };
        let request = WindowActivateRequest {
            target: WindowCommandTarget {
                window_id: Some("pid:1/window:0".to_owned()),
                query: WindowQuery::default(),
            },
            recipe: Some("to_interact".to_owned()),
            steps: Vec::new(),
            allow_ambiguous: false,
            select: None,
        };
        let steps = resolve_activation_steps(&request, &window);
        assert_eq!(
            steps,
            vec![
                "unhide_app",
                "unminimize_window",
                "activate_app",
                "raise_window",
                "switch_to_window_space"
            ]
        );
    }

    #[test]
    fn visible_window_match_should_not_fallback_to_any_pid_window() {
        let visible = vec![VisibleWindowInfo {
            pid: 7,
            title: Some("Another".to_owned()),
            rect: Some(AxRect {
                x: 10,
                y: 20,
                width: 300,
                height: 200,
            }),
        }];

        assert!(match_visible_window(
            &visible,
            7,
            Some("Missing"),
            Some(AxRect {
                x: 50,
                y: 60,
                width: 300,
                height: 200,
            }),
        )
        .is_none());
    }

    #[test]
    fn visible_window_match_should_accept_exact_rect_when_cg_title_differs() {
        let visible = vec![VisibleWindowInfo {
            pid: 7,
            title: Some("T".to_owned()),
            rect: Some(AxRect {
                x: 304,
                y: 180,
                width: 920,
                height: 464,
            }),
        }];

        assert!(match_visible_window(
            &visible,
            7,
            Some("rdog-window-e2e-states-49955-63775"),
            Some(AxRect {
                x: 304,
                y: 180,
                width: 920,
                height: 464,
            }),
        )
        .is_some());
    }

    #[test]
    fn select_frontmost_should_prefer_focused_window() {
        let candidates = vec![
            WindowCandidate {
                window_id: "pid:11/window:0".to_owned(),
                locator_lifetime: "short_lived",
                app: WindowAppDescriptor {
                    name: "Editor".to_owned(),
                    pid: 11,
                    bundle_id: Some("com.example.Editor".to_owned()),
                    hidden: false,
                    frontmost: true,
                },
                title: Some("Background".to_owned()),
                rect: None,
                coordinate_space: WINDOW_COORDINATE_SPACE,
                state: None,
                recipes: None,
            },
            WindowCandidate {
                window_id: "pid:11/window:1".to_owned(),
                locator_lifetime: "short_lived",
                app: WindowAppDescriptor {
                    name: "Editor".to_owned(),
                    pid: 11,
                    bundle_id: Some("com.example.Editor".to_owned()),
                    hidden: false,
                    frontmost: true,
                },
                title: Some("Focused".to_owned()),
                rect: None,
                coordinate_space: WINDOW_COORDINATE_SPACE,
                state: None,
                recipes: None,
            },
        ];

        let selected = select_candidate_with_resolver(
            &candidates,
            Some(WindowSelectPolicy::Frontmost),
            |candidate| {
                Ok(ResolvedWindow {
                    app: RunningApp {
                        pid: candidate.app.pid,
                        name: candidate.app.name.clone(),
                        bundle_id: candidate.app.bundle_id.clone(),
                        hidden: candidate.app.hidden,
                        frontmost: candidate.app.frontmost,
                    },
                    window_index: if candidate.window_id.ends_with("/window:1") {
                        1
                    } else {
                        0
                    },
                    title: candidate.title.clone(),
                    rect: candidate.rect,
                    minimized: false,
                    fullscreen_space: false,
                    focused: candidate.window_id.ends_with("/window:1"),
                    current_space: true,
                    occluded: false,
                })
            },
        )
        .unwrap();

        assert_eq!(selected.window_id, "pid:11/window:1");
    }

    #[test]
    fn resolve_single_window_should_use_direct_window_id_lookup_even_when_find_result_is_truncated()
    {
        let find_response = WindowFindResponse {
            kind: "window-find",
            schema: WINDOW_SCHEMA,
            platform: "macos".to_owned(),
            status: "complete".to_owned(),
            capabilities: WindowCapabilities::complete(),
            match_count: 30,
            returned_count: 20,
            snapshot_id: "window-snapshot-42".to_owned(),
            observed_at_unix_ms: 42,
            matches: Vec::new(),
        };
        let target = WindowCommandTarget {
            window_id: Some("pid:77/window:25".to_owned()),
            query: WindowQuery::default(),
        };

        let resolved = resolve_single_window_with_resolver(
            &find_response,
            &target,
            false,
            None,
            None,
            |window_id| {
                assert_eq!(window_id, "pid:77/window:25");
                Ok(ResolvedWindow {
                    app: RunningApp {
                        pid: 77,
                        name: "Editor".to_owned(),
                        bundle_id: Some("com.example.Editor".to_owned()),
                        hidden: false,
                        frontmost: false,
                    },
                    window_index: 25,
                    title: Some("Late Match".to_owned()),
                    rect: None,
                    minimized: false,
                    fullscreen_space: false,
                    focused: false,
                    current_space: false,
                    occluded: false,
                })
            },
        )
        .unwrap();

        assert_eq!(resolved.0.window_index, 25);
        assert_eq!(resolved.1, "window-snapshot-42");
        assert_eq!(resolved.2, 42);
    }
}
