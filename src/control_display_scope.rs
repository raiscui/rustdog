use crate::{
    control_ax::AxRect,
    control_observation::resolve_observation_ref,
    control_protocol::{
        normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
        split_object_fields,
    },
};
use serde::Serialize;
use serde_json::{json, Value};
use std::io;

pub const DISPLAY_ID_STABILITY_SESSION: &str = "session";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisplayRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl DisplayRect {
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        let right = i64::from(self.x) + i64::from(self.width);
        let bottom = i64::from(self.y) + i64::from(self.height);
        i64::from(x) >= i64::from(self.x)
            && i64::from(y) >= i64::from(self.y)
            && i64::from(x) < right
            && i64::from(y) < bottom
    }

    pub fn intersects(&self, other: &DisplayRect) -> bool {
        self.overlap_area(other) > 0
    }

    pub fn overlap_area(&self, other: &DisplayRect) -> u64 {
        let left = i64::from(self.x).max(i64::from(other.x));
        let top = i64::from(self.y).max(i64::from(other.y));
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= left || bottom <= top {
            return 0;
        }
        ((right - left) as u64) * ((bottom - top) as u64)
    }

    fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }
}

impl From<AxRect> for DisplayRect {
    fn from(rect: AxRect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DisplaySummary {
    pub display_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stable_key: Option<String>,
    pub primary: bool,
    pub name: String,
    pub os_rect: DisplayRect,
    pub image_rect: DisplayRect,
    pub scale_factor: f32,
    pub rotation: f32,
    pub display_id_stability: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayScope {
    pub display: DisplaySelector,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplaySelector {
    Id(String),
    NameContains(String),
    ContainsPoint {
        x: i32,
        y: i32,
    },
    WindowId(String),
    WindowRef {
        observation_id: String,
        ref_id: String,
    },
}

impl DisplaySelector {
    pub fn to_value(&self) -> Value {
        match self {
            Self::Id(id) => json!({"id": id}),
            Self::NameContains(name_contains) => json!({"name_contains": name_contains}),
            Self::ContainsPoint { x, y } => json!({"contains_point": {"x": x, "y": y}}),
            Self::WindowId(window_id) => json!({"window_id": window_id}),
            Self::WindowRef {
                observation_id,
                ref_id,
            } => json!({"window_ref": ref_id, "observation_id": observation_id}),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DisplayScopeResolution {
    pub selector: Value,
    pub resolved: DisplaySummary,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_overlap_ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWindowRef {
    pub window_id: String,
}

pub fn parse_display_scope(input: &str, kind: &str) -> io::Result<DisplayScope> {
    let inner = object_inner(input, kind)?;
    if inner.is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空对象")));
    }

    let mut display = None::<DisplaySelector>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "display" => assign_once(
                &mut display,
                "display",
                kind,
                parse_display_selector(raw_value, &format!("{kind}.display"))?,
            )?,
            "display_id" => {
                return Err(invalid_data(format!(
                    "{kind}.display_id 不是请求字段;请使用 scope:{{display:{{id:\"...\"}}}}"
                )))
            }
            _ => return Err(invalid_data(format!("{kind} 不支持字段: {field_name}"))),
        }
    }

    Ok(DisplayScope {
        display: required_field(display, kind, "display")?,
    })
}

pub fn parse_display_selector(input: &str, kind: &str) -> io::Result<DisplaySelector> {
    let inner = object_inner(input, kind)?;
    if inner.is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空对象")));
    }

    let mut id = None::<String>;
    let mut name_contains = None::<String>;
    let mut contains_point = None::<(i32, i32)>;
    let mut window_id = None::<String>;
    let mut window_ref = None::<String>;
    let mut observation_id = None::<String>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "id" => assign_once(
                &mut id,
                "id",
                kind,
                parse_non_empty_string(&format!("{kind}.id"), raw_value)?,
            )?,
            "name_contains" => assign_once(
                &mut name_contains,
                "name_contains",
                kind,
                parse_non_empty_string(&format!("{kind}.name_contains"), raw_value)?,
            )?,
            "contains_point" => assign_once(
                &mut contains_point,
                "contains_point",
                kind,
                parse_contains_point(raw_value, &format!("{kind}.contains_point"))?,
            )?,
            "window_id" => assign_once(
                &mut window_id,
                "window_id",
                kind,
                parse_non_empty_string(&format!("{kind}.window_id"), raw_value)?,
            )?,
            "window_ref" => assign_once(
                &mut window_ref,
                "window_ref",
                kind,
                parse_non_empty_string(&format!("{kind}.window_ref"), raw_value)?,
            )?,
            "observation_id" => assign_once(
                &mut observation_id,
                "observation_id",
                kind,
                parse_non_empty_string(&format!("{kind}.observation_id"), raw_value)?,
            )?,
            "ref" | "ref_id" => {
                return Err(invalid_data(format!(
                    "{kind} 不使用 ref 作为 display selector;窗口引用请使用 window_ref + observation_id"
                )))
            }
            _ => return Err(invalid_data(format!("{kind} 不支持字段: {field_name}"))),
        }
    }

    let selector_count = usize::from(id.is_some())
        + usize::from(name_contains.is_some())
        + usize::from(contains_point.is_some())
        + usize::from(window_id.is_some())
        + usize::from(window_ref.is_some());
    if selector_count != 1 {
        return Err(invalid_data(format!(
            "{kind} 必须且只能包含 id/name_contains/contains_point/window_id/window_ref 中的一种 selector"
        )));
    }

    if let Some(ref_id) = window_ref {
        return Ok(DisplaySelector::WindowRef {
            observation_id: required_field(observation_id, kind, "observation_id")?,
            ref_id,
        });
    }
    if observation_id.is_some() {
        return Err(invalid_data(format!(
            "{kind}.observation_id 必须和 window_ref 一起出现"
        )));
    }
    if let Some(id) = id {
        return Ok(DisplaySelector::Id(id));
    }
    if let Some(name_contains) = name_contains {
        return Ok(DisplaySelector::NameContains(name_contains));
    }
    if let Some((x, y)) = contains_point {
        return Ok(DisplaySelector::ContainsPoint { x, y });
    }
    Ok(DisplaySelector::WindowId(
        window_id.expect("selector_count guarantees window_id exists"),
    ))
}

pub fn resolve_observation_window_ref(
    observation_id: &str,
    ref_id: &str,
) -> io::Result<ResolvedWindowRef> {
    let entry = resolve_observation_ref(observation_id, ref_id)?;
    if entry.kind != "window" {
        return Err(window_ref_invalid_error(ref_id, &entry.kind));
    }
    Ok(ResolvedWindowRef {
        window_id: entry.backend_id,
    })
}

pub fn resolve_display_scope<F>(
    scope: &DisplayScope,
    displays: &[DisplaySummary],
    mut window_rect_for_selector: F,
) -> io::Result<DisplayScopeResolution>
where
    F: FnMut(&DisplaySelector) -> io::Result<Option<DisplayRect>>,
{
    let (display, overlap_ratio) = match &scope.display {
        DisplaySelector::Id(id) => (resolve_display_by_id(displays, id)?.clone(), None::<f64>),
        DisplaySelector::NameContains(name_contains) => (
            resolve_display_by_name_contains(displays, name_contains)?.clone(),
            None,
        ),
        DisplaySelector::ContainsPoint { x, y } => {
            (resolve_display_by_point(displays, *x, *y)?.clone(), None)
        }
        DisplaySelector::WindowId(_) | DisplaySelector::WindowRef { .. } => {
            let Some(rect) = window_rect_for_selector(&scope.display)? else {
                return Err(display_not_found_error("window selector 没有可用 rect"));
            };
            let (display, overlap_ratio) = resolve_display_by_rect(displays, &rect)?;
            (display.clone(), Some(overlap_ratio))
        }
    };

    Ok(DisplayScopeResolution {
        selector: scope.display.to_value(),
        resolved: display,
        status: "applied",
        display_overlap_ratio: overlap_ratio,
    })
}

pub fn display_contains_point(display: &DisplaySummary, x: i32, y: i32) -> bool {
    display.os_rect.contains_point(x, y)
}

pub fn display_intersects_rect(display: &DisplaySummary, rect: DisplayRect) -> bool {
    display.os_rect.intersects(&rect)
}

pub fn resolve_display_by_id<'a>(
    displays: &'a [DisplaySummary],
    id: &str,
) -> io::Result<&'a DisplaySummary> {
    displays
        .iter()
        .find(|display| display.display_id == id)
        .ok_or_else(|| display_not_found_error(format!("display id 未找到: {id}")))
}

fn resolve_display_by_name_contains<'a>(
    displays: &'a [DisplaySummary],
    name_contains: &str,
) -> io::Result<&'a DisplaySummary> {
    let matches = displays
        .iter()
        .filter(|display| {
            display
                .name
                .to_ascii_lowercase()
                .contains(&name_contains.to_ascii_lowercase())
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [display] => Ok(*display),
        [] => Err(display_not_found_error(format!(
            "没有 display name 包含: {name_contains}"
        ))),
        _ => Err(ambiguous_display_selector_error(
            "name_contains",
            matches.iter().map(|display| display.display_id.as_str()),
        )),
    }
}

fn resolve_display_by_point(
    displays: &[DisplaySummary],
    x: i32,
    y: i32,
) -> io::Result<&DisplaySummary> {
    displays
        .iter()
        .find(|display| display.os_rect.contains_point(x, y))
        .ok_or_else(|| display_not_found_error(format!("point 不在任何 display 内: ({x},{y})")))
}

fn resolve_display_by_rect<'a>(
    displays: &'a [DisplaySummary],
    rect: &DisplayRect,
) -> io::Result<(&'a DisplaySummary, f64)> {
    let Some((display, area)) = displays
        .iter()
        .map(|display| (display, display.os_rect.overlap_area(rect)))
        .max_by_key(|(_, area)| *area)
    else {
        return Err(display_not_found_error("没有可用 display"));
    };
    if area == 0 {
        return Err(display_not_found_error("window rect 不与任何 display 相交"));
    }
    let rect_area = u64::from(rect.width) * u64::from(rect.height);
    let ratio = if rect_area == 0 {
        0.0
    } else {
        area as f64 / rect_area as f64
    };
    Ok((display, ratio))
}

fn parse_contains_point(input: &str, kind: &str) -> io::Result<(i32, i32)> {
    let inner = object_inner(input, kind)?;
    if inner.is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空对象")));
    }
    let mut x = None::<i32>;
    let mut y = None::<i32>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();
        match field_name.as_str() {
            "x" => assign_once(&mut x, "x", kind, parse_i32("x", raw_value)?)?,
            "y" => assign_once(&mut y, "y", kind, parse_i32("y", raw_value)?)?,
            _ => return Err(invalid_data(format!("{kind} 不支持字段: {field_name}"))),
        }
    }
    Ok((required_field(x, kind, "x")?, required_field(y, kind, "y")?))
}

pub fn display_scope_report(resolution: &DisplayScopeResolution) -> Value {
    json!({
        "selector": resolution.selector,
        "resolved": resolution.resolved,
        "status": resolution.status,
        "display_overlap_ratio": resolution.display_overlap_ratio,
    })
}

pub fn window_ref_invalid_error(ref_id: &str, actual_kind: &str) -> io::Error {
    let payload = json!({
        "kind": "display-scope",
        "error_code": "WINDOW_REF_INVALID",
        "performed": false,
        "ref": ref_id,
        "actual_kind": actual_kind,
        "message": "window_ref 必须指向 window ref",
    });
    io::Error::new(io::ErrorKind::InvalidInput, payload.to_string())
}

pub fn display_not_found_error(message: impl Into<String>) -> io::Error {
    let payload = json!({
        "kind": "display-scope",
        "error_code": "DISPLAY_NOT_FOUND",
        "performed": false,
        "message": message.into(),
    });
    io::Error::new(io::ErrorKind::InvalidInput, payload.to_string())
}

fn ambiguous_display_selector_error<'a>(
    selector_kind: &'static str,
    display_ids: impl IntoIterator<Item = &'a str>,
) -> io::Error {
    let payload = json!({
        "kind": "display-scope",
        "error_code": "AMBIGUOUS_DISPLAY_SELECTOR",
        "performed": false,
        "selector": selector_kind,
        "display_ids": display_ids.into_iter().collect::<Vec<_>>(),
    });
    io::Error::new(io::ErrorKind::InvalidInput, payload.to_string())
}

fn parse_non_empty_string(kind: &str, input: &str) -> io::Result<String> {
    let value = parse_quoted_payload(input)?;
    if value.trim().is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空")));
    }
    Ok(value)
}

fn parse_i32(field_name: &str, input: &str) -> io::Result<i32> {
    input
        .parse::<i32>()
        .map_err(|_| invalid_data(format!("`{field_name}` 必须是 32 位整数: {input}")))
}

fn assign_once<T>(slot: &mut Option<T>, field_name: &str, kind: &str, value: T) -> io::Result<()> {
    if slot.is_some() {
        return Err(invalid_data(format!("{kind} 的 `{field_name}` 字段重复")));
    }
    *slot = Some(value);
    Ok(())
}

fn required_field<T>(value: Option<T>, kind: &str, field_name: &str) -> io::Result<T> {
    value.ok_or_else(|| invalid_data(format!("{kind} 缺少 `{field_name}` 字段")))
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: &str, name: &str, x: i32, y: i32, width: u32, height: u32) -> DisplaySummary {
        DisplaySummary {
            display_id: id.to_owned(),
            stable_key: Some(format!("test:{id}")),
            primary: id == "d1",
            name: name.to_owned(),
            os_rect: DisplayRect {
                x,
                y,
                width,
                height,
            },
            image_rect: DisplayRect {
                x,
                y,
                width,
                height,
            },
            scale_factor: 1.0,
            rotation: 0.0,
            display_id_stability: DISPLAY_ID_STABILITY_SESSION,
        }
    }

    #[test]
    fn parse_display_scope_should_accept_all_selector_shapes() {
        assert_eq!(
            parse_display_scope(r#"{display:{id:"d2"}}"#, "@observe.scope")
                .unwrap()
                .display,
            DisplaySelector::Id("d2".to_owned())
        );
        assert!(matches!(
            parse_display_scope(
                r#"{display:{contains_point:{x:1800,y:500}}}"#,
                "@observe.scope"
            )
            .unwrap()
            .display,
            DisplaySelector::ContainsPoint { x: 1800, y: 500 }
        ));
        assert_eq!(
            parse_display_scope(
                r#"{display:{window_ref:"@e4",observation_id:"obs-1"}}"#,
                "@observe.scope"
            )
            .unwrap()
            .display,
            DisplaySelector::WindowRef {
                observation_id: "obs-1".to_owned(),
                ref_id: "@e4".to_owned(),
            }
        );
    }

    #[test]
    fn parse_display_scope_should_reject_top_level_display_id_and_ref_selector() {
        assert!(parse_display_scope(r#"{display_id:"d2"}"#, "@observe.scope").is_err());
        assert!(parse_display_scope(r#"{display:{ref:"@d2"}}"#, "@observe.scope").is_err());
    }

    #[test]
    fn parse_display_scope_should_reject_window_ref_without_observation_id() {
        assert!(parse_display_scope(r#"{display:{window_ref:"@e4"}}"#, "@observe.scope").is_err());
    }

    #[test]
    fn resolve_display_scope_should_reject_ambiguous_name_and_gap_point() {
        let displays = vec![
            display("d1", "DELL left", 0, 0, 100, 100),
            display("d2", "DELL right", 120, 0, 100, 100),
        ];
        let ambiguous = DisplayScope {
            display: DisplaySelector::NameContains("DELL".to_owned()),
        };
        let err = resolve_display_scope(&ambiguous, &displays, |_| Ok(None)).unwrap_err();
        assert!(err.to_string().contains("AMBIGUOUS_DISPLAY_SELECTOR"));

        let gap = DisplayScope {
            display: DisplaySelector::ContainsPoint { x: 110, y: 50 },
        };
        let err = resolve_display_scope(&gap, &displays, |_| Ok(None)).unwrap_err();
        assert!(err.to_string().contains("DISPLAY_NOT_FOUND"));
    }

    #[test]
    fn resolve_display_scope_should_pick_largest_window_overlap() {
        let displays = vec![
            display("d1", "left", 0, 0, 100, 100),
            display("d2", "right", 100, 0, 100, 100),
        ];
        let scope = DisplayScope {
            display: DisplaySelector::WindowId("win-1".to_owned()),
        };
        let resolution = resolve_display_scope(&scope, &displays, |_| {
            Ok(Some(DisplayRect {
                x: 80,
                y: 0,
                width: 80,
                height: 100,
            }))
        })
        .unwrap();
        assert_eq!(resolution.resolved.display_id, "d2");
    }
}
