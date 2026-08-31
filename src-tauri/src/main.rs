#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Listener, Manager,
};
use tauri_plugin_autostart::ManagerExt;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
};

/// 将 widget 窗口置底（藏在其它程序下方，不抢焦点）
#[cfg(target_os = "windows")]
fn set_widget_bottom(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) {
    let autostart = app.autolaunch();
    let _ = if enabled {
        autostart.enable()
    } else {
        autostart.disable()
    };
}

#[tauri::command]
fn set_widget_enabled(app: AppHandle, enabled: bool) {
    if let Some(widget) = app.get_webview_window("widget") {
        let _ = if enabled { widget.show() } else { widget.hide() };
    }
}

fn main() {
    let widget_only = std::env::args().any(|a| a == "--widget");

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--widget"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![set_autostart, set_widget_enabled])
        .setup(move |app| {
            // 小组件窗口：无边框透明 320x420，可拖拽，置底
            let widget = tauri::WebviewWindowBuilder::new(
                app,
                "widget",
                tauri::WebviewUrl::App("widget.html".into()),
            )
            .title("课表小组件")
            .inner_size(320.0, 420.0)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .skip_taskbar(true)
            .resizable(false)
            .build()?;

            #[cfg(target_os = "windows")]
            {
                if let Ok(hwnd) = widget.hwnd() {
                    set_widget_bottom(hwnd);
                }
            }

            // 主窗口（--widget 模式不创建，只留小组件收托盘）
            if !widget_only {
                tauri::WebviewWindowBuilder::new(
                    app,
                    "main",
                    tauri::WebviewUrl::App("index.html".into()),
                )
                .title("课表应用")
                .inner_size(1280.0, 800.0)
                .center()
                .build()?;
            }

            // 托盘菜单：呼出主窗 / 小组件 / 退出
            let show_main_i = MenuItem::with_id(app, "show_main", "显示主窗口", true, None::<&str>)?;
            let show_widget_i = MenuItem::with_id(app, "show_widget", "显示小组件", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_main_i, &show_widget_i, &quit_i])?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show_main" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        } else {
                            let _ = tauri::WebviewWindowBuilder::new(
                                app,
                                "main",
                                tauri::WebviewUrl::App("index.html".into()),
                            )
                            .title("课表应用")
                            .inner_size(1280.0, 800.0)
                            .center()
                            .build();
                        }
                    }
                    "show_widget" => {
                        if let Some(w) = app.get_webview_window("widget") {
                            let _ = w.show();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // 主程序改动事件广播同步给 widget
            let handle = app.handle().clone();
            let widget_handle = handle.clone();
            handle.listen_any("schedule:changed", move |ev| {
                if let Some(widget) = widget_handle.get_webview_window("widget") {
                    let _ = widget.emit("schedule:changed", ev.payload().to_owned());
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            // 小组件关闭改为隐藏（常驻托盘），避免误关退出
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "widget" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
