mod state;
mod gui {
    pub mod settings_window;
}
mod events;
mod ri_table;
// ====================模块定义====================
use crossbeam_channel as channel;
use i_slint_backend_winit::WinitWindowAccessor;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
//use std::collections::HashMap;
use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use tracing;
//use tracing_subscriber::fmt; // 或者 use tracing_subscriber;
use crate::ri_table::win_vkey_to_rdev_str;
use state::AppState;

slint::include_modules!();

fn default_top_boundary() -> i32 {
    0
}
fn default_grid_size() -> i32 {
    4
}

#[derive(Clone, Debug)]
struct BarNote {
    rdev_key_name: String,
    x: f32,
    width: f32,
    y: f32,
    height: f32,
    color: String,
    is_growing: bool,
}

#[derive(Debug, Clone)]
enum MyKeyEvent {
    Press { rdev_name: String },
    Release { rdev_name: String },
}

fn apply_snapping(value: i32, grid_size: i32) -> i32 {
    if grid_size <= 0 {
        return value;
    }
    ((value as f32 / grid_size as f32).round() as i32) * grid_size
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct KeyConfig {
    rdev_key_name: String,
    display_name: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color_pressed: String,
}

fn default_border_width() -> i32 {
    1
}
fn default_border_color() -> String {
    "#555555".into()
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct AppConfig {
    #[serde(default = "default_grid_size")]
    grid_size: i32,
    #[serde(default = "default_top_boundary")]
    top_boundary: i32,

    #[serde(default = "default_border_width")]
    global_border_width: i32,
    #[serde(default = "default_border_color")]
    global_border_color: String,

    keys: Vec<KeyConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            top_boundary: default_top_boundary(),
            grid_size: default_grid_size(),
            global_border_width: default_border_width(),
            global_border_color: default_border_color(),
            keys: vec![KeyConfig {
                rdev_key_name: "KeyA".into(),
                display_name: "A".into(),
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 80.0,
                color_pressed: "#4A90E2".into(),
            }],
        }
    }
}

fn load_config() -> AppConfig {
    let path = Path::new("config.json");
    if path.exists() {
        let content = fs::read_to_string(path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_else(|_| AppConfig::default())
    } else {
        let config = AppConfig::default();
        save_config(&config);
        config
    }
}

fn save_config(config: &AppConfig) {
    let content = serde_json::to_string_pretty(config).unwrap();
    fs::write("config.json", content).expect("无法写入配置文件");
}

fn hex_str_to_color(hex_str: &str) -> slint::Color {
    let hex_str = hex_str.trim_start_matches('#');
    u32::from_str_radix(hex_str, 16)
        .map(|rgb| slint::Color::from_argb_encoded(rgb | 0xFF000000))
        .unwrap_or(slint::Color::from_argb_u8(255, 85, 85, 85))
}

fn render_bar_models(notes: &[BarNote]) -> ModelRc<KeyData> {
    let mut bar_data_list = Vec::new();
    for note in notes {
        let hex_str = note.color.trim_start_matches('#');
        let parsed_color = u32::from_str_radix(hex_str, 16)
            .map(|rgb| slint::Color::from_argb_encoded(rgb | 0xFF000000))
            .unwrap_or(slint::Color::from_argb_u8(255, 255, 255, 255));

        bar_data_list.push(KeyData {
            name: note.rdev_key_name.clone().into(),
            display_name: "".into(),
            is_pressed: false,
            x: note.x,
            y: note.y,
            w: note.width,
            h: note.height,
            pressed_color: parsed_color,
            color_hex: note.color.clone().into(),
            selected: false,
        });
    }
    Rc::new(VecModel::from(bar_data_list)).into()
}

fn render_key_models(config: &AppConfig) -> slint::ModelRc<KeyData> {
    tracing::debug!(
        "[DEBUG] render_key_models: 开始渲染，按键数量: {}",
        config.keys.len()
    );
    let key_models: Vec<KeyData> = config
        .keys
        .iter()
        .map(|k| KeyData {
            name: k.rdev_key_name.clone().into(),
            display_name: k.display_name.clone().into(),
            is_pressed: false,
            x: k.x,
            y: k.y,
            w: k.width,
            h: k.height,
            color_hex: k.color_pressed.clone().into(),
            pressed_color: slint::Color::from_argb_encoded(
                u32::from_str_radix(k.color_pressed.trim_start_matches('#'), 16)
                    .unwrap_or(0x4A90E2)
                    | 0xFF000000,
            ),
            selected: false,
        })
        .collect();

    slint::ModelRc::from(std::rc::Rc::new(slint::VecModel::from(key_models)))
}

#[cfg(windows)]
fn make_window_no_activate(window: &winit::window::Window) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongW, SetWindowLongW, WS_EX_NOACTIVATE,
    };

    let hwnd = match window.window_handle().unwrap().as_raw() {
        RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut c_void),
        _ => return,
    };

    unsafe {
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        // 结合 backup 里的透明穿透属性，保证 overlay 悬浮窗表现正常，同时避免 DWM 绘制死锁
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_NOACTIVATE.0 as i32);
    }
}

/// ====================主函数====================
fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    tracing::debug!("[DEBUG] 程序启动，正在初始化...");

    let (tx, rx) = channel::unbounded::<MyKeyEvent>();
    let state = AppState::new(load_config());

    let ui = MainWindow::new().unwrap();

    // 1. 初始化 UI 全局表现属性
    {
        let cfg = state.config.lock().unwrap();
        ui.set_global_border_width(cfg.global_border_width);
        ui.set_global_border_color(hex_str_to_color(&cfg.global_border_color));
        ui.set_top_boundary_px(cfg.top_boundary);
        ui.set_keys(render_key_models(&cfg));
    }

    // 2. 绑定主窗体基础拖拽与关闭事件
    let ui_weak = ui.as_weak();
    ui.on_gui_drag_window({
        let ui_weak = ui_weak.clone();
        move || {
            if let Some(ui) = ui_weak.upgrade() {
                let _ = ui.window().with_winit_window(|w| w.drag_window());
            }
        }
    });
    ui.on_request_close(|| slint::quit_event_loop().unwrap());

    // 3. 注册唤起设置界面的路由
    let ui_weak_for_route = ui.as_weak();
    let state_for_route = state.clone();
    ui.on_request_settings(move || {
        let settings = SettingsWindow::new().unwrap();
        gui::settings_window::setup_settings_window(
            settings,
            state_for_route.clone(),
            ui_weak_for_route.clone(),
        );
    });

    // 4. 开启后台高性能 Timer 渲染时钟驱动
    let _event_timer = events::start_event_timer(rx, state.clone(), ui.as_weak());

    // 5. 优雅启动 Winit 底层置顶及无边框窗口
    ui.show().unwrap();
    /*
       let (keys_count, top_boundary_val) = {
           let cfg = state.config.lock().unwrap();
           (cfg.keys.len(), cfg.top_boundary)
       };
    */
    ui.window().with_winit_window(move |window| {
        window.set_transparent(true);
        window.set_decorations(false);
        window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
        #[cfg(windows)]
        make_window_no_activate(window);
        window.set_outer_position(winit::dpi::Position::Logical(
            winit::dpi::LogicalPosition::new(100.0, 100.0),
        ));
    });

    init_platform_input_listener(tx, &ui);

    tracing::debug!("[DEBUG] ---> 启动 Slint 全局事件循环...");
    slint::run_event_loop().unwrap();
}

fn update_key_visual_state(ui_weak: &slint::Weak<MainWindow>, key_name: String, is_pressed: bool) {
    if let Some(ui) = ui_weak.upgrade() {
        let model = ui.get_keys();
        for idx in 0..model.row_count() {
            if let Some(mut data) = model.row_data(idx) {
                if data.name == key_name {
                    data.is_pressed = is_pressed;
                    model.set_row_data(idx, data);
                    break;
                }
            }
        }
    }
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
            let class_name: Vec<u16> = "KeyTick_Sink_Class\0".encode_utf16().collect();
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
