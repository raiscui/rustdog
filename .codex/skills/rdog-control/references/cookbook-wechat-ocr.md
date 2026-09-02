# WeChat / No-AX App Content Cookbook (OCR Content Layer)

Use this cookbook when the user wants to locate or click content inside WeChat
(or any other app that denies AX access to its content) by *what the content
says*, for example:

- "click the 发现 tab in WeChat"
- "find the chat entry whose last message mentions 发货"
- "open the moments entry that contains 双十一"

The content layer is the **OCR content layer** (`rdog.ocr.v1`): text line boxes
detected from a fresh screenshot. OCR boxes are **coordinate hints**, never AX
refs. Spec: `specs/rdog-ocr-content-layer-plan.md`.

## Table of Contents

- [Use When](#use-when)
- [Default Rule](#default-rule)
- [Recipe: OCR Locate-and-Act](#recipe-ocr-locate-and-act)
- [Request Shape](#request-shape)
- [Reading the OCR Layer](#reading-the-ocr-layer)
- [Matching Rules](#matching-rules)
- [Guarded Coordinate Action](#guarded-coordinate-action)
- [Fresh Verification](#fresh-verification)
- [Do Not](#do-not)
- [Failure Paths](#failure-paths)
- [Known Limitations](#known-limitations)

## Use When

- The target app is WeChat, or any app whose AX tree is unavailable/untrusted
  for content (Temporary No-AX policy).
- The user names content by visible text, not by control semantics.
- AX lookups returned nothing usable, and the app is known no-AX.

Do not use this cookbook as the primary path when the app exposes a real AX
tree — prefer `@ax-find` / `@web-find` semantics first (AX identity beats
pixel/OCR hints whenever both exist).

## Default Rule

Every derived coordinate must come from a **fresh** capture, stay inside the
resolved window rect, and be re-verified by another fresh capture after the
action. OCR text never becomes an AX ref, an observation ref, or a durable
selector — it is a one-shot coordinate hint bound to the capture it came from.

## Recipe: OCR Locate-and-Act

1. Resolve the window (AX identity is fine — the no-AX restriction applies to
   *content*, not to window management):
   ```text
   @window-find:app:WeChat
   ```
2. Capture a fresh window observation with the OCR layer:
   ```text
   @screenshot:{target:"window",window:{window_id:"pid:<pid>/window:0"},include_ocr:true,include_ax:false}
   ```
3. Locate the target: match `ocr.boxes[].text` with the rules below; the box
   `bbox` is `[x, y, w, h]` in os-logical screen coordinates.
4. Click the box center with a guarded coordinate action:
   ```text
   @click:{x:<cx>,y:<cy>,button:"left",count:1,hold_ms:80,coordinate_space:"os-logical",guard:{display:{...}}}
   ```
   Confirm the point is still inside the fresh window rect before sending.
5. Re-capture with `include_ocr:true` and verify the expected change (new
   selection highlight, opened chat, text changed). Failure → report honestly,
   do not retry blindly beyond the agreed retry budget.

## Request Shape

- `include_ocr:true` adds the `rdog.ocr.v1` layer to the screenshot manifest.
- `include_ax:false` for WeChat content (policy); keep `include_windows:true`
  if you also need the window snapshot in the same observation.
- Engine unavailable is a **request-level failure** (`OCR_ENGINE_UNAVAILABLE`,
  `OCR_TIMEOUT`) — surface it, do not fall back to pixel guessing silently.

## Reading the OCR Layer

```json
"ocr": {
  "schema": "rdog.ocr.v1",
  "engine": "oar",
  "language": "zh-Hans",
  "coordinate_space": "os-logical",
  "boxes": [
    {"index": 0, "text": "发现", "bbox": [1716, 174, 86, 41], "confidence": 0.98}
  ]
}
```

- Line-level boxes aggregated from the detector; coordinates are os-logical
  **screen** coordinates (same space as `@click`), so no manual conversion is
  needed for window captures.
- `confidence` is passed through unfiltered. Treat `>= 0.5` as the soft
  reference for agent matching; lower values are detector noise with mixed
  good/bad quality.

## Matching Rules

Learned from live WeChat evaluation (map #95, #99):

- Match case-insensitively and **punctuation-insensitively**; prefer substring
  or suffix matching over exact equality.
- Card/list content is split across adjacent line boxes — join neighbor boxes
  (top-to-bottom, close y, overlapping x) before matching multi-line phrases.
- Beware single-frame jitter: one frame may miss a button or misread a digit.
  Re-capture once before declaring a miss, and never target a box whose text
  matched but confidence is far below the 0.5 reference.
- The display area of apps like Calculator carries stale state across runs;
  when asserting a state change, match the *suffix* the current action must
  produce, not exact equality with an assumed clean state.

## Guarded Coordinate Action

- Use `coordinate_space:"os-logical"` — the same space as the OCR bbox.
- Attach `guard.display` when the display scope is known.
- Before sending, confirm the click point is inside the **fresh** window rect
  (the one returned by the same observation the OCR layer came from).
- The daemon also enforces post-action verification policy; a stale window or
  moved window invalidates the coordinates — re-run from step 2.

## Fresh Verification

- Re-capture with `include_ocr:true` after the action and assert the expected
  OCR-visible change (suffix of new content, highlight change, opened pane).
- One verification frame is one sample: re-capture once on jitter before
  concluding failure.
- If the target app keeps persistent state (Calculator expression history),
  prefer suffix assertions over exact equality.

## Do Not

- Do not treat OCR text as an AX ref, refmap entry, observation ref, or durable
  selector. Next action needs a new capture.
- Do not use `@ax-find` / `@ax-get` / `@ax-action` / `rdog ax-diff` for WeChat
  content (Temporary No-AX policy).
- Do not reuse boxes across captures, runs, or window moves.
- Do not click on a single ambiguous frame; re-capture and re-match.
- Do not fall back to "pixel guessing" when the OCR layer returns a request
  level error — fix the engine path (`OAR_HOME`, model download) instead.

## Failure Paths

| Symptom | Reason / Event | Action |
| --- | --- | --- |
| Model cache missing | `OCR_ENGINE_UNAVAILABLE` | Point `OAR_HOME` at a valid cache; first run downloads from ModelScope |
| Recognition slower than budget | `OCR_TIMEOUT` | Retry once; warm engine is fast (~0.6s), cold start pays init |
| Composite unchanged | `SCREENSHOT_STALE_FRAME` | WindowServer did not produce a new frame; activate window and re-capture |
| OCR text matched, click no-op | (none — silent) | The window was occluded/moved between capture and click; redo from `@window-find` |

## Known Limitations

- Single-digit / single-character boxes jitter (misreads like `1`↔`3` were
  observed in live runs). Prefer text targets of two or more characters.
- The overlay preview draws boxes on screen for human observers; boxes are
  click-through and never intercept input.
- Multi-display: the OCR bbox space is the global os-logical virtual desktop
  (composite origin), so boxes are directly usable with `@click` regardless of
  which display the window sits on.
- Engine is PP-OCRv6 tiny via oar-ocr; handwriting and very small fonts are
  the known weak spots (`rec small` bench pending).
