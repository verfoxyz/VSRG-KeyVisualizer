use i_slint_backend_winit::WinitWindowAccessor;
use rdev::{Event, EventType, Key, listen};
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use crossbeam_channel as channel;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW,
    SetWindowLongW,
    GWL_EXSTYLE,
    WS_DISABLED,
    WS_EX_NOACTIVATE,
    WS_EX_TRANSPARENT,
    WS_EX_LAYERED,
};
use std::ffi::c_void;
slint::include_modules!();

// 移除 BackendEvent 包装，直接使用 rdev::Event

// 吸附步长定义
const GRID_SIZE: i32 = 10;
fn apply_snapping(value: i32) -> i32 {
    // 四舍五入到最近的 GRID_SIZE 倍数
    ((value as f32 / GRID_SIZE as f32).round() as i32) * GRID_SIZE
}
// 定义按键配置结构体
#[derive(Serialize, Deserialize, Clone)]
struct KeyConfig {
    rdev_key_name: String,
    display_name: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color_pressed: String, // 存储如 "#FF0000"
}
// 定义应用配置结构体
#[derive(Serialize, Deserialize, Clone)]
struct AppConfig {
    keys: Vec<KeyConfig>,
}
// 实现默认配置
impl Default for AppConfig {
    fn default() -> Self {
        Self {
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
// 加载配置
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
// 保存配置
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
            // 如果 Slint 里是 float，这里直接赋值或转 f32；如果是 logical_length，保持 f32
            x: k.x as f32,
            y: k.y as f32,
            w: k.width as f32,
            h: k.height as f32,
            color_hex: k.color_pressed.clone().into(),
            pressed_color: slint::Color::from_argb_encoded(
                u32::from_str_radix(k.color_pressed.trim_start_matches('#'), 16)
                    .unwrap_or(0x4A90E2)
                    | 0xFF000000,
            ), // 这里不需要 .into()，因为字段本身就是 slint::Color
        })
        .collect(); // map 结束

    // 返回语句必须在 collect 之后，闭包之外
    slint::ModelRc::from(std::rc::Rc::new(slint::VecModel::from(key_models)))
}

fn make_window_no_activate(window: &winit::window::Window) {
    use windows::Win32::Foundation::HWND;

    let hwnd = match window.window_handle().unwrap().as_raw() {
        RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut c_void),
        _ => return,
    };

    unsafe {
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        // 添加不激活和透明穿透样式
        SetWindowLongW(
            hwnd,
            GWL_EXSTYLE,
            ex_style 
            | WS_EX_NOACTIVATE.0 as i32 
            | WS_EX_TRANSPARENT.0 as i32 
            | WS_EX_LAYERED.0 as i32 
            | WS_DISABLED.0 as i32,
        );
    }
}

fn main() {
    // 1. 创建通信管道（使用 crossbeam-channel 支持非阻塞发送）
    let (tx, rx) = channel::unbounded::<rdev::Event>();

    let config = Arc::new(Mutex::new(load_config()));
    let ui = MainWindow::new().unwrap();

    // 定义一个共享的捕获模式状态
    let capture_mode = Arc::new(Mutex::new(false));

    // 定义对话框句柄，让监听器可以关闭对话框
    let dialog_holder = Arc::new(Mutex::new(None));

    // 定义设置窗口句柄，用于更新预览列表
    let settings_holder = Arc::new(Mutex::new(None));

    // 初始化按键名称到索引的映射（用于快速查找）
    let key_map: Arc<HashMap<String, usize>> = {
        let config_inner = config.lock().unwrap();
        let mut map = HashMap::new();
        for (idx, k) in config_inner.keys.iter().enumerate() {
            map.insert(k.rdev_key_name.clone(), idx);
        }
        Arc::new(map)
    };

    // 初始化 UI 模型
    let model = render_key_models(&config.lock().unwrap());
    ui.set_keys(model.clone());
    // --- 窗口基础设置 ---
    ui.window().with_winit_window(|window| {
        window.set_transparent(true);
        window.set_decorations(false);
        window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
        make_window_no_activate(window);
    });

    // --- 回调处理 ---
    let ui_weak = ui.as_weak();
    ui.on_gui_drag_window(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let _ = ui.window().with_winit_window(|w| w.drag_window());
        }
    });

    // --- 进入设置窗口闭包前克隆 ---
    let capture_mode_for_settings = capture_mode.clone();
    let config_for_settings = config.clone();
    let ui_weak_for_settings = ui.as_weak();
    let dialog_holder_for_settings = dialog_holder.clone();
    let settings_holder_for_settings = settings_holder.clone(); // 新增：克隆 settings_holder

    // 双击打开设置窗口
    ui.on_request_settings(move || {
        let settings = SettingsWindow::new().unwrap();

        // 1. 初始化预览列表（将 config 同步到 settings 窗口）
        if let Some(main_ui) = ui_weak_for_settings.clone().upgrade() {
            // 获取主窗口的按键模型并设置给配置窗口
            settings.set_root_preview_keys(main_ui.get_keys());
        }

        // 2. 将设置窗口的弱引用存入全局，让监听器可以更新预览
        *settings_holder_for_settings.lock().unwrap() = Some(settings.as_weak());

        // 2. 监听属性变化
        let settings_weak = settings.as_weak();

        // --- 修复点 2：在设置窗口内部的回调闭包前再次克隆 ---
        let config_for_update = config_for_settings.clone();
        let ui_weak_for_update = ui_weak_for_settings.clone();
        settings.on_update_key_config(move |index, x, y, w, h, color| {
            let idx = index as usize; // 转换 i32 为 usize 以便索引

            let snapped_x = apply_snapping(x);
            let snapped_y = apply_snapping(y);

            // 1. 更新内存中的 config (注意类型转换)
            let mut config_inner = config_for_update.lock().unwrap();
            if idx < config_inner.keys.len() {
                config_inner.keys[idx].x = snapped_x as f32;
                config_inner.keys[idx].y = snapped_y as f32;
                config_inner.keys[idx].width = w as f32;
                config_inner.keys[idx].height = h as f32;
                config_inner.keys[idx].color_pressed = color.to_string();
            }

            // 2. 更新主 UI 的 Model
            if let Some(main_ui) = ui_weak_for_update.upgrade() {
                let model = main_ui.get_keys();
                if let Some(mut data) = model.row_data(idx) {
                    // 注意：如果 Slint 里定义的是 length，这里赋值必须也是 f32
                    data.x = snapped_x as f32;
                    data.y = snapped_y as f32;
                    data.w = w as f32;
                    data.h = h as f32;

                    // 颜色处理：color 是 SharedString，需要转成字符串再解析
                    let hex_str = color.as_str().trim_start_matches('#');
                    if let Ok(rgb) = u32::from_str_radix(hex_str, 16) {
                        data.pressed_color = slint::Color::from_argb_encoded(rgb | 0xFF000000);
                    }

                    model.set_row_data(idx, data);
                }
            }
        });

        // 保存配置回调（只保存，不关闭窗口）
        let config_for_save = config_for_settings.clone();
        settings.on_save_config_clicked(move || {
            save_config(&config_for_save.lock().unwrap());
        });

        // --- 在 on_add_new_key 闭包前克隆 ---
        let capture_mode_for_add_key = capture_mode_for_settings.clone();
        let dialog_holder_clone = dialog_holder_for_settings.clone();
        let settings_weak_for_add_key = settings.as_weak(); // 使用弱引用
        settings.on_add_new_key(move || {
            // 设置捕获模式为 true
            let mut mode = capture_mode_for_add_key.lock().unwrap();
            *mode = true;
            println!("进入捕获模式，请按下按键...");

            // 显式释放焦点：设置 capturing_mode 属性，让透明 TouchArea 接管焦点
            if let Some(s) = settings_weak_for_add_key.upgrade() {
                s.set_capturing_mode(true);
            }

            // 显示捕获提示对话框
            let capture_dialog = KeyCaptureDialog::new().unwrap();
            // 将对话框的弱引用存入全局，让 start_listener 负责关闭它
            *dialog_holder_clone.lock().unwrap() = Some(capture_dialog.as_weak());

            capture_dialog.window().with_winit_window(|window| {
                // 设置点击穿透，这样它就不会拦截任何鼠标/键盘输入聚焦
                make_window_no_activate(window);
                // 尝试让窗口不接受输入焦点
                // 注意：不同操作系统的表现可能不同
            });
            capture_dialog.show().unwrap();
        });

        let settings_holder_for_close = settings_holder_for_settings.clone();
        settings.on_close_clicked(move || {
            if let Some(s) = settings_weak.upgrade() {
                s.hide().unwrap();
            }
            // 清除设置窗口引用
            *settings_holder_for_close.lock().unwrap() = None;
        });

        settings.show().unwrap();
    });
    ui.on_request_close(|| slint::quit_event_loop().unwrap());

    // 2. 启动监听线程
    start_listener(tx);

    // 3. UI 线程的事件分发器 (每 10ms 检查一次消息，使用非阻塞锁)
    let ui_weak = ui.as_weak();
    let config_clone = config.clone();
    let capture_mode_clone = capture_mode.clone();
    let dialog_holder_clone = dialog_holder.clone();
    let settings_holder_clone = settings_holder.clone();
    let key_map_clone = key_map.clone();

    let event_timer = slint::Timer::default();
    event_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(10),
        move || {
            // 不锁定 config，只处理事件
            while let Ok(event) = rx.try_recv() {
                handle_backend_input(
                    event.event_type,
                    &ui_weak,
                    &capture_mode_clone,
                    &dialog_holder_clone,
                    &settings_holder_clone,
                    &key_map_clone,
                );
            }
        },
    );

    ui.run().unwrap();
}

//=========================监听按键事件=========================

/// 统一的按键监听函数（生产者）
/// 专门开一个线程给 rdev，不做任何其他杂事
fn start_listener(tx: crossbeam_channel::Sender<rdev::Event>) {
    thread::spawn(move || {
        // 在 Windows 上，rdev 内部会启动消息循环
        if let Err(e) = listen(move |event| {
            // 使用 try_send。如果 UI 真的卡死了，宁愿丢包也不要卡住系统键盘
            let _ = tx.try_send(event);
        }) {
            eprintln!("Hook Error: {:?}", e);
        }
    });
}

/// 处理按键捕获逻辑
fn process_key_capture(
    k: rdev::Key,
    ui_weak: &slint::Weak<MainWindow>,
    dialog_handle: &Arc<Mutex<Option<slint::Weak<KeyCaptureDialog>>>>,
    settings_handle: &Arc<Mutex<Option<slint::Weak<SettingsWindow>>>>,
) {
    let rdev_name = format!("{:?}", k);

    // 1. 关闭对话框
    if let Some(weak_dialog) = dialog_handle.lock().unwrap().take() {
        if let Some(dialog) = weak_dialog.upgrade() {
            let _ = dialog.hide();
        }
    }

    // 2. 退出捕获模式，释放焦点区域
    if let Some(weak_settings) = settings_handle.lock().unwrap().as_ref() {
        if let Some(settings) = weak_settings.upgrade() {
            settings.set_capturing_mode(false);
        }
    }

    // 3. 处理 ESC 退出逻辑（不添加按键）
    if k == rdev::Key::Escape {
        return;
    }

    // 4. 更新配置和主界面
    let display_name = rdev_name.replace("Key", "").to_uppercase();

    // 创建新按键配置
    let new_key = KeyConfig {
        rdev_key_name: rdev_name.into(),
        display_name: display_name.into(),
        x: 10.0,
        y: 10.0,
        width: 80.0,
        height: 80.0,
        color_pressed: "#4A90E2".into(),
    };

    // 更新配置
    let config = Arc::new(Mutex::new(load_config()));
    let mut config_inner = config.lock().unwrap();
    config_inner.keys.push(new_key);
    save_config(&config_inner);

    // 更新 UI（重新渲染整个模型）
    if let Some(ui) = ui_weak.upgrade() {
        let model = render_key_models(&config_inner);
        ui.set_keys(model.clone());

        // 同时更新设置窗口的预览列表
        if let Some(weak_settings) = settings_handle.lock().unwrap().as_ref() {
            if let Some(settings) = weak_settings.upgrade() {
                settings.set_root_preview_keys(model);
            }
        }
    }
}

/// 更新按键视觉状态（使用 HashMap 快速查找）
fn update_key_visual_state(
    ui_weak: &slint::Weak<MainWindow>,
    k: rdev::Key,
    is_press: bool,
    key_map: &HashMap<String, usize>,
) {
    let target_key_str = format!("{:?}", k);

    // 通过 HashMap 直接获取索引，O(1) 查找
    if let Some(&index) = key_map.get(&target_key_str) {
        if let Some(ui) = ui_weak.upgrade() {
            let model = ui.get_keys();
            if let Some(mut data) = model.row_data(index) {
                data.is_pressed = is_press;
                model.set_row_data(index, data);
            }
        }
    }
}

/// 封装逻辑处理器（消费者）
/// 运行在 UI 线程，处理从监听线程接收到的所有事件
fn handle_backend_input(
    event_type: rdev::EventType,
    ui_weak: &slint::Weak<MainWindow>,
    capture_mode: &Arc<Mutex<bool>>,
    dialog_handle: &Arc<Mutex<Option<slint::Weak<KeyCaptureDialog>>>>,
    settings_handle: &Arc<Mutex<Option<slint::Weak<SettingsWindow>>>>,
    key_map: &HashMap<String, usize>,
) {
    // 使用 try_lock 避免阻塞
    if let Ok(mut is_capturing) = capture_mode.try_lock() {
        match event_type {
            rdev::EventType::KeyPress(k) => {
                if *is_capturing {
                    // 捕获模式下的逻辑 (UI 线程安全)
                    *is_capturing = false;
                    process_key_capture(
                        k,
                        ui_weak,
                        dialog_handle,
                        settings_handle,
                    );
                } else {
                    println!("{:?}\r", k); 
                    // 正常变色逻辑（使用 HashMap 快速查找）
                    update_key_visual_state(ui_weak, k, true, key_map);
                }
            }
            rdev::EventType::KeyRelease(k) => {
                if !*is_capturing {
                    update_key_visual_state(ui_weak, k, false, key_map);
                }
            }
            _ => {}
        }
    }
}