//! `orbit-cli`: exercise an orbit-node mailbox from the command line.
//!
//! Payloads sent with this tool are NOT end-to-end encrypted: the node
//! operator can read them. Use it only with test data.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use orbit_protocol::NodeAddress;
use orbit_protocol::mailbox::{DepositToken, MAX_FETCH_ITEMS, MailboxId};
use orbit_transport::{MailboxClient, client_endpoint};

#[derive(Parser)]
#[command(version, about = "Test client for orbit-node mailboxes (test data only: no E2EE)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a mailbox key file and print the mailbox ID.
    Keygen { key: PathBuf },
    /// Create the mailbox on a node (needs the node's registration code).
    Register {
        #[arg(long)]
        node: NodeAddress,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        code: Option<String>,
    },
    /// Create a deposit token, register it and print it for a sender.
    TokenCreate {
        #[arg(long)]
        node: NodeAddress,
        #[arg(long)]
        key: PathBuf,
    },
    /// Revoke a deposit token.
    TokenRevoke {
        #[arg(long)]
        node: NodeAddress,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        token: String,
    },
    /// Deposit a text into someone's mailbox.
    Send {
        #[arg(long)]
        node: NodeAddress,
        /// Recipient mailbox ID (hex).
        #[arg(long)]
        to: String,
        #[arg(long)]
        token: String,
        #[arg(long)]
        text: String,
    },
    /// Print stored items; with --ack delete them afterwards.
    Fetch {
        #[arg(long)]
        node: NodeAddress,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        ack: bool,
    },
    /// Print mailbox usage and limits.
    Status {
        #[arg(long)]
        node: NodeAddress,
        #[arg(long)]
        key: PathBuf,
    },
}

type CliResult = Result<(), String>;

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse().command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Command) -> CliResult {
    match command {
        Command::Keygen { key } => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
            write_private(&key, hex::encode(bytes).as_bytes())?;
            println!("mailbox: {}", mailbox_of(&SigningKey::from_bytes(&bytes)).to_hex());
            Ok(())
        }
        Command::Register { node, key, code } => {
            let key = read_key(&key)?;
            let mut client = connect(&node).await?;
            let created = client
                .authenticate(&key, code.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            println!(
                "mailbox {} {}",
                mailbox_of(&key).to_hex(),
                if created { "created" } else { "already exists" }
            );
            finish(client).await
        }
        Command::TokenCreate { node, key } => {
            let client = owner(&node, &key).await?;
            let token = DepositToken::generate().map_err(|e| e.to_string())?;
            client.add_deposit_token(&token).await.map_err(|e| e.to_string())?;
            println!("token: {}", token.to_hex());
            finish(client).await
        }
        Command::TokenRevoke { node, key, token } => {
            let token = DepositToken::from_hex(&token).ok_or("token must be 64 hex characters")?;
            let client = owner(&node, &key).await?;
            client.remove_deposit_token(&token).await.map_err(|e| e.to_string())?;
            println!("token revoked");
            finish(client).await
        }
        Command::Send { node, to, token, text } => {
            let mailbox = MailboxId::from_hex(&to).ok_or("--to must be 64 hex characters")?;
            let token = DepositToken::from_hex(&token).ok_or("--token must be 64 hex characters")?;
            let client = connect(&node).await?;
            let receipt = client
                .deposit(&mailbox, &token, text.as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            println!(
                "{} {} (expires at {} ms)",
                if receipt.duplicate { "already stored" } else { "stored" },
                receipt.id.to_hex(),
                receipt.expires_at_ms
            );
            finish(client).await
        }
        Command::Fetch { node, key, ack } => {
            let client = owner(&node, &key).await?;
            let mut after = 0;
            let mut ids = Vec::new();
            loop {
                let (items, more) = client.fetch(after, MAX_FETCH_ITEMS).await.map_err(|e| e.to_string())?;
                for item in &items {
                    println!(
                        "#{} {} {}",
                        item.seq,
                        &item.id.to_hex()[..16],
                        String::from_utf8_lossy(&item.envelope)
                    );
                    after = item.seq;
                    ids.push(item.id);
                }
                if !more {
                    break;
                }
            }
            if ack && !ids.is_empty() {
                for chunk in ids.chunks(orbit_protocol::mailbox::MAX_ACK_ITEMS) {
                    client.ack(chunk).await.map_err(|e| e.to_string())?;
                }
                println!("acknowledged {}", ids.len());
            }
            finish(client).await
        }
        Command::Status { node, key } => {
            let client = owner(&node, &key).await?;
            let status = client.status().await.map_err(|e| e.to_string())?;
            println!(
                "items {}/{}, bytes {}/{}, tokens {}, ttl {} s",
                status.items,
                status.max_items,
                status.bytes,
                status.max_bytes,
                status.deposit_tokens,
                status.ttl_seconds
            );
            finish(client).await
        }
    }
}

async fn connect(node: &NodeAddress) -> Result<MailboxClient, String> {
    let endpoint = client_endpoint().await.map_err(|e| e.to_string())?;
    MailboxClient::connect(&endpoint, node).await.map_err(|e| e.to_string())
}

async fn owner(node: &NodeAddress, key: &Path) -> Result<MailboxClient, String> {
    let key = read_key(key)?;
    let mut client = connect(node).await?;
    client.authenticate(&key, None).await.map_err(|e| e.to_string())?;
    Ok(client)
}

async fn finish(client: MailboxClient) -> CliResult {
    client.close();
    client.endpoint().close().await;
    Ok(())
}

fn mailbox_of(key: &SigningKey) -> MailboxId {
    MailboxId(key.verifying_key().to_bytes())
}

fn read_key(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = [0u8; 32];
    hex::decode_to_slice(text.trim(), &mut bytes).map_err(|_| "key file must contain 64 hex characters")?;
    Ok(SigningKey::from_bytes(&bytes))
}

fn write_private(path: &Path, contents: &[u8]) -> CliResult {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(contents).map_err(|e| e.to_string())
}
