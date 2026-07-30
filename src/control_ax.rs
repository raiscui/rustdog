use crate::{
    control_observation::selector::{
        AppSelector, DurableSelectorDraft, ElementSelector, SelectorEnvelope, SelectorKind,
        SelectorRect, SelectorRedaction, WindowSelector,
    },
    control_observation::{
        observation_ref_name, record_observation_with_selectors, resolve_observation_ref,
        stale_observation_ref_error, ObservationRefEntry, ObservationRoot,
    },
    control_protocol::{
        normalize_object_field_name, object_inner, parse_compact_atom,
        parse_compact_ax_button_sequence, parse_compact_window_pair, parse_compact_window_selector,
        parse_quoted_payload, split_object_field, split_object_fields, CompactWindowSelector,
        KeyDelivery, KeyMode, KeyRequest,
    },
    control_window::{resolve_unique_app_window_id, WindowActionReport, WindowActionVerifyReport},
};
use serde_json::json;
use std::io;


pub mod types;
pub mod tree;
pub use self::tree::{
    capture_ax_find_snapshot, capture_current_ax_subtree, capture_current_ax_window_snapshot,
    capture_default_ax_snapshot, current_ax_platform,
    resolve_current_ax_target_rect, resolve_target_id_in_snapshot,
};
pub mod input;
pub use self::input::{perform_default_key_delivery, perform_default_type_text};
use self::input::{remap_type_text_ax_value_error, remap_type_text_targeted_keyboard_error};
pub mod press;
pub use self::press::{
    perform_default_ax_action, perform_default_ax_focus,
    perform_default_ax_press, perform_default_ax_press_sequence,
    perform_default_ax_press_with_postcondition,
    perform_default_ax_scroll, perform_default_ax_set_value,
};
use self::tree::{
    collect_element_refs, window_selector_draft, element_selector_draft,
    app_selector_for_window, window_selector_for_ax_window, selector_rect_from_ax_rect,
    reserve_existing_ref_index, capture_ax_find_snapshot_with,
    capture_semantic_target_snapshot,
    capture_semantic_target_snapshot_with, direct_ax_target_id,
    materialize_app_window_target, materialize_app_window_target_with,
    collect_matching_element_ids, find_ax_element_by_id, ax_snapshot_status_error,
};
pub use self::types::*;


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


impl AxValueSetMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
        }
    }
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


impl AxTarget {
    fn validate(&self) -> io::Result<()> {
        let has_ref = self.ref_id.is_some();
        let has_observation_id = self.observation_id.is_some();
        let has_semantic = self.window_id.is_some()
            || self.app.is_some()
            || self.process.is_some()
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

        if self.app.is_some()
            && (self.window_id.is_some() || self.process.is_some() || self.window_title.is_some())
        {
            return Err(invalid_data(
                "AX target.app 不能与 window_id / process / window_title 混用",
            ));
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
        // window_id、进程名和标题是并列约束.任一条件冲突都必须拒绝该窗口,
        // 避免调用方提供的归属边界被较宽的语义字段覆盖.
        matches_optional(&self.window_id, Some(window.id.as_str()))
            && matches_optional(&self.process, Some(window.process_name.as_str()))
            && matches_optional(&self.window_title, window.title.as_deref())
    }

    fn matches_element(&self, element: &AxElement) -> bool {
        matches_optional(&self.role, Some(element.role.as_str()))
            && matches_optional(&self.subrole, element.subrole.as_deref())
            && matches_optional(&self.name, element.name.as_deref())
            && matches_optional(&self.description, element.description.as_deref())
    }
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


impl AxWindow {
    fn element_count(&self) -> usize {
        self.elements.iter().map(AxElement::tree_count).sum()
    }
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



impl AxPressPostconditionReport {
    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX guarded press 序列化失败: {err}")))
    }
}


impl AxPressSequenceStepReport {
    fn success(index: usize, description: String, report: AxActionReport) -> Self {
        Self {
            index,
            description,
            performed: report.performed,
            status: report.status,
            target_id: report.target_id,
            error: None,
        }
    }

    fn failed(index: usize, description: String, error: String) -> Self {
        Self {
            index,
            description,
            performed: false,
            status: "failed",
            target_id: None,
            error: Some(error),
        }
    }
}


impl AxPressSequenceReport {
    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX press sequence 序列化失败: {err}")))
    }
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
            error_code: None,
            activation: None,
        }
    }

    pub fn with_activation(mut self, activation: WindowActionReport) -> Self {
        self.activation = Some(activation);
        self
    }

    pub fn activation_failed(activation: WindowActionReport) -> Self {
        let backend = activation.platform.clone();
        let window_id = activation.window_id.clone();
        let error_code = activation.error_code.map(str::to_owned);
        Self {
            kind: "ax-focus",
            backend,
            target_id: None,
            window_id,
            activated: false,
            performed: false,
            status: "failed",
            error_code,
            activation: Some(activation),
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX focus response 序列化失败: {err}")))
    }
}

pub fn window_activation_verified(report: &WindowActionReport) -> bool {
    if report.status != "ok" {
        return false;
    }
    matches!(
        report.verify.as_ref(),
        Some(WindowActionVerifyReport::Activate(verify))
            if verify.status == "passed"
                && verify.focused
                && verify.frontmost
                && !verify.hidden
                && !verify.minimized
    )
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

pub mod query;

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
    let trimmed = input.trim();
    if !trimmed.starts_with('{') {
        let fields = trimmed.split(',').collect::<Vec<_>>();
        let (window_selector, description, postcondition) = match fields.as_slice() {
            [_, _] => {
                let (window_selector, description) =
                    parse_compact_window_pair("@ax-press", trimmed)?;
                (window_selector, description, None)
            }
            [selector, description, role, expected_value, max_attempts] => {
                let window_selector =
                    parse_compact_window_selector("@ax-press", selector)?;
                let description = parse_compact_atom("@ax-press", description)?;
                let role = parse_compact_atom("@ax-press", role)?;
                let expected_value = parse_compact_atom("@ax-press", expected_value)?;
                let max_attempts = parse_compact_atom("@ax-press", max_attempts)?
                    .parse::<usize>()
                    .ok()
                    .filter(|attempts| (1..=3).contains(attempts))
                    .ok_or_else(|| {
                        invalid_data("@ax-press postcondition max_attempts 必须是1到3")
                    })?;
                (
                    window_selector,
                    description,
                    Some(AxPressPostcondition {
                        role,
                        expected_value,
                        max_attempts,
                    }),
                )
            }
            _ => {
                return Err(invalid_data(
                    "@ax-press 短格式必须是selector,description或selector,description,role,expected_value,max_attempts",
                ))
            }
        };
        let mut target = AxTarget {
            role: Some("AXButton".to_owned()),
            description: Some(description),
            ..AxTarget::default()
        };
        match window_selector {
            CompactWindowSelector::WindowId(window_id) => target.window_id = Some(window_id),
            CompactWindowSelector::App(app) => target.app = Some(app),
        }
        target.validate()?;
        return Ok(AxPressRequest {
            target,
            postcondition,
        });
    }

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
        postcondition: None,
    })
}

pub fn parse_ax_press_sequence_payload(input: &str) -> io::Result<AxPressSequenceRequest> {
    let trimmed = input.trim();
    if trimmed.starts_with('{') {
        return Err(invalid_data(
            "@ax-press-sequence 当前只接受 shell-safe 短格式",
        ));
    }

    let (window_selector, descriptions) =
        parse_compact_ax_button_sequence("@ax-press-sequence", trimmed)?;
    let targets = descriptions
        .into_iter()
        .map(|description| {
            let mut target = AxTarget {
                role: Some("AXButton".to_owned()),
                description: Some(description),
                ..AxTarget::default()
            };
            match &window_selector {
                CompactWindowSelector::WindowId(window_id) => {
                    target.window_id = Some(window_id.clone());
                }
                CompactWindowSelector::App(app) => target.app = Some(app.clone()),
            }
            target.validate()?;
            Ok(target)
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(AxPressSequenceRequest { targets })
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





fn parse_ax_target(input: &str) -> io::Result<AxTarget> {
    let inner = object_inner(input, "AX target")?;
    if inner.is_empty() {
        return Err(invalid_data("AX target 不能为空"));
    }

    let mut target = AxTarget::default();
    let mut id_seen = false;
    let mut ref_seen = false;
    let mut observation_id_seen = false;
    let mut window_id_seen = false;
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
            "window_id" => {
                reject_duplicate(&mut window_id_seen, "AX target", "window_id")?;
                target.window_id = Some(parse_non_empty_string("AX target.window_id", raw_value)?);
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

pub(crate) fn to_invalid_input(err: io::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, err.to_string())
}

pub(crate) fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub(crate) fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_snapshot(request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    macos::snapshot(request)
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_capture_current_subtree(
    target_id: &str,
    request: &AxTreeRequest,
) -> io::Result<AxCapturedSubtree> {
    macos::capture_current_subtree(target_id, request)
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_capture_current_window(
    window_id: &str,
    request: &AxTreeRequest,
) -> io::Result<AxSnapshot> {
    macos::capture_window(window_id, request)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_capture_current_window(
    _window_id: &str,
    _request: &AxTreeRequest,
) -> io::Result<AxSnapshot> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX targeted window capture 当前只支持 macOS",
    ))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_capture_current_subtree(
    _target_id: &str,
    _request: &AxTreeRequest,
) -> io::Result<AxCapturedSubtree> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX subtree capture 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_resolve_current_target_rect(target_id: &str) -> io::Result<AxResolvedTargetRect> {
    macos::resolve_current_target_rect(target_id)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_resolve_current_target_rect(_target_id: &str) -> io::Result<AxResolvedTargetRect> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX target rect 当前只支持 macOS",
    ))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_snapshot(_request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX snapshot 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_perform_action(request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
    macos::perform_action(request)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_perform_action(_request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX action 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_set_value(request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
    macos::set_value(request)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_set_value(_request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX set value 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_key_delivery(request: &KeyRequest) -> io::Result<KeyDeliveryReport> {
    macos::deliver_key(request)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_key_delivery(request: &KeyRequest) -> io::Result<KeyDeliveryReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("key delivery {:?} 当前只支持 macOS", request.delivery),
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_focus(request: &AxFocusRequest) -> io::Result<AxFocusReport> {
    macos::focus(request)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_focus(_request: &AxFocusRequest) -> io::Result<AxFocusReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX focus 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_scroll(request: &AxScrollRequest) -> io::Result<AxScrollReport> {
    macos::scroll(request)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_scroll(_request: &AxScrollRequest) -> io::Result<AxScrollReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX scroll 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_type_text(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
    macos::type_text(request)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn platform_type_text(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
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
    use crate::control_ax::press::{
        materialize_press_sequence_request_with, normalize_ax_verification_value,
        observe_current_ax_values_with, perform_ax_press_sequence_with,
        perform_ax_press_with_postcondition_with,
    };
    use std::cell::Cell;

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
    fn direct_ax_target_id_should_resolve_ids_and_observation_refs_without_snapshot() {
        let direct = direct_ax_target_id(&AxTarget {
            id: Some("pid:1/window:0/path:2".to_owned()),
            ..AxTarget::default()
        })
        .expect("direct id should validate");
        assert_eq!(direct.as_deref(), Some("pid:1/window:0/path:2"));

        let header = record_observation_with_selectors(
            "ax",
            "@observe ax",
            ObservationRoot {
                schema: AX_SCHEMA.to_owned(),
                platform: "macos".to_owned(),
                coordinate_space: "os-logical".to_owned(),
            },
            vec![ObservationRefEntry {
                ref_id: "@e1".to_owned(),
                backend_id: "pid:7/window:0/path:3".to_owned(),
                kind: "ax-element".to_owned(),
            }],
            Vec::new(),
        )
        .expect("observation should record");

        let from_ref = direct_ax_target_id(&AxTarget {
            ref_id: Some("@e1".to_owned()),
            observation_id: Some(header.observation_id),
            ..AxTarget::default()
        })
        .expect("observation ref should resolve to backend id");
        assert_eq!(from_ref.as_deref(), Some("pid:7/window:0/path:3"));

        let semantic = direct_ax_target_id(&AxTarget {
            role: Some("AXButton".to_owned()),
            name: Some("OK".to_owned()),
            ..AxTarget::default()
        })
        .expect("semantic target should defer to snapshot resolver");
        assert!(semantic.is_none());
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
                postcondition: None,
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
    fn parse_ax_press_payload_should_accept_window_scoped_targets() {
        assert_eq!(
            parse_ax_press_payload("pid:123/window:0,1").unwrap(),
            AxPressRequest {
                target: AxTarget {
                    window_id: Some("pid:123/window:0".to_owned()),
                    role: Some("AXButton".to_owned()),
                    description: Some("1".to_owned()),
                    ..AxTarget::default()
                },
                postcondition: None,
            }
        );

        let compact_app = parse_ax_press_payload("app:Calculator,1").unwrap();
        assert_eq!(compact_app.target.app.as_deref(), Some("Calculator"));
        assert!(compact_app.target.window_id.is_none());

        let object = parse_ax_press_payload(
            r#"{target:{window_id:"pid:123/window:0",role:"AXButton",description:"1"}}"#,
        )
        .unwrap();
        assert_eq!(object.target.window_id.as_deref(), Some("pid:123/window:0"));
    }

    #[test]
    fn parse_ax_press_sequence_should_keep_order_and_reject_unsafe_fields() {
        let request =
            parse_ax_press_sequence_payload("app:Calculator,8,加,4,等于,乘,5,等于").unwrap();
        let descriptions = request
            .targets
            .iter()
            .map(|target| target.description.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(descriptions, ["8", "加", "4", "等于", "乘", "5", "等于"]);

        assert!(parse_ax_press_sequence_payload("app:Calculator").is_err());
        assert!(parse_ax_press_sequence_payload("app:,1").is_err());
        assert!(parse_ax_press_sequence_payload("app:Calculator App,1").is_err());
        let too_many = format!("app:Calculator,{}", vec!["1"; 33].join(","));
        assert!(parse_ax_press_sequence_payload(&too_many).is_err());
    }

    #[test]
    fn parse_ax_press_sequence_should_preserve_observed_descriptions() {
        // 通用AX协议只传递模型fresh观察到的按钮描述,不得按应用知识改写字段.
        let request = parse_ax_press_sequence_payload("app:Demo,+,减,确认").unwrap();
        let descriptions = request
            .targets
            .iter()
            .map(|target| target.description.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(descriptions, ["+", "减", "确认"]);

        // 通用compact atom继续拒绝不安全字符,但错误不能包含任务专用拆分hint.
        let error = parse_ax_press_sequence_payload("app:Demo,1*2")
            .unwrap_err()
            .to_string();
        assert!(!error.contains("逐按钮"));

        let trailing = parse_ax_press_sequence_payload("app:Demo,继续,确认,").unwrap();
        let trailing_descriptions = trailing
            .targets
            .iter()
            .map(|target| target.description.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(trailing_descriptions, ["继续", "确认"]);

        for empty_item in ["app:Demo,继续,,确认", "app:Demo,继续,确认,,"] {
            assert!(parse_ax_press_sequence_payload(empty_item).is_err());
        }
    }

    #[test]
    fn parse_ax_press_payload_should_accept_generic_postcondition() {
        let request = parse_ax_press_payload("app:Demo,重置,AXStaticText,ready,3").unwrap();
        assert_eq!(request.target.app.as_deref(), Some("Demo"));
        assert_eq!(request.target.description.as_deref(), Some("重置"));
        assert_eq!(
            request.postcondition,
            Some(AxPressPostcondition {
                role: "AXStaticText".to_owned(),
                expected_value: "ready".to_owned(),
                max_attempts: 3,
            })
        );

        for invalid_attempts in ["0", "4", "many"] {
            assert!(parse_ax_press_payload(&format!(
                "app:Demo,重置,AXStaticText,ready,{invalid_attempts}"
            ))
            .is_err());
        }
    }

    #[test]
    fn guarded_ax_press_should_stop_when_fresh_postcondition_matches() {
        let request = parse_ax_press_payload("app:Demo,重置,AXStaticText,ready,3").unwrap();
        let resolve_calls = Cell::new(0usize);
        let press_calls = Cell::new(0usize);
        let observe_calls = Cell::new(0usize);

        let report = perform_ax_press_with_postcondition_with(
            &request,
            |app| {
                resolve_calls.set(resolve_calls.get() + 1);
                assert_eq!(app, "Demo");
                Ok("pid:321/window:0".to_owned())
            },
            |_| {
                press_calls.set(press_calls.get() + 1);
                Ok(AxActionReport::press(
                    "test",
                    Some(format!("pid:321/window:0/path:{}", press_calls.get())),
                ))
            },
            |window_id, role| {
                observe_calls.set(observe_calls.get() + 1);
                assert_eq!(window_id, "pid:321/window:0");
                assert_eq!(role, "AXStaticText");
                Ok(if observe_calls.get() == 1 {
                    vec!["pending".to_owned()]
                } else {
                    vec!["ready".to_owned()]
                })
            },
        )
        .unwrap();

        assert_eq!(resolve_calls.get(), 1);
        assert_eq!(press_calls.get(), 2);
        assert_eq!(observe_calls.get(), 2);
        assert!(report.performed);
        assert!(report.verified);
        assert_eq!(report.status, "ok");
        assert_eq!(report.attempt_count, 2);
        assert_eq!(report.steps.len(), 2);
        assert!(report.steps.iter().all(|step| step.performed));
        assert!(!report.steps[0].verified);
        assert!(report.steps[1].verified);
    }

    #[test]
    fn guarded_ax_press_should_fail_closed_at_attempt_limit() {
        let request = parse_ax_press_payload("pid:321/window:0,重置,AXStaticText,ready,3").unwrap();
        let press_calls = Cell::new(0usize);

        let report = perform_ax_press_with_postcondition_with(
            &request,
            |_| panic!("window_id target不应解析app"),
            |_| {
                press_calls.set(press_calls.get() + 1);
                Ok(AxActionReport::press(
                    "test",
                    Some("pid:321/window:0/path:1".to_owned()),
                ))
            },
            |_, _| Ok(vec!["pending".to_owned()]),
        )
        .unwrap();

        assert_eq!(press_calls.get(), 3);
        assert!(report.performed);
        assert!(!report.verified);
        assert_eq!(report.status, "failed");
        assert_eq!(report.attempt_count, 3);
        assert_eq!(report.steps.len(), 3);
        assert!(report
            .error
            .as_deref()
            .is_some_and(|error| { error.contains("未在3次动作内满足") }));
    }

    #[test]
    fn ax_postcondition_comparison_should_remove_bidi_controls() {
        assert_eq!(normalize_ax_verification_value("\u{200e}0\u{200f}"), "0");
        assert_eq!(
            normalize_ax_verification_value("\u{2066}ready\u{2069}"),
            "ready"
        );
    }

    #[test]
    fn observe_current_ax_values_should_reach_deeply_nested_static_text() {
        // Calculator 等应用的 AXStaticText result value 常位于 depth >= 5。
        // 这里手工构造一个 depth=6 的 snapshot,验证通用 fresh 观察能取到。
        fn leaf(role: &str, value: &str) -> AxElement {
            AxElement {
                id: format!("id-{role}"),
                ref_id: None,
                role: role.to_owned(),
                subrole: None,
                name: None,
                value: Some(value.to_owned()),
                value_redacted: false,
                description: None,
                rect: None,
                enabled: Some(true),
                actions: Vec::new(),
                ax_path: Vec::new(),
                children: Vec::new(),
            }
        }

        let mut nested = leaf("AXStaticText", "0");
        for index in 1..=6 {
            nested = AxElement {
                id: format!("id-group-{index}"),
                ref_id: None,
                role: "AXGroup".to_owned(),
                subrole: None,
                name: None,
                value: None,
                value_redacted: false,
                description: None,
                rect: None,
                enabled: Some(true),
                actions: Vec::new(),
                ax_path: Vec::new(),
                children: vec![nested],
            };
        }
        let window = AxWindow {
            id: "pid:7/window:0".to_owned(),
            ref_id: None,
            pid: 7,
            process_name: "Calculator".to_owned(),
            title: Some("Calculator".to_owned()),
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: None,
            focused: Some(true),
            elements: vec![nested],
        };
        let snapshot = AxSnapshot::complete("test", vec![window], false);

        let captured_request_depth = Cell::new(0u8);
        let values = observe_current_ax_values_with("pid:7/window:0", "AXStaticText", |_, req| {
            captured_request_depth.set(req.depth);
            Ok(snapshot.clone())
        })
        .unwrap();

        assert_eq!(captured_request_depth.get(), AX_POSTCONDITION_DEPTH);
        assert!(captured_request_depth.get() >= 6);
        assert_eq!(values, vec!["0".to_owned()]);
    }

    #[test]
    fn press_sequence_should_resolve_app_once_and_preserve_partial_failure() {
        let request = parse_ax_press_sequence_payload("app:Calculator,1,加,2").unwrap();
        let resolve_calls = Cell::new(0usize);
        let request = materialize_press_sequence_request_with(&request, |app| {
            resolve_calls.set(resolve_calls.get() + 1);
            assert_eq!(app, "Calculator");
            Ok("pid:123/window:0".to_owned())
        })
        .unwrap();

        assert_eq!(resolve_calls.get(), 1);
        assert!(request.targets.iter().all(|target| {
            target.app.is_none() && target.window_id.as_deref() == Some("pid:123/window:0")
        }));

        let mut call_index = 0usize;
        let report = perform_ax_press_sequence_with(&request, |_| {
            let index = call_index;
            call_index += 1;
            if index == 1 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "目标歧义"));
            }
            Ok(AxActionReport::press(
                "test",
                Some(format!("pid:123/window:0/path:{index}")),
            ))
        });

        assert!(!report.performed);
        assert_eq!(report.status, "failed");
        assert_eq!(report.step_count, 3);
        assert_eq!(report.steps.len(), 2);
        assert!(report.steps[0].performed);
        assert!(!report.steps[1].performed);
        assert_eq!(report.steps[1].description, "加");
        assert_eq!(report.steps[1].error.as_deref(), Some("目标歧义"));
        assert_eq!(report.failed_index, Some(1));
        assert_eq!(report.error.as_deref(), Some("目标歧义"));
        assert_eq!(call_index, 2);
    }

    #[test]
    fn ax_target_window_ownership_should_fail_closed() {
        let direct_id_with_window = parse_ax_press_payload(
            r#"{target:{id:"pid:123/window:0/path:0",window_id:"pid:123/window:0"}}"#,
        );
        assert!(direct_id_with_window.is_err());

        let ref_with_window = parse_ax_press_payload(
            r#"{target:{ref:"@e1",observation_id:"obs-1",window_id:"pid:123/window:0"}}"#,
        );
        assert!(ref_with_window.is_err());

        let mixed_app = AxTarget {
            app: Some("Calculator".to_owned()),
            window_id: Some("pid:123/window:0".to_owned()),
            role: Some("AXButton".to_owned()),
            ..AxTarget::default()
        };
        assert!(mixed_app.validate().is_err());
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

    #[test]
    fn ax_find_window_identity_should_route_only_to_targeted_capture() {
        let global_calls = Cell::new(0);
        let targeted_calls = Cell::new(0);
        let request =
            parse_ax_find_payload(r#"{window:{window_id:"pid:7/window:1"},role:"AXButton"}"#)
                .unwrap();

        let snapshot = capture_ax_find_snapshot_with(
            &request,
            |_| {
                global_calls.set(global_calls.get() + 1);
                Ok(AxSnapshot::complete("global", Vec::new(), false))
            },
            |window_id, _| {
                targeted_calls.set(targeted_calls.get() + 1);
                assert_eq!(window_id, "pid:7/window:1");
                Ok(AxSnapshot::complete("targeted", Vec::new(), false))
            },
        )
        .unwrap();

        assert_eq!(snapshot.platform, "targeted");
        assert_eq!(global_calls.get(), 0);
        assert_eq!(targeted_calls.get(), 1);
    }

    #[test]
    fn semantic_target_window_id_should_route_only_to_targeted_capture() {
        let global_calls = Cell::new(0);
        let targeted_calls = Cell::new(0);
        let target = AxTarget {
            window_id: Some("pid:7/window:1".to_owned()),
            role: Some("AXButton".to_owned()),
            description: Some("1".to_owned()),
            ..AxTarget::default()
        };

        let snapshot = capture_semantic_target_snapshot_with(
            &target,
            &AxTreeRequest::default(),
            |_| {
                global_calls.set(global_calls.get() + 1);
                Ok(AxSnapshot::complete("global", Vec::new(), false))
            },
            |window_id, _| {
                targeted_calls.set(targeted_calls.get() + 1);
                assert_eq!(window_id, "pid:7/window:1");
                Ok(AxSnapshot::complete("targeted", Vec::new(), false))
            },
        )
        .unwrap();

        assert_eq!(snapshot.platform, "targeted");
        assert_eq!(global_calls.get(), 0);
        assert_eq!(targeted_calls.get(), 1);
    }

    #[test]
    fn semantic_target_without_window_id_should_keep_global_capture() {
        let global_calls = Cell::new(0);
        let targeted_calls = Cell::new(0);
        let target = AxTarget {
            role: Some("AXButton".to_owned()),
            description: Some("1".to_owned()),
            ..AxTarget::default()
        };

        let snapshot = capture_semantic_target_snapshot_with(
            &target,
            &AxTreeRequest::default(),
            |_| {
                global_calls.set(global_calls.get() + 1);
                Ok(AxSnapshot::complete("global", Vec::new(), false))
            },
            |_, _| {
                targeted_calls.set(targeted_calls.get() + 1);
                Ok(AxSnapshot::complete("targeted", Vec::new(), false))
            },
        )
        .unwrap();

        assert_eq!(snapshot.platform, "global");
        assert_eq!(global_calls.get(), 1);
        assert_eq!(targeted_calls.get(), 0);
    }

    #[test]
    fn ax_find_without_window_identity_should_keep_global_capture() {
        let global_calls = Cell::new(0);
        let targeted_calls = Cell::new(0);
        let request = parse_ax_find_payload(r#"{role:"AXButton"}"#).unwrap();

        let snapshot = capture_ax_find_snapshot_with(
            &request,
            |_| {
                global_calls.set(global_calls.get() + 1);
                Ok(AxSnapshot::complete("global", Vec::new(), false))
            },
            |_, _| {
                targeted_calls.set(targeted_calls.get() + 1);
                Ok(AxSnapshot::complete("targeted", Vec::new(), false))
            },
        )
        .unwrap();

        assert_eq!(snapshot.platform, "global");
        assert_eq!(global_calls.get(), 1);
        assert_eq!(targeted_calls.get(), 0);
    }
}
