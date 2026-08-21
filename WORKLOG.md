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
