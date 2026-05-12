use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{self, Read, Result, Write};
use std::net::TcpStream;
use std::thread::{self, JoinHandle};

fn pipe_thread<R, W>(
    mut reader: R,
    mut writer: W,
    eof_message: Option<&'static str>,
) -> JoinHandle<Result<()>>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0_u8; 1024];

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    if let Some(message) = eof_message {
                        log::warn!("{message}");
                    }
                    return Ok(());
                }
                Ok(len) => writer.write_all(&buffer[..len])?,
                Err(err) => return Err(err),
            }

            writer.flush()?;
        }
    })
}

fn join_io_thread(handle: JoinHandle<Result<()>>) -> Result<()> {
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("pty bridge thread panicked")),
    }
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

fn default_pty_size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

// 把已经建立好的 socket 挂到 PTY 里的本地 shell 上。
pub fn shell_from_stream(stream: TcpStream, shell: &str) -> Result<()> {
    // 直接把 socket 绑给 shell 的 stdio,并不能给 bash 一个真正的终端。
    // 没有 PTY 时,job control、readline、tab completion 都会缺失。
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(default_pty_size())
        .map_err(to_io_error)?;

    let mut command = CommandBuilder::new(shell);
    command.arg("-i");

    let mut child = pair.slave.spawn_command(command).map_err(to_io_error)?;
    drop(pair.slave);

    let pty_reader = pair.master.try_clone_reader().map_err(to_io_error)?;
    let pty_writer = pair.master.take_writer().map_err(to_io_error)?;

    let socket_reader = stream.try_clone()?;
    let socket_writer = stream;

    let socket_to_pty = pipe_thread(socket_reader, pty_writer, None);
    let pty_to_socket = pipe_thread(pty_reader, socket_writer, Some("Connection lost"));

    let status = child.wait().map_err(to_io_error)?;

    join_io_thread(pty_to_socket)?;

    // 当 shell 已退出时,网络输入线程大概率还在等远端继续发数据。
    // 不强行等待它,保持和 listener 一样的收口策略。
    if socket_to_pty.is_finished() {
        join_io_thread(socket_to_pty)?;
    }

    log::warn!("Shell exited with status {status}");

    Ok(())
}
