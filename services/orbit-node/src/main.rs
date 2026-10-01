//! `orbit-node` command line.
//!
//! ```text
//! orbit-node init                  print an example configuration
//! orbit-node run --config <path>   run the node until SIGINT/SIGTERM
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use orbit_node::{Config, Node};
use tracing_subscriber::EnvFilter;

fn usage() -> ExitCode {
    eprintln!("usage:\n  orbit-node init\n  orbit-node run --config <path>");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command] if command == "init" => {
            print!("{}", orbit_node::config::EXAMPLE);
            ExitCode::SUCCESS
        }
        [command, flag, path] if command == "run" && flag == "--config" => run(PathBuf::from(path)),
        _ => usage(),
    }
}

fn run(path: PathBuf) -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let config = match Config::load(&path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("cannot start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(async move {
        let node = match Node::start(config).await {
            Ok(node) => node,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
        // Clients need this exact string to reach the node.
        println!("node address: {}", node.address());
        wait_for_shutdown().await;
        tracing::info!("shutting down");
        node.shutdown().await;
        ExitCode::SUCCESS
    })
}

async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
