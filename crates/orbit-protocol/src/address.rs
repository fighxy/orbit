//! Text form of a peer or node address.
//!
//! * `<64 hex chars>` is a direct endpoint id. The QUIC/TLS key is the
//!   address; sockets are discovered (or taken from the in-process map).
//! * `<64 hex chars>@<ip:port>[,<ip:port>...]` is a mailbox node with
//!   directly reachable UDP sockets.
//!
//! The endpoint ID is the QUIC/TLS public key, so a dial verifies the peer
//! and not only an IP.

use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAddress {
    /// Ed25519 public key of the node's transport identity.
    pub endpoint_id: [u8; 32],
    /// Directly reachable UDP addresses.
    pub addrs: Vec<SocketAddr>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("node address must be <64 hex chars> or <64 hex chars>@<ip:port>[,<ip:port>]: {0}")]
pub struct AddressParseError(&'static str);

impl FromStr for NodeAddress {
    type Err = AddressParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let (id, sockets) = match s.split_once('@') {
            Some((id, sockets)) => (id, Some(sockets)),
            None => (s, None),
        };
        let mut endpoint_id = [0u8; 32];
        hex::decode_to_slice(id, &mut endpoint_id).map_err(|_| AddressParseError("bad endpoint id"))?;
        let addrs = match sockets {
            None => Vec::new(),
            Some(sockets) => {
                let addrs = sockets
                    .split(',')
                    .map(|addr| addr.trim().parse::<SocketAddr>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| AddressParseError("bad socket address"))?;
                if addrs.is_empty() {
                    return Err(AddressParseError("no socket address"));
                }
                addrs
            }
        };
        Ok(Self { endpoint_id, addrs })
    }
}

impl fmt::Display for NodeAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let id = hex::encode(self.endpoint_id);
        if self.addrs.is_empty() {
            return f.write_str(&id);
        }
        write!(f, "{id}@")?;
        for (i, addr) in self.addrs.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{addr}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let text = format!("{}@203.0.113.10:7443,[2001:db8::1]:7443", "ab".repeat(32));
        let address: NodeAddress = text.parse().unwrap();
        assert_eq!(address.addrs.len(), 2);
        assert_eq!(address.to_string(), text);
    }

    #[test]
    fn bare_endpoint_id_round_trips_without_sockets() {
        let text = "ab".repeat(32);
        let address: NodeAddress = text.parse().unwrap();
        assert!(address.addrs.is_empty());
        assert_eq!(address.to_string(), text);
    }

    #[test]
    fn rejects_malformed() {
        assert!("abc".parse::<NodeAddress>().is_err());
        assert!(format!("{}@", "ab".repeat(32)).parse::<NodeAddress>().is_err());
        assert!("zz@127.0.0.1:1".parse::<NodeAddress>().is_err());
        assert!(format!("{}@host:1", "ab".repeat(32)).parse::<NodeAddress>().is_err());
    }
}
