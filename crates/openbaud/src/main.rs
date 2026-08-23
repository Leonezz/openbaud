mod cmd_init;
mod cmd_ports;
mod cmd_run;
mod cmd_schema;

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
    /// Execute a sedimented command or workflow: openbaud run <device>/<name>
    Run {
        /// Spec as <device>/<command-or-workflow>
        spec: String,
        /// Serial port path (see `openbaud ports`); optional when the device
        /// profile declares a selector. `replay:<capture>` replays an .obcap
        /// (relative paths resolve against --workspace)
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
        /// Result JSONs longer than this many bytes are written in full to
        /// .openbaud/out/ and printed as a summary carrying a full_result
        /// path; raise it to force large results inline
        #[arg(long, default_value_t = openbaud::output::DEFAULT_MAX_INLINE_BYTES)]
        max_inline_bytes: usize,
    },
    /// Print the JSON Schema of a knowledge format (--example for annotated YAML)
    Schema {
        /// Which format to describe
        kind: cmd_schema::Kind,
        /// Print an annotated YAML example instead of the JSON Schema
        #[arg(long)]
        example: bool,
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
        Command::Run { spec, port, sets, workspace, acknowledge_risk, max_inline_bytes } => {
            cmd_run::run(&spec, port.as_deref(), &sets, &workspace, acknowledge_risk, max_inline_bytes)
                .await
        }
        Command::Schema { kind, example } => cmd_schema::run(kind, example),
    };
    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
