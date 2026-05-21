use slint::PlatformError;

slint::slint! {
    export component AppWindow inherits Window {
        title: "Click-Through Template";
        no-frame: true;
        width: 400px;
        height: 300px;
        always-on-top: true;
        background: transparent;

        callback request-drag();

        Rectangle {
            width: 100%; height: 50%;
            background: #000000; // 穿透区
        }

        Rectangle {
            y: 150px; width: 100%; height: 50%;
            background: #202020; // 拖动区
            Text { text: "拖动区域"; color: white; }
            TouchArea {
                pointer-event(event) => {
                    if (event.kind == PointerEventKind.down) { root.request-drag(); }
                }
            }
        }
    }
}

fn main() -> Result<(), PlatformError> {
    let ui = AppWindow::new()?;
    let ui_weak = ui.as_weak();
    ui.show()?;

    #[cfg(target_os = "windows")]
    init_windows_platform(ui_weak);

    ui.run()
}

#[cfg(target_os = "windows")]
fn init_windows_platform(ui_weak: slint::Weak<AppWindow>) {
    slint::spawn_local(async move {
        use i_slint_backend_winit::WinitWindowAccessor;
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, WPARAM};
        use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
        use windows::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongW, HTCAPTION, HWND_TOPMOST, LWA_COLORKEY, SWP_FRAMECHANGED,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SendMessageW, SetLayeredWindowAttributes,
            SetWindowLongW, SetWindowPos, WM_NCLBUTTONDOWN, WS_EX_LAYERED,
        };

        let ui = match ui_weak.upgrade() {
            Some(u) => u,
            None => return,
        };

        if let Ok(winit_window) = ui.window().winit_window().await {
            if let Ok(handle) = winit_window.window_handle() {
                if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                    let hwnd = HWND(win32_handle.hwnd.get() as *mut std::ffi::c_void);

                    unsafe {
                        // 1. 设置分层样式
                        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
                        // 这里如果返回 Result，直接使用 .is_err() 或 unwrap()
                        let _ =
                            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);

                        // 2. 激活颜色键穿透 (黑色穿透)
                        // 关键修复：不要检查 .0，直接使用 .is_err() 判断是否失败
                        if SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY).is_err() {
                            panic!("Failed to set layered window attributes");
                        }

                        // 3. 刷新窗口属性
                        if SetWindowPos(
                            hwnd,
                            Some(HWND_TOPMOST),
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                        )
                        .is_err()
                        {
                            panic!("Failed to set window pos");
                        }
                    }

                    ui.on_request_drag(move || unsafe {
                        // ReleaseCapture 和 SendMessage 不返回 Result，无需处理
                        let _ = ReleaseCapture();
                        let _ = SendMessageW(
                            hwnd,
                            WM_NCLBUTTONDOWN,
                            Some(WPARAM(HTCAPTION as usize)),
                            Some(LPARAM(0)),
                        );
                    });
                }
            }
        }
    })
    .unwrap();
}
