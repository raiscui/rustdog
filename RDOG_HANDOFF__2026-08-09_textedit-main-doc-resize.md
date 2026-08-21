# TextEdit MAIN_DOC + resize 600x400 — INCOMPLETE HANDOFF

## Status note (one-line)
2026-08-09 01:46 — TextEdit MAIN_DOC write done via AXValue, **resize + verify NOT done** (iteration budget hit).

## DONE
1. `rdog control @ping @capabilities#1` → pong + 11/11 capabilities available.
2. `rdog control @open-app:TextEdit` → ok:true.
3. `rdog control @window-find:TextEdit` → 6 windows found. Frontmost = `pid:6586/window:0`, title "未命名6", initial rect `{x:1828,y:96,width:586,height:476}`. Snapshot `window-snapshot-1786239944822`.
4. `rdog control '@ax-find:{window:{window_id:"pid:6586/window:0"},role:"AXTextArea",limit:5}'` → match: `pid:6586/window:0/path:0.0`, prior value `"RDOG_COPY_TEST_42"`.
5. `rdog control '@type-text:{target:{id:"pid:6586/window:0/path:0.0"},text:"MAIN_DOC",mode:"ax-value"}'` → `delivered_via:"ax-value"`, `performed:true`, `status:"ok"`.

## REMAINING (in order)
1. **Verify text content** (REQUIRED before resize, per protocol):
   ```
   rdog control '@ax-find:{window:{window_id:"pid:6586/window:0"},role:"AXTextArea",include_values:true,limit:5}'
   ```
   Expect `value:"MAIN_DOC"`. If not, do NOT proceed — re-issue type-text.
2. **Resize** to 600x400:
   ```
   rdog control '@window-resize:{window_id:"pid:6586/window:0",size:{width:600,height:400}}'
   ```
3. **Fresh window-find verify** (rect):
   ```
   rdog control @window-find:TextEdit
   ```
   Expect `pid:6586/window:0.rect.width == 600` and `height == 400`.
4. **Fresh AXWindow rect verify**:
   ```
   rdog control '@ax-find:{window:{window_id:"pid:6586/window:0"},role:"AXWindow",limit:5}'
   ```
   Expect match rect `{width:600, height:400}`.
5. **Fresh AXTextArea text verify** (must still be MAIN_DOC):
   ```
   rdog control '@ax-find:{window:{window_id:"pid:6586/window:0"},role:"AXTextArea",include_values:true,limit:5}'
   ```
   Expect `value:"MAIN_DOC"`.

## Expected final result
- AXWindow rect = `{width:600, height:400}`
- AXTextArea value = `"MAIN_DOC"`

## Next-agent starting position
- Window target is locked: `pid:6586/window:0` (frontmost TextEdit, Untitled6).
- Window_id is short_lived — re-issue `@window-find` if more than ~30s passes between calls.
- AXTextArea id `pid:6586/window:0/path:0.0` is also short_lived — re-`@ax-find` if stale.
- Daemon confirmed healthy; no need to re-ping unless something fails.
- 5 remaining rdog calls (steps 1–5 above); budget should be plenty.
- Skip anti-pattern: do NOT pipe output through jq/grep/tail; read `@response` literally from full stdout.

## Hard rules to respect
- Never reuse `@eN` refs across turns.
- Never use `@screenshot` as primary source — fresh AX read is canonical.
- Do NOT use Esc/Delete/Cmd+A shortcuts to clear — already done via mode:"ax-value" replace.
- Do NOT compress the 5 verify steps into one mega-call.
