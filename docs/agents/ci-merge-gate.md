# CI 合并门: 必需检查表与「未运行 / 失败」口径

本文件是 `.github/workflows/ci.yml` 的配套契约: **门上到底等哪几条检查**, 以及检查红了
时怎么判是"没跑到"还是"跑失败"。

## 0. 为什么有这份文件

2026-11-26 实锤 (RAI-397): `main` 的 `test (windows-latest)` 连续红了一整天, 一次测试都
没跑到 —— 红的形状与"测试真的失败"完全一样, 只能人肉翻日志才看出是 `ort-sys` 的 build
script 在 `copy-dylibs` 里 panic。同一时期 `main` 上还有 `recording_e2e` 夹具和
`figment::Jail` 两处红, 三个红点混在一起, 合并门实际上退化成"人工归因"。

结论: **"没跑到" 与 "失败" 必须是两个可辨识的事实**, 而且这个事实要由 job 名和注解承载,
不能靠谁记得去读日志。

## 1. 必需检查表

`main` 的分支保护必需检查必须与下表逐项一致 (既不多等一条, 也不白跑一条):

| 检查名 | 含义 | 红了的含义 |
| --- | --- | --- |
| `build (ubuntu-latest)` | 编译 `rdog` 二进制 | 代码 / 工具链 / 原生依赖 / 外部依赖的问题 |
| `build (macos-latest)` | 同上 | 同上 |
| `build (windows-latest)` | 同上 (含 `ort-sys` 的 DLL 铺放断言) | 同上 |
| `test (ubuntu-latest)` | 跑 `cargo test --bin rdog` + `recording_e2e` | 测试失败 |
| `test (macos-latest)` | 同上 | 测试失败 |
| `test (windows-latest)` | 跑 `cargo test --bin rdog` (Windows 上无 e2e, 见 §4) | 测试失败 |
| `knowledge carriers` | 载体断言 + 记账报告 | 载体与 `src/` 分叉 |

**检查名的真相源是 `.github/workflows/ci.yml` 里的 job 名**, 不是本表。改 job 名必须同票
改本表 **和** 仓库设置里的必需检查列表 —— 后者只有仓库管理员能改, 改 workflow 的人拿不到
这个权限时, 在 PR 里写明"需要在 Settings → Branches 里换名字", 不要静默放行。

> 注意: 本仓库的 GitHub App 身份**读不到** branch protection (`403 Resource not accessible
> by integration`), 所以"设置侧是否已对齐"只能由人核对。这条是已知缺口, 不是本文件的省略。

## 2. 触发面

```yaml
on:
  push: {branches: [main]}
  pull_request: {branches: [main], types: [opened, synchronize, reopened, ready_for_review]}
  workflow_dispatch:
```

`pull_request.types` **必须显式列 `ready_for_review`**。默认类型不含它, 草稿期开立的 PR
标 ready 后一个 run 都不会起, 而门会把"该 head 0 个必需检查"读成"没有失败" (2026-11-26
在 `raiscui/pireel` 的真实事故形态)。

## 3. 红点怎么读

| 现象 | 结论 | 去哪看 |
| --- | --- | --- |
| `build (os)` 红, 同 OS 的 `test (os)` **skipped** | **测试未运行**。门应当拦住, 但不能当成"测试失败"记 | `build` job 里失败步骤的 `::error title=…` 注解 |
| `build (os)` 绿, `test (os)` 红 | 测试真的失败 | `test` job 的 `Unit tests` / 断言步骤日志 |
| `test (os)` 绿, 但日志里 `tests did not run` 是红的 | 断言步骤红 = 零测试被执行 | `assert-tests-ran.py` 输出 |
| `build (os)` 红的注解是 `build prep failed` | 准备阶段 (工具链 / 拉取 / 目录) 出了问题, 不是测试结论 | `cargo-prep` 步骤日志 |
| 该 head 一个 run 都没有 | `missing-checks`, 门结构性缺口 | `ci-check` skill 第 2/3 检查 |
| `knowledge carriers` 红 | 载体断言与 `src/` 真相源分叉 | 脚本输出 |

两类"未运行"的注解标题是本口径的接口:

- `onnxruntime dist unavailable` / `onnxruntime dist digest mismatch` — 外部依赖拿不到或被篡改
  (见 §4), 测试没跑;
- `build prep failed` — 准备阶段异常 (工具链 / registry 等), 同样没有测试证据;
- `tests did not run` — 测试框架退出码 0 但执行数为 0 (filter 打错, 或 `#[cfg(windows)]`
  之类平台门把整组测试编译掉了)。

**这些都不许改判成绿。** 需要的是"可辨识", 不是"放行"。

两条结构性保证 (2026-11-26 第一版实现被评审指出后补的):

- **每个 OS 的 `test` 只依赖自己 OS 的 `build`** (六个独立 job, 不是一对 matrix),
  所以 Windows 的外部依赖红了不会连带吞掉 ubuntu / macOS 的测试证据;
- **测试目标 (`#[cfg(test)]` 模块与 `tests/`) 在 build 阶段用 `--no-run` 编译**。
  否则"测试代码编译不过"会落在 test 阶段, 把 cargo 的编译失败与测试失败混成一个红点,
  零执行断言还会被跳过。

## 4. `ort-sys` / onnxruntime 的获取契约

`oar-ocr` → `ort` → `ort-sys` 在 build 时按目标三元组取一份 pyke 预编译 onnxruntime。
这条链上有两个已知坑, 都由 CI 侧兜住:

1. **dist 选择与校验** — `scripts/ci/prefetch-ort-dist.py` 从 `Cargo.lock` 锁定的那个
   `ort-sys` crate 内嵌的 `build/download/dist.tsv` 读目标→URL→sha256 (**不手抄摘要**),
   校验后用 pyke 自己的缓存布局落到 `<cache_root>/dfbin/<target>/<sha256>/`, 于是
   `ort-sys` build script 命中缓存、完全跳过网络。选行规则与 `resolve_dist()` 一致:
   本仓库没有开启任何 ort backend feature, 取该目标第一行 (Windows `+directml`、
   macOS `+coreml`, 那是 pyke 的基线包)。
2. **`copy-dylibs` 要求 `<OUT_DIR 祖先>/examples` 存在** — `ort-sys` 的
   `build/dynamic_link.rs` 把 dist 里的 DLL 复制到 `<OUT_DIR 的第 3 个祖先>` 及其
   `examples/`、`deps/`, 而 `cargo build --bin rdog` 从不创建 `examples/`。Windows 的
   dist 带 `DirectML.dll`, 于是 build script panic (`Os { code: 3, kind: NotFound }`,
   `dynamic_link.rs:42`); Linux 的 dist 只有静态 `libonnxruntime.a`, 匹配不到 `.so`,
   所以这条路在 Linux 上是**潜伏**的。CI 由 `cargo-prep` 的
   `Create the dylib copy targets ort-sys expects` 步骤铺好两个层级的目录。

**改 `Cargo.lock` 里的 `ort-sys` 版本 = 复核这条链**: dist 表内容、`copy-dylibs` 的目录假设
都可能变。复核方式 (本地可跑):

```bash
python3 scripts/ci/prefetch-ort-dist.py            # 打印命中的 dist 与其 sha256
cargo clean -p ort-sys && cargo build --bin rdog    # 走一遍 build script
```

`ort-sys` 想绕过本机制时它只有一条路: 缓存目录不存在 → 自己下载, 所以预取失败必须**红**,
不能 `continue-on-error`。

## 5. 修改本工作流时的义务

- 改 job 名 → 同票改 §1 的表 + 提醒管理员改仓库设置的必需检查;
- 新增/退役一条检查 → 同上, 并跑一遍 `ci-check` skill 的五项;
- 任何 `paths` 过滤都别加在这份 workflow 上: 静态必需表 + 路径门控 = 门结构性不可达
  (`ci-check` skill 第 3 检查的反例)。

## 6. 已知红的登记与到期日

**已知红不是豁免, 是一个带到期日的债。** 登记格式固定为四列, 缺一列就当没登记:

| 现象 | 归因 | 负责人 | 到期日 |
| --- | --- | --- | --- |
| (当前无未清的已知红) | — | — | — |

规则:

- 到期日必须是具体日期 (`YYYY-MM-DD`), 不写"下轮"、"等 RAI-4xx"这类相对说法;
- **到期未清 = 阻塞项**, 与"本改动坏了东西"同等处理 —— 不允许再用"这是已知红"给新 PR
  放行, 也不允许把到期日续期当成处置;
- 清掉后从表里删除并在对应票面留回执 (哪次运行、哪个 head 变绿);
- 登记项必须有归因入口 (哪条注解 / 哪个测试名), 否则无法验证它到底还在不在。

当前本仓库的三类红 (2026-11-26 RAI-397 起点) 的去向: 工具链漂移与 flake 已在
`718eb8c4` 清掉; 「未运行」这一类由本文件的 §4 机制消掉; 若将来再现, 按上表重开一行。
