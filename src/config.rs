use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    time::Duration,
};
use zenoh::config::EndPoint;

use crate::control_transport::ControlTransportKind;

const DEFAULT_CONFIG_FILE_NAME: &str = "rdog.toml";

/// unixpipe socket 路径总长上限(单位:字节)。
///
/// macOS 的 `sun_path` 限制是 104 字节。
/// 我们留 4 字节给末尾 NUL + 拼装容差,把所有"自动推导"和"显式覆盖"路径都卡在 100 字节以内。
/// 超过时 daemon 启动 fail-fast,避免运行时 bind 报 ENAMETOOLONG。
pub(crate) const UNIXPIPE_SOCKET_PATH_MAX_BYTES: usize = 100;

const LEGACY_CONFIG_FILE_NAME: &str = "rcat.toml";
const WINDOWS_CONFIG_FILE_NAME: &str = "rdog_win.toml";
const MACOS_CONFIG_FILE_NAME: &str = "rdog_macos.toml";
const LINUX_CONFIG_FILE_NAME: &str = "rdog_linux.toml";
const LEGACY_WINDOWS_CONFIG_FILE_NAME: &str = "rcat_win.toml";
const LEGACY_MACOS_CONFIG_FILE_NAME: &str = "rcat_macos.toml";
const LEGACY_LINUX_CONFIG_FILE_NAME: &str = "rcat_linux.toml";

const WINDOWS_EXAMPLE_CONFIG_TEMPLATE: &str = include_str!("../rdog_win.toml");
const MACOS_EXAMPLE_CONFIG_TEMPLATE: &str = include_str!("../rdog_macos.toml");
const LINUX_EXAMPLE_CONFIG_TEMPLATE: &str = include_str!("../rdog_linux.toml");

/// `rdog daemon` 的完整运行配置。
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct DaemonConfig {
    pub daemon: DaemonSettings,
    pub observation: ObservationConfig,
    pub hidden: HiddenResidentConfig,
    pub outbound: OutboundConfig,
    pub inbound: InboundConfig,
    pub zenoh: ZenohConfig,
}

/// daemon 级别的通用设置。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct DaemonSettings {
    pub retry_seconds: u64,
}

/// Windows 隐藏常驻模式的附加配置。
///
/// 这部分配置不会改变普通 `daemon` 的默认行为。
/// 只有显式进入隐藏常驻入口时才会被使用。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct HiddenResidentConfig {
    pub log_file: PathBuf,
}

/// GUI observation 持久化状态配置。
///
/// 这里的 durable state 只保存 observation metadata 和 selector 线索。
/// 短期 `@eN` ref 仍然只能在当前 daemon 进程内使用。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ObservationConfig {
    pub durable_enabled: bool,
    pub state_dir: Option<PathBuf>,
    pub retention_observations: usize,
    pub retention_bytes: u64,
    pub persist_values: bool,
    pub persist_screenshots: bool,
    pub write_ref_cache: bool,
}

/// 主动连出端点的配置。
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct OutboundConfig {
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub shell: Option<String>,
    pub mode: EndpointMode,
}

/// 被动监听端点的配置。
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct InboundConfig {
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub shell: Option<String>,
    pub mode: EndpointMode,
    pub transport: ControlTransportKind,
}

/// Zenoh router/client control-plane profile 的配置。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ZenohConfig {
    pub enabled: bool,
    pub mode: ZenohMode,
    pub namespace: String,
    pub daemon_name: Option<String>,
    /// Legacy peer-era field kept only to produce explicit migration errors.
    pub connect_endpoints: Vec<String>,
    pub listen_endpoints: Vec<String>,
    pub request_timeout_ms: u64,
    pub startup_guard_window_ms: u64,
    pub key_input_events: KeyInputEventsConfig,
    /// 本机 fast path 的 unixpipe 配置。
    /// macOS / Linux 默认 `enabled = true`,Windows 编译期强制 `false`。
    /// 客户端可以借此走 Unix domain socket,避免 UDP loopback 的协议开销。
    pub unixpipe: UnixpipeConfig,
}

/// `@key` 成功执行后,对外发布 Zenoh 键盘事件的配置。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct KeyInputEventsConfig {
    pub enabled: bool,
    #[serde(alias = "key_expr")]
    pub keyexpr: String,
}

/// 本机 fast path 的 unixpipe 配置。
///
/// `enabled = true` 时,daemon 会按 (namespace, daemon_name) 推导一条 Unix domain socket
/// 路径,加入 zenoh listen_endpoints。客户端优先尝试 unixpipe connect,失败再回退 UDP scout。
///
/// 注意:Windows 上 `enabled` 在编译期被强制为 `false`,运行时不可改。
/// 不用 `#[derive(Default)]` 是因为 unix / windows 平台 `enabled` 默认值不同,
/// 需要两个 `impl Default` 走 `#[cfg(unix)]` / `#[cfg(windows)]` 分支。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct UnixpipeConfig {
    /// 是否启用 unixpipe endpoint。
    /// macOS / Linux 默认 `true`,Windows 默认 `false`(编译期强制)。
    pub enabled: bool,
    /// 显式覆盖 socket 路径。None = 按 (namespace, daemon_name) 自动推导 `$TMPDIR/rdog-{ns}-{name}.sock`。
    /// 显式给的路径在配置校验时必须满足 `sun_path` 长度上限(<= 100 字节)。
    pub socket_path: Option<PathBuf>,
}

/// daemon 端点的会话模式。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointMode {
    #[default]
    Interactive,
    Control,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ZenohMode {
    #[default]
    Router,
    #[serde(alias = "peer")]
    PeerLegacy,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self { retry_seconds: 5 }
    }
}

impl Default for HiddenResidentConfig {
    fn default() -> Self {
        Self {
            log_file: PathBuf::from("rdog_hidden.log"),
        }
    }
}

impl Default for ObservationConfig {
    fn default() -> Self {
        Self {
            durable_enabled: true,
            state_dir: None,
            retention_observations: 256,
            retention_bytes: 50 * 1024 * 1024,
            persist_values: false,
            persist_screenshots: false,
            write_ref_cache: true,
        }
    }
}

impl Default for ZenohConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ZenohMode::Router,
            namespace: String::new(),
            daemon_name: None,
            connect_endpoints: Vec::new(),
            listen_endpoints: Vec::new(),
            request_timeout_ms: 3_000,
            startup_guard_window_ms: 1_000,
            key_input_events: KeyInputEventsConfig::default(),
            unixpipe: UnixpipeConfig::default(),
        }
    }
}

impl Default for KeyInputEventsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            keyexpr: String::new(),
        }
    }
}

#[cfg(unix)]
impl Default for UnixpipeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            socket_path: None,
        }
    }
}

#[cfg(windows)]
impl Default for UnixpipeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            socket_path: None,
        }
    }
}

impl DaemonConfig {
    /// 返回当前配置计算出的重试间隔。
    pub fn retry_interval(&self) -> Duration {
        Duration::from_secs(self.daemon.retry_seconds)
    }
}

/// 从默认值、TOML 配置和环境变量中加载 daemon 配置。
#[cfg_attr(not(any(test, windows)), allow(dead_code))]
pub fn load_daemon_config(config_path: Option<&Path>) -> io::Result<DaemonConfig> {
    let config = load_daemon_config_unvalidated(config_path)?;
    validate_daemon_config(&config)?;
    Ok(config)
}

/// 从默认值、TOML 配置和环境变量中加载 daemon 配置,但暂不做最终校验。
///
/// 这个入口只给“CLI 还要继续覆盖 transport/profile 字段”的路径使用。
pub fn load_daemon_config_unvalidated(config_path: Option<&Path>) -> io::Result<DaemonConfig> {
    let figment = build_figment(config_path)?;
    let config: DaemonConfig = figment.extract().map_err(|err| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("无法解析 daemon 配置: {err}"),
        )
    })?;

    Ok(config)
}

/// 在当前目录生成三份平台配置模版。
///
/// 默认不会覆盖已有文件。
/// 只有显式传入 `force = true` 时,才允许重写现有平台模板。
pub fn write_example_configs_in_place(force: bool) -> io::Result<Vec<PathBuf>> {
    write_example_configs(force)
}

fn build_figment(config_path: Option<&Path>) -> io::Result<Figment> {
    let mut figment = Figment::from(Serialized::defaults(DaemonConfig::default()));

    // 只有两种情况会尝试读文件:
    // 1. 用户显式传了 `--config`,此时文件缺失要报错。
    // 2. 默认平台文件存在,此时自动合并进去。
    if let Some(config_path) = config_path {
        let resolved_path = config_path.to_path_buf();

        if !resolved_path.exists() {
            return Err(io::Error::new(
                ErrorKind::NotFound,
                format!("找不到配置文件: {}", resolved_path.display()),
            ));
        }

        figment = figment.merge(Toml::file(&resolved_path));
    } else {
        for candidate_path in default_config_file_candidates() {
            if candidate_path.exists() {
                // 新 `rdog_*` 文件是默认真相源。
                // 旧 `rcat_*` 和 `rcat.toml` 只作为升级 fallback,避免已有部署立刻失效。
                figment = figment.merge(Toml::file(&candidate_path));
                break;
            }
        }
    }

    Ok(figment
        // 旧前缀只作为升级兼容层。
        // 新旧同时存在时,后 merge 的 `RDOG_` 保持最高优先级。
        .merge(Env::prefixed("RCAT_").split("__"))
        .merge(Env::prefixed("RDOG_").split("__")))
}

fn default_platform_config_file_name() -> &'static str {
    if cfg!(windows) {
        WINDOWS_CONFIG_FILE_NAME
    } else if cfg!(target_os = "macos") {
        MACOS_CONFIG_FILE_NAME
    } else {
        LINUX_CONFIG_FILE_NAME
    }
}

fn legacy_platform_config_file_name() -> &'static str {
    if cfg!(windows) {
        LEGACY_WINDOWS_CONFIG_FILE_NAME
    } else if cfg!(target_os = "macos") {
        LEGACY_MACOS_CONFIG_FILE_NAME
    } else {
        LEGACY_LINUX_CONFIG_FILE_NAME
    }
}

fn default_config_file_candidates() -> [PathBuf; 4] {
    [
        PathBuf::from(default_platform_config_file_name()),
        PathBuf::from(legacy_platform_config_file_name()),
        PathBuf::from(DEFAULT_CONFIG_FILE_NAME),
        PathBuf::from(LEGACY_CONFIG_FILE_NAME),
    ]
}

fn example_config_templates() -> [(&'static str, &'static str); 3] {
    [
        (WINDOWS_CONFIG_FILE_NAME, WINDOWS_EXAMPLE_CONFIG_TEMPLATE),
        (MACOS_CONFIG_FILE_NAME, MACOS_EXAMPLE_CONFIG_TEMPLATE),
        (LINUX_CONFIG_FILE_NAME, LINUX_EXAMPLE_CONFIG_TEMPLATE),
    ]
}

fn write_example_configs(force: bool) -> io::Result<Vec<PathBuf>> {
    let templates = example_config_templates();

    if !force {
        for (path, _) in templates {
            if Path::new(path).exists() {
                return Err(io::Error::new(
                    ErrorKind::AlreadyExists,
                    format!(
                        "配置文件已存在: {}。如需覆盖,请使用 `rdog config init --force`",
                        path
                    ),
                ));
            }
        }
    }

    let mut written_paths = Vec::with_capacity(templates.len());
    for (path, contents) in templates {
        fs::write(path, contents)?;
        written_paths.push(PathBuf::from(path));
    }

    Ok(written_paths)
}

#[cfg_attr(not(any(test, windows)), allow(dead_code))]
fn validate_daemon_config(config: &DaemonConfig) -> io::Result<()> {
    if !config.outbound.enabled && !config.inbound.enabled && !config.zenoh.enabled {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "daemon 至少要启用一个端点,请在配置中打开 outbound、inbound 或 zenoh",
        ));
    }

    validate_endpoint(
        "outbound",
        config.outbound.enabled,
        config.outbound.host.as_deref(),
        config.outbound.port,
        config.outbound.shell.as_deref(),
    )?;
    validate_endpoint(
        "inbound",
        config.inbound.enabled,
        config.inbound.host.as_deref(),
        config.inbound.port,
        config.inbound.shell.as_deref(),
    )?;
    if config.inbound.transport == ControlTransportKind::WebSocket
        && config.inbound.mode != EndpointMode::Control
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "phase 1 websocket transport 只允许和 inbound.mode = \"control\" 组合",
        ));
    }
    validate_zenoh_config(
        &config.zenoh,
        config.outbound.enabled || config.inbound.enabled,
    )?;
    validate_observation_config(&config.observation)?;

    Ok(())
}

/// 供 Zenoh router daemon CLI 路径复用的配置校验入口。
pub fn validate_zenoh_daemon_profile(config: &DaemonConfig) -> io::Result<()> {
    validate_zenoh_config(
        &config.zenoh,
        config.outbound.enabled || config.inbound.enabled,
    )?;
    validate_observation_config(&config.observation)
}

/// TCP daemon 路径的严格校验。
///
/// 当最终 transport 解析为 TCP 时,不能因为 `[zenoh] enabled=true`
/// 就绕过“至少一个 TCP endpoint 启用”的基本约束。
pub fn validate_tcp_daemon_profile(config: &DaemonConfig) -> io::Result<()> {
    if !config.outbound.enabled && !config.inbound.enabled {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "TCP daemon 至少要启用一个端点,请在配置中打开 outbound 或 inbound,或改用 zenoh transport",
        ));
    }

    validate_endpoint(
        "outbound",
        config.outbound.enabled,
        config.outbound.host.as_deref(),
        config.outbound.port,
        config.outbound.shell.as_deref(),
    )?;
    validate_endpoint(
        "inbound",
        config.inbound.enabled,
        config.inbound.host.as_deref(),
        config.inbound.port,
        config.inbound.shell.as_deref(),
    )?;
    validate_observation_config(&config.observation)?;

    Ok(())
}

fn validate_observation_config(config: &ObservationConfig) -> io::Result<()> {
    if config.retention_observations == 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "observation.retention_observations 必须大于 0",
        ));
    }

    if config.retention_bytes < 1024 * 1024 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "observation.retention_bytes 不能小于 1 MiB",
        ));
    }

    if config
        .state_dir
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "observation.state_dir 不能是空字符串; 省略该字段表示使用平台默认路径",
        ));
    }

    Ok(())
}

fn validate_zenoh_config(config: &ZenohConfig, tcp_endpoints_enabled: bool) -> io::Result<()> {
    if !config.enabled {
        return Ok(());
    }

    if config.mode == ZenohMode::PeerLegacy {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "旧配置 `zenoh.mode = \"peer\"` 已废弃; 请改用 `zenoh.mode = \"router\"`",
        ));
    }

    if tcp_endpoints_enabled {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "首版不支持同时启用 zenoh 与 inbound/outbound TCP endpoint",
        ));
    }

    let Some(daemon_name) = config.daemon_name.as_deref() else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh 已启用,但缺少 daemon_name",
        ));
    };
    crate::zenoh_identity::validate_daemon_name(daemon_name)?;
    let _ = crate::zenoh_identity::resolve_namespace(Some(&config.namespace), Some(daemon_name))?;

    if config.request_timeout_ms == 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.request_timeout_ms 必须大于 0",
        ));
    }

    if config.startup_guard_window_ms == 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.startup_guard_window_ms 必须大于 0",
        ));
    }

    validate_key_input_events_config(&config.key_input_events)?;

    if config
        .connect_endpoints
        .iter()
        .any(|endpoint| endpoint.trim().is_empty())
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.connect_endpoints 不能包含空字符串",
        ));
    }

    if !config.connect_endpoints.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "router profile 不再使用 `zenoh.connect_endpoints`; 请把 daemon 入口写到 `zenoh.listen_endpoints`, control 侧改用 `--entry-point`",
        ));
    }

    if config
        .listen_endpoints
        .iter()
        .any(|endpoint| endpoint.trim().is_empty())
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.listen_endpoints 不能包含空字符串",
        ));
    }

    if config.listen_endpoints.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.listen_endpoints 至少需要一个 endpoint,并且必须包含一个非 serial 的 control 入口",
        ));
    }

    let mut has_client_reachable_endpoint = false;
    for endpoint in &config.listen_endpoints {
        validate_zenoh_endpoint(endpoint)?;
        if !is_serial_endpoint(endpoint) {
            has_client_reachable_endpoint = true;
        }
    }

    if !has_client_reachable_endpoint {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "router profile 至少需要一个非 serial 的 listen endpoint,供 `rdog control` 自动发现并加入网络（`--entry-point` 仅作 fallback）",
        ));
    }

    validate_unixpipe_config(&config.unixpipe)?;

    Ok(())
}

fn validate_unixpipe_config(config: &UnixpipeConfig) -> io::Result<()> {
    let Some(path) = config.socket_path.as_ref() else {
        return Ok(());
    };

    let path_str = path.as_os_str();
    if path_str.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.unixpipe.socket_path 不能为空字符串",
        ));
    }

    if path_str.len() > UNIXPIPE_SOCKET_PATH_MAX_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "zenoh.unixpipe.socket_path 太长: {} 字节,上限 {} 字节(macOS sun_path 限制 104 字节,Zenoh unixpipe 会派生 _uplink/_downlink FIFO,留 9 字节容差)",
                path_str.len(),
                UNIXPIPE_SOCKET_PATH_MAX_BYTES
            ),
        ));
    }

    Ok(())
}

fn validate_key_input_events_config(config: &KeyInputEventsConfig) -> io::Result<()> {
    if !config.enabled {
        return Ok(());
    }

    let keyexpr = config.keyexpr.trim();
    if keyexpr.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.key_input_events 已启用,但缺少 keyexpr",
        ));
    }

    zenoh::key_expr::nonwild_keyexpr::new(keyexpr).map_err(|err| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("zenoh.key_input_events.keyexpr 非法: {err}"),
        )
    })?;

    Ok(())
}

fn validate_zenoh_endpoint(endpoint: &str) -> io::Result<()> {
    endpoint.parse::<EndPoint>().map_err(|err| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("无法解析 zenoh endpoint `{endpoint}`: {err}"),
        )
    })?;
    Ok(())
}

fn is_serial_endpoint(endpoint: &str) -> bool {
    endpoint.trim_start().starts_with("serial/")
}

fn validate_endpoint(
    name: &str,
    enabled: bool,
    host: Option<&str>,
    port: Option<u16>,
    shell: Option<&str>,
) -> io::Result<()> {
    if !enabled {
        return Ok(());
    }

    if host.is_none_or(str::is_empty) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{name} 已启用,但缺少 host"),
        ));
    }

    if port.is_none() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{name} 已启用,但缺少 port"),
        ));
    }

    if shell.is_none_or(str::is_empty) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{name} 已启用,但缺少 shell"),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use figment::Jail;

    fn to_figment_error(err: io::Error) -> figment::Error {
        figment::Error::from(err.to_string())
    }

    mod load_daemon_config {
        use super::*;

        #[test]
        fn should_use_defaults_when_no_file_or_env_present() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                let config = load_daemon_config(None).unwrap_err();

                assert_eq!(config.kind(), ErrorKind::InvalidInput);
                Ok(())
            });
        }

        #[test]
        fn should_override_toml_values_with_environment_variables() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    default_platform_config_file_name(),
                    r#"[outbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
"#,
                )?;
                jail.set_env("RDOG_OUTBOUND__PORT", "5555");

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert_eq!(config.outbound.port, Some(5555));
                assert_eq!(config.daemon.retry_seconds, 5);
                assert_eq!(config.outbound.mode, EndpointMode::Interactive);
                assert_eq!(config.inbound.transport, ControlTransportKind::Tcp);
                Ok(())
            });
        }

        #[test]
        fn should_accept_legacy_environment_variables_as_fallback() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RCAT_OUTBOUND__ENABLED", "true");
                jail.set_env("RCAT_OUTBOUND__HOST", "127.0.0.1");
                jail.set_env("RCAT_OUTBOUND__PORT", "4444");
                jail.set_env("RCAT_OUTBOUND__SHELL", "/bin/bash");

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert!(config.outbound.enabled);
                assert_eq!(config.outbound.port, Some(4444));
                Ok(())
            });
        }

        #[test]
        fn should_prefer_new_environment_variables_over_legacy_fallback() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RCAT_OUTBOUND__ENABLED", "true");
                jail.set_env("RCAT_OUTBOUND__HOST", "127.0.0.1");
                jail.set_env("RCAT_OUTBOUND__PORT", "4444");
                jail.set_env("RCAT_OUTBOUND__SHELL", "/bin/bash");
                jail.set_env("RDOG_OUTBOUND__PORT", "5555");

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert_eq!(config.outbound.port, Some(5555));
                Ok(())
            });
        }

        #[test]
        fn should_fail_when_explicit_config_file_is_missing() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                let err = load_daemon_config(Some(Path::new("missing.toml"))).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::NotFound);
                Ok(())
            });
        }

        #[test]
        fn should_fail_when_enabled_outbound_is_missing_host() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    default_platform_config_file_name(),
                    r#"[outbound]
enabled = true
port = 4444
shell = "/bin/bash"
"#,
                )?;

                let err = load_daemon_config(None).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::InvalidInput);
                Ok(())
            });
        }

        #[test]
        fn should_reject_websocket_transport_when_inbound_mode_is_interactive() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    default_platform_config_file_name(),
                    r#"[inbound]
enabled = true
host = "127.0.0.1"
port = 5555
shell = "/bin/bash"
mode = "interactive"
transport = "websocket"
"#,
                )?;

                let err = load_daemon_config(None).unwrap_err();

                assert!(matches!(
                    err.kind(),
                    ErrorKind::InvalidInput | ErrorKind::InvalidData
                ));
                assert!(
                    err.to_string()
                        .contains("只允许和 inbound.mode = \"control\" 组合"),
                    "unexpected error: {err}"
                );
                Ok(())
            });
        }

        #[test]
        fn should_load_hidden_log_file_from_toml() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    default_platform_config_file_name(),
                    r#"[hidden]
log_file = "custom-hidden.log"

[inbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
"#,
                )?;

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert_eq!(config.hidden.log_file, PathBuf::from("custom-hidden.log"));
                Ok(())
            });
        }

        #[test]
        fn should_load_observation_config_from_toml_and_env() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    default_platform_config_file_name(),
                    r#"[observation]
durable_enabled = true
state_dir = "custom-observations"
retention_observations = 16
retention_bytes = 2097152
persist_values = false
persist_screenshots = false
write_ref_cache = true

[inbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
"#,
                )?;
                jail.set_env("RDOG_OBSERVATION__RETENTION_OBSERVATIONS", "32");

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert!(config.observation.durable_enabled);
                assert_eq!(
                    config.observation.state_dir,
                    Some(PathBuf::from("custom-observations"))
                );
                assert_eq!(config.observation.retention_observations, 32);
                assert_eq!(config.observation.retention_bytes, 2_097_152);
                assert!(!config.observation.persist_values);
                assert!(!config.observation.persist_screenshots);
                assert!(config.observation.write_ref_cache);
                Ok(())
            });
        }

        #[test]
        fn should_reject_invalid_observation_retention() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    default_platform_config_file_name(),
                    r#"[observation]
retention_observations = 0
retention_bytes = 2097152

[inbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
"#,
                )?;

                let err = load_daemon_config(None).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::InvalidInput);
                assert!(err.to_string().contains("retention_observations"));
                Ok(())
            });
        }

        #[test]
        fn should_reject_too_small_observation_retention_bytes() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    default_platform_config_file_name(),
                    r#"[observation]
retention_bytes = 1024

[inbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
"#,
                )?;

                let err = load_daemon_config(None).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::InvalidInput);
                assert!(err.to_string().contains("retention_bytes"));
                Ok(())
            });
        }

        #[test]
        fn should_fallback_to_default_rdog_toml_when_platform_file_is_missing() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    DEFAULT_CONFIG_FILE_NAME,
                    r#"[outbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
"#,
                )?;

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert_eq!(config.outbound.port, Some(4444));
                Ok(())
            });
        }

        #[test]
        fn should_fallback_to_legacy_platform_config_when_new_platform_file_is_missing() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    legacy_platform_config_file_name(),
                    r#"[outbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
"#,
                )?;

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert_eq!(config.outbound.port, Some(4444));
                Ok(())
            });
        }

        #[test]
        fn should_prefer_new_platform_config_over_legacy_platform_config() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    default_platform_config_file_name(),
                    r#"[outbound]
enabled = true
host = "127.0.0.1"
port = 5555
shell = "/bin/bash"
"#,
                )?;
                jail.create_file(
                    legacy_platform_config_file_name(),
                    r#"[outbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
"#,
                )?;

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert_eq!(config.outbound.port, Some(5555));
                Ok(())
            });
        }

        #[test]
        fn should_fallback_to_legacy_rdog_toml_when_new_default_file_is_missing() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_file(
                    LEGACY_CONFIG_FILE_NAME,
                    r#"[outbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"
"#,
                )?;

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert_eq!(config.outbound.port, Some(4444));
                Ok(())
            });
        }

        #[test]
        fn should_load_zenoh_profile_from_environment() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RDOG_ZENOH__ENABLED", "true");
                jail.set_env("RDOG_ZENOH__NAMESPACE", "lab");
                jail.set_env("RDOG_ZENOH__DAEMON_NAME", "mini-a.lab");
                jail.set_env("RDOG_ZENOH__LISTEN_ENDPOINTS", "[\"tcp/0.0.0.0:7447\"]");
                jail.set_env("RDOG_ZENOH__KEY_INPUT_EVENTS__ENABLED", "true");
                jail.set_env(
                    "RDOG_ZENOH__KEY_INPUT_EVENTS__KEYEXPR",
                    "rdog/lab/daemon/mini-a.lab/member/mini-a.lab/keyinput",
                );

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert!(config.zenoh.enabled);
                assert_eq!(config.zenoh.namespace, "lab");
                assert_eq!(config.zenoh.daemon_name.as_deref(), Some("mini-a.lab"));
                assert_eq!(
                    config.zenoh.listen_endpoints,
                    vec!["tcp/0.0.0.0:7447".to_string()]
                );
                assert!(config.zenoh.key_input_events.enabled);
                assert_eq!(
                    config.zenoh.key_input_events.keyexpr,
                    "rdog/lab/daemon/mini-a.lab/member/mini-a.lab/keyinput"
                );
                Ok(())
            });
        }

        #[test]
        fn should_infer_namespace_from_daemon_name_when_not_explicitly_set() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RDOG_ZENOH__ENABLED", "true");
                jail.set_env("RDOG_ZENOH__DAEMON_NAME", "mini-a.lab");
                jail.set_env("RDOG_ZENOH__LISTEN_ENDPOINTS", "[\"tcp/0.0.0.0:7447\"]");

                let config = load_daemon_config(None).map_err(to_figment_error)?;

                assert_eq!(config.zenoh.daemon_name.as_deref(), Some("mini-a.lab"));
                Ok(())
            });
        }

        #[test]
        fn should_fail_when_legacy_peer_mode_is_requested() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RDOG_ZENOH__ENABLED", "true");
                jail.set_env("RDOG_ZENOH__MODE", "peer");
                jail.set_env("RDOG_ZENOH__DAEMON_NAME", "mini-a.lab");
                jail.set_env("RDOG_ZENOH__LISTEN_ENDPOINTS", "[\"tcp/0.0.0.0:7447\"]");

                let err = load_daemon_config(None).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::InvalidInput);
                assert!(err.to_string().contains("`zenoh.mode = \"peer\"` 已废弃"));
                Ok(())
            });
        }

        #[test]
        fn should_fail_when_zenoh_profile_only_has_serial_endpoint() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RDOG_ZENOH__ENABLED", "true");
                jail.set_env("RDOG_ZENOH__DAEMON_NAME", "mini-a.lab");
                jail.set_env(
                    "RDOG_ZENOH__LISTEN_ENDPOINTS",
                    "[\"serial//dev/ttyFAKE#baudrate=115200\"]",
                );

                let err = load_daemon_config(None).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::InvalidInput);
                assert!(err
                    .to_string()
                    .contains("至少需要一个非 serial 的 listen endpoint"));
                Ok(())
            });
        }

        #[test]
        fn should_fail_when_zenoh_name_is_invalid() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RDOG_ZENOH__ENABLED", "true");
                jail.set_env("RDOG_ZENOH__NAMESPACE", "lab");
                jail.set_env("RDOG_ZENOH__DAEMON_NAME", "Mini A");

                let err = load_daemon_config(None).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::InvalidInput);
                assert!(err.to_string().contains("只允许小写字母"));
                Ok(())
            });
        }

        #[test]
        fn should_fail_when_zenoh_listen_endpoints_contains_empty_string() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RDOG_ZENOH__ENABLED", "true");
                jail.set_env("RDOG_ZENOH__NAMESPACE", "lab");
                jail.set_env("RDOG_ZENOH__DAEMON_NAME", "mini-a.lab");
                jail.set_env("RDOG_ZENOH__LISTEN_ENDPOINTS", "[\"\"]");

                let err = load_daemon_config(None).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::InvalidInput);
                assert!(err.to_string().contains("listen_endpoints"));
                Ok(())
            });
        }

        #[test]
        fn should_fail_when_key_input_events_are_enabled_without_keyexpr() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RDOG_ZENOH__ENABLED", "true");
                jail.set_env("RDOG_ZENOH__NAMESPACE", "lab");
                jail.set_env("RDOG_ZENOH__DAEMON_NAME", "mini-a.lab");
                jail.set_env("RDOG_ZENOH__LISTEN_ENDPOINTS", "[\"tcp/0.0.0.0:7447\"]");
                jail.set_env("RDOG_ZENOH__KEY_INPUT_EVENTS__ENABLED", "true");

                let err = load_daemon_config(None).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::InvalidInput);
                assert!(err.to_string().contains("缺少 keyexpr"));
                Ok(())
            });
        }

        #[test]
        fn should_fail_when_key_input_event_keyexpr_is_invalid() {
            Jail::expect_with(|jail| {
                jail.clear_env();
                jail.set_env("RDOG_ZENOH__ENABLED", "true");
                jail.set_env("RDOG_ZENOH__NAMESPACE", "lab");
                jail.set_env("RDOG_ZENOH__DAEMON_NAME", "mini-a.lab");
                jail.set_env("RDOG_ZENOH__LISTEN_ENDPOINTS", "[\"tcp/0.0.0.0:7447\"]");
                jail.set_env("RDOG_ZENOH__KEY_INPUT_EVENTS__ENABLED", "true");
                jail.set_env(
                    "RDOG_ZENOH__KEY_INPUT_EVENTS__KEYEXPR",
                    "rdog/lab/**/keyinput",
                );

                let err = load_daemon_config(None).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::InvalidInput);
                assert!(err.to_string().contains("keyexpr 非法"));
                Ok(())
            });
        }

        #[test]
        fn unixpipe_default_should_match_platform_expectation() {
            // macOS / Linux 上 UnixpipeConfig::default().enabled 必须为 true,
            // Windows 上必须为 false。Windows 编译期 `impl Default` 与 unix 不互通,
            // 跨平台 default 值差异是产品契约,本测试锁住。
            #[cfg(unix)]
            {
                let cfg = UnixpipeConfig::default();
                assert!(cfg.enabled, "unix 平台 default 应该启用 unixpipe fast path");
                assert!(
                    cfg.socket_path.is_none(),
                    "未显式给路径时 socket_path 必须为 None"
                );
            }
            #[cfg(windows)]
            {
                let cfg = UnixpipeConfig::default();
                assert!(!cfg.enabled, "Windows 平台 default 必须禁用 unixpipe");
                assert!(cfg.socket_path.is_none());
            }
        }

        #[test]
        fn zenoh_config_default_should_include_unixpipe_field() {
            // 防止新增字段后忘记在 ZenohConfig::default() 初始化,造成 toml 反序列化缺字段
            // 时静默走错默认值。
            let cfg = ZenohConfig::default();
            // socket_path 在 default 下必然 None。
            assert!(cfg.unixpipe.socket_path.is_none());
            // enabled 跟平台走(已由 unixpipe_default_should_match_platform_expectation 锁住)。
            #[cfg(unix)]
            assert!(cfg.unixpipe.enabled);
            #[cfg(windows)]
            assert!(!cfg.unixpipe.enabled);
        }

        #[test]
        fn validate_unixpipe_config_should_reject_oversized_socket_path() {
            // 101 字节的 path 必须被拒绝(> 100 字节上限)。
            let oversized: String = std::iter::repeat('a').take(101).collect();
            let mut cfg = UnixpipeConfig::default();
            cfg.socket_path = Some(PathBuf::from(oversized));

            let err = validate_unixpipe_config(&cfg).unwrap_err();
            assert_eq!(err.kind(), ErrorKind::InvalidInput);
            assert!(err.to_string().contains("zenoh.unixpipe.socket_path 太长"));
            assert!(err.to_string().contains("101 字节"));
        }

        #[test]
        fn validate_unixpipe_config_should_accept_under_limit_socket_path() {
            // 100 字节正好踩上限,必须能通过(> 100 才会 fail)。
            let at_limit: String = std::iter::repeat('a').take(100).collect();
            let mut cfg = UnixpipeConfig::default();
            cfg.socket_path = Some(PathBuf::from(at_limit));

            validate_unixpipe_config(&cfg).expect("100 字节应该通过");
        }

        #[test]
        fn validate_unixpipe_config_should_skip_when_socket_path_is_none() {
            // socket_path == None 时,enabled 与否都应当直接通过(socket 路径由 daemon 端自动推导)。
            let cfg = UnixpipeConfig::default();
            validate_unixpipe_config(&cfg).expect("socket_path=None 时必须通过");
        }
    }

    mod write_example_config_file {
        use super::*;
        use std::fs;

        #[test]
        fn should_create_all_platform_config_templates_when_files_are_missing() {
            Jail::expect_with(|_| {
                let paths = write_example_configs_in_place(false).map_err(to_figment_error)?;

                assert_eq!(
                    paths,
                    vec![
                        PathBuf::from(WINDOWS_CONFIG_FILE_NAME),
                        PathBuf::from(MACOS_CONFIG_FILE_NAME),
                        PathBuf::from(LINUX_CONFIG_FILE_NAME),
                    ]
                );
                assert_eq!(
                    fs::read_to_string(WINDOWS_CONFIG_FILE_NAME).map_err(to_figment_error)?,
                    WINDOWS_EXAMPLE_CONFIG_TEMPLATE
                );
                assert_eq!(
                    fs::read_to_string(MACOS_CONFIG_FILE_NAME).map_err(to_figment_error)?,
                    MACOS_EXAMPLE_CONFIG_TEMPLATE
                );
                assert_eq!(
                    fs::read_to_string(LINUX_CONFIG_FILE_NAME).map_err(to_figment_error)?,
                    LINUX_EXAMPLE_CONFIG_TEMPLATE
                );
                Ok(())
            });
        }

        #[test]
        fn should_reject_existing_platform_config_file_without_force() {
            Jail::expect_with(|jail| {
                jail.create_file(WINDOWS_CONFIG_FILE_NAME, "old")?;

                let err = write_example_configs_in_place(false).unwrap_err();

                assert_eq!(err.kind(), ErrorKind::AlreadyExists);
                Ok(())
            });
        }

        #[test]
        fn should_overwrite_existing_platform_config_files_when_force_enabled() {
            Jail::expect_with(|jail| {
                jail.create_file(WINDOWS_CONFIG_FILE_NAME, "old")?;
                jail.create_file(MACOS_CONFIG_FILE_NAME, "old")?;
                jail.create_file(LINUX_CONFIG_FILE_NAME, "old")?;

                let paths = write_example_configs_in_place(true).map_err(to_figment_error)?;

                assert_eq!(paths.len(), 3);
                assert_eq!(
                    fs::read_to_string(WINDOWS_CONFIG_FILE_NAME).map_err(to_figment_error)?,
                    WINDOWS_EXAMPLE_CONFIG_TEMPLATE
                );
                assert_eq!(
                    fs::read_to_string(MACOS_CONFIG_FILE_NAME).map_err(to_figment_error)?,
                    MACOS_EXAMPLE_CONFIG_TEMPLATE
                );
                assert_eq!(
                    fs::read_to_string(LINUX_CONFIG_FILE_NAME).map_err(to_figment_error)?,
                    LINUX_EXAMPLE_CONFIG_TEMPLATE
                );
                Ok(())
            });
        }
    }
}
