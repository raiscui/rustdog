// =====================================================================
// rdog ax-diff 子命令入口
//
// 目的: 对两份 AxSnapshot JSON 做"结构化 diff",而不是字符串 diff。
//
// 为什么不直接用 jq / diff 字符串对比:
//   1. AxSnapshot 里 `observation` 块、`ref` 字段、`ax_path` 索引都是
//      每次观察会重新分配/漂移的字段,纯字符串 diff 会产生大量噪音。
//   2. 字符串 diff 不能告诉 model "哪个按钮从 description:"首页" 变成了
//      AXLink.description:"点点 ai"",这种语义级变化才是 GUI agent 真正
//      关心的"页面前后状态差"。
//
// 设计:
//   - 规范化 (normalize.rs): 移除 observation / ref / ax_path, 按 id 排序。
//   - 配对 (diff.rs): 两 pass 独立 —— window shallow + element deep。
//     Pass 1 只看 window 自身 shallow 字段; Pass 2 不管 window 是否变,
//     都递归配对 element 树。这样小红书 "feed 改 / nav 改" 这种典型
//     场景不会被 window 自身未变而漏报。
//   - 输出 (output.rs): text (人/agent 友好) / json (程序消费) / summary。
//   - 退出码: 0 = 相同; 1 = 有差异; 2 = 用法错误; 3 = JSON 解析失败。
// =====================================================================

use crate::ax_diff::diff::compute_diff;
use crate::ax_diff::types::report_has_changes;
use serde_json::Value;
use std::io;
use std::path::PathBuf;

pub(crate) mod diff;
mod normalize;
mod output;
mod types;

// 让外部 (main.rs / tests) 还能继续用 ax_diff::DiffReport 等
#[allow(unused_imports)]
use types::{DiffReport, ElementDiff, ElementDiffKind, FieldChange, WindowDiff, WindowDiffKind};

/// 子命令参数,和 src/input.rs 的 Command::AxDiff 字段一一对应。
#[derive(Debug)]
pub struct AxDiffOptions {
    /// "before" 快照 JSON 文件路径。必填。
    pub before: PathBuf,
    /// "after" 快照 JSON 文件路径。必填。
    pub after: PathBuf,
    /// 输出格式: text | json | summary。默认 text。
    pub format: OutputFormat,
    /// 只统计差异, 不打印完整字段 before/after。
    pub quiet: bool,
    /// 限制递归打印 element-field 改动的最大深度,避免巨型树。
    pub max_depth: usize,
    /// text 格式下最多打印 N 个 element 改动,超出截断并提示。
    /// 适合 model context 紧张场景。None 表示不限制。
    /// 只影响 text 输出,不影响 summary 计数和 json 完整输出。
    pub top_changes: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
    Summary,
}

impl OutputFormat {
    fn from_str(s: &str) -> io::Result<Self> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "summary" => Ok(Self::Summary),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("未知 --format: {other}, 期望 text | json | summary"),
            )),
        }
    }
}

/// 子命令入口,被 main.rs 的 Command::AxDiff 调起。
pub fn run(opts: AxDiffOptions) -> i32 {
    let before_text = match std::fs::read_to_string(&opts.before) {
        Ok(t) => t,
        Err(err) => {
            eprintln!(
                "rdog ax-diff: 无法读取 before 文件 {}: {err}",
                opts.before.display()
            );
            return 2;
        }
    };
    let after_text = match std::fs::read_to_string(&opts.after) {
        Ok(t) => t,
        Err(err) => {
            eprintln!(
                "rdog ax-diff: 无法读取 after 文件 {}: {err}",
                opts.after.display()
            );
            return 2;
        }
    };
    let before_json: Value = match serde_json::from_str(&before_text) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("rdog ax-diff: before JSON 解析失败: {err}");
            return 3;
        }
    };
    let after_json: Value = match serde_json::from_str(&after_text) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("rdog ax-diff: after JSON 解析失败: {err}");
            return 3;
        }
    };
    let before = normalize::normalize_snapshot(&before_json);
    let after = normalize::normalize_snapshot(&after_json);
    let report = compute_diff(&before, &after, opts.max_depth);
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let write_result = match opts.format {
        OutputFormat::Text => {
            output::write_text_report(&mut out, &report, opts.quiet, opts.top_changes)
        }
        OutputFormat::Json => output::write_json_report(&mut out, &report),
        OutputFormat::Summary => output::write_summary_report(&mut out, &report),
    };
    if let Err(err) = write_result {
        eprintln!("rdog ax-diff: 写报告失败: {err}");
        return 2;
    }
    if report_has_changes(&report) {
        1
    } else {
        0
    }
}

/// 解析命令行参数,独立于 clap,这样可以单独跑 `rdog ax-diff --help` 测试。
pub fn parse_options(argv: &[String]) -> io::Result<AxDiffOptions> {
    let mut before: Option<PathBuf> = None;
    let mut after: Option<PathBuf> = None;
    let mut format: Option<OutputFormat> = None;
    let mut quiet = false;
    let mut max_depth: usize = 4;
    let mut top_changes: Option<usize> = None;
    let mut i = 0;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "--before" => {
                i += 1;
                before = Some(PathBuf::from(next_value(argv, i, "--before")?));
            }
            "--after" => {
                i += 1;
                after = Some(PathBuf::from(next_value(argv, i, "--after")?));
            }
            "--format" => {
                i += 1;
                format = Some(OutputFormat::from_str(&next_value(argv, i, "--format")?)?);
            }
            "--quiet" | "-q" => quiet = true,
            "--top-changes" => {
                i += 1;
                let raw = next_value(argv, i, "--top-changes")?;
                let n = raw.parse::<usize>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--top-changes 必须是正整数")
                })?;
                top_changes = Some(n);
            }
            "--max-depth" => {
                i += 1;
                let raw = next_value(argv, i, "--max-depth")?;
                max_depth = raw.parse::<usize>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--max-depth 必须是正整数")
                })?;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other if other.starts_with("--") => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("未知参数: {other}"),
                ));
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("位置参数不被支持: {other} (请用 --before / --after)"),
                ));
            }
        }
        i += 1;
    }
    let before = before
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "缺少 --before <FILE>"))?;
    let after =
        after.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "缺少 --after <FILE>"))?;
    Ok(AxDiffOptions {
        before,
        after,
        format: format.unwrap_or(OutputFormat::Text),
        quiet,
        top_changes,
        max_depth,
    })
}

fn next_value(argv: &[String], idx: usize, flag: &str) -> io::Result<String> {
    argv.get(idx)
        .cloned()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{flag} 缺少值")))
}

fn print_help() {
    println!(
        "rdog ax-diff - 对两份 AxSnapshot JSON 做结构化 diff\n\
         \n\
         用法:\n  \
           rdog ax-diff --before <FILE> --after <FILE> [--format text|json|summary] [--quiet] [--max-depth N]\n\
         \n\
         行为:\n  \
           - 移除 observation 块、ref 字段、ax_path 索引等漂移字段\n  \
           - 按 window id 配对 windows,按 element id 配对 elements\n  \
           - 输出 added/removed/modified 列表 + 字段级 before/after\n\
         \n\
         退出码:\n  \
           0 = 相同, 1 = 有差异, 2 = 用法错误, 3 = JSON 解析失败\n\
         \n\
         典型用法:\n  \
           # 1) 抓 before 快照\n  \
           rdog control mac.lab <<< '@observe#1:...'\n  \
           # 2) 触发动作, 抓 after 快照\n  \
           rdog control mac.lab <<< '@observe#2:...'\n  \
           # 3) 抽取两份 savefile JSON 之后做结构化 diff\n  \
           rdog ax-diff --before before.json --after after.json --format text"
    );
}

// ---------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{parse_options, run, AxDiffOptions, OutputFormat};
    use crate::ax_diff::diff::compute_diff;
    use crate::ax_diff::normalize::normalize_snapshot;
    use crate::ax_diff::types::report_has_changes;
    use serde_json::json;

    fn xhs_before() -> serde_json::Value {
        json!({
            "kind": "ax-tree",
            "schema": "rdog.ax.v1",
            "platform": "macos",
            "window_count": 1,
            "element_count": 2,
            "truncated": false,
            "observation": {"source_command": "@observe#1"},
            "windows": [{
                "id": "pid:8231/window:0",
                "ref": "@e1",
                "pid": 8231,
                "process_name": "Google Chrome",
                "title": "小红书 - Google Chrome",
                "role": "AXWindow",
                "elements": [{
                    "id": "pid:8231/window:0/path:0",
                    "ref": "@e2",
                    "role": "AXWebArea",
                    "ax_path": [0],
                    "actions": [],
                    "children": [
                        {
                            "id": "pid:8231/window:0/path:0.0",
                            "ref": "@e3",
                            "role": "AXLink",
                            "description": "首页",
                            "ax_path": [0, 0],
                            "actions": ["AXPress"],
                            "children": []
                        },
                        {
                            "id": "pid:8231/window:0/path:0.1",
                            "ref": "@e4",
                            "role": "AXLink",
                            "description": "点点",
                            "ax_path": [0, 1],
                            "actions": ["AXPress"],
                            "children": []
                        }
                    ]
                }]
            }]
        })
    }

    fn xhs_after_click_home() -> serde_json::Value {
        json!({
            "kind": "ax-tree",
            "schema": "rdog.ax.v1",
            "platform": "macos",
            "window_count": 1,
            "element_count": 2,
            "truncated": false,
            "observation": {"source_command": "@observe#2"},
            "windows": [{
                "id": "pid:8231/window:0",
                "ref": "@e1",
                "pid": 8231,
                "process_name": "Google Chrome",
                "title": "小红书 - Google Chrome",
                "role": "AXWindow",
                "elements": [{
                    "id": "pid:8231/window:0/path:0",
                    "ref": "@e2",
                    "role": "AXWebArea",
                    "ax_path": [0],
                    "actions": [],
                    "children": [
                        {
                            "id": "pid:8231/window:0/path:0.0",
                            "ref": "@e3",
                            "role": "AXLink",
                            "description": "首页",
                            "ax_path": [0, 0],
                            "actions": ["AXPress", "AXShowMenu"],
                            "children": []
                        },
                        {
                            "id": "pid:8231/window:0/path:0.1",
                            "ref": "@e4",
                            "role": "AXLink",
                            "description": "点点 ai",
                            "ax_path": [0, 1],
                            "actions": ["AXPress"],
                            "children": []
                        }
                    ]
                }]
            }]
        })
    }

    #[test]
    fn normalize_should_drop_observation_ref_and_ax_path() {
        let normalized = normalize_snapshot(&xhs_before());
        assert!(normalized.get("observation").is_none());
        let win = &normalized["windows"][0];
        assert!(win.get("ref").is_none());
        let elem = &win["elements"][0];
        assert!(elem.get("ref").is_none());
        assert!(elem.get("ax_path").is_none());
        let child = &elem["children"][0];
        assert!(child.get("ref").is_none());
        assert!(child.get("ax_path").is_none());
    }

    #[test]
    fn diff_should_report_only_changed_element_field() {
        let before = normalize_snapshot(&xhs_before());
        let after = normalize_snapshot(&xhs_after_click_home());
        let report = compute_diff(&before, &after, 4);
        assert_eq!(report.windows_added, 0);
        assert_eq!(report.windows_removed, 0);
        assert_eq!(report.windows_modified, 0);
        assert_eq!(report.elements_added, 0);
        assert_eq!(report.elements_removed, 0);
        // 修改应该命中两个 element:
        //  - path:0.0 actions 多了一个 AXShowMenu
        //  - path:0.1 description 从 "点点" 变成 "点点 ai"
        assert_eq!(report.elements_modified, 2);
        let changed: Vec<String> = report
            .elements
            .iter()
            .flat_map(|(_, e)| e.changed_fields.iter().map(|f| f.field.clone()))
            .collect();
        assert!(
            changed.iter().any(|f| f.contains("description")),
            "expected description change, got: {changed:?}"
        );
        assert!(
            changed.iter().any(|f| f.contains("actions")),
            "expected actions change, got: {changed:?}"
        );
    }

    #[test]
    fn diff_should_treat_different_observation_as_no_op() {
        // observation 块是 drift,必须被规范化掉,否则无意义 diff。
        let mut a = xhs_before();
        let mut b = xhs_before();
        a["observation"] = json!({"source_command": "@observe#1"});
        b["observation"] = json!({"source_command": "@observe#42", "scope": "ax"});
        let na = normalize_snapshot(&a);
        let nb = normalize_snapshot(&b);
        let report = compute_diff(&na, &nb, 4);
        assert_eq!(report.windows_modified, 0);
        assert_eq!(report.elements_modified, 0);
    }

    #[test]
    fn diff_should_report_added_window() {
        let mut before = xhs_before();
        before.as_object_mut().unwrap().remove("windows");
        let before = normalize_snapshot(&before);
        let after = normalize_snapshot(&xhs_after_click_home());
        let report = compute_diff(&before, &after, 4);
        assert_eq!(report.windows_added, 1);
        assert_eq!(report.elements_added, 3); // path:0 (AXWebArea) + path:0.0 (home) + path:0.1 (dot)
    }

    #[test]
    fn exit_code_uses_diff_result() {
        let a = normalize_snapshot(&xhs_before());
        let b = normalize_snapshot(&xhs_before());
        let r = compute_diff(&a, &b, 4);
        assert!(!report_has_changes(&r));
        let c = normalize_snapshot(&xhs_after_click_home());
        let r2 = compute_diff(&a, &c, 4);
        assert!(report_has_changes(&r2));
    }

    #[test]
    fn parse_options_requires_before_and_after() {
        let argv: Vec<String> = vec!["--before".into(), "a.json".into()];
        let err = parse_options(&argv).unwrap_err();
        assert!(err.to_string().contains("--after"));
    }

    #[test]
    fn parse_options_accepts_format_and_quiet() {
        let argv: Vec<String> = vec![
            "--before".into(),
            "a.json".into(),
            "--after".into(),
            "b.json".into(),
            "--format".into(),
            "summary".into(),
            "--quiet".into(),
            "--max-depth".into(),
            "3".into(),
        ];
        let opts = parse_options(&argv).unwrap();
        assert_eq!(opts.format, OutputFormat::Summary);
        assert!(opts.quiet);
        assert_eq!(opts.max_depth, 3);
    }

    #[test]
    fn run_text_format_returns_1_when_changed() {
        // 真实跑一次 run, 验证 stdout 输出和退出码。
        let dir = std::env::temp_dir().join("rdog_ax_diff_test");
        std::fs::create_dir_all(&dir).unwrap();
        let before = dir.join("before.json");
        let after = dir.join("after.json");
        std::fs::write(&before, xhs_before().to_string()).unwrap();
        std::fs::write(&after, xhs_after_click_home().to_string()).unwrap();
        let opts = AxDiffOptions {
            before,
            after,
            format: OutputFormat::Summary,
            quiet: false,
            max_depth: 4,
            top_changes: None,
        };
        let code = run(opts);
        assert_eq!(code, 1);
    }

    #[test]
    fn run_text_format_returns_0_when_unchanged() {
        let dir = std::env::temp_dir().join("rdog_ax_diff_test_unchanged");
        std::fs::create_dir_all(&dir).unwrap();
        let before = dir.join("before.json");
        let after = dir.join("after.json");
        let same = xhs_before();
        std::fs::write(&before, same.to_string()).unwrap();
        std::fs::write(&after, same.to_string()).unwrap();
        let opts = AxDiffOptions {
            before,
            after,
            format: OutputFormat::Summary,
            quiet: false,
            max_depth: 4,
            top_changes: None,
        };
        let code = run(opts);
        assert_eq!(code, 0);
    }

    #[test]
    fn parse_options_accepts_top_changes() {
        let argv: Vec<String> = vec![
            "--before".into(),
            "a.json".into(),
            "--after".into(),
            "b.json".into(),
            "--top-changes".into(),
            "5".into(),
        ];
        let opts = parse_options(&argv).unwrap();
        assert_eq!(opts.top_changes, Some(5));
    }

    #[test]
    fn parse_options_top_changes_rejects_non_integer() {
        let argv: Vec<String> = vec![
            "--before".into(),
            "a.json".into(),
            "--after".into(),
            "b.json".into(),
            "--top-changes".into(),
            "abc".into(),
        ];
        let err = parse_options(&argv).unwrap_err();
        assert!(
            err.to_string().contains("--top-changes"),
            "expected --top-changes in error, got: {err}"
        );
    }

    #[test]
    fn run_text_format_with_top_changes_truncates() {
        // xhs_after_click_home 有 2 个 element 改动, 用 top_changes=1 应该只
        // 打印前 1 个, 末尾带截断提示。
        let dir = std::env::temp_dir().join("rdog_ax_diff_test_top_changes");
        std::fs::create_dir_all(&dir).unwrap();
        let before = dir.join("before.json");
        let after = dir.join("after.json");
        std::fs::write(&before, xhs_before().to_string()).unwrap();
        std::fs::write(&after, xhs_after_click_home().to_string()).unwrap();
        let opts = AxDiffOptions {
            before,
            after,
            format: OutputFormat::Text,
            quiet: false,
            max_depth: 4,
            top_changes: Some(1),
        };
        let code = run(opts);
        // 差异存在 -> 退出码 1
        assert_eq!(code, 1);
    }

    #[test]
    fn run_text_format_top_changes_larger_than_total_does_not_truncate() {
        // top_changes 远大于实际 element 数, 不应出现截断提示
        let dir = std::env::temp_dir().join("rdog_ax_diff_test_top_changes_large");
        std::fs::create_dir_all(&dir).unwrap();
        let before = dir.join("before.json");
        let after = dir.join("after.json");
        std::fs::write(&before, xhs_before().to_string()).unwrap();
        std::fs::write(&after, xhs_after_click_home().to_string()).unwrap();
        let opts = AxDiffOptions {
            before,
            after,
            format: OutputFormat::Text,
            quiet: false,
            max_depth: 4,
            top_changes: Some(100),
        };
        let code = run(opts);
        assert_eq!(code, 1);
    }
}
