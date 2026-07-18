//! Zenoh运行时门面。
//!
//! 外部调用者继续通过本模块访问稳定interface;session、unixpipe与local-default
//! 的实现细节分别收敛在内部深模块中。

mod local_default;
#[cfg(unix)]
pub(crate) mod process_lease;
mod session;
#[cfg(test)]
mod test_support;
mod unixpipe;

pub use local_default::find_local_daemon_name;
#[cfg(unix)]
pub use local_default::register_local_default_daemon;
pub use session::{
    open_client_session, open_router_session, resolve_client_connect_endpoints, UnixpipeClientProbe,
};
pub use unixpipe::compose_listen_endpoints;
#[cfg(unix)]
pub use unixpipe::prepare_unixpipe_listener;
