//! Terminal output: one place for the look of the tool.

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

pub fn banner(subtitle: &str) {
    eprintln!();
    eprintln!("  {} {}", style("stdlinux").magenta().bold(), style(subtitle).dim());
    eprintln!();
}

pub fn section(title: &str) {
    eprintln!("\n  {}", style(title).bold());
}

pub fn ok(msg: impl AsRef<str>) {
    eprintln!("  {} {}", style("✓").green().bold(), msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    eprintln!("  {} {}", style("!").yellow().bold(), msg.as_ref());
}

pub fn fail(msg: impl AsRef<str>) {
    eprintln!("  {} {}", style("✗").red().bold(), msg.as_ref());
}

pub fn info(msg: impl AsRef<str>) {
    eprintln!("  {} {}", style("·").dim(), msg.as_ref());
}

/// Indented follow-up line under a check (a fix, a detail).
pub fn hint(msg: impl AsRef<str>) {
    for line in msg.as_ref().lines() {
        eprintln!("      {}", style(line).dim());
    }
}

pub fn spinner(msg: impl Into<String>) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("  {spinner:.magenta} {msg} {elapsed:.dim}").unwrap());
    pb.set_message(msg.into());
    pb.enable_steady_tick(Duration::from_millis(90));
    pb
}

pub fn download_bar(total: u64, what: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template(
            "  {spinner:.magenta} {msg} {bar:28.magenta/dim} {bytes}/{total_bytes} {bytes_per_sec:.dim}",
        )
        .unwrap()
        .progress_chars("━━─"),
    );
    pb.set_message(what.to_string());
    pb
}
