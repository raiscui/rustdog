# 任务计划: AX screenshot manifest 与 AX control 能力规划

## [2026-05-14 15:12:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [计划]: 启用 AX planning 支线上下文

### 目标
生成一份可执行计划,覆盖 `@screenshot` manifest 增加 macOS AX 窗口/UI 元素信息,以及新增 AXPress 等 AX 控制命令.

### 阶段
- [x] 阶段1: 读取现有 screenshot manifest,control protocol,mouse control 实现入口
- [ ] 阶段2: 形成协议与架构方案
- [ ] 阶段3: 生成 `.omx/plans/rdog-ax-screenshot-manifest-control-plan.md`
- [ ] 阶段4: 验证 Mermaid 与 diff,交付计划摘要

### 关键决定
- AX 元数据只在 macOS 有实现;其他平台返回 `unsupported` 或省略 `accessibility` 字段.
- AX 坐标必须使用 screenshot manifest 已有 `os-logical` 坐标语义.
- AXPress 属于显式 AX 控制能力,不能混入鼠标点击语义.

### 状态
**目前在阶段2** - 正在把已读代码事实整理成执行计划.

## [2026-05-14 15:22:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: AX plan 已生成并完成基础验证

### 已完成阶段
- [x] 阶段2: 形成协议与架构方案
- [x] 阶段3: 生成 `.omx/plans/rdog-ax-screenshot-manifest-control-plan.md`
- [x] 阶段4: 验证 Mermaid 与 diff,交付计划摘要

### 验证
- `beautiful-mermaid-rs --ascii < /tmp/rdog-ax-plan-1.mmd`: 通过.
- `beautiful-mermaid-rs --ascii < /tmp/rdog-ax-plan-2.mmd`: 通过.
- `git diff --check`: 通过.

### 状态
**AX planning 已完成** - 本轮只生成计划,未进入源码实现.

## [2026-05-14 15:30:22] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Ralplan]: 启动 AX plan 共识审查

### 当前目标
- 对 `.omx/plans/rdog-ax-screenshot-manifest-control-plan.md` 做 `$ralplan` 共识规划.
- 输出加强后的 plan,PRD 和 test-spec,不进入源码实现.

### 当前步骤
- [x] 创建 `.omx/context/rdog-ax-screenshot-manifest-control-20260514T073022Z.md`.
- [ ] Planner 加强计划.
- [ ] Architect 审查.
- [ ] Critic 审查.
- [ ] 根据反馈修订计划并生成 PRD/test-spec.

### 状态
**目前在 Planner 阶段** - 正在加强计划并准备交给架构审查.

## [2026-05-14 15:39:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Ralplan]: Planner 草案完成

### 已完成
- 生成 `.omx/drafts/rdog-ax-ralplan-planner-draft.md`.
- 草案补充了 `src/control_core.rs` 与 `src/control_actions.rs` 的分发边界.
- 草案补充了 Apple AX API 的外部事实: attribute 读取,position/size,perform action.

### 状态
**目前在 Architect 阶段** - 下一步顺序执行架构审查.

## [2026-05-14 15:45:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Ralplan]: Architect 审查完成

### Verdict
- Architect: ITERATE.

### 必须修订
- Phase 1 收紧为 `include_ax`, `@ax-tree`, `@ax-press`.
- 增加 `ax_required` 字段,区分 AX metadata best-effort 与 hard requirement.
- `@ax-set-value` 暂列后续非目标.
- 明确 screenshot producer 与 executor 的分发边界.

### 状态
**目前在 Critic 阶段** - 下一步审查测试性和验收标准.

## [2026-05-14 15:49:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Ralplan]: Critic 审查完成

### Verdict
- Critic: ITERATE.

### 必须修订
- 增加 `ax_required:false` 默认语义.
- `include_ax:true, ax_required:false` 权限失败时保留 screenshot bundle,manifest 标记 `permission_denied`.
- `include_ax:true, ax_required:true` 权限失败时返回 code 77.
- Phase 1 只做 `include_ax`, `@ax-tree`, `@ax-press`.
- `@ax-tree` Phase 1 返回 structured `@response` summary,大 payload savefile 后续再做.

### 状态
**目前在 Planner 修订阶段** - 下一步生成 consensus plan,PRD,test-spec.

## [2026-05-14 15:56:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: AX ralplan 共识计划完成

### 已完成
- 生成 `.omx/plans/rdog-ax-screenshot-manifest-control-consensus-plan.md`.
- 生成 `.omx/plans/prd-rdog-ax-screenshot-manifest-control.md`.
- 生成 `.omx/plans/test-spec-rdog-ax-screenshot-manifest-control.md`.
- 吸收 Architect/Critic 修订: `ax_required`,权限降级,Phase 1 范围收紧,`@ax-tree` structured response.

### 验证
- `beautiful-mermaid-rs --ascii < /tmp/rdog-ax-consensus-1.mmd`: 通过.
- `beautiful-mermaid-rs --ascii < /tmp/rdog-ax-consensus-2.mmd`: 通过.
- `git diff --check`: 通过.
- `rg` 检查确认 `ax_required`, `permission_denied`, `@ax-tree`, `@ax-press`, Phase 1 non-goals 已落入 plan/PRD/test-spec.

### 状态
**Ralplan 已完成** - 本轮只规划,未实现代码.

## [2026-05-14 16:03:30] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [收口]: 清理 ralplan active 状态

### 现象
- Stop hook 提示 ralplan 仍 active,phase 仍为 planning.
- 实际 artifact 已存在: consensus plan,PRD,test-spec 均已生成.

### 处理
- 执行 `omx state clear --input '{"mode":"ralplan"}' --json`.
- 同步将全局和 session `skill-active-state.json` 中的 ralplan 标记为 inactive/complete.

### 验证
- `omx state list-active --json`: 返回 `active_modes: []`.
- `omx state get-status --input '{"mode":"ralplan"}' --json`: 返回空 statuses.

### 状态
**ralplan 运行态已完成** - 当前没有 active OMX workflow.

## [2026-05-14 16:34:28] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Ralph]: 启动 AX 长期规格迁移与 Phase 1 实现

### 当前目标
- 先把 `.omx/plans/rdog-ax-screenshot-manifest-control-consensus-plan.md` 迁入 repo 长期规格 `specs/rdog-ax-screenshot-manifest-control-plan.md`.
- 同步更新 `AGENTS.md` 长期知识索引,让后续协议,manifest schema,权限语义和 AX control 修改都有稳定入口.
- 然后按 consensus plan 执行 Phase 1: `include_ax`, `ax_required`, `@ax-tree`, `@ax-press`.

### 阶段
- [x] 阶段1: 迁移 specs 长期计划并更新 `AGENTS.md`
- [x] 阶段2: 读取协议,截图,控制执行现状并实现 parser/model/backend
- [x] 阶段3: 集成 screenshot manifest 与 AX control dispatch
- [ ] 阶段4: 补测试,同步相关文档和 skill
- [ ] 阶段5: 运行验证,架构复核,Ralph 收口

### 约束
- 不触碰当前未提交的鼠标 E2E 支线文件,除非 AX 实现必须引用.
- `include_ax:false` 必须保持现有 screenshot 行为,且不能调用 AX provider.
- `include_ax:true,ax_required:false` 权限失败必须降级写入 manifest.
- `include_ax:true,ax_required:true` 权限失败必须返回 code 77.

### 状态
**目前在阶段2** - 长期规格和索引已落地,下一步读取现有协议/截图/control 执行路径并实现 Phase 1.

### 阶段1验证
- 新增 `specs/rdog-ax-screenshot-manifest-control-plan.md`.
- `AGENTS.md` 已新增该规格的长期知识索引.
- `beautiful-mermaid-rs --ascii < /tmp/rdog-ax-spec-1.mmd`: 通过.
- `beautiful-mermaid-rs --ascii < /tmp/rdog-ax-spec-2.mmd`: 通过.
- `git diff --check -- AGENTS.md specs/rdog-ax-screenshot-manifest-control-plan.md`: 通过.

## [2026-05-14 16:49:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [行动]: 开始 AX Phase 1 代码实现

### 已确认代码入口
- `src/control_protocol.rs` 负责 line-control parser,当前 `@screenshot` 对未知字段严格报错.
- `src/screenshot.rs` 负责 composite image + manifest bundle,manifest 当前没有 AX 字段.
- `src/control_core.rs` 让 `ControlCommand::Screenshot` 直接走 screenshot producer,其他动作走 executor.
- `src/control_actions.rs` 已支持 structured `response_value_json`,适合承载 `@ax-tree` / `@ax-press`.
- `src/control_mouse.rs` 的 parser/plan/report 分层可作为 AX 模块风格参考.

### 当前行动
- 新增 `src/control_ax.rs`,先定义协议数据结构,parser,AX snapshot/report 和 fake-test 友好的 backend 边界.
- 回接 `ControlCommand::AxTree` / `ControlCommand::AxPress`.
- 后续再把 screenshot manifest 可选挂载 `accessibility`.

### 状态
**阶段2进行中** - 正在新增 AX 模块和协议解析.

## [2026-05-14 17:08:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [进展]: AX parser,model,screenshot 集成切片通过

### 已完成
- 新增 `src/control_ax.rs`,包含 AX 数据模型,parser,默认 backend,macOS FFI 和单测.
- `src/control_protocol.rs` 已支持 `@screenshot` AX 字段,`@ax-tree`,`@ax-press`.
- `src/screenshot.rs` 已支持可选 `accessibility` manifest,并按 `ax_required` 处理权限降级或失败.
- `src/control_actions.rs` 已分发 `@ax-tree` / `@ax-press`.
- 修复 `parse_quoted_payload` 对非 ASCII / 中文字符串按 byte 解析导致乱码的问题.

### 已验证
- `cargo test --package rustdog --bin rdog -- control_protocol::tests::parse_should_support_screenshot_ax_fields --exact`: 通过.
- `cargo test --package rustdog --bin rdog -- control_protocol::tests::parse_should_support_ax_tree_and_ax_press --exact`: 通过.
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`: 5 passed.
- `cargo test --package rustdog --bin rdog -- screenshot::tests --nocapture`: 16 passed.
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`: 9 passed.

### 状态
**目前在阶段4** - 代码主路径已接通,正在补明确 dispatch 测试和文档/skill 同步.

## [2026-05-14 17:37:26] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [行动]: 继续 AX Ralph 验证与收口

### 当前目标
- 从阶段4/阶段5继续,不重做已经完成的 specs 迁移和主路径实现.
- 补真实 daemon/control smoke,证明 `@ax-tree` 不是只在 unit test 里能解析,而是能经由 line-control 到达 daemon 执行层.
- 完成 Ralph 要求的复核,deslop,回归验证和状态收口.

### 上一轮验证口径更正
- 曾尝试使用 `rdog listen 127.0.0.1 0 --mode control` 做 smoke.
- 该入口不成立,因为当前 CLI 的 `listen` 没有 `--mode` 参数.
- 正确 smoke 路径应复用现有测试中的 daemon inbound control lane: 启动 `rdog daemon`,通过 `RDOG_INBOUND__MODE=control` 开启 line-control,再用 `rdog control 127.0.0.1 PORT` 发送 `@ax-tree`.

### 待办
- [ ] 读取当前 diff 和关键实现文件,确认没有漏接分发或文档.
- [ ] 启动本地 daemon/control smoke 验证 `@ax-tree`.
- [ ] 运行完整目标验证命令.
- [ ] 做架构复核和 Ralph deslop pass.
- [ ] post-deslop 回归验证.
- [ ] 更新 `WORKLOG__ax_plan.md` 并清理 Ralph 状态.

### 状态
**目前在阶段5** - 准备执行真实 line-control smoke 和最终验证.

## [2026-05-14 17:39:30] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [验证]: 本地 daemon/control AX smoke 完成

### 验证命令
- 先执行 `cargo build --package rustdog --bin rdog`,确保 `target/debug/rdog` 来自当前源码.
- 用临时端口启动 `target/debug/rdog daemon`,环境变量包括 `RDOG_INBOUND__MODE=control`.
- 再通过 `target/debug/rdog control 127.0.0.1 PORT` 发送 `@ax-tree#1:{scope:"windows",depth:1,max_elements:20,include_values:false}`.

### 关键输出
- `CONTROL_STATUS=0`
- `@response {"id":1,"code":77,"error":"macOS Accessibility 权限不足: 请给实际执行 rdog daemon/control 的进程授予辅助功能权限,授权后重启该进程"}`

### 结论
- 这次 smoke 证明 `@ax-tree` 已经通过真实 daemon inbound control lane 到达 AX backend.
- 当前机器该进程缺少 Accessibility 权限,所以返回 code 77 是预期权限边界.
- 这个结果不证明 AX tree 完整采集成功,但证明 line-control 分发,错误映射和权限语义已贯通.

### 状态
**阶段5进行中** - 下一步运行完整目标验证矩阵.

## [2026-05-14 17:42:30] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [验证]: AX Phase 1 完整目标验证通过

### 已运行验证
- `cargo fmt -- --check`: 通过.
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`: 13 passed.
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`: 5 passed.
- `cargo test --package rustdog --bin rdog -- screenshot::tests --nocapture`: 16 passed.
- `cargo test --package rustdog --bin rdog -- control_actions::tests --nocapture`: 14 passed.
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`: 10 passed.
- `cargo test --tests --no-run`: 通过,集成测试目标均可编译.
- `cargo build --package rustdog --bin rdog`: 通过.
- `git diff --check`: 通过.
- `beautiful-mermaid-rs --ascii` 验证 `specs/rdog-ax-screenshot-manifest-control-plan.md` 两个 Mermaid 图: 通过.
- `beautiful-mermaid-rs --ascii` 验证 `specs/rdog-multi-display-screenshot-coordinate-plan.md` 两个 Mermaid 图: 通过.

### 关键动态证据
- screenshot real smoke 命中 Screen Recording 权限边界,输出 `real screenshot smoke hit permission boundary: macOS Screen Recording permission denied for rdog process`,测试按预期通过.
- AX daemon/control smoke 命中 Accessibility 权限边界,返回 code 77,证明 `@ax-tree` 真实执行路径贯通.

### 状态
**阶段5进行中** - 进入架构复核和 deslop pass.

## [2026-05-14 17:48:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Deslop]: 拆分 AX macOS 后端

### 发现
- `src/control_ax.rs` 新增后达到 1582 行,超过项目规则中静态语言文件尽量不超过 1000 行的边界.
- 这不是运行时功能 bug,但属于可维护性坏味道: 协议模型,parser,公共 backend trait 和 macOS FFI 全挤在一个文件中.

### 处理
- 保留 `src/control_ax.rs` 作为 AX 协议模型,parser,公共入口和测试.
- 新增 `src/control_ax/macos.rs`,承载 macOS Accessibility FFI backend.
- `src/control_ax.rs` 中仅保留 `#[cfg(target_os = "macos")] mod macos;`.

### 当前验证
- 首次 `cargo fmt -- --check` 指出新拆出文件需要格式化.
- 已执行 `cargo fmt`,下一步重新运行 post-deslop 回归验证.

### 状态
**阶段5进行中** - 正在做 post-deslop 回归验证.

## [2026-05-14 17:54:34] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: AX Ralph 实现与验证收口

### 最终待办状态
- [x] 读取当前 diff 和关键实现文件,确认没有漏接分发或文档.
- [x] 启动本地 daemon/control smoke 验证 `@ax-tree`.
- [x] 运行完整目标验证命令.
- [x] 做架构复核和 Ralph deslop pass.
- [x] post-deslop 回归验证.
- [x] 更新 `WORKLOG__ax_plan.md` 并清理 Ralph 状态.

### 架构复核结论
- `@screenshot include_ax` 没有改变截图坐标真相源,AX rect 明确复用 `coordinate_space:"os-logical"`.
- `@ax-tree` 和 `@ax-press` 走 typed command,没有引入 `@AXPress:"b"` 这种 AppleScript 局部变量式协议.
- PermissionDenied 继续映射到 code 77,Unsupported 继续映射到 code 78.
- `include_ax:false` 的测试证明不会调用 AX provider,兼容默认截图路径.
- `include_ax:true,ax_required:false` 和 `ax_required:true` 的权限语义都有测试覆盖.

### Post-deslop 验证
- `cargo fmt -- --check`: 通过.
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`: 13 passed.
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`: 5 passed.
- `cargo test --package rustdog --bin rdog -- screenshot::tests --nocapture`: 16 passed.
- `cargo test --package rustdog --bin rdog -- control_actions::tests --nocapture`: 14 passed.
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`: 10 passed.
- `cargo test --tests --no-run`: 通过.
- `cargo build --package rustdog --bin rdog`: 通过.
- `git diff --check`: 通过.
- post-deslop daemon/control smoke: `@ax-tree#2` 返回 code 77,证明拆分后真实 line-control 权限边界仍贯通.

### 状态
**AX Ralph 已完成** - 下一步仅需清理 `.omx` Ralph active 状态并交付摘要.

## [2026-05-15 10:53:40] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [行动]: 接续 review diff 并准备本地提交

### 当前目标
- 对 AX Phase 1 已落地 diff 做提交前审查,确认没有混入 mouse E2E 支线文件.
- 重新运行提交前验证命令,用本轮 fresh output 支撑 commit.
- 仅 stage AX 相关实现,规格,文档和本支线工作记录,执行 local commit,不 push.
- 提交后回答 Phase 2 是否仍然存在,以及推荐的下一步顺序.

### 待办
- [ ] 审查 `git diff --stat` / 关键文件 diff / staged 范围.
- [ ] 运行 fresh 验证: fmt, focused AX tests, integration compile, build, diff check.
- [ ] stage AX 相关文件并复核 staged diff.
- [ ] 创建 Lore protocol local commit.
- [ ] 复核提交后工作树,说明 Phase 2 边界.

### 状态
**目前在提交前审查** - 先确认 diff 范围和是否存在阻断问题.

## [2026-05-15 11:00:20] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Review]: 提交前协议边界收紧

### 审查发现
- `@ax-press` 的 macOS target id parser 对 `pid:.../window:.../path:.../extra` 这类多余路径段过于宽松,可能静默忽略尾部内容.
- 这不是 Phase 1 主路径阻断,但属于控制协议解析边界,提交前应收紧.

### 已处理
- `src/control_ax/macos.rs` 的 target id parser 改为只允许 `pid/window` 或 `pid/window/path` 两种形态.
- 空 `path:`、尾随点、多余段、非数字 path step 都返回 `InvalidInput`.
- 补 `parse_target_id_should_reject_malformed_paths` 单测.

### 下一步
- 运行 fresh 验证: fmt, focused AX tests, tests no-run, build, diff check.

### 状态
**进入验证阶段** - 先用自动化输出确认提交前状态.

## [2026-05-15 11:08:40] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [验证]: 提交前 fresh 验证通过

### 已运行验证
- `cargo fmt -- --check`: 通过.
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`: 13 passed.
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`: 5 passed.
- `cargo test --package rustdog --bin rdog -- control_ax:: --nocapture`: 6 passed,包含 macOS target id parser 边界测试.
- `cargo test --package rustdog --bin rdog -- screenshot::tests --nocapture`: 16 passed.
- `cargo test --package rustdog --bin rdog -- control_actions::tests --nocapture`: 14 passed.
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`: 10 passed.
- `cargo test --package rustdog --bin rdog -- shell::tests --nocapture`: 9 passed.
- `cargo test --tests --no-run`: 通过,所有 integration test target 编译完成.
- `cargo build --package rustdog --bin rdog`: 通过.
- `git diff --check`: 通过.

### 当前结论
- Phase 1 代码和文档可以进入 local commit.
- 当前仍有 mouse E2E 支线未提交文件,本轮 stage 时必须排除.

### 状态
**验证完成** - 下一步 stage AX 相关文件并复核 staged diff.

## [2026-05-15 11:13:10] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [验证补充]: Mermaid 校验命令修正

### 现象
- 第一次执行 `sed -n '206,224p' specs/rdog-ax-screenshot-manifest-control-plan.md | beautiful-mermaid-rs --ascii` 时,把 fenced code block 的 ```mermaid 也传给了 CLI.
- `beautiful-mermaid-rs` 返回 `Invalid mermaid header: "```mermaid"`,这是工具输入格式错误,不是文档 Mermaid 内容错误.

### 修正
- 改用 `awk` 只抽取 Mermaid fenced block 内部内容再传给 `beautiful-mermaid-rs --ascii`.
- `specs/rdog-ax-screenshot-manifest-control-plan.md` 的两个 Mermaid 图校验通过.
- `specs/rdog-multi-display-screenshot-coordinate-plan.md` 的两个 Mermaid 图校验通过.

### 状态
**文档校验完成** - 进入 stage 和本地 commit.
