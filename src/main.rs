// 告诉 Windows 链接器这是一个 GUI 应用，不显示控制台窗口
#![cfg_attr(windows, windows_subsystem = "windows")]

mod state;
mod gui {
    pub mod settings_window;
}
mod events;
mod physics;
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
use std::thread;
use tracing;
//use tracing_subscriber::fmt; // 或者 use tracing_subscriber;
use crate::ri_table::win_vkey_to_rdev_str;
use state::AppState;


slint::include_modules!();

fn default_top_boundary() -> i32 {
    0
}
#[derive(Clone, Debug)]
struct BarNote {
    rdev_key_name: String,
    x: i32,
    width: i32,
    y: i32,
    height: i32,
    color: String,
    is_growing: bool,
}

#[derive(Debug, Clone)]
enum MyKeyEvent {
    Press { rdev_name: String },
    Release { rdev_name: String },
}


#[derive(Serialize, Deserialize, Clone, Debug)]
struct KeyConfig {
    rdev_key_name: String,
    display_name: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color_pressed: String,
}

fn default_border_width() -> i32 {
    1
}
fn default_border_color() -> String {
    "#555555".into()
}
fn default_margin_width() -> i32 {
    10
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct AppConfig {
    #[serde(default = "default_top_boundary")]
    top_boundary: i32,

    #[serde(default = "default_border_width")]
    global_border_width: i32,
    #[serde(default = "default_border_color")]
    global_border_color: String,
    #[serde(default = "default_margin_width")]
    key_margin_width: i32,

    keys: Vec<KeyConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            top_boundary: default_top_boundary(),
            global_border_width: default_border_width(),
            global_border_color: default_border_color(),
            key_margin_width: default_margin_width(),
            keys: vec![KeyConfig {
                rdev_key_name: "KeyA".into(),
                display_name: "A".into(),
                x: 10,
                y: 10, // 物理 Y：按键顶部距窗口底部 10px
                width: 80,
                height: 80,
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

/// 默认按键颜色（当配置文件中颜色解析失败时使用）
const DEFAULT_KEY_COLOR: u32 = 0x4A90E2;

/// 将十六进制颜色字符串（含或不含 # 前缀）解析为 slint::Color
fn hex_str_to_color(hex_str: &str) -> slint::Color {
    let hex_str = hex_str.trim_start_matches('#');
    let rgb = u32::from_str_radix(hex_str, 16).unwrap_or(DEFAULT_KEY_COLOR);
    slint::Color::from_argb_encoded(rgb | 0xFF000000)
}

/// 统一转换 trait：将不同类型转换为 KeyData
trait ToKeyData {
    fn to_key_data(&self) -> KeyData;
}

impl ToKeyData for BarNote {
    fn to_key_data(&self) -> KeyData {
        KeyData {
            name: self.rdev_key_name.clone().into(),
            display_name: "".into(),
            is_pressed: false,
            x: self.x as f32,
            y: self.y as f32,
            w: self.width as f32,
            h: self.height as f32,
            pressed_color: hex_str_to_color(&self.color),
            color_hex: self.color.clone().into(),
            selected: false,
        }
    }
}

impl ToKeyData for KeyConfig {
    fn to_key_data(&self) -> KeyData {
        KeyData {
            name: self.rdev_key_name.clone().into(),
            display_name: self.display_name.clone().into(),
            is_pressed: false,
            x: self.x as f32,
            y: self.y as f32,
            w: self.width as f32,
            h: self.height as f32,
            pressed_color: hex_str_to_color(&self.color_pressed),
            color_hex: self.color_pressed.clone().into(),
            selected: false,
        }
    }
}

/// 通用渲染函数：将任意实现了 `ToKeyData` 的切片转换为 `ModelRc<KeyData>`
fn create_model<T: ToKeyData>(items: &[T]) -> ModelRc<KeyData> {
    let data: Vec<KeyData> = items.iter().map(|i| i.to_key_data()).collect();
    Rc::new(VecModel::from(data)).into()
}

/// 线程安全的 HWND 包装（HWND 本身是 `*mut c_void`，不自动实现 Send）
#[derive(Clone, Copy)]
struct SafeHWND(windows::Win32::Foundation::HWND);
unsafe impl Send for SafeHWND {}
unsafe impl Sync for SafeHWND {}

/// 存储主窗口 HWND，供拖动回调使用
static MAIN_HWND: std::sync::Mutex<Option<SafeHWND>> = std::sync::Mutex::new(None);

#[cfg(windows)]
fn make_window_clickthrough(window: &winit::window::Window) {
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongW, HWND_TOPMOST, LWA_COLORKEY, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SetLayeredWindowAttributes, SetWindowLongW, SetWindowPos,
        WS_EX_LAYERED,
    };

    let hwnd = match window.window_handle().unwrap().as_raw() {
        RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut c_void),
        _ => return,
    };

    unsafe {
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);

        // 1. 添加分层样式（LWA_COLORKEY 需要 WS_EX_LAYERED）
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);

        // 2. 激活颜色键穿透（黑色像素 → 透明 → 点击穿透到下层窗口）
        if SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY).is_err() {
            tracing::error!("SetLayeredWindowAttributes 失败");
        }

        // 3. ⚠️ 关键：SetWindowPos 刷新窗口非客户区，使 WS_EX_LAYERED 生效
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }

    tracing::debug!("[clickthrough] 主窗口 HWND 已存储");
    *MAIN_HWND.lock().unwrap() = Some(SafeHWND(hwnd));
}

fn calculate_window_size(config: &AppConfig) -> (i32, i32) {
    // 宽度：按键中最右边的位置 + 边距（无上限限制）
    let width = if config.keys.is_empty() {
        1200
    } else {
        let max_right = config.keys.iter().map(|k| k.x + k.width).max().unwrap_or(0);
        max_right + config.key_margin_width
    };

    // 高度 = 最低按键底部物理 Y + top_boundary（顶部留白区域）
    // key.y = 按键顶部物理 Y（从底部向上），按键底部 = key.y + key.h
    let height = if config.keys.is_empty() {
        500
    } else {
        let max_bottom = config
            .keys
            .iter()
            .map(|k| k.y + k.height)
            .max()
            .unwrap_or(0);
        max_bottom + config.top_boundary
    };

    (width, height)
}

/// 创建设置窗口并绑定回调
fn create_settings_window(
    state: &AppState,
    main_ui_weak: &slint::Weak<MainWindow>,
) -> Option<SettingsWindow> {
    let settings = SettingsWindow::new().ok()?;
    settings.show().unwrap();
    gui::settings_window::setup_settings_window(settings, state.clone(), main_ui_weak.clone());
    // setup_settings_window 会存储弱引用，窗口由 Slint 事件循环保持存活
    // 我们不持有 Rust 端的强引用，window 关闭后自动清理
    None
}

/// ====================主函数====================
fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    tracing::debug!("[DEBUG] 程序启动，正在初始化...");

    let (tx, rx) = channel::unbounded::<MyKeyEvent>();
    let state = AppState::new(load_config());

    let ui = match MainWindow::new() {
        Ok(window) => window,
        Err(e) => {
            eprintln!("无法创建主窗口: {}", e);
            std::process::exit(1);
        }
    };

    // 1. 初始化 UI 全局表现属性
    {
        let cfg = match state.config.lock() {
            Ok(guard) => guard,
            Err(e) => {
                eprintln!("致命错误：获取配置锁失败: {}", e);
                std::process::exit(1); 
            }
        };
        let (width, height) = calculate_window_size(&cfg);
        //转换单位
        ui.set_window_width_px(width);
        ui.set_window_height_px(height);
        ui.set_global_border_width(cfg.global_border_width);
        ui.set_global_border_color(hex_str_to_color(&cfg.global_border_color));
        ui.set_key_margin_width(cfg.key_margin_width);
        ui.set_top_boundary_px(cfg.top_boundary);
        ui.set_keys(create_model(&cfg.keys));
        // 计算按键区域高度：最大物理 Y 范围 + 底部边距
        let max_bottom = cfg.keys.iter().map(|k| k.y + k.height).max().unwrap_or(0);
        let key_area_h = if max_bottom > 0 {
            max_bottom + cfg.key_margin_width
        } else {
            100
        };
        ui.set_key_area_height(key_area_h);
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
        let guard = MAIN_HWND.lock().unwrap();
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
    ui.on_request_close(|| slint::quit_event_loop().unwrap());

    // 4. 开启后台高性能 Timer 渲染时钟驱动
    let _event_timer = events::start_event_timer(rx, state.clone(), ui.as_weak());

    // 5. 显示窗口
    ui.show().unwrap();

    // 6. ⚠️ 必须使用 spawn_local + async 在事件循环内部获取 winit 窗口
    //    同步的 with_winit_window 在 show() 后可能因窗口未就绪而静默跳过
    {
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

                    winit_window.set_outer_position(winit::dpi::Position::Logical(
                        winit::dpi::LogicalPosition::new(100.0, 100.0),
                    ));

                    tracing::debug!("[setup] 窗口穿透属性已激活");
                }
                Err(e) => {
                    tracing::error!("[setup] 获取 winit 窗口失败: {:?}", e);
                }
            }
        })
        .unwrap();
    }

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
