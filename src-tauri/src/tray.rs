use std::sync::atomic::Ordering;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewWindowBuilder};

use crate::commands::{
    emit_governor_update, update_governor_state_sync, AppState, GovernorStateDto,
};
use crate::core::config::GovernorProfile;
use crate::core::governor;

pub const TRAY_ID: &str = "rtv-tray";

pub struct TrayMenuState {
    pub status_item: MenuItem<tauri::Wry>,
    pub profile_off: CheckMenuItem<tauri::Wry>,
    pub profile_balanced: CheckMenuItem<tauri::Wry>,
    pub profile_gaming: CheckMenuItem<tauri::Wry>,
    pub profile_coding: CheckMenuItem<tauri::Wry>,
}

fn format_status_label(enabled: bool, profile: GovernorProfile, ram_pct: Option<u8>) -> String {
    let profile_text = if !enabled || profile == GovernorProfile::Off {
        "Idle".to_string()
    } else {
        match profile {
            GovernorProfile::Off => "Idle".to_string(),
            GovernorProfile::BalancedWork => "Balanced Work".to_string(),
            GovernorProfile::GamingFocus => "Gaming Focus".to_string(),
            GovernorProfile::CodingCompile => "Coding & Compile".to_string(),
        }
    };

    match ram_pct {
        Some(pct) => format!("Auto-Governor: {profile_text} | RAM: {pct}%"),
        None => format!("Auto-Governor: {profile_text}"),
    }
}

pub fn update_tray_summary(
    app: &AppHandle,
    enabled: bool,
    profile: GovernorProfile,
    ram_pct: Option<u8>,
) {
    let Some(menu_state) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let text = format_status_label(enabled, profile, ram_pct);
    let _ = menu_state.status_item.set_text(text);

    let effective = if enabled {
        profile
    } else {
        GovernorProfile::Off
    };
    let _ = menu_state
        .profile_off
        .set_checked(effective == GovernorProfile::Off);
    let _ = menu_state
        .profile_balanced
        .set_checked(effective == GovernorProfile::BalancedWork);
    let _ = menu_state
        .profile_gaming
        .set_checked(effective == GovernorProfile::GamingFocus);
    let _ = menu_state
        .profile_coding
        .set_checked(effective == GovernorProfile::CodingCompile);
}

pub fn show_or_recreate_main_window(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.window_visible.store(true, Ordering::Relaxed);
    }

    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }

    // Recreate main WebView window if it was closed/unloaded when minimized to tray.
    if let Some(win_cfg) = app.config().app.windows.first().cloned() {
        if let Ok(builder) = WebviewWindowBuilder::from_config(app, &win_cfg) {
            if let Ok(win) = builder.build() {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    }
}

pub fn restore_and_quit(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let cfg_dir = state.cfg_dir.clone();
        let mut gov = state.governor.lock().unwrap_or_else(|e| e.into_inner());
        governor::restore_all_on_exit(&mut gov, &cfg_dir);
    }
    app.exit(0);
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let (init_enabled, init_profile) = app
        .try_state::<AppState>()
        .map(|s| {
            let g = s.governor.lock().unwrap_or_else(|e| e.into_inner());
            (g.enabled, g.profile)
        })
        .unwrap_or((false, GovernorProfile::Off));

    let effective = if init_enabled {
        init_profile
    } else {
        GovernorProfile::Off
    };

    let status_item = MenuItem::with_id(
        app,
        "status",
        format_status_label(init_enabled, init_profile, None),
        false,
        None::<&str>,
    )?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let boost_now = MenuItem::with_id(app, "boost_now", "Boost Now", true, None::<&str>)?;

    let profile_off = CheckMenuItem::with_id(
        app,
        "profile_off",
        "Off",
        true,
        effective == GovernorProfile::Off,
        None::<&str>,
    )?;
    let profile_balanced = CheckMenuItem::with_id(
        app,
        "profile_balanced",
        "Balanced Work",
        true,
        effective == GovernorProfile::BalancedWork,
        None::<&str>,
    )?;
    let profile_gaming = CheckMenuItem::with_id(
        app,
        "profile_gaming",
        "Gaming Focus",
        true,
        effective == GovernorProfile::GamingFocus,
        None::<&str>,
    )?;
    let profile_coding = CheckMenuItem::with_id(
        app,
        "profile_coding",
        "Coding & Compile",
        true,
        effective == GovernorProfile::CodingCompile,
        None::<&str>,
    )?;

    let profile_submenu = Submenu::with_items(
        app,
        "Profile",
        true,
        &[
            &profile_off,
            &profile_balanced,
            &profile_gaming,
            &profile_coding,
        ],
    )?;

    let sep2 = PredefinedMenuItem::separator(app)?;
    let open_dashboard =
        MenuItem::with_id(app, "open_dashboard", "Open Dashboard", true, None::<&str>)?;
    let quit_restore =
        MenuItem::with_id(app, "quit_restore", "Quit & Restore", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &sep1,
            &boost_now,
            &profile_submenu,
            &sep2,
            &open_dashboard,
            &quit_restore,
        ],
    )?;

    app.manage(TrayMenuState {
        status_item,
        profile_off,
        profile_balanced,
        profile_gaming,
        profile_coding,
    });

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("rtv-highperf")
        .show_menu_on_left_click(false);

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            match id {
                "boost_now" => {
                    let app_handle = app.clone();
                    std::thread::spawn(move || {
                        let Some(state) = app_handle.try_state::<AppState>() else {
                            return;
                        };
                        let exclusions = state
                            .config
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .exclusions
                            .clone();
                        let fg_pid = governor::get_foreground_pid().unwrap_or(0);
                        if let Ok(report) = governor::trim_idle_ram(&exclusions, 150, fg_pid) {
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            let dto = {
                                let mut gov =
                                    state.governor.lock().unwrap_or_else(|e| e.into_inner());
                                gov.last_trim_freed_mb = report.freed_mb;
                                gov.last_trim_at_ms = Some(now_ms);
                                GovernorStateDto::from(&*gov)
                            };
                            emit_governor_update(&app_handle, &dto);
                        }
                    });
                }
                "profile_off" | "profile_balanced" | "profile_gaming" | "profile_coding" => {
                    let (enabled, profile) = match id {
                        "profile_balanced" => (true, GovernorProfile::BalancedWork),
                        "profile_gaming" => (true, GovernorProfile::GamingFocus),
                        "profile_coding" => (true, GovernorProfile::CodingCompile),
                        _ => (false, GovernorProfile::Off),
                    };
                    let app_handle = app.clone();
                    std::thread::spawn(move || {
                        if let Ok(dto) = update_governor_state_sync(&app_handle, enabled, profile) {
                            update_tray_summary(&app_handle, dto.enabled, dto.profile, None);
                        }
                    });
                }
                "open_dashboard" => {
                    show_or_recreate_main_window(app);
                }
                "quit_restore" => {
                    restore_and_quit(app);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            match event {
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => {
                    show_or_recreate_main_window(app);
                }
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => {
                    show_or_recreate_main_window(app);
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}
