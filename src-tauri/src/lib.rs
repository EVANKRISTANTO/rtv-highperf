pub mod cli;
pub mod commands;
pub mod core;
pub mod tray;

use std::sync::atomic::Ordering;
use std::time::Duration;

use sysinfo::System;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

use crate::commands::{
    apply_finding_fix, apply_process_action, apply_tweaks, emit_governor_update,
    export_backup_reg, get_config, get_governor_state, get_privilege_status, get_system_snapshot,
    list_appx_packages, list_backups, list_power_plans, list_startup_items, list_tweaks,
    remove_appx_package, restart_as_admin, restore_backup, revert_tweaks, run_cleaner,
    save_settings, sample_snapshot_from_state, scan_cleaner, set_autostart, set_governor_state,
    set_power_plan, toggle_startup_item, trim_idle_ram_now, AppState, GovernorStateDto,
};
use crate::core::{
    config::{default_config_dir, load_config, GovernorProfile},
    governor, tweaker,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Handle --cli subcommands before initializing Tauri GUI so headless CLI exits cleanly.
    let (start_minimized, cli_exit_code) = cli::check_and_run_cli();
    if let Some(code) = cli_exit_code {
        std::process::exit(code);
    }

    let cfg_dir = default_config_dir();
    let backup_dir = tweaker::default_backup_dir();

    // Recover any dirty power scheme left behind by a prior crash/unclean shutdown.
    let _ = governor::recover_dirty_power_scheme(&cfg_dir);

    let cfg = load_config(&cfg_dir).unwrap_or_default();
    let app_state = AppState::new(cfg.clone(), cfg_dir.clone(), backup_dir, start_minimized);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|a| a == "--minimized") {
                tray::show_or_recreate_main_window(app);
            }
        }))
        .manage(app_state)
        .setup(move |app| {
            if start_minimized {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.hide();
                }
            }

            tray::setup_tray(app.handle())?;

            // Resume GamingFocus power scheme if governor was persisted as active.
            if cfg.governor_enabled && cfg.active_profile == GovernorProfile::GamingFocus {
                let app_for_init = app.handle().clone();
                std::thread::spawn(move || {
                    let _ = commands::update_governor_state_sync(
                        &app_for_init,
                        true,
                        GovernorProfile::GamingFocus,
                    );
                });
            }

            // Spawn unified 1.5s background loop for Auto-Governor ticks + visible dashboard telemetry.
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut gov_sys = System::new_all();
                gov_sys.refresh_cpu_usage();
                let mut last_emitted_gov: Option<GovernorStateDto> = None;

                loop {
                    std::thread::sleep(Duration::from_millis(1500));

                    let Some(state) = app_handle.try_state::<AppState>() else {
                        break;
                    };

                    let cfg_snap = state
                        .config
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone();
                    let cfg_dir_local = state.cfg_dir.clone();

                    if cfg_snap.governor_enabled && cfg_snap.active_profile != GovernorProfile::Off {
                        let dto = {
                            let mut gov_guard =
                                state.governor.lock().unwrap_or_else(|e| e.into_inner());
                            governor::tick_governor_with_sys(
                                &mut gov_guard,
                                &cfg_snap,
                                &cfg_dir_local,
                                &mut gov_sys,
                            );
                            GovernorStateDto::from(&*gov_guard)
                        };

                        let changed = last_emitted_gov
                            .as_ref()
                            .map(|prev| {
                                prev.enabled != dto.enabled
                                    || prev.profile != dto.profile
                                    || prev.last_boosted_pid != dto.last_boosted_pid
                                    || prev.eco_qos_pids != dto.eco_qos_pids
                                    || prev.last_trim_freed_mb != dto.last_trim_freed_mb
                                    || prev.last_trim_at != dto.last_trim_at
                            })
                            .unwrap_or(true);

                        if changed {
                            emit_governor_update(&app_handle, &dto);
                            tray::update_tray_summary(&app_handle, dto.enabled, dto.profile, None);
                            last_emitted_gov = Some(dto);
                        }
                    } else if last_emitted_gov
                        .as_ref()
                        .map(|prev| prev.enabled)
                        .unwrap_or(false)
                    {
                        let dto = {
                            let mut gov_guard =
                                state.governor.lock().unwrap_or_else(|e| e.into_inner());
                            governor::restore_all_on_exit(&mut gov_guard, &cfg_dir_local);
                            GovernorStateDto::from(&*gov_guard)
                        };
                        emit_governor_update(&app_handle, &dto);
                        tray::update_tray_summary(&app_handle, dto.enabled, dto.profile, None);
                        last_emitted_gov = Some(dto);
                    }

                    if state.window_visible.load(Ordering::Relaxed) {
                        let snap = sample_snapshot_from_state(&state);
                        let ram_pct = if snap.ram_total_bytes > 0 {
                            Some(
                                ((snap.ram_used_bytes as f64 / snap.ram_total_bytes as f64) * 100.0)
                                    .round()
                                    .clamp(0.0, 100.0) as u8,
                            )
                        } else {
                            None
                        };
                        let (gov_enabled, gov_profile) = {
                            let g = state.governor.lock().unwrap_or_else(|e| e.into_inner());
                            (g.enabled, g.profile)
                        };
                        tray::update_tray_summary(&app_handle, gov_enabled, gov_profile, ram_pct);
                        let _ = app_handle.emit("telemetry-tick", &snap);
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if let Some(state) = app.try_state::<AppState>() {
                    let (minimize_to_tray, unload_webview) = {
                        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
                        (cfg.minimize_to_tray, cfg.unload_webview_on_tray)
                    };

                    if minimize_to_tray {
                        state.window_visible.store(false, Ordering::Relaxed);
                        if !unload_webview {
                            api.prevent_close();
                            let _ = window.hide();
                        }
                        return;
                    }
                }

                // Minimize-to-tray disabled: restore altered priorities/power plan before exit.
                tray::restore_and_quit(app);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_system_snapshot,
            get_privilege_status,
            restart_as_admin,
            get_config,
            save_settings,
            set_autostart,
            set_governor_state,
            get_governor_state,
            apply_process_action,
            trim_idle_ram_now,
            list_power_plans,
            set_power_plan,
            scan_cleaner,
            run_cleaner,
            list_startup_items,
            toggle_startup_item,
            list_tweaks,
            apply_tweaks,
            revert_tweaks,
            list_backups,
            restore_backup,
            export_backup_reg,
            list_appx_packages,
            remove_appx_package,
            apply_finding_fix,
        ])
        .build(tauri::generate_context!())
        .expect("error while building rtv-highperf application");

    app.run(|app_handle, event| match event {
        RunEvent::ExitRequested { api, code, .. } => {
            if code.is_none() {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    let stay_in_tray = {
                        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
                        cfg.minimize_to_tray && cfg.unload_webview_on_tray
                    };
                    if stay_in_tray {
                        api.prevent_exit();
                    }
                }
            }
        }
        RunEvent::Exit => {
            if let Some(state) = app_handle.try_state::<AppState>() {
                let cfg_dir = state.cfg_dir.clone();
                let mut gov = state.governor.lock().unwrap_or_else(|e| e.into_inner());
                governor::restore_all_on_exit(&mut gov, &cfg_dir);
            }
        }
        _ => {}
    });
}
