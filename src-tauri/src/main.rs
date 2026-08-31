#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
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

// ==================== 自动更新机制 ====================
// 远程清单地址：可通过环境变量 SCHEDULE_UPDATE_URL 覆盖（便于测试/配置真实仓库）
const UPDATE_URL_KEY: &str = "SCHEDULE_UPDATE_URL";
const DEFAULT_UPDATE_URL: &str =
    "https://raw.githubusercontent.com/YOUR_REPO/schedule/main/version.json";
// 更新暂存目录：%APPDATA%\schedule\update\
const OLD_FILE_MAX_AGE_DAYS: u64 = 60; // 旧版备份/暂存超期自动清理阈值（天）

fn manifest_url() -> String {
    std::env::var(UPDATE_URL_KEY).unwrap_or_else(|_| DEFAULT_UPDATE_URL.to_string())
}

fn update_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("schedule").join("update")
}

fn new_exe_path() -> PathBuf {
    update_dir().join("schedule_new.exe")
}

fn new_version_marker() -> PathBuf {
    update_dir().join("schedule_new.version")
}

/// 语义化版本比较：a > b
fn version_gt(a: &str, b: &str) -> bool {
    let pa: Vec<u32> = a
        .trim()
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let pb: Vec<u32> = b
        .trim()
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    for i in 0..pa.len().max(pb.len()) {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

#[derive(serde::Deserialize)]
struct UpdateManifest {
    version: String,
    url: String,
    #[serde(default)]
    hash: String,
}

/// 拉取远程 version.json（5s 超时），失败静默返回 None
fn fetch_manifest() -> Option<UpdateManifest> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let resp = client.get(manifest_url()).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<UpdateManifest>().ok()
}

/// 计算文件 SHA256（小写 hex）
fn sha256_file(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(format!("{:x}", Sha256::digest(&buf)))
}

/// 下载远程 exe 到暂存目录，校验 SHA256 后落位待命
fn download_and_verify(manifest: &UpdateManifest) -> bool {
    let dir = update_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        let _ = e;
        return false;
    }
    let tmp = dir.join("schedule_new.tmp");
    let dest = new_exe_path();

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(_) => return false,
    };
    let bytes = match client.get(&manifest.url).send().and_then(|r| r.bytes()) {
        Ok(b) => b.to_vec(),
        Err(_) => return false,
    };
    if bytes.is_empty() {
        return false;
    }
    if let Err(e) = std::fs::write(&tmp, &bytes) {
        let _ = e;
        return false;
    }
    // 哈希校验：远程未提供 hash 时跳过校验
    if !manifest.hash.is_empty() {
        let got = sha256_file(&tmp).unwrap_or_default();
        if !got.eq_ignore_ascii_case(&manifest.hash) {
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
    }
    // 落位为新 exe，并记录新版本号供二次登录提示
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = e;
        return false;
    }
    let _ = std::fs::write(new_version_marker(), manifest.version.trim());
    true
}

/// 清理超期旧版备份与暂存文件（超过 60 天自动删除，静默）
fn cleanup_old_files() {
    let dir = update_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let now = std::time::SystemTime::now();
        for e in entries.flatten() {
            let p = e.path();
            let age_days = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .map(|d| d.as_secs() / 86400)
                .unwrap_or(0);
            if age_days > OLD_FILE_MAX_AGE_DAYS {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    // exe 同目录旧版备份 schedule_old.exe
    if let Ok(cur) = std::env::current_exe() {
        if let Some(parent) = cur.parent() {
            let old = parent.join("schedule_old.exe");
            if old.exists() {
                if let Ok(meta) = old.metadata() {
                    if let Ok(t) = meta.modified() {
                        if let Some(d) = now_age_days(t) {
                            if d > OLD_FILE_MAX_AGE_DAYS {
                                let _ = std::fs::remove_file(&old);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn now_age_days(t: std::time::SystemTime) -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(t)
        .ok()
        .map(|d| d.as_secs() / 86400)
}

/// 后台静默检查 + 下载：启动时调用，失败/超时/断网静默跳过
fn background_update_check(app: AppHandle) {
    cleanup_old_files();
    let local = app.package_info().version.to_string();
    let Some(manifest) = fetch_manifest() else {
        return;
    };
    if !version_gt(&manifest.version, &local) {
        return; // 远程不高于本地，无需更新
    }
    let _ = download_and_verify(&manifest);
}

// ==================== Tauri 命令 ====================

/// 返回当前本地版本号（来自 tauri.conf.json version）
#[tauri::command]
fn get_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 检查更新状态：返回本地版本、是否已下载新版本、新版本号
#[tauri::command]
fn check_update(app: AppHandle) -> serde_json::Value {
    let local = app.package_info().version.to_string();
    let new_exe = new_exe_path();
    let downloaded = new_exe.exists();
    let new_version = std::fs::read_to_string(new_version_marker())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    serde_json::json!({
        "local_version": local,
        "downloaded": downloaded,
        "has_new": downloaded && !new_version.is_empty() && version_gt(&new_version, &local),
        "new_version": new_version,
    })
}

/// 立即更新：备份当前 exe -> 用新 exe 覆盖 -> 重启新版本
#[tauri::command]
fn apply_update(app: AppHandle) -> bool {
    let new_exe = new_exe_path();
    if !new_exe.exists() {
        return false;
    }
    let Ok(cur) = std::env::current_exe() else {
        return false;
    };
    let Some(dir) = cur.parent() else {
        return false;
    };
    let old_exe = dir.join("schedule_old.exe");
    let upd_dir = update_dir();

    // 批处理：等待本进程退出后，备份旧版 -> 覆盖 -> 启动新版
    let bat = upd_dir.join("apply_update.bat");
    let bat_content = format!(
        "@echo off\r\nchcp 65001 >nul\r\ntimeout /t 2 /nobreak >nul\r\n\
         move /y \"{}\" \"{}\" >nul 2>&1\r\n\
         move /y \"{}\" \"{}\" >nul 2>&1\r\n\
         start \"\" \"{}\"\r\n\
         del \"%~f0\"\r\n",
        cur.display(),
        old_exe.display(),
        new_exe.display(),
        cur.display(),
        cur.display()
    );
    if let Err(e) = std::fs::create_dir_all(&upd_dir) {
        let _ = e;
    }
    if std::fs::write(&bat, bat_content).is_err() {
        return false;
    }
    // 异步启动批处理，然后退出当前进程
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "\"\"", &bat.to_string_lossy()])
        .spawn();
    app.exit(0);
    true
}

// ==================== 原有逻辑 ====================

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
        .invoke_handler(tauri::generate_handler![
            set_autostart,
            set_widget_enabled,
            get_version,
            check_update,
            apply_update
        ])
        .setup(move |app| {
            // 后台静默检查更新（不阻塞启动，失败静默）
            let app_handle = app.handle().clone();
            std::thread::spawn(move || background_update_check(app_handle));

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
