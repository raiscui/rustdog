## [2026-08-20 18:30:00] [Session ID: current] 任务完成: control_ax 拆分方案最终确定

### 任务内容
- 通过架构审查识别 rustdog 代码库的架构摩擦点
- 深入 grilling 拆分 control_ax.rs 方案
- 生成完整的实施计划和领域模型更新

### 完成过程

#### 1. 架构探索（探索智能体）
- 分析 git 历史找出热点区域（ax, observation, runner, control）
- 探索 control_ax.rs 依赖关系和调用模式
- 统计实际函数分布（53 个公开函数，4298 行代码）
- 发现循环依赖：control_ax ↔ control_observation（双向调用）
- 识别两套 cache（ImplicitObserve 5s vs Progressive 300s）

**关键发现**：
- control_actions.rs 是最重调用方（11+ 函数）
- AxObservationCache 与 ObservationStore 职责重叠
- 测试覆盖率低（perform/parse 核心逻辑依赖集成测试）

#### 2. Grilling 三轮质询（21 个问题）

**第一轮（Q1-Q7）**：挑战拆分假设
- Q1: 循环依赖如何打破？→ 混合模式（同步调用 + 接口边界）
- Q2: Cache 归属？→ 移到 control_observation
- Q3: parse vs perform 分离？→ 模块内分层（protocol + execute）
- Q4: ax_input 是否太小？→ 独立，但需高层接口
- Q5: 子文件去向？→ macos.rs 独立成 ax_platform（暂时 TODO）
- Q6: 测试策略？→ 测试驱动拆分（关键路径优先）
- Q7: 调用方影响？→ 新路径 + deprecated facade

**第二轮（Q8-Q14）**：接口设计细节
- Q8: ObservationCapture adapter（长期 seam，满足 ADR-0005）
- Q9: AxSnapshotCache 独立 struct，由 ObservationStore 持有
- Q10: execute_ax_action 统一入口 + 数据化 routing 表
- Q11: type_text 分层 API（简单版 + 高级版）
- Q12: 暂不定义 PlatformAx trait（YAGNI）
- Q13: 测试优先级：cache 验证 → parse → perform
- Q14: Facade 只为内部保留，标记 deprecated

**第三轮（Q15-Q21）**：实施验证
- Q15: 增量式实施（ax_input → ax_action → ax_query）
- Q16: ObservationCapture 是长期 seam（写入 CONTEXT.md）
- Q17: Cache 支持多种 TTL policy（per-entry）
- Q18: Routing 表数据化（const ROUTES）
- Q19: type_text 分层 API（80/20 原则）
- Q20: 增量迁移（每个模块拆完立即迁移调用方）
- Q21: 暂时接受 ax_action 70KB（标记 TODO）

#### 3. ADR 冲突检查（验证智能体）
- **冲突 1**：Q1 完全反转依赖 vs ADR-0005（implicit_observe 同步链路）
  - **修正**：混合模式，保留同步调用
- **冲突 2**：Q7 facade vs ADR-0006（@flow 嵌入需要完整字段）
  - **修正**：facade 必须完整代理，或内部调用绕过 facade

#### 4. 产出文件

**ADR-0008**: docs/adr/0008-control-ax-module-split.md
- 完整记录拆分决策
- 三个模块的职责和接口设计
- 实施阶段和验收标准
- ADR-0005/0006 兼容性说明

**实施计划**: specs/control-ax-split-implementation-plan.md
- 三阶段详细步骤（ax_input → ax_action → ax_query）
- 每个阶段的代码示例和测试策略
- 风险缓解措施
- 时间线（预计 2-3 周）

**CONTEXT.md 更新**:
- 新增 **Observation Capture** 定义（长期 seam）
- 新增 **AX Snapshot Cache** 定义（加速层，支持多 policy）

**架构审查报告**: /var/folders/.../architecture-review-20260820.html
- 可视化展示 4 个摩擦点
- Before/After 对比图
- 优先级推荐

### 总结感悟

#### 关键收获

1. **Grilling 方法论的威力**
   - 21 个问题形成完整决策树
   - 每个决策都基于前置条件（frontier 概念）
   - 三轮质询：假设挑战 → 接口设计 → 实施验证

2. **领域模型同步的重要性**
   - ObservationCapture 从"临时 adapter"升级为"长期 seam"
   - 在 CONTEXT.md 中明确定义，成为领域概念
   - 避免"只有代码，没有语言"的困境

3. **ADR 作为设计护栏**
   - 两处冲突都是真实的架构风险
   - ADR-0005（implicit_observe 5s TTL）阻止了过度抽象
   - ADR-0006（@flow 字段完整性）保证了兼容性

4. **增量式实施降低风险**
   - 每个模块独立 PR，blast radius 小
   - ax_input 作为"拆分模板"验证流程
   - 最复杂的 ax_query + Cache 放最后（此时已有经验）

5. **数据化 routing 表的价值**
   - 直接解决架构审查中的"摩擦点 #4"
   - 13 个 action 一眼可见
   - 新增 action = 修改数据表，而非写新函数

#### 值得复用的规律

- **Deletion Test**：判断模块深度的最佳方法（删除后复杂度去哪了？）
- **80/20 API 分层**：简单版覆盖常见场景，高级版暴露完整控制
- **YAGNI 原则**：PlatformAx trait 延迟到真正需要多平台时
- **测试驱动拆分**：关键路径优先（cache 验证），易测其次（parse），难测最后（perform）
- **Facade 退出策略**：只为内部保留，标记 deprecated，明确删除时间线

#### 后续执行建议

1. **先执行 Phase 1（ax_input）**
   - 最小模块，验证流程
   - 1-2 天完成，快速获得反馈

2. **Phase 1 完成后 code review**
   - 验证增量拆分流程是否顺畅
   - 调整 Phase 2/3 计划（如有必要）

3. **关键路径测试不可跳过**
   - cache 验证逻辑是最容易出错的部分
   - 必须在 Phase 3 开始前完成

4. **每个 PR 都附带验收清单**
   - 功能测试通过
   - 架构验收通过
   - 文档同步更新

---

**下一步行动**：
- [ ] 将 specs/control-ax-split-implementation-plan.md 添加到 task_plan.md
- [ ] 开始 Phase 1 实施（创建 ax_input/ 目录）
- [ ] 或者：等待用户反馈，调整方案细节

## [2026-08-21 13:30:00] [Session ID: current] Ticket #02: 迁移 control_actions 到 ax_input API

### 任务内容
将 control_actions.rs 中的输入操作迁移到新的 ax_input 高级 API，并标记旧函数为 deprecated。

### 完成过程

#### 1. 迁移 control_actions.rs
- 修改 `execute_type_text()`: 
  - 从 `perform_default_type_text()` 迁移到 `ax_input::type_text_with_config()`
  - 保持功能完全一致
- 修改 `execute_key()`:
  - 从 `perform_default_key_delivery()` 迁移到 `ax_input::send_key_with_config()`
  - 保持功能完全一致

#### 2. 标记旧函数 deprecated
- 在 `control_ax/input.rs` 中标记：
  - `perform_default_type_text` → `#[deprecated(since = "0.9.0")]`
  - `perform_default_key_delivery` → `#[deprecated(since = "0.9.0")]`
- Deprecated message 指向新 API

#### 3. 实现 Facade 模式
- Deprecated 函数完整代理到新 API：
  - `perform_default_type_text()` → `ax_input::type_text_with_config()`
  - `perform_default_key_delivery()` → `ax_input::send_key_with_config()`
- 确保向后兼容

#### 4. 修复循环依赖
- `ax_input` 模块直接调用底层实现：
  - `type_text_with_config()` → `SystemAxBackend.type_text()`
  - `send_key_with_config()` → `platform_key_delivery()`
- 避免 ax_input → control_ax/input → ax_input 的循环

#### 5. 测试验证
- 运行 ax_input 专项测试：16 passed, 0 failed
- 运行全量测试：883 passed, 2 failed (预先存在)
- 验证 deprecated warnings 正常显示

### 总结感悟

#### 架构设计
1. **Facade 模式的价值**：旧函数完整代理到新 API，既保持兼容又引导迁移
2. **循环依赖处理**：通过直接调用底层实现打破循环，清晰的依赖层次
3. **渐进式迁移**：标记 deprecated 而非直接删除，给调用方迁移时间

#### 测试策略
1. **专项测试先行**：先验证核心模块测试通过
2. **全量测试覆盖**：确保无回归
3. **Deprecated 验证**：确认警告机制正常工作

#### 实施经验
1. **阅读代码是关键**：先理解调用关系再动手，避免破坏功能
2. **测试驱动迁移**：每个修改后立即测试，快速发现问题
3. **编译器是最好的向导**：利用编译错误找到所有需要修改的位置

### 后续建议
1. **继续迁移 Ticket #03**：处理 web_rpc 和 integration tests 中的调用
2. **监控 deprecated 使用**：定期检查是否还有代码使用旧 API
3. **准备 Ticket #11**：在所有迁移完成后进行最终清理和文档更新


## [2026-08-21 16:00:00] [Session ID: current] Ticket #03: 完成 ax_action 模块拆分

### 任务内容
将 control_ax.rs 中的 press action 相关逻辑拆分到新的 ax_action 模块。

### 完成过程

#### 1. 创建模块结构
- 创建 `src/ax_action/` 目录
- 创建三个核心文件：
  - `mod.rs`: 模块导出和路由表框架
  - `protocol.rs`: press action 的 payload 解析
  - `execute.rs`: press action 的执行逻辑

#### 2. 协议层实现 (protocol.rs)
- 实现 `parse_press()` 函数：
  - 支持 JSON 格式：`{"target": {"id": "..."}, "postcondition": {...}}`
  - 支持 Compact 格式：`app:APP,description:删除`
  - 复用 `control_ax::parse_ax_press_payload()` 的解析逻辑
- 清晰的职责边界：只做数据转换，不含业务逻辑校验

#### 3. 执行层实现 (execute.rs)
- 实现两个公开函数：
  - `press()`: 统一入口，根据是否有 postcondition 路由到不同逻辑
  - `press_with_postcondition()`: 完整 postcondition 验证流程
- 内部实现：
  - 复用 `control_ax::legacy_press_with_postcondition()` 的核心逻辑
  - 类型转换：`AxPressPostconditionReport` → `AxActionReport`
- 错误处理：postcondition 验证失败时返回明确错误

#### 4. 路由层框架 (mod.rs)
- 定义 `ActionRoute` 结构：action 名称 → parser/executor 的映射
- 定义 `ACTION_ROUTES` 常量表：数据化 routing（当前只有 "press"）
- 实现 `execute_ax_action()` 统一入口：
  - 根据 action 名称查找 route
  - 调用对应的 parser/executor
  - 标准化返回类型
- 标记 `#[allow(dead_code)]`：Ticket #03 启用后使用

#### 5. 类型系统完善
- 在 `control_ax/types.rs` 中为 `AxTarget` 添加：
  - `#[derive(Serialize, Deserialize)]`
  - `#[serde(rename_all = "camelCase")]`
- 修复 `ref_id` 字段的 serde 映射：
  - 添加 `#[serde(rename = "ref")]` 注解
  - 确保 JSON 中的 `"ref"` 字段正确映射到 Rust 的 `ref_id`

#### 6. 测试验证
- 在 `execute.rs` 中添加单元测试：
  - `test_press_basic()`: 基础 press 功能
  - `test_press_with_postcondition_success()`: postcondition 成功验证
  - `test_press_with_postcondition_fail()`: postcondition 验证失败
- 运行专项测试：3 passed
- 运行全量测试：892 passed, 2 failed (预先存在)
- 编译验证：无警告（deprecated 函数已标记 `#[allow(dead_code)]`）

#### 7. 向后兼容处理
- 保留 `control_ax::legacy_press_with_postcondition()` 用于复用
- 标记 `#[allow(dead_code)]`：等 Ticket #03 启用后自然使用

### 总结感悟

#### 架构设计
1. **分层清晰**：protocol (解析) → execute (逻辑) → mod (路由)，职责边界明确
2. **数据化 routing**：`ACTION_ROUTES` 常量表，新增 action 只需修改数据
3. **类型安全的桥接**：通过 AxActionReport 统一返回类型，消除类型膨胀
4. **复用而非重写**：复用 `legacy_press_with_postcondition()` 核心逻辑，避免重复

#### 实施经验
1. **serde 注解是关键**：`ref_id` 映射问题提醒我们 serde 注解必须完整
2. **测试先行**：单元测试覆盖关键路径，快速发现类型映射问题
3. **增量验证**：每个文件创建后立即编译，快速定位问题
4. **允许 dead_code**：新模块未启用前标记 `#[allow(dead_code)]`，避免无意义警告

#### 值得复用的规律
- **Facade 模式**：旧代码通过 deprecated 函数代理到新 API，平滑迁移
- **数据化配置**：routing 表、parser/executor 映射都数据化，易于扩展
- **类型转换策略**：定义统一返回类型，在边界处做类型转换
- **测试策略**：关键路径优先（postcondition 验证），边界情况其次

### 后续建议
1. **继续 Ticket #04-#10**：将其他 12 个 action 逐个迁移到 ax_action
2. **启用 routing 表**：在 web_rpc 或 control_computer_act 中调用 `execute_ax_action()`
3. **监控 deprecated 使用**：确保所有调用方已迁移
4. **最终清理 (Ticket #11)**：删除 deprecated 函数和 legacy 实现

---

**下一步行动**：
- [ ] 开始 Ticket #04：迁移 click action
- [ ] 或者：继续其他优先级更高的任务


## [2026-08-21 16:40:00] [Session ID: current] Ticket #04: 迁移通用 AX action 到 ax_action

### 任务内容
把 `perform_default_ax_action` 从 control_ax 迁到 ax_action，并接入 routing 表。

### 完成过程

#### 1. protocol 层
- 新增 `parse_action()`，两种输入：
  - JSON Value 对象 -> serde 反序列化
  - 字符串 -> 复用 `control_ax::parse_ax_action_payload`（line-control 对象字面量）

#### 2. execute 层
- 新增 `perform_action()`，直接调 `SystemAxBackend.perform_action`
- 不再经过 control_ax 的 facade，依赖层次是 ax_action -> control_ax backend

#### 3. routing 表
- `ACTION_ROUTES` 从 1 条扩到 7 条：press + 6 个通用 action
- 6 个通用 action 共用同一对 `parse_action_dynamic` / `execute_action_dynamic`
- 新增 action 只需加一行数据，不写新函数

#### 4. 类型系统
- `AxActionRequest`、`AxActionName` 补 `Serialize` / `Deserialize`
- routing 表内部走 Value 传递，这两个派生是必需的

#### 5. 调用方迁移
- `control_actions.rs::execute_ax_action` -> `ax_action::perform_action`
- `control_web/act.rs::build_default_web_act_response_json` -> `ax_action::perform_action`
- 两个调用方迁完后 `perform_default_ax_action` 零引用，直接删除，没留 deprecated 壳

### 遇到的问题与修正

**问题：compact 格式测试失败**
- 现象：`test_parse_action_compact_format` 报 "@ax-action payload 必须是对象"
- 原因：我先假设 `@ax-action` 像 `@ax-press` 一样支持 `app:X,description:Y` 裸 compact，但读 `parse_ax_action_payload` 源码后确认它只接受对象字面量
- 修正：改测试而不是改生产代码，拆成两个测试：
  - `test_parse_action_object_literal_string`：对象字面量字符串能解析
  - `test_parse_action_rejects_bare_compact`：裸 compact 被拒绝（这是既有契约）

这里的教训是：**先读被复用函数的实现，再写测试**。假设契约相同就会写出反映错误期望的测试。

### 验证结果
- ax_action 定向测试：15 passed
- `cargo check --tests`：0 warning 0 error
- 全量 nextest：978 passed, 21 skipped（之前 2 个 flaky 失败在进程隔离下也通过了）

### 总结感悟

#### 数据化 routing 的实际收益
6 个通用 action 只加了 6 行数据，共用一对 wrapper 函数。如果按老写法要写 6 个 `route_*` 函数。这是摩擦点 #4 想解决的问题，现在有了实证。

#### deprecated 不是默认选项
`perform_default_ax_action` 只有 2 个调用方，都在同一个 repo 里。迁完就能删，没必要留 deprecated 壳。deprecated 是给"调用方不在我手上"的情况用的，内部函数直接删更干净。

#### 测试反映的是契约，不是愿望
compact 格式那个失败提醒我：写测试前要确认被测函数真实支持什么。测试失败时先问"是代码错了还是我的期望错了"，不要条件反射改代码。

### 后续
- 剩余 4 个待迁移：set_value / focus / scroll / press_sequence
- press_sequence 按 grilling 决策保持独立，不进 routing 表
- routing 表还没有生产调用方，等 web_rpc 或 control_computer_act 接入

## [2026-08-21 17:10:00] [Session ID: current] Ticket #05: 迁移三个专用 AX action

### 任务内容
把 set_value / focus / scroll 从 control_ax 迁到 ax_action，routing 表补齐到 10 条。

### 完成过程

#### 1. 用宏收敛 dynamic wrapper 样板
迁移前每个 action 需要手写一对 `parse_*_dynamic` / `execute_*_dynamic`，逐字相同，只有类型名不同。
Ticket #04 结束时已经有 2 对 = 4 个函数，再加 3 个 action 会变成 5 对 = 10 个函数。

改成 `dynamic_route!` 宏声明，每个 action 一行：
```rust
dynamic_route!(parse_focus_dynamic, execute_focus_dynamic,
               AxFocusRequest, protocol::parse_focus, execute::focus);
```
5 个 action 从 10 个手写函数收敛成 5 行宏调用。

#### 2. protocol / execute 层
- `parse_set_value` / `parse_focus` / `parse_scroll`：字符串走 line-control 对象字面量，JSON Value 走 serde
- `set_value` / `focus` / `scroll`：直连 `SystemAxBackend`

`focus` 的文档里写清了一件容易搞错的事：它只做 AX 层聚焦，窗口激活（activate）由 `control_actions` 在外层先行处理，不属于这个函数的职责。

#### 3. 类型
5 个类型补 serde 派生：`AxSetValueRequest` / `AxFocusRequest` / `AxScrollRequest` / `AxValueSetMode` / `AxScrollDirection`。

#### 4. 调用方迁移
`control_actions.rs` 三处，其中 `focus` 是作为函数指针传给 `execute_ax_focus_with` 的，用 `focus as ax_focus` 重命名导入避免与局部参数名冲突。

三个旧函数迁完零引用，直接删除。

#### 5. routing 表覆盖测试
新加三个测试：
- `test_routing_table_covers_all_migrated_actions`：10 个 action 全在表里，且条目数与清单一致（数量断言会在新增 action 忘记同步测试时失败）
- `test_routing_table_has_no_duplicate_names`：重复名字会让后一条永远命中不到
- `test_specialized_actions_reject_invalid_payload`：三个新 action 的无效 payload 在 parser 层就被拒绝

### 验证
- ax_action 定向测试：18 passed
- `cargo check --tests`：0 warning 0 error
- 全量 nextest：981 passed, 21 skipped

### 总结感悟

#### 样板代码到第三次就该收敛
第 1 对 wrapper 是必要的，第 2 对还能忍，第 3 对开始就是抄写。宏在这里不是炫技，是因为这些函数除了类型名之外**逐字相同**——这正是宏该做的事。如果它们有任何实质差异，就不该用宏。

#### 数量断言让覆盖测试不会腐烂
`assert_eq!(ACTION_ROUTES.len(), expected.len())` 这一行让"新增 action 但忘记加测试"变成编译后立刻失败，而不是悄悄漏掉覆盖。只检查"每个期望的都在表里"是不够的——那样加了新 action 也不会有人提醒你。

#### 三个旧函数又是直接删除
和 Ticket #04 一样：调用方都在 repo 内，迁完就删。到目前为止 control_ax 里被移走的 4 个 action 函数都没留 deprecated 壳。

### 后续
- 剩 1 个：`perform_default_ax_press_sequence`（按 grilling Q3 决策保持独立，不进 routing 表）
- routing 表 10 条已就位，但仍无生产调用方；接入 RPC 边界是独立的一步

## [2026-08-21 18:00:00] [Session ID: current] Ticket #06: 迁移 press_sequence，阶段 2 收尾

### 任务内容
把 `perform_default_ax_press_sequence` 及其两个 helper 从 control_ax 迁到 ax_action，完成阶段 2。

### 完成过程

#### 1. resolve_app 从内部依赖改为注入参数
旧实现里 `materialize_press_sequence_request` 硬编码调用 `resolve_unique_app_window_id`，
真正可测的 `_with` 版本藏在私有层，外部只能测到硬编码那一层。

新签名把 resolve_app 提到公开入口：
```rust
pub fn press_sequence(
    request: &AxPressSequenceRequest,
    resolve_app: impl FnOnce(&str) -> io::Result<String>,
) -> AxPressSequenceReport
```

这样 `control_actions.rs` 传真实解析器，测试传 stub，不再需要一对 `f` / `f_with` 函数。
两个 helper（materialize / perform_press_sequence_with）保持私有，只服务模块内和测试。

#### 2. 搬迁 + 补充测试
从 control_ax 搬来 1 个原子性回归测试（app 只解析一次 + 中途失败保留已完成步骤），
另补 3 个之前没有的边界：
- 全部成功时 status 为 ok、failed_index 为空
- 混用不同 app selector 被拒绝（防止执行中途漂移窗口）
- 空 target 列表在 materialize 阶段失败，不产出步骤报告

press_sequence 从"1 个测试"变成"4 个测试"，覆盖成功/部分失败/参数非法三类路径。

#### 3. 删除旧实现
`perform_default_ax_press_sequence` + `materialize_press_sequence_request` +
`materialize_press_sequence_request_with` + `perform_ax_press_sequence_with` 四个函数全删。
调用方只有 control_actions 一处，迁完零引用，没留 deprecated 壳。

`parse_ax_press_sequence_payload` 留在 control_ax（它是 protocol 层，不属于本次迁移范围）。

### 遇到的错误

**错误 1：deprecated 属性插错位置**
用 Python 正则插入 `#[deprecated]` 时，脚本匹配到了文件前部的 `AxObservationCacheEntry`
而不是目标函数，导致 11 个不相关的 deprecated 警告（"AxObservationCacheEntry: use
ax_action::press_sequence instead" 这种明显错乱的提示）。

原因是我用 `re.search` 配 `(///.*?\n)*` 这种贪婪度不明确的模式去定位函数，
而没有先验证匹配到的是哪一处。修正方式是改用逐行扫描精确匹配 `pub fn` 那一行。

教训：**在源码里插入属性，用行匹配而不是跨行正则**。跨行正则在长文件里很容易匹配到
上游某个结构体的文档注释块。

**错误 2：把测试插进了函数体**
用 `s.rindex("}\n")` 定位"最后一个大括号"来追加测试，但那个位置是
`perform_press_sequence_with` 的函数结尾，不是 `mod tests` 的结尾，
结果测试被插进了函数体内部，报 "expected one of `.`, `;`, `?`" 。

修正方式是先把误插块切出来，把函数体正确收尾，再包进一个独立的
`mod press_sequence_tests`。

教训：**`rindex("}")` 不等于"文件末尾的 mod 结尾"**。追加测试要么在文件末尾新建 mod，
要么精确定位目标 mod 的边界，不能靠"最后一个括号"。

### 验证
- press_sequence 相关测试：7 passed（含 control_ax 的 parser 测试）
- `cargo check --tests`：0 warning 0 error
- 全量 nextest：984 passed, 21 skipped

### 总结感悟

#### 注入参数比 `_with` 后缀更干净
旧代码用 `f` / `f_with` 一对函数来兼顾"生产调用简单"和"测试可注入"。
把依赖提到公开签名后，一个函数同时满足两者，调用方多传一个参数的代价远小于维护两个函数。

这个模式在 control_ax 里还有别的地方在用（`perform_ax_press_with_postcondition_with`
接了三个注入参数），迁移那部分时可以顺手统一。

#### 阶段 2 完成情况
control_ax 里的 7 个 action 执行函数，5 个已迁走并删除，剩 2 个：
- `perform_default_ax_press`：被 ax_action::press 内部复用（作为 legacy 实现）
- `perform_default_ax_press_with_postcondition`：同上

这两个是 press 的底层实现，不是"待迁移的调用入口"，属于 Ticket #11 清理范围。

### 后续
- routing 表 10 条 + press_sequence 独立函数都已就位，但 `execute_ax_action` 仍无生产调用方
- 接入 RPC 边界是独立一步，会动到请求分发路径

## [2026-08-27 10:45:00] [Session ID: current] 任务名称: Ticket #11 - press legacy 搬迁 + control_ax action 层清空

### 任务内容
- press 实现层 (perform_default_ax_press / _with_postcondition + 8 个私有 helper + CLEAR_ACTION_HINT)
  从 control_ax.rs 整体迁入 ax_action/execute.rs, 重写为真实实现, 删除 legacy import
- ax_action::press / press_with_postcondition 成为生产路径, control_actions 3 个调用点迁移
- 删除 control_ax/input.rs (deprecated facade 零调用) 与 ax_input/input.rs (重复实现),
  remap_type_text_* 错误命名函数收编为 control_ax/macos.rs 私有 (唯一消费者就地私有化)
- ax_input 收敛为单一 with_config API, 删除零调用的简单包装层 (type_text / send_key) 及其
  "result.is_ok() || result.is_err()" 恒真断言测试, 换成函数签名编译时验证
- 迁入 5 个 press 实现层测试 (clear hint / guarded 重试 / fail-closed / bidi 归一化 / 深层观察)

### 完成过程
- 上一会话完成 11.1-11.5 的代码实施; 本轮确认存量状态后执行验证门禁并收尾提交
- 验证: cargo check --tests 0 warning 0 error; 定向测试 45/45; 全量 nextest 969 passed,
  1 failed (tests/control_tty.rs 箭头键测试, LATER_PLANS.md 2026-08-19 已登记的既有 TTY
  时序 flake, 失败模式与当时记录逐字一致, 与本次改动无关), 21 skipped

### 总结感悟
- remap 类纯错误命名函数: 消费者唯一时就地私有化优于搬到公共模块, 消除跨模块跳转
- ax_input 简单 API 的教训: 为"80% 场景易用"预建的包装层, 迁移完成后零调用即删,
  恒真断言测试纯属噪音。设计 API 入口时先等真实调用方出现
