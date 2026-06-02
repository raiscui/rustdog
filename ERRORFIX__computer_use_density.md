## [2026-05-29 17:42:55] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 错误修复: `@web-find` 深层 AXWebArea false negative

### 问题
- 真实小红书页面里左侧“首页”可见,且 `@ax-get` 钻入同一个 `AXWebArea` 能看到 `AXLink description:"首页"`。
- 但默认 `@web-find` / `@web-act` 在 active browser 上返回 `WEB_MATCH_NOT_FOUND`。
- 单纯提高全局 `depth/max_elements` 仍不是理想方案,因为它会让默认路径变重,并且从 window root 算 depth 容易浪费在非目标树上。

### 原因
- 初始 `@web-find` 快照从 window root 捕获,浅层 `AXWebArea` 能被发现,但深层 page-owned 导航链接未落入初始匹配树。
- 目标内容其实在同一个 `AXWebArea` 内,所以正确修复不是全局加深,而是只刷新这个 WebArea 子树。

### 修复
- 增加 `AxCapturedSubtree` 和 `capture_current_ax_subtree()`。
- macOS 后端新增 `capture_current_subtree(target_id, request)`,复用当前 target id 重新构建子树。
- `@web-find` 在初始匹配 0 时调用 targeted WebArea subtree refresh,并重新匹配。
- `@web-act` 复用同一 refresh 逻辑,避免新增并行页面搜索真相源。
- 增加回归测试覆盖“浅 WebArea 无匹配 -> refresh WebArea 子树 -> 找到深层首页”。

### 验证
- `cargo test --package rustdog --bin rdog -- control_web --quiet`: 11 个测试通过。
- `cargo test --package rustdog --bin rdog -- gui_bench --quiet`: 16 个测试通过。
- `cargo test --package rustdog --test computer_use_density --quiet`: 3 个测试通过。
- `@web-find#401` live 返回 `status:"complete",match_count:1`,trace 中 `refresh-web-area-subtree` 为 `ok`。

### 残留问题
- `@web-act verify:true` 在页面发生真实内容切换时仍可能超时。
- 当前 live 快速闭环建议用 `@web-find` 获取 id 后直接 `@ax-action`,并用前后截图 diff 证明瀑布流变化。
- bounded AX verification / live visual verifier 已记录到 `LATER_PLANS__computer_use_density.md`。

## [2026-06-01 18:03:20] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 错误修复: `@web-act` blocker 未覆盖新增 window ref invalid 分支

### 问题
- 给 `@web-find` 增加 `BrowserWindowRefInvalid` 后,`cargo test --package rustdog --bin rdog -- control_web --quiet` 编译失败。
- Rust 报错 `E0004: non-exhaustive patterns`,指出 `control_web::WebMatchResolution::BrowserWindowRefInvalid(_)` 没有被 `src/control_web/act.rs` 覆盖。

### 原因
- `@web-act` 复用 `resolve_web_matches()`。
- 新增的 read-only resolution blocker 不只影响 `@web-find`;所有消费 `WebMatchResolution` 的 action path 都必须同步处理。

### 修复
- 在 `web_act_resolution_blocker()` 增加 `BrowserWindowRefInvalid` 分支。
- 返回 `status:"blocked"`、`performed:false`、`verified:false`、`error_code:"WINDOW_REF_INVALID"`。

### 验证
- `cargo test --package rustdog --bin rdog -- control_web --quiet`: 17 passed。
- `cargo test --package rustdog --bin rdog --quiet`: 314 passed。
