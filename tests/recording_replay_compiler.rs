//! Integration tests for the Recording Replay Compiler prototype.
//!
//! Run with:
//!     cargo test --test recording_replay_compiler -- --nocapture

use std::{fs, path::PathBuf};

#[path = "../src/bin/replay_compiler.rs"]
mod replay_compiler_lib;

use replay_compiler_lib::{Compiler, FlowStep};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/replay-compiler")
        .join(name)
}

#[test]
fn fixture_journal_parses() {
    let input = fs::read_to_string(fixture_path("journal_optimizations.jsonl"))
        .expect("read fixture");
    let compiler = Compiler::default();
    let script = compiler.compile(&input).expect("compile");
    assert!(!script.steps.is_empty(), "expected non-empty flow script");
}

#[test]
fn determinism_byte_equal() {
    let input = fs::read_to_string(fixture_path("journal_optimizations.jsonl"))
        .expect("read fixture");
    let compiler = Compiler::default();
    let a = compiler.compile(&input).expect("first compile");
    let a_bytes = compiler.serialize_canonical(&a).expect("first serialize");
    let b = compiler.compile(&input).expect("second compile");
    let b_bytes = compiler.serialize_canonical(&b).expect("second serialize");
    assert_eq!(a_bytes, b_bytes, "determinism violation");
}

#[test]
fn all_optimization_passes_represented() {
    let input = fs::read_to_string(fixture_path("journal_optimizations.jsonl"))
        .expect("read fixture");
    let compiler = Compiler::default();
    let script = compiler.compile(&input).expect("compile");
    let passes: std::collections::BTreeSet<String> =
        script.steps.iter().map(|s| s.provenance().pass.clone()).collect();

    let required = [
        "debounce",
        "mouse_move_coalesce",
        "scroll_coalesce",
        "text_merge",
        "semantic_promotion",
        "coordinate_fallback",
        "window_precondition",
        "redacted_parameter",
        "sleep_mark",
    ];
    let missing: Vec<&str> = required
        .iter()
        .filter(|r| !passes.contains(**r))
        .copied()
        .collect();
    assert!(missing.is_empty(), "missing passes: {missing:?}");
}

#[test]
fn provenance_journal_index_ranges_are_consistent() {
    let input = fs::read_to_string(fixture_path("journal_optimizations.jsonl"))
        .expect("read fixture");
    let compiler = Compiler::default();
    let script = compiler.compile(&input).expect("compile");
    let total_events = input.lines().filter(|l| !l.trim().is_empty()).count();
    for step in &script.steps {
        let (start, end) = step.provenance().journal_index_range;
        assert!(start < end, "step has empty range: {step:?}");
        assert!(start < total_events, "start out of bounds: {start}");
        assert!(end <= total_events, "end out of bounds: {end}");
    }
}

#[test]
fn semantic_promotion_suppresses_coordinate_click() {
    let input = fs::read_to_string(fixture_path("journal_optimizations.jsonl"))
        .expect("read fixture");
    let compiler = Compiler::default();
    let script = compiler.compile(&input).expect("compile");
    let clicks: Vec<_> = script
        .steps
        .iter()
        .filter(|s| matches!(s, FlowStep::Click { .. }))
        .collect();
    assert_eq!(
        clicks.len(),
        1,
        "expected exactly 1 Click step (one was suppressed), got {}",
        clicks.len()
    );
    assert_eq!(clicks[0].provenance().pass, "coordinate_fallback");
}

#[test]
fn redacted_ax_value_carries_parameter_descriptor() {
    let input = fs::read_to_string(fixture_path("journal_optimizations.jsonl"))
        .expect("read fixture");
    let compiler = Compiler::default();
    let script = compiler.compile(&input).expect("compile");
    let redacted = script
        .steps
        .iter()
        .find_map(|s| match s {
            FlowStep::AxValue { locator, parameter, .. } => Some((locator.clone(), parameter.clone())),
            _ => None,
        });
    let (locator, parameter) = redacted.expect("expected an AxValue step");
    assert!(locator.contains("password"));
    assert_eq!(parameter.as_deref(), Some("typed_text"));
}
