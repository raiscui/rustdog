use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use std::collections::BTreeMap;

pub const SELECTOR_DRAFT_SCHEMA: &str = "rdog.selector.draft.v1";
pub const SELECTOR_RECORD_SCHEMA: &str = "rdog.selector.record.v1";
pub const PERMANENT_SELECTOR_SCHEMA: &str = "rdog.selector.v1";

/// P1 selector draft 的类型。
///
/// 它是 durable 恢复线索,不是 action target 的隐式替代品。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectorKind {
    AxWindow,
    AxElement,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SelectorEnvelope {
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<AppSelector>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowSelector>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<ElementSelector>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<SelectorAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSelector {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid_hint: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSelector {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<SelectorRect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementSelector {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ax_path: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectorAnchor {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectorRedaction {
    pub value_persisted: bool,
    pub screenshot_persisted: bool,
}

impl SelectorRedaction {
    pub fn metadata_only() -> Self {
        Self {
            value_persisted: false,
            screenshot_persisted: false,
        }
    }
}

/// P2 永久 selector 的匹配模式。
///
/// 第一版只区分 exact / contains,避免过早引入 fuzzy / ranking。
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectorMatchMode {
    Exact,
    Contains,
}

/// P2 稳定 selector。
///
/// `constraints` 是 stable identity 的来源。
/// `hints` 只帮助 resolver 加速或解释,不参与 primary fingerprint。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermanentSelector {
    pub schema: String,
    pub selector_id: String,
    pub fingerprint: String,
    pub kind: SelectorKind,
    pub platform: String,
    pub constraints: SelectorConstraints,
    pub hints: SelectorHints,
    pub source: SelectorSource,
    pub redaction: SelectorRedaction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SelectorConstraints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<AppSelectorConstraints>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowSelectorConstraints>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<ElementSelectorConstraints>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<SelectorAnchorConstraints>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSelectorConstraints {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSelectorConstraints {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_match: Option<SelectorMatchMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementSelectorConstraints {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_match: Option<SelectorMatchMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_match: Option<SelectorMatchMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectorAnchorConstraints {
    pub scope: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_match: Option<SelectorMatchMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SelectorHints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<SelectorRect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ax_path: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectorSource {
    pub observation_id: String,
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub draft_selector_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableSelectorRecord {
    pub schema: String,
    pub selector_id: String,
    pub observation_id: String,
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub kind: SelectorKind,
    pub backend_id_hint: String,
    pub selector: SelectorEnvelope,
    pub redaction: SelectorRedaction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permanent_selector: Option<PermanentSelector>,
}

/// observation 生成阶段的 selector 草稿。
///
/// 此时还没有 `observation_id`,所以不能直接写成 durable record。
/// 这样可以保持 `observation_id` 仍由内存 store 单一生成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableSelectorDraft {
    pub ref_id: String,
    pub kind: SelectorKind,
    pub backend_id_hint: String,
    pub selector: SelectorEnvelope,
    pub redaction: SelectorRedaction,
}

impl DurableSelectorDraft {
    pub fn new(
        ref_id: impl Into<String>,
        kind: SelectorKind,
        backend_id_hint: impl Into<String>,
        selector: SelectorEnvelope,
        redaction: SelectorRedaction,
    ) -> Self {
        Self {
            ref_id: ref_id.into(),
            kind,
            backend_id_hint: backend_id_hint.into(),
            selector,
            redaction,
        }
    }

    pub fn into_record(self, observation_id: impl Into<String>) -> DurableSelectorRecord {
        DurableSelectorRecord::new(
            observation_id,
            self.ref_id,
            self.kind,
            self.backend_id_hint,
            self.selector,
            self.redaction,
        )
    }
}

impl DurableSelectorRecord {
    pub fn new(
        observation_id: impl Into<String>,
        ref_id: impl Into<String>,
        kind: SelectorKind,
        backend_id_hint: impl Into<String>,
        selector: SelectorEnvelope,
        redaction: SelectorRedaction,
    ) -> Self {
        let observation_id = observation_id.into();
        let ref_id = ref_id.into();
        let draft_selector_id = selector_id_for(&observation_id, &ref_id);
        let permanent_selector = PermanentSelector::from_durable_parts(
            observation_id.clone(),
            ref_id.clone(),
            draft_selector_id.clone(),
            kind.clone(),
            backend_id_hint.into(),
            selector.clone(),
            redaction.clone(),
        );
        Self {
            schema: SELECTOR_RECORD_SCHEMA.to_owned(),
            selector_id: permanent_selector.selector_id.clone(),
            observation_id,
            ref_id,
            kind,
            backend_id_hint: permanent_selector
                .hints
                .backend_id
                .clone()
                .unwrap_or_default(),
            selector,
            redaction,
            permanent_selector: Some(permanent_selector),
        }
    }

    pub fn permanent_selector(&self) -> PermanentSelector {
        self.permanent_selector.clone().unwrap_or_else(|| {
            let draft_selector_id = if self.schema == SELECTOR_DRAFT_SCHEMA {
                self.selector_id.clone()
            } else {
                selector_id_for(&self.observation_id, &self.ref_id)
            };
            PermanentSelector::from_durable_parts(
                self.observation_id.clone(),
                self.ref_id.clone(),
                draft_selector_id,
                self.kind.clone(),
                self.backend_id_hint.clone(),
                self.selector.clone(),
                self.redaction.clone(),
            )
        })
    }

    pub fn stable_selector_id(&self) -> String {
        self.permanent_selector().selector_id
    }
}

impl PermanentSelector {
    pub fn from_durable_parts(
        observation_id: String,
        ref_id: String,
        draft_selector_id: String,
        kind: SelectorKind,
        backend_id_hint: String,
        selector: SelectorEnvelope,
        redaction: SelectorRedaction,
    ) -> Self {
        let constraints = SelectorConstraints::from_envelope(&selector);
        let hints = SelectorHints::from_envelope(&selector, backend_id_hint);
        let fingerprint = fingerprint_for(&kind, &selector.platform, &constraints);
        let selector_id = stable_selector_id_from_fingerprint(&fingerprint);
        Self {
            schema: PERMANENT_SELECTOR_SCHEMA.to_owned(),
            selector_id,
            fingerprint,
            kind,
            platform: selector.platform,
            constraints,
            hints,
            source: SelectorSource {
                observation_id,
                ref_id,
                draft_selector_id,
            },
            redaction,
        }
    }
}

impl SelectorConstraints {
    fn from_envelope(selector: &SelectorEnvelope) -> Self {
        Self {
            app: selector.app.as_ref().map(AppSelectorConstraints::from_app),
            window: selector
                .window
                .as_ref()
                .map(WindowSelectorConstraints::from_window),
            element: selector
                .element
                .as_ref()
                .map(ElementSelectorConstraints::from_element),
            anchors: selector
                .anchors
                .iter()
                .map(SelectorAnchorConstraints::from_anchor)
                .collect(),
        }
    }
}

impl AppSelectorConstraints {
    fn from_app(app: &AppSelector) -> Self {
        Self {
            name: app.name.clone(),
            bundle_id: app.bundle_id.clone(),
        }
    }
}

impl WindowSelectorConstraints {
    fn from_window(window: &WindowSelector) -> Self {
        Self {
            role: window.role.clone(),
            title: window.title.clone(),
            title_match: window.title.as_ref().map(|_| SelectorMatchMode::Exact),
        }
    }
}

impl ElementSelectorConstraints {
    fn from_element(element: &ElementSelector) -> Self {
        Self {
            role: element.role.clone(),
            subrole: element.subrole.clone(),
            name: element.name.clone(),
            name_match: element.name.as_ref().map(|_| SelectorMatchMode::Exact),
            description: element.description.clone(),
            description_match: element
                .description
                .as_ref()
                .map(|_| SelectorMatchMode::Exact),
            actions: element.actions.clone(),
        }
    }
}

impl SelectorAnchorConstraints {
    fn from_anchor(anchor: &SelectorAnchor) -> Self {
        Self {
            scope: "ancestor".to_owned(),
            role: anchor.role.clone(),
            name: anchor.name.clone(),
            name_match: anchor.name.as_ref().map(|_| SelectorMatchMode::Contains),
        }
    }
}

impl SelectorHints {
    fn from_envelope(selector: &SelectorEnvelope, backend_id_hint: String) -> Self {
        Self {
            pid: selector.app.as_ref().and_then(|app| app.pid_hint),
            rect: selector
                .window
                .as_ref()
                .and_then(|window| window.rect.clone()),
            ax_path: selector
                .element
                .as_ref()
                .map(|element| element.ax_path.clone())
                .unwrap_or_default(),
            backend_id: (!backend_id_hint.is_empty()).then_some(backend_id_hint),
        }
    }
}

fn fingerprint_for(
    kind: &SelectorKind,
    platform: &str,
    constraints: &SelectorConstraints,
) -> String {
    let value = serde_json::json!({
        "schema": PERMANENT_SELECTOR_SCHEMA,
        "kind": kind,
        "platform": platform,
        "constraints": constraints,
    });
    let canonical = canonical_json(&value);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn stable_selector_id_from_fingerprint(fingerprint: &str) -> String {
    let short = fingerprint
        .strip_prefix("sha256:")
        .unwrap_or(fingerprint)
        .chars()
        .take(16)
        .collect::<String>();
    format!("sel-v1-{short}")
}

fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => serde_json::to_string(value).unwrap_or_default(),
        serde_json::Value::Array(values) => {
            let values = values.iter().map(canonical_json).collect::<Vec<_>>();
            format!("[{}]", values.join(","))
        }
        serde_json::Value::Object(map) => {
            let ordered = map.iter().collect::<BTreeMap<_, _>>();
            let fields = ordered
                .into_iter()
                .map(|(key, value)| {
                    let key = serde_json::to_string(key).unwrap_or_default();
                    format!("{key}:{}", canonical_json(value))
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", fields.join(","))
        }
    }
}

pub fn selector_id_for(observation_id: &str, ref_id: &str) -> String {
    format!(
        "sel-{}-{}",
        sanitize_selector_part(observation_id),
        sanitize_selector_part(ref_id)
    )
}

fn sanitize_selector_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> SelectorEnvelope {
        SelectorEnvelope {
            platform: "macos".to_owned(),
            app: Some(AppSelector {
                name: "System Settings".to_owned(),
                bundle_id: Some("com.apple.systempreferences".to_owned()),
                pid_hint: Some(123),
            }),
            window: Some(WindowSelector {
                title: Some("储存空间".to_owned()),
                role: "AXWindow".to_owned(),
                rect: Some(SelectorRect {
                    x: 10,
                    y: 20,
                    width: 300,
                    height: 200,
                }),
            }),
            element: Some(ElementSelector {
                role: "AXButton".to_owned(),
                subrole: None,
                name: Some("储存空间".to_owned()),
                description: None,
                actions: vec!["AXPress".to_owned()],
                ax_path: vec![7, 3],
            }),
            anchors: Vec::new(),
        }
    }

    fn ax_window_envelope() -> SelectorEnvelope {
        let mut selector = envelope();
        selector.element = None;
        selector
    }

    fn window_envelope() -> SelectorEnvelope {
        SelectorEnvelope {
            platform: "macos".to_owned(),
            app: Some(AppSelector {
                name: "TextEdit".to_owned(),
                bundle_id: Some("com.apple.TextEdit".to_owned()),
                pid_hint: Some(42),
            }),
            window: Some(WindowSelector {
                title: Some("release-notes.txt".to_owned()),
                role: "AXWindow".to_owned(),
                rect: Some(SelectorRect {
                    x: 120,
                    y: 80,
                    width: 640,
                    height: 480,
                }),
            }),
            element: None,
            anchors: Vec::new(),
        }
    }

    #[test]
    fn permanent_selector_id_should_not_depend_on_observation_or_ref() {
        let first = DurableSelectorRecord::new(
            "obs-1",
            "@e1",
            SelectorKind::AxElement,
            "pid:123/window:0/path:7.3",
            envelope(),
            SelectorRedaction::metadata_only(),
        );
        let second = DurableSelectorRecord::new(
            "obs-2",
            "@e9",
            SelectorKind::AxElement,
            "pid:456/window:0/path:8.4",
            envelope(),
            SelectorRedaction::metadata_only(),
        );

        assert_eq!(first.selector_id, second.selector_id);
        assert!(first.selector_id.starts_with("sel-v1-"));
        assert_eq!(
            first.permanent_selector().source.draft_selector_id,
            "sel-obs-1--e1"
        );
    }

    #[test]
    fn permanent_selector_fingerprint_should_ignore_hints() {
        let first = DurableSelectorRecord::new(
            "obs-1",
            "@e1",
            SelectorKind::AxElement,
            "pid:123/window:0/path:7.3",
            envelope(),
            SelectorRedaction::metadata_only(),
        );
        let mut changed_hint = envelope();
        changed_hint.app.as_mut().unwrap().pid_hint = Some(999);
        changed_hint.window.as_mut().unwrap().rect = Some(SelectorRect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        });
        changed_hint.element.as_mut().unwrap().ax_path = vec![1, 2, 3];
        let second = DurableSelectorRecord::new(
            "obs-2",
            "@e2",
            SelectorKind::AxElement,
            "pid:999/window:0/path:1.2.3",
            changed_hint,
            SelectorRedaction::metadata_only(),
        );

        assert_eq!(
            first.permanent_selector().fingerprint,
            second.permanent_selector().fingerprint
        );
        assert_eq!(first.selector_id, second.selector_id);
    }

    #[test]
    fn permanent_selector_roundtrip_should_keep_schema_and_source() {
        let record = DurableSelectorRecord::new(
            "obs-1",
            "@e1",
            SelectorKind::AxElement,
            "pid:123/window:0/path:7.3",
            envelope(),
            SelectorRedaction::metadata_only(),
        );
        let selector = record.permanent_selector();
        let value = serde_json::to_value(&selector).unwrap();

        assert_eq!(value["schema"], PERMANENT_SELECTOR_SCHEMA);
        assert_eq!(value["source"]["observation_id"], "obs-1");
        assert_eq!(value["source"]["ref"], "@e1");
        assert_eq!(value["constraints"]["element"]["name_match"], "exact");
        assert_eq!(value["hints"]["pid"], 123);
    }

    #[test]
    fn permanent_selector_should_match_golden_fixture() {
        let record = DurableSelectorRecord::new(
            "obs-1",
            "@e1",
            SelectorKind::AxElement,
            "pid:123/window:0/path:7.3",
            envelope(),
            SelectorRedaction::metadata_only(),
        );
        let actual = serde_json::to_value(record.permanent_selector()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/observation_selectors/ax_element_selector_v1.json"
        ))
        .unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn ax_window_selector_should_match_golden_fixture() {
        let record = DurableSelectorRecord::new(
            "obs-1",
            "@w1",
            SelectorKind::AxWindow,
            "pid:123/window:0",
            ax_window_envelope(),
            SelectorRedaction::metadata_only(),
        );
        let actual = serde_json::to_value(record.permanent_selector()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/observation_selectors/ax_window_selector_v1.json"
        ))
        .unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn window_selector_should_match_golden_fixture() {
        let record = DurableSelectorRecord::new(
            "obs-1",
            "@w2",
            SelectorKind::Window,
            "pid:42/window:0",
            window_envelope(),
            SelectorRedaction::metadata_only(),
        );
        let actual = serde_json::to_value(record.permanent_selector()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/observation_selectors/window_selector_v1.json"
        ))
        .unwrap();

        assert_eq!(actual, expected);
    }
}
