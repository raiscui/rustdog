# rdog 用户配置目录 Plan

> 这是 `rustdog` 用户级默认配置目录 (`~/.rdog/`) 的规划。
> 该文件是设计契约的长期入口;实施细节(代码、测试、CLI 行为)沉淀在后续 `WORKLOG.md`。

## 1. 背景与动机

`rdog daemon` 当前只从**当前工作目录**查找平台配置文件
(`rdog_macos.toml` / `rdog_linux.toml` / `rdog_win.toml`,以及 `rcat_*` / `rdog.toml` / `rcat.toml` 兼容层,
见 `src/config.rs` 的 `default_config_file_candidates`)。这带来三个实际痛点:

1. **daemon 配置与工作目录绑定**:用户在任意目录执行 `rdog daemon`,可能读到完全不同的配置,
   甚至找不到配置而静默使用内置默认值(`zenoh.enabled=false`),表现为"daemon 起来但 @ping 找不到"。
2. **`rdog config init` 只能落在 cwd**:生成的模板散落在各个工作目录,没有统一的用户级配置真相源。
3. **本机默认 daemon 没有稳定身份**:`rdog control @ping` 依赖 local-default registry
   (`~/.local/state/rustdog/local-default/`),而 registry 里的 daemon_name / namespace 来自 daemon 启动时
   的配置;配置若不稳定,本机默认寻址就跟着漂移。

用户明确提议:创建 `~/.rdog/` 作为用户级默认配置目录,让"本机默认 daemon"有一份稳定的配置真相源。

## 2. 目标

为 `rdog daemon` 增加用户级默认配置目录 `~/.rdog/`,并把配置查找、`config init` 生成位置收敛到
"用户目录为默认、cwd 为项目级覆盖、env 为运行时覆盖"的稳定分层。

**不在本轮范围**:
- 不实现 daemon 生命周期托管 (launchd / systemd,那是独立的方向 1,见 `## 8. 后续` 的关联)。
- 不改变 `--config` 显式路径语义 (`--config` 缺失时仍 fail-fast)。
- 不删除或改写 `rcat_*` legacy 兼容层。
- 不改变 env 覆盖优先级 (`RDOG_` / `RCAT_` 仍为最高运行时覆盖)。
- 不改动 control 端 (`rdog control`) 的 CLI 参数模型;control 端不加载 daemon 配置,
  本机默认寻址继续走 local-default registry。

## 3. 设计契约

### 3.1 用户配置目录解析

- 用户配置根: `~/.rdog/` (绝对路径 = `$HOME/.rdog`,Windows 为 `%USERPROFILE%\.rdog`)。
- 目录内文件名沿用平台模板约定,与 cwd 查找一致:
  - macOS: `~/.rdog/rdog_macos.toml`
  - Linux: `~/.rdog/rdog_linux.toml`
  - Windows: `~/.rdog/rdog_win.toml`
  - legacy 兼容名 (`rdog.toml` / `rcat_*`) 不进入用户目录查找,避免在用户级再铺一层兼容噪音。
- 解析优先级 (从低到高,与 figment 后 merge 覆盖前 merge 一致):
  1. `DaemonConfig::default()` (内置默认值,最低)
  2. `~/.rdog/<platform>.toml` (用户级默认,新增)
  3. cwd `<platform>.toml` + legacy 候选 (项目级覆盖,兼容现有部署)
  4. `RDOG_` / `RCAT_` 环境变量 (运行时覆盖,最高)
- `--config <path>` 显式指定时,跳过 2/3 两档文件查找,只 merge 显式文件 (现有语义保留)。

### 3.2 `rdog config init` 行为

- 无 `--config` 时:默认生成三份平台模板到 `~/.rdog/` (用户级配置根),不写入 cwd。
  - 已存在且未 `--force` → `AlreadyExists` 报错 (与现有行为一致,只是目标目录换成用户目录)。
- 有 `--config <path>` 时:生成到指定路径 (现有行为保留,便于项目级或自定义位置)。
- 成功输出列出实际写入的绝对路径,便于用户确认。

### 3.3 与 local-default registry 的配合

- daemon 以用户级配置启动时,`[zenoh] namespace` / `daemon_name` / `[zenoh.unixpipe] local_default = true`
  决定其 local-default 身份;registry 路径不变 (`~/.local/state/rustdog/local-default/`)。
- 用户级配置是"本机默认 daemon"的稳定真相源:同一用户在任何 cwd 执行 `rdog daemon`,
  只要不传 `--config` 且 cwd 无平台文件,就会以同一身份注册。
- cwd 项目文件覆盖用户级配置时,daemon 身份随 cwd 配置走;这是项目级覆盖的预期语义,
  不应在 `rdog control` 侧做任何额外关联。

### 3.4 daemon transport 推断 (实施补充)

- 无显式 `--transport` 时,transport 由**合并后配置**的 `[zenoh] enabled` 决定 (不再要求 `--config` 显式传入):
  - `zenoh.enabled = true` → Zenoh (用户级或 cwd 配置加载的 zenoh profile 自动生效)
  - `zenoh.enabled = false` → TCP (纯默认值场景保持历史 TCP daemon 语义)
- 显式 `--transport tcp|zenoh` 始终最高优先,不受配置影响。
- 影响: `rdog daemon` 在任意 cwd 执行时,若 `~/.rdog/<platform>.toml` 是 zenoh profile,
  自动以 Zenoh transport 启动并注册 local-default;集成测试 spawn daemon 时必须
  显式设置 `RDOG_ZENOH__ENABLED=false` (env 最高优先级) 或 `--transport tcp` 保持 TCP 语义。

### 3.5 错误处理契约

- `~/.rdog/` 目录不存在:按"无用户配置"处理,不自动创建 (避免 `rdog daemon` 在只读 HOME 上
  产生副作用);只有 `rdog config init` 才创建目录。
- 用户目录文件存在但解析失败:与 cwd 文件同样 fail-fast,错误信息带完整路径。
- `$HOME` 未设置且非 Windows:回退到 cwd 行为 (用户目录档跳过),保持与现有代码一致的降级。
- 用户目录文件存在 + cwd 文件存在:两者都 merge,cwd 覆盖用户目录,不报冲突
  (分层覆盖是设计语义,不是歧义)。

## 4. 验收矩阵

| # | 场景 | 预期 |
|---|---|---|
| 1 | 只有 `~/.rdog/rdog_macos.toml`,无 cwd 平台文件 | `rdog daemon` 从用户目录加载,`@ping` 可用 |
| 2 | 用户目录 + cwd 都有平台文件,字段冲突 | cwd 值生效 (覆盖用户目录) |
| 3 | 无 `--config`,两者都没有 | 内置默认值生效;若 zenoh 全关,校验报错提示至少启用一个端点 |
| 4 | `--config ./custom.toml` | 只读显式文件,跳过 2/3 档 |
| 5 | `RDOG_ZENOH__ENABLED=false` + 文件启用 | env 生效 (最高覆盖) |
| 6 | `rdog config init` (无 --config) | 三份模板写入 `~/.rdog/`,返回绝对路径;再次运行报 AlreadyExists |
| 7 | `rdog config init --force` | 覆盖 `~/.rdog/` 已有模板 |
| 8 | `rdog config init --config ./x.toml` | 写入指定路径,不动用户目录 |
| 9 | macOS 上 `~/.rdog/rdog_macos.toml` 配 `local_default=true` + `mac.lab` | `rdog daemon` 无 `--config` 自动走 Zenoh;`rdog control @ping` 经 local-default registry 命中本机 daemon |
| 10 | `$HOME` 未设置 (非 Windows) | 用户目录档跳过,行为与当前一致 |
| 11 | 用户目录文件 TOML 非法 | fail-fast,错误信息含 `~/.rdog/rdog_macos.toml` 全路径 |
| 12 | Windows | 查找 `%USERPROFILE%\.rdog\rdog_win.toml`,行为与 unix 一致 |
| 13 | 无 `--config` + `~/.rdog` zenoh profile (inbound/outbound 均 false) | transport 推断为 Zenoh,不报"TCP 至少要启用一个端点" |

## 5. 测试决策

- 测试缝选在配置查找分层:构造临时 `$HOME` (或注入测试用的用户配置根),验证 3.1 的 merge 顺序。
- 不测 figment 内部,只测外部可观察结果 (最终 `DaemonConfig` 字段值)。
- `config init` 测试:临时 `$HOME` 下验证生成路径、AlreadyExists、force、`--config` 分支。
- 现有 `src/config.rs` 测试保持不变 (default / validate 契约不回归);
  新增测试文件或测试模块覆盖用户目录档,并复用现有 `with_tmpdir` 风格隔离。
- e2e 不做新增;验收矩阵 9 依赖真实 daemon + local-default registry,纳入后续手动/脚本 smoke。

## 6. Out of Scope

- launchd / systemd daemon 生命周期托管 (方向 1)。
- control 端加载 daemon 配置 (control 保持 CLI 参数 + registry 寻址)。
- 多用户 / 多 HOME 切换支持。
- `~/.rdog/` 目录的自动创建策略 (仅 `config init` 创建)。
- Windows hidden-daemon 与用户目录的联动。

## 7. 实施顺序 (建议)

1. `src/config.rs`:新增用户配置根解析函数 (`resolve_user_config_dir`) 与候选路径。
2. `build_figment`:把用户目录档插入默认值与 cwd 档之间,保持 `--config` 分支不变。
3. `write_example_configs`:无 `--config` 时目标目录改为用户配置根,返回绝对路径。
4. `config init` CLI 提示与错误文案同步。
5. 补测试 (验收矩阵 1-8、10-12),跑定向 + 串行全量。
6. 更新 `AGENTS.md` 索引与 `rdog-control` skill 的 daemon 启动指引 (若文案涉及配置路径)。

## 8. 后续 (方向 1 关联)

用户级配置目录是本机默认 daemon 的配置真相源;下一步的方向 1 (launchd / systemd 托管
`rdog daemon`) 将消费同一份 `~/.rdog/` 配置,使开机自启的 daemon 以稳定身份注册 local-default,
`rdog control @ping` 无需手动启动。两者配套后才构成完整的"默认本机、无需手动起 daemon"体验。
