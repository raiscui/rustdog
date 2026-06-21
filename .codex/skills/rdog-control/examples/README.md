# rdog ax-diff example fixtures

These two JSON files are minimal AxSnapshot fixtures that demonstrate a
typical "click a sidebar item and see its action set / description change"
scenario on a Chrome Xiaohongshu page.

They are the canonical example for the `rdog ax-diff` subcommand and
the AX-JSON-diff evidence pattern documented in
`references/cookbook-web-content.md` and `SKILL.md`.

## Files

- `xhs_before.json` — AxSnapshot taken before clicking "首页" (home)
  on the Xiaohongshu left navigation. The "点点" link has description
  `"点点"` and only the `AXPress` action. The "首页" link also has
  `AXPress` only.
- `xhs_after.json` — AxSnapshot taken after a click that added an
  `AXShowMenu` action to the "首页" link and changed the "点点" link
  description to `"点点 ai"`.

## Run The Diff

```bash
# default text format (人/agent 友好)
rdog ax-diff --before xhs_before.json --after xhs_after.json --format text

# single-line summary
rdog ax-diff --before xhs_before.json --after xhs_after.json --format summary

# full JSON (程序消费)
rdog ax-diff --before xhs_before.json --after xhs_after.json --format json
```

Expected text output (one element gets a new `AXShowMenu` action,
the other element's description changes from `"点点"` to `"点点 ai"`):

```text
windows: +0 -0 ~0 | elements: +0 -0 ~2

[element ~] pid:8231/window:0/path:0.0 (window: pid:8231/window:0)
    actions.added : [] -> ["\"AXShowMenu\""]
    actions.removed : [] -> []

[element ~] pid:8231/window:0/path:0.1 (window: pid:8231/window:0)
    description : "点点" -> "点点 ai"
```

Exit code is `1` because the two snapshots differ.

## Capturing Real Snapshots

The fixture ids are `pid:8231/window:0/path:...`. To get real snapshots
from a target machine, use `@observe` over `rdog control`:

```bash
rdog control mac.lab '@observe#1:{mode:"hybrid",include_screenshot:false,include_ax:true,include_windows:true,ax_mode:"interactive"}'
```

Then extract `value.windows` from the `@response` and write it to a JSON
file. `rdog ax-diff` works directly on the full `value` object too —
it just ignores the non-`windows` fields.
