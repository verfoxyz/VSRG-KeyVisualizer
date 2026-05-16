use crossbeam_channel as channel;
use i_slint_backend_winit::WinitWindowAccessor;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

slint::include_modules!();

// 1. 统一两端的事件流格式：定义跨平台的自定义输入事件
#[derive(Debug, Clone)]
enum MyKeyEvent {
    Press { rdev_name: String },
    Release { rdev_name: String },
}

// 吸附步长定义
// 辅助函数：提供反序列化默认值
fn default_grid_size() -> i32 {
    4
}

// 吸附步长函数（动态接收 grid_size 参数）
fn apply_snapping(value: i32, grid_size: i32) -> i32 {
    if grid_size <= 0 {
        return value;
    } // 防御性编程
    ((value as f32 / grid_size as f32).round() as i32) * grid_size
}

#[derive(Serialize, Deserialize, Clone)]
struct KeyConfig {
    rdev_key_name: String,
    display_name: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color_pressed: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct AppConfig {
    // 全局步进属性，默认值为 4
    #[serde(default = "default_grid_size")]
    grid_size: i32,
    keys: Vec<KeyConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            grid_size: default_grid_size(),
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

fn render_key_models(config: &AppConfig) -> slint::ModelRc<KeyData> {
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
        })
        .collect();

    slint::ModelRc::from(std::rc::Rc::new(slint::VecModel::from(key_models)))
}

// Windows 专有的不激活和透明穿透函数
fn make_window_no_activate(window: &winit::window::Window) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongW, SetWindowLongW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
            WS_EX_TRANSPARENT,
        };

        let hwnd = match window.window_handle().unwrap().as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut c_void),
            _ => return,
        };

        unsafe {
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(
                hwnd,
                GWL_EXSTYLE,
                ex_style
                    | WS_EX_NOACTIVATE.0 as i32
                    | WS_EX_TRANSPARENT.0 as i32
                    | WS_EX_LAYERED.0 as i32,
            );
        }
    }
}

fn main() {
    // 创建通信管道，数据统一为自定义的 MyKeyEvent
    let (tx, rx) = channel::unbounded::<MyKeyEvent>();

    let config = Arc::new(Mutex::new(load_config()));
    let ui = MainWindow::new().unwrap();

    let capture_mode = Arc::new(Mutex::new(false));
    let dialog_holder = Arc::new(Mutex::new(None));
    let settings_holder = Arc::new(Mutex::new(None));

    let key_map: Arc<HashMap<String, usize>> = {
        let config_inner = config.lock().unwrap();
        let mut map = HashMap::new();
        for (idx, k) in config_inner.keys.iter().enumerate() {
            map.insert(k.rdev_key_name.clone(), idx);
        }
        Arc::new(map)
    };

    let model = render_key_models(&config.lock().unwrap());
    ui.set_keys(model.clone());

    ui.window().with_winit_window(|window| {
        window.set_transparent(true);
        window.set_decorations(false);
        window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
        make_window_no_activate(window);
    });

    let ui_weak = ui.as_weak();
    ui.on_gui_drag_window(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let _ = ui.window().with_winit_window(|w| w.drag_window());
        }
    });

    let capture_mode_for_settings = capture_mode.clone();
    let config_for_settings = config.clone();
    let ui_weak_for_settings = ui.as_weak();
    let dialog_holder_for_settings = dialog_holder.clone();
    let settings_holder_for_settings = settings_holder.clone();

    ui.on_request_settings(move || {
        let settings = SettingsWindow::new().unwrap();
        // 启动设置窗口时，从内存锁中读取 grid_size 并注入 UI
        let current_grid_size = config_for_settings.lock().unwrap().grid_size;
        settings.set_global_grid_size(current_grid_size);
        if let Some(main_ui) = ui_weak_for_settings.clone().upgrade() {
            settings.set_root_preview_keys(main_ui.get_keys());
        }

        *settings_holder_for_settings.lock().unwrap() = Some(settings.as_weak());

        let settings_weak = settings.as_weak();
        let config_for_update = config_for_settings.clone();
        let ui_weak_for_update = ui_weak_for_settings.clone();
        settings.on_update_key_config(move |index, x, y, w, h, color| {
            let idx = index as usize; // 转换 i32 为 usize 以便索引

            // 获取当前的步长
            let mut config_inner = config_for_update.lock().unwrap();
            let g_size = config_inner.grid_size;

            // 应用动态的全局步长
            let snapped_x = apply_snapping(x, g_size);
            let snapped_y = apply_snapping(y, g_size);

            // 1. 更新内存中的 config (注意类型转换)
            if idx < config_inner.keys.len() {
                config_inner.keys[idx].x = snapped_x as f32;
                config_inner.keys[idx].y = snapped_y as f32;
                config_inner.keys[idx].width = w as f32;
                config_inner.keys[idx].height = h as f32;
                config_inner.keys[idx].color_pressed = color.to_string();
            }

            if let Some(main_ui) = ui_weak_for_update.upgrade() {
                let model = main_ui.get_keys();
                if let Some(mut data) = model.row_data(idx) {
                    data.x = snapped_x as f32;
                    data.y = snapped_y as f32;
                    data.w = w as f32;
                    data.h = h as f32;

                    let hex_str = color.as_str().trim_start_matches('#');
                    if let Ok(rgb) = u32::from_str_radix(hex_str, 16) {
                        data.pressed_color = slint::Color::from_argb_encoded(rgb | 0xFF000000);
                    }
                    model.set_row_data(idx, data);
                }
            }
        });

        // 👈 新增：处理配置窗口发送的"删除按键"请求
        let config_for_delete = config_for_settings.clone();
        let ui_weak_for_delete = ui_weak_for_settings.clone();
        let settings_weak_for_delete = settings.as_weak();

        settings.on_delete_key(move |index| {
            let idx = index as usize;
            let mut config_inner = config_for_delete.lock().unwrap();

            if idx < config_inner.keys.len() {
                // 1. 从全局内存向量中移除数据
                config_inner.keys.remove(idx);
                println!("🗑️ 内存中的第 {} 号按键已被移除，点击保存后生效。", idx);

                // 2. 实时渲染全新的 Model 结构体
                let new_model = render_key_models(&config_inner);

                // 3. 刷新 MainWindow（大键盘皮肤层）
                if let Some(main_ui) = ui_weak_for_delete.upgrade() {
                    main_ui.set_keys(new_model.clone());
                }

                // 4. 刷新当前 SettingsWindow 的预览方块层
                if let Some(s) = settings_weak_for_delete.upgrade() {
                    s.set_root_preview_keys(new_model);
                }
            }
        });
        // 保存配置回调（只保存，不关闭窗口）
        let config_for_save = config_for_settings.clone();
        let settings_weak_for_save = settings.as_weak();
        settings.on_save_config_clicked(move || {
            if let Some(s) = settings_weak_for_save.upgrade() {
                let idx = s.get_selected_index() as usize;
                let mut config_inner = config_for_save.lock().unwrap();
                let g_size = config_inner.grid_size;

                if idx != -1 as isize as usize && idx < config_inner.keys.len() {
                    config_inner.keys[idx].x = apply_snapping(s.get_current_x(), g_size) as f32;
                    config_inner.keys[idx].y = apply_snapping(s.get_current_y(), g_size) as f32;
                    config_inner.keys[idx].width = s.get_current_w() as f32;
                    config_inner.keys[idx].height = s.get_current_h() as f32;
                    config_inner.keys[idx].color_pressed = s.get_current_color().to_string();
                }

                // 统一持久化（AppConfig 结构体会自动带上最新的 grid_size 字段）
                save_config(&config_inner);
                println!(
                    "🎉 全局配置与全新步长 [{}] 已成功保存到 config.json！",
                    g_size
                );
            }
        });

        // 新增：当用户在 UI 调整步进数字时，同步到 Rust 内存
        let config_for_grid = config_for_settings.clone();
        settings.on_grid_size_edited(move |new_size| {
            let mut config_inner = config_for_grid.lock().unwrap();
            config_inner.grid_size = new_size;
            println!(
                "⚙️ 全局步进已更改为: {} 像素（点击保存后写入文件）",
                new_size
            );
        });

        let capture_mode_for_add_key = capture_mode_for_settings.clone();
        let dialog_holder_clone = dialog_holder_for_settings.clone();
        let settings_weak_for_add_key = settings.as_weak();

        settings.on_add_new_key(move || {
            let mut mode = capture_mode_for_add_key.lock().unwrap();
            *mode = true;

            if let Some(s) = settings_weak_for_add_key.upgrade() {
                s.set_capturing_mode(true);
            }

            let capture_dialog = KeyCaptureDialog::new().unwrap();
            *dialog_holder_clone.lock().unwrap() = Some(capture_dialog.as_weak());

            capture_dialog.window().with_winit_window(|window| {
                make_window_no_activate(window);
            });
            capture_dialog.show().unwrap();
        });

        let settings_holder_for_close = settings_holder_for_settings.clone();
        settings.on_close_clicked(move || {
            if let Some(s) = settings_weak.upgrade() {
                s.hide().unwrap();
            }
            *settings_holder_for_close.lock().unwrap() = None;
        });

        settings.show().unwrap();
    });

    ui.on_request_close(|| slint::quit_event_loop().unwrap());

    // 2. 启动监听器 (根据平台不同，内部实现完全不同)
    init_platform_input_listener(tx, &ui);

    // 3. UI 线程的事件分发器
    let ui_weak = ui.as_weak();
    let capture_mode_clone = capture_mode.clone();
    let dialog_holder_clone = dialog_holder.clone();
    let settings_holder_clone = settings_holder.clone();
    let key_map_clone = key_map.clone();
    let config_for_timer = config.clone();

    let event_timer = slint::Timer::default();
    event_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(10),
        move || {
            while let Ok(event) = rx.try_recv() {
                handle_backend_input(
                    event,
                    &ui_weak,
                    &capture_mode_clone,
                    &dialog_holder_clone,
                    &settings_holder_clone,
                    &key_map_clone,
                    &config_for_timer,
                );
            }
        },
    );

    ui.run().unwrap();
}

//========================= 统一的消息处理器 =========================

fn handle_backend_input(
    event: MyKeyEvent,
    ui_weak: &slint::Weak<MainWindow>,
    capture_mode: &Arc<Mutex<bool>>,
    dialog_handle: &Arc<Mutex<Option<slint::Weak<KeyCaptureDialog>>>>,
    settings_handle: &Arc<Mutex<Option<slint::Weak<SettingsWindow>>>>,
    key_map: &HashMap<String, usize>,
    config: &Arc<Mutex<AppConfig>>,
) {
    if let Ok(mut is_capturing) = capture_mode.try_lock() {
        match event {
            MyKeyEvent::Press { rdev_name } => {
                if *is_capturing {
                    *is_capturing = false;
                    process_key_capture(rdev_name, ui_weak, dialog_handle, settings_handle, config);
                } else {
                    update_key_visual_state(ui_weak, &rdev_name, true, key_map);
                }
            }
            MyKeyEvent::Release { rdev_name } => {
                if !*is_capturing {
                    update_key_visual_state(ui_weak, &rdev_name, false, key_map);
                }
            }
        }
    }
}

fn process_key_capture(
    rdev_name: String,
    ui_weak: &slint::Weak<MainWindow>,
    dialog_handle: &Arc<Mutex<Option<slint::Weak<KeyCaptureDialog>>>>,
    settings_handle: &Arc<Mutex<Option<slint::Weak<SettingsWindow>>>>,
    config: &Arc<Mutex<AppConfig>>,
) {
    if let Some(weak_dialog) = dialog_handle.lock().unwrap().take() {
        if let Some(dialog) = weak_dialog.upgrade() {
            let _ = dialog.hide();
        }
    }

    if let Some(weak_settings) = settings_handle.lock().unwrap().as_ref() {
        if let Some(settings) = weak_settings.upgrade() {
            settings.set_capturing_mode(false);
        }
    }

    if rdev_name == "Escape" {
        return;
    }

    let display_name = rdev_name.replace("Key", "").to_uppercase();

    let new_key = KeyConfig {
        rdev_key_name: rdev_name.into(),
        display_name: display_name.into(),
        x: 10.0,
        y: 10.0,
        width: 80.0,
        height: 80.0,
        color_pressed: "#4A90E2".into(),
    };

    // 核心修改：不再通过 load_config/save_config 读写磁盘，直接加到全局内存锁中！
    let model = {
        let mut config_inner = config.lock().unwrap();
        config_inner.keys.push(new_key);
        render_key_models(&config_inner) // 实时根据最新内存渲染 model
    };

    if let Some(ui) = ui_weak.upgrade() {
        ui.set_keys(model.clone());

        if let Some(weak_settings) = settings_handle.lock().unwrap().as_ref() {
            if let Some(settings) = weak_settings.upgrade() {
                settings.set_root_preview_keys(model);
            }
        }
        println!("🆕 新按键已暂存至内存，点击\"Save Config\"后才会写入磁盘。");
    }
}

fn update_key_visual_state(
    ui_weak: &slint::Weak<MainWindow>,
    target_key_str: &str,
    is_press: bool,
    key_map: &HashMap<String, usize>,
) {
    if let Some(&index) = key_map.get(target_key_str) {
        if let Some(ui) = ui_weak.upgrade() {
            let model = ui.get_keys();
            if let Some(mut data) = model.row_data(index) {
                data.is_pressed = is_press;
                model.set_row_data(index, data);
            }
        }
    }
}

//========================= 核心分平台条件编译区 =========================

/// [Unix 平台实现]：只编译 rdev 全局钩子监听
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

//========================= 核心分平台条件编译区 =========================

/// [Unix 平台实现]：只编译 rdev 全局钩子监听
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

/// [Windows 平台实现]：修正类型不匹配与安全规范后的 Raw Input 全局后台监听
#[cfg(windows)]
fn init_platform_input_listener(tx: channel::Sender<MyKeyEvent>, _ui: &MainWindow) {
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::{
        GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
        RIDEV_INPUTSINK, RIM_TYPEKEYBOARD, RegisterRawInputDevices,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
        HWND_MESSAGE, RegisterClassExW, TranslateMessage, WM_INPUT, WNDCLASSEXW,
    };
    use windows::core::w;

    thread::spawn(move || {
        unsafe {
            // 1. 全局静态变量存储 Sender，供窗口回调过程使用
            static mut GLOBAL_TX: Option<channel::Sender<MyKeyEvent>> = None;
            GLOBAL_TX = Some(tx);

            // 2. 定义后台纯消息窗口的窗口过程 (WndProc)
            unsafe extern "system" fn raw_input_wnd_proc(
                hwnd: HWND,
                msg: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                if msg == WM_INPUT {
                    let mut size: u32 = 0;
                    let h_raw = HRAWINPUT(lparam.0 as *mut std::ffi::c_void);

                    // 显式包裹在 unsafe 块中以符合新版规范
                    unsafe {
                        // 第一次获取：拿到缓冲区所需大小
                        let _ = GetRawInputData(
                            h_raw,
                            RID_INPUT,
                            None,
                            &mut size,
                            size_of::<RAWINPUTHEADER>() as u32,
                        );

                        if size > 0 {
                            let mut buffer = vec![0u8; size as usize];
                            let fetch_res = GetRawInputData(
                                h_raw,
                                RID_INPUT,
                                Some(buffer.as_mut_ptr() as *mut std::ffi::c_void),
                                &mut size,
                                size_of::<RAWINPUTHEADER>() as u32,
                            );

                            if fetch_res != u32::MAX {
                                let raw = &*(buffer.as_ptr() as *const RAWINPUT);

                                if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
                                    let k = raw.data.keyboard;
                                    let rdev_name = win_vkey_to_rdev_string(k.VKey);
                                    let is_break = (k.Flags as u32 & 0x0001) != 0;

                                    if let Some(tx) = (*std::ptr::addr_of!(GLOBAL_TX)).as_ref() {
                                        let event = if is_break {
                                            MyKeyEvent::Release { rdev_name }
                                        } else {
                                            MyKeyEvent::Press { rdev_name }
                                        };
                                        let _ = tx.try_send(event);
                                    }
                                }
                            }
                        }
                    }
                }
                // 显式包裹 unsafe
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }

            // 3. 注册一个专门的后台窗口类
            let class_name = w!("RawInputMsgOnlyWindowClass");
            let wnd_class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(raw_input_wnd_proc),
                hInstance: HINSTANCE(null_mut()),
                lpszClassName: class_name,
                ..zeroed()
            };

            if RegisterClassExW(&wnd_class) == 0 {
                eprintln!("Windows 后台消息窗口类注册失败！");
                return;
            }

            // 4. 创建纯消息窗口 (关键修改：HWND_MESSAGE 和 HINSTANCE 包装为 Some)
            let hwnd_msg_sink = CreateWindowExW(
                Default::default(),
                class_name,
                w!("RawInputSink"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE), // 修复：从 HWND 变为 Option<HWND>
                None,
                Some(HINSTANCE(null_mut())), // 修复：从 HINSTANCE 变为 Option<HINSTANCE>
                None,
            )
            .expect("Windows 纯消息窗口创建失败！");

            // 5. 将 Raw Input 注册到这个独立的后台窗口上
            let mut devices: [RAWINPUTDEVICE; 1] = zeroed();
            devices[0].usUsagePage = 1;
            devices[0].usUsage = 6; // 键盘
            devices[0].dwFlags = RIDEV_INPUTSINK;
            devices[0].hwndTarget = hwnd_msg_sink; // 这里的包装通过 CreateWindowExW 隐式解出，无需修改

            if RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32).is_err() {
                eprintln!("Windows Raw Input 注册失败！");
                return;
            }

            println!("Windows Raw Input 纯消息后台监听已启动...");

            // 6. 独立线程标准的 Windows 消息泵
            let mut msg = std::mem::zeroed();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                // 修复：此处 HWND 过滤改传入 None 监听线程全消息
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

/// 辅助函数：将 Windows 虚拟键码转换为你当前所用的 rdev 命名规则字符串
/// 从而保证你同一套 config.json 既能在 Unix 上跑，也能在 Windows 上跑
#[cfg(windows)]
fn win_vkey_to_rdev_string(vkey: u16) -> String {
    match vkey {
        0x41..=0x5A => format!("Key{}", (vkey as u8 as char)), // A-Z
        0x30..=0x39 => format!("Key{}", (vkey as u8 as char)), // 0-9 (建议按上文的优化改)

        // --- 控制键 ---
        0x1B => "Escape".into(),
        0x20 => "Space".into(),
        0x0D => "Return".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x25 => "LeftArrow".into(),
        0x26 => "UpArrow".into(),
        0x27 => "RightArrow".into(),
        0x28 => "DownArrow".into(),

        // --- 符号键 (美式键盘标准) ---
        0xBA => "SemiColon".into(),    // ; :
        0xDE => "Quote".into(),        // ' "
        0xBB => "Equal".into(),        // = +
        0xBD => "Minus".into(),        // - _
        0xDC => "BackSlash".into(),    // \ |
        0xDB => "LeftBracket".into(),  // [ {
        0xDD => "RightBracket".into(), // ] }
        0xC0 => "BackQuote".into(),    // ` ~
        0xBF => "Slash".into(),        // / ?

        // --- 小键盘符号 ---
        0x6A => "KpMultiply".into(), // *
        0x6B => "KpAdd".into(),      // +
        0x6D => "KpSubtract".into(), // -
        0x6F => "KpDivide".into(),   // /

        _ => format!("Unknown(0x{:X})", vkey),
    }
}
