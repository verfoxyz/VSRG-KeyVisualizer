use crossbeam_channel as channel;
use i_slint_backend_winit::WinitWindowAccessor;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::collections::HashMap;
use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;

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

fn default_border_width() -> i32 { 1 }
fn default_border_color() -> String { "#555555".into() }
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

fn make_window_no_activate(window: &winit::window::Window) {
    #[cfg(windows)]
    {
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
            SetWindowLongW(
                hwnd,
                GWL_EXSTYLE,
                ex_style | WS_EX_NOACTIVATE.0 as i32,
            );
        }
    }
}

fn main() {
    let (tx, rx) = channel::unbounded::<MyKeyEvent>();

    let init_config = load_config();
    let mut current_top_boundary = init_config.top_boundary;
    let config = Arc::new(Mutex::new(init_config));
    let temp_config = Arc::new(Mutex::new(AppConfig::default()));

    let ui = MainWindow::new().unwrap();
    
    let init_config_for_ui = config.lock().unwrap();
    ui.set_global_border_width(init_config_for_ui.global_border_width);
    ui.set_global_border_color(hex_str_to_color(&init_config_for_ui.global_border_color));
    ui.set_top_boundary_px(current_top_boundary);

    let active_notes: Arc<Mutex<Vec<BarNote>>> = Arc::new(Mutex::new(Vec::new()));
    let capture_mode = Arc::new(Mutex::new(false));
    let dialog_holder = Arc::new(Mutex::new(None));
    let settings_holder = Arc::new(Mutex::new(None));

    let model = render_key_models(&config.lock().unwrap());
    ui.set_keys(model.clone());

    // 修复点一：初始化阶段仅配置基础窗体属性，避免在此处直接改变高级底层 ExStyle 造成 DWM 初始化死锁。
    ui.window().with_winit_window(|window| {
        window.set_transparent(true);
        window.set_decorations(false);
        window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
    });

    let ui_weak = ui.as_weak();
    ui.on_gui_drag_window(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let _ = ui.window().with_winit_window(|w| w.drag_window());
        }
    });

    let capture_mode_for_settings = capture_mode.clone();
    let config_for_settings = config.clone();
    let temp_config_for_settings = temp_config.clone();
    let ui_weak_for_settings = ui.as_weak();
    let dialog_holder_for_settings = dialog_holder.clone();
    let settings_holder_for_settings = settings_holder.clone();

    ui.on_request_settings(move || {
        let settings = SettingsWindow::new().unwrap();
        
        let real_config = config_for_settings.lock().unwrap().clone();
        *temp_config_for_settings.lock().unwrap() = real_config.clone();

        settings.set_global_grid_size(real_config.grid_size);
        settings.set_global_top_boundary(real_config.top_boundary);
        settings.set_global_border_width(real_config.global_border_width);
        settings.set_global_border_color(hex_str_to_color(&real_config.global_border_color));
        settings.set_root_preview_keys(render_key_models(&real_config));

        *settings_holder_for_settings.lock().unwrap() = Some(settings.as_weak());

        let temp_config_for_border = temp_config_for_settings.clone();
        settings.on_global_border_edited(move |width, color| {
            let mut tmp_inner = temp_config_for_border.lock().unwrap();
            tmp_inner.global_border_width = width;
            tmp_inner.global_border_color = color.to_string();
        });

        let selected_indices = Arc::new(Mutex::new(Vec::<usize>::new()));

        let settings_weak = settings.as_weak();
        let temp_config_for_update = temp_config_for_settings.clone();
        let settings_weak_for_update = settings_weak.clone();
        let selected_indices_for_update = selected_indices.clone();
        
        settings.on_update_key_config(move |index, x, y, w, h, color| {
            let idx = index as usize;
            let mut tmp_inner = temp_config_for_update.lock().unwrap();
            let g_size = tmp_inner.grid_size;
            let select_set = selected_indices_for_update.lock().unwrap();

            if idx >= tmp_inner.keys.len() {
                return;
            }

            let snapped_x = apply_snapping(x, g_size) as f32;
            let snapped_y = apply_snapping(y, g_size) as f32;
            let new_w = w as f32;
            let new_h = h as f32;
            let new_color = color.to_string();

            let old_key = &tmp_inner.keys[idx];
            let x_changed = snapped_x != old_key.x;
            let y_changed = snapped_y != old_key.y;
            let w_changed = new_w != old_key.width;
            let h_changed = new_h != old_key.height;
            let color_changed = new_color != old_key.color_pressed;

            let targets: Vec<usize> = if select_set.contains(&idx) {
                select_set.clone()
            } else {
                vec![idx]
            };

            if let Some(s) = settings_weak_for_update.upgrade() {
                let model = s.get_root_preview_keys();
                
                for &target_idx in &targets {
                    if target_idx < tmp_inner.keys.len() {
                        if x_changed { tmp_inner.keys[target_idx].x = snapped_x; }
                        if y_changed { tmp_inner.keys[target_idx].y = snapped_y; }
                        if w_changed { tmp_inner.keys[target_idx].width = new_w; }
                        if h_changed { tmp_inner.keys[target_idx].height = new_h; }
                        if color_changed { tmp_inner.keys[target_idx].color_pressed = new_color.clone(); }

                        if let Some(mut data) = model.row_data(target_idx) {
                            if x_changed { data.x = snapped_x; }
                            if y_changed { data.y = snapped_y; }
                            if w_changed { data.w = new_w; }
                            if h_changed { data.h = new_h; }
                            if color_changed {
                                data.color_hex = color.clone();
                                let hex_str = color.as_str().trim_start_matches('#');
                                if let Ok(rgb) = u32::from_str_radix(hex_str, 16) {
                                    data.pressed_color = slint::Color::from_argb_encoded(rgb | 0xFF000000);
                                }
                            }
                            model.set_row_data(target_idx, data);
                        }
                    }
                }
            }
        });

        let settings_weak_for_click = settings_weak.clone();
        let selected_indices_for_click = selected_indices.clone();
        settings.on_key_clicked_in_settings(move |index, ctrl_pressed| {
            let idx = index as usize;
            if let Some(s) = settings_weak_for_click.upgrade() {
                let mut select_set = selected_indices_for_click.lock().unwrap();
                
                if ctrl_pressed {
                    if let Some(pos) = select_set.iter().position(|&i| i == idx) {
                        select_set.remove(pos);
                    } else {
                        select_set.push(idx);
                    }
                } else {
                    select_set.clear();
                    select_set.push(idx);
                }

                s.set_selected_index(idx as i32);

                let model = s.get_root_preview_keys();
                for i in 0..model.row_count() {
                    if let Some(mut data) = model.row_data(i) {
                        data.selected = select_set.contains(&i);
                        model.set_row_data(i, data);
                    }
                }
            }
        });

        let temp_config_for_delete = temp_config_for_settings.clone();
        let settings_weak_for_delete = settings_weak.clone();

        settings.on_delete_key(move |index| {
            let idx = index as usize;
            let mut tmp_inner = temp_config_for_delete.lock().unwrap();

            if idx < tmp_inner.keys.len() {
                tmp_inner.keys.remove(idx);
                let new_model = render_key_models(&tmp_inner);
                if let Some(s) = settings_weak_for_delete.upgrade() {
                    s.set_root_preview_keys(new_model);
                }
            }
        });

        let config_for_save = config_for_settings.clone();
        let temp_config_for_save = temp_config_for_settings.clone();
        let ui_weak_for_save = ui_weak_for_settings.clone();
        let settings_weak_for_save = settings_weak.clone();
        let selected_indices_for_save = selected_indices.clone();
        
        settings.on_save_config_clicked(move || {
            if let Some(s) = settings_weak_for_save.upgrade() {
                let idx = s.get_selected_index() as usize;
                let mut tmp_inner = temp_config_for_save.lock().unwrap();
                let g_size = tmp_inner.grid_size;

                if idx < tmp_inner.keys.len() {
                    tmp_inner.keys[idx].x = apply_snapping(s.get_current_x(), g_size) as f32;
                    tmp_inner.keys[idx].y = apply_snapping(s.get_current_y(), g_size) as f32;
                    tmp_inner.keys[idx].width = s.get_current_w() as f32;
                    tmp_inner.keys[idx].height = s.get_current_h() as f32;
                    tmp_inner.keys[idx].color_pressed = s.get_current_color().to_string();
                }

                let mut real_config = config_for_save.lock().unwrap();
                *real_config = tmp_inner.clone();
                save_config(&real_config);

                if let Some(main_ui) = ui_weak_for_save.upgrade() {
                    main_ui.set_keys(render_key_models(&real_config));
                    main_ui.set_global_border_width(real_config.global_border_width);
                    main_ui.set_global_border_color(hex_str_to_color(&real_config.global_border_color));
                    main_ui.set_top_boundary_px(real_config.top_boundary);
                }
                
                selected_indices_for_save.lock().unwrap().clear();
                s.hide().unwrap();
            }
        });

        let temp_config_for_grid = temp_config_for_settings.clone();
        settings.on_grid_size_edited(move |new_size| {
            let mut tmp_inner = temp_config_for_grid.lock().unwrap();
            tmp_inner.grid_size = new_size;
        });

        let temp_config_for_boundary = temp_config_for_settings.clone();
        settings.on_top_boundary_edited(move |new_boundary| {
            let mut tmp_inner = temp_config_for_boundary.lock().unwrap();
            tmp_inner.top_boundary = new_boundary;
        });

        let capture_mode_for_add_key = capture_mode_for_settings.clone();
        let dialog_holder_clone = dialog_holder_for_settings.clone();
        let settings_weak_for_add_key = settings_weak.clone();

        settings.on_add_new_key(move || {
            let mut mode = capture_mode_for_add_key.lock().unwrap();
            *mode = true; 

            if let Some(s) = settings_weak_for_add_key.upgrade() {
                s.set_capturing_mode(true);
            }

            let capture_dialog = KeyCaptureDialog::new().unwrap();
            capture_dialog.show().unwrap();
            *dialog_holder_clone.lock().unwrap() = Some(capture_dialog.as_weak());
        });

        let settings_holder_for_close = settings_holder_for_settings.clone();
        let settings_weak_for_close = settings_weak.clone();

        settings.on_close_clicked(move || {
            if let Some(s) = settings_weak_for_close.upgrade() {
                s.hide().unwrap();
            }
            *settings_holder_for_close.lock().unwrap() = None;
        });

        settings.show().unwrap();
    });

    ui.on_request_close(|| slint::quit_event_loop().unwrap());

    init_platform_input_listener(tx, &ui);

    let ui_weak = ui.as_weak();
    let active_notes_for_timer = active_notes.clone();
    let config_for_timer = config.clone();
    let temp_config_for_timer = temp_config.clone();

    let settings_holder_for_timer = settings_holder.clone();
    let dialog_holder_for_timer = dialog_holder.clone();
    let capture_mode_for_timer = capture_mode.clone();

    // 修复点二：建立一客制化单次定时器，保证在 Slint 主事件循环跑起来的第一帧后，再动态注入 WS_EX_NOACTIVATE。
    let init_timer = slint::Timer::default();
    let ui_weak_for_init = ui.as_weak();
    init_timer.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_millis(100),
        move || {
            if let Some(main_ui) = ui_weak_for_init.upgrade() {
                main_ui.window().with_winit_window(|window| {
                    make_window_no_activate(window);
                });
            }
        }
    );

    let event_timer = slint::Timer::default();
    event_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(16),
        move || {
            let mut notes = active_notes_for_timer.lock().unwrap();
            
            while let Ok(event) = rx.try_recv() {
                let is_capturing = *capture_mode_for_timer.lock().unwrap();
                
                if is_capturing {
                    if let MyKeyEvent::Press { rdev_name } = event {
                        if rdev_name == "Escape" {
                            *capture_mode_for_timer.lock().unwrap() = false;
                            if let Some(s) = settings_holder_for_timer.lock().unwrap().as_ref().and_then(|s| s.upgrade()) {
                                s.set_capturing_mode(false);
                            }
                            if let Some(d) = dialog_holder_for_timer.lock().unwrap().as_ref().and_then(|d| d.upgrade()) {
                                d.hide().unwrap();
                            }
                            *dialog_holder_for_timer.lock().unwrap() = None;
                            continue;
                        }

                        let mut tmp_inner = temp_config_for_timer.lock().unwrap();
                        let spawn_x = (tmp_inner.keys.len() * 90) as f32 + 10.0;

                        let new_key = KeyConfig {
                            rdev_key_name: rdev_name.clone(),
                            display_name: rdev_name.replace("Key", ""),
                            x: spawn_x,
                            y: 10.0,
                            width: 80.0,
                            height: 80.0,
                            color_pressed: "#4A90E2".into(),
                        };

                        tmp_inner.keys.push(new_key);

                        let new_model = render_key_models(&tmp_inner);
                        if let Some(s) = settings_holder_for_timer.lock().unwrap().as_ref().and_then(|s| s.upgrade()) {
                            s.set_root_preview_keys(new_model);
                            s.set_capturing_mode(false);
                        }
                        
                        if let Some(d) = dialog_holder_for_timer.lock().unwrap().as_ref().and_then(|d| d.upgrade()) {
                            d.hide().unwrap();
                        }
                        *dialog_holder_for_timer.lock().unwrap() = None;
                        *capture_mode_for_timer.lock().unwrap() = false;
                    }
                    continue; 
                }

                let config_inner = config_for_timer.lock().unwrap();
                match event {
                    MyKeyEvent::Press { rdev_name } => {
                        if config_inner.keys.iter().any(|k| k.rdev_key_name == rdev_name) {
                            if let Some(key_cfg) = config_inner.keys.iter().find(|k| k.rdev_key_name == rdev_name) {
                                for note in notes.iter_mut().filter(|n| n.rdev_key_name == rdev_name && n.is_growing) {
                                    note.is_growing = false;
                                }

                                let spawn_y = key_cfg.y + key_cfg.height;
                                notes.push(BarNote {
                                    rdev_key_name: rdev_name.clone(),
                                    x: key_cfg.x,
                                    width: key_cfg.width,
                                    y: spawn_y,
                                    height: 0.0,
                                    color: key_cfg.color_pressed.clone(),
                                    is_growing: true,
                                });
                            }
                            update_key_visual_state(&ui_weak, rdev_name, true);
                        }
                    }
                    MyKeyEvent::Release { rdev_name } => {
                        if config_inner.keys.iter().any(|k| k.rdev_key_name == rdev_name) {
                            for note in notes.iter_mut().filter(|n| n.rdev_key_name == rdev_name) {
                                note.is_growing = false;
                            }
                            update_key_visual_state(&ui_weak, rdev_name, false);
                        }
                    }
                }
            }

            let move_speed = 6.0;
            for note in notes.iter_mut() {
                if note.is_growing {
                    note.height += move_speed;
                } else {
                    note.y += move_speed;
                }
            }
            
            let boundary_val = {
                config_for_timer.lock().unwrap().top_boundary
            };
            let max_height = 200.0 + boundary_val as f32;
            notes.retain(|note| note.is_growing || (note.y - note.height) < max_height);

            if let Some(main_ui) = ui_weak.upgrade() {
                main_ui.set_bar_notes(render_bar_models(&notes));
            }
        },
    );

    ui.run().unwrap();
}

fn update_key_visual_state(
    ui_weak: &slint::Weak<MainWindow>,
    key_name: String,
    is_pressed: bool,
) {
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

    thread::spawn(move || unsafe {
        static mut GLOBAL_TX: Option<channel::Sender<MyKeyEvent>> = None;
        GLOBAL_TX = Some(tx);

        unsafe extern "system" fn raw_input_wnd_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            if msg == WM_INPUT {
                let mut size: u32 = 0;
                let h_raw = HRAWINPUT(lparam.0 as *mut std::ffi::c_void);

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
            // 修复点三：明确传递当前纯消息窗体的 HWND，使其完全闭环，不与主线程 winit 发生链条抢占。
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

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

        let hwnd_msg_sink = CreateWindowExW(
            Default::default(),
            class_name,
            w!("RawInputSink"),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(HINSTANCE(null_mut())),
            None,
        )
        .expect("Windows 纯消息窗口创建失败！");

        let mut devices: [RAWINPUTDEVICE; 1] = zeroed();
        devices[0].usUsagePage = 1;
        devices[0].usUsage = 6;
        devices[0].dwFlags = RIDEV_INPUTSINK;
        devices[0].hwndTarget = hwnd_msg_sink;

        if RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32).is_err() {
            eprintln!("Windows Raw Input 注册失败！");
            return;
        }

        let mut msg = std::mem::zeroed();
        // 修复点四：指定明确捕获 hwnd_msg_sink 的消息流，切断对 winit 主线程潜在的虚假全局消息拦截。
        while GetMessageW(&mut msg, Some(hwnd_msg_sink), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

#[cfg(windows)]
fn win_vkey_to_rdev_string(vkey: u16) -> String {
    match vkey {
        0x41..=0x5A => format!("Key{}", (vkey as u8 as char)),
        0x30..=0x39 => format!("Key{}", (vkey as u8 as char)),
        0x1B => "Escape".into(),
        0x20 => "Space".into(),
        0x0D => "Return".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x25 => "LeftArrow".into(),
        0x26 => "UpArrow".into(),
        0x27 => "RightArrow".into(),
        0x28 => "DownArrow".into(),
        0xBA => "SemiColon".into(),
        0xDE => "Quote".into(),
        0xBB => "Equal".into(),
        0xBD => "Minus".into(),
        0xDC => "BackSlash".into(),
        0xDB => "LeftBracket".into(),
        0xDD => "RightBracket".into(),
        0xC0 => "BackQuote".into(),
        0xBF => "Slash".into(),
        0x6A => "KpMultiply".into(),
        0x6B => "KpAdd".into(),
        0x6D => "KpSubtract".into(),
        0x6F => "KpDivide".into(),
        _ => format!("Unknown(0x{:X})", vkey),
    }
}