mod cmd_init;
mod cmd_ports;
mod cmd_run;

use openbaud::{engine, mcp, workspace};

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "openbaud", version, about = "Serial-port capability layer and knowledge format for coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List serial ports
    Ports,
    /// Scaffold an openbaud workspace in DIR
    Init {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Run the MCP server over stdio (launched by the host agent via .mcp.json)
    Mcp,
    /// Execute a sedimented command: openbaud run <device>/<command> --port <p>
    Run {
        /// Command spec as <device>/<command>
        spec: String,
        /// Serial port path (see `openbaud ports`)
        #[arg(long)]
        port: Option<String>,
        /// Parameter values as key=value (repeatable); values parsed as JSON, else string
        #[arg(long = "set")]
        sets: Vec<String>,
        /// Workspace directory (defaults to the current directory)
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
        /// Required to execute commands marked risk=danger
        #[arg(long)]
        acknowledge_risk: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Ports => cmd_ports::run(),
        Command::Init { dir } => cmd_init::run(&dir),
        Command::Mcp => {
            let root = std::env::current_dir().expect("cannot determine current directory");
            let ctx = || -> anyhow::Result<Arc<mcp::Ctx>> {
                Ok(Arc::new(mcp::Ctx {
                    sessions: engine::session::SessionManager::default(),
                    workspace: workspace::Workspace::at(&root),
                    audit: engine::audit::Audit::new(&root)?,
                }))
            };
            match ctx() {
                Ok(ctx) => mcp::serve(ctx).await,
                Err(e) => Err(e),
            }
        }
        Command::Run { spec, port, sets, workspace, acknowledge_risk } => {
            cmd_run::run(&spec, port.as_deref(), &sets, &workspace, acknowledge_risk).await
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
