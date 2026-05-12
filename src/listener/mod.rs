use colored::Colorize;
use rustyline::error::ReadlineError;
#[cfg(unix)]
use std::io::BufRead;
#[cfg(unix)]
use std::io::IsTerminal;
use std::io::{self, stdin, stdout, Read, Result, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};

#[cfg(unix)]
mod termios_handler;

#[cfg(unix)]
use signal_hook::{consts, flag, iterator::Signals};

#[cfg(unix)]
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

#[cfg(unix)]
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub struct Opts {
    pub host: String,
    pub port: String,
    pub exec: Option<String>,
    pub block_signals: bool,
    pub mode: Mode,
}

pub enum Mode {
    Normal,
    Interactive,
    LocalInteractive,
}

fn print_connection_received() {
    log::info!("Connection Received");
}

// It will complain on unix systems without this lint rule.
#[allow(dead_code)]
fn print_feature_not_supported() {
    log::error!("This feature is not supported on your platform");
}

fn pipe_thread<R, W>(
    mut r: R,
    mut w: W,
    eof_message: Option<&'static str>,
) -> JoinHandle<Result<()>>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0; 1024];

        loop {
            match r.read(&mut buffer) {
                Ok(0) => {
                    if let Some(message) = eof_message {
                        log::warn!("{message}");
                    }
                    return Ok(());
                }
                Ok(len) => {
                    w.write_all(&buffer[..len])?;
                }
                Err(err) => {
                    return Err(err);
                }
            }

            w.flush()?;
        }
    })
}

fn listen_tcp_normal(stream: TcpStream, opts: &Opts) -> Result<()> {
    if let Some(exec) = &opts.exec {
        stream
            .try_clone()?
            .write_all(format!("{}\n", exec).as_bytes())?;
    }

    let (stdin_thread, stdout_thread) = (
        pipe_thread(stdin(), stream.try_clone()?, None),
        pipe_thread(stream, stdout(), Some("Connection lost")),
    );

    print_connection_received();

    join_io_thread(stdout_thread)?;

    // 当远端先断开时,stdin 线程很可能还阻塞在终端输入上。
    // 这里不强行等待它,让一次性 CLI 直接返回,由进程退出时回收线程。
    if stdin_thread.is_finished() {
        join_io_thread(stdin_thread)?;
    }

    Ok(())
}

fn block_signals(should_block: bool) -> Result<()> {
    if should_block {
        #[cfg(unix)]
        {
            Signals::new(&[consts::SIGINT])?;
        }

        #[cfg(not(unix))]
        {
            print_feature_not_supported();
            return Err(io::Error::other(
                "blocking signals is only supported on unix",
            ));
        }
    }

    Ok(())
}

#[cfg(unix)]
fn install_suspend_exit_flag() -> Result<Arc<AtomicBool>> {
    // `^Z` 默认会把前台进程挂起。
    // 对 `listen` 来说,这会留下还占着端口的 stopped 进程。
    let should_exit = Arc::new(AtomicBool::new(false));

    flag::register(consts::SIGTSTP, Arc::clone(&should_exit))
        .map_err(|err| io::Error::other(err.to_string()))?;

    Ok(should_exit)
}

#[cfg(unix)]
fn accept_connection(
    listener: &TcpListener,
    should_exit: &AtomicBool,
) -> Result<(TcpStream, std::net::SocketAddr)> {
    // 把阻塞式 `accept()` 改成短轮询。
    // 这样收到 `SIGTSTP` 后,主线程能尽快看见退出标记并释放监听 socket。
    listener.set_nonblocking(true)?;

    loop {
        if should_exit.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "listener received SIGTSTP and is shutting down",
            ));
        }

        match listener.accept() {
            Ok((stream, addr)) => {
                // 监听 socket 为了可轮询退出被设成了 nonblocking。
                // 已接入的会话仍然应该回到原来的阻塞式语义,否则后续读不到数据时会把
                // `WouldBlock` 误当成真正错误。
                stream.set_nonblocking(false)?;
                return Ok((stream, addr));
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(err) => return Err(err),
        }
    }
}
// Listen on given host and port
pub fn listen(opts: &Opts) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", opts.host, opts.port))?;

    #[cfg(not(unix))]
    {
        if let Mode::Interactive = opts.mode {
            print_feature_not_supported();
            return Err(io::Error::other(
                "interactive listen mode is only supported on unix",
            ));
        }
    }

    log::info!("Listening on {}:{}", opts.host.green(), opts.port.cyan());

    #[cfg(unix)]
    let suspend_exit_flag = install_suspend_exit_flag()?;

    #[cfg(unix)]
    let (mut stream, _) = match accept_connection(&listener, suspend_exit_flag.as_ref()) {
        Ok(stream) => stream,
        Err(err) if err.kind() == io::ErrorKind::Interrupted => return Ok(()),
        Err(err) => return Err(err),
    };

    #[cfg(not(unix))]
    let (mut stream, _) = listener.accept()?;

    match &opts.mode {
        Mode::Interactive => {
            // It exists it if isn't unix above
            block_signals(opts.block_signals)?;

            #[cfg(unix)]
            {
                termios_handler::setup_fd()?;
                listen_tcp_normal(stream, opts)?;
            }
        }
        Mode::LocalInteractive => {
            let output_thread = pipe_thread(stream.try_clone()?, stdout(), Some("Connection lost"));

            print_connection_received();

            readline_decorator(
                || output_thread.is_finished(),
                |command| stream.write_all(format!("{command}\n").as_bytes()),
            )?;

            // local-interactive 退出时,显式关闭写半边,让对端知道不会再有新输入。
            // 这样控制模式下的响应可以完整回流回来,避免本地进程过早退出吞掉输出。
            match stream.shutdown(Shutdown::Write) {
                Ok(()) => {}
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::NotConnected | io::ErrorKind::BrokenPipe
                    ) => {}
                Err(err) => return Err(err),
            }
            join_io_thread(output_thread)?;
        }
        Mode::Normal => {
            block_signals(opts.block_signals)?;
            listen_tcp_normal(stream, opts)?;
        }
    }

    Ok(())
}

/* readline_decorator takes in a function, A mutable closure
 * which will perform the sending of data depending on the transport protocol. */
fn readline_decorator(
    mut should_stop: impl FnMut() -> bool,
    mut f: impl FnMut(String) -> Result<()>,
) -> Result<()> {
    #[cfg(unix)]
    if !stdin().is_terminal() {
        return line_reader_decorator(should_stop, f);
    }

    let mut rl = rustyline::DefaultEditor::new().map_err(to_io_error)?;

    loop {
        if should_stop() {
            return Ok(());
        }

        match rl.readline(">> ") {
            Ok(command) => {
                rl.add_history_entry(command.clone().as_str())
                    .map_err(to_io_error)?;
                f(command)?;
            }
            Err(err) => match err {
                ReadlineError::Interrupted | ReadlineError::Eof => return Ok(()),
                err => return Err(to_io_error(err)),
            },
        }
    }
}

#[cfg(unix)]
fn line_reader_decorator(
    mut should_stop: impl FnMut() -> bool,
    mut f: impl FnMut(String) -> Result<()>,
) -> Result<()> {
    let stdin = stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut line = String::new();

    loop {
        if should_stop() {
            return Ok(());
        }

        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            return Ok(());
        }

        let command = line.trim_end_matches(['\r', '\n']).to_owned();
        f(command)?;
    }
}

fn join_io_thread(handle: JoinHandle<Result<()>>) -> Result<()> {
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("listener worker thread panicked")),
    }
}

fn to_io_error(err: ReadlineError) -> io::Error {
    io::Error::other(err.to_string())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::accept_connection;
    #[cfg(unix)]
    use std::{
        io,
        net::TcpListener,
        sync::{atomic::AtomicBool, Arc},
    };

    #[cfg(unix)]
    #[test]
    fn accept_connection_should_stop_when_suspend_flag_is_set() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral listener should bind");
        let should_exit = Arc::new(AtomicBool::new(true));

        let err = accept_connection(&listener, should_exit.as_ref()).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    }
}
