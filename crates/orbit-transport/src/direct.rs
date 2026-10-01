//! Direct device sessions on Iroh.
//!
//! The invite carries only the endpoint id. Sockets and a relay URL are
//! resolved by the n0 network (`presets::N0`: hole punch, then relay).
//! `Endpoint::online` is intentionally not awaited here: without a WAN it
//! waits forever, and a same-machine test must still deliver.
//!
//! Two engines in one process cannot see each other's DNS record in time, so
//! each bound endpoint publishes its loopback sockets in a process-local map.
//! Separate OS processes ignore that map and use n0.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{LazyLock, Mutex};

use iroh::endpoint::{Connection, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, PublicKey, SecretKey};
use orbit_protocol::NodeAddress;
use orbit_protocol::mailbox::MAX_ENVELOPE_BYTES;

use crate::{Result, TransportError};

pub const ALPN: &[u8] = b"orbit/direct/1";

static LOCAL: LazyLock<Mutex<HashMap<[u8; 32], Vec<SocketAddr>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Listening endpoint whose id is stable for the `SecretKey`.
pub struct DirectEndpoint {
    pub endpoint: Endpoint,
    id: [u8; 32],
}

impl std::fmt::Debug for DirectEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectEndpoint").finish_non_exhaustive()
    }
}

impl Drop for DirectEndpoint {
    fn drop(&mut self) {
        unpublish(&self.id);
    }
}

/// One inbound envelope. [`DirectEnvelope::ack`] tells the sender whether it
/// was committed locally.
pub struct DirectEnvelope {
    pub bytes: Vec<u8>,
    send: SendStream,
    conn: Connection,
}

impl std::fmt::Debug for DirectEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectEnvelope")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl DirectEnvelope {
    pub async fn ack(mut self, stored: bool) -> Result<()> {
        self.send
            .write_all(&[u8::from(stored)])
            .await
            .map_err(|error| TransportError::Connection(error.to_string()))?;
        self.send
            .finish()
            .map_err(|error| TransportError::Connection(error.to_string()))?;
        // Dropping the connection here races the sender's read on a
        // single-threaded runtime: the ack byte is lost and the sender sees
        // "connection lost". Wait until the sender closes after reading.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.conn.closed()).await;
        Ok(())
    }
}

pub async fn bind_direct(secret: SecretKey) -> Result<DirectEndpoint> {
    // Relays and n0 DNS come from the preset. Overriding the relay here would
    // force every real device back to a pasted IP.
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .map_err(|error| TransportError::Endpoint(error.to_string()))?;
    let id = endpoint_id(&endpoint);
    let addrs = loopback_sockets(&endpoint);
    if addrs.is_empty() {
        endpoint.close().await;
        return Err(TransportError::Endpoint("endpoint has no local socket".into()));
    }
    publish(id, addrs);
    Ok(DirectEndpoint { endpoint, id })
}

pub async fn send_envelope(endpoint: &Endpoint, node: &NodeAddress, envelope: &[u8]) -> Result<()> {
    deliver(endpoint, node, envelope, true).await
}

async fn deliver(endpoint: &Endpoint, node: &NodeAddress, envelope: &[u8], use_local: bool) -> Result<()> {
    if envelope.is_empty() || envelope.len() > MAX_ENVELOPE_BYTES {
        return Err(TransportError::Protocol("envelope has an invalid size"));
    }
    let addr = dial_addr(node, use_local)?;
    let conn = endpoint
        .connect(addr, ALPN)
        .await
        .map_err(|error| TransportError::Connect(error.to_string()))?;
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|error| TransportError::Connection(error.to_string()))?;
    let len = u32::try_from(envelope.len()).map_err(|_| TransportError::Protocol("envelope has an invalid size"))?;
    let mut frame = Vec::with_capacity(4 + envelope.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(envelope);
    send.write_all(&frame)
        .await
        .map_err(|error| TransportError::Connection(error.to_string()))?;
    send.finish()
        .map_err(|error| TransportError::Connection(error.to_string()))?;
    let ack = recv
        .read_to_end(8)
        .await
        .map_err(|error| TransportError::Connection(error.to_string()))?;
    conn.close(0u32.into(), b"bye");
    if ack.as_slice() != [1] {
        return Err(TransportError::Protocol("peer did not store the envelope"));
    }
    Ok(())
}

/// The next accepted envelope, or `None` when the endpoint is closed.
pub async fn next_envelope(endpoint: &Endpoint) -> Result<Option<DirectEnvelope>> {
    let Some(incoming) = endpoint.accept().await else {
        return Ok(None);
    };
    let conn = incoming
        .await
        .map_err(|error| TransportError::Connect(error.to_string()))?;
    let (send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|error| TransportError::Connection(error.to_string()))?;
    let mut len_buf = [0u8; 4];
    read_exact(&mut recv, &mut len_buf).await?;
    let len = usize::try_from(u32::from_be_bytes(len_buf)).unwrap_or(usize::MAX);
    if len == 0 || len > MAX_ENVELOPE_BYTES {
        return Err(TransportError::Protocol("envelope has an invalid size"));
    }
    let mut bytes = vec![0u8; len];
    read_exact(&mut recv, &mut bytes).await?;
    Ok(Some(DirectEnvelope { bytes, send, conn }))
}

async fn read_exact(recv: &mut RecvStream, buf: &mut [u8]) -> Result<()> {
    recv.read_exact(buf)
        .await
        .map_err(|error| TransportError::Connection(error.to_string()))
}

fn dial_addr(node: &NodeAddress, use_local: bool) -> Result<EndpointAddr> {
    let id = PublicKey::from_bytes(&node.endpoint_id).map_err(|_| TransportError::Connect("bad endpoint id".into()))?;
    let mut addr = EndpointAddr::new(id);
    for socket in &node.addrs {
        addr = addr.with_ip_addr(*socket);
    }
    if use_local {
        for socket in local_sockets(&node.endpoint_id) {
            addr = addr.with_ip_addr(socket);
        }
    }
    Ok(addr)
}

fn endpoint_id(endpoint: &Endpoint) -> [u8; 32] {
    *endpoint.secret_key().public().as_bytes()
}

fn loopback_sockets(endpoint: &Endpoint) -> Vec<SocketAddr> {
    endpoint
        .bound_sockets()
        .into_iter()
        .filter_map(|addr| {
            if !addr.is_ipv4() || addr.port() == 0 {
                return None;
            }
            let ip = if addr.ip().is_unspecified() {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            } else {
                addr.ip()
            };
            Some(SocketAddr::new(ip, addr.port()))
        })
        .collect()
}

fn publish(id: [u8; 32], addrs: Vec<SocketAddr>) {
    LOCAL
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(id, addrs);
}

fn unpublish(id: &[u8; 32]) {
    LOCAL.lock().unwrap_or_else(|error| error.into_inner()).remove(id);
}

fn local_sockets(id: &[u8; 32]) -> Vec<SocketAddr> {
    LOCAL
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(id)
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn direct_envelope_round_trip_in_process() {
        let server = bind_direct(SecretKey::generate()).await.expect("bind server");
        let client = bind_direct(SecretKey::generate()).await.expect("bind client");
        let id = endpoint_id(&server.endpoint);
        let listening = server.endpoint.clone();
        let received = tokio::spawn(async move {
            let incoming = next_envelope(&listening).await.expect("accept").expect("open");
            let bytes = incoming.bytes.clone();
            incoming.ack(true).await.expect("ack");
            bytes
        });
        // The accept future has to be polled before the dial, or the handshake
        // can finish with nobody reading the stream.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let node = NodeAddress {
            endpoint_id: id,
            addrs: Vec::new(),
        };
        send_envelope(&client.endpoint, &node, b"sealed").await.expect("send");
        assert_eq!(received.await.expect("task"), b"sealed");
    }

    /// Proves a bare endpoint id is enough when the in-process map is empty.
    /// Requires the n0 relay and DNS path; skipped unless `ORBIT_N0=1`.
    #[tokio::test]
    async fn direct_dial_by_id_over_n0() {
        if std::env::var("ORBIT_N0").ok().as_deref() != Some("1") {
            return;
        }
        let server = bind_direct(SecretKey::generate()).await.expect("bind server");
        let client = bind_direct(SecretKey::generate()).await.expect("bind client");
        let online = tokio::time::timeout(std::time::Duration::from_secs(25), async {
            server.endpoint.online().await;
            client.endpoint.online().await;
        })
        .await;
        assert!(online.is_ok(), "n0 relay was not reached");
        let id = endpoint_id(&server.endpoint);
        unpublish(&id);
        let listening = server.endpoint.clone();
        let received = tokio::spawn(async move {
            let incoming = next_envelope(&listening).await.expect("accept").expect("open");
            let bytes = incoming.bytes.clone();
            incoming.ack(true).await.expect("ack");
            bytes
        });
        let node = NodeAddress {
            endpoint_id: id,
            addrs: Vec::new(),
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            deliver(&client.endpoint, &node, b"via-n0", false),
        )
        .await
        .expect("n0 dial timed out")
        .expect("n0 dial");
        assert_eq!(received.await.expect("task"), b"via-n0");
    }
}
