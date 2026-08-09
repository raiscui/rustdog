## Problem Statement

每次 `cargo install --path ./` 重新发布 rdog 二进制后,macOS 的 Accessibility 与 Screen Recording 授权都会失效,必须重新到系统设置手动授权。原因是默认 adhoc 代码签名把 designated requirement (DR) 钉死为二进制内容哈希 (cdhash),重新编译即产生新身份,系统将新二进制视为"新程序"。

## Solution

为本地发布的 rdog 建立跨构建稳定的签名身份:安装后用固定 identifier + 自定义 DR (`designated => identifier "rdog"`) 重新签名,DR 字节级一致,TCC 授权按 DR 匹配并跨版本保留。首次切换到新身份需一次性重新授权 (Accessibility + Screen Recording),之后重编、重装、升级均不再需要授权。可选封装安装脚本,内置 DR 校验,失败即中止 (fail-closed)。

## User Stories

1. 作为 rdog 开发者,我希望重新 `cargo install` 后不需要重新授权 Accessibility,以便持续开发 daemon 的 GUI 控制能力。
2. 作为 rdog 开发者,我希望升级到新版本后授权保留,以便升级不打断工作流。
3. 作为 rdog 开发者在另一台 Mac 上构建安装,我希望使用同一条命令得到相同签名身份,以便权限流程可复现。
4. 作为 rdog 用户,我希望方案不需要 Apple 开发者账号或付费证书,以便任何机器都能使用。
5. 作为 rdog 开发者,我希望签名不改变二进制行为,以便它只是发布流程的一部分而非运行时依赖。
6. 作为 rdog 开发者,我希望安装脚本能校验签名身份稳定,以便无人值守或 agent 驱动安装时自动发现身份漂移。
7. 作为 rdog 开发者,我希望首次切换身份后只授权一次,以便一次性成本可控且可预期。
8. 作为 rdog 开发者,我希望签名失败或 DR 不符时流程明确报错,以便快速排错。
9. 作为 daemon 进程,我希望 control 与 daemon 使用同一签名身份,以便授权落在正确的进程上。
10. 作为 rdog 开发者,我希望在身份失效时有清晰的恢复路径 (重签 + 重新授权),以便不陷入黑盒排查。
11. 作为 rdog 开发者,我希望方案不依赖 Xcode,以便仅装 Command Line Tools 的环境也能工作。

## Implementation Decisions

- 签名命令 (核心决策,来自本机验证;内联文本必须以 `=` 前缀,否则 codesign 按文件路径解析):
  `codesign -f -s - --identifier "rdog" --requirements '=designated => identifier "rdog"' <installed-bin>`
- 签名对象:安装后的 rdog 二进制 (daemon 与 control 为同一二进制),TCC 授权落点即为 daemon 进程;`cargo install` 复制产物时签名随文件保留,因此"先 install 后重签"最省事。
- identifier 命名:统一使用 `rdog`,与版本无关,全局唯一。
- 可选工程化:封装安装脚本,流程为 build/install → 重签 → 用 `codesign -d --requirements -` 断言输出为 `designated => identifier "rdog"`,断言失败即中止。
- 一次性迁移:现有 cdhash 身份的授权记录将作废,首次切换后需重新授权 Accessibility + Screen Recording,此后稳定。
- 明确不采用:自签名证书方案 (多一个 825 天过期维护点)、Apple Development 证书 (年过期)、PPPC/MDM profile (企业管控,本地开发过重)。
- 机制背景:macOS TCC 授权按 code requirement 记录;固定 DR 方案已在 hermes-agent PR #73681 与 claude-code issue #57679 社区验证;rdog 权限生命周期背景见 `specs/rdog-macos-operation-capture-research.md`。

## Testing Decisions

- 好的测试:只验证外部可观察行为 (签名后 DR 稳定、权限保留),不测试 codesign 内部实现。
- 测试缝选在签名身份不变量:同一构建产物重签前后 DR 不变;不同内容二进制签同一 identifier 后 DR 字节级一致 (仅路径行不同);重编 → 重装 → 再断言 DR 不变。
- 断言方式:安装后运行 `codesign -d --requirements - <installed-bin>` 并匹配 `designated => identifier "rdog"`。
- 权限保留的最终验证只能人工一次性完成 (系统设置授权状态或 daemon 实际执行 AX/截图操作);自动化覆盖的是"DR 不变量",这是权限保留的必要条件,二者共同构成验收。
- 现有 prior art:仓库无 dev-workflow 级测试;验收以脚本形式放入 scripts/ 或等价位置,与既有 CLI smoke 分离。

## Out of Scope

- 自签名证书方案 (已评估排除)。
- App Store 分发、notarization、Developer ID 签名。
- PPPC / MDM 企业管控配置。
- 修改 rdog runtime 代码、协议或权限设计本身。
- Windows / Linux 平台 (TCC 为 macOS 专属机制)。

## Further Notes

- 本机已取证:两个内容不同 (sha256 不同) 的二进制,签同一 identifier 后 `Internal requirements count=1 size=52` 完全一致,DR diff 仅路径行。
- 默认 adhoc 签名现状:`Identifier=rdog-<随机后缀>`、DR 钉死 cdhash,重编必变;本方案将其替换为固定身份。
- 该 spec 由 2026-08-09 会话取证与方案讨论直接合成,无访谈;验收脚本是否纳入仓库待实施时确认。

---
来源: GitHub issue #40 (2026-08-09 会话 to-spec)
