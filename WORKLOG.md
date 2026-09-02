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

## [2026-08-27 11:20:00] [Session ID: current] 任务名称: $implement all - routing 表终局 + 双轴 review

### 任务内容
- 终局处理 "routing 表接入生产路径" 遗留项: 全仓库证据确认动态层零生产消费者后,
  删除 ACTION_ROUTES / execute_ax_action 字符串 API / dynamic_route! 宏 / protocol.rs
  (共 559 行, 14 个测试), mod.rs 收敛为纯 re-export (2e8239e)
- ADR-0008 加 Amendment, 两个 ax-split spec 头部加 as-built 执行状态 (78a60ab)
- 双轴 code-review (并行子代理): Standards 无硬性违规, 7 个判断项; Spec 确认
  @ax-press wire 响应形状逐字保留、删除边界干净
- 当场修复 review 判断项: macos.rs 两个逐字重复的 remap 函数收敛为
  remap_type_text_path_error(err, path_label), 输出契约由测试逐字锁定

### 完成过程
- 判定 "接入" 无诚实路径的依据: ControlCommand 到分发点已是强类型, 接入需人为
  Value 往返; compact 行协议 / ui_script / web RPC 三个边界全部选择强类型
- 测试数 970 → 956, 差值正好等于删除的动态层测试数, 无误伤

### 总结感悟
- 为设想中 RPC 边界预建的抽象, 经过 10 次真实迁移都没等到消费者 -- 这本身就是
  最强的反证。删除比人为接入更忠实于 "implement all" 的意图
- 遗留规划文档的正确处理是 "as-built 增补 + 保留历史", 不是重写; ADR 用 Amendment
  机制承接设计演进, 决策链路可追溯

## [2026-08-28 11:05:00] [Session ID: current] 任务名称: ADR-0008 阶段 3 - ax_query 无状态捕获核心

### 任务内容
- 新建 src/ax_query/ (mod.rs + capture.rs ~420 行): tree.rs 中零 observation/protocol
  依赖的 capture / 查找 / target 物化函数迁入, 模块文档写明纯度契约
- 全部 capture 消费方 (observation/producer, screenshot, control_actions,
  control_web, computer_act/verify, ax_action, observation.rs) 改从 ax_query 导入
- tree.rs 收缩为 target 解析 + selector 富化桥接层; query.rs 保留为 verb 层
- ax_action 的纯遍历拆为 ax_query::collect_ax_role_values + 验证层归一化
- 双轴 review 后: find/semantic 同形分发器统一为 capture_scoped_snapshot,
  find 包装器回归 verb 层 (AppMenu 短路保序), mod.rs 依赖文档如实化
- 文档: ADR-0008 Amendment 2, 两个 spec 状态头, CONTEXT.md 两条术语 as-built 化

### 完成过程
- 关键判断: scratch tickets 07/08 (ObservationCapture adapter / cache TTL 迁移)
  被 #51/#54/#55 的现实超越 -- epoch 真相源分离已落地并对抗性验证, 迁移只剩
  物理位置收益; query.rs 实为 verb 实现而非纯查询引擎, 不能整搬
- review 发现 ax_query 吃 verb 层 AxFindRequest 的边界渗漏, 用 "已解析原语"
  方案修复 (优于搬 types.rs: 避免把 DisplayScope 和 observation ref 解析拖进共享内核)

### 总结感悟
- 阶段规划要先核对 "设计时假定的现状" 是否被后续演进改变, 本轮 3 张 tickets
  有 2 张半已被 #51/#54/#55 静默超越
- 底层模块的入参用 "已解析原语" (window_id) 而非上层请求类型, 是保持依赖
  单向的最有效手段, 比事后搬类型更干净

### 验证
- cargo check --tests 0 warning 0 error; 纯度断言 (grep) 通过
- 定向 21/21; 全量 nextest 955/956 (唯一失败为既有 control_tty flake)

## [2026-08-28 14:30:00] [Session ID: current] 任务名称: LATER_PLANS 三项延后改造收尾

### 任务内容
- R1: press() 双分支收敛 -- 证据: 全部调用方只传 postcondition: None,
  双分支是已删除的 routing 表遗产。类型级方案 press(target: &AxTarget),
  guarded core / press_sequence 注入参数同步收窄, simplified-report 转换分支删除
- R2 (决策不做): status/kind/action 转枚举 -- 实为 &'static str + 10 个同族 report
  一致 + 集中构造器, 孤立转换制造不一致; 认识修正后记 LATER_PLANS (全族 sweep 另排)
- R3: ax_input 从转发壳升级为真模块 -- type-text 投递策略 (模式分发/Auto 回退链/
  can_fallback 边界/remap 命名) 自 macos.rs 迁入 ax_input/execute.rs, 平台路径与
  信任检查注入, AxBackend 删除 type_text; 有意 wire 修正: Auto 非可恢复错误的
  双重 remap 前缀收敛为单次 (kind 与前缀保留)
- 双轴 review 修复: LATER_PLANS 补写 (此前声称已记实际未写, 属执行疏漏, 勘误在案),
  stub 去重, AxElement unused 修复, 测试计数勘误, 非 macOS Auto 标签漂移记录;
  驳回误报: press_plain 仍被 press_with_postcondition 注入, 非 Middle Man

### 总结感悟
- 记录与执行必须同步核销: "已记入 LATER_PLANS" 这句话被 reviewer 用 git diff
  当场证伪。账本声明要写成"已写入 <文件> <行>"这种可验证形式
- 平台文件里藏着的"策略" (回退链/门禁/错误命名) 是模块拆分时最容易漏掉的
  真实资产; 拆出来配上注入测试后, 覆盖从 1 个纯函数测试变成 5 个行为测试

### 验证
- cargo check / check --tests 双零 warning; 定向 36/36;
  全量 nextest 958/959 (唯一失败为既有 control_tty flake)

## [2026-08-28 15:20:00] [Session ID: current] 任务名称: R5 control_tty 假 flake 根因修复

### 任务内容
- 按 现象→假设→证伪 的纪律排查被标为 flake 两周的 TTY 箭头键测试失败
- 三个假设两个被实验证伪 (script PTY 内 is_terminal 为 true; 代码路径零变动),
  TERM 受控实验锁定根因: 非交互 harness 的 TERM=dumb 触发 rustyline 正确的
  dumb 终端降级, 方向键序列不做本地编辑
- 修复: 测试显式固定 TERM=xterm-256color (测试意图就是模拟交互终端,
  不应继承 harness 环境); 生产代码零改动

### 总结感悟
- "交互终端绿 + CI/agent 红" 是环境决定性的指纹, 不是 flake 的指纹;
  第一件事应该是让测试固定它假设的环境, 而不是怀疑时序
- 失败输出的字节形状直接指向代码分支: ESC 透传 == 非 TTY 整行读取路径,
  这个比对把排查范围从 "raw mode 竞态" 直接拽回 "rustyline 根本没接管"

### 验证
- TERM=dumb / 默认环境单测均 PASS; 全量 nextest 959/959, 21 skipped,
  本分支工作以来首次完整绿灯

## [2026-08-28 17:00:00] [Session ID: current] 任务名称: 分支审查发现修正 (P2x1 + P3x2)

### 任务内容
- P2: 清除 62c782e 误入库的垃圾文件 -- .tmp/pi-prompts/test.txt 删除并让 .tmp/
  进 .gitignore; .codegraph/daemon.pid 解除跟踪 (目录级 .gitignore 本就全忽略,
  历史跟踪覆盖了规则); fmt 夹带事实在修正 commit 与 spec 状态头留档
- P3: ax_query/mod.rs 纯度文档改为如实表述 (依赖清单补全, 解析职责措辞收窄);
  ADR Amendment 2 与两份 spec 状态头补全 R1/R3/macos.rs 三处已实施分歧;
  .scratch tickets 07-11 全部标记真实处置状态
- 修正全程无行为改动, 纯卫生与文档

### 总结感悟
- 双 skill 审查的驳回机制运转良好: 两项子代理误报 (存量代码误判为本分支引入)
  都在核实后驳回, 避免了无意义返工; 发现也全部可验证可执行
- 被跟踪文件会覆盖后加的 gitignore 规则, "已忽略却还在库里" 的解法永远是
  git rm --cached, 而不是改规则

### 验证
- cargo check --tests 0 warning; fmt clean; 全量 nextest 959/959, 21 skipped
- "warning 里 unused 的符号" 常常只是当前编译目标不用, 删除前必须 grep 全部 cfg 引用
- LATER_PLANS 记录的噪音源 (FIFO) 与真实匹配模式 (.pipe_uplink) 不一致时, 先读扫描代码再清理

## [2026-08-22 00:00:00] [Session ID: current] Ticket #04 完成: 通用 action 迁移

### 任务内容
将 `execute_ax_action` 从 `perform_default_ax_action` 迁移到 `ax_action::perform_action`。

### 完成过程

#### 1. 迁移调用路径
- 修改 `control_actions.rs`:
  - import: 删除 `perform_default_ax_action`，添加 `ax_action::perform_action`
  - `execute_ax_action`: `perform_default_ax_action(request)` → `perform_action(request)`

#### 2. 验证编译
- 编译通过（0 errors）
- 已启动后台测试验证

### 架构改进

#### 调用链简化
**Before**:
```
control_actions::execute_ax_action
  → control_ax::perform_default_ax_action
    → SystemAxBackend.perform_action
```

**After**:
```
control_actions::execute_ax_action
  → ax_action::perform_action
    → SystemAxBackend.perform_action
```

减少一层中间调用。

#### 职责清晰化
- `control_actions.rs`: RPC routing 层，不关心 AX 实现细节
- `ax_action::perform_action`: 统一 action 执行入口
- `SystemAxBackend`: 平台实现

### 待验证
- [ ] 后台测试通过（b5tpdy6ja）
- [ ] press_sequence 测试通过（b3uzz7d02）

### 下一步
- 等待测试结果
- 如果通过，更新 task_plan.md 标记 Ticket #04 完成
- 准备 Ticket #05: 标记 perform_default_ax_action 为 deprecated

## [2026-08-22 01:50:00] [Session ID: zcode-sess_fa3b551c] 任务名称: 闲时 continuous-learning 整理 (无人值守)

### 任务内容
- 回读默认六文件 + EXPERIENCE.md + docs/solutions 7 份 + 8-13~8-20 git 提交链, 按七项门禁评估 Capture 候选
- Capture: docs/solutions/architecture-patterns/ax-observation-cached-progressive-queries.md
  (承接 60f9e26 + 1066123 + 8af9e12 三个提交的 cached progressive queries 架构契约, 此前无文档载体)
- Scoped Refresh: docs/glossary.md 的 verify_failed 条目仍写 "ok:false error_code" 旧语义,
  已按 outcome 三态现状 Update, 并补 outcome 术语 (证据: src/control_computer_act/outcome.rs:16 与
  error_envelope.rs:63 注释)
- AGENTS.md: Domain docs 节明确 CONTEXT.md (canonical) 与 docs/glossary.md (computer-act surface)
  分工; 长期文件索引新增该 solution 与 docs/glossary.md 两条入口
- LATER_PLANS 清理 4 条已落地条目: warning 清理 (8-09 完成, 752125b)、admin transport event
  (8-09 定位不改)、guard/FIFO 清理 (8-09 完成)、screenshot timeout-trace flaky (881b300 加
  TIMEOUT_TRACE_TEST_LOCK 于 capture_trace helper, 三个调用点全部串行化)
- Gate 判定 skip 的候选: trusted changes (gui-resource-epoch solution 8-20 已承接)、
  screenshot serialize (琐细细粒度, 同族知识已在 EXPERIENCE 2026-07-18 唯一共享锁条目)

### 完成过程
- 工作区 src/control_actions.rs 有进行中的 ax_action 重构导致 bin 编译失败, 未触碰用户改动,
  改用 git worktree 在干净 HEAD (8af9e12) 复跑测试后清理
- EXPERIENCE.md 保持不动: 8-13 条目的 upstream Pi 已 Capture 说明仍准确; 8-08 ledger 条目由
  workflows/macos-ops-interaction-efficiency.md 完整承接 (计量口径/分类/认证门槛逐一核对)

### 总结感悟
- 8-13 之后的 session 大量落地功能但未维护六文件 (WORKLOG 停在 8-09), 闲时整理要靠
  git log --stat 考古补齐知识链; solution last_updated 与提交时间对齐是健康的信号
- 用户进行中的重构会让主工作区无法编译, worktree 干净态复跑是无人值守任务获取动态证据的
  可复用手段, 不碰用户工作区

## [2026-08-22 12:47:43] [Session ID: zcode-sess_fa3b551c] 任务名称: continuous-learning 全量整理 (用户显式调用)

### 任务内容
- 清偿 2026-08-09 遗留欠账: EXPERIENCE.md 积压 27 段候选全量核验分流
- Capture x3: gui-target-owner-evidence-gate (含 WeChat 政策漂移修复) /
  zenoh-hello-locator-priority / daemon-log-sentinel-e2e-contract
- 发现并处置重大漂移: WeChat Temporary No-AX Policy (fail-closed 安全政策) 于
  2026-07-28 被 92a3d06 skill 瘦身无记录移除, AGENTS.md 与 EXPERIENCE 双悬空;
  政策本体抢救进 solution, 悬空引用修复, 恢复决策记入 LATER_PLANS
- EPIPHANY_LOG 续档: 999 行 -> 归档 31 条已核查旧档 + 新档写入 token 优化删除
  安全政策的系统性风险; manifest 见
  archive/manifests/ARCHIVE_MANIFEST__2026-08-22_epiphany_rollover_wechat_policy_drift.md

### 完成过程
- 27 段候选逐段定向取证: 11 段确认既有载体承接, 3 段通过七项门禁 Capture,
  其余确认代码/spec/skill 即载体保留索引 (处置明细见 EXPERIENCE 尾部指针条目)
- 三份 solution 全部通过 frontmatter + claims 双校验 (0 flags)
- AGENTS.md: 修 skill 条目悬空声明 + 追加 3 条 solution 索引

### 总结感悟
- 积压收件箱的全量核验最大价值不是 Capture 数量, 而是抓到了 "安全政策被 token
  优化静默删除 + 双处引用悬空 25 天" 这种单 Session 视角永远看不到的漂移
- 经验类知识放 canonical skill (会被度量裁剪的载体) 等于没放; durable 载体 +
  skill 引用才是安全政策的正确分层

## [2026-08-22 13:13:08] [Session ID: zcode-sess_fa3b551c] 任务名称: WeChat no-AX 政策恢复进 SKILL.md + git 提交

### 任务内容
- 用户决策 "同意恢复": 政策原文 (2026-07-14 版) 从 git 历史 (92a3d06^) 逐字恢复进
  SKILL.md v2.28-wechat-noax-restore, 位于 Native App Lane 末尾
- Safety 节加 WeChat 指针行; 政策节尾部加来源注 (durable 载体 solution 路径 +
  "safety boundary, 禁止 token 优化移除" 声明)
- 载体同步: solution Evidence 更新恢复状态; AGENTS.md skill 注释翻转;
  LATER_PLANS 决策项清除; EPIPHANY_LOG 追加决议

### 完成过程
- 验证矩阵: 政策正文与 92a3d06^ 逐字 diff 一致; fence 14 个成对; 关键句
  (xinWeChat / 文件传输助手 / 政策标题) 全在; git diff --check 干净;
  solution 双校验复跑 0 flags
- git 分两笔提交: (1) skill 政策恢复; (2) continuous-learning 知识批次

### 总结感悟
- 恢复用 "从 git 历史逐字复原 + diff 验证" 而非凭记忆重写, 保证安全政策零语义漂移
- 来源注里显式声明 "safety boundary" 是给未来优化 pass 的护栏, 把这次教训固化进载体本身

## [2026-08-28 13:10:00] [Session ID: current] 任务名称: Task/Spawn Phase 1 @spawn 四原语实施

### 任务内容
- feature/task-spawn-phase1 分支: 6cf386a (spec) + c0e3863 (实现)
- 新增 src/task_control.rs (registry/waiter/ring buffer/取消) 与 parsers/task.rs
- control_core 四专门分支, shell/tests.rs mock 补分支, spec 偏差同步

### 完成过程
- 调研协议接入模式后按 PTY 全局 registry 模式实现
- 修复自引入死锁 (cancel 持 registry 锁调 finalize 再锁)
- 修复并行会话留在工作区的非法嵌套 use (语法修, 意图保留; 后被其 29d49e5 收编)
- 处理错分支提交事故: feature 重建 + fix 分支摘除 + .mimosa 剔除
- 验证: 25 新测试绿, 全量 984/984, 0 warning, fmt clean

### 总结感悟
- 多会话共享工作区时, 提交前必查当前分支, 永不 git add -A
- e2e 测试暴露 render_structured_success_response 的信封行为 (有 request id 才包
  value 信封), 协议消费者要兼容两种形状
## [2026-08-28 20:10:00] [Session ID: zcode-idle-20260828] 任务名称: 闲时 continuous-learning 整理 (无人值守)

### 任务内容
- 回读默认六文件 + __a2a_research 支线集 + EXPERIENCE.md + docs/solutions/ 全量
- Compound Gate 裁决本轮候选: 2 capture / 3 inbox 保留 / 其余确认已承接
- a2a_research 支线三文件归档 + manifest; LATER_PLANS A2A 条目清理
- AGENTS.md 索引同步 (2 新 solution + 2 既有遗漏修复)

### 完成过程
- Capture x2 (双校验均 0 flags, 中途修正 1 处相对路径 flag):
  - test-failures/tty-term-dumb-environment-deterministic-failure.md
    ("交互终端绿 + CI 红 = 环境决定性失败" 诊断指纹, 受控实验双向验证)
  - best-practices/parallel-test-global-state-single-lock.md
    (env / tracing / singleton 三例同模式的模块顶层唯一锁纪律)
- Scoped Refresh 判定: 无漂移 -- ax-split (PR #61) 后缓存仍在 control_ax,
  resource_lane 未动, 既有 solution 不受影响; 评测与测试诊断域不重叠
- 归档: git mv 三文件至 archive/branch_contexts/a2a_research/,
  manifest 记录候选去向 (全部由 specs/rdog-task-spawn-control-plan.md 承接)
- EPIPHANY_LOG 回顾: 2 条均已解决, 无新增重大风险, 不追加
- 全程未触碰并行会话进行中的 src/ 改动 (task-spawn Phase 1 实施中)

### 总结感悟
- 8-22 全量复盘处置清单存在盲区: 8-18/8-19 三条测试隔离候选未出现在
  逐条处置记录中, 本轮靠时间戳比对才补上裁决 -- 处置记录应显式覆盖"复盘时点
  之前的全部条目", 而不是只列代表性条目
- 三例同模式的第三次出现 (singleton) 是模式成熟的信号, 单例时判"代码即载体"
  的条目, 在多例印证后值得升级为正式 solution

## [2026-08-28 16:10:00] [Session ID: current] 任务名称: 处理 PR #62/#63 (CI 修复落地 + Phase 1 推进)

### 任务内容

- 接手两个 open PR 的处置: PR #62 (CI 修复, UNSTABLE) 与 PR #63 (Phase 1, DRAFT)
- PR #62: 修正标题/body (原纯 docs 标题未反映 CI 修复内容), 修正 PR #63 body
  中已失效的 29d49e5 hash 引用 (被并行会话 amend 为 10d495e)
- PR #62 ubuntu unit tests 4 个失败的根因定位与修复 (e1f61dc):
  - 3 个 open_app envelope 测试缺 #[cfg(target_os = "macos")] 门控
    (非 macOS 分支绕过 mock), 补门控 + 新增非 macOS 镜像测试
  - shell_lane timeout duration 2001ms: GNU coreutils kill("-TERM","-<pid>")
    参数歧义 (docker strace 实证解析为 kill(-2, TERM) ESRCH), 孤儿持管道;
    terminate_process_tree 改 libc::kill(2) 直发进程组信号
- PR #62 合并 (ubuntu 首绿; macos 4 个 screenshot 失败与 main 同轮同款存量)
- PR #63: 合并 main 解冲突 (control_actions.rs 一行注释差异取 main 版,
  WORKLOG 双会话记录按时间序保留), 本地 task 19/19 验证后推送
- PR #63 macos flake 判定: 跨 main/#62/#63 多轮比对矩阵 + recording e2e
  本地 6 轮 30/30 复现, 确认全部为存量抽签 flake, Phase 1 无回归

### 完成过程

- 与并行会话在同一分支上协作: 发现对方已先行修复混入问题 (29d49e5→10d495e)
  和 gbm/drm 依赖, 避免了重复劳动; 我的增量是 unit tests 层的根因修复
- 全程 git worktree 隔离操作 PR 分支, 未触碰主工作区 (phase2 检出) 和 .mimosa
- 根因验证遵守 现象→假设→验证→结论: docker 容器 python 复刻 + strace 决定性证据

### 总结感悟

- CI 每修一层暴露下一层存量: Build→链接→unit tests, 修复链要逐层实证不跳步
- GNU/BSD 工具链的同名命令参数解析可能有静默歧义, 跨平台代码避免经外部命令
  做关键路径; strace 是这类问题的最终裁判
- 多会话同分支协作时, 动手前先 fetch + ls-remote 确认远端头, 别按记忆里的
  hash 干活

## [2026-08-28 15:30:00] [Session ID: current] 任务名称: Task/Spawn Phase 2 四票实施

### 任务内容
- feature/task-spawn-phase2: seq/帧族/推送接线/client 链路/e2e, 四票全关
- stacked PR #69 (base = PR #63 分支)

### 完成过程
- 按依赖链 #64→65→66→67 顺序实施, 每票实现+测试+commit+comment+close
- 排障链: nextest 空挂 (tail 管道缓冲+真死锁) → 单线程串行定位测试自死锁 →
  e2e 三轮失败逐层剥: 白名单拒绝 → invocation 报错 → completed 竞态 →
  bridge 生命周期误判 → wait 行稳定窗口
- 手动 daemon+client 复现两次, status 探针隔离出"收割正常/推送丢失"

### 总结感悟
- e2e 失败输出 (错误消息) 每轮都精确定位了下一层问题, 没有一轮白费
- one-shot 多行 client 的帧时序是真实契约, 注释进测试而非依赖巧合
- 后台任务帧推送的验收必须双侧 (client 可见性 + daemon 发布证据)

## [2026-08-28 21:35:00] [Session ID: current] 任务名称: CI 修复战役 -- 双平台历史首次全绿

### 任务内容
- 从 "研究 ubuntu CI 怎么修" 打穿到 "仓库 CI 历史首次双平台全绿"
- 6 层根因逐层定位与修复 (详见 task_plan 同日条目), 每层有独立动态证据
- 交付: PR #62 (部分, 并行会话合并) + PR #70 (测试稳定性 5 连发, 本人合并)

### 总结感悟
- "端口监听 != 就绪" / "固定 sleep 等置位" / "外部 kill 二进制方言" 三个坑
  都是跨平台/负载敏感的经典形态, 值得进 docs/solutions
- 本地容器复现 + 探针观测 > 反复盲推 CI; 一轮 CI 7 分钟, 一个探针 7 秒
- 与并行会话在同一工作目录协作要极谨慎: 每次提交前 git diff --stat 核对,
  他们的 WIP 会出现在你的 git add 范围里

### 验证
- CI run 33151443006: ubuntu ✓ + macOS ✓ (仓库首次)
- 本地: macOS 全量 nextest 绿; linux 容器全量单测除 OrbStack 网络专属项外绿

## [2026-08-28 23:20:00] [Session ID: current] 任务名称: merge 链 + Phase 3 #72/#73 实施

### 任务内容
- PR #77 (recording 修复) + PR #69 (Phase 2) 相继 merge, main 转绿
- #72: agent keyexpr + rdog.agentmsg.v1 envelope (8 单测)
- #73: daemon 侧 mailbox 全链 (store/通配 sub/三命令/e2e)

### 完成过程
- mailbox 跨主机单点归属: 通配 sub + 注册集合过滤, 未注册不缓存
- @agent-ack 对象格式踩坑: serde 严格模式不支持项目无引号 key 协议,
  改手写字段循环 (与 pty parser 同款)
- e2e 测试进程 zenoh 直投: dev-dep zenoh + 显式 connect entry-point
- 分发挂载曾静默失败 (replace 无 assert), 后续挂载全部 assert

### 总结感悟
- replace 型脚本改代码必须 assert anchor, 否则静默失败很难发现
- 通配 sub + 注册集合是 zenoh 广播语义下做单点归属的轻量模式

## [2026-08-29 00:30:00] [Session ID: current] 任务名称: Phase 3 五票连打 (#72-#76)

### 任务内容
- agent messaging 全链: envelope/mailbox/agent runtime/卡片/收口 e2e
- PR #78 五个实现提交, 全量 1022/1022

### 完成过程
- 排障三连: @response 前缀剥离 -> from/to 混淆 -> reply sub 时序契约
- 一次测试文件误删事故 (git checkout 丢未提交测试) 重写恢复

### 总结感悟
- zenoh pub 的'无订阅者即丢'是隐式契约, 测试必须先 sub 后触发
- 防丢与防重的语义权衡要靠真实场景驱动 (#76 的 pre-start 场景推翻了
  #73 的注册过滤设计 — 简化正确)

## [2026-08-30 11:10:00] [Session ID: current] 任务名称: continuous-learning 全链复盘 (main)

### 任务内容
- 范围: PR #63-#92 全链 (task-spawn 1-3 + 认证 A/B + 测试隔离) 的经验沉淀消费
- Capture x5 (全部双校验 0 flags):
  - test-failures/silent-replace-anchor-assertion.md (两次静默未命中合并)
  - conventions/zenoh-pub-sub-declaration-propagation.md (声明传播竞态)
  - conventions/e2e-isolated-home-credentials.md (三方同源凭证)
  - conventions/env-deterministic-ci-red-vs-local-green.md (读直到安静家族)
  - design-patterns/mailbox-delivery-loss-precedence.md (防丢语义修正)
- skill x1: self-learning.script-replace-must-assert-anchor (用户级, 两次事故满足门槛)
- EXPERIENCE 处置记录 + AGENTS.md 5 条索引 + LATER_PLANS ax-split 条目清理

### 完成过程
- 六文件按本轮时间窗 (08-26~08-30) 分组, 与 08-28 闲时整理的边界清晰
  (那次消费了 tty-term/parallel-lock/a2a 归档, 本次消费其后新增)
- 5 个候选全部过七项门禁: 动态证据 (e2e/CI run/对拍) + 静态证据 (代码/票)
  双全; 无重叠 (与 TERM=dumb 是同族互补非同义, 互相引用)
- inbox 保留 2 条 (commit 主题拆分等第三次实践 / 配置隔离的平台缺口)

### 总结感悟
- 本轮 Capture 的 5 条有 4 条来自"测试失败域" — 测试是这套工作流
  产知识最密集的地方
- "环境决定性失败"家族三条 solution 互相引用成谱系, 后续遇到
  本地/CI 差异可直接按谱系排查

## [2026-09-01 12:10:00] [Session ID: wayfinder-ocr-chart] 任务名称: Wayfinder 建图 - OCR 辅助内容识别

### 任务内容
- 为"集成 OCR 辅助 no-AX app 内容识别"建 Wayfinder map (GitHub issue #95) 与 6 张 child ticket (#96-#101)
- 覆盖: GitHub Issues (map/sub-issues/dependencies), 本地六文件报备

### 完成过程
- 两轮 grilling 与用户逐项确认 8 项决策 (终点=规格+决策 / 引擎开放比选 / macOS 先行 / 通用 no-AX 内容层 / @screenshot include_ocr / 纯 os-logical 坐标 / 跟随截图生命周期 / 三件套验收)
- 摸底: rusto-rs 仅 12 stars/v0.2.5 (RapidOCR+MNN); e2e 已外挂 tesseract (eng); WeChat no-AX 政策允许路径正是 OCR 要补的环节
- sub-issues API 挂 6 张票, native dependencies 拉 7 条阻塞边, T1 引擎比选 (#96) 派后台 research subagent

### 总结感悟
- rusto-rs 是用户点名项但成熟度过低, 开放比选 + Apple Vision 纳入候选是更稳的路径; 最终由 #96 research + #99 prototype (HITL) 双证据拍板
- 政策红线已在 map Notes 固化: OCR 文本不得当作 AX 语义 ref, 坐标动作仍走 guarded coordinate 门禁

## [2026-09-01 16:30:00] [Session ID: wayfinder-ocr-chart] 任务名称: Wayfinder work-through - OCR map 三票推进

### 任务内容
- 以 work-through 模式推进 map #95: 解决 #98 (政策衔接, grilling)、#96 (引擎比选, 后台 research)、#100 (模型分发, 后台 research)、#97 (manifest schema, grilling)
- #99 由"WeChat OCR 选型 demo"重定义为"Vision 量化数据采集"
- 用户拍板 Apple Vision; CONTEXT.md 新增术语 "OCR 内容层"

### 完成过程
- #98: 一轮 grilling 4 项 (政策 v2 纳入 OCR / skill 政策段+cookbook / 坐标时效复用现有门禁 / 正例+负例验收), resolution+close+map 回写
- #96/#100: 后台 research subagent 完成, 均核验过 comment/close/map 三步落盘; 关键发现 = Vision zh-Hans 需 macOS 11+ (rev2) 且不支持语言会静默降级, daemon 必须 fail-closed 探测
- #97: 两轮 grilling 6 项 (行级 box / rdog.ocr.v1 / 请求级失败 / confidence 透传 / 双层共存 / 引擎原序), resolution+close+map 回写; #99 重定义 body+title

### 总结感悟
- Vision "不支持语言静默降级"是 fail-open 陷阱的典型样本, 与仓库 TERM=dumb 环境决定性失败家族同源: 显式探测 + 显式指定语言是唯一防御
- research 豁免单票规则很好用: 两张 research 票全程后台并行, HITL 票才占用用户会话
- map 的 Decisions so far 已成体系 (5 行索引), #101 规格票的素材基本齐备, 只差 #99 的量化数据

## [2026-09-02 10:45:00] [Session ID: wayfinder-ocr-chart] 任务名称: #99 Vision 真实 WeChat 截图量化数据采集

### 任务内容
- 完成 map #95 的 #99 prototype 票: Vision OCR 在真实 WeChat 深色模式窗口的三档分辨率量化评测
- 产出: 指标表 + 失效类清单 + #101 阈值建议 (resolution comment), 标注图本地留档

### 完成过程
- 环境摸底: pgrep + CGWindowList 定位窗口 (坑: owner 名是本地化 '微信' 不是 'WeChat'), screencapture -l 定向截图
- Swift vision_ocr.swift: supportedRecognitionLanguages 探测 + zh-Hans accurate + 三次计时 (冷/热) + 归一化坐标转像素 + JSON 落盘; 标注改用 Python PIL (AppKit NSGraphicsContext 在脚本模式 nil 崩溃)
- 评测: agent 人工转写为 GT, difflib 行级最佳匹配 (含相邻3行拼接); 逐行点名区分"真漏检"与"评测伪影"
- 收尾: resolution (不含任何聊天内容) + close + map 第五条决策回写

### 总结感悟
- 数据挖掘三连: 50% 缩放召回反超原图 (91% vs 87%, Vision 内部自缩放), <40% 是悬崖 (63% + 置信度塌 0.30), 置信度离散桶 0.30 好坏混杂不能当硬阈值
- 评测脚本自身会造假: 三处归一化缺陷把 5 条正常识别伪装成 miss; "逐行点名"是区分脚本伪影与真失败的必要步骤
- CGWindowList owner 名跟系统语言走, 窗口定位代码必须不依赖 owner 名或同时匹配中英

## [2026-09-02 11:30:00] [Session ID: wayfinder-ocr-chart] 任务名称: #101 规格撰写 + map #95 到站

### 任务内容
- 终点交付 specs/rdog-ocr-content-layer-plan.md (11 节正式规格, 含已校验 mermaid 链路图)
- 配套: AGENTS.md 索引 / evidence-gate 政策 v2 / SKILL.md 一句更新 (cookbook 留实现期)
- map #95 全 6 票关闭后到站 close

### 完成过程
- 规格汇总 5 项已关决策 + #99 实测阈值; beautiful-mermaid-rs 校验 mermaid 语法通过
- evidence-gate Guidance 增补第 6 条 (政策 v2 + 新红线), frontmatter last_updated/tag 同步
- SKILL.md 在 WeChat 政策段插入 include_ocr 一句 (token-lean, 标注 spec 路径与 "never AX refs")

### 总结感悟
- Wayfinder 全程跑通: research 两票后台并行, grilling/prototype 三票用户在环, 决策密度高且每票有 resolution 落盘
- 规格刻意把 cookbook 留给实现期: 描述不存在的协议行为必然失真, 文档清单分 [x]/[ ] 是诚实边界

## [2026-09-02 13:00:00] [Session ID: wayfinder-ocr-chart] 任务名称: OCR 引擎性能横评与 spec v1.1 修订

### 任务内容
- 用户反馈 Vision 太慢 -> 同截图同 GT 横评 Vision fast / oar-ocr v6 tiny / tesseract chi_sim
- 结论: oar-ocr 全面胜出, 用户拍板换主引擎并完全移除 Vision; spec 修订至 v1.1
- 证据与拍板均固化到 #96 (追加 comment x2)

### 完成过程
- oar-ocr: /tmp/ocr99/oar_bench harness (auto-download feature, OAR_HOME 隔离, v6 tiny 6MB 模型), 推理 ~0.35s 尺度不敏感, 召回 89/93/83%, 热加载 77ms
- Vision fast: swift 变体实测, 中文不可用 (17%/2%/0%, 系统性乱码) — "fast 档救速度"路线排除
- tesseract chi_sim: 3.3s 出局; 排障发现 Leptonica 读不了 /tmp 绝对路径, 相对路径/管道可绕
- spec v1.1 整体重写 (修订记录制), AGENTS.md 索引与 #96 追溯 comment 同步

### 总结感悟
- 初版引擎决策时"性能"只有文档级证据, 这次证明实测横评不可省: fast 档不可用和 oar 的尺度鲁棒性都是意料外事实
- 引擎反转但政策/schema/链路不动 — 前期把引擎无关层 (schema v1/失败语义框架/政策红线) 独立设计, 修订成本大幅下降
- Wayfinder 图关闭后用户新证据触发的引擎重审: 走"追加证据 comment + 窄修订 + 追溯 comment", 不重开图

## [2026-09-02 14:10:00] [Session ID: wayfinder-ocr-chart] 任务名称: rusto-rs (MNN) 补测入组

### 任务内容
- 用户点名补测 MNN 路线 (rusto-rs), 同截图同 GT 同协议入组横评
- 结论: 不动摇 oar-ocr 主引擎; 证据固化 #96 追加 comment

### 完成过程
- 克隆 rusto-rs, 修正 tier 名下载 PP-OCRv6 tiny 模型 (det 2.2M/rec 4.7M/dict 27K), release 编译 1m13s (bin 名 rusto-rs 非 rusto)
- 基准: 三图 x3 次, TSV 输出转 eval JSON; 发现 dict 档位错配致高置信度乱码, 重下正确字典后复测
- 评测: 行级召回 30/26/15% (det 跨栏合并 32 大框), 文本层召回 87% (认字同档), 冷进程 1.0s (地板 0.48s)

### 总结感悟
- "高置信度乱码"是 rec 字典错配的指纹: 模型/字典/后处理三件套必须同源同档, 任何 OCR 集成都要先跑 GT 冒烟再采信置信度
- 行级召回与文本层召回要分开看: 前者衡量"框好不好点", 后者衡量"字认得对不对", 混用会误判引擎优劣

## [2026-09-02 16:00:00] [Session ID: wayfinder-ocr-chart] 任务名称: OCR 内容层第一阶段实现 (spec v1.1)

### 任务内容
- 实现 specs/rdog-ocr-content-layer-plan.md 的第一阶段: oar-ocr 引擎接入 + @screenshot include_ocr 协议 + rdog.ocr.v1 manifest 层 + fail-closed reason code

### 完成过程
- codegraph 摸底: ScreenshotRequest/parser/ScreenshotManifest/bundle builder/错误透传约定 (io::Error JSON 对象 -> render_control_action_error_response 透传 + 注入 code/id)
- 新模块 src/control_ocr.rs: schema 类型 + OcrError (OCR_ENGINE_UNAVAILABLE/OCR_TIMEOUT) + 专用 worker 线程常驻引擎 (OnceLock + mpsc), 初始化等待 (60s, 含模型下载) 与推理超时 (2s) 分离
- 接线: include_ocr 字段 (parser 重复报错) / ScreenshotManifest.ocr + WindowScreenshotManifest.ocr / composite 原点 virtual_bounds + window 路径原点 captured_os_rect (对裁剪图 OCR, 不跑整桌面) / primary 兼容入口显式拒绝
- 测试: control_ocr 单测 (reason code/JSON 载荷/坐标换算) + parser 测试 (含 WeChat 窗口入口用例) + 真机冒烟 (env 门控 #[ignore] 测试)
- 排障: E0433 (不点名 oar_ocr_core 类型, 字段访问内联 min/max); 冒烟超时假阳性 -> 磁盘满 (清 4.3GB); debug profile ONNX 慢 5-20x (冒烟须 --release)
- 体积: 无 oar 19M / 含 oar 46M (+27MB) 回填 spec §8

### 总结感悟
- 镜像 include_ax 模式接新层非常顺: 前期 schema/失败语义独立于引擎设计, 引擎反转后接线照画
- "环境缺失应显式失败而非跳过"纪律再次生效: 冒烟测试 env 缺失 panic 而非静默 pass
- 磁盘满会造成超时/断言类假阳性: 排障先查 df 再怀疑代码; 编译产物 (incremental/多 target) 是 mac 开发机磁盘杀手
- rg 的 -r 是 replace 不是 recursive, 用错会把输出整个换掉

## [2026-09-02 17:00:00] [Session ID: wayfinder-ocr-chart] 任务名称: live OCR 三件套 e2e 实现

### 任务内容
- 新增 tests/control_ocr_e2e.rs: 计算器三件套 + 负例, RDOG_OCR_LIVE_E2E 门控 + VIA_TERMINAL 模式
- ort 链接方式核实: 静态链接 (无 dylib 伴随, 主二进制自包含), spec §8 已回填

### 完成过程
- 复用 control_mouse_e2e/support.rs (ControlSession/start_daemon 模式) + control_ax_e2e 的 via-terminal 方案 (.command 脚本 + open -a Terminal + OAR_HOME export + 日志/端口就绪轮询)
- 排障: TCC 归属链问题 (agent shell spawn 的 daemon 无屏幕录制归属) -> 二进制按 install-signed 方案重签 identifier=rdog 仍不够 -> via-terminal 后错误显式化为 code 77 -> 需 Terminal 一次性授权
- 教训: e2e 门控测试不能用 --ignored 过滤 (env 门控测试直接跑); oar 模型缓存文件名是 pp-ocrv6_tiny_*.onnx 不是 det.mnn (与 rusto 混淆)

### 总结感悟
- repo 已有三层 live e2e 基建: opt-in env 门控 / via-terminal TCC 归属 / spawn 点 env 注入 checklist — 新 e2e 全部照搬零发明
- TCC 问题是权限归属链问题, 不是签名问题: 重签解决"跨构建保留", 解决不了"从哪生"

## [2026-09-02 18:00:00] [Session ID: wayfinder-ocr-chart] 任务名称: live OCR 三件套 e2e 验收

### 任务内容
- tests/control_ocr_e2e.rs (计算器三件套 + 负例, env 门控 + via-terminal) 实现与 live 调试
- python 驱动下三件套闭环一次通过; rust harness 不稳定项开票 #102

### 完成过程
- TCC 排障链: adhoc 随机身份 -> install-signed 方案重签 identifier=rdog -> Terminal 屏幕录制授权 -> via-terminal 模式捕获成功
- live 调试发现并修复: Calculator 重开恢复上次显示值 (显示区 "5" 污染按钮 "5" 定位) / 首次点击被窗口激活吞掉 / 遮挡窗口不进截图 (每截图前 activate) / 单帧 OCR 按钮抖动 (重拍 + conf>=0.5 + 候选对)
- 坐标偏移假说被全局画框验证推翻: OCR 框在全局坐标对位准确, 偏移是跨 run 窗口级联的假象

### 总结感悟
- live GUI e2e 的不稳定三兄弟: 窗口遮挡 / 焦点吞点击 / 位置级联 — 每次截图前 activate + 同帧断言 + 重拍是标准防御
- "推翻自己的偏移结论"靠的是全局坐标画框这个决定性实验: 假设要用可证伪实验检验, 不能靠连续推理
- TCC 归属链: 重签解决跨构建保留, via-terminal 解决"从哪生", 两者缺一不可

## [2026-09-02 19:40:00] [Session ID: wayfinder-ocr-chart] 任务名称: live 三件套闭环通过 + 陈旧帧缺陷发现

### 任务内容
- live OCR 三件套 (读/定位点击/fresh 验证 + 负例) 在 python 驱动下完整通过
- 发现窗口截图路径陈旧帧缺陷, 开票 #103; e2e 稳定化项在 #102

### 完成过程
- 多轮排障: 点击吞没 (activate 前置) / Calculator 状态恢复 (显示区残留污染定位 -> 按钮区域过滤) / 单帧按钮抖动 (重拍 + conf 过滤 + 候选对) / 坐标偏移假说 (全局画框证伪)
- 决定性对照: daemon window capture (README 旧帧) vs screencapture 真值 (Calculator "77") 同刻分叉
- 小图 2x 放大预处理落地 (借鉴 BK98), 大小图冒烟均过

### 总结感悟
- 陈旧帧缺陷的发现路径: OCR 定位失败 -> 不猜, 拿 screencapture 真值对照 daemon 输出 -> 同刻分叉即证据
- composite 有 stale 守卫而 window 没有, 是防御不对称; "fresh 证据"承诺要求所有截图路径同权新鲜
- 点击 "77" 是 OCR->点击精度最好的正面证据: 两次独立实验的框中心都精确命中按钮

## [2026-09-02 21:50:00] [Session ID: wayfinder-ocr-chart] 任务名称: #102 处理 - overlay 渲染调试与显示器休眠发现

### 任务内容
- 调试 overlay 面板 0x0 渲染问题; 发现显示器休眠/锁屏是最大混杂因素
- overlay 实现重构为子进程架构 + 主 runloop 手动冲刷; 完成面板创建与窗口注册 (正确 bounds/layer)

### 完成过程
- 诊断链: 面板对象自认配置成功 (frame/level/visible) 但 CGWindowList 0x0 layer0 -> 补 finishLaunching 无效 -> 全屏截图全黑实锤显示器休眠 -> caffeinate 唤醒恢复
- objc2 0.6 学习: mtm.alloc::<T>() / MainThreadMarker::new_unchecked 的 AppKit 风险 / runMode_beforeDate 手动 runloop / NSDefaultRunLoopMode 是 extern static 需 unsafe
- 发现 runloop 缺失问题: 不跑主 runloop, 窗口订购永不提交到窗口服务器 (注册 0x0)
- 诚实纠偏: #103 结论加了休眠混杂因素注释, 需亮屏重验

### 总结感悟
- "截图全黑 = 显示器休眠"应是 live GUI 测试的第一前置检查 (亮度均值一算便知)
- AppKit helper UI 的正道: 子进程承载主线程 AppKit, 父子间 stdin/IPC; 任何"后台线程 unchecked MTM"的取巧都会在真实 AppKit 校验下崩
- runMode_before_date 在无任务时立即返回, 30ms 预算不等于 30ms 等待, 循环需自控节奏
