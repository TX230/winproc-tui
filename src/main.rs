use std::io;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{
        EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::Terminal;

mod app;
mod cli;
mod config;
mod mcp;
mod model;
mod platform;
mod samplers;
mod startup;
mod terminal;
mod ui;

pub(crate) use app::App;
use app::run_tui;
use cli::Cli;
use config::{
    build_runtime_config, load_config, migrate_legacy_config, resolve_config_paths,
    write_app_config,
};
use terminal::{AppTerminal, buffered_backend};

fn main() -> Result<()> {
    let cli = Cli::parse();
    if matches!(cli.command, Some(cli::Command::McpBridge)) {
        return mcp::bridge::run();
    }
    if let Some(request) = cli.file_users_helper {
        return samplers::file_users_scan::run_helper(&request);
    }
    let _single_instance = platform::acquire_single_instance()
        .context("failed to check for another winproc-tui instance")?
        .ok_or_else(|| anyhow::anyhow!("winproc-tui is already running"))?;
    platform::install_console_control_handler()
        .context("failed to install console control handler")?;
    let config_paths = resolve_config_paths()?;
    migrate_legacy_config(&config_paths)?;
    let config_path = config_paths.active;

    let result = (|| {
        let config = load_config(&config_path)?;
        let mouse_enabled = config.general.mouse;
        with_terminal_session(
            || setup_terminal(mouse_enabled),
            |terminal| run_application_session(terminal, &config_path, config),
            |terminal| restore_terminal(terminal, mouse_enabled),
        )
    })();
    platform::mark_shutdown_complete();
    result
}

fn run_application_session(
    terminal: &mut AppTerminal,
    config_path: &std::path::Path,
    mut config: config::AppConfig,
) -> Result<()> {
    config::prepare_app_config(&mut config);
    if config.investigation.as_ref().is_some_and(|investigation| {
        investigation.startup == config::InvestigationStartup::ChooseProfile
    }) && startup::choose_startup_investigation(terminal, &mut config)?
        == startup::StartupOutcome::Quit
    {
        return Ok(());
    }

    let mut runtime = build_runtime_config(config)?;
    runtime.config_path = Some(config_path.to_path_buf());
    let mut app = App::new(runtime)?;
    let run_result = run_tui(terminal, &mut app);
    if run_result.is_ok() {
        write_app_config(config_path, &app)?;
    }
    run_result
}

fn with_terminal_session<S, T>(
    setup: impl FnOnce() -> Result<S>,
    operation: impl FnOnce(&mut S) -> Result<T>,
    restore: impl FnOnce(&mut S) -> Result<()>,
) -> Result<T> {
    let mut session = setup()?;
    let operation_result = operation(&mut session);
    let restore_result = restore(&mut session);
    restore_result?;
    operation_result
}

fn setup_terminal(mouse_enabled: bool) -> Result<AppTerminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    if mouse_enabled {
        execute!(stdout, EnableMouseCapture)?;
    }
    let backend = buffered_backend(stdout);
    Terminal::new(backend).context("failed to create terminal")
}

fn restore_terminal(terminal: &mut AppTerminal, mouse_enabled: bool) -> Result<()> {
    let end_result = execute!(terminal.backend_mut(), EndSynchronizedUpdate);
    disable_raw_mode()?;
    if mouse_enabled {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    end_result?;
    Ok(())
}

#[cfg(test)]
mod tests;
