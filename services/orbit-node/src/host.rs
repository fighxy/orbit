//! `orbit-node host`: run this computer as a node and publish an address
//! other clients on the network can paste into Contacts.

use std::fs;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::io::Write;
#[cfg(windows)]
use std::process::{Command, Stdio};

use crate::config::Config;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostOptions {
    pub data_dir: PathBuf,
    pub port: u16,
    pub public_ip: Option<Ipv4Addr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedHost {
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
    pub port: u16,
    pub registration_code: String,
}

pub fn default_data_dir() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("Orbit").join("node");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local").join("share").join("orbit-node");
    }
    PathBuf::from("orbit-node-data")
}

pub fn parse_args(args: &[String]) -> Result<HostOptions, String> {
    let mut options = HostOptions {
        data_dir: default_data_dir(),
        port: 7443,
        public_ip: None,
    };
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args.get(index + 1).ok_or_else(|| format!("missing value for {flag}"))?;
        match flag {
            "--data-dir" => options.data_dir = PathBuf::from(value),
            "--port" => {
                options.port = value.parse().map_err(|_| format!("invalid port: {value}"))?;
            }
            "--public" => {
                options.public_ip = Some(value.parse().map_err(|_| format!("invalid IPv4 address: {value}"))?);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 2;
    }
    if options.port == 0 {
        return Err("port must be fixed so the address can be shared".into());
    }
    Ok(options)
}

/// Writes a stable config: same node key directory and registration code
/// across restarts, and a concrete IPv4 address instead of `0.0.0.0`.
pub fn prepare(options: &HostOptions) -> Result<PreparedHost, String> {
    fs::create_dir_all(&options.data_dir)
        .map_err(|error| format!("cannot create {}: {error}", options.data_dir.display()))?;
    let public_ip = match options.public_ip {
        Some(ip) => ip,
        None => primary_ipv4().ok_or_else(|| {
            "no shareable IPv4 address was found; pass --public <ip> (the address other devices use to reach this computer)".to_owned()
        })?,
    };
    if !is_shareable(public_ip) {
        return Err(format!(
            "{public_ip} cannot be shared; pass --public with the address other devices use to reach this computer"
        ));
    }
    let registration_code = load_or_create_code(&options.data_dir.join("registration-code"))?;
    let config_path = options.data_dir.join("host.toml");
    let text = render_config(&options.data_dir, options.port, public_ip, &registration_code);
    Config::parse(&text).map_err(|error| error.to_string())?;
    fs::write(&config_path, text).map_err(|error| format!("cannot write {}: {error}", config_path.display()))?;
    Ok(PreparedHost {
        data_dir: options.data_dir.clone(),
        config_path,
        port: options.port,
        registration_code,
    })
}

pub fn share_text(address: &str, code: &str, port: u16) -> String {
    format!(
        "\
node address: {address}
registration code: {code}

Адрес можно отправить людям, с которыми хотите общаться.
В клиенте откройте «Контакты и подключение» и вставьте адрес и код.
Устройства должны быть в одной сети Wi-Fi, либо до этого компьютера должен быть открыт UDP-порт {port}.
"
    )
}

/// Saves the card next to the node data and copies it to the clipboard.
pub fn publish(dir: &Path, address: &str, code: &str, port: u16) -> String {
    let mut text = share_text(address, code, port);
    let path = dir.join("share.txt");
    match fs::write(&path, &text) {
        Ok(()) => text.push_str(&format!("Файл с адресом: {}\n", path.display())),
        Err(error) => eprintln!("не удалось записать {}: {error}", path.display()),
    }
    if copy_text(&text) {
        text.push_str("Текст скопирован в буфер обмена.\n");
    }
    text
}

pub fn allow_udp(port: u16) {
    #[cfg(windows)]
    {
        let name = format!("Orbit Node UDP {port}");
        let _ = Command::new("netsh")
            .args(["advfirewall", "firewall", "delete", "rule", &format!("name={name}")])
            .status();
        let added = Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={name}"),
                "dir=in",
                "action=allow",
                "protocol=UDP",
                &format!("localport={port}"),
            ])
            .status();
        if !added.map(|status| status.success()).unwrap_or(false) {
            eprintln!(
                "Брандмауэр Windows не открыл UDP {port}. Запустите программу от имени администратора один раз или разрешите её вручную."
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = port;
    }
}

pub fn is_shareable(ip: Ipv4Addr) -> bool {
    !ip.is_unspecified() && !ip.is_loopback() && !ip.is_broadcast() && !ip.is_multicast() && !ip.is_link_local()
}

fn primary_ipv4() -> Option<Ipv4Addr> {
    // UDP connect does not send a packet; the kernel fills in the source
    // address of the default route, which is the LAN address to share.
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()? {
        SocketAddr::V4(addr) => is_shareable(*addr.ip()).then_some(*addr.ip()),
        SocketAddr::V6(_) => None,
    }
}

fn load_or_create_code(path: &Path) -> Result<String, String> {
    if path.exists() {
        let code = fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?
            .trim()
            .to_owned();
        if code.len() < 12 || code.chars().any(char::is_whitespace) {
            return Err(format!(
                "{} must contain one registration code of at least 12 characters",
                path.display()
            ));
        }
        return Ok(code);
    }
    let code = random_code()?;
    fs::write(path, &code).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(code)
}

fn random_code() -> Result<String, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes).map_err(|error| format!("random number generator failed: {error}"))?;
    Ok(bytes
        .iter()
        .map(|byte| ALPHABET[(*byte as usize) % ALPHABET.len()] as char)
        .collect())
}

fn render_config(data_dir: &Path, port: u16, public_ip: Ipv4Addr, code: &str) -> String {
    let dir = toml_basic_string(&data_dir.display().to_string());
    let code = toml_basic_string(code);
    format!(
        "\
data_dir = {dir}
listen = [\"0.0.0.0:{port}\"]
public_addrs = [\"{public_ip}:{port}\"]

[mailbox]
registration_code = {code}
"
    )
}

fn toml_basic_string(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' | '"' => {
                out.push('\\');
                out.push(character);
            }
            character if character.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(character))),
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn copy_text(text: &str) -> bool {
    #[cfg(windows)]
    {
        let mut child = match Command::new("clip").stdin(Stdio::piped()).spawn() {
            Ok(child) => child,
            Err(_) => return false,
        };
        if let Some(mut stdin) = child.stdin.take()
            && stdin.write_all(text.as_bytes()).is_err()
        {
            return false;
        }
        child.wait().map(|status| status.success()).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_missing_flag_value_and_port_zero() {
        assert!(parse_args(&["--port".into()]).is_err());
        assert!(parse_args(&["--port".into(), "0".into()]).is_err());
        assert!(parse_args(&["--public".into(), "not-an-ip".into()]).is_err());
        assert!(parse_args(&["--nope".into()]).is_err());
    }

    #[test]
    fn parses_an_explicit_address() {
        let options = parse_args(&[
            "--data-dir".into(),
            "node-data".into(),
            "--port".into(),
            "7444".into(),
            "--public".into(),
            "203.0.113.10".into(),
        ])
        .unwrap();
        assert_eq!(options.data_dir, PathBuf::from("node-data"));
        assert_eq!(options.port, 7444);
        assert_eq!(options.public_ip, Some(Ipv4Addr::new(203, 0, 113, 10)));
    }

    #[test]
    fn shareable_addresses_exclude_loopback_and_wildcards() {
        assert!(!is_shareable(Ipv4Addr::UNSPECIFIED));
        assert!(!is_shareable(Ipv4Addr::LOCALHOST));
        assert!(!is_shareable(Ipv4Addr::new(169, 254, 1, 1)));
        assert!(is_shareable(Ipv4Addr::new(192, 168, 1, 20)));
        assert!(is_shareable(Ipv4Addr::new(203, 0, 113, 10)));
    }

    #[test]
    fn prepared_config_advertises_the_public_address_and_keeps_the_code() {
        let dir = tempfile::tempdir().unwrap();
        let options = HostOptions {
            data_dir: dir.path().to_owned(),
            port: 7443,
            public_ip: Some(Ipv4Addr::new(192, 168, 1, 20)),
        };
        let first = prepare(&options).unwrap();
        let second = prepare(&options).unwrap();
        assert_eq!(first.registration_code, second.registration_code);
        assert_eq!(first.registration_code.len(), 20);
        let text = fs::read_to_string(&first.config_path).unwrap();
        let config = Config::parse(&text).unwrap();
        assert_eq!(config.public_addrs, vec!["192.168.1.20:7443".parse().unwrap()]);
        assert_eq!(
            config.mailbox.registration_code.as_deref(),
            Some(first.registration_code.as_str())
        );
        let card = share_text("ab@192.168.1.20:7443", &first.registration_code, 7443);
        assert!(card.contains("192.168.1.20:7443"));
        assert!(card.contains(&first.registration_code));
        assert!(!card.contains("0.0.0.0"));
    }
}
