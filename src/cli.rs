use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "winproc-tui",
    version,
    about = "Windows process investigation TUI"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
    #[arg(long, hide = true)]
    pub(crate) file_users_helper: Option<String>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Serve MCP over stdio; start WinProc TUI in Windows Terminal when needed.
    McpBridge,
}
