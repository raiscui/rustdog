# 任务计划: 探索 rustdog 架构摩擦点

## [2026-08-20 12:20:00] [Session ID: omx-1787256000000-arch01] 架构摩擦点探索

### 目标
识别 rustdog 代码库中的真实架构摩擦点，不是理论违反，而是体验上的摩擦。

### 背景
- 热点区域: ax 模块 (最频繁改动)、observation 模块、runner 模块、control 模块
- 领域语言已在 CONTEXT.md 定义
- ADRs 在 docs/adr/ 中

### 探索策略
1. 从热点区域开始: ax、observation、runner、control
2. 阅读关键模块的代码结构
3. 应用 deletion test: 删除某模块会集中复杂度还是只是移动复杂度?
4. 注意测试覆盖: 哪些部分难以测试? 为什么?
5. 注意模块边界: 哪些地方职责不清晰?

### 阶段
- [x] 阶段1: 探索 ax 模块 (AX query/diff/changes)
- [x] 阶段2: 探索 observation 模块 (durable store/snapshot)
- [x] 阶段3: 探索 control_computer_act 模块 (mutation/verification)
- [x] 阶段4: 探索 control_observation 模块 (observation creation/scope)
- [x] 阶段5: 综合分析并输出摩擦点报告

### 当前状态
**已完成** - 识别出 4 个主要摩擦点，准备输出最终报告

## [2026-08-20 13:10:00] [Session ID: omx-1787115582924-n1rbi] #54 实施接续

### 目标
将 #53 的 trusted changes decision 接入现有 `@computer-act` successor response, 保持 dispatch、验证、postcondition 和 successor capture failure 的语义分离。

### 方案
- [ ] 方案 A: 复用唯一 pre/successor capture, 输出 `changes` 决策并补 unavailable/fallback 回归测试。
- [ ] 方案 B: 只扩展 verification, 不产出 changes wire。该方案不满足 #54, 不采用。

### 阶段
- [x] 读取现有 executor、verify 和 changes-first 实现
- [ ] 接入 response contract 与 failure 语义
- [ ] 补测试并验证
- [ ] review、提交、关闭 ticket

### 当前状态
**阶段 2** - 先确认 response 类型和测试 seam, 再进行最小代码修改。

## [2026-08-20 16:18:11] [Session ID: omx-1787115582924-n1rbi7] #54 实施恢复

### 目标
从 #53 的已验证 trusted changes decision 继续,将其接入 `@computer-act` 的唯一 successor capture 路径,并完成测试、审查、提交和 ticket 收尾。

### 待办
- [x] 恢复工作树、前置提交和 #54 现有分析。
- [ ] 先补 response contract 与 capture failure 的失败测试。
- [ ] 实现 `changes` 接入并修正 successor target epoch 契约。
- [ ] 运行定向测试、`cargo check -j 2` 和全量 nextest。
- [ ] 完成双轴 review,只提交 #54 文件并关闭 #54。

### 遇到错误
- `omx state status --json` 不是有效子命令。已改用 `omx state list-active --json`,确认当前没有遗留 OMX mode。

### 当前状态
**阶段 2** - 正在回读 changes-first 和 computer-act response seam,随后先写最小失败测试。

## [2026-08-20 16:35:00] [Session ID: omx-1787115582924-n1rbi7] #54 response 接入完成

### 阶段进展
- [x] 先补 response contract 与 capture failure 的失败测试。
- [x] 复用唯一 pre/successor capture 输出 `changes/full/unavailable`。
- [x] successor target 改为 observation store 的稳定 resource epoch。
- [x] changes-first 7/7、successor capture 6/6、computer-act 30/30。
- [x] `cargo check -j 2` 通过,修复并复验 1 个 unused import warning。
- [ ] 运行 binary 全量单测和 Standards/Spec 双轴 review。
- [ ] 修复 review findings 后运行最终全量 nextest、格式和 diff 检查。
- [ ] 只提交 #54 文件并关闭 ticket。

### 当前状态
**阶段 3** - response contract 已实现,正在扩大验证与审查。

## [2026-08-20 16:39:00] [Session ID: omx-1787115582924-n1rbi7] binary 全量测试失败调查

### 现象
- `cargo test -j 2 --bin rdog` 为 864 passed、1 failed、1 ignored。
- 失败项是 `control_actions::tests::direct_ref_mutation_executor_routes_cover_ax_window_and_web_commands`,错误为 PID-backed mutation 未进入 resource lane。

### 假设与验证计划
- 主假设: 并行单测共享全局 observation/resource state,新增 observation contract 测试与既有 direct-ref 测试发生干扰。
- 备选解释: successor target 的 resource epoch 改动影响了底层 mutation resolver。
- 推翻主假设的证据: 该测试 exact 单跑或与 successor 测试串行组合仍失败。
- 下一步: exact 单跑,再用单线程过滤组合复跑;证据确认后才决定是否修改生产代码。

## [2026-08-20 16:48:00] [Session ID: omx-1787115582924-n1rbi7] binary 并行失败结论

### 已验证结论
- 失败项 exact 单跑 1/1 通过,生产 mutation resolver 没有因 #54 epoch 改动而退化。
- 移除本轮纯 response 测试的全局 store 写入后,full cargo test 仍失败,且驱逐位置从 resize 后移到 web-act。
- `OBSERVATION_STORE` 是 test binary 内共享 singleton,默认容量 64;并行 suite 会在长测试执行期间记录超过容量的 observation,使测试自己的 id 被驱逐。

### 决定
- 不扩大 production/test 默认容量,不在 #54 引入跨模块全局测试锁。
- 正式全量门禁继续使用项目约定的 cargo-nextest 进程隔离。
- #54 保留真实 resource epoch 的 exact 回归测试,纯 response 测试使用手工 header,避免无意义污染 singleton。

### 当前状态
**阶段 3** - 等待 Standards/Spec 独立 review,随后修复 findings 并运行 nextest。

## [2026-08-20 17:10:00] [Session ID: omx-1787115582924-n1rbi7] review findings 修复

### Review 结果
- Standards: `REQUEST CHANGES`,指出 executor 组合 contract 测试不足,以及 verify/changes 重复计算完整 AX diff。
- Architecture: `WATCH`,同样要求补齐真实 response assembly contract。

### 已完成修复
- trusted changes decision 现在携带唯一 `DiffReport`;best_effort/always verification 直接复用。identity fallback 为 full 时,仅 verify 请求完整报告才补算 diff。
- executor 的 successor、postcondition、outcome 装配收敛到同一生产函数。
- 新增 after-capture failure 组合测试,断言 base id、changes unavailable、postcondition unavailable、outcome unknown,且不伪造 successor/target。
- 成功 contract 同时断言 base/successor observation id、target observation id 和稳定 resource epoch。
- `cargo check -j 2`、新增 exact test 和 successor capture 5/5 通过。

### 当前状态
**阶段 3** - 正在进行 findings 窄范围复审。

## [2026-08-20 17:25:00] [Session ID: omx-1787115582924-n1rbi7] #54 实施完成

### 最终状态
- [x] response contract 与 capture failure 测试。
- [x] trusted changes/full/unavailable 接入。
- [x] successor target 稳定 resource epoch。
- [x] 单一 DiffReport 复用,无重复 trusted diff。
- [x] `cargo check -j 2` 和定向测试。
- [x] 双轴 review 修复并复审通过。
- [x] 全量 nextest 945 passed、21 skipped。
- [x] Scoped Refresh 与 solution 校验。
- [ ] 仅暂存 #54 文件、提交并关闭 ticket。

### 当前状态
**阶段 4** - 代码和知识验证完成,正在执行 scoped commit 与 tracker 收尾。

## [2026-08-20 17:45:00] [Session ID: omx-1787115582924-n1rbi7] #54 全部完成

### 完成项
- [x] scoped commit `479adb6`。
- [x] #54 实现证据评论与 CLOSED 状态核验。
- [x] #55 blockers #52/#53/#54 全部 closed。
- [x] 父 #51 保持 OPEN。
- [x] 无 staged 残留,其他会话工作树改动保持不动。

### 状态
**完成** - #54 已实现、验证、审查、提交并关闭。下一张已解锁 ticket 是 #55。

## [2026-08-20 17:47:58] [Session ID: omx-1787115582924-n1rbi7] #55 对抗式验证实施

### 目标
为 cached AX query 与 successor changes 建立可复跑的静态门禁,再运行固定 2-case x 5-remote-model canary。结果只作为 operational evidence,不替代完整 5 x 8 产品认证。

### 方案
- [x] 方案 A: 复用现有 resource lane、cache、changes-first、output budget 测试 seam,仅补验收缺口,并复用评测仓库 runner 执行 canary。
- [ ] 方案 B: 新增独立验证框架或生产诊断接口。现有 seam 足够时不采用,避免平行真相源。

### 待办
- [x] 恢复 #54 收尾状态并核对 #55 blockers、正文和工作树。
- [ ] 建立 #55 acceptance criteria 到现有动态测试的覆盖矩阵。
- [ ] 先补失败测试,覆盖缺失的交错、错误 reason code 和精确 output boundary。
- [ ] 运行定向测试、`cargo check -j 2` 和全量 cargo-nextest。
- [ ] 固定并运行 2-case x 5-remote-model canary,生成 request count、Post-action Evidence 和 case success 对比。
- [ ] 完成双轴 review、只提交 #55 文件、评论并关闭 #55,父 #51 保持 OPEN。

### 当前状态
**阶段 2** - 正在盘点现有测试覆盖,确认真正缺口后再编辑代码。

## [2026-08-20 18:05:00] [Session ID: omx-1787115582924-n1rbi7] #55 验证与实现收口

### 阶段进展
- [x] acceptance criteria 与现有测试 seam 对照完成。
- [x] cached @ax-get 统一 target_not_found / stale_observation_cache reason code,补 executor 动态覆盖。
- [x] 输出预算补齐精确 byte/line boundary 与 UTF-8 boundary 覆盖。
- [x] 定向测试通过: resource 5、cache 4、changes 7、budget 4、actions 36。
- [x] cargo check -j 2、rustfmt、全量 cargo nextest run -j 2 --no-fail-fast 通过: 949 passed、21 skipped。
- [x] 评测 runner 单测通过。
- [ ] 当前 binary 的 2-case x 5-remote-model canary 未能启动: runner 引用的 upstream Pi /Users/cuiluming/Library/pnpm/pi 不存在。历史 2026-08-15/16 结果不能替代当前 binary 证据。
- [ ] 不关闭 #55,保留外部 canary blocker;父 #51 保持 OPEN。

### 当前状态
**阶段 4** - 代码验证完成,正在写账本、做 scoped diff review 和提交;产品 canary 留待 Pi binary 恢复后复跑。

## [2026-08-20 18:12:00] [Session ID: omx-1787115582924-n1rbi7] #55 当前收尾状态

### 完成项
- [x] 3 个 #55 代码/测试文件已 scoped commit: 8af9e12 test(ax): harden cached query validation。
- [x] Issue #55 已发布实现、验证和 canary blocker 评论。
- [x] 其他会话改动未暂存、未撤回;父 #51 保持 OPEN。

### 未完成项
- [ ] 当前 upstream Pi binary 缺失,无法生成当前 binary 的 2-case x 5-remote-model ledger,因此 #55 暂不关闭。

### 当前状态
**阻塞于外部评测依赖** - 代码实现和静态/动态门禁已完成;恢复 /Users/cuiluming/Library/pnpm/pi 后应从 canary 步骤继续,不能把历史 artifact 当作当前认证。
