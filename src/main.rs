mod config;
mod desktop;
mod doctor;
mod import;
mod install;
mod launch;
mod system;
mod tune;
mod ui;
mod wine;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use config::{presets, Config};
use console::style;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "stdlinux", version, about = "Low-latency osu! stable on Linux")]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download Wine, set up the prefix and the game, register file associations
    Install {
        /// Do not ask anything; take the recommended choice everywhere
        #[arg(short, long)]
        yes: bool,
        /// Ignore existing osu! installs made by other tools
        #[arg(long)]
        fresh: bool,
        /// Preset to start with (see `stdlinux preset`)
        #[arg(long)]
        preset: Option<String>,
        /// Use this folder for the game (must contain osu!.exe, or be empty for a new download)
        #[arg(long)]
        osu_dir: Option<PathBuf>,
    },
    /// Start osu!
    Run {
        /// Arguments passed to osu!.exe
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Import beatmaps (.osz), skins (.osk), replays (.osr) or osu:// links
    Import {
        #[arg(required = true)]
        items: Vec<String>,
    },
    /// Show presets, or switch to one
    Preset { name: Option<String> },
    /// Check the system for things that cost latency or smoothness
    Doctor {
        /// Measure the real mouse report rate instead
        #[arg(long)]
        mouse: bool,
    },
    /// Apply the system settings that need root (realtime limits, CPU sleep control)
    Tune {
        #[arg(short, long)]
        yes: bool,
        /// Remove what `tune` wrote
        #[arg(long)]
        undo: bool,
    },
    /// Show the resolved settings and the environment the game is started with
    Show,
    /// Stop the game and its Wine server
    Kill,
    /// Remove Wine, the prefix and desktop integration (the game folder is kept)
    Uninstall {
        #[arg(short, long)]
        yes: bool,
    },
}

fn preset_cmd(mut cfg: Config, name: Option<String>) -> Result<()> {
    match name {
        None => {
            ui::banner("presets");
            for p in presets() {
                let mark = if p.name == cfg.preset { style("●").magenta() } else { style("○").dim() };
                eprintln!("  {mark} {:<9} {}", style(p.name).bold(), style(p.about).dim());
            }
            eprintln!();
            ui::hint(format!(
                "Switch: stdlinux preset <name>. Fine-tune single settings under [overrides] in\n{}",
                config::config_path().display()
            ));
        }
        Some(n) => {
            if !presets().iter().any(|p| p.name == n) {
                bail!("unknown preset '{n}' (run `stdlinux preset` for the list)");
            }
            cfg.preset = n;
            cfg.save()?;
            ui::ok(format!("preset set to {}", cfg.preset));
        }
    }
    Ok(())
}

fn show(cfg: &Config) -> Result<()> {
    let s = cfg.settings()?;
    let p = launch::plan(&s);
    ui::banner("settings");
    ui::info(launch::describe(cfg, &p, &s));
    ui::info(format!("prefix   {}", cfg.prefix.display()));
    ui::info(format!("game     {}", cfg.osu_dir.display()));
    ui::info(format!("wine     {}", wine::Wine::locate().root.display()));
    ui::info(format!("config   {}", config::config_path().display()));
    ui::section("Environment");
    for (k, v) in &p.env {
        match v {
            Some(v) => println!("  {k}={v}"),
            None => println!("  unset {k}"),
        }
    }
    for (k, v) in &cfg.env {
        println!("  {k}={v}");
    }
    Ok(())
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load()?;
    match cli.command {
        // Bare `stdlinux`: do the obvious next thing.
        None if cfg.installed() => launch::run(&cfg, &[]),
        None => install::install(cfg, install::Options { yes: false, fresh: false, preset: None, osu_dir: None }),
        Some(Cmd::Install { yes, fresh, preset, osu_dir }) => {
            install::install(cfg, install::Options { yes, fresh, preset, osu_dir })
        }
        Some(Cmd::Run { args }) => launch::run(&cfg, &args),
        Some(Cmd::Import { items }) => import::import(&cfg, &items),
        Some(Cmd::Preset { name }) => preset_cmd(cfg, name),
        Some(Cmd::Doctor { mouse: true }) => doctor::mouse_test(8),
        Some(Cmd::Doctor { mouse: false }) => {
            ui::banner("doctor");
            let n = doctor::report(&cfg, false);
            eprintln!();
            if n == 0 {
                ui::ok("no problems found");
            } else {
                ui::info(format!("{n} thing(s) to look at"));
            }
            Ok(())
        }
        Some(Cmd::Tune { yes, undo }) => tune::tune(yes, undo),
        Some(Cmd::Show) => show(&cfg),
        Some(Cmd::Kill) => launch::kill(&cfg),
        Some(Cmd::Uninstall { yes }) => install::uninstall(&cfg, yes),
    }
}

fn main() {
    if let Err(e) = real_main() {
        ui::fail(format!("{e:#}"));
        std::process::exit(1);
    }
}
