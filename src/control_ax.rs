use crate::{
    control_observation::selector::{
        AppSelector, DurableSelectorDraft, ElementSelector, SelectorEnvelope, SelectorKind,
        SelectorRect, SelectorRedaction, WindowSelector,
    },
    control_observation::{
        observation_ref_name, record_observation_with_selectors, resolve_observation_ref,
        stale_observation_ref_error, ObservationHeader, ObservationRefEntry, ObservationRoot,
    },
    control_protocol::{
        normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
        split_object_fields, KeyDelivery, KeyMode, KeyRequest,
    },
};
use serde::Serialize;
use serde_json::json;
use std::io;

pub const AX_SCHEMA: &str = "rdog.ax.v1";
pub const AX_WINDOWS_DEPTH: u8 = 1;
pub const AX_WINDOWS_MAX_ELEMENTS: u16 = 80;
pub const AX_WINDOWS_INCLUDE_VALUES: bool = false;
pub const AX_INTERACTIVE_DEPTH: u8 = 2;
pub const AX_INTERACTIVE_MAX_ELEMENTS: u16 = 200;
pub const AX_INTERACTIVE_INCLUDE_VALUES: bool = false;
pub const DEFAULT_AX_DEPTH: u8 = 4;
pub const DEFAULT_AX_MAX_ELEMENTS: u16 = 1000;
pub const DEFAULT_AX_INCLUDE_VALUES: bool = true;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AxMode {
    Windows,
    Interactive,
    Full,
}

impl AxMode {
    pub fn preset(self) -> AxModePreset {
        match self {
            Self::Windows => AxModePreset {
                depth: AX_WINDOWS_DEPTH,
                max_elements: AX_WINDOWS_MAX_ELEMENTS,
                include_values: AX_WINDOWS_INCLUDE_VALUES,
            },
            Self::Interactive => AxModePreset {
                depth: AX_INTERACTIVE_DEPTH,
                max_elements: AX_INTERACTIVE_MAX_ELEMENTS,
                include_values: AX_INTERACTIVE_INCLUDE_VALUES,
            },
            Self::Full => AxModePreset {
                depth: DEFAULT_AX_DEPTH,
                max_elements: DEFAULT_AX_MAX_ELEMENTS,
                include_values: DEFAULT_AX_INCLUDE_VALUES,
            },
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AxModePreset {
    pub depth: u8,
    pub max_elements: u16,
    pub include_values: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxTreeRequest {
    pub scope: AxTreeScope,
    pub depth: u8,
    pub max_elements: u16,
    pub include_values: bool,
}

impl Default for AxTreeRequest {
    fn default() -> Self {
        Self {
            scope: AxTreeScope::Windows,
            depth: DEFAULT_AX_DEPTH,
            max_elements: DEFAULT_AX_MAX_ELEMENTS,
            include_values: DEFAULT_AX_INCLUDE_VALUES,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AxTreeScope {
    Windows,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxPressRequest {
    pub target: AxTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxActionRequest {
    pub target: AxTarget,
    pub action: AxActionName,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AxActionName {
    Press,
    Open,
    Confirm,
    Cancel,
    ShowMenu,
    ScrollToVisible,
}

impl AxActionName {
    pub fn protocol_str(self) -> &'static str {
        match self {
            Self::Press => "AXPress",
            Self::Open => "AXOpen",
            Self::Confirm => "AXConfirm",
            Self::Cancel => "AXCancel",
            Self::ShowMenu => "AXShowMenu",
            Self::ScrollToVisible => "AXScrollToVisible",
        }
    }

    pub fn report_str(self) -> &'static str {
        self.protocol_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxSetValueRequest {
    pub target: AxTarget,
    pub value: String,
    pub mode: AxValueSetMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxFocusRequest {
    pub target: Option<AxTarget>,
    pub window_id: Option<String>,
    pub activate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxScrollRequest {
    pub target: AxTarget,
    pub direction: AxScrollDirection,
    pub pages: u16,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AxScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

impl AxScrollDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AxValueSetMode {
    Replace,
    Append,
}

impl AxValueSetMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeTextRequest {
    pub target: AxTarget,
    pub text: String,
    pub mode: TypeTextMode,
    pub allow_clipboard: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TypeTextMode {
    Auto,
    AxValue,
    TargetedKeyboard,
    Clipboard,
}

impl TypeTextMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::AxValue => "ax-value",
            Self::TargetedKeyboard => "targeted-keyboard",
            Self::Clipboard => "clipboard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardRestoreStatus {
    pub restored: bool,
    pub skipped_reason: Option<&'static str>,
}

impl ClipboardRestoreStatus {
    pub fn restored() -> Self {
        Self {
            restored: true,
            skipped_reason: None,
        }
    }

    pub fn skipped(reason: &'static str) -> Self {
        Self {
            restored: false,
            skipped_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AxTarget {
    pub id: Option<String>,
    pub ref_id: Option<String>,
    pub observation_id: Option<String>,
    pub process: Option<String>,
    pub window_title: Option<String>,
    pub role: Option<String>,
    pub subrole: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl AxTarget {
    fn validate(&self) -> io::Result<()> {
        let has_ref = self.ref_id.is_some();
        let has_observation_id = self.observation_id.is_some();
        let has_semantic = self.process.is_some()
            || self.window_title.is_some()
            || self.role.is_some()
            || self.subrole.is_some()
            || self.name.is_some()
            || self.description.is_some();

        if self.id.is_some() {
            if has_ref || has_observation_id || has_semantic {
                return Err(invalid_data(
                    "AX target id 不能与 ref / observation_id / semantic locator 混用",
                ));
            }
            return Ok(());
        }

        if has_ref || has_observation_id {
            if !has_ref || !has_observation_id {
                return Err(invalid_data("AX target.ref 必须和 observation_id 一起出现"));
            }
            if has_semantic {
                return Err(invalid_data("AX target.ref 不能和 semantic locator 混用"));
            }
            return Ok(());
        }

        if !has_semantic {
            return Err(invalid_data("AX target 不能为空"));
        }

        if self.role.is_none()
            && self.subrole.is_none()
            && self.name.is_none()
            && self.description.is_none()
        {
            return Err(invalid_data(
                "AX semantic target 必须至少包含 role/subrole/name/description 之一",
            ));
        }

        Ok(())
    }

    fn matches_window(&self, window: &AxWindow) -> bool {
        matches_optional(&self.process, Some(window.process_name.as_str()))
            && matches_optional(&self.window_title, window.title.as_deref())
    }

    fn matches_element(&self, element: &AxElement) -> bool {
        matches_optional(&self.role, Some(element.role.as_str()))
            && matches_optional(&self.subrole, element.subrole.as_deref())
            && matches_optional(&self.name, element.name.as_deref())
            && matches_optional(&self.description, element.description.as_deref())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub struct AxRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxSnapshot {
    pub schema: &'static str,
    pub platform: String,
    pub capture_status: String,
    pub permission_status: String,
    pub coordinate_space: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<ObservationHeader>,
    pub window_count: usize,
    pub element_count: usize,
    pub truncated: bool,
    pub windows: Vec<AxWindow>,
}

impl AxSnapshot {
    pub fn complete(
        platform: impl Into<String>,
        mut windows: Vec<AxWindow>,
        truncated: bool,
    ) -> Self {
        windows.sort_by(|a, b| {
            a.pid
                .cmp(&b.pid)
                .then_with(|| a.id.cmp(&b.id))
                .then_with(|| a.title.cmp(&b.title))
        });
        let element_count = windows.iter().map(AxWindow::element_count).sum();
        Self {
            schema: AX_SCHEMA,
            platform: platform.into(),
            capture_status: "complete".to_owned(),
            permission_status: "granted".to_owned(),
            coordinate_space: "os-logical",
            observation: None,
            window_count: windows.len(),
            element_count,
            truncated,
            windows,
        }
    }

    pub fn permission_denied(platform: impl Into<String>) -> Self {
        Self::empty_status(platform, "permission_denied", "denied")
    }

    pub fn unsupported() -> Self {
        Self::empty_status("unsupported", "unsupported", "unknown")
    }

    fn empty_status(
        platform: impl Into<String>,
        capture_status: impl Into<String>,
        permission_status: impl Into<String>,
    ) -> Self {
        Self {
            schema: AX_SCHEMA,
            platform: platform.into(),
            capture_status: capture_status.into(),
            permission_status: permission_status.into(),
            coordinate_space: "os-logical",
            observation: None,
            window_count: 0,
            element_count: 0,
            truncated: false,
            windows: Vec::new(),
        }
    }

    pub fn with_observation(mut self, source_command: &str) -> io::Result<Self> {
        let mut refs = Vec::new();
        let mut selector_drafts = Vec::new();
        let mut next_ref_index = 1usize;
        for window in &mut self.windows {
            let ref_id = match &window.ref_id {
                Some(ref_id) => {
                    reserve_existing_ref_index(ref_id, &mut next_ref_index);
                    ref_id.clone()
                }
                None => {
                    let ref_id = observation_ref_name(next_ref_index);
                    next_ref_index += 1;
                    window.ref_id = Some(ref_id.clone());
                    ref_id
                }
            };
            refs.push(ObservationRefEntry {
                ref_id: ref_id.clone(),
                backend_id: window.id.clone(),
                kind: "window".to_owned(),
            });
            selector_drafts.push(window_selector_draft(&self.platform, window, &ref_id));
            let app_selector = app_selector_for_window(window);
            let window_selector = window_selector_for_ax_window(window);
            collect_element_refs(
                &self.platform,
                &app_selector,
                &window_selector,
                &mut next_ref_index,
                &mut window.elements,
                &mut refs,
                &mut selector_drafts,
            );
        }

        self.observation = Some(record_observation_with_selectors(
            "ax",
            source_command,
            ObservationRoot {
                schema: self.schema.to_owned(),
                platform: self.platform.clone(),
                coordinate_space: self.coordinate_space.to_owned(),
            },
            refs,
            selector_drafts,
        )?);
        Ok(self)
    }

    pub fn to_tree_value_json(&self) -> io::Result<String> {
        let value = json!({
            "kind": "ax-tree",
            "schema": self.schema,
            "platform": self.platform,
            "capture_status": self.capture_status,
            "permission_status": self.permission_status,
            "coordinate_space": self.coordinate_space,
            "observation": self.observation,
            "window_count": self.window_count,
            "element_count": self.element_count,
            "truncated": self.truncated,
            "windows": self.windows,
        });
        serde_json::to_string(&value)
            .map_err(|err| io::Error::other(format!("AX tree response 序列化失败: {err}")))
    }

    fn contains_element_id(&self, target_id: &str) -> bool {
        self.windows.iter().any(|window| {
            window.id == target_id
                || window
                    .elements
                    .iter()
                    .any(|element| element.contains_id(target_id))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxWindow {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    pub ref_id: Option<String>,
    pub pid: i32,
    pub process_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<AxRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    pub elements: Vec<AxElement>,
}

impl AxWindow {
    fn element_count(&self) -> usize {
        self.elements.iter().map(AxElement::tree_count).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxElement {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    pub ref_id: Option<String>,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub value_redacted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<AxRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub actions: Vec<String>,
    pub ax_path: Vec<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<AxElement>,
}

impl AxElement {
    fn tree_count(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(AxElement::tree_count)
            .sum::<usize>()
    }

    fn contains_id(&self, target_id: &str) -> bool {
        self.id == target_id
            || self
                .children
                .iter()
                .any(|child| child.contains_id(target_id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxResolvedTargetRect {
    pub target_id: String,
    pub target_type: &'static str,
    pub window_id: Option<String>,
    pub rect: Option<AxRect>,
}

fn collect_element_refs(
    platform: &str,
    app_selector: &AppSelector,
    window_selector: &WindowSelector,
    next_ref_index: &mut usize,
    elements: &mut [AxElement],
    refs: &mut Vec<ObservationRefEntry>,
    selector_drafts: &mut Vec<DurableSelectorDraft>,
) {
    for element in elements {
        let ref_id = match &element.ref_id {
            Some(ref_id) => {
                reserve_existing_ref_index(ref_id, next_ref_index);
                ref_id.clone()
            }
            None => {
                let ref_id = observation_ref_name(*next_ref_index);
                *next_ref_index += 1;
                element.ref_id = Some(ref_id.clone());
                ref_id
            }
        };
        refs.push(ObservationRefEntry {
            ref_id: ref_id.clone(),
            backend_id: element.id.clone(),
            kind: "element".to_owned(),
        });
        selector_drafts.push(element_selector_draft(
            platform,
            app_selector,
            window_selector,
            element,
            &ref_id,
        ));

        if !element.children.is_empty() {
            collect_element_refs(
                platform,
                app_selector,
                window_selector,
                next_ref_index,
                &mut element.children,
                refs,
                selector_drafts,
            );
        }
    }
}

fn window_selector_draft(platform: &str, window: &AxWindow, ref_id: &str) -> DurableSelectorDraft {
    DurableSelectorDraft::new(
        ref_id.to_owned(),
        SelectorKind::AxWindow,
        window.id.clone(),
        SelectorEnvelope {
            platform: platform.to_owned(),
            app: Some(app_selector_for_window(window)),
            window: Some(window_selector_for_ax_window(window)),
            element: None,
            anchors: Vec::new(),
        },
        SelectorRedaction::metadata_only(),
    )
}

fn element_selector_draft(
    platform: &str,
    app_selector: &AppSelector,
    window_selector: &WindowSelector,
    element: &AxElement,
    ref_id: &str,
) -> DurableSelectorDraft {
    DurableSelectorDraft::new(
        ref_id.to_owned(),
        SelectorKind::AxElement,
        element.id.clone(),
        SelectorEnvelope {
            platform: platform.to_owned(),
            app: Some(app_selector.clone()),
            window: Some(window_selector.clone()),
            element: Some(ElementSelector {
                role: element.role.clone(),
                subrole: element.subrole.clone(),
                name: element.name.clone(),
                description: element.description.clone(),
                actions: element.actions.clone(),
                ax_path: element.ax_path.clone(),
            }),
            anchors: Vec::new(),
        },
        SelectorRedaction::metadata_only(),
    )
}

fn app_selector_for_window(window: &AxWindow) -> AppSelector {
    AppSelector {
        name: window.process_name.clone(),
        bundle_id: None,
        pid_hint: Some(window.pid),
    }
}

fn window_selector_for_ax_window(window: &AxWindow) -> WindowSelector {
    WindowSelector {
        title: window.title.clone(),
        role: window.role.clone(),
        rect: window.rect.map(selector_rect_from_ax_rect),
    }
}

fn selector_rect_from_ax_rect(rect: AxRect) -> SelectorRect {
    SelectorRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

fn reserve_existing_ref_index(ref_id: &str, next_ref_index: &mut usize) {
    let Some(index) = ref_id
        .strip_prefix("@e")
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return;
    };
    *next_ref_index = (*next_ref_index).max(index.saturating_add(1));
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxActionReport {
    pub kind: &'static str,
    pub action: String,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub performed: bool,
    pub status: &'static str,
}

impl AxActionReport {
    pub fn press(backend: impl Into<String>, target_id: Option<String>) -> Self {
        Self {
            kind: "ax",
            action: "press".to_owned(),
            backend: backend.into(),
            target_id,
            performed: true,
            status: "ok",
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX action response 序列化失败: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxPerformedActionReport {
    pub kind: &'static str,
    pub action: String,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub performed: bool,
    pub status: &'static str,
}

impl AxPerformedActionReport {
    pub fn success(
        backend: impl Into<String>,
        target_id: Option<String>,
        action: AxActionName,
    ) -> Self {
        Self {
            kind: "ax-action",
            action: action.report_str().to_owned(),
            backend: backend.into(),
            target_id,
            performed: true,
            status: "ok",
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX action response 序列化失败: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxSetValueReport {
    pub kind: &'static str,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub mode: &'static str,
    pub performed: bool,
    pub status: &'static str,
    pub settable: bool,
    pub old_value_redacted: bool,
    pub new_value_redacted: bool,
}

impl AxSetValueReport {
    pub fn success(
        backend: impl Into<String>,
        target_id: Option<String>,
        mode: AxValueSetMode,
        old_value_redacted: bool,
        new_value_redacted: bool,
    ) -> Self {
        Self {
            kind: "ax-set-value",
            backend: backend.into(),
            target_id,
            mode: mode.as_str(),
            performed: true,
            status: "ok",
            settable: true,
            old_value_redacted,
            new_value_redacted,
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX set value response 序列化失败: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeTextReport {
    pub kind: &'static str,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub mode: &'static str,
    pub delivered_via: &'static str,
    pub performed: bool,
    pub status: &'static str,
    pub used_clipboard: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipboard_restore_policy: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipboard_restored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipboard_restore_skipped_reason: Option<&'static str>,
}

impl TypeTextReport {
    pub fn ax_value_success(
        backend: impl Into<String>,
        target_id: Option<String>,
        mode: TypeTextMode,
    ) -> Self {
        Self {
            kind: "type-text",
            backend: backend.into(),
            target_id,
            mode: mode.as_str(),
            delivered_via: "ax-value",
            performed: true,
            status: "ok",
            used_clipboard: false,
            clipboard_restore_policy: None,
            clipboard_restored: None,
            clipboard_restore_skipped_reason: None,
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("type-text response 序列化失败: {err}")))
    }

    pub fn targeted_keyboard_success(
        backend: impl Into<String>,
        target_id: Option<String>,
    ) -> Self {
        Self {
            kind: "type-text",
            backend: backend.into(),
            target_id,
            mode: "targeted-keyboard",
            delivered_via: "targeted-keyboard",
            performed: true,
            status: "ok",
            used_clipboard: false,
            clipboard_restore_policy: None,
            clipboard_restored: None,
            clipboard_restore_skipped_reason: None,
        }
    }

    pub fn clipboard_success(
        backend: impl Into<String>,
        target_id: Option<String>,
        restore: ClipboardRestoreStatus,
    ) -> Self {
        Self {
            kind: "type-text",
            backend: backend.into(),
            target_id,
            mode: "clipboard",
            delivered_via: "clipboard",
            performed: true,
            status: "ok",
            used_clipboard: true,
            clipboard_restore_policy: Some("restore-if-unchanged"),
            clipboard_restored: Some(restore.restored),
            clipboard_restore_skipped_reason: restore.skipped_reason,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyDeliveryReport {
    pub kind: &'static str,
    pub backend: String,
    pub key: String,
    pub mode: &'static str,
    pub delivery: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    pub performed: bool,
    pub status: &'static str,
}

impl KeyDeliveryReport {
    pub fn success(
        backend: impl Into<String>,
        request: &KeyRequest,
        target_pid: Option<i32>,
        window_id: Option<String>,
    ) -> Self {
        Self {
            kind: "key",
            backend: backend.into(),
            key: request.key.clone(),
            mode: key_mode_as_str(request.mode),
            delivery: request.delivery.as_str(),
            target_pid,
            window_id,
            performed: true,
            status: "ok",
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("key response 序列化失败: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxFocusReport {
    pub kind: &'static str,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    pub activated: bool,
    pub performed: bool,
    pub status: &'static str,
}

impl AxFocusReport {
    pub fn success(
        backend: impl Into<String>,
        target_id: Option<String>,
        window_id: Option<String>,
        activated: bool,
    ) -> Self {
        Self {
            kind: "ax-focus",
            backend: backend.into(),
            target_id,
            window_id,
            activated,
            performed: true,
            status: "ok",
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX focus response 序列化失败: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxScrollReport {
    pub kind: &'static str,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub direction: &'static str,
    pub pages: u16,
    pub line_steps: i32,
    pub delivered_via: &'static str,
    pub performed: bool,
    pub status: &'static str,
}

impl AxScrollReport {
    pub fn success(
        backend: impl Into<String>,
        target_id: Option<String>,
        direction: AxScrollDirection,
        pages: u16,
        line_steps: i32,
        delivered_via: &'static str,
    ) -> Self {
        Self {
            kind: "ax-scroll",
            backend: backend.into(),
            target_id,
            direction: direction.as_str(),
            pages,
            line_steps,
            delivered_via,
            performed: true,
            status: "ok",
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX scroll response 序列化失败: {err}")))
    }
}

mod query;

pub use query::{
    build_ax_find_response_json, build_ax_get_response_json, parse_ax_find_payload,
    parse_ax_get_payload, AxFindQuery, AxFindRequest, AxGetRequest,
};

pub trait AxBackend {
    fn snapshot(&self, request: &AxTreeRequest) -> io::Result<AxSnapshot>;
    fn perform_action(&self, request: &AxActionRequest) -> io::Result<AxPerformedActionReport>;
    fn set_value(&self, request: &AxSetValueRequest) -> io::Result<AxSetValueReport>;
    fn focus(&self, request: &AxFocusRequest) -> io::Result<AxFocusReport>;
    fn scroll(&self, request: &AxScrollRequest) -> io::Result<AxScrollReport>;
    fn type_text(&self, request: &TypeTextRequest) -> io::Result<TypeTextReport>;
}

#[derive(Debug, Copy, Clone, Default)]
pub struct SystemAxBackend;

impl AxBackend for SystemAxBackend {
    fn snapshot(&self, request: &AxTreeRequest) -> io::Result<AxSnapshot> {
        platform_snapshot(request)
    }

    fn perform_action(&self, request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
        platform_perform_action(request)
    }

    fn set_value(&self, request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
        platform_set_value(request)
    }

    fn focus(&self, request: &AxFocusRequest) -> io::Result<AxFocusReport> {
        platform_focus(request)
    }

    fn scroll(&self, request: &AxScrollRequest) -> io::Result<AxScrollReport> {
        platform_scroll(request)
    }

    fn type_text(&self, request: &TypeTextRequest) -> io::Result<TypeTextReport> {
        platform_type_text(request)
    }
}

pub fn capture_default_ax_snapshot(request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    SystemAxBackend.snapshot(request)
}

pub fn resolve_current_ax_target_rect(target: &AxTarget) -> io::Result<AxResolvedTargetRect> {
    let request = AxTreeRequest {
        depth: 8,
        max_elements: 5000,
        include_values: false,
        ..AxTreeRequest::default()
    };
    let snapshot = capture_default_ax_snapshot(&request)?;
    if snapshot.capture_status != "complete" {
        return Err(ax_snapshot_status_error(&snapshot));
    }

    let target_id = resolve_target_id_in_snapshot(&snapshot, target)?;
    for window in &snapshot.windows {
        if window.id == target_id {
            return Ok(AxResolvedTargetRect {
                target_id,
                target_type: "window",
                window_id: Some(window.id.clone()),
                rect: window.rect,
            });
        }

        if let Some(element) = find_ax_element_by_id(&window.elements, &target_id) {
            return Ok(AxResolvedTargetRect {
                target_id,
                target_type: "element",
                window_id: Some(window.id.clone()),
                rect: element.rect,
            });
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("AX target id 已失效或不存在: {target_id}"),
    ))
}

pub fn perform_default_ax_press(request: &AxPressRequest) -> io::Result<AxActionReport> {
    let report = SystemAxBackend.perform_action(&AxActionRequest {
        target: request.target.clone(),
        action: AxActionName::Press,
    })?;
    Ok(AxActionReport::press(report.backend, report.target_id))
}

pub fn perform_default_ax_action(request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
    SystemAxBackend.perform_action(request)
}

pub fn perform_default_ax_set_value(request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
    SystemAxBackend.set_value(request)
}

pub fn perform_default_key_delivery(request: &KeyRequest) -> io::Result<Option<KeyDeliveryReport>> {
    match request.delivery {
        KeyDelivery::Global => Ok(None),
        KeyDelivery::PidTargeted | KeyDelivery::WindowTargeted => {
            platform_key_delivery(request).map(Some)
        }
    }
}

pub fn perform_default_ax_focus(request: &AxFocusRequest) -> io::Result<AxFocusReport> {
    SystemAxBackend.focus(request)
}

pub fn perform_default_ax_scroll(request: &AxScrollRequest) -> io::Result<AxScrollReport> {
    SystemAxBackend.scroll(request)
}

pub fn perform_default_type_text(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
    SystemAxBackend.type_text(request)
}

pub fn current_ax_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "unsupported"
    }
}

pub fn parse_ax_tree_payload(input: &str) -> io::Result<AxTreeRequest> {
    let inner = object_inner(input, "@ax-tree")?;
    if inner.is_empty() {
        return Ok(AxTreeRequest::default());
    }

    let mut scope = None::<AxTreeScope>;
    let mut mode = None::<AxMode>;
    let mut depth = None::<u8>;
    let mut max_elements = None::<u16>;
    let mut include_values = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "scope" => assign_once(
                &mut scope,
                "scope",
                "@ax-tree",
                parse_ax_tree_scope(raw_value)?,
            )?,
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@ax-tree",
                parse_ax_mode_payload("@ax-tree", raw_value)?,
            )?,
            "depth" => assign_once(&mut depth, "depth", "@ax-tree", parse_ax_depth(raw_value)?)?,
            "max_elements" => assign_once(
                &mut max_elements,
                "max_elements",
                "@ax-tree",
                parse_ax_max_elements(raw_value)?,
            )?,
            "include_values" => assign_once(
                &mut include_values,
                "include_values",
                "@ax-tree",
                parse_bool_literal("@ax-tree", "include_values", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-tree 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let preset = mode.unwrap_or(AxMode::Full).preset();
    Ok(AxTreeRequest {
        scope: scope.unwrap_or(AxTreeScope::Windows),
        depth: depth.unwrap_or(preset.depth),
        max_elements: max_elements.unwrap_or(preset.max_elements),
        include_values: include_values.unwrap_or(preset.include_values),
    })
}

pub fn parse_ax_press_payload(input: &str) -> io::Result<AxPressRequest> {
    let inner = object_inner(input, "@ax-press")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-press 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-press",
                parse_ax_target(raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-press 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxPressRequest {
        target: required_field(target, "@ax-press", "target")?,
    })
}

pub fn parse_ax_action_payload(input: &str) -> io::Result<AxActionRequest> {
    let inner = object_inner(input, "@ax-action")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-action 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut action = None::<AxActionName>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-action",
                parse_ax_target(raw_value)?,
            )?,
            "action" => assign_once(
                &mut action,
                "action",
                "@ax-action",
                parse_ax_action_name(raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-action 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxActionRequest {
        target: required_field(target, "@ax-action", "target")?,
        action: required_field(action, "@ax-action", "action")?,
    })
}

pub fn parse_ax_set_value_payload(input: &str) -> io::Result<AxSetValueRequest> {
    let inner = object_inner(input, "@ax-set-value")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-set-value 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut value = None::<String>;
    let mut mode = None::<AxValueSetMode>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-set-value",
                parse_ax_target(raw_value)?,
            )?,
            "value" => assign_once(
                &mut value,
                "value",
                "@ax-set-value",
                parse_quoted_payload(raw_value)?,
            )?,
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@ax-set-value",
                parse_ax_value_mode(raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-set-value 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxSetValueRequest {
        target: required_field(target, "@ax-set-value", "target")?,
        value: required_field(value, "@ax-set-value", "value")?,
        mode: mode.unwrap_or(AxValueSetMode::Replace),
    })
}

pub fn parse_ax_focus_payload(input: &str) -> io::Result<AxFocusRequest> {
    let inner = object_inner(input, "@ax-focus")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-focus 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut window_id = None::<String>;
    let mut activate = None::<bool>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-focus",
                parse_ax_target(raw_value)?,
            )?,
            "window_id" => assign_once(
                &mut window_id,
                "window_id",
                "@ax-focus",
                parse_non_empty_string("@ax-focus.window_id", raw_value)?,
            )?,
            "activate" => assign_once(
                &mut activate,
                "activate",
                "@ax-focus",
                parse_bool_literal("@ax-focus", "activate", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-focus 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    if target.is_none() && window_id.is_none() {
        return Err(invalid_data("@ax-focus 至少需要 `target` 或 `window_id`"));
    }
    if target.is_some() && window_id.is_some() {
        return Err(invalid_data(
            "@ax-focus 不能同时携带 `target` 和 `window_id`",
        ));
    }

    Ok(AxFocusRequest {
        target,
        window_id,
        activate: activate.unwrap_or(false),
    })
}

pub fn parse_ax_scroll_payload(input: &str) -> io::Result<AxScrollRequest> {
    let inner = object_inner(input, "@ax-scroll")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-scroll 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut direction = None::<AxScrollDirection>;
    let mut pages = None::<u16>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-scroll",
                parse_ax_target(raw_value)?,
            )?,
            "direction" => assign_once(
                &mut direction,
                "direction",
                "@ax-scroll",
                parse_ax_scroll_direction(raw_value)?,
            )?,
            "pages" => assign_once(
                &mut pages,
                "pages",
                "@ax-scroll",
                parse_ax_scroll_pages(raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-scroll 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxScrollRequest {
        target: required_field(target, "@ax-scroll", "target")?,
        direction: required_field(direction, "@ax-scroll", "direction")?,
        pages: pages.unwrap_or(1),
    })
}

pub fn parse_type_text_payload(input: &str) -> io::Result<TypeTextRequest> {
    let inner = object_inner(input, "@type-text")?;
    if inner.is_empty() {
        return Err(invalid_data("@type-text 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut text = None::<String>;
    let mut mode = None::<TypeTextMode>;
    let mut allow_clipboard = None::<bool>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@type-text",
                parse_ax_target(raw_value)?,
            )?,
            "text" => assign_once(
                &mut text,
                "text",
                "@type-text",
                parse_quoted_payload(raw_value)?,
            )?,
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@type-text",
                parse_type_text_mode(raw_value)?,
            )?,
            "allow_clipboard" => assign_once(
                &mut allow_clipboard,
                "allow_clipboard",
                "@type-text",
                parse_bool_literal("@type-text", "allow_clipboard", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@type-text 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let mode = mode.unwrap_or(TypeTextMode::Auto);
    let allow_clipboard = allow_clipboard.unwrap_or(false);
    if matches!(mode, TypeTextMode::Clipboard) && !allow_clipboard {
        return Err(invalid_data(
            "@type-text mode:\"clipboard\" 需要显式 `allow_clipboard:true`",
        ));
    }

    Ok(TypeTextRequest {
        target: required_field(target, "@type-text", "target")?,
        text: required_field(text, "@type-text", "text")?,
        mode,
        allow_clipboard,
    })
}

pub fn resolve_target_id_in_snapshot(
    snapshot: &AxSnapshot,
    target: &AxTarget,
) -> io::Result<String> {
    target.validate().map_err(to_invalid_input)?;

    if let Some(id) = &target.id {
        if snapshot.contains_element_id(id) {
            return Ok(id.clone());
        }
        return Err(invalid_input(format!("AX target id 已失效或不存在: {id}")));
    }

    if let (Some(observation_id), Some(ref_id)) =
        (target.observation_id.as_deref(), target.ref_id.as_deref())
    {
        let entry = resolve_observation_ref(observation_id, ref_id)?;
        if snapshot.contains_element_id(&entry.backend_id) {
            return Ok(entry.backend_id);
        }
        return Err(stale_observation_ref_error(
            observation_id,
            ref_id,
            format!("backend id 已不在当前 AX snapshot 中: {}", entry.backend_id),
        ));
    }

    let mut matches = Vec::<String>::new();

    for window in &snapshot.windows {
        if !target.matches_window(window) {
            continue;
        }
        collect_matching_element_ids(target, &window.elements, &mut matches);
        if matches.len() > 1 {
            return Err(invalid_input("AX semantic target 匹配到多个元素"));
        }
    }

    match matches.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(invalid_input("AX semantic target 未匹配到元素")),
        _ => Err(invalid_input("AX semantic target 匹配到多个元素")),
    }
}

fn collect_matching_element_ids(
    target: &AxTarget,
    elements: &[AxElement],
    matches: &mut Vec<String>,
) {
    for element in elements {
        if target.matches_element(element) {
            matches.push(element.id.clone());
        }
        collect_matching_element_ids(target, &element.children, matches);
    }
}

fn find_ax_element_by_id<'a>(elements: &'a [AxElement], target_id: &str) -> Option<&'a AxElement> {
    for element in elements {
        if element.id == target_id {
            return Some(element);
        }
        if let Some(found) = find_ax_element_by_id(&element.children, target_id) {
            return Some(found);
        }
    }
    None
}

fn ax_snapshot_status_error(snapshot: &AxSnapshot) -> io::Error {
    let kind = match snapshot.capture_status.as_str() {
        "permission_denied" => io::ErrorKind::PermissionDenied,
        "unsupported" => io::ErrorKind::Unsupported,
        _ => io::ErrorKind::Other,
    };
    let value = json!({
        "kind": "ax-target-resolution",
        "error_code": "AX_SNAPSHOT_UNAVAILABLE",
        "capture_status": snapshot.capture_status.as_str(),
        "permission_status": snapshot.permission_status.as_str(),
        "platform": snapshot.platform.as_str(),
        "message": "AX snapshot 不可用,无法解析 mouse target rect",
    });
    io::Error::new(kind, value.to_string())
}

fn parse_ax_target(input: &str) -> io::Result<AxTarget> {
    let inner = object_inner(input, "AX target")?;
    if inner.is_empty() {
        return Err(invalid_data("AX target 不能为空"));
    }

    let mut target = AxTarget::default();
    let mut id_seen = false;
    let mut ref_seen = false;
    let mut observation_id_seen = false;
    let mut process_seen = false;
    let mut window_title_seen = false;
    let mut role_seen = false;
    let mut subrole_seen = false;
    let mut name_seen = false;
    let mut description_seen = false;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "id" => {
                reject_duplicate(&mut id_seen, "AX target", "id")?;
                target.id = Some(parse_non_empty_string("AX target.id", raw_value)?);
            }
            "ref" | "ref_id" => {
                reject_duplicate(&mut ref_seen, "AX target", "ref")?;
                target.ref_id = Some(parse_non_empty_string("AX target.ref", raw_value)?);
            }
            "observation_id" => {
                reject_duplicate(&mut observation_id_seen, "AX target", "observation_id")?;
                target.observation_id = Some(parse_non_empty_string(
                    "AX target.observation_id",
                    raw_value,
                )?);
            }
            "process" | "process_name" => {
                reject_duplicate(&mut process_seen, "AX target", "process")?;
                target.process = Some(parse_non_empty_string("AX target.process", raw_value)?);
            }
            "window_title" | "title" => {
                reject_duplicate(&mut window_title_seen, "AX target", "window_title")?;
                target.window_title =
                    Some(parse_non_empty_string("AX target.window_title", raw_value)?);
            }
            "role" => {
                reject_duplicate(&mut role_seen, "AX target", "role")?;
                target.role = Some(parse_non_empty_string("AX target.role", raw_value)?);
            }
            "subrole" => {
                reject_duplicate(&mut subrole_seen, "AX target", "subrole")?;
                target.subrole = Some(parse_non_empty_string("AX target.subrole", raw_value)?);
            }
            "name" => {
                reject_duplicate(&mut name_seen, "AX target", "name")?;
                target.name = Some(parse_non_empty_string("AX target.name", raw_value)?);
            }
            "description" => {
                reject_duplicate(&mut description_seen, "AX target", "description")?;
                target.description =
                    Some(parse_non_empty_string("AX target.description", raw_value)?);
            }
            _ => {
                return Err(invalid_data(format!(
                    "AX target 包含未知字段: {field_name}"
                )))
            }
        }
    }

    target.validate()?;
    Ok(target)
}

fn parse_ax_action_name(input: &str) -> io::Result<AxActionName> {
    let value = parse_quoted_payload(input)?;
    match value.to_ascii_lowercase().as_str() {
        "axpress" | "press" => Ok(AxActionName::Press),
        "axopen" | "open" => Ok(AxActionName::Open),
        "axconfirm" | "confirm" => Ok(AxActionName::Confirm),
        "axcancel" | "cancel" => Ok(AxActionName::Cancel),
        "axshowmenu" | "showmenu" | "show_menu" => Ok(AxActionName::ShowMenu),
        "axscrolltovisible" | "scrolltovisible" | "scroll_to_visible" => {
            Ok(AxActionName::ScrollToVisible)
        }
        _ => Err(invalid_data(format!(
            "@ax-action 当前只支持安全 action allowlist: {value}"
        ))),
    }
}

fn parse_ax_value_mode(input: &str) -> io::Result<AxValueSetMode> {
    let value = parse_quoted_payload(input)?;
    match value.to_ascii_lowercase().as_str() {
        "replace" => Ok(AxValueSetMode::Replace),
        "append" => Ok(AxValueSetMode::Append),
        _ => Err(invalid_data(format!(
            "@ax-set-value 当前只支持 mode=\"replace\" | \"append\": {value}"
        ))),
    }
}

fn parse_type_text_mode(input: &str) -> io::Result<TypeTextMode> {
    let value = parse_quoted_payload(input)?;
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(TypeTextMode::Auto),
        "ax-value" | "ax_value" => Ok(TypeTextMode::AxValue),
        "targeted-keyboard" | "targeted_keyboard" => Ok(TypeTextMode::TargetedKeyboard),
        "clipboard" => Ok(TypeTextMode::Clipboard),
        _ => Err(invalid_data(format!(
            "@type-text 当前只支持 mode=\"auto\" | \"ax-value\" | \"targeted-keyboard\" | \"clipboard\": {value}"
        ))),
    }
}

fn parse_ax_scroll_direction(input: &str) -> io::Result<AxScrollDirection> {
    let value = parse_quoted_payload(input)?;
    match value.to_ascii_lowercase().as_str() {
        "up" => Ok(AxScrollDirection::Up),
        "down" => Ok(AxScrollDirection::Down),
        "left" => Ok(AxScrollDirection::Left),
        "right" => Ok(AxScrollDirection::Right),
        _ => Err(invalid_data(format!(
            "@ax-scroll 的 `direction` 只支持 \"up\" | \"down\" | \"left\" | \"right\": {value}"
        ))),
    }
}

fn parse_ax_scroll_pages(input: &str) -> io::Result<u16> {
    let pages = input
        .parse::<u16>()
        .map_err(|_| invalid_data(format!("@ax-scroll 的 `pages` 必须是正整数: {input}")))?;
    if pages == 0 {
        return Err(invalid_data("@ax-scroll 的 `pages` 必须大于 0"));
    }
    Ok(pages)
}

fn remap_type_text_ax_value_error(err: io::Error) -> io::Error {
    let message = err.to_string();
    match err.kind() {
        io::ErrorKind::Unsupported => io::Error::new(
            io::ErrorKind::Unsupported,
            "type-text 当前只支持 macOS AXValue 路径",
        ),
        io::ErrorKind::InvalidInput => io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("type-text AXValue 路径失败: {message}"),
        ),
        io::ErrorKind::PermissionDenied => io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("type-text AXValue 路径失败: {message}"),
        ),
        _ => io::Error::other(format!("type-text AXValue 路径失败: {message}")),
    }
}

fn remap_type_text_targeted_keyboard_error(err: io::Error) -> io::Error {
    let message = err.to_string();
    match err.kind() {
        io::ErrorKind::Unsupported => io::Error::new(
            io::ErrorKind::Unsupported,
            "type-text 当前只支持 macOS targeted keyboard 路径",
        ),
        io::ErrorKind::InvalidInput => io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("type-text targeted keyboard 路径失败: {message}"),
        ),
        io::ErrorKind::PermissionDenied => io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("type-text targeted keyboard 路径失败: {message}"),
        ),
        _ => io::Error::other(format!("type-text targeted keyboard 路径失败: {message}")),
    }
}

fn key_mode_as_str(mode: KeyMode) -> &'static str {
    match mode {
        KeyMode::PressRelease => "press_release",
        KeyMode::Press => "press",
        KeyMode::Release => "release",
    }
}

fn parse_ax_tree_scope(input: &str) -> io::Result<AxTreeScope> {
    let scope = parse_quoted_payload(input)?;
    match scope.to_ascii_lowercase().as_str() {
        "windows" => Ok(AxTreeScope::Windows),
        _ => Err(invalid_data(format!(
            "@ax-tree 当前只支持 scope=\"windows\": {scope}"
        ))),
    }
}

pub(crate) fn parse_ax_mode_payload(kind: &str, input: &str) -> io::Result<AxMode> {
    let mode = parse_quoted_payload(input)?;
    match mode.to_ascii_lowercase().as_str() {
        "windows" | "summary" | "skeleton" => Ok(AxMode::Windows),
        "interactive" | "controls" => Ok(AxMode::Interactive),
        "full" => Ok(AxMode::Full),
        _ => Err(invalid_data(format!(
            "{kind} 当前只支持 mode/ax_mode=\"windows\" | \"skeleton\" | \"interactive\" | \"full\": {mode}"
        ))),
    }
}

pub(crate) fn parse_ax_depth(input: &str) -> io::Result<u8> {
    let depth = input
        .parse::<u8>()
        .map_err(|_| invalid_data(format!("@ax-tree 的 `depth` 必须是无符号整数: {input}")))?;
    if depth == 0 {
        return Err(invalid_data("@ax-tree 的 `depth` 必须大于 0"));
    }
    Ok(depth)
}

pub(crate) fn parse_ax_max_elements(input: &str) -> io::Result<u16> {
    let max_elements = input.parse::<u16>().map_err(|_| {
        invalid_data(format!(
            "@ax-tree 的 `max_elements` 必须是无符号整数: {input}"
        ))
    })?;
    if max_elements == 0 {
        return Err(invalid_data("@ax-tree 的 `max_elements` 必须大于 0"));
    }
    Ok(max_elements)
}

pub(crate) fn parse_bool_literal(kind: &str, field_name: &str, input: &str) -> io::Result<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_data(format!(
            "{kind} 的 `{field_name}` 必须是 true 或 false: {input}"
        ))),
    }
}

fn parse_non_empty_string(kind: &str, input: &str) -> io::Result<String> {
    let value = parse_quoted_payload(input)?;
    if value.is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空")));
    }
    Ok(value)
}

fn matches_optional(expected: &Option<String>, actual: Option<&str>) -> bool {
    match expected {
        Some(expected) => actual == Some(expected.as_str()),
        None => true,
    }
}

fn assign_once<T>(slot: &mut Option<T>, field_name: &str, kind: &str, value: T) -> io::Result<()> {
    if slot.is_some() {
        return Err(invalid_data(format!(
            "{kind} 对象 payload 的 `{field_name}` 字段重复"
        )));
    }
    *slot = Some(value);
    Ok(())
}

fn reject_duplicate(seen: &mut bool, kind: &str, field_name: &str) -> io::Result<()> {
    if *seen {
        return Err(invalid_data(format!("{kind} 的 `{field_name}` 字段重复")));
    }
    *seen = true;
    Ok(())
}

fn required_field<T>(value: Option<T>, kind: &str, field_name: &str) -> io::Result<T> {
    value.ok_or_else(|| invalid_data(format!("{kind} 对象 payload 缺少必填字段 `{field_name}`")))
}

fn to_invalid_input(err: io::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, err.to_string())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(target_os = "macos")]
fn platform_snapshot(request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    macos::snapshot(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_snapshot(_request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX snapshot 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
fn platform_perform_action(request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
    macos::perform_action(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_perform_action(_request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX action 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
fn platform_set_value(request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
    macos::set_value(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_set_value(_request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX set value 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
fn platform_key_delivery(request: &KeyRequest) -> io::Result<KeyDeliveryReport> {
    macos::deliver_key(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_key_delivery(request: &KeyRequest) -> io::Result<KeyDeliveryReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("key delivery {:?} 当前只支持 macOS", request.delivery),
    ))
}

#[cfg(target_os = "macos")]
fn platform_focus(request: &AxFocusRequest) -> io::Result<AxFocusReport> {
    macos::focus(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_focus(_request: &AxFocusRequest) -> io::Result<AxFocusReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX focus 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
fn platform_scroll(request: &AxScrollRequest) -> io::Result<AxScrollReport> {
    macos::scroll(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_scroll(_request: &AxScrollRequest) -> io::Result<AxScrollReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX scroll 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
fn platform_type_text(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
    macos::type_text(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_type_text(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
    let detail = match request.mode {
        TypeTextMode::Auto | TypeTextMode::AxValue => "macOS AXValue 路径",
        TypeTextMode::TargetedKeyboard => "macOS targeted keyboard 路径",
        TypeTextMode::Clipboard => "macOS clipboard 路径",
    };
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("type-text 当前只支持 {detail}"),
    ))
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ax_snapshot_should_count_nested_elements_and_render_tree_response() {
        let snapshot = AxSnapshot::complete(
            "macos",
            vec![AxWindow {
                id: "pid:1/window:0".to_owned(),
                ref_id: None,
                pid: 1,
                process_name: "System Information".to_owned(),
                title: Some("关于本机".to_owned()),
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
                    name: Some("关闭".to_owned()),
                    value: None,
                    value_redacted: false,
                    description: Some("关闭按钮".to_owned()),
                    rect: None,
                    enabled: Some(true),
                    actions: vec!["AXPress".to_owned()],
                    ax_path: vec![0],
                    children: Vec::new(),
                }],
            }],
            false,
        );

        assert_eq!(snapshot.window_count, 1);
        assert_eq!(snapshot.element_count, 1);
        let value = snapshot.to_tree_value_json().unwrap();
        assert!(value.contains(r#""kind":"ax-tree""#));
        assert!(value.contains(r#""schema":"rdog.ax.v1""#));

        let observed = snapshot.with_observation("@ax-tree").unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&observed.to_tree_value_json().unwrap()).unwrap();
        assert_eq!(value["observation"]["scope"], "ax");
        assert_eq!(value["observation"]["source_command"], "@ax-tree");
        assert_eq!(value["observation"]["ref_count"], 2);
        assert_eq!(value["observation"]["selector_count"], 2);
        assert_eq!(value["windows"][0]["ref"], "@e1");
        assert_eq!(value["windows"][0]["elements"][0]["ref"], "@e2");
    }

    #[test]
    fn resolve_target_should_reject_stale_or_ambiguous_locators() {
        let button = |id: &str| AxElement {
            id: id.to_owned(),
            ref_id: None,
            role: "AXButton".to_owned(),
            subrole: None,
            name: Some("OK".to_owned()),
            value: None,
            value_redacted: false,
            description: None,
            rect: None,
            enabled: Some(true),
            actions: vec!["AXPress".to_owned()],
            ax_path: vec![0],
            children: Vec::new(),
        };
        let snapshot = AxSnapshot::complete(
            "macos",
            vec![AxWindow {
                id: "pid:1/window:0".to_owned(),
                ref_id: None,
                pid: 1,
                process_name: "App".to_owned(),
                title: Some("Win".to_owned()),
                role: "AXWindow".to_owned(),
                subrole: None,
                rect: None,
                focused: None,
                elements: vec![
                    button("pid:1/window:0/path:0"),
                    button("pid:1/window:0/path:1"),
                ],
            }],
            false,
        );

        let target = AxTarget {
            id: Some("pid:1/window:0/path:404".to_owned()),
            ..AxTarget::default()
        };
        assert_eq!(
            resolve_target_id_in_snapshot(&snapshot, &target)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );

        let target = AxTarget {
            process: Some("App".to_owned()),
            window_title: Some("Win".to_owned()),
            role: Some("AXButton".to_owned()),
            name: Some("OK".to_owned()),
            ..AxTarget::default()
        };
        assert_eq!(
            resolve_target_id_in_snapshot(&snapshot, &target)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn secure_element_should_serialize_redaction_without_value() {
        let element = AxElement {
            id: "pid:1/window:0/path:0".to_owned(),
            ref_id: None,
            role: "AXSecureTextField".to_owned(),
            subrole: None,
            name: Some("Password".to_owned()),
            value: None,
            value_redacted: true,
            description: None,
            rect: None,
            enabled: Some(true),
            actions: Vec::new(),
            ax_path: vec![0],
            children: Vec::new(),
        };
        let value = serde_json::to_value(&element).unwrap();
        assert_eq!(value["value_redacted"], true);
        assert!(value.get("value").is_none());
    }

    #[test]
    fn parse_ax_tree_payload_should_validate_limits() {
        assert_eq!(
            parse_ax_tree_payload(
                r#"{scope:"windows",mode:"interactive",depth:4,max_elements:1000,include_values:false}"#
            )
            .unwrap(),
            AxTreeRequest {
                scope: AxTreeScope::Windows,
                depth: 4,
                max_elements: 1000,
                include_values: false,
            }
        );
        assert!(parse_ax_tree_payload(r#"{depth:0}"#).is_err());
        assert!(parse_ax_tree_payload(r#"{max_elements:0}"#).is_err());
        assert_eq!(
            parse_ax_tree_payload(r#"{mode:"windows"}"#).unwrap(),
            AxTreeRequest {
                scope: AxTreeScope::Windows,
                depth: AX_WINDOWS_DEPTH,
                max_elements: AX_WINDOWS_MAX_ELEMENTS,
                include_values: AX_WINDOWS_INCLUDE_VALUES,
            }
        );
    }

    #[test]
    fn parse_ax_press_payload_should_require_target() {
        assert_eq!(
            parse_ax_press_payload(r#"{target:{id:"pid:1/window:0/path:0"}}"#).unwrap(),
            AxPressRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
            }
        );
        assert!(parse_ax_press_payload(r#"{target:{}}"#).is_err());
        assert!(parse_ax_press_payload(r#"{target:{process:"App"}}"#).is_err());

        let request =
            parse_ax_press_payload(r#"{target:{ref:"@e2",observation_id:"obs-1"}}"#).unwrap();
        assert_eq!(request.target.ref_id.as_deref(), Some("@e2"));
        assert_eq!(request.target.observation_id.as_deref(), Some("obs-1"));

        assert!(parse_ax_press_payload(r#"{target:{ref:"@e2"}}"#).is_err());
        assert!(parse_ax_press_payload(
            r#"{target:{ref:"@e2",observation_id:"obs-1",role:"AXButton"}}"#
        )
        .is_err());
    }

    #[test]
    fn parse_ax_action_payload_should_support_allowlisted_actions() {
        assert_eq!(
            parse_ax_action_payload(r#"{target:{id:"pid:1/window:0/path:0"},action:"AXShowMenu"}"#)
                .unwrap(),
            AxActionRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
                action: AxActionName::ShowMenu,
            }
        );
        assert!(parse_ax_action_payload(
            r#"{target:{id:"pid:1/window:0/path:0"},action:"AXRaise"}"#
        )
        .is_err());
    }

    #[test]
    fn parse_ax_action_payload_should_report_generic_ax_target_errors() {
        let error = parse_ax_action_payload(
            r#"{target:{id:"pid:1/window:0/path:0",id:"pid:1/window:0/path:1"},action:"AXPress"}"#,
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("AX target"), "unexpected error: {message}");
        assert!(
            !message.contains("@ax-press target"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn parse_ax_set_value_payload_should_allow_empty_value_and_append_mode() {
        assert_eq!(
            parse_ax_set_value_payload(
                r#"{target:{id:"pid:1/window:0/path:0"},value:"",mode:"append"}"#
            )
            .unwrap(),
            AxSetValueRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
                value: String::new(),
                mode: AxValueSetMode::Append,
            }
        );
    }

    #[test]
    fn ax_set_value_report_should_keep_real_redaction_state() {
        assert_eq!(
            AxSetValueReport::success(
                "macos-accessibility",
                Some("pid:1/window:0/path:0".to_owned()),
                AxValueSetMode::Append,
                true,
                true,
            ),
            AxSetValueReport {
                kind: "ax-set-value",
                backend: "macos-accessibility".to_owned(),
                target_id: Some("pid:1/window:0/path:0".to_owned()),
                mode: "append",
                performed: true,
                status: "ok",
                settable: true,
                old_value_redacted: true,
                new_value_redacted: true,
            }
        );
    }

    #[test]
    fn parse_type_text_payload_should_default_to_auto_without_clipboard() {
        assert_eq!(
            parse_type_text_payload(r#"{target:{id:"pid:1/window:0/path:0"},text:"hello"}"#)
                .unwrap(),
            TypeTextRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
                text: "hello".to_owned(),
                mode: TypeTextMode::Auto,
                allow_clipboard: false,
            }
        );
        assert_eq!(
            parse_type_text_payload(
                r#"{target:{id:"pid:1/window:0/path:0"},text:"hello",mode:"targeted-keyboard"}"#
            )
            .unwrap(),
            TypeTextRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
                text: "hello".to_owned(),
                mode: TypeTextMode::TargetedKeyboard,
                allow_clipboard: false,
            }
        );
        let error = parse_type_text_payload(
            r#"{target:{id:"pid:1/window:0/path:0"},text:"hello",mode:"clipboard"}"#,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("mode:\"clipboard\" 需要显式 `allow_clipboard:true`"),
            "unexpected error: {error}"
        );
        assert_eq!(
            parse_type_text_payload(
                r#"{target:{id:"pid:1/window:0/path:0"},text:"hello",mode:"clipboard",allow_clipboard:true}"#
            )
            .unwrap(),
            TypeTextRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
                text: "hello".to_owned(),
                mode: TypeTextMode::Clipboard,
                allow_clipboard: true,
            }
        );
    }

    #[test]
    fn type_text_clipboard_report_should_expose_restore_status() {
        let restored = TypeTextReport::clipboard_success(
            "macos-clipboard+cg-event-post-to-pid",
            Some("pid:1/window:0/path:0".to_owned()),
            ClipboardRestoreStatus::restored(),
        );
        let restored_json = serde_json::to_value(restored).unwrap();
        assert_eq!(restored_json["kind"], "type-text");
        assert_eq!(restored_json["mode"], "clipboard");
        assert_eq!(restored_json["used_clipboard"], true);
        assert_eq!(
            restored_json["clipboard_restore_policy"],
            "restore-if-unchanged"
        );
        assert_eq!(restored_json["clipboard_restored"], true);
        assert!(restored_json
            .get("clipboard_restore_skipped_reason")
            .is_none());

        let skipped = TypeTextReport::clipboard_success(
            "macos-clipboard+cg-event-post-to-pid",
            Some("pid:1/window:0/path:0".to_owned()),
            ClipboardRestoreStatus::skipped("clipboard-changed"),
        );
        let skipped_json = serde_json::to_value(skipped).unwrap();
        assert_eq!(skipped_json["clipboard_restored"], false);
        assert_eq!(
            skipped_json["clipboard_restore_skipped_reason"],
            "clipboard-changed"
        );
    }

    #[test]
    fn parse_ax_focus_payload_should_accept_target_or_window_id() {
        assert_eq!(
            parse_ax_focus_payload(r#"{window_id:"pid:1/window:0",activate:true}"#).unwrap(),
            AxFocusRequest {
                target: None,
                window_id: Some("pid:1/window:0".to_owned()),
                activate: true,
            }
        );
        assert_eq!(
            parse_ax_focus_payload(r#"{target:{id:"pid:1/window:0/path:0"}}"#).unwrap(),
            AxFocusRequest {
                target: Some(AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                }),
                window_id: None,
                activate: false,
            }
        );
        assert!(parse_ax_focus_payload(r#"{}"#).is_err());
        assert!(parse_ax_focus_payload(
            r#"{window_id:"pid:1/window:0",target:{id:"pid:1/window:0/path:0"}}"#
        )
        .is_err());
    }

    #[test]
    fn parse_ax_scroll_payload_should_accept_direction_and_pages() {
        assert_eq!(
            parse_ax_scroll_payload(
                r#"{target:{id:"pid:1/window:0/path:0"},direction:"down",pages:2}"#
            )
            .unwrap(),
            AxScrollRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
                direction: AxScrollDirection::Down,
                pages: 2,
            }
        );
        assert!(parse_ax_scroll_payload(
            r#"{target:{id:"pid:1/window:0/path:0"},direction:"spin"}"#
        )
        .is_err());
    }

    #[test]
    fn remap_type_text_ax_value_error_should_use_type_text_protocol_name() {
        let unsupported = remap_type_text_ax_value_error(io::Error::new(
            io::ErrorKind::Unsupported,
            "AX set value 当前只支持 macOS",
        ));
        assert_eq!(unsupported.kind(), io::ErrorKind::Unsupported);
        assert!(
            unsupported
                .to_string()
                .contains("type-text 当前只支持 macOS AXValue 路径"),
            "unexpected error: {unsupported}"
        );

        let invalid = remap_type_text_ax_value_error(io::Error::new(
            io::ErrorKind::InvalidInput,
            "目标 AX 元素不支持 AXValue",
        ));
        assert_eq!(invalid.kind(), io::ErrorKind::InvalidInput);
        assert!(
            invalid.to_string().contains("type-text AXValue 路径失败"),
            "unexpected error: {invalid}"
        );
    }
}
