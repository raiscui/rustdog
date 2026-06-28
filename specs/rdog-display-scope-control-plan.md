# rdog display scope resolver 多显示器控制计划

## Implementation Status

2026-06-25 第一版已落地:

- 共享 resolver: `src/control_display_scope.rs`。
- 请求侧 canonical 形态: `scope:{display:{...}}`;mouse action guard 使用 `guard:{display:{...}}`。
- 已接入: `@observe`、`@window-find`、`@ax-find`、`@web-find`、`@web-act`、`@mouse-move`、`@click`、`@drag`、`@wheel`、`@bootstrap` nested observe。
- 已拒绝: 顶层 `display_id`、`scope:{display:{ref:"@d2"}}`、缺少 `observation_id` 的 `window_ref` selector、`@mouse-button` display guard。
- `@screenshot` manifest 已补 `display_id`、`display_id_stability`、`stable_key`、`primary`,并保留 `id` / `is_primary` 兼容 alias。
- `@observe` visual lane 当前是 metadata-only display scope: 响应必须诚实返回 `scope_applied:false` 与 `scope_reason:"metadata_only"`,直到后续真正裁剪 scoped image。

## Requirements Summary

用户已经确认多显示器控制请求侧采用结构化 display scope:

```text
scope:{display:{id:"d2"}}
```

同时第一版要支持 `display:{name_contains:"DELL"}`、`display:{contains_point:{...}}`、`display:{window_id:"..."}`、`display:{window_ref:"@e4",observation_id:"obs-..."}` 这类 resolver。
`display_id` 保留为 resolve 后的显示器身份字段,不作为顶层请求字段。
本计划目标是让 `rdog` 在双屏 / 多屏环境下,支持 agent 快速指定或过滤目标显示器,并让后续观察、查询、语义动作和鼠标 fallback 都继承同一个 display scope。

当前事实:

- `@screenshot` 已经默认捕获所有 active displays,返回 composite JPEG 和 manifest JSON;manifest 是截图坐标和后续鼠标坐标之间的单一真相源。见 `specs/rdog-multi-display-screenshot-coordinate-plan.md:5-18`。
- 现有 manifest 已包含 `display_count`、`displays[].os_rect`、`displays[].image_rect`、`gaps` 等字段。见 `specs/rdog-multi-display-screenshot-coordinate-plan.md:65-80`。
- 鼠标协议已经要求复用 `@screenshot` manifest 的 `os-logical` 坐标语义。见 `specs/rdog-mouse-control-coordinate-plan.md:5-6` 和 `specs/rdog-mouse-control-coordinate-plan.md:50-62`。
- observation 设计已经有 `scope` 字段,并明确 `@eN` 是 observation 内短期 locator。见 `specs/rdog-observation-scoped-refmap-plan.md:53-63` 和 `specs/rdog-observation-scoped-refmap-plan.md:65-84`。
- CLI one-shot 层把末尾连续以 `@` 开头的 argv 抽成 one-shot lines。见 `src/main.rs:59-80`。因此 display 不应使用 `@d2` 这种容易被误解为 control line 的形状。
- `parse_control_line` 已经将 `@observe`、`@click`、`@window-find`、`@ax-find`、`@web-find` 等显式命令分发给各自 payload parser。见 `src/control_protocol.rs:295-430`。
- 当前 `ObserveRequest` 已经有 `mode`、`target`、`include_screenshot`、`include_ax`、`include_windows` 等字段,适合扩展 display scope。见 `src/control_observation/observe/request.rs:78-94`。
- 当前 `@observe` producer 会分别收集 windows、AX、screenshot 三类 section。见 `src/control_observation/observe/producer.rs:25-82`。
- 当前 mouse target resolver 已能把 observation ref 解析为 `os-logical` 点,并在响应中返回 `target_resolution`。见 `src/control_mouse/target.rs:191-219`。

## Design Principles

1. display scope 是结构化 selector,不是 observation 内 UI ref。
   `scope:{display:{id:"d2"}}` 是显示器选择器;`ref:"@e12"` 仍只表示某次 observation 内的 UI 元素引用。

2. `os-logical` 继续是唯一坐标语义。
   display scope 只负责过滤和防误操作,不引入第二套坐标系统。

3. observation 先收窄,action 再继承。
   agent 应先得到 displays summary 或使用 resolver,再发 scoped `@observe`。`@window-find`、`@ax-find`、`@web-find`、mouse guard 都应继承或显式使用同一个 display scope。

4. 兼容现有协议。
   裸 `@observe`、`@screenshot display:"all"`、现有 mouse 坐标 payload 都继续可用。新字段是可选增强。

5. 错误必须可恢复。
   display 不存在、display scope 无匹配窗口、坐标落到其他显示器或 gap 时,返回结构化错误,不要静默 fallback 到 primary display。

## Decision Drivers

- 降低多显示器 GUI 任务的歧义和 token 成本。
- 避免 `ref:"@d2"` 在 one-shot 命令和 LLM 生成命令时产生语义混淆。
- 保留结构化 resolver,让 agent 能用显示器名称、坐标点或窗口身份快速选屏。
- 复用现有 screenshot manifest、observe scope、mouse target resolver,避免新建并行控制面。

## Viable Options

### Option A: structured `scope.display` resolver as canonical request shape

示例:

```text
@observe#1:{mode:"hybrid",scope:{display:{id:"d2"}}}
@observe#2:{mode:"hybrid",scope:{display:{name_contains:"DELL"}}}
@click#3:{target:{ref:"@e12",observation_id:"obs-..."},guard:{display:{id:"d2"}}}
```

优点:

- 不和 `@eN` UI ref 命名空间冲突。
- shell one-shot 更稳,不会把 display 身份写成 `@...`。
- 结构化表达 resolver,能自然支持 `id`、`name_contains`、`contains_point`、`window_id`、`window_ref + observation_id`。
- resolve 后仍能统一返回 `display_id`,适合日志、selector、错误信息和长期恢复。

缺点:

- 需要定义 display id 稳定性规则。
- 需要定义 resolver 的歧义处理和错误契约。
- 需要在 manifest / observe response 中补充 id 生成与 resolved selector 记录。

### Option B: reuse `ref:"@d2"` for display

优点:

- 表面上和 UI ref 形态一致。
- agent 短期使用时看起来统一。

缺点:

- 和现有 `@eN` observation ref 语义混淆。
- CLI one-shot 规则中 `@...` 有特殊含义,即使 payload 内部安全,也容易误导 LLM 生成错误命令。
- display 比 UI ref 更稳定,不适合放进短期 ref 命名空间。

结论: 选 Option A。Option B 被拒绝,因为它把 surface selector 和 ephemeral UI ref 混成一类。

## Proposed Protocol Shape

### Display summary

`@bootstrap` / `@observe` / `@screenshot` manifest 中返回 displays summary:

```json
{
  "displays": [
    {
      "display_id": "d1",
      "stable_key": "macos:main:0",
      "primary": true,
      "name": "Built-in Retina Display",
      "os_rect": {"x":0,"y":0,"width":1728,"height":1117},
      "image_rect": {"x":0,"y":0,"width":1728,"height":1117},
      "scale_factor": 2.0,
      "rotation": 0
    }
  ]
}
```

`display_id` 第一版可以按当前 enumerated display order 生成 `d1`、`d2`。
响应必须声明 `display_id_stability:"session"`。
如果后续平台能提供稳定 display UUID,再提升为 `display_id_stability:"device"`,但不得破坏旧字段。

### Observe display scope

唯一请求形态是 `scope.display` 对象:

```text
@observe#1:{mode:"hybrid",scope:{display:{id:"d2"}}}
@observe#2:{mode:"hybrid",scope:{display:{name_contains:"DELL"}}}
@observe#3:{mode:"hybrid",scope:{display:{contains_point:{x:1800,y:500}}}}
@observe#4:{mode:"hybrid",scope:{display:{window_id:"win-123"}}}
@observe#5:{mode:"hybrid",scope:{display:{window_ref:"@e4",observation_id:"obs-..."}}}
```

解析后进入:

```rust
DisplaySelector::Id("d2")
DisplaySelector::NameContains("DELL")
DisplaySelector::ContainsPoint { x: 1800, y: 500 }
DisplaySelector::WindowId("win-123")
DisplaySelector::WindowRef { observation_id: "obs-...", ref_id: "@e4" }
```

第一版不接受顶层 `display_id:"d2"`。
原因是同一含义保留两种协议形态会让 agent、文档和测试都出现分叉。

### Query filters

显式 filter:

```text
@window-find#5:{title_contains:"Chrome",scope:{display:{id:"d2"}}}
@ax-find#6:{role:"AXButton",name_contains:"发布",scope:{display:{name_contains:"DELL"}}}
@web-find#7:{text_contains:"提交",scope:{display:{window_id:"win-123"}}}
@web-find#8:{text_contains:"提交",scope:{display:{window_ref:"@e4",observation_id:"obs-..."}}}
```

如果 query 来自 scoped observation 的后续动作,实现可以通过 target ref / observation header 继承 display scope。
如果没有 inheritance context,显式 `scope.display` 优先。

### Mouse guard

坐标 fallback:

```text
@click#9:{x:1900,y:640,coordinate_space:"os-logical",guard:{display:{id:"d2"}}}
```

ref target:

```text
@click#10:{target:{ref:"@e12",observation_id:"obs-..."},guard:{display:{id:"d2"}}}
```

执行前必须校验:

- resolved point 落在 resolver 得到的 `display_id` 对应 `os_rect` 内。
- point 不落在 manifest gaps。
- 如果 observation header 记录了 display scope,且 guard 解析出的 `display_id` 不一致,返回 `DISPLAY_SCOPE_MISMATCH`。

支持范围:

- `@mouse-move` 支持 `guard:{display:{...}}`。
- `@click` 支持 `guard:{display:{...}}`。
- `@drag` 支持 `guard:{display:{...}}`,并要求 from/to 两端都在同一个 resolved display 内;跨 display drag 另开后续能力。
- `@wheel` 支持 `guard:{display:{...}}`。
- `@mouse-button` 不支持 display guard,因为它没有坐标 target。

## Implementation Steps

### Step 1: 写长期规格

更新或新增:

- `specs/rdog-display-scope-control-plan.md`
- 在 `AGENTS.md` 长期知识索引中加入新规格入口。
- 视内容重叠程度,同步引用 `specs/rdog-multi-display-screenshot-coordinate-plan.md`、`specs/rdog-observation-scoped-refmap-plan.md`、`specs/rdog-mouse-control-coordinate-plan.md`。

规格必须明确:

- `scope:{display:{...}}` 是唯一 display scope 请求形态。
- `display_id` 是 resolver 成功后的显示器身份字段,不作为顶层请求字段。
- `ref:"@d2"` 不作为 display selector。
- 顶层 `display_id:"d2"` 不作为第一版兼容写法。
- `@eN` 只保留给 observation 内 UI ref。
- `display_id_stability` 第一版为 `session`。

### Step 2: 建模 display selector 和 display scope

必须新增共享 resolver 模块:

- `src/control_display_scope.rs`
- 新增类型:
  - `DisplayId`
  - `DisplaySelector`
  - `DisplayScope`
  - `DisplaySummary`
  - `DisplayScopeResolution`

硬约束:

- `@observe`、`@window-find`、`@ax-find`、`@web-find`、mouse guard 都必须调用同一个 resolver。
- 不允许各模块自行解析 `scope.display`。
- resolver 是 display scope 的唯一真相源,负责返回 resolved `display_id`、display rect、歧义状态和错误码。
- resolver 不执行 UI action,只做只读解析。

最小 selector:

```rust
pub enum DisplaySelector {
    Id(String),
    NameContains(String),
    ContainsPoint { x: i32, y: i32 },
    WindowId(String),
    WindowRef { observation_id: String, ref_id: String },
}
```

默认无 `scope.display` 时等价于不做 display 过滤,不是 `DisplaySelector::All`。
如果后续要支持 `primary` 或 `index`,必须单独扩展 resolver 并补歧义规则;第一版不做。

`window_id` 与 `window_ref` 边界:

- `window_id` 只接受当前 window catalog / backend 返回的 live window id。
- `window_ref` 是 observation 内短期窗口引用,必须与 `observation_id` 成对出现。
- 不允许 `window_ref` 缺少 `observation_id`。
- 非 window 类型 ref 解析为 display 时返回 `WINDOW_REF_INVALID`。
- stale observation ref 返回 `STALE_REF` / `OBSERVATION_EXPIRED`,不回退到窗口标题猜测。

输出统一包含:

- `display_id`
- `primary`
- `name`
- `os_rect`
- `image_rect`
- `scale_factor`
- `rotation`
- `display_id_stability`

### Step 3: 扩展 screenshot manifest displays

现有 manifest 已经有 `display_count` 和 `displays`。见 `specs/rdog-multi-display-screenshot-coordinate-plan.md:65-80`。
本步在每个 display object 上补:

- `display_id`
- `display_id_stability`
- `stable_key` 可选
- `name` 可选
- `primary` / `is_primary` 口径需要对齐。当前 e2e 使用 `is_primary` 读取 primary display,见 `tests/control_mouse_e2e.rs:182-190`。

迁移口径:

- `display_id` 是新 canonical 字段。
- 现有 manifest 的 `id` 保留为兼容 alias。
- 在第一版中 `id == display_id` 必须恒成立。
- 新文档和 skill 只指导 agent 读取 `display_id`。
- 旧测试仍可读 `id`,但新增测试必须断言 `id` 和 `display_id` 一致。
- `primary` 与现有 `is_primary` 第一版都保留,二者必须一致;后续再决定是否弃用其中之一。

验收重点:

- 旧字段不移除。
- 新字段在 all-display composite 和 primary single 两种模式下都一致。
- `display_count == displays.len()` 仍成立,现有测试参考 `tests/zenoh_router_client.rs:1865-1881`。

### Step 4: 扩展 `@observe` request 和 response

落点:

- `src/control_observation/observe/request.rs`
- `src/control_observation/observe/producer.rs`
- `src/control_observation/observe/response.rs`

`ObserveRequest` 当前字段见 `src/control_observation/observe/request.rs:78-94`。
新增:

```rust
pub display_scope: Option<DisplayScope>,
```

parser 支持:

```text
@observe:{scope:{display:{id:"d2"}}}
@observe:{scope:{display:{name_contains:"DELL"}}}
@observe:{scope:{display:{contains_point:{x:1800,y:500}}}}
@observe:{scope:{display:{window_id:"win-123"}}}
@observe:{scope:{display:{window_ref:"@e4",observation_id:"obs-..."}}}
```

response 在顶层增加:

```json
{
  "scope": {
    "display": {
      "selector": {"id":"d2"},
      "resolved": {"display_id":"d2","os_rect":{...}},
      "status": "applied"
    }
  },
  "displays": {...}
}
```

producer 过滤规则:

- screenshot: `@observe` 带 `scope.display` 且 `include_screenshot:true` 时,visual lane 必须返回目标 display 的 scoped image。
  - 实现上可以先捕获 all-display composite,再按 resolved display `image_rect` 裁出单 display image。
  - response 必须写 `visual.scope_applied:true`、`visual.resolved_display_id` 和 scoped image manifest。
  - 如果第一阶段暂时无法裁剪 scoped image,不能伪装为 scoped screenshot;必须返回 `visual.scope_applied:false` 和 `visual.scope_reason:"metadata_only"`,并且 acceptance 不能标记视觉 scope 已完成。
- windows: 过滤 window rect 与 display `os_rect` 相交的窗口。
- AX: 过滤 window rect 与 display `os_rect` 相交的 AX window / elements。
- refs: 只从过滤后的 sections 生成。

### Step 5: 扩展 window / AX / web query display filter

优先级:

1. `@window-find`
2. `@ax-find`
3. `@web-find`

原因:

- window 是 display scope 的自然边界。
- AX 可通过 window rect 过滤。
- WebArea 通常应先被 target window 限定,再进入网页内容搜索。

唯一字段形态:

```text
scope:{display:{id:"d2"}}
scope:{display:{name_contains:"DELL"}}
scope:{display:{contains_point:{x:1800,y:500}}}
scope:{display:{window_id:"win-123"}}
scope:{display:{window_ref:"@e4",observation_id:"obs-..."}}
```

响应需要包含:

```json
"display_scope": {
  "selector": {"id":"d2"},
  "resolved_display_id": "d2",
  "matched_before_filter": 12,
  "matched_after_filter": 3
}
```

### Step 6: 扩展 mouse guard

落点:

- `src/control_mouse/request.rs`
- `src/control_mouse/target.rs`
- mouse action response builder

请求结构:

```rust
pub struct MouseDisplayGuard {
    pub display: DisplaySelector,
}
```

挂载范围:

- `MouseMoveRequest` 增加 `guard: Option<MouseDisplayGuard>`。
- `ClickRequest` 增加 `guard: Option<MouseDisplayGuard>`。
- `DragRequest` 增加 `guard: Option<MouseDisplayGuard>`。
- `WheelRequest` 增加 `guard: Option<MouseDisplayGuard>`。
- `MouseButtonRequest` 不增加 guard。它只有 button down/up,没有坐标 target,不能判断 display 归属。

当前 mouse ref target 解析在 `src/control_mouse/target.rs:191-219`,已经会生成 resolved point 和 `target_resolution`。
新增 guard 后,在返回 `PreparedEndpoint::Point` 前执行:

- 解析 `guard.display` selector。
- 读取 observation header 中的 display scope,如果存在。
- 校验 resolved point 是否落在目标 display rect。
- 如果不匹配,返回结构化 no-action 或 error。

推荐错误:

```json
{
  "error_code": "DISPLAY_SCOPE_MISMATCH",
  "performed": false,
  "requested_display_id": "d2",
  "resolved_display_id": "d1",
  "point": {"x": 1200, "y": 540}
}
```

坐标 fallback 没有 observation header 时,需要从最近 manifest 或请求内 summary 中解析 display rect。
第一版如果没有可验证 display catalog / manifest,可以返回 `DISPLAY_GUARD_NEEDS_DISPLAY_CATALOG`。不要假装通过。

`@drag` 需要同时校验 from/to 两个 endpoint。第一版要求两端落在同一个 resolved display 内。
跨 display drag 单独作为后续能力处理,不要在第一版悄悄放行。

### Step 6.5: 扩展 `@bootstrap` nested observe display scope

`@bootstrap` 用来减少 agent 起手探测成本,但它不能再新增一套 display scope 真相源。
第一版只允许通过 nested observe 透传 display scope:

```text
@bootstrap#1:{mode:"gui",observe:{mode:"hybrid",scope:{display:{id:"d2"}}}}
```

硬约束:

- `@bootstrap` 顶层不新增 `scope`。
- `@bootstrap` 顶层不接受 `display_id`。
- display scope 只通过 nested `observe.scope.display` 传入。
- bootstrap response 复用 observe response 内的 `scope.display`、`displays` 和 resolved `display_id`。
- `@bootstrap:{scope:{display:{id:"d2"}}}` 与 `@bootstrap:{display_id:"d2"}` 都必须作为 negative test 拒绝。

### Step 7: 更新 skill 和 agent 工作流

更新:

- `.codex/skills/rdog-control/SKILL.md`
- `.codex/skills/rdog-control/references/protocol.md`
- `.codex/skills/rdog-control/references/control-workflow.md`
- `.codex/skills/rdog-control/references/cookbook-web-content.md` 如涉及浏览器窗口定位。

新默认流程:

1. `@bootstrap` 或 `@observe` 获取 displays summary。
2. 根据用户指定、窗口位置、window ref 或 `name_contains` 形成 `scope.display` selector。
3. 发 scoped `@observe:{scope:{display:{id:"d2"}}}` 或 `@observe:{scope:{display:{name_contains:"DELL"}}}`。
4. 优先 `@window-find` / `@ax-find` / `@web-find`。
5. 语义 action 优先。
6. mouse fallback 必须带 `guard:{display:{id:"d2"}}` 或同一个 display selector。
7. 用 scoped observe 验证。

### Step 8: 测试与验证

单元测试:

- `parse_observe_payload_should_accept_scope_display_id`
- `parse_observe_payload_should_accept_scope_display_name_contains`
- `parse_observe_payload_should_accept_scope_display_contains_point`
- `parse_observe_payload_should_accept_scope_display_window_id`
- `parse_observe_payload_should_accept_scope_display_window_ref_with_observation_id`
- `parse_observe_payload_should_reject_window_ref_without_observation_id`
- `parse_observe_payload_should_reject_top_level_display_id`
- `parse_observe_payload_should_reject_ref_at_display_selector`
- `display_selector_should_resolve_id_name_point_window_id_window_ref`
- `display_selector_should_reject_ambiguous_name_contains`
- `display_selector_should_reject_gap_contains_point`
- `window_find_should_filter_by_display_rect`
- `ax_find_should_filter_by_display_rect`
- `mouse_guard_should_accept_point_inside_display`
- `mouse_guard_should_reject_point_outside_display`
- `mouse_guard_should_reject_observation_scope_mismatch`
- `mouse_button_should_reject_display_guard`
- `bootstrap_should_forward_nested_observe_display_scope`
- `bootstrap_should_reject_top_level_display_scope`
- `bootstrap_should_reject_top_level_display_id`

集成 / e2e:

- 现有 screenshot manifest e2e 继续通过。
- 新增 ignored 多显示器 smoke:
  - 从 displays summary 找到 `primary == true` 的条目,再用 `@observe:{scope:{display:{id:"<primary display_id>"}}}` 返回 primary display scope。
  - 如果检测到 2+ displays,选择非 primary display 执行 read-only scoped observe。
  - mouse guard 使用安全 no-op 点验证 inside/outside 判定,不点击破坏性位置。

命令:

```bash
cargo fmt -- --check
cargo test --package rustdog --bin rdog -- control_observation::observe::tests --exact
cargo test --package rustdog --bin rdog -- control_mouse::target_tests --exact
cargo test --package rustdog --test zenoh_router_client -- control_should_return_screenshot_bundle_over_zenoh --exact
cargo test --package rustdog --test control_mouse_e2e -- --ignored
git diff --check
```

如果新增 Mermaid 图到 markdown,必须用:

```bash
beautiful-mermaid-rs --ascii < specs/rdog-display-scope-control-plan.md
```

或对抽出的 `.mmd` stdin 验证。不要把文件路径直接作为参数传给 `beautiful-mermaid-rs`。

## Acceptance Criteria

- `@observe:{scope:{display:{id:"d2"}}}` 能 parse,并解析到内部 `DisplaySelector::Id("d2")`。
- `@observe:{scope:{display:{name_contains:"DELL"}}}` 能 parse,并在 0/1/N 命中时分别返回 not_found / resolved / ambiguous。
- `@observe:{scope:{display:{contains_point:{x:1800,y:500}}}}` 能 parse,并按 `os-logical` 点解析 display。
- `@observe:{scope:{display:{window_id:"win-123"}}}` 能 parse,并按窗口 rect 最大 overlap display 解析。
- `@observe:{scope:{display:{window_ref:"@e4",observation_id:"obs-..."}}}` 能 parse,并必须验证 ref kind 为 window。
- `@observe:{scope:{display:{window_ref:"@e4"}}}` 明确失败,错误文案说明 `window_ref` 必须搭配 `observation_id`。
- 非 window ref 作为 display resolver 时返回 `WINDOW_REF_INVALID`,不回退到标题或坐标猜测。
- `@observe:{display_id:"d2"}` 明确失败,错误文案说明第一版只接受 `scope.display`。
- `@observe:{scope:{display:{ref:"@d2"}}}` 明确失败,错误文案说明 display selector 不使用 `ref`。
- `@bootstrap:{observe:{scope:{display:{id:"d2"}}}}` 支持,并把 nested observe scope 转发到 observe response。
- `@bootstrap:{scope:{display:{id:"d2"}}}` 明确失败。
- `@bootstrap:{display_id:"d2"}` 明确失败。
- `@bootstrap` / `@observe` response 至少包含 `display_id`、`display_id_stability`、`primary`、`os_rect`。
- all-display screenshot manifest 中每个 display 都有 `display_id`,同时保持旧字段兼容。
- 第一版 manifest 中 `id == display_id` 必须恒成立。
- 第一版 manifest 中 `primary == is_primary` 必须恒成立。
- scoped observe 生成的 refs 只来自目标 display 相交窗口 / AX 元素。
- scoped observe 带 `include_screenshot:true` 时,visual lane 必须返回 scoped display image;如果暂时只能 metadata filter,必须返回 `visual.scope_applied:false` 和 `visual.scope_reason:"metadata_only"`。
- `@window-find scope:{display:{id:"d2"}}` 返回的窗口全部与 `d2.os_rect` 相交。
- `@ax-find scope:{display:{id:"d2"}}` 返回元素全部位于 `d2.os_rect` 内或属于与 `d2` 相交窗口。
- `@click` / `@drag` / `@wheel` 的 guard 能阻止跨 display 执行。
- `@mouse-button` 带 `guard:{display:{...}}` 时明确失败,因为它没有坐标 target。
- 坐标落在 gap 时继续拒绝,不因 display guard 放宽。
- 单屏用户仍可裸 `@observe` / `@click` 使用旧流程。
- 多屏 e2e 在 2+ displays 时至少验证 primary 和 non-primary 两条 read-only scoped observe。

## Risks and Mitigations

### Risk 1: display_id 稳定性被误读为永久设备 ID

缓解:

- 第一版明确 `display_id_stability:"session"`。
- 持久 selector 只能记录 display hints,不能把 `d2` 当跨重启永久 ID。

### Risk 1.5: name_contains / contains_point / window_id resolver 产生歧义

缓解:

- `name_contains` 命中多个 display 时返回 `AMBIGUOUS_DISPLAY_SELECTOR`,不自动选择。
- `contains_point` 命中 gap 或无显示器时返回 `DISPLAY_NOT_FOUND`。
- `window_id` 对跨屏窗口按最大 overlap display 解析,并在响应中返回 `display_overlap_ratio`。

### Risk 1.6: window_ref 解析混入非窗口 ref 或过期 observation

缓解:

- `window_ref` 必须与 `observation_id` 成对出现。
- resolver 必须检查 ref kind,非 window ref 返回 `WINDOW_REF_INVALID`。
- stale ref 返回 `STALE_REF` / `OBSERVATION_EXPIRED`,不做窗口标题猜测。

### Risk 2: window / AX rect 与 display rect 边界相交导致元素归属歧义

缓解:

- 第一版采用 intersection 规则。
- 响应中记录 `display_overlap_ratio`。
- 如果窗口跨屏,优先按元素 rect 判断;元素无 rect 时按窗口最大 overlap display。

### Risk 3: mouse guard 需要最近 manifest,但执行时 manifest 不可用

缓解:

- ref target 优先从 observation header 继承 display scope。
- coordinate fallback 如果没有 display catalog / manifest 证据,返回 `DISPLAY_GUARD_NEEDS_DISPLAY_CATALOG`。
- skill 指引 agent 先 scoped observe 再 mouse fallback。

### Risk 4: 平台 backend 多屏坐标支持不一致

缓解:

- 继续以 `os-logical` 为协议真相源。
- 平台不支持负坐标或 virtual desktop 坐标时返回 Unsupported。
- Windows / Linux 多屏 smoke 分开 ignored 测试,不要把 macOS 成功推断到其他平台。

### Risk 5: bootstrap 又长出一套 display scope 入口

缓解:

- `@bootstrap` 只透传 nested `observe.scope.display`。
- 顶层 `scope` 和顶层 `display_id` 都写入 parser negative tests。
- response 不新造 bootstrap 专属 display scope 字段,复用 observe response。

## Verification Steps

1. 先跑 parser 和纯函数单测,确认 `scope.display` resolver 和错误拒绝都符合协议。
2. 跑 screenshot manifest 测试,确认旧字段和新增 display id 并存。
3. 跑 observe response 测试,确认 display scope 出现在 response,并影响 refs/windows/accessibility。
4. 跑 visual lane scoped screenshot 测试,确认 scoped image 真被裁剪,或明确返回 `metadata_only`。
5. 跑 bootstrap parser / response 测试,确认 nested observe display scope 能转发,顶层 `scope` / `display_id` 会被拒绝。
6. 跑 mouse target resolver 测试,确认 guard inside/outside/scope mismatch,并确认 `@mouse-button` 拒绝 guard。
7. 跑 Zenoh screenshot bundle e2e,确认远程 control path 中 `scope.display` payload 和 resolved `display_id` 字段不会被传输层破坏。
8. 在真实多显示器机器上跑 ignored smoke,验证 primary/non-primary scope 都能被观察,且不会误点。

## ADR

### Decision

采用 `scope:{display:{...}}` 作为多显示器控制请求侧的唯一 canonical 协议字段。
支持 `id`、`name_contains`、`contains_point`、`window_id`、`window_ref + observation_id` 五类第一版 resolver。
`display_id` 作为 resolver 成功后的显示器身份字段返回。
不采用 `ref:"@d2"` 表示 display。

### Drivers

- display 是显示器身份,不是 observation 内 UI ref。
- 避免和 one-shot `@...` 命令形状混淆。
- 支持 agent 快速按名称、坐标、live window id 或 observation window ref 选择显示器。
- 复用现有 `@observe` scope、screenshot manifest 和 mouse target resolver。
- `@bootstrap` 只复用 nested observe scope,不新增第二条 display scope 入口。

### Alternatives considered

- `ref:"@d2"`: 拒绝。它混淆 display identity 和 UI ref,也让 shell/LLM 命令生成更容易误读。
- 顶层 `display_id:"d2"`: 拒绝作为第一版兼容写法。它无法表达 `name_contains`、`contains_point`、`window_id`、`window_ref + observation_id` resolver,并会和结构化 scope 形成两个真相源。
- 只在 `@screenshot` 支持 `display:"primary"` / `display:"d2"`: 拒绝。它只能减少截图范围,不能约束 AX/window/web query 和 mouse action。
- 只在 mouse 命令加 display guard: 拒绝。它太晚,agent 仍然会在观察和定位阶段面对全桌面歧义。

### Why chosen

`scope.display` 能把多屏选择前置到 observation scope,再自然传递给 query 和 action。
它能用结构化字段表达不同 resolver,同时把最终身份统一收敛到 resolved `display_id`。

### Consequences

- 需要给 display summary 增加 ID 生成和稳定性说明。
- 需要给 display resolver 增加歧义和 not_found 错误契约。
- 需要在 observe、window、AX、web、mouse 多个模块传递 display scope。
- 需要在 bootstrap parser 中拒绝顶层 `scope` / `display_id`,并只转发 nested observe scope。
- 需要更新 skill,避免 agent 继续写 `ref:"@d2"`。

### Follow-ups

- 后续可以增加平台稳定 display UUID。
- 后续可以增加 `primary:true`、`index` 等 resolver,但第一版先不做,避免枚举顺序和 primary 语义混淆。
- 后续可以把 screenshot backend 优化成真正只捕获 scoped display,而不是先捕获 all-display 再过滤 response。
