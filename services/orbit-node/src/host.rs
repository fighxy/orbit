//! `orbit-node host`: run this computer as a node and publish an address
//! other clients on the network can paste into Contacts.

use std::fs;
#[cfg(windows)]
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::{Command, Stdio};

use crate::config::Config;
