use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(name = "rustdog", version, arg_required_else_help(true))]
pub struct Opts {
    #[clap(subcommand)]
    pub command: Command,
    // #[clap(short, long)]
    // verbose: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum Transport {
    Tcp,
    Zenoh,
    #[value(name = "zenoh-peer", hide = true)]
    ZenohPeerLegacy,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start a listener for incoming connections
    #[clap(alias = "l")]
    Listen {
        /// Interactive
        #[clap(short, long, name = "interactive")]
        interactive: bool,

        /// Block exit signals like CTRL-C
        #[clap(short, long, conflicts_with = "local-interactive")]
        block_signals: bool,

        /// Local interactive
        #[clap(
            short,
            long,
            name = "local-interactive",
            conflicts_with = "interactive"
        )]
        local_interactive: bool,

        /// Execute command when connection received
        #[clap(short, long)] // hidden
        exec: Option<String>,

        // Host:ip, IP if only 1 value provided
        #[clap(num_args = ..=2)]
        host: Vec<String>,
    },

    /// Connect to a controller and expose a local shell over that socket
    #[clap(alias = "c")]
    Connect {
        /// The shell to use
        #[clap(short, long)]
        shell: String,

        /// I/O mode for the exposed shell session.
        /// `control` 会进入显式的 `@...` 控制协议接收模式。
        #[clap(short = 'm', long, value_enum, default_value_t = crate::shell::ShellMode::Interactive)]
        mode: crate::shell::ShellMode,

        // Host:ip, IP if only 1 value provided
        #[clap(num_args = ..=2)]
        host: Vec<String>,
    },

    /// Connect as a line-based control sender over stdio
    Control {
        /// WebSocket URL for the remote control endpoint, for example `ws://127.0.0.1:5555/control`
        #[clap(long, conflicts_with = "host")]
        url: Option<String>,

        /// Explicit transport to use for the control lane.
        ///
        /// 省略时由运行层根据 `--url`、Zenoh 选项和位置参数推断,
        /// 避免 clap 默认值吞掉“用户没有显式选择”的语义。
        #[clap(long, value_enum)]
        transport: Option<Transport>,

        /// Namespace used by the Zenoh router/client control profile.
        #[clap(long)]
        namespace: Option<String>,

        /// Human-facing daemon target name in the Zenoh router/client control profile.
        #[clap(long = "target-name")]
        target_name: Option<String>,

        /// Optional router entry point fallback when autodiscovery is unavailable.
        #[clap(long = "entry-point")]
        entry_point: Vec<String>,

        /// Open a remote PTY session and run the command after `--`.
        #[clap(long)]
        pty: bool,

        /// Close a remote PTY session by id through the control plane.
        #[clap(long = "pty-close", conflicts_with = "pty")]
        pty_close: Option<String>,

        /// Detach from a remote PTY session by id without terminating the process.
        #[clap(long = "pty-detach", conflicts_with_all = ["pty", "pty_close"])]
        pty_detach: Option<String>,

        /// Attach to a detached remote PTY session by id.
        #[clap(long = "pty-attach", conflicts_with_all = ["pty", "pty_close", "pty_detach"])]
        pty_attach: Option<String>,

        /// TCP host/port, TCP port shorthand, or Zenoh target-name shorthand.
        ///
        /// 单个非端口值会被推断为 Zenoh target-name。
        #[clap(value_name = "HOST_OR_TARGET", num_args = 0..=2, conflicts_with = "url")]
        host: Vec<String>,

        /// Command argv for `--pty`; must appear after `--`.
        #[clap(last = true, value_name = "COMMAND", num_args = 0..)]
        pty_command: Vec<String>,
    },

    /// Start config-driven daemon mode
    #[clap(alias = "d")]
    Daemon {
        /// Optional daemon config file path
        #[clap(short, long)]
        config: Option<PathBuf>,

        /// Transport profile to use for daemon mode.
        #[clap(long, value_enum)]
        transport: Option<Transport>,

        /// Namespace used by the Zenoh router profile.
        #[clap(long)]
        namespace: Option<String>,

        /// Human-facing daemon name in the Zenoh router profile.
        #[clap(long = "name")]
        daemon_name: Option<String>,

        /// Deprecated legacy peer/peer entry-point override. Router mode rejects this.
        #[clap(long = "entry-point", hide = true)]
        entry_point: Vec<String>,
    },

    /// Start a Windows-only hidden resident daemon without changing existing daemon behavior
    #[clap(alias = "hd")]
    HiddenDaemon {
        /// Optional daemon config file path
        #[clap(short, long)]
        config: Option<PathBuf>,

        /// Internal re-entry flag for the detached hidden child process
        #[clap(long, hide = true)]
        child: bool,

        /// Internal resolved log file path for the detached hidden child process
        #[clap(long, hide = true)]
        log_file: Option<PathBuf>,
    },

    /// Manage daemon config files
    #[clap(alias = "cfg")]
    Config {
        #[clap(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Create local platform-specific config templates
    Init {
        /// Overwrite existing platform config templates
        #[clap(short, long)]
        force: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::{Command, Opts, Transport};
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn hidden_daemon_should_parse_visible_config_argument() {
        let opts = Opts::parse_from(["rdog", "hidden-daemon", "--config", "custom.toml"]);

        match opts.command {
            Command::HiddenDaemon {
                config,
                child,
                log_file,
            } => {
                assert_eq!(config, Some(PathBuf::from("custom.toml")));
                assert!(!child);
                assert_eq!(log_file, None);
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn hidden_daemon_should_parse_internal_child_arguments() {
        let opts = Opts::parse_from([
            "rdog",
            "hidden-daemon",
            "--child",
            "--log-file",
            "rdog_hidden.log",
        ]);

        match opts.command {
            Command::HiddenDaemon {
                config,
                child,
                log_file,
            } => {
                assert_eq!(config, None);
                assert!(child);
                assert_eq!(log_file, Some(PathBuf::from("rdog_hidden.log")));
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn control_should_parse_zenoh_arguments() {
        let opts = Opts::parse_from([
            "rdog",
            "control",
            "--transport",
            "zenoh",
            "--namespace",
            "lab",
            "--target-name",
            "mini-a.lab",
            "--entry-point",
            "tcp/127.0.0.1:7447",
        ]);

        match opts.command {
            Command::Control {
                url,
                transport,
                namespace,
                target_name,
                entry_point,
                host,
                ..
            } => {
                assert_eq!(url, None);
                assert_eq!(transport, Some(Transport::Zenoh));
                assert_eq!(namespace.as_deref(), Some("lab"));
                assert_eq!(target_name.as_deref(), Some("mini-a.lab"));
                assert_eq!(entry_point, vec!["tcp/127.0.0.1:7447".to_string()]);
                assert!(host.is_empty());
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn control_should_parse_websocket_url_without_host_port() {
        let opts = Opts::parse_from(["rdog", "control", "--url", "ws://127.0.0.1:5555/control"]);

        match opts.command {
            Command::Control {
                url,
                transport,
                namespace,
                target_name,
                entry_point,
                host,
                ..
            } => {
                assert_eq!(url.as_deref(), Some("ws://127.0.0.1:5555/control"));
                assert_eq!(transport, None);
                assert_eq!(namespace, None);
                assert_eq!(target_name, None);
                assert!(entry_point.is_empty());
                assert!(host.is_empty());
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn control_should_parse_single_positional_target_candidate() {
        let opts = Opts::parse_from(["rdog", "control", "mini-a.lab"]);

        match opts.command {
            Command::Control {
                url,
                transport,
                namespace,
                target_name,
                entry_point,
                host,
                ..
            } => {
                assert_eq!(url, None);
                assert_eq!(transport, None);
                assert_eq!(namespace, None);
                assert_eq!(target_name, None);
                assert!(entry_point.is_empty());
                assert_eq!(host, vec!["mini-a.lab".to_string()]);
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn control_should_parse_pty_command_after_double_dash() {
        let opts = Opts::parse_from(["rdog", "control", "mac.lab", "--pty", "--", "codex"]);

        match opts.command {
            Command::Control {
                pty,
                host,
                pty_command,
                pty_close,
                pty_detach,
                pty_attach,
                ..
            } => {
                assert!(pty);
                assert_eq!(host, vec!["mac.lab".to_string()]);
                assert_eq!(pty_command, vec!["codex".to_string()]);
                assert_eq!(pty_close, None);
                assert_eq!(pty_detach, None);
                assert_eq!(pty_attach, None);
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn control_should_parse_pty_detach_and_attach() {
        let detach_opts =
            Opts::parse_from(["rdog", "control", "mac.lab", "--pty-detach", "sess-1"]);
        match detach_opts.command {
            Command::Control {
                pty,
                pty_close,
                pty_detach,
                pty_attach,
                ..
            } => {
                assert!(!pty);
                assert_eq!(pty_close, None);
                assert_eq!(pty_detach.as_deref(), Some("sess-1"));
                assert_eq!(pty_attach, None);
            }
            command => panic!("unexpected command: {command:?}"),
        }

        let attach_opts =
            Opts::parse_from(["rdog", "control", "mac.lab", "--pty-attach", "sess-2"]);
        match attach_opts.command {
            Command::Control {
                pty,
                pty_close,
                pty_detach,
                pty_attach,
                ..
            } => {
                assert!(!pty);
                assert_eq!(pty_close, None);
                assert_eq!(pty_detach, None);
                assert_eq!(pty_attach.as_deref(), Some("sess-2"));
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn daemon_should_parse_zenoh_arguments() {
        let opts = Opts::parse_from([
            "rdog",
            "daemon",
            "--transport",
            "zenoh",
            "--namespace",
            "lab",
            "--name",
            "mini-a.lab",
            "--entry-point",
            "tcp/127.0.0.1:7447",
        ]);

        match opts.command {
            Command::Daemon {
                config,
                transport,
                namespace,
                daemon_name,
                entry_point,
            } => {
                assert_eq!(config, None);
                assert_eq!(transport, Some(Transport::Zenoh));
                assert_eq!(namespace.as_deref(), Some("lab"));
                assert_eq!(daemon_name.as_deref(), Some("mini-a.lab"));
                assert_eq!(entry_point, vec!["tcp/127.0.0.1:7447".to_string()]);
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }
}
