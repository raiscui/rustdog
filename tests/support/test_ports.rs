//! Loopback TCP test ports are unique within one test process. PID staggering
//! does not reserve separate ranges for different processes. The bind probe is
//! dropped before returning, so another process can still claim the port.

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
    let pid = std::process::id();
    let start = pid % (PORT_COUNT / PID_STRIDE) * PID_STRIDE;

    loop {
        // Suites claim at most hundreds of ports, including the stress
        // test. Long in-process loops or many blocked candidates can exhaust
        // the pool since failed probes also consume a claim.
        let offset = CLAIMED
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |offset| {
                (offset < PORT_COUNT).then_some(offset + 1)
            })
            .unwrap_or_else(|_| {
                panic!("test TCP port range exhausted (pid={pid}, capacity={PORT_COUNT})")
            });
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
