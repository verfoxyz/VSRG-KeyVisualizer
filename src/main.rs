// 告诉 Windows 链接器这是一个 GUI 应用，不显示控制台窗口,仅在 Release 模式下生效
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod configs;
mod core;
mod events;
mod gui {
    pub mod param_panel_window;
    pub mod settings_window;
}
mod physics;
mod platform;
mod ri_table;
mod ui;

// ====================模块定义====================
use crossbeam_channel as channel;
use i_slint_backend_winit::WinitWindowAccessor;
use slint::ComponentHandle;
use std::thread;
//use tracing_subscriber::fmt; // 或者 use tracing_subscriber;
use crate::core::config_def::{AppConfig, MyKeyEvent};
use crate::platform::window::{calculate_window_size, make_window_clickthrough, restore_window_position, MAIN_HWND};
use crate::ri_table::win_vkey_to_rdev_str;
use ui::state::AppState;


slint::include_modules!();

// 公开 re-export 供其他模块使用
pub use core::color::{hex_str_to_color, merge_alpha, split_alpha};
pub use core::config_def::{BarNote, KeyConfig, MyKeyEvent as MyKeyEventAlias};
pub use platform::window::SafeHWND;
pub use ui::model::{compute_key_ratios, create_model, create_model_with_selection, KeyIndexMap, ToKeyData};

/// 加载配置（新系统：从 configs/profiles/ 读取）
///
/// 首次调用时会初始化目录结构和迁移旧 config.json。
/// 返回 (AppConfig, profile_name)。
fn load_config() -> (AppConfig, String) {
    configs::initialize();
    configs::load_active_profile()
}

/// 保存配置到当前激活的 profile
///
/// 需要从外部传入 profile 名（从 AppState 获取）。
pub fn save_config_to_profile(name: &str, config: &AppConfig) {
    configs::save_profile(name, config);
}

/// 创建设置窗口并绑定回调
fn create_settings_window(
    state: &AppState,
    main_ui_weak: &slint::Weak<MainWindow>,
) -> Option<SettingsWindow> {
    // 检查是否已有设置窗口打开，防止重复创建
    if let Some(holder) = state.settings_holder.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && let Some(existing) = holder.upgrade() {
            if let Err(e) = existing.show() {
                tracing::error!("重复激活设置窗口失败: {}", e);
            }
            return None;
        }
    let settings = SettingsWindow::new().ok()?;
    if let Err(e) = settings.show() {
        tracing::error!("显示设置窗口失败: {}", e);
        return None;
    }
    gui::settings_window::setup_settings_window(settings, state.clone(), main_ui_weak.clone());
    None
}

/// ========================================主函数========================================
/// ======================================== MAIN ========================================
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let max_level = if cfg!(debug_assertions) {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    tracing_subscriber::fmt()
        .with_max_level(max_level)
        .init();
    tracing::info!("[INFO] 程序启动，正在初始化...");

    let (tx, rx) = channel::unbounded::<MyKeyEvent>();
    let (init_config, profile_name) = load_config();
    tracing::info!("加载配置 profile: {}", profile_name);
    let state = AppState::new(init_config, &profile_name);

    // 初始化按键位置缓存
    {
        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let mut cache = state.key_positions.lock().unwrap_or_else(|e| e.into_inner());
        cache.clear();
        for k in &cfg.keys {
            cache.push((k.rdev_key_name.clone(), k.x, k.y));
        }
    }

    let ui = MainWindow::new()?;

    // 1. 初始化 UI 全局表现属性
    {
        let cfg = state.config.lock().unwrap_or_else(|e| e.into_inner());
        let (width, height) = calculate_window_size(&cfg);
        //转换单位
        ui.set_window_width_px(width);
        ui.set_window_height_px(height);
        ui.set_global_border_width(cfg.global_border_width);
        ui.set_global_border_color(hex_str_to_color(&cfg.global_border_color));
        ui.set_global_key_color(hex_str_to_color(&cfg.global_key_color));
        ui.set_key_margin_width(cfg.key_margin_width);
        ui.set_top_boundary_px(cfg.top_boundary);
        let key_model = create_model(&cfg.keys);
        compute_key_ratios(&key_model, width as f32, height as f32);
        ui.set_keys(key_model);
        ui.set_flow_direction(cfg.flow_direction);
        // 计算按键区域高度：最大物理 Y 范围 + 底部边距
        let max_bottom = cfg.keys.iter().map(|k| k.y + k.height).max().unwrap_or(0);
        let key_area_h = if max_bottom > 0 {
            max_bottom + cfg.key_margin_width
        } else {
            100
        };
        ui.set_key_area_height(key_area_h);

        // 根据构建类型控制调试覆盖层显示
        let is_debug = cfg!(debug_assertions);
        ui.set_show_debug_overlay(is_debug);
    }

    // 2. 绑定主窗体单击/双击检测事件
    // ⚠️ moved 每帧触发多次，用标志位防止反复中断/重启拖动
    use std::sync::atomic::{AtomicBool, Ordering};
    static DRAG_ACTIVE: AtomicBool = AtomicBool::new(false);

    let last_click = std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
    let ui_weak_click = ui.as_weak();
    let state_for_click = state.clone();
    ui.on_gui_click(move |_x, _y| {
        // 重置拖动标志，允许下次拖动
        DRAG_ACTIVE.store(false, Ordering::SeqCst);

        let now = std::time::Instant::now();
        let elapsed = now.duration_since(*last_click.lock().unwrap());
        *last_click.lock().unwrap() = now;

        if elapsed.as_millis() < 300 {
            // 双击：打开设置窗口
            create_settings_window(&state_for_click, &ui_weak_click);
        }
        // 单击不再触发拖拽，由 gui_drag_window 回调处理
    });

    // 3. 绑定主窗体拖拽事件（由按键区域 TouchArea 的 moved 触发）
    // 使用 Code_template 方案：ReleaseCapture + SendMessage(WM_NCLBUTTONDOWN, HTCAPTION)
    ui.on_gui_drag_window(move || {
        if DRAG_ACTIVE.swap(true, Ordering::SeqCst) {
            return; // 已在拖动中，跳过
        }
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
        use windows::Win32::UI::WindowsAndMessaging::{HTCAPTION, SendMessageW, WM_NCLBUTTONDOWN};
        let guard = MAIN_HWND.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(safe) = *guard {
            let hwnd = safe.0;
            unsafe {
                let _ = ReleaseCapture();
                let _ = SendMessageW(
                    hwnd,
                    WM_NCLBUTTONDOWN,
                    Some(WPARAM(HTCAPTION as usize)),
                    Some(LPARAM(0)),
                );
            }
        }
    });
    // 绑定右键菜单中的"打开配置"
    let state_for_settings = state.clone();
    let ui_weak_settings = ui.as_weak();
    ui.on_request_settings(move || {
        create_settings_window(&state_for_settings, &ui_weak_settings);
    });
    // 保存窗口位置 + 当前 profile
    let state_for_close = state.clone();
    let ui_weak_close = ui.as_weak();
    ui.on_request_close(move || {
        if let Some(ui) = ui_weak_close.upgrade() {
            let pos = ui.window().position();
            let mut cfg = state_for_close.config.lock().unwrap_or_else(|e| e.into_inner());
            cfg.window_x = Some(pos.x);
            cfg.window_y = Some(pos.y);
            let profile = state_for_close.current_profile.lock().unwrap_or_else(|e| e.into_inner()).clone();
            save_config_to_profile(&profile, &cfg);
        }
        if let Err(e) = slint::quit_event_loop() {
            tracing::error!("退出事件循环失败: {}", e);
        }
    });

    // 4. 开启后台高性能 Timer 渲染时钟驱动
    let _event_timer = events::start_event_timer(rx, state.clone(), ui.as_weak());

    // 5. 显示窗口
    ui.show()?;

    // 6. ⚠️ 必须使用 spawn_local + async 在事件循环内部获取 winit 窗口
    //    同步的 with_winit_window 在 show() 后可能因窗口未就绪而静默跳过
    {
        let state_for_winit = state.clone();
        let ui_weak = ui.as_weak();
        slint::spawn_local(async move {
            let ui = match ui_weak.upgrade() {
                Some(u) => u,
                None => {
                    tracing::error!("[setup] 窗口弱引用已失效");
                    return;
                }
            };

            match ui.window().winit_window().await {
                Ok(winit_window) => {
                    #[cfg(windows)]
                    make_window_clickthrough(&winit_window);

                    // 恢复窗口位置（含多屏边界检测）
                    let cfg = state_for_winit.config.lock().unwrap_or_else(|e| e.into_inner());
                    restore_window_position(&winit_window, &cfg);

                    tracing::debug!("[setup] 窗口穿透属性已激活");
                }
                Err(e) => {
                    tracing::error!("[setup] 获取 winit 窗口失败: {:?}", e);
                }
            }
        })?;
    }

    init_platform_input_listener(tx, &ui);

    tracing::debug!("[DEBUG] ---> 启动 Slint 全局事件循环...");
    slint::run_event_loop()?;
    Ok(())
}

#[cfg(unix)]
fn init_platform_input_listener(tx: channel::Sender<MyKeyEvent>, _ui: &MainWindow) {
    thread::spawn(move || {
        if let Err(e) = rdev::listen(move |event| {
            let my_event = match event.event_type {
                rdev::EventType::KeyPress(k) => Some(MyKeyEvent::Press {
                    rdev_name: format!("{:?}", k),
                }),
                rdev::EventType::KeyRelease(k) => Some(MyKeyEvent::Release {
                    rdev_name: format!("{:?}", k),
                }),
                _ => None,
            };
            if let Some(ev) = my_event {
                let _ = tx.try_send(ev);
            }
        }) {
            eprintln!("Unix rdev Hook 错误: {:?}", e);
        }
    });
}

#[cfg(windows)]
fn init_platform_input_listener(tx: channel::Sender<MyKeyEvent>, _ui: &MainWindow) {
    use std::mem::size_of;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::{
        GetRawInputData,
        HRAWINPUT,
        RAWINPUT,
        RAWINPUTDEVICE,
        RAWINPUTHEADER,
        RID_INPUT, // 某些版本中，这俩属于 UI::Input 模块
        RIDEV_INPUTSINK,
        RIM_TYPEKEYBOARD,
        RegisterRawInputDevices,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW,
        DefWindowProcW,
        DestroyWindow,
        DispatchMessageW,
        GWLP_USERDATA,
        GetMessageW,
        HWND_MESSAGE, // WM_INPUT 和 HWND_MESSAGE 实际属于 WindowsAndMessaging
        RegisterClassW,
        TranslateMessage,
        WM_DESTROY,
        WM_INPUT,
        WNDCLASSW,
        WS_EX_LEFT,
    };
    use windows::core::PCWSTR;

    let tx_clone = tx.clone();

    thread::spawn(move || {
        unsafe {
            // 1. 定义私有窗口回调
            unsafe extern "system" fn wnd_proc(
                hwnd: HWND,
                msg: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                if msg == WM_INPUT {
                    // 从 GWLP_USERDATA 取出之前存入的 Sender 指针
                    let tx_ptr = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                            hwnd,
                            GWLP_USERDATA,
                        )
                    } as *const channel::Sender<MyKeyEvent>;

                    if !tx_ptr.is_null() {
                        // 解引用裸指针
                        let tx = unsafe { &*tx_ptr };

                        let mut size: u32 = 0;

                        // 获取所需缓冲区大小
                        let ret = unsafe {
                            GetRawInputData(
                                HRAWINPUT(lparam.0 as *mut std::ffi::c_void),
                                RID_INPUT,
                                None,
                                &mut size,
                                size_of::<RAWINPUTHEADER>() as u32,
                            )
                        };

                        if ret != u32::MAX {
                            let mut buffer = vec![0u8; size as usize];

                            // 获取实际的 Raw Input 数据
                            let ret = unsafe {
                                GetRawInputData(
                                    HRAWINPUT(lparam.0 as *mut std::ffi::c_void),
                                    RID_INPUT,
                                    Some(buffer.as_mut_ptr() as *mut std::ffi::c_void),
                                    &mut size,
                                    size_of::<RAWINPUTHEADER>() as u32,
                                )
                            };

                            if ret != u32::MAX {
                                // 解引用裸指针
                                let raw = unsafe { &*(buffer.as_ptr() as *const RAWINPUT) };

                                if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
                                    // 访问联合体(union)字段
                                    let keyboard = unsafe { &raw.data.keyboard };
                                    let vkey = keyboard.VKey;
                                    let flags = keyboard.Flags;

                                    if vkey != 255 {
                                        let is_release = (flags & 1) != 0;
                                        let key_name = win_vkey_to_rdev_str(vkey);

                                        let event = if is_release {
                                            MyKeyEvent::Release {
                                                rdev_name: key_name.to_string(),
                                            }
                                        } else {
                                            MyKeyEvent::Press {
                                                rdev_name: key_name.to_string(),
                                            }
                                        };
                                        let _ = tx.send(event);
                                    }
                                }
                            }
                        }
                    }
                    return LRESULT(0);
                } else if msg == WM_DESTROY {
                    unsafe {
                        windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                    }
                    return LRESULT(0);
                }

                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }

            // 2. 注册窗口类
            let class_name: Vec<u16> = "VSRG_KeyVisualizer_Sink_Class\0".encode_utf16().collect();
            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: HINSTANCE(std::ptr::null_mut()),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassW(&wnd_class);

            // 3. 创建纯消息窗口 (修复 Option<HWND> 和 HWND_MESSAGE 的匹配问题)
            let hwnd_msg_sink = CreateWindowExW(
                WS_EX_LEFT,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(std::ptr::null()),
                windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE), // 修复点：某些版本此参数需要 Some() 包裹
                None,
                Some(HINSTANCE(std::ptr::null_mut())),
                None,
            )
            .expect("无法创建 Windows 消息监听窗口");

            // 将 tx 的指针存入窗口自定义数据区（修复 isize 到 *mut c_void 的类型转换）
            let tx_boxed = Box::new(tx_clone);
            windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                hwnd_msg_sink,
                GWLP_USERDATA,
                Box::into_raw(tx_boxed) as isize, // 保持存入为整数
            );

            // 4. 注册 Raw Input 监听设备
            let devices = [RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x06,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd_msg_sink,
            }];

            if RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32).is_err() {
                eprintln!("Windows Raw Input 注册失败！");
                return;
            }

            // 5. 完美的 GetMessageW 安全循环 (适配 Some(hwnd_msg_sink))
            let mut msg = std::mem::zeroed();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                // 第二个参数改成 None 监听全队列消息
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            // 6. 线程退出时清理资源
            let tx_ptr = windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                hwnd_msg_sink,
                GWLP_USERDATA,
                0,
            ) as *mut channel::Sender<MyKeyEvent>;
            if !tx_ptr.is_null() {
                let _ = Box::from_raw(tx_ptr);
            }
            let _ = DestroyWindow(hwnd_msg_sink);
        }
    });
}