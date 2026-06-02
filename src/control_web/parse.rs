use super::*;

pub fn parse_web_find_payload(input: &str) -> io::Result<WebFindRequest> {
    let inner = object_inner(input, "@web-find")?;
    if inner.is_empty() {
        return Err(invalid_data("@web-find 对象 payload 不能为空"));
    }

    let mut target = None::<WebFindTarget>;
    let mut query = None::<WebFindQuery>;
    let mut roles = None::<Vec<String>>;
    let mut limit = None::<u16>;
    let mut depth = None::<u8>;
    let mut max_elements = None::<u16>;
    let mut include_values = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@web-find",
                parse_web_find_target(raw_value)?,
            )?,
            "match" => assign_once(
                &mut query,
                "match",
                "@web-find",
                parse_web_find_match(raw_value)?,
            )?,
            "roles" => assign_once(
                &mut roles,
                "roles",
                "@web-find",
                parse_string_array(raw_value, "@web-find.roles")?,
            )?,
            "limit" => assign_once(&mut limit, "limit", "@web-find", parse_limit(raw_value)?)?,
            "depth" => assign_once(
                &mut depth,
                "depth",
                "@web-find",
                parse_u8(raw_value, "depth")?,
            )?,
            "max_elements" => assign_once(
                &mut max_elements,
                "max_elements",
                "@web-find",
                parse_u16(raw_value, "max_elements")?,
            )?,
            "include_values" => assign_once(
                &mut include_values,
                "include_values",
                "@web-find",
                parse_bool(raw_value, "@web-find", "include_values")?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@web-find 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let roles = roles.unwrap_or_else(|| {
        DEFAULT_WEB_FIND_ROLES
            .iter()
            .map(|role| (*role).to_owned())
            .collect()
    });
    if roles.is_empty() {
        return Err(invalid_data("@web-find.roles 不能为空数组"));
    }

    Ok(WebFindRequest {
        target: target.unwrap_or_default(),
        query: required_field(query, "@web-find", "match")?,
        roles,
        limit: limit.unwrap_or(DEFAULT_WEB_FIND_LIMIT),
        depth: depth.unwrap_or(DEFAULT_WEB_FIND_DEPTH),
        max_elements: max_elements.unwrap_or(DEFAULT_WEB_FIND_MAX_ELEMENTS),
        include_values: include_values.unwrap_or(DEFAULT_WEB_FIND_INCLUDE_VALUES),
    })
}

pub(crate) fn parse_web_find_target(input: &str) -> io::Result<WebFindTarget> {
    let inner = object_inner(input, "@web-find.target")?;
    if inner.is_empty() {
        return Err(invalid_data("@web-find.target 不能为空"));
    }

    let mut target = WebFindTarget::default();
    let mut browser_seen = false;
    let mut app_seen = false;
    let mut window_id_seen = false;
    let mut window_ref_seen = false;
    let mut observation_id_seen = false;
    let mut title_seen = false;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "browser" => {
                reject_duplicate(&mut browser_seen, "@web-find.target", "browser")?;
                target.browser = parse_browser_target(raw_value)?;
            }
            "app" | "process" | "process_name" => {
                reject_duplicate(&mut app_seen, "@web-find.target", "app")?;
                target.app = Some(parse_non_empty_string("@web-find.target.app", raw_value)?);
            }
            "window_id" => {
                reject_duplicate(&mut window_id_seen, "@web-find.target", "window_id")?;
                target.window_id = Some(parse_non_empty_string(
                    "@web-find.target.window_id",
                    raw_value,
                )?);
            }
            "window_ref" | "ref" | "ref_id" => {
                reject_duplicate(&mut window_ref_seen, "@web-find.target", "window_ref")?;
                target.window_ref = Some(parse_non_empty_string(
                    "@web-find.target.window_ref",
                    raw_value,
                )?);
            }
            "observation_id" => {
                reject_duplicate(
                    &mut observation_id_seen,
                    "@web-find.target",
                    "observation_id",
                )?;
                target.observation_id = Some(parse_non_empty_string(
                    "@web-find.target.observation_id",
                    raw_value,
                )?);
            }
            "window_title_contains" | "title_contains" => {
                reject_duplicate(&mut title_seen, "@web-find.target", "window_title_contains")?;
                target.window_title_contains = Some(parse_non_empty_string(
                    "@web-find.target.window_title_contains",
                    raw_value,
                )?);
            }
            _ => {
                return Err(invalid_data(format!(
                    "@web-find.target 不支持字段: {field_name}"
                )))
            }
        }
    }

    validate_web_find_target(&target)?;
    Ok(target)
}

fn validate_web_find_target(target: &WebFindTarget) -> io::Result<()> {
    if target.window_id.is_some() && target.window_ref.is_some() {
        return Err(invalid_data(
            "@web-find.target.window_id 不能与 window_ref 混用",
        ));
    }

    match (target.window_ref.as_ref(), target.observation_id.as_ref()) {
        (Some(_), Some(_)) => Ok(()),
        (Some(_), None) => Err(invalid_data(
            "@web-find.target.window_ref 必须和 observation_id 一起出现",
        )),
        (None, Some(_)) => Err(invalid_data(
            "@web-find.target.observation_id 必须和 window_ref 一起出现",
        )),
        (None, None) => Ok(()),
    }
}

pub(crate) fn parse_web_find_match(input: &str) -> io::Result<WebFindQuery> {
    let inner = object_inner(input, "@web-find.match")?;
    if inner.is_empty() {
        return Err(invalid_data("@web-find.match 不能为空"));
    }

    let mut text = None::<String>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "text" | "text_contains" => assign_once(
                &mut text,
                "text",
                "@web-find.match",
                parse_non_empty_string("@web-find.match.text", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@web-find.match 不支持字段: {field_name}"
                )))
            }
        }
    }

    Ok(WebFindQuery {
        text: required_field(text, "@web-find.match", "text")?,
    })
}

fn parse_browser_target(input: &str) -> io::Result<WebFindBrowserTarget> {
    let value = parse_non_empty_string("@web-find.target.browser", input)?;
    match value.to_ascii_lowercase().as_str() {
        "active" => Ok(WebFindBrowserTarget::Active),
        _ => Err(invalid_data(format!(
            "@web-find.target.browser 当前只支持 \"active\": {value}"
        ))),
    }
}
