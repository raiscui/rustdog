use std::{
    io::ErrorKind,
    net::{Ipv4Addr, TcpListener},
    sync::atomic::{AtomicU32, Ordering},
};

const FIRST_PORT: u32 = 20_000;
const PORT_COUNT: u32 = 32_768 - FIRST_PORT;
const PID_STRIDE: u32 = 128;

pub fn allocate_tcp_port() -> u16 {
    static CLAIMED: AtomicU32 = AtomicU32::new(0);
    let start = std::process::id() % (PORT_COUNT / PID_STRIDE) * PID_STRIDE;

    loop {
        let offset = CLAIMED
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |offset| {
                (offset < PORT_COUNT).then_some(offset + 1)
            })
            .expect("test TCP port range exhausted");
        let port = (FIRST_PORT + (start + offset) % PORT_COUNT) as u16;
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            Ok(_) => return port,
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::AddrInUse | ErrorKind::PermissionDenied
                ) => {}
            Err(err) => panic!("test TCP port probe failed for {port}: {err}"),
        }
    }
}
