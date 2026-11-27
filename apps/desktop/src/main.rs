#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use qo_server::DesktopClient;
use std::{
    env, io,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

fn connect_or_start() -> io::Result<DesktopClient> {
    if let Ok(client) = DesktopClient::discover() {
        return Ok(client);
    }
    let mut command = Command::new(env::current_exe()?);
    command
        .arg("--serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW for the background service.
    }
    let mut child = command.spawn()?;
    // Startup includes creating the private discovery files. Keep the UI wait bounded.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(client) = DesktopClient::discover() {
            thread::spawn(move || child.wait());
            return Ok(client);
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "QO service exited ({status}); check port {} and the runtime directory",
                qo_server::Settings::load()?.port
            )));
        }
        if Instant::now() >= deadline {
            thread::spawn(move || child.wait());
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "QO service startup timed out",
            ));
        }
        thread::sleep(Duration::from_millis(100)); // Poll for atomic endpoint publication.
    }
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args().nth(1).as_deref() == Some("--serve") {
        return Ok(qo_server::run()?);
    }
    let background = env::args().any(|arg| arg == "--background");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|arg| arg == "--background") {
                show_main(app);
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--background"])
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let login_url = connect_or_start()?.login_link()?.parse()?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(login_url))
                .title("QO")
                .visible(!background)
                .build()?;
            let show = MenuItem::with_id(app, "show", "打开 QO", true, None::<&str>)?;
            let browser = MenuItem::with_id(app, "browser", "在浏览器中打开", true, None::<&str>)?;
            let auto = CheckMenuItem::with_id(
                app,
                "autostart",
                "登录时启动",
                true,
                app.autolaunch().is_enabled()?,
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "退出桌面壳", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &browser, &auto, &quit])?;
            let icon = app.default_window_icon().ok_or("missing QO icon")?.clone();
            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("QO")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "browser" => {
                        let app = app.clone();
                        thread::spawn(move || {
                            if qo_server::open_browser().is_err() {
                                let _ = app
                                    .notification()
                                    .builder()
                                    .title("QO")
                                    .body("无法在默认浏览器中打开 QO，请确认本机服务正在运行。")
                                    .show();
                            }
                        });
                    }
                    "autostart" => {
                        let manager = app.autolaunch();
                        let result = auto.is_checked().and_then(|checked| {
                            if checked {
                                manager.enable()
                            } else {
                                manager.disable()
                            }
                            .map_err(|err| tauri::Error::Anyhow(err.into()))
                        });
                        if result.is_err() {
                            if let Ok(enabled) = manager.is_enabled() {
                                let _ = auto.set_checked(enabled);
                            }
                            let _ = app
                                .notification()
                                .builder()
                                .title("QO")
                                .body("无法更新开机自启设置")
                                .show();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if window.hide().is_ok() {
                    let _ = window
                        .app_handle()
                        .notification()
                        .builder()
                        .title("QO")
                        .body("服务仍在后台运行，可从托盘打开窗口。")
                        .show();
                }
            }
        })
        .run(tauri::generate_context!())?;
    Ok(())
}
