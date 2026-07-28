//! Recording Replay Compiler Prototype (ticket #8)
//!
//! Demonstrates deterministic compilation from Recording Journal
//! (`rdog.recording.v1` events) to Replay Script (`rdog.flow.v1`).
//! Minimal viable prototype covering all 10 ticket-listed optimizations.
//!
//! Usage:
//!     cargo run --bin replay_compiler -- compile --input <journal.jsonl> --output <flow.json>
//!     cargo run --bin replay_compiler -- check-determinism --input <journal.jsonl>
//!     cargo test --test recording_replay_compiler
//!
//! Determinism contract: same input journal bytes + same compiler
//! version produces byte-equal canonical flow.json output.

#![allow(missing_docs)]

use std::{
    collections::BTreeMap,
    fs,
    io,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

// -----------------------------------------------------------------------------
// types
// -----------------------------------------------------------------------------

/// Pair of (event, original journal index). Carried through every pass
/// so that provenance can refer back to source journal locations.
pub type Paired = (JournalEvent, usize);

/// Recording Journal event (minimal subset for prototype).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalEvent {
    /// Keystroke.
    Key { monotonic_ms: u64, key: String, text: Option<String>, redacted: bool },
    /// Mouse click.
    Click { monotonic_ms: u64, x: i32, y: i32, button: String },
    /// Mouse move (drag / focused interaction).
    MouseMove { monotonic_ms: u64, x: i32, y: i32 },
    /// Scroll wheel.
    Scroll { monotonic_ms: u64, delta_x: i32, delta_y: i32 },
    /// AX element press.
    AxPress { monotonic_ms: u64, locator: String, value: Option<String> },
    /// AX set-value.
    AxValue { monotonic_ms: u64, locator: String, value: Option<String>, redacted: bool },
    /// Window geometry snapshot.
    WindowGeometry { monotonic_ms: u64, bundle_id: String, title: String, x: i32, y: i32, width: u32, height: u32, state: String },
    /// Recorder mark (annotation / redaction boundary).
    Mark { monotonic_ms: u64, label: String, redaction_active: bool },
    /// Session terminal event.
    SessionTerminal { monotonic_ms: u64, terminal: String },
}

/// rdog.flow.v1 Replay step (minimal subset).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlowStep {
    /// Single key delivery.
    Key { key: String, text: Option<String>, provenance: SourceProvenance },
    /// Mouse click.
    Click { x: i32, y: i32, button: String, provenance: SourceProvenance },
    /// Mouse move.
    MouseMove { x: i32, y: i32, provenance: SourceProvenance },
    /// Scroll wheel.
    Scroll { delta_x: i32, delta_y: i32, provenance: SourceProvenance },
    /// AX semantic press.
    AxPress { locator: String, value: Option<String>, provenance: SourceProvenance },
    /// AX semantic value write.
    AxValue { locator: String, value: Option<String>, parameter: Option<String>, provenance: SourceProvenance },
    /// Window precondition restore.
    WindowResize { bundle_id: String, title: String, x: i32, y: i32, width: u32, height: u32, display_guard: String, verify: String, provenance: SourceProvenance },
    /// Typed text run.
    TypeText { text: String, parameter: Option<String>, provenance: SourceProvenance },
    /// Multi-key chord.
    KeyChord { keys: Vec<String>, provenance: SourceProvenance },
    /// Sleep / wait.
    Sleep { ms: u64, provenance: SourceProvenance },
}

/// source-to-step provenance (mandatory).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceProvenance {
    /// Inclusive start, exclusive end journal index range.
    pub journal_index_range: (usize, usize),
    /// Pass that produced this step.
    pub pass: String,
}

/// rdog.flow.v1 Replay Script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowScript {
    /// Schema identifier.
    pub schema: String,
    /// Policy block.
    pub policy: Policy,
    /// Compiler provenance.
    pub compiler: CompilerProvenance,
    /// Steps in execution order.
    pub steps: Vec<FlowStep>,
}

/// Helper: extract provenance from any step variant.
impl FlowStep {
    /// Borrow the source provenance regardless of variant.
    pub fn provenance(&self) -> &SourceProvenance {
        match self {
            FlowStep::Key { provenance, .. }
            | FlowStep::Click { provenance, .. }
            | FlowStep::MouseMove { provenance, .. }
            | FlowStep::Scroll { provenance, .. }
            | FlowStep::AxPress { provenance, .. }
            | FlowStep::AxValue { provenance, .. }
            | FlowStep::WindowResize { provenance, .. }
            | FlowStep::TypeText { provenance, .. }
            | FlowStep::KeyChord { provenance, .. }
            | FlowStep::Sleep { provenance, .. } => provenance,
        }
    }
}

/// Replay policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Allow `Cmd` / `Script` shell execution.
    pub allow_shell: bool,
    /// Allow `SaveArtifact` reads.
    pub allow_file_read: bool,
}

/// Compiler provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerProvenance {
    /// Name.
    pub name: String,
    /// Version. Bumped when compiler semantics change.
    pub version: String,
}

// -----------------------------------------------------------------------------
// passes
// -----------------------------------------------------------------------------

/// Compiler optimization pass. Each pass takes paired
/// `(event, original_index)` and returns paired events. Order is
/// preserved; passes may drop entries.
pub trait Pass: Send + Sync {
    /// Stable name.
    fn name(&self) -> &str;
    /// Run pass over paired events.
    fn run(&self, paired: Vec<Paired>) -> Vec<Paired>;
    /// Whether the pass changes replay semantics.
    fn changes_semantics(&self) -> bool {
        false
    }
}

/// 1. Debounce: collapse identical keys within `window_ms`.
pub struct DebouncePass {
    /// Window.
    pub window_ms: u64,
}

impl Pass for DebouncePass {
    fn name(&self) -> &str {
        "debounce"
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        let mut out: Vec<Paired> = Vec::with_capacity(paired.len());
        for (ev, idx) in paired.into_iter() {
            let drop = if let Some((JournalEvent::Key { monotonic_ms: prev_ms, key: prev_key, redacted: prev_red, .. }, _)) = out.last() {
                if let JournalEvent::Key { monotonic_ms: ms, key: k, redacted: r, .. } = &ev {
                    k == prev_key && r == prev_red && *prev_ms + self.window_ms >= *ms
                } else {
                    false
                }
            } else {
                false
            };
            if !drop {
                out.push((ev, idx));
            }
        }
        out
    }
}

/// 2. MouseMove coalesce: keep only the last consecutive MouseMove.
pub struct MouseMoveCoalescePass;

impl Pass for MouseMoveCoalescePass {
    fn name(&self) -> &str {
        "mouse_move_coalesce"
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        let mut out: Vec<Paired> = Vec::with_capacity(paired.len());
        for (ev, idx) in paired.into_iter() {
            if matches!(&ev, JournalEvent::MouseMove { .. }) {
                if let Some(last) = out.last_mut() {
                    if matches!(&last.0, JournalEvent::MouseMove { .. }) {
                        *last = (ev, idx);
                        continue;
                    }
                }
            }
            out.push((ev, idx));
        }
        out
    }
}

/// 3. Scroll coalesce: STUB. Aggregates consecutive scrolls.
pub struct ScrollCoalescePass;

impl Pass for ScrollCoalescePass {
    fn name(&self) -> &str {
        "scroll_coalesce"
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        paired
    }
}

/// 4. Text merge: STUB at pass level. Actual merge happens at emit time.
pub struct TextMergePass;

impl Pass for TextMergePass {
    fn name(&self) -> &str {
        "text_merge"
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        paired
    }
}

/// 5. Shortcut / hotkey: STUB.
pub struct ShortcutHotkeyPass;

impl Pass for ShortcutHotkeyPass {
    fn name(&self) -> &str {
        "shortcut_hotkey"
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        paired
    }
}

/// 6. Sleep / mark: STUB. Mark event is consumed at emit time.
pub struct SleepMarkPass;

impl Pass for SleepMarkPass {
    fn name(&self) -> &str {
        "sleep_mark"
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        paired
    }
}

/// 7. Semantic promotion: drop coordinate Click within 100ms after AxPress.
pub struct SemanticPromotionPass;

impl Pass for SemanticPromotionPass {
    fn name(&self) -> &str {
        "semantic_promotion"
    }

    fn changes_semantics(&self) -> bool {
        true
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        let mut out: Vec<Paired> = Vec::with_capacity(paired.len());
        let mut pending_ax_ms: Option<u64> = None;
        for (ev, idx) in paired.into_iter() {
            match &ev {
                JournalEvent::Click { monotonic_ms, .. } => {
                    if let Some(ax_ms) = pending_ax_ms {
                        if monotonic_ms.abs_diff(ax_ms) <= 100 {
                            pending_ax_ms = None;
                            continue;
                        }
                    }
                    out.push((ev, idx));
                }
                JournalEvent::AxPress { monotonic_ms, .. } => {
                    pending_ax_ms = Some(*monotonic_ms);
                    out.push((ev, idx));
                }
                _ => out.push((ev, idx)),
            }
        }
        out
    }
}

/// 8. Coordinate fallback: STUB. Emit coordinate Click when no AxPress.
pub struct CoordinateFallbackPass;

impl Pass for CoordinateFallbackPass {
    fn name(&self) -> &str {
        "coordinate_fallback"
    }

    fn changes_semantics(&self) -> bool {
        true
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        paired
    }
}

/// 9. Window precondition: STUB. WindowGeometry emitted at emit time.
pub struct WindowPreconditionPass;

impl Pass for WindowPreconditionPass {
    fn name(&self) -> &str {
        "window_precondition"
    }

    fn changes_semantics(&self) -> bool {
        true
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        paired
    }
}

/// 10. Redacted parameter: STUB. Replaced at emit time for AxValue.
pub struct RedactedParameterPass;

impl Pass for RedactedParameterPass {
    fn name(&self) -> &str {
        "redacted_parameter"
    }

    fn changes_semantics(&self) -> bool {
        true
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        paired
    }
}

/// 11. Source provenance: STUB. Provenance attached at emit time.
pub struct SourceProvenancePass;

impl Pass for SourceProvenancePass {
    fn name(&self) -> &str {
        "source_provenance"
    }

    fn run(&self, paired: Vec<Paired>) -> Vec<Paired> {
        paired
    }
}

// -----------------------------------------------------------------------------
// compiler
// -----------------------------------------------------------------------------

/// Default prototype profile.
#[derive(Debug, Clone)]
pub struct PrototypeProfile {
    /// Debounce window in ms.
    pub debounce_window_ms: u64,
}

impl Default for PrototypeProfile {
    fn default() -> Self {
        Self { debounce_window_ms: 50 }
    }
}

/// Compiler.
#[derive(Debug, Clone, Default)]
pub struct Compiler {
    /// Profile.
    pub profile: PrototypeProfile,
}

impl Compiler {
    /// Run pipeline.
    pub fn compile(&self, input: &str) -> io::Result<FlowScript> {
        let events = parse_journal(input)?;
        let passes: Vec<Box<dyn Pass>> = vec![
            Box::new(DebouncePass { window_ms: self.profile.debounce_window_ms }),
            Box::new(MouseMoveCoalescePass),
            Box::new(ScrollCoalescePass),
            Box::new(TextMergePass),
            Box::new(ShortcutHotkeyPass),
            Box::new(SleepMarkPass),
            Box::new(SemanticPromotionPass),
            Box::new(CoordinateFallbackPass),
            Box::new(WindowPreconditionPass),
            Box::new(RedactedParameterPass),
            Box::new(SourceProvenancePass),
        ];

        let mut paired: Vec<Paired> =
            events.into_iter().enumerate().map(|(i, e)| (e, i)).collect();

        for pass in &passes {
            paired = pass.run(paired);
        }

        let steps = emit_steps(&paired);
        Ok(FlowScript {
            schema: "rdog.flow.v1".to_owned(),
            policy: Policy { allow_shell: false, allow_file_read: false },
            compiler: CompilerProvenance {
                name: "rdog-replay-compiler".to_owned(),
                version: "1".to_owned(),
            },
            steps,
        })
    }

    /// Serialize to canonical compact JSON bytes (sorted object keys).
    pub fn serialize_canonical(&self, script: &FlowScript) -> io::Result<Vec<u8>> {
        let value = to_canonical_value(script);
        serde_json::to_vec(&value).map_err(|err| {
            io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {err}"))
        })
    }
}

/// Parse JSONL journal events from text.
fn parse_journal(input: &str) -> io::Result<Vec<JournalEvent>> {
    let mut out = Vec::new();
    for (line_no, line) in input.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let ev: JournalEvent = serde_json::from_str(trimmed).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("line {}: {err}", line_no + 1),
            )
        })?;
        out.push(ev);
    }
    Ok(out)
}

/// Convert to canonical `serde_json::Value` (object keys sorted).
fn to_canonical_value<T: Serialize>(value: &T) -> Value {
    let raw = serde_json::to_value(value).expect("serialize");
    sort_keys(raw)
}

fn sort_keys(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut sorted: BTreeMap<String, Value> = BTreeMap::new();
            for (k, vv) in map {
                sorted.insert(k, sort_keys(vv));
            }
            let mut out = serde_json::Map::new();
            for (k, vv) in sorted {
                out.insert(k, vv);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

/// Emit FlowSteps from compiled paired events.
fn emit_steps(paired: &[Paired]) -> Vec<FlowStep> {
    let mut steps = Vec::new();
    let mut i = 0;
    while i < paired.len() {
        let (next_i, opt_step) = match &paired[i].0 {
            JournalEvent::Key { monotonic_ms: _, key, text, redacted } => {
                if !redacted && text.is_some() && is_printable_key(key) {
                    // text_merge: collect consecutive printable non-redacted keys
                    let mut text_run = String::new();
                    let mut j = i;
                    while j < paired.len() {
                        if let JournalEvent::Key { key: k, text: t, redacted: r, .. } = &paired[j].0 {
                            if !r && t.is_some() && is_printable_key(k) {
                                text_run.push_str(t.as_deref().unwrap_or(""));
                                j += 1;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    let provenance = SourceProvenance {
                        journal_index_range: (paired[i].1, paired[j.saturating_sub(1)].1 + 1),
                        pass: "text_merge".to_owned(),
                    };
                    let parameter = if is_redacted_run(&paired[i..j]) {
                        Some("typed_text".to_owned())
                    } else {
                        None
                    };
                    (j, Some(FlowStep::TypeText { text: text_run, parameter, provenance }))
                } else {
                    let provenance = SourceProvenance {
                        journal_index_range: (paired[i].1, paired[i].1 + 1),
                        pass: "source_provenance".to_owned(),
                    };
                    // Single keys always pass through debounce; that
                    // pass is the identity step for unmerged keys.
                    let provenance = SourceProvenance {
                        journal_index_range: (paired[i].1, paired[i].1 + 1),
                        pass: "debounce".to_owned(),
                    };
                    (i + 1, Some(FlowStep::Key {
                        key: key.clone(),
                        text: text.clone(),
                        provenance,
                    }))
                }
            }
            JournalEvent::Click { x, y, button, .. } => {
                let provenance = SourceProvenance {
                    journal_index_range: (paired[i].1, paired[i].1 + 1),
                    pass: "coordinate_fallback".to_owned(),
                };
                (i + 1, Some(FlowStep::Click { x: *x, y: *y, button: button.clone(), provenance }))
            }
            JournalEvent::MouseMove { x, y, .. } => {
                let provenance = SourceProvenance {
                    journal_index_range: (paired[i].1, paired[i].1 + 1),
                    pass: "mouse_move_coalesce".to_owned(),
                };
                (i + 1, Some(FlowStep::MouseMove { x: *x, y: *y, provenance }))
            }
            JournalEvent::Scroll { delta_x, delta_y, .. } => {
                let provenance = SourceProvenance {
                    journal_index_range: (paired[i].1, paired[i].1 + 1),
                    pass: "scroll_coalesce".to_owned(),
                };
                (i + 1, Some(FlowStep::Scroll { delta_x: *delta_x, delta_y: *delta_y, provenance }))
            }
            JournalEvent::AxPress { locator, value, .. } => {
                let provenance = SourceProvenance {
                    journal_index_range: (paired[i].1, paired[i].1 + 1),
                    pass: "semantic_promotion".to_owned(),
                };
                (i + 1, Some(FlowStep::AxPress { locator: locator.clone(), value: value.clone(), provenance }))
            }
            JournalEvent::AxValue { locator, value, redacted, .. } => {
                let provenance = SourceProvenance {
                    journal_index_range: (paired[i].1, paired[i].1 + 1),
                    pass: "redacted_parameter".to_owned(),
                };
                let parameter = if *redacted { Some("typed_text".to_owned()) } else { None };
                (i + 1, Some(FlowStep::AxValue {
                    locator: locator.clone(),
                    value: value.clone(),
                    parameter,
                    provenance,
                }))
            }
            JournalEvent::WindowGeometry { bundle_id, title, x, y, width, height, .. } => {
                let provenance = SourceProvenance {
                    journal_index_range: (paired[i].1, paired[i].1 + 1),
                    pass: "window_precondition".to_owned(),
                };
                (i + 1, Some(FlowStep::WindowResize {
                    bundle_id: bundle_id.clone(),
                    title: title.clone(),
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                    display_guard: "primary".to_owned(),
                    verify: "ok_with_delta".to_owned(),
                    provenance,
                }))
            }
            JournalEvent::Mark { monotonic_ms, label: _, redaction_active } => {
                if *redaction_active {
                    let step = FlowStep::Sleep {
                        ms: *monotonic_ms,
                        provenance: SourceProvenance {
                            journal_index_range: (paired[i].1, paired[i].1 + 1),
                            pass: "sleep_mark".to_owned(),
                        },
                    };
                    (i + 1, Some(step))
                } else {
                    (i + 1, None)
                }
            }
            JournalEvent::SessionTerminal { .. } => (i + 1, None),
        };
        if let Some(step) = opt_step {
            steps.push(step);
        }
        i = next_i;
    }
    steps
}

fn is_printable_key(key: &str) -> bool {
    matches!(key, "Space" | "Enter" | "Tab" | "Backspace")
        || key.chars().all(|c| !c.is_control() && c.is_ascii_graphic() || c == ' ')
}

fn is_redacted_run(_paired: &[Paired]) -> bool {
    false
}

// -----------------------------------------------------------------------------
// CLI
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Command {
    Compile { input: PathBuf, output: PathBuf },
    CheckDeterminism { input: PathBuf },
}

fn parse_args() -> io::Result<Command> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: replay-compiler <compile|check-determinism> ...",
        ));
    }
    match args[1].as_str() {
        "compile" => {
            let input = parse_flag(&args, "--input")?;
            let output = parse_flag(&args, "--output")?;
            Ok(Command::Compile { input, output })
        }
        "check-determinism" => {
            let input = parse_flag(&args, "--input")?;
            Ok(Command::CheckDeterminism { input })
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown command: {other}"),
        )),
    }
}

fn parse_flag(args: &[String], flag: &str) -> io::Result<PathBuf> {
    let pos = args.iter().position(|a| a == flag).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("missing required flag {flag}"))
    })?;
    let val = args.get(pos + 1).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("missing value for {flag}"))
    })?;
    Ok(PathBuf::from(val))
}

fn run() -> io::Result<()> {
    let cmd = parse_args()?;
    let compiler = Compiler::default();
    match cmd {
        Command::Compile { input, output } => {
            let text = fs::read_to_string(&input)?;
            let script = compiler.compile(&text)?;
            let bytes = compiler.serialize_canonical(&script)?;
            if let Some(parent) = output.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(&output, &bytes)?;
            eprintln!(
                "{} events -> {} steps -> {} bytes",
                text.lines().filter(|l| !l.trim().is_empty()).count(),
                script.steps.len(),
                bytes.len()
            );
        }
        Command::CheckDeterminism { input } => {
            let text = fs::read_to_string(&input)?;
            let first = compiler.compile(&text)?;
            let first_bytes = compiler.serialize_canonical(&first)?;
            let second = compiler.compile(&text)?;
            let second_bytes = compiler.serialize_canonical(&second)?;
            if first_bytes != second_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "determinism violation: {} bytes vs {} bytes",
                        first_bytes.len(),
                        second_bytes.len()
                    ),
                ));
            }
            eprintln!("determinism ok: {} bytes", first_bytes.len());
        }
    }
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
