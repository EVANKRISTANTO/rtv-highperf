use std::path::Path;

use clap::{Parser, Subcommand};
use serde::Serialize;
use sysinfo::System;

use crate::core::{
    cleaner,
    config::{default_config_dir, load_config, GovernorProfile},
    detective,
    governor::{self, GovernorState},
    startup, tweaker,
};
use crate::core::safety::AppError;

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "rtv-highperf",
    version,
    about = "Open-source, offline-first Windows 10/11 performance optimizer and diagnostics studio"
)]
pub struct CliArgs {
    /// Start the desktop GUI minimized to the Windows system tray
    #[arg(long)]
    pub minimized: bool,

    /// Run a headless CLI subcommand without launching the desktop GUI
    #[arg(long)]
    pub cli: bool,

    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum CliCommand {
    /// Print a live JSON system health and telemetry snapshot
    Status,
    /// Run a one-shot foreground boost / idle RAM trim pass
    Boost {
        /// Trim working sets of idle background processes above 150 MB
        #[arg(long)]
        trim_ram: bool,
    },
    /// Scan or clean allowlisted temporary and cache directories
    Clean {
        /// Preview cleanable bytes and file counts without deleting files
        #[arg(long)]
        dry_run: bool,
        /// Specific category IDs to scan/clean (comma-separated or repeated)
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        categories: Vec<String>,
        /// Move deleted files to the Windows Recycle Bin when supported
        #[arg(long)]
        recycle_bin: bool,
    },
    /// Restore a saved Safe Tweaker JSON backup snapshot
    Revert {
        /// Restore the most recent backup snapshot
        #[arg(long)]
        latest: bool,
        /// Optional explicit snapshot ID to restore
        #[arg(long)]
        snapshot_id: Option<String>,
    },
}

#[derive(Debug, Serialize)]
struct BoostCliReport {
    boosted_pid: Option<u32>,
    eco_qos_count: usize,
    trim_report: Option<governor::RamTrimReport>,
}

#[derive(Debug, Serialize)]
struct RevertCliReport {
    restored_snapshot_id: String,
    label: String,
    restored_entries: usize,
}

#[derive(Debug, Serialize)]
struct DryRunCleanReport {
    dry_run: bool,
    total_bytes: u64,
    total_files: u64,
    categories: Vec<cleaner::CleanCategoryReport>,
}

/// Parses process arguments from `args` using `clap`.
pub fn parse_cli_from<I, T>(args: I) -> Result<CliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    CliArgs::try_parse_from(args)
}

/// Inspects current process arguments. If `--cli` (or a CLI subcommand) was requested,
/// executes the headless CLI command and returns `Some(exit_code)`. Returns `None` when
/// the GUI should be launched (with `minimized` flag in the `bool` out-param or via `CliArgs`).
pub fn check_and_run_cli() -> (bool, Option<i32>) {
    let parsed = match CliArgs::try_parse() {
        Ok(args) => args,
        Err(err) => {
            let code = err.exit_code();
            let _ = err.print();
            return (false, Some(code));
        }
    };

    if !parsed.cli && parsed.command.is_none() {
        return (parsed.minimized, None);
    }

    let Some(cmd) = parsed.command else {
        eprintln!("Error: --cli requires a subcommand (status, boost, clean, revert).");
        return (false, Some(1));
    };

    let cfg_dir = default_config_dir();
    let backup_dir = tweaker::default_backup_dir();

    match execute_cli_command(cmd, &cfg_dir, &backup_dir) {
        Ok(json) => {
            println!("{json}");
            (false, Some(0))
        }
        Err(err) => {
            let err_json = serde_json::to_string_pretty(&err)
                .unwrap_or_else(|_| format!("{{\"error\":\"{err}\"}}"));
            eprintln!("{err_json}");
            (false, Some(1))
        }
    }
}

pub fn execute_cli_command(
    cmd: CliCommand,
    cfg_dir: &Path,
    backup_dir: &Path,
) -> Result<String, AppError> {
    let cfg = load_config(cfg_dir)?;

    match cmd {
        CliCommand::Status => {
            let mut sys = System::new_all();
            // Brief pause so sysinfo CPU usage deltas reflect real load rather than 0%.
            std::thread::sleep(std::time::Duration::from_millis(200));
            let startup_count = startup::list_startup_items()
                .into_iter()
                .filter(|item| item.enabled)
                .count();
            let snap = detective::sample_system(&mut sys, &cfg, None, startup_count, &[]);
            Ok(serde_json::to_string_pretty(&snap)?)
        }
        CliCommand::Boost { trim_ram } => {
            let mut gov_cfg = cfg.clone();
            gov_cfg.governor_enabled = true;
            if gov_cfg.active_profile == GovernorProfile::Off {
                gov_cfg.active_profile = GovernorProfile::BalancedWork;
            }

            let mut state = GovernorState::default();
            let mut sys = System::new_all();
            std::thread::sleep(std::time::Duration::from_millis(150));
            governor::tick_governor_with_sys(&mut state, &gov_cfg, cfg_dir, &mut sys);

            let trim_report = if trim_ram {
                let fg = governor::get_foreground_pid().unwrap_or(0);
                Some(governor::trim_idle_ram(&gov_cfg.exclusions, 150, fg)?)
            } else {
                None
            };

            let report = BoostCliReport {
                boosted_pid: state.last_boosted_pid,
                eco_qos_count: state.eco_qos_pids.len(),
                trim_report,
            };
            // Ensure one-shot CLI boost does not leave dangling dirty power scheme state.
            let _ = governor::recover_dirty_power_scheme(cfg_dir);
            Ok(serde_json::to_string_pretty(&report)?)
        }
        CliCommand::Clean {
            dry_run,
            categories,
            recycle_bin,
        } => {
            let all_reports = cleaner::scan_categories();
            if !categories.is_empty() {
                for id in &categories {
                    if !all_reports.iter().any(|r| r.id == *id) {
                        return Err(AppError::InvalidInput(format!(
                            "Unknown cleaner category '{id}'"
                        )));
                    }
                }
            }

            if dry_run {
                let filtered: Vec<_> = if categories.is_empty() {
                    all_reports
                } else {
                    all_reports
                        .into_iter()
                        .filter(|r| categories.contains(&r.id))
                        .collect()
                };
                let total_bytes = filtered.iter().map(|c| c.bytes).sum();
                let total_files = filtered.iter().map(|c| c.file_count).sum();
                let report = DryRunCleanReport {
                    dry_run: true,
                    total_bytes,
                    total_files,
                    categories: filtered,
                };
                Ok(serde_json::to_string_pretty(&report)?)
            } else {
                let target_ids: Vec<String> = if categories.is_empty() {
                    all_reports
                        .into_iter()
                        .filter(|c| !c.opt_in_default_off && !c.requires_admin)
                        .map(|c| c.id)
                        .collect()
                } else {
                    categories
                };
                let res = cleaner::clean_categories(&target_ids, recycle_bin)?;
                Ok(serde_json::to_string_pretty(&res)?)
            }
        }
        CliCommand::Revert {
            latest,
            snapshot_id,
        } => {
            let backups = tweaker::list_backups(backup_dir);
            let target = if let Some(id) = snapshot_id {
                backups
                    .into_iter()
                    .find(|b| b.id == id)
                    .ok_or_else(|| AppError::InvalidInput(format!("Backup snapshot '{id}' not found")))?
            } else if latest {
                backups.into_iter().next().ok_or_else(|| {
                    AppError::InvalidInput("No backup snapshots available to revert".to_string())
                })?
            } else {
                return Err(AppError::InvalidInput(
                    "Specify --latest or --snapshot-id <id> to revert".to_string(),
                ));
            };

            tweaker::restore_snapshot(&target.id, backup_dir)?;
            let report = RevertCliReport {
                restored_snapshot_id: target.id,
                label: target.label,
                restored_entries: target.entries.len(),
            };
            Ok(serde_json::to_string_pretty(&report)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parses_clean_dry_run_and_revert_latest() {
        let clean_args = parse_cli_from([
            "rtv-highperf",
            "--cli",
            "clean",
            "--dry-run",
            "--categories",
            "user_temp,shader_cache",
            "--recycle-bin",
        ])
        .expect("should parse --cli clean --dry-run");
        assert!(clean_args.cli);
        assert!(!clean_args.minimized);
        assert_eq!(
            clean_args.command,
            Some(CliCommand::Clean {
                dry_run: true,
                categories: vec!["user_temp".to_string(), "shader_cache".to_string()],
                recycle_bin: true,
            })
        );

        let revert_args = parse_cli_from(["rtv-highperf", "--cli", "revert", "--latest"])
            .expect("should parse --cli revert --latest");
        assert!(revert_args.cli);
        assert_eq!(
            revert_args.command,
            Some(CliCommand::Revert {
                latest: true,
                snapshot_id: None,
            })
        );

        let status_args =
            parse_cli_from(["rtv-highperf", "--cli", "status"]).expect("should parse --cli status");
        assert!(status_args.cli);
        assert_eq!(status_args.command, Some(CliCommand::Status));

        let boost_args = parse_cli_from(["rtv-highperf", "--cli", "boost", "--trim-ram"])
            .expect("should parse --cli boost --trim-ram");
        assert!(boost_args.cli);
        assert_eq!(
            boost_args.command,
            Some(CliCommand::Boost { trim_ram: true })
        );

        let min_args =
            parse_cli_from(["rtv-highperf", "--minimized"]).expect("should parse --minimized");
        assert!(min_args.minimized);
        assert!(!min_args.cli);
        assert_eq!(min_args.command, None);
    }
}
