# 任务计划: macOS Apple 菜单鼠标 E2E

## [2026-05-14 12:24:10] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 创建测试任务

### 目标
- 新增一个默认 ignored 的 macOS E2E 测试。
- 测试通过真实 `rdog daemon` + `rdog control` 路径执行鼠标移动和点击。
- 动作序列: 截图读取 manifest -> 移动到左上角 Apple 图标 -> 点击 Apple 图标 -> 点击“关于本机”。

### 阶段
- [x] 阶段1: 读取现有 control lane / screenshot / mouse smoke 测试结构
- [x] 阶段2: 实现 macOS-only ignored E2E
- [x] 阶段3: 运行格式化和目标编译/测试验证
- [ ] 阶段4: 记录工作结果

### 关键约束
- 真实鼠标副作用测试必须 `#[ignore]`,不能默认运行。
- 坐标从 screenshot manifest 的 `os-logical` 语义推导,不新增第二套坐标解释。
- 该测试依赖 macOS Accessibility 和 Screen Recording 权限。

### 状态
**目前在阶段4** - 已将 E2E 拆到 `tests/control_mouse_e2e.rs`,准备记录交付结果。

## [2026-05-14 12:33:20] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: E2E 实现和验证完成

### 已完成
- 新增 `tests/control_mouse_e2e.rs`。
- 测试默认 `#[ignore]`,只在手动指定 ignored test 时真实移动鼠标和点击。
- 保持 `tests/control_lanes.rs` 在 1000 行以内,避免把真实 GUI 测试塞进已有 lane 测试文件。

### 验证
- `cargo fmt -- --check`: 通过。
- `cargo test --test control_mouse_e2e --no-run`: 通过。
- `cargo test --test control_lanes --no-run`: 通过。
- `cargo test --test control_mouse_e2e`: 0 passed, 1 ignored,默认不会点击。
- `cargo test --test control_lanes`: 8 passed, 1 ignored。

### 状态
**当前任务完成** - 未执行真实 ignored 点击测试,因为它会移动鼠标并点击系统菜单。

## [2026-05-14 12:41:20] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 用户观察到第二次点击未命中

### 现象
- 用户手动运行 ignored E2E,命令 exit 0。
- 人眼观察到鼠标点到了 Apple icon。
- 人眼观察到第二次没有点到弹出菜单里的“关于本机”。

### 当前假设
- 主假设: 测试里用固定偏移 `about_this_mac_x/y` 推导菜单项坐标不够可靠。
- 备选解释: 测试只验证 mouse response 和最终 screenshot bundle,没有验证“关于本机”窗口真的出现,所以出现协议假阳性。

### 下一步
- 做最小复现实验: 只打开 Apple 菜单并保留截图/manifest,分析菜单真实位置。
- 修复方向必须继续复用 screenshot manifest 的 `os-logical` 坐标语义。

## [2026-05-14 12:43:41] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 进入第二次点击未命中调试

### 现象
- ignored E2E exit 0,但用户肉眼观察到只点开了 Apple 菜单。
- 第二次点击没有命中 Apple 菜单里的“关于本机”。

### 假设
- 主假设: `about_this_mac_x/y` 固定偏移没有匹配真实 Apple 菜单项位置。
- 备选解释: 测试只校验协议 response,没有校验最终 UI 状态,因此协议成功掩盖了交互失败。

### 验证计划
- 先改测试为两段式: 初始截图定位 primary display 和 Apple icon -> 点击 Apple icon -> 菜单打开后截图。
- 从菜单打开后的截图里推导“关于本机”点击坐标,不再使用未验证固定偏移。
- 点击后再截图并加入视觉状态断言,避免协议假阳性。

### 状态
**目前在调试阶段** - 下一步读取 screenshot manifest / image contract,然后编辑 `tests/control_mouse_e2e.rs`。

## [2026-05-14 12:51:41] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 重新运行真实 E2E

### 用户补充
- 用户说明上一轮运行时有人为鼠标移动干扰。

### 当前动作
- 保持当前动态菜单定位实现不变。
- 重新运行 ignored E2E,用新一轮截图证据判断是否仍然失败。

### 状态
**目前在验证阶段** - 正在重新跑真实 GUI 鼠标测试。

## [2026-05-14 12:52:35] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 真实 E2E 复跑通过

### 已验证结论
- 无人为鼠标移动干扰时,当前测试能点开 Apple 菜单,并从菜单截图推导“关于本机”点击坐标。
- 本轮动态证据中 Apple 图标坐标是 `(14,12)`,“关于本机”点击坐标是 `(112,36)`。
- 测试最终截图视觉断言通过,不再只是协议 response 成功。

### 验证
- `cargo fmt -- --check`: 通过。
- `cargo test --test control_mouse_e2e --no-run`: 通过。
- `cargo test --test control_mouse_e2e`: 通过,1 ignored,默认不真实点击。
- `cargo test --test control_mouse_e2e daemon_control_lane_should_click_apple_menu_about_this_mac_via_rdog_control -- --ignored --exact --nocapture`: 通过。
- `git diff --check`: 通过。

### 状态
**当前调试完成** - 已确认重跑通过,准备补 WORKLOG / ERRORFIX 并交付。

## [2026-05-14 13:02:10] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 收紧验收为 About 窗口内容

### 用户纠正
- 不能把“最终截图有变化”判断为成功。
- 成功必须是独立“关于本机/本机信息”窗口显示出来。
- 窗口内容必须包含 `MacBook Air` 和 `macOS 15.7.5 (24G624)`。

### 现象
- 上一版测试的最终视觉断言太弱,它只能证明屏幕有变化,不能证明 About 窗口出现。

### 新假设
- 主假设: 打开菜单和第二次点击被拆成两个 `rdog control` 进程,新进程边界可能关闭了菜单,导致坐标正确也点不到菜单项。
- 备选解释: 第二次点击坐标仍然偏离真实“关于本机”菜单项中心。

### 验证计划
- 改成单个长连接 `rdog control` session,菜单打开后不关闭控制连接。
- 在同一 session 内读取菜单截图,推导第二次点击坐标,再继续写入点击命令。
- 最终截图后使用 OCR 验证 `MacBook Air` 和 `macOS 15.7.5 (24G624)` 文本。

### 状态
**目前在修正阶段** - 准备编辑测试 harness 和最终验收断言。

## [2026-05-14 12:57:01] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 更正上一条记录时间

### 更正
- 上一条记录标题中的时间被手工写成了 。
- 实际记录发生在当前时间附近,以本条为准。

### 状态
**继续修正测试** - 下一步仍是把成功标准改成 About 窗口内容验证。

## [2026-05-14 12:56:31] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 更正上一条记录方式错误

### 更正
- 上一条上下文追加命令因为未引用 heredoc,触发了反引号命令替换。
- 这属于记录过程错误,不是 `rdog` 测试逻辑错误。

### 状态
**继续修正测试** - 下一步仍是把成功标准改成 About 窗口内容验证。

## [2026-05-14 12:58:20] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: OCR 硬验收失败

### 现象
- 单连接真实 E2E 运行后,最终截图 OCR 没有 `MacBook Air`。
- OCR 结果主要是 OpenWarp / Codex 桌面内容,说明独立“关于本机”窗口没有出现。

### 已验证结论
- 之前“最终截图有变化”不是合格验收标准。
- 当前实现仍未满足用户定义的成功条件。

### 下一步
- 保留失败现场截图路径,分析菜单打开截图中真实菜单项位置。
- 重点验证第二次点击坐标是否落在“关于本机”行,以及菜单是否在点击前仍然打开。

## [2026-05-14 13:03:20] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 重新打开菜单后菜单截图不稳定

### 现象
- 真实 E2E 在菜单截图阶段失败。
- 失败信息: `Apple menu diff is too small for a stable click target: ChangedRegion { rect: LogicalRect { x: 12, y: 142, width: 213, height: 41 }, changed_pixels: 103 }`。

### 当前判断
- 当前不能继续把点击坐标推进到最终 About 窗口验收。
- 必须先让“菜单打开截图”本身稳定,否则第二次点击坐标没有可靠来源。

### 下一步
- 找到最新失败现场截图目录。
- 分析初始截图和菜单截图差异,确认是菜单未打开、菜单被截图动作关闭,还是菜单位置/等待时序问题。

## [2026-05-14 13:08:30] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 继续被中断的真实 E2E 验证

### 当前状态
- 未发现残留的 `control_mouse_e2e` / `rdog daemon` / `target/debug/rdog` 进程。
- 继续执行单条 ignored E2E,仍以 About 窗口 OCR 文本为成功标准。

### 状态
**正在验证** - 重新运行真实 GUI E2E。

## [2026-05-14 13:12:30] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 撤回全屏 OCR 判定

### 现象
- 最新真实 E2E 最终截图没有看到独立“关于本机”窗口。
- OCR 匹配到 `MacBook Air` 和 `macOS 15.7.5 (246624)` 来自终端/聊天文字,不是 About 窗口。

### 已回滚结论
- 不能用全屏 OCR 作为成功标准。
- 必须先定位新出现的独立窗口区域,只对该区域做 OCR 或做窗口形态验证。

### 下一步
- 排查 `@key escape` 是否真的关闭了菜单。
- 加强失败条件: 任意 `@response` code 非 0 都必须失败,不能只检查 code 77。
- 最终验收只允许来自独立窗口区域的 OCR 文本。

## [2026-05-14 13:24:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 继续修正 About 窗口硬验收

### 现象
- 用户确认“全屏 OCR 出现 MacBook Air”仍然不能算成功,因为文字可能来自终端或聊天窗口。
- 真正成功必须看到独立“关于本机”窗口,并且该窗口内容包含 `MacBook Air` 与 `macOS 15.7.5 (24G624)`。

### 假设
- 主假设: 测试的最终判定范围太大,把终端/聊天内容误当成 About 窗口内容。
- 备选解释: 菜单打开/关闭时序不稳定,导致 `about_this_mac` 坐标正确但点击时菜单已经关闭。

### 验证计划
- 将最终 OCR 从整张虚拟桌面改为“候选新窗口区域”的 OCR。
- 点击 About 前用截图确认菜单仍然可见,然后立即点击菜单项。
- 对所有 control response 增加非零 code / error 文本检查,避免协议失败被隐藏。

### 状态
**目前在修正阶段** - 下一步编辑 `tests/control_mouse_e2e.rs`,然后跑格式化、编译和默认 ignored 验证。

## [2026-05-14 13:50:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 修正 About 菜单项坐标推导

### 现象
- 最新真实 E2E 中 `about_this_mac=(116,98)` 换算到截图 y=222。
- 从菜单截图看,这个位置已经接近“系统设置...”行,不是第一行“关于本机”中心。
- press-drag-release 能产生大块变化,但 OCR 不是 About 窗口内容。

### 已推翻假设
- “窗口检测阈值太高”不成立。失败时只有底部窗口变化或菜单项高亮变化,没有包含 `MacBook Air` 的独立 About 窗口。

### 当前假设
- 主假设: `detect_menu_panel_top` 的连续宽行阈值跳过了第一行,把面板顶边误识别到第二行附近。
- 备选解释: macOS 菜单选择对 press-drag-release 仍有时序要求,但当前必须先修正明显偏下的目标坐标。

### 下一步
- 把下拉面板顶边检测改成“第一条足够宽的变化行”,不要求三行连续。
- 重新跑格式化、编译、默认 ignored 测试和真实 ignored E2E。

## [2026-05-14 13:58:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: About 窗口已打开但 build 号未显示

### 现象
- 最新真实 E2E 已经打开独立“关于本机”窗口。
- 局部 OCR 读到 `MacBook Air` 和 `macOS Sequoia 15.7.5`。
- 局部 OCR 没有读到 `24G624`,截图显示 About 窗口默认只显示版本号,没有 build 号。

### 当前假设
- 主假设: 需要点击 About 窗口里的 macOS 版本号行,macOS 才会把 build 号显示出来。
- 备选解释: About 窗口当前样式不在同一个位置显示 build 号,需要用窗口局部 OCR/截图确认可见内容。

### 下一步
- 保存检测到的 About 窗口区域,推导版本号行点击点。
- 通过 rdog 鼠标命令点击版本号行,再截图并只 OCR 同一个独立窗口区域。
- 重新跑格式化、编译、默认 ignored 测试和真实 ignored E2E。

## [2026-05-14 13:56:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 接入二次点击版本号行

### 当前动作
- 修正 `assert_about_window_build_evidence` 的输出格式问题。
- 在主 E2E 开始前关闭残留“关于本机”窗口,避免上一轮失败现场污染初始截图。
- 使用首次 About 窗口区域推导版本号行点击点,通过 `rdog control` 点击后再次截图。
- 最终只在同一个独立窗口区域内 OCR `MacBook Air`、`15.7.5` 和 `24G624`。

### 状态
**目前在执行阶段** - 下一步编辑 `tests/control_mouse_e2e.rs`,然后重新跑编译和真实 ignored E2E。

## [2026-05-14 14:16:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 将内容验收改为 AX 文本

### 现象
- 窗口截图 crop 已证明独立 About 窗口可见。
- OCR 对 `MacBook Air` 不稳定,同一窗口会识别成 `Mac AR` 一类乱码。
- macOS Accessibility 直接从 `System Information` 的“关于本机”窗口读到稳定文本,包含 `MacBook Air` 与 `15.7.5`。

### 当前决定
- 保留截图差分和 crop,用于证明独立窗口真实显示。
- 内容字段改用窗口自身 AX 文本验证,避免把 OCR 误差当作交互失败。
- build 号仍以点击版本号行后同一窗口 AX 文本是否出现 `24G624` 为准。

### 状态
**目前在执行阶段** - 下一步编辑测试 helper 并重新跑完整验证。

## [2026-05-14 14:24:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 用 AX 文本坐标推导版本号点击点

### 已验证结论
- 手工通过 AX 点击 `Sequoia 15.7.5` 静态文本后, About 窗口文本变成 `macOS15.7.5 (24G624)`。
- 因此 build 号可展开,上次 rdog 点击失败是因为版本号点击点 `(792,463)` 偏离真实文本中心。

### 当前动作
- 新增 helper 从 `System Information` About 窗口读取包含 `15.7.5` 的 static text 位置和大小。
- 把该 AX 文本中心点作为 rdog `@click` 的 `os-logical` 坐标。
- build 号验收继续读取同一个 About 窗口 AX 文本。

### 状态
**目前在执行阶段** - 下一步编辑测试并重新跑完整验证。

## [2026-05-14 14:38:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 撤回点击版本号展开假设

### 现象
- 用户确认版本号和 build 号就显示在“关于本机”界面上,无需点击版本号文本。
- 因此上一条“点击版本号行后才显示 build 号”的判断不能作为当前实现方向。

### 当前假设
- 主假设: 测试失败来自取证路径或文本读取范围不准,不是 UI 需要额外点击。
- 备选解释: AX 文本读取可能只抓到了部分 static text,或者窗口尚未稳定时过早读取。
- 推翻主假设的证据: 在窗口稳定后,同一 About 窗口 AX 文本仍确实没有 `24G624`,且截图 crop 也证明该 build 号不可见。

### 下一步
- 检查 `tests/control_mouse_e2e.rs` 当前 diff 和 helper 实现。
- 移除版本号二次点击流程。
- 将最终成功条件改为: 独立 About 窗口存在,同一窗口文本包含 `MacBook Air`、`15.7.5` 和 `24G624`。
- 重新跑格式化、编译、默认 ignored 测试和真实 ignored E2E。

### 状态
**目前在修正阶段** - 下一步读取测试文件并做最小可证伪修改。

## [2026-05-14 14:51:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 当前 UI 不点击时未见 build 号

### 动态证据
- AX 窗口几何证明独立窗口存在: `System Information` / `关于本机`,position=`595,142`,size=`280,499`。
- AX static text 不点击时读到 `MacBook Air` 与 `macOSSequoia 15.7.5`,未读到 `24G624`。
- 用 AX 窗口 rect 裁最终截图后 OCR,同样读到 `MacBook Air` 和 `macOS Sequoia 15.7.5`,未读到 `24G624`。

### 结论
- “需要点击版本号展开 build”作为测试流程已按用户最新反馈撤回。
- 当前测试成功条件改成验证界面当前可见内容: 独立 About 窗口 + `MacBook Air` + `15.7.5`。
- `24G624` 暂不作为本轮无点击 E2E 的强断言,否则会和当前动态截图/AX 证据冲突。

### 下一步
- 把独立窗口证据改成 AX 窗口几何优先,再保存同一窗口 crop 作为视觉证据。
- 保留截图差分作为诊断候选,但不再要求大块 changed_pixels。
- 重新跑验证。

### 状态
**目前在修正阶段** - 下一步编辑测试实现。

## [2026-05-14 14:57:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: build 号非本轮强断言

### 用户最新口径
- `24G624` 需要点击版本号后才能读到。
- 本轮未点击时没读到 `24G624` 没关系。

### 当前决定
- E2E 成功条件固定为: 通过 `rdog control` 打开独立“关于本机”窗口,并验证窗口文本包含 `MacBook Air` 与 `15.7.5`。
- 不再点击版本号,不再要求 `24G624`。
- AX 读取退回已验证有效的一层 static text/text field 读取,避免递归 `entire contents` 在当前窗口上返回空文本。

### 状态
**目前在修正阶段** - 下一步编辑测试,然后重新跑格式化、编译、默认 ignored 测试和真实 ignored E2E。

## [2026-05-16 23:39:30] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: review 当前 mouse E2E diff

### 现象
- `tests/control_mouse_e2e.rs` 编译通过,默认测试显示 1 ignored.
- 当前测试代码仍声明并强断言 `24G624`.
- 这与用户后续口径冲突: `24G624` 要点击版本号后才能读到,没读到没关系.

### 决定
- 本轮 E2E 成功条件改回: 独立 About This Mac 窗口存在,同一窗口 AX 文本包含 `MacBook Air` 和 `15.7.5`.
- 不在本轮强断言 build 号 `24G624`.

### 下一步
- 移除 build 常量和 AX 文本 build 断言.
- 重新跑 fmt、compile、默认 ignored 测试和真实 ignored E2E.

### 状态
**正在修正 mouse E2E 验收口径** - 先让测试契约与用户最新口径一致.

## [2026-05-16 23:55:45] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: About 窗口 E2E 真实验证通过

### 现象
- 第一轮真实 E2E 失败在 `about_this_mac=(210,36)`,菜单第一项点击点偏右且偏上,没有打开独立窗口.
- 修正后第二轮出现独立窗口候选 `319x506`,但旧阈值 `ABOUT_WINDOW_MIN_WIDTH=320` 过硬,差 1 像素导致误判失败.
- 第三轮 About 窗口 crop OCR 已读到 `MacBook Air` 和 `macOS Sequoia 15.7.5`,但 AX `entire contents` 只返回 `PROCESS:System Information`.
- 第四轮把菜单 y 坐标固定为 primary display 顶部加菜单栏高度后,真实 E2E 通过.

### 已验证结论
- 成功证据必须来自独立 About 窗口区域,不能来自整屏 OCR.
- 当前机器上 About 窗口 AX 文本不稳定,但独立窗口 crop OCR 能稳定读到 `MacBook Air` 和 `15.7.5`.
- `24G624` 不作为本轮强断言,符合用户“需要点击版本号后才读到,没读到没关系”的最新口径.

### 验证
- `cargo test --package rustdog --test control_mouse_e2e --no-run` -> passed.
- `cargo test --package rustdog --test control_mouse_e2e` -> 0 passed, 1 ignored.
- `cargo test --package rustdog --test control_mouse_e2e daemon_control_lane_should_click_apple_menu_about_this_mac_via_rdog_control -- --ignored --exact --nocapture` -> 1 passed, 37.78s.
- `cargo fmt -- --check` -> passed.
- `git diff --check` -> passed.
- E2E 输出: `about_this_mac=(110,54)`,独立窗口 crop OCR 读到 `MacBook Air` 和 `macOS Sequoia 15.7.5`.
- 收尾后 `pgrep -fl "System Information|System Settings"` 无残留.

### 状态
**mouse E2E 已完成** - 准备 local commit,只提交 `tests/control_mouse_e2e.rs` 和 `__mouse_e2e` 支线记录.

## [2026-05-16 23:55:45] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 拆分 mouse E2E support 文件

### 现象
- `tests/control_mouse_e2e.rs` 之前达到 1177 行,超过仓库对静态语言文件的质量线.
- 主测试文件现在已压到 976 行,支持代码移入 `tests/control_mouse_e2e/support.rs` 后为 219 行.

### 已完成
- 将 control session / daemon / port / workdir / binary / process wait 等通用 harness 拆到 `tests/control_mouse_e2e/support.rs`.
- 主测试文件仅保留坐标推导,截图差分,独立 About 窗口证据和测试流程.

### 下一步
- 重新跑默认 ignored 测试和真实 ignored E2E,确认拆分不改变行为.
- 如果 fresh live 仍通过,再做 local commit.

### 状态
**正在做结构收口** - 先满足文件大小约束,再复验行为.

## [2026-05-17 00:05:39] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 恢复上下文后继续复验

### 当前目标
- 继续上次未完成的 mouse E2E 收尾。
- 验证 support 拆分后以及 About 窗口 crop padding 放大后的真实行为。
- 只提交 mouse E2E 相关文件,不混入 `AGENTS.md`、`task_plan__ax_plan.md` 或 `__mouse_ralph` 旧支线文件。

### 下一步
- 运行 `cargo fmt --all`。
- 运行 `cargo test --package rustdog --test control_mouse_e2e --no-run`。
- 运行默认 ignored smoke。
- 运行真实 macOS live ignored E2E。
- 若全部通过,补写支线记录并做 local commit。

### 状态
**正在复验阶段** - 当前先证明最后一轮测试结构调整没有破坏真实桌面行为.

## [2026-05-17 00:08:32] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: live E2E 复验失败并进入诊断

### 现象
- support 拆分后的编译检查通过。
- 默认 ignored smoke 通过。
- 真实 live E2E 失败在独立 About 窗口区域检测: `About This Mac must open an independent window, but no large new window region was found`。
- 失败输出里已有多个 changed candidates,最大候选是 `152x86`,另有 `516x14`、`277x21`、`33x484`、`37x464` 等区域。

### 当前假设
- 主假设: `@click` 已命中 About 菜单项,但当前 diff-based 独立窗口检测阈值或合并逻辑没有覆盖真实窗口内容变化形态。
- 备选解释: 第二次点击仍可能落在菜单项附近但没有真正打开 About 窗口,候选变化只是菜单收起或桌面局部变化。
- 推翻主假设的证据: final screenshot 中完全没有独立 About 窗口,且 AX 也找不到 System Information/About 窗口。

### 下一步
- 查看失败轮次的 menu/final 截图。
- 读取当前测试里的窗口检测逻辑和阈值。
- 做最小可证伪修改,优先让测试使用 AX 窗口几何作为 independent-window 证据,截图 diff/crop 作为辅助证据。

### 状态
**正在诊断阶段** - 先确认失败是点击问题还是检测问题,再改测试.

## [2026-05-17 00:17:29] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 暂停 live 鼠标测试

### 用户约束
- 用户当前正在交互使用这台计算机。
- 现在不要运行任何会移动鼠标、点击、拖拽或滚轮的 live mouse 测试。

### 当前状态
- 已停止继续执行鼠标类操作验证。
- 可以保留当前代码和支线记录,但后续 live E2E 需要等用户明确允许后再跑。

### 状态
**暂停 live 验证** - 不再触发任何真实鼠标控制,等待桌面可被测试时再继续.
