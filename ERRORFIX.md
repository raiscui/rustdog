## [2026-08-17 16:25:00] [Session ID: omx-1786949079888-fq4u2i] 归档修正: 移除已失效的 LFM2.5 重试建议

### 现象
- `lfm25_ops` 归档 manifest 仍写有“本地服务就绪后重新运行完整矩阵”的未来待办。

### 原因
- 该归档生成时 LFM2.5 仍属于可重试范围,后来用户明确放弃该模型方向。

### 修复
- 删除 manifest 中的重试待办,明确归档只保留历史调查与验证事实。

### 验证
- 已重新阅读 manifest,确认不再存在 LFM2.5 后续执行入口。

## [2026-08-18 10:40:00] [Session ID: omx-1786949079888-fq4u2i] Bugfix: cached AX resource epoch source

### 现象
- AX snapshot 注册缓存时,`with_observation` 已经消费 `resource_epoch_capture`,缓存无法再直接读取 capture-start epoch。

### 原因
- 缓存实现错误地在 snapshot 缺少 capture 时回读当前 resource epoch,可能把旧 observation 误判为新鲜。

### 修复
- 缓存注册改为通过 `resolve_observation_resource_epoch` 从 observation store 获取创建时资源 epoch。

### 验证
- 新增缓存 round-trip、resource write 后 stale、unknown observation/ref 和 executor cache-hit 测试。
- `cargo check -j 2` 通过;定向测试通过;串行 `cargo test -j 2 -- --test-threads=1` 通过 924 项。

### 未完成边界
- 并行全量测试仍会触发现有全局 observation store 容量竞态;串行全量测试是本次完整验证口径。
## [2026-08-18 17:50:00] [Session ID: omx-1786949079888-fq4u2i] Qwen DashScope key 注入路径误判

### 现象
- Qwen 3.7 和 Qwen 3.6 首轮 suite 各为 0/8,artifact 显示 provider 未找到模型 API key。

### 候选假设
- 初始候选是 `DASHSCOPE_API_KEY` 缺失。备选解释是运行 runner 的 shell 没有加载仓库的 direnv 环境。

### 验证
- 检查 `.envrc` 发现它加载 `.envrc.private`。
- `direnv exec .` 动态读取到 `DASHSCOPE_API_KEY=present`。
- 使用 `direnv exec .` 启动 daemon 并重跑同一 runner: Qwen 3.7 和 Qwen 3.6 均 `successCount=8`,`runCount=8`。

### 结论
- "API key 不存在" 的初始假设被动态证据推翻。已确认问题是 shell 未继承 direnv,而非 DashScope 凭据缺失或 rdog 控制链故障。
- 后续 Qwen 测试命令必须显式使用 `direnv exec .`。

## [2026-08-19 15:23:42] [Session ID: omx-1787115582924-n1rbi7] durable observations 根目录无界增长

### 问题
- 默认 observation 根累计 7,510 个一级目录,大部分是没有 observation/selector 的测试空 store。
- 旧日期 cleanup 在收尾审查中发现会删除任意子目录,存在未知数据误删风险。

### 原因
- durable store 按 daemon name 隔离,旧 `open` 在 daemon 启动时立即物化目录和空文件。
- 普通集成测试生成一次性 daemon identity,却继承真实 HOME 且未关闭 durable observation。
- 单 store count/byte retention 不限制 sibling store 数量。
- cleanup 删除前缺少 `looks_like_observation_store` 识别门槛。

### 修复
- 新 store 延迟到第一次 record 才物化。
- 默认 store 使用日期目录并跨日移动同一状态;每小时清理 7 天前 store。
- root maintenance lock 与 daemon owner lock 保护移动和活动状态。
- 未知子目录只记录 `skipped_unknown_stores`,不进入删除分支。
- 普通集成测试关闭 durable observation,专项测试使用临时 `state_dir`。
- 旧空测试 store 经 dry-run、quarantine 和逐项复核后删除。

### 验证
- cleanup 未知目录测试 1/1,durable 测试 12/12,observation/config 测试 42/42。
- quarantine 复核 7,422 项,0 错误;68 个含 observation 的目录全部保留。
- 全量 nextest 936/936 通过;真实 HOME 根保持 88 个目录、23 MiB。

## [2026-08-19 15:23:42] [Session ID: omx-1787115582924-n1rbi7] 全量验证中的测试时钟与进程隔离错误

### 问题
- observe epoch 测试要求两次独立 wall-clock 取值相差不超过 1ms,全量和 exact 均稳定失败。
- recording E2E 并行时收到空 response,失败后还会遗留 daemon 子进程。

### 原因
- epoch 契约只要求等于 primary observation 创建时间,没有 1ms response 组装时限。
- recording 使用 host-global capture 能力,但 E2E 没有跨线程/跨进程串行保护;panic 又跳过显式 cleanup。

### 修复
- 删除无规格依据的 1ms 接近性断言,保留 epoch 等值和 fallback 等值测试。
- recording E2E helper 获取测试专用文件锁;guard Drop 在异常路径 kill/wait 子 daemon。

### 验证
- observe exact 1/1 通过。
- recording 普通并行 cargo test 5/5,nextest 5/5。
- 最终全量 nextest 936 passed,21 skipped;遗留的两个旧测试 daemon 和临时目录已清理。
## [2026-08-20 17:25:00] [Session ID: omx-1787115582924-n1rbi7] 问题: #54 successor changes contract 与 resource epoch

### 现象
- `@computer-act` 已返回 successor observation/target,但没有接入 #53 的 trusted changes decision。
- `successor_target.epoch` 使用 observation 创建时间,而下一次 PID-backed mutation 实际校验 resource epoch。
- 初版接入让 verify 与 changes-first 各自计算一次完整 AX diff。

### 原因
- pre snapshot 只为 verify 采集,且没有 observation metadata,无法通过 trusted identity gate。
- successor target 构造函数复用了 `created_at_unix_ms`,没有从 `observation_id + ref` 解析 resource lane snapshot。
- verification 与 changes-first 原先是两条独立消费路径,没有共享 #53 已产出的 `DiffReport`。

### 修复
- pre/successor 继续各采集一次,两者都记录 observation;同一对 snapshot 生成 changes/full/unavailable。
- successor target 从 observation store 读取对应 ref 的稳定 resource epoch。
- trusted changes 的 `DiffReport` 直接供 best-effort/always verification 使用;full fallback 才按需补算。
- executor 统一装配 successor、changes、postcondition 和 outcome;after capture 缺失时返回 unavailable/unknown,不伪造 successor。

### 验证
- `cargo check -j 2`: 通过,无 warning。
- 定向测试: computer-act 31/31、verify 18/18、changes-first 7/7。
- `cargo nextest run -j 2 --no-fail-fast`: 945 passed,21 skipped。
- 双轴复审: Standards `APPROVE`,Architecture `CLEAR`。
- solution frontmatter/claims: 通过,0 flags。

## [2026-08-20 18:05:00] [Session ID: omx-1787115582924-n1rbi7] 问题: cached AX query 生命周期错误码不稳定

### 现象
- cached @ax-get 对不存在 ref 或过期 observation 直接透传 observation store 的 STALE_REF / OBSERVATION_EXPIRED。

### 原因
- resolve_cached_ax_get 在读取 ref 前没有把内部 observation 生命周期错误映射到 cached query 的公共错误契约。

### 修复
- 在共享 cached query 入口统一映射: STALE_REF -> target_not_found,其他 observation 生命周期错误 -> stale_observation_cache。
- 增加 helper 与 executor 两层动态回归测试。

### 验证
- cargo test -j 2 --bin rdog control_ax::tests::cached_ax_get -- --nocapture: 4 passed。
- cargo test -j 2 --bin rdog control_actions::tests::cached_ax_get_executor -- --nocapture: 2 passed。
- cargo nextest run -j 2 --no-fail-fast: 949 passed、21 skipped。

### 测试环境发现
- `cargo test -j 2 --bin rdog` 的同进程并行测试会共享 64-entry observation singleton,使长时间 direct-ref 测试的 observation 被驱逐。exact 单跑通过,nextest 进程隔离全量通过。
- 该既有隔离问题已记录到 `LATER_PLANS.md`,没有通过扩大 production capacity 掩盖。
