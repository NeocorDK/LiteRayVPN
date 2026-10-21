//! TCP-connect latency to a server, used for the server list before connecting.

use std::time::{Duration, Instant};

use tokio::net::{lookup_host, TcpStream};
use tokio::time::timeout;

pub async fn tcp_ping(host: &str, port: u16, limit: Duration) -> Option<u32> {
    let addr = timeout(limit, lookup_host((host, port))).await.ok()?.ok()?.next()?;
    let started = Instant::now();
    timeout(limit, TcpStream::connect(addr)).await.ok()?.ok()?;
    Some(started.elapsed().as_millis().max(1) as u32)
}
