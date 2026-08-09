# 任务计划: 修复 Xiaohongshu AXWebArea 识别与首页语义点击

## [2026-07-24 10:48:47] [Session ID: omx-1784789038072-clve0o] [计划]: 建立动态证据后执行 RED -> GREEN

### 目标

在不使用坐标 fallback、不绕过页面安全验证的前提下,让 rustdog 对正确 Xiaohongshu Chrome 窗口的 `@web-find text:"\u9996\u9875"` 返回语义目标,并让 `@web-act action:"press"` 产生 `performed:true` 与 fresh 后验证。

### 阶段

- [x] 阶段1: 回读 Bonsai 0/5 历史证据,登记跨仓库索引,开启独立支线上下文
- [x] 阶段2: 读取 rustdog 领域约束、Web cookbook 和历史经验,建立 red-capable live 只读探针
- [x] 阶段3: 用同一 fresh window id 至少3次对比 targeted observe、`@ax-find role:AXWebArea` 与 `@web-find`
- [ ] 阶段4: 排序3到5个可证伪假设,再用 CodeGraph 追踪已发生的失败调用链
- [ ] 阶段5: 在经确认的公共 seam 写 RED 回归测试,实现最正确修复,完成精确与相关 nextest 验证
- [ ] 阶段6: 重跑 live 探针和语义 press,用 fresh AX/window/URL 证据证明首页点击
- [ ] 阶段7: 回到 Bonsai-demo,用 llama-server 与全新输出目录重跑 1 warmup + 5 scored
- [ ] 阶段8: 更新后缀 notes/WORKLOG/ERRORFIX,回溯 LATER_PLANS/EPIPHANY_LOG,把摘要迁回主线

### 两个执行方向

1. 不惜代价的正确方案:先构建可重复 live 探针,再用真实 bug pattern 的 RED 测试锁定修复,完成 rustdog live 回归和 Bonsai 1+5。
2. 先能用再优雅:在评测 runner 里等待或改用坐标点击。这会掩盖 `AX_WEB_AREA_NOT_FOUND`,也破坏语义和窗口所有权门禁,本轮不采用。

### 当前现象、假设与反证标准

- 已观察现象:Bonsai 5个 scored 样本都导航到 `xiaohongshu.com/explore`,但 `@web-find` 在 `find-ax-web-area` 阶段返回 `AX_WEB_AREA_NOT_FOUND`。
- 主假设:target window 过滤之前全局 AX snapshot 被截断,使目标窗口的 `AXWebArea` 没有进入后续搜索集合。
- 最强备选解释:Xiaohongshu 页面在查询时尚未向 macOS Accessibility 暴露 `AXWebArea`,与 rustdog 遍历无关。
- 推翻主假设的证据:如果未截断的 targeted `@ax-find role:"AXWebArea"` 也在多个延时点稳定返回0,则不能把缺失归因于 snapshot 截断。

### 安全和不变量

- 每一轮都重新获取 fresh window id;不复用历史 `pid:63270/window:0`。
- 不使用坐标点击,不绕过登录、验证码、人机校验或其他安全状态。
- 不修改用户真实 `~/.pi/agent` 配置,不 reset 或清理用户和其他 Session 的 dirty worktree。
- 动作必须先证明目标窗口唯一且 fresh;side effect 后必须重新观测。

### 状态

目前在阶段2:读取 Web/AX 相关历史经验与 cookbook,然后创建快速、可重复、只读的 live 反馈循环。

## [2026-07-24 11:00:00] [Session ID: omx-1784789038072-clve0o] [行动]: 建立当前 daemon 与 fresh Chrome 基线

- 历史证据确认 2026-05 的同一 Xiaohongshu 场景曾由 `refresh-web-area-subtree` 成功找到唯一 `AXLink.description:"\u9996\u9875"`;这只是回归对照,不代表当前版本仍正常。
- 先执行当前已安装 `rdog control @ping`,然后用只读 `@window-find` 和 targeted AX 查询建立 fresh 窗口基线。
- 如果当前 focused 窗口不是 Xiaohongshu,只在唯一所有权门禁通过后打开一个新标签并导航;未通过时不执行任何 side effect。
- 探针必须保存命令、stdout/stderr、解析响应与同一 window id,并对用户精确症状做自动断言。

## [2026-07-24 12:44:05] [Session ID: omx-1784789038072-clve0o] [错误与调整]: 首版探针合并 frame 后超时

- 现象:首版脚本把 targeted observe、`@ax-find` 和 `@web-find` 放在同一 `rdog control` 调用里,20秒后抛出 `subprocess.TimeoutExpired`。
- 证据边界:该命令没有生成可解析的3条完整响应,所以不能用于判断哪一步是产品阻塞。
- 调整:每条只读命令改为独立子进程、独立计时和独立落盘;targeted observe 收窄为 `ax_mode:"windows"`,深度检索只交给 `@ax-find`。
- 超时应保存已获得的 partial stdout/stderr,同时标记该轮为 infrastructure blocker,不伪装成红灯。

## [2026-07-24 12:46:33] [Session ID: omx-1784789038072-clve0o] [阶段完成]: live 只读红灯为 3/3

- [x] 探针通过 Ruff 和 `py_compile`,可由单一命令无人值守重跑。
- [x] 三轮都在 `pid:63270/window:0` 上完成;targeted observe 未混入另一扇 Chrome 窗口。
- [x] 三轮 `@ax-find` 都返回唯一 `AXWebArea`,三轮 `@web-find` 都返回 `AX_WEB_AREA_NOT_FOUND`。
- [x] 页面加载时序假设已被当前证据推翻;不再用额外 sleep 堆叠假修复。
- 动态证据目录:`target/rdog-debug/xhs-web-area/probe-20260724-124542`。

### 当前状态

阶段4进行中:按 `notes__xhs_web_area.md` 的4个排序假设追踪静态调用链,先确认失败轮真正执行的 capture、window resolution 与 subtree refresh 路径。

## [2026-07-24 12:50:00] [Session ID: omx-1784789038072-clve0o] [最小证伪实验]: 只提高 web-find 全局 snapshot 预算

- 静态路径:`build_default_web_find_response_json` 无条件调用 `capture_default_ax_snapshot`,再在 `resolve_web_matches` 里选 target window。
- 对照路径:`@ax-find window_id` 通过 `capture_ax_find_snapshot_with` 直接调用 targeted window capture,三轮都找到 WebArea。
- 本实验只把同一窗口 `@web-find` 的 `max_elements` 从默认2000提到5000,不改窗口、匹配文本、roles、页面或时序。
- 预测:如果因全局截断丢失 WebArea,提高预算后 trace 应进入 `find-ax-web-area:ok`,并由既有 WebArea subtree refresh 找到首页。
- 推翻条件:如果5000预算下 snapshot 不截断且仍 `AX_WEB_AREA_NOT_FOUND`,则主假设不成立,应转向 window mapping 或 WebArea 过滤错误。

## [2026-07-24 13:05:00] [Session ID: omx-1784789038072-clve0o] [根因确认与口径回滚]: explicit target 未下推到 AX capture

- [x] 动态证据:fresh 3/3 的 global web snapshot 是 `truncated:true`,选中正确主窗口但没有 WebArea;targeted ax snapshot 紧邻返回 `truncated:false,match_count:1`。
- [x] 静态证据:`build_default_web_find_response_json` 总是 global capture,之后才选显式 target;WebArea 缺失时不会进入已有 subtree refresh。
- [x] 已验证根因:显式 window-scoped web target 未参与 capture root 选择,共享全局元素预算在目标 WebArea 展开前耗尽。
- [x] 时序解释已推翻;文本/role 过滤未参与 WebArea 的更早失败,也已排除。
- 口径回滚:数分钟后2500预算实验重用了不再 fresh 的 window id,其 transient window 结果不用于根因判断;只作为短期 identity 风险记录。
- [x] 阶段4完成。

### RED seam 决定

- 选择 `build_default_web_find_response_json` 的完整 response 行为,通过 capture dependency injection 注入 global 与 targeted fixture。
- fixture 复现真实 bug pattern:同一 target window 在 global fixture 中截断且无 WebArea,在 targeted fixture 中含 WebArea 和首页。
- 同时增加 active-browser 对照,确保没有显式 `window_id/window_ref` 时仍走 global capture。
- 这一 seam 已由用户对整体 RED -> GREEN 计划的连续 `确认/继续` 授权覆盖;本轮不新增与用户行为无关的内部测试。

### 当前状态

阶段5进行中:先编写一个失败的完整 response 回归测试,运行精确 nextest 证明 RED,然后才修改生产 capture 选择。

## [2026-07-24 13:00:06] [Session ID: omx-1784789038072-clve0o] [记录更正与 RED]: capture 选择测试已稳定失败

- 记录更正:上一条 task plan 和 `notes__xhs_web_area.md` 的 `13:05:00` 是手工预估时间,比实际 `date` 输出提前了5分钟;该两条的技术内容保留,但时间归属以本更正记录为准。
- 行为不变的 seam refactor 完成后,`test(control_web)` 为19 passed。
- RED 命令:`rtk cargo nextest run --package rustdog --bin rdog -E 'test(web_find_explicit_window_should_capture_target_before_global_budget_truncation)'`。
- RED 结果:0 passed,1 failed;`global_capture_calls` 实际为1,期望为0,targeted capture 未发生。
- 这是对真实 bug pattern 的断言失败,不是缺少 symbol 或 fixture 无法编译。
- 下一个垂直切片只修改 capture root 选择,不改 matcher、roles、refresh 和 action 逻辑。

## [2026-07-24 13:03:31] [Session ID: omx-1784789038072-clve0o] [GREEN]: explicit target 使用 targeted AX window capture

- [x] `capture_current_ax_window_snapshot` 改为 crate-visible,复用 `@ax-find window_id` 已验证的同一 capture 真相源。
- [x] 新的 `control_web::capture` 模块集中 Web capture 编排,避免继续扩大已超1000行的 `control_web.rs`。
- [x] 显式 `target.window_id` 与可解析 window ref 优先 targeted capture;active-browser 继续 global capture。
- [x] targeted capture 失败时回退 global,使既有 `BROWSER_WINDOW_NOT_FOUND` / `WINDOW_REF_INVALID` 结构化 blocker 仍由原 response 路径生成。
- [x] RED 回归已转绿:1 passed,613 skipped。
- [x] active-browser 不变量测试:1 passed,614 skipped。
- [x] `test(control_web)`:21 passed,594 skipped。
- [x] `cargo check --package rustdog --bin rdog --quiet`:0 error;6个 warning 来自本次未修改的 `control_actions` / `control_computer_act` 既有模块,本支线不擅自扩大修复范围。

### 结构气味记录

- `src/control_web.rs` 原有1102行,`src/control_ax.rs` 原有2621行,已超仓库建议上限。
- 本轮只把新 capture 编排落在小模块,没有顺手重构整个 AX backend;任务收口时会单独给出优化建议。

### 当前状态

生产代码的静态 GREEN 已完成,但 live daemon 仍是旧安装版。下一步先通过原 `rdog-daemon` tmux 终端干净停止,安装当前工作树,再在同一终端重启并跑 fresh live 探针。

## [2026-07-24 13:10:56] [Session ID: omx-1784789038072-clve0o] [继续执行]: live GREEN 与首页语义点击

- 从已安装当前工作树并重启 daemon 的 checkpoint 继续,不重复已经完成的根因分析和 RED -> GREEN。
- 先重新获取当前左屏 Chrome inventory,只接受 fresh、focused、标题为 Xiaohongshu、几何为主窗口的唯一 window id。
- 用 fresh id 重跑3轮只读探针。停止条件是3/3 `@web-find text:"首页"` 成功,且不再出现 `AX_WEB_AREA_NOT_FOUND`。
- 随后重新获取 window id,用 `@web-find` 返回的语义目标执行 `@web-act action:"press"`,禁止坐标 fallback。
- side effect 必须由 fresh window、AX、URL及页面内容变化证据验收。登录、验证码或人机校验出现时 fail closed。
- live 动作通过后再运行 control_web/control_ax 相关 Rust 验证,最后回到 Bonsai-demo 启动 llama-server 并从全新目录执行 `1 warmup + 5 scored`。

### 当前状态

阶段6进行中:先确认 daemon 和 fresh Xiaohongshu 主窗口,再运行 live GREEN 探针。

## [2026-07-24 13:14:44] [Session ID: omx-1784789038072-clve0o] [live GREEN 部分通过]: WebArea 已修复,首页匹配仍未通过

### 现象

- 新导航后的 fresh Xiaohongshu 主窗口为 `pid:63270/window:0`,标题和1470x863几何匹配。
- 3/3 targeted `@ax-find role:"AXWebArea"` 成功。
- 3/3 `@web-find` trace进入 `find-ax-web-area:ok`,原 `AX_WEB_AREA_NOT_FOUND` 为0/3。
- 3/3 页面匹配及 subtree refresh返回0,新错误为 `WEB_MATCH_NOT_FOUND`;探针按合同退出2。

### 当前假设与验证

- 主假设:WebArea定向刷新预算或web matcher字段/role没有覆盖左侧“首页”,但更深targeted AX树仍有该节点。
- 最强备选:当前页面标题正确,但页面正文处于登录、错误或未完整渲染状态,AX树本身没有“首页”。
- 最小验证:重新获取 fresh window id,对同窗口执行大预算 `@ax-find text:"首页"`,并保存 fresh screenshot/AX后验。
- 推翻主假设:如果大预算 targeted AX查询同样为0,就不能修改web matcher;应先解释当前页面状态。

### 状态

阶段6继续:原WebArea根因的 live 修复已经成立,但“首页”动作验收尚未成立,不会把部分GREEN报告成完成。

## [2026-07-24 13:16:30] [Session ID: omx-1784789038072-clve0o] [环境门禁]: 当前页面无“首页”且登录遮罩不可语义关闭

- 完整targeted AX抓取694个元素且`truncated:false`,没有“首页”;因此上一条“更深AX有首页”的主假设不成立。
- fresh截图显示当前`/explore`左侧首项为“发现”,页面中央存在手机号登录遮罩。
- `@web-find text:"发现"`找到唯一左侧`AXLink`,证明WebArea refresh和page matcher当前正常。
- 登录遮罩没有可识别的`AXDialog`或page-owned“关闭”控件;全窗口唯一`description:"关闭"`是Chrome标签页关闭按钮,不能误用。
- 在不使用坐标、不触碰登录按钮的约束下,当前不能安全执行背景“发现”链接,也不能把它虚报为文字“首页”点击。
- 先完成不依赖页面状态的Rust相关验证和代码审查。live动作与Bonsai 1+5等待页面登录/遮罩门禁恢复后继续。

## [2026-07-24 13:20:38] [Session ID: omx-1784789038072-clve0o] [审查调整]: 补齐 capture 不变量测试

- 限定diff审查确认新模块无坐标、mouse或click fallback。
- 现有测试已覆盖显式window id targeted capture和active-browser global capture。
- 增加两个测试:可解析window ref也必须targeted capture;targeted backend失败必须回退global并返回原结构化blocker,不能泄漏IO错误。
- 只改测试,不扩大生产修复。完成后重跑`test(control_web)`、fmt和diff check。

## [2026-07-24 13:22:23] [Session ID: omx-1784789038072-clve0o] [最小交互实验]: 用Esc关闭可选登录提示

- fresh截图显示登录遮罩存在明确X,表明它可被普通用户关闭;本实验不提交手机号、验证码或登录动作。
- 先fresh查询并激活唯一Xiaohongshu主窗口,只发送一次`Esc`。
- 验证门禁:modal中的登录按钮应消失,侧栏登录按钮保留;fresh截图遮罩消失,URL和窗口所有权不变。
- 如果Esc无效,立即停止,不改用坐标或点击无语义X。

## [2026-07-24 13:23:53] [Session ID: omx-1784789038072-clve0o] [执行错误与回滚]: 手工复用了已经重绑的window id

- fresh `@window-find` 返回Xiaohongshu主窗口`pid:63270/window:1`,但后续激活命令错误沿用上一轮`pid:63270/window:0`。
- `/window:0`当时已经重绑为84x77 transient窗口,激活明确返回`WINDOW_FOCUS_NOT_ACQUIRED`。
- 随后的单次Esc虽然伴随截图中遮罩消失,但动作没有通过正确target门禁,不能作为targeted成功证据。
- 本轮立即回滚“targeted Esc成功”口径,不重复Esc,也不把该动作计入验收。
- 修正方式:用单一Python验收脚本从fresh window-find响应自动提取window id,同一进程执行find/action/fresh后验,禁止手工复制短期id。

## [2026-07-24 13:27:23] [Session ID: omx-1784789038072-clve0o] [新根因确认]: web-act漏接targeted capture策略

### 动态证据

- 同一fresh `window_ref + observation_id`先由`@web-find`找到唯一“发现”。
- 紧邻`@web-act`将ref解析到同一`pid:63270/window:1`,但响应快照为`ref_count:2013,truncated:true`,并返回`BROWSER_WINDOW_NOT_FOUND`。
- 动作为`performed:false`,live脚本正确fail closed。

### 静态证据

- `build_default_web_act_response_json`仍无条件调用`capture_default_ax_snapshot`。
- action retry与full-snapshot verification也通过同一global capture closure。
- 因此window id重绑这一备选解释被同ref解析结果和静态入口共同排除。

### RED -> GREEN计划

- 先做行为不变seam refactor,让default web-act的global/window/action/refresh依赖可注入,并确认现有23个control_web测试保持绿灯。
- 新RED复现global fixture截断无WebArea、targeted fixture含唯一首页链接;断言显式窗口动作不调用global且`performed:true`。
- GREEN时让web-find和web-act共享capture模块的target选择,包括initial、retry和full-snapshot verification;active-browser仍global。

## [2026-07-24 13:34:38] [Session ID: omx-1784789038072-clve0o] [GREEN]: web-act共享targeted capture

- RED精确失败为`global_capture_calls left:1 right:0`;GREEN为1 passed。
- `@web-act`的initial、retry和full-snapshot verification现在复用`capture_web_snapshot_with`,与`@web-find`共享target选择真相源。
- 完整`test(control_web)`为24 passed;`test(control_ax)`为30 passed。
- fmt、diff check通过;cargo check为0 error。6个warning仍来自其他并行修改模块。
- 下一步安装当前二进制,从`rdog-daemon`原tmux终端干净停止并重启,再跑fresh原子live动作。

## [2026-07-24 13:38:19] [Session ID: omx-1784789038072-clve0o] [live完成]: 当前homepage语义入口已点击并刷新

- 原子脚本从fresh window-find自动取得`window_ref + observation_id`,未手工复用window id。
- 当前站点UI漂移:原prompt的“首页”不存在,当前homepage入口为唯一`AXLink.description:"发现"`;artifact记录`labelDrift:true`。
- `@web-act`返回`performed:true,verified:true,status:"complete"`,动作target与verification target id一致。
- fresh后验仍是同一1470x863 focused Xiaohongshu主窗口,重新定位“发现”为1个匹配。
- `imgdiff`检测586205个差异像素;前后截图显示URL增加`channel_id=homefeed_recommend`,瀑布流卡片整体刷新。
- [x] 阶段6完成。下一步进入阶段7,回到Bonsai-demo适配显式UI标签漂移并执行全新1+5。

## [2026-07-24 14:18:00] [Session ID: omx-1784789038072-clve0o] [阶段7失败诊断]: 首页文本子串歧义与同URL标签复位歧义

### 动态现象

- 新 suite 的 warmup 有5个Pi turn,scored-01有7个Pi turn,已证明样本内部确实是多轮agent chat/tool loop。
- scored-01的`@web-find text:"发现"`返回两个`AXLink`:侧栏精确文字`发现`,以及包含`发现`的笔记标题。
- `@web-act`因此返回`WEB_MATCH_AMBIGUOUS`,明确`performed:false,verified:false`。
- 本轮tab数从56增至57,但baseline和新标签都在`xiaohongshu.com/explore`,active tab description与URL相同,reset按`active_tab_did_not_change`安全停止。

### 已验证结论与修复边界

- rustdog matcher当前只有大小写不敏感`contains`,`WebFindQuery`没有精确匹配模式。动态歧义与静态调用链一致。
- runner的reset门禁没有误判实现错误;测试baseline本身与目标同URL,导致无法证明当前active tab身份。不能通过删除门禁修复。
- rustdog采用向后兼容的`mode:"exact"`;默认仍为contains。
- Pi评测system prompt使用exact与AXLink role。runner准备一个与目标不同的专用baseline,使`Cmd+W`复位有可验证的active-tab变化。

### 下一步

- [ ] 为exact parser和exact-vs-contains行为写RED测试。
- [ ] 实现类型化match mode,完成control_web精确nextest、完整control_web与cargo check。
- [ ] 重装并重启rdog daemon,用当前含`发现`笔记标题的页面做live exact验证。
- [ ] 更新Pi prompt与runner baseline准备,重跑Python测试和只读preflight。
- [ ] 从全新目录重跑1 warmup + 5 scored,逐样本核验多轮turn与GUI证据。

## [2026-07-24 14:22:00] [Session ID: omx-1784789038072-clve0o] [RED -> GREEN]: Web文本精确匹配模式

- RED精确测试稳定失败:`@web-find.match 不支持字段: mode`;0 passed,1 failed。
- 新增类型化`WebTextMatchMode::{Exact,Contains}`,默认`Contains`保留既有协议行为。
- `mode:"exact"`对description/name/value执行大小写不敏感的完整相等匹配,不再把包含短标签的长笔记标题计为候选。
- exact行为、默认contains兼容、未知mode错误3个精确测试通过。
- 完整`test(control_web)`、`test(control_ax)`、cargo check、fmt check和diff check通过。
- 下一步重装当前rustdog二进制并重启既有daemon,然后在当前真实页面上对比contains与exact候选数并执行一次exact语义press。

## [2026-07-24 14:24:00] [Session ID: omx-1784789038072-clve0o] [验证口径回滚]: 完整control_web尚未通过

- 上一条记录中"完整test(control_web)、test(control_ax)、cargo check通过"不成立。
- 直接工具输出被RTK过滤为空,但tee日志显示两个`WebFindQuery`测试initializer缺少`mode`,完整test编译失败。
- targeted三个测试通过的结论仍成立;扩大验证必须修复fixture后重新执行,不能把空输出当成功。

## [2026-07-24 14:28:00] [Session ID: omx-1784789038072-clve0o] [daemon生命周期]: 旧tmux随直接进程退出

- 已向原`rdog-daemon` pane `%20`发送Ctrl+C。
- 该session以rdog作为直接进程,daemon退出后tmux session自动销毁,因此无法在同一pane重启。
- 下一步先确认旧local-default不再响应,然后安装当前工作树并创建同名新tmux session;不得同时保留两个daemon。

## [2026-07-24 14:36:00] [Session ID: omx-1784789038072-clve0o] [live exact GREEN]: 真实页面从2个contains候选收敛为1个exact入口

- 已安装当前工作树并重启`rdog-daemon`,新pane为`%22`;真实ping返回`@response "pong"`。
- live只读artifact:`target/rdog-debug/xhs-web-area/exact-match-live-20260724-141926`。
- 同一fresh 1470x863 Chrome窗口中,contains `发现`返回2个AXLink;exact返回唯一`AXLink.description:"发现"`。
- 本探针`performed:false`,没有产生页面副作用;最终side effect交给Pi多轮suite验证。
- [x] exact parser/行为RED -> GREEN与完整Rust验证完成。
- [x] daemon安装、重启和live exact验证完成。
- [ ] 等待Bonsai 1+5结果后收口跨仓库日志。

## [2026-07-24 15:48:10] [Session ID: omx-1784789038072-clve0o] [回归续办]: active-browser capture与bare key兼容

### 新动态证据

- Bonsai v5中,`target:{browser:"active"}`已选中正确focused小红书Chrome,但global snapshot缺少AXWebArea并返回`AX_WEB_AREA_NOT_FOUND`。
- Codex direct对照artifact:`Bonsai-demo/.scratch/pi-bonsai-rdog-xhs/artifacts/06/codex-direct-rdog-compare-v3-20260724-155500`。
- 同一页面active exact find为0;fresh显式window exact find为1;显式web-act为`performed:true`,`verified:true`;动作后验与reset均通过。
- Pi/Bonsai 8B在warmup与一个scored样本把quoted key命令截断为`rdog control '@key:`,rdog尚未收到该调用。

### 目标与执行顺序

- [ ] 用CodeGraph刷新`capture_web_snapshot_with`和active target选择路径。
- [ ] 写RED测试:global只含唯一focused Chrome窗口且无WebArea,targeted capture含唯一"发现"链接;active请求最终必须使用targeted结果。
- [ ] GREEN实现两阶段capture:global只负责active窗口消歧,fresh window id再做targeted capture;ambiguous/not-found保持原blocker,targeted失败回退global。
- [ ] 写RED测试并实现bare key短语法:`@key:Cmd+T`,`@key:Return`,`@key:Esc`;quoted/object语法保持兼容。
- [ ] 跑精确nextest、完整control_web/control_protocol、fmt与cargo check,不修改其他Session文件。
- [ ] 安装当前工作树,干净重启唯一daemon,live验证active exact find与bare key。

### 状态

当前只读代码与测试上下文。尚未修改生产代码。

## [2026-07-24 15:54:00] [Session ID: omx-1784789038072-clve0o] [静态GREEN]: active two-stage capture与bare key

- RED:两个精确测试0 passed,2 failed。active路径targeted调用实际0次;bare `Cmd+T`被parser拒绝。
- GREEN:同两项精确测试2 passed。
- 扩大验证:`test(control_web) | test(control_protocol)`共100 passed,521 skipped。
- `cargo fmt --all -- --check`通过;`cargo check --package rustdog --bin rdog`为0 errors。
- cargo check显示6个既有warning,均来自其他Session正在修改的`control_actions`/`control_computer_act`;本支线不改动这些文件。
- [x] active请求先global消歧唯一浏览器,再用fresh backend id做targeted capture。
- [x] ambiguous/not-found保留global snapshot;targeted backend失败回退global,既有结构化blocker不变。
- [x] bare key只接受非空且无空白payload;quoted/object语法测试继续通过。
- [ ] 从原`rdog-daemon` tmux终端停止旧进程,确认退出后安装、重启单一daemon并live验证。

## [2026-07-25 01:30:24] [Session ID: omx-1784789038072-clve0o] [支线完成]: active capture与Pi 5/5闭环

- [x] 旧daemon从原pane `%22`停止,确认local-default registry消失后安装当前工作树。
- [x] 新唯一daemon在pane `%26`运行,PID 39508,ping返回pong。
- [x] live bare key、active exact find、active web-act和verified reset全部通过。
- [x] rdog-control skill升至1.8,同步bare key协议和示例;Pi skill路径为该文件symlink。
- [x] 下游Bonsai v6完成1 warmup + 5 scored;正式5/5 success,6/6 performed/verified/reset。
- [x] rustdog daemon保持运行;未commit/push,未修改或清理其他Session工作树内容。

### 状态

本支线目标已完成。长期结构气味仍是`control_web.rs`和`control_ax.rs`超出建议行数,未在本次bug fix中扩大重构。
