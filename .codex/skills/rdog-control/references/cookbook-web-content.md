# Web Page Content Cookbook

Use this cookbook when the user wants to inspect, search, or click controls inside the current browser page.
The goal is to stay inside the page's accessibility subtree, not to mix page content with browser chrome.

## Table of Contents

- [Use When](#use-when)
- [Default Rule](#default-rule)
- [Recipe: AXWebArea-first](#recipe-axwebarea-first)
- [Recommended Query Shape](#recommended-query-shape)
- [Matching Fields](#matching-fields)
- [Action Selection](#action-selection)
- [Fast Repeat Path](#fast-repeat-path)
- [Do Not](#do-not)
- [Evidence Pattern](#evidence-pattern)
- [Known Limitation](#known-limitation)
- [Example: Xiaohongshu Left Navigation](#example-xiaohongshu-left-navigation)
- [Future Cookbook Template](#future-cookbook-template)

## Use When

Use this recipe when the user's target is phrased as page content, for example:

- "click the Home button in the web page"
- "find the sidebar item inside Xiaohongshu"
- "list buttons inside the currently active tab"
- "operate a link, button, menu item, or text field rendered by the page"

Do not use this as the main path when the user explicitly asks for browser chrome, for example:

- a tab title or tab close button
- address bar / omnibox
- toolbar buttons
- extension buttons
- bookmarks bar
- browser menu commands

## Default Rule

For "web page content" targets, start from the active browser window's `AXWebArea`, not from the entire `AXWindow`.

The preferred mental model is:

```text
active browser window
└── current tab page
    └── AXWebArea
        ├── AXLink / AXButton / AXMenuButton / AXTextField / ...
        └── page-owned content only
```

This keeps browser tabs, address bar, toolbar buttons, extensions, and bookmarks out of the primary search space.
It also reduces token load and lowers the risk of matching the wrong "Home", "Search", or "Back" control.

## Recipe: AXWebArea-first

1. Run a read-only GUI bootstrap first:
   ```text
   @bootstrap#1:{mode:"gui",capability_policy:"fresh",observe:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}}
   ```
   Parse the `rdog.bootstrap.v1` response before acting.
   If the daemon is older, fall back to `@ping#1`, `@capabilities#2`, and `@observe#3:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}` in one session.
2. For read-only page-content lookup, use `@web-find` before manual AX drilling:
   `@web-find#2:{target:{browser:"active"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}`
   - It searches the active browser window's `AXWebArea`.
   - It excludes browser chrome.
   - It returns candidate ids / refs, matched field, actions, and trace evidence.
   - It does not click, type, focus, scroll, or move the mouse.
   - If the active browser is ambiguous, use a known window id instead:
     `@web-find#22:{target:{window_id:"pid:96405/window:3"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}`.
   - If you have a fresh window ref from `@observe` or `@window-find`, use it directly:
     `@web-find#23:{target:{window_ref:"@e1",observation_id:"obs-..."},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}`.
     This resolves the short-lived ref to its backend `window_id` and then uses the same window-scoped lookup.
     This keeps the lookup read-only while avoiding `BROWSER_WINDOW_AMBIGUOUS`.
3. For a simple page-content press action, use `@web-act` when side effects are intended:
   `@web-act#3:{target:{browser:"active"},match:{text:"首页"},action:"press",verify:true}`
   - It reuses the same `AXWebArea` search shape.
   - It executes only a unique `AXPress` match.
   - It re-finds once if the first action target is stale-like.
   - It verifies with a fresh `AXWebArea` subtree first, with fresh AX snapshot fallback when subtree refresh is unavailable.
   - It does not use mouse fallback.
   - The same window-scoped target can be used when the intended browser window is already known:
     `@web-act#23:{target:{window_id:"pid:96405/window:3"},match:{text:"首页"},action:"press",verify:true}`.
   - A fresh window ref also works:
     `@web-act#24:{target:{window_ref:"@e1",observation_id:"obs-..."},match:{text:"首页"},action:"press",verify:true}`.
4. If `@web-find` / `@web-act` is unavailable or returns a structured blocker, locate the active browser window manually:
   - Prefer a current observation or `@window-find` result.
   - Keep `active_window_only:true` when the user asks about the current tab or active page.
5. Find the `AXWebArea` inside that window:
   - Use `@ax-tree` / `@ax-get` with a small depth first.
   - Drill into the window subtree until one or more `AXWebArea` nodes appear.
   - Prefer the visible/current `AXWebArea` associated with the active tab.
6. Search only inside the chosen `AXWebArea` subtree:
   - Increase depth and max element count only after the `AXWebArea` root is known.
   - Search fields such as `description`, `name`, and `value`.
7. Prefer semantic action over raw mouse:
   - If the target exposes `AXPress`, use `@ax-action ... action:"AXPress"`.
   - Keep coordinate or mouse fallback explicit and evidence-based.
8. Verify with a fresh AX read, page state, URL change, or screenshot if visual proof is allowed.
   For feed-changing pages, prefer a before/after screenshot diff; `performed:true` is action evidence, not visual success proof.

## Recommended Query Shape

`@web-find` is the preferred read-only helper for this scope:

```text
@web-find#id:{target:{browser:"active"},match:{text:"..."},roles:["AXLink","AXButton","AXMenuButton","AXGroup"],limit:20}
@web-find#id:{target:{window_id:"pid:96405/window:3"},match:{text:"..."},roles:["AXLink","AXButton","AXMenuButton","AXGroup"],limit:20}
@web-find#id:{target:{window_ref:"@e1",observation_id:"obs-..."},match:{text:"..."},roles:["AXLink","AXButton","AXMenuButton","AXGroup"],limit:20}
```

`@web-act` is the preferred one-shot semantic press helper when the user explicitly wants the action:

```text
@web-act#id:{target:{browser:"active"},match:{text:"..."},action:"press",verify:true}
@web-act#id:{target:{window_id:"pid:96405/window:3"},match:{text:"..."},action:"press",verify:true}
@web-act#id:{target:{window_ref:"@e1",observation_id:"obs-..."},match:{text:"..."},action:"press",verify:true}
```

The helper maps to this logical query shape:

```json
{
  "scope": "active_web_area",
  "active_window_only": true,
  "target_window_id": null,
  "target_window_ref": null,
  "observation_id": null,
  "include_browser_chrome": false,
  "visible_only": true,
  "actionable_only": true,
  "match_fields": ["description", "name", "value"],
  "prefer_roles": ["AXLink", "AXButton", "AXMenuButton", "AXGroup"],
  "max_depth": 8,
  "max_elements": 800,
  "limit": 20
}
```

When `target.window_id` is present, set `scope` to `target_window_web_area` and treat `active_window_only` as false.
When `target.window_ref + observation_id` is present, first resolve the short-lived ref to a backend `window_id`; if it is expired, stale, or not a window ref, stop with `WINDOW_REF_INVALID`.
Do not use this as an implicit activation step; if the window is hidden or not interactable, that is a separate `@window-activate` / `@ax-focus` decision.

When the helper is not available, implement the same shape manually with `@window-find`, `@ax-get`, `@ax-tree`, and follow-up `@ax-action` / `@ax-press`.

## Matching Fields

For browser page content, do not rely on `name` alone.

Check these fields in order:

1. `description`
2. `name`
3. `value`
4. role-specific text exposed by child static-text nodes, if the node itself is an actionable container

Why this matters:

- Many web links in Chrome expose the visible or accessibility-label text through `description`.
- A node may be actionable even when `name` is empty.
- Some web frameworks put text on a nested child while the clickable ancestor owns `AXPress`.

## Action Selection

Prefer action targets with:

- role `AXLink`, `AXButton`, or `AXMenuButton`
- action `AXPress`
- visible rect inside the selected `AXWebArea`
- text match in `description`, `name`, or `value`

If the text is on a non-actionable child, walk upward to the nearest actionable ancestor inside the same `AXWebArea`.
Do not jump outside the `AXWebArea` unless the user explicitly wants browser chrome.

## Fast Repeat Path

Use this only after a fresh `@web-find`, `@ax-get`, or equivalent AX read has proven the target is page-owned and exposes `AXPress`.

For repeated clicks on the same page-owned control:

1. Cache the selected AX id returned by `@web-find`.
2. For the next click, call `@ax-action` directly:
   `@ax-action#id:{target:{id:"pid:.../window:0/path:..."},action:"AXPress"}`
3. If the task's success is visual, capture before/after screenshots and compare the relevant page region.
4. If direct `@ax-action` returns stale, not found, or action failed, re-run `@web-find` and update the cached id.

Live Xiaohongshu evidence showed this pattern clearly:

- `@web-find` found the deep “首页” link through `refresh-web-area-subtree`.
- `@web-act verify:false` returned `performed:true`, but still took about `11.25s`.
- Direct `@ax-action` on the cached page-owned AX id returned in about `0.03s`.
- The click was only counted successful after the feed crop changed in before/after screenshot diff.

Do not put this live fast path into the default fixture runner.
It is a side-effectful live workflow and must stay opt-in.

## Do Not

- Do not start a web-content search from the entire `AXWindow` unless `AXWebArea` cannot be found.
- Do not treat OCR, screenshot recognition, or visible pixels as AX evidence.
- Do not report "AX can capture it" unless the response actually includes the matching AX node or a verified AX path.
- Do not mix current-tab page controls with background tabs.
- Do not create browser-specific one-off guesses before checking whether the page exposes standard AX roles.

## Evidence Pattern

A good evidence chain for this scenario includes:

```text
@capabilities#1
@web-find#2:{target:{browser:"active"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}
@ax-action#3:{target:{id:"pid:.../window:0/path:..."},action:"AXPress"}
```

When answering the user, cite the AX facts:

- target role
- matched field, for example `description:"首页"`
- action availability, for example `actions:["AXPress"]`
- the selected path or ref
- `action_result.performed`
- visual proof when the task changes visible content, for example a cropped before/after screenshot diff

For feed-changing pages, the success criterion is the feed changing, not merely `performed:true`.
If daemon `@screenshot` is permission-denied but the agent is on the same Mac, local `screencapture` can be used as visual evidence; say explicitly that the visual proof came from local capture, not daemon screenshot.

## Known Limitation

Live testing has exposed two important edge cases:

- `@ax-get` on the Chrome `AXWebArea` subtree could expose matching page links under `description`.
- A broad `@ax-find` with `description_contains` could still return `match_count:0` in the same scenario.
- `@web-act verify:true` can time out after the action on pages that heavily re-render, even when the click has already changed the feed.

Treat these as known limitations to route around, not as proof that AX cannot capture the page content.
When broad AX search misses a web page target, use `@web-find` or drill into `AXWebArea` with `@ax-get`.
When `@web-act verify:true` times out on a feed-changing page, inspect whether action happened with screenshot diff, then prefer the cached-id `@ax-action` fast path for repeats.

## Example: Xiaohongshu Left Navigation

Live AX evidence from a Chrome Xiaohongshu page showed that the left navigation items were available under the page `AXWebArea`.
The relevant pattern was:

```text
@ax-get#17:{target:{id:"pid:8231/window:0/path:0.0.0.0.1.0.0.0"},depth:8,max_elements:2000,include_values:true}
```

Observed page-owned navigation controls included:

| Text | AX role | Field | Action |
| --- | --- | --- | --- |
| 首页 | `AXLink` | `description:"首页"` | `AXPress` |
| 点点 | `AXLink` | `description:"点点 ai"` | `AXPress` |
| 直播 | `AXLink` | `description:"直播"` | `AXPress` |
| 发布 | `AXLink` | `description:"发布"` | `AXPress` |
| 消息 | `AXLink` | `description:"消息"` | `AXPress` |

The conclusion for this specific page is:

- AX can capture the left navigation list.
- The correct proof is the AX subtree under `AXWebArea`, not OCR and not screenshot recognition.
- The preferred click path is `AXPress` on the matched `AXLink`; mouse coordinates are a fallback only.

## Future Cookbook Template

Use this shape for later scenario cookbooks such as WeChat or Finder:

```markdown
# Scenario Name Cookbook

## Use When
## Default Rule
## Recipe
## Recommended Query Shape
## Matching Fields
## Action Selection
## Do Not
## Evidence Pattern
## Known Limitation
## Verified Examples
```

Only create a new cookbook file after there is verified experience to record.
