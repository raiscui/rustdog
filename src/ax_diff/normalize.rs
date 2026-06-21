// ax_diff/normalize.rs
//
// 把 rdog daemon 返回的 AxSnapshot JSON 归一化为稳定形态:
//   - 移除顶层 `observation` 块 (drift)
//   - 移除每个 element 的 `ref` 和 `ax_path` (drift)
//   - 递归对 children 同样处理
//   - 元素按 id 稳定排序,让后续 diff 不被数组顺序影响
//
// 不直接绑定 rdog 内部的 `AxSnapshot` 类型,因为 ax-diff 是无 daemon
// 依赖的纯工具,只把 Value 当作自由 JSON 树处理。

use serde_json::Value;

pub fn normalize_snapshot(value: &Value) -> Value {
    let mut v = value.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("observation");
    }
    if let Some(windows) = v.get_mut("windows").and_then(|w| w.as_array_mut()) {
        for w in windows.iter_mut() {
            if let Some(wobj) = w.as_object_mut() {
                wobj.remove("ref");
            }
            if let Some(elements) = w.get_mut("elements").and_then(|e| e.as_array_mut()) {
                for e in elements.iter_mut() {
                    normalize_element(e);
                }
                // 排序: 按 element id 稳定排序,让 diff 不被数组顺序影响
                elements.sort_by(|a, b| element_sort_key(a).cmp(&element_sort_key(b)));
            }
        }
        windows.sort_by(|a, b| {
            let ai = a.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let bi = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
            ai.cmp(bi)
        });
    }
    v
}

fn normalize_element(value: &mut Value) {
    if let Some(obj) = value.as_object_mut() {
        obj.remove("ref");
        obj.remove("ax_path");
    }
    if let Some(children) = value.get_mut("children").and_then(|c| c.as_array_mut()) {
        for c in children.iter_mut() {
            normalize_element(c);
        }
        children.sort_by(|a, b| element_sort_key(a).cmp(&element_sort_key(b)));
    }
}

fn element_sort_key(value: &Value) -> String {
    value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}
