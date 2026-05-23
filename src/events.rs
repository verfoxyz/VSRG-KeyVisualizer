// src/events.rs
use crossbeam_channel::Receiver;
use slint::ComponentHandle;
use crate::state::AppState;
use crate::{MyKeyEvent, BarNote, KeyConfig, calculate_window_size, create_model, update_key_visual_state};

/// 高层语义事件总线 
pub enum AppEvent {
    HardwareKeyPress { rdev_name: String },
    HardwareKeyRelease { rdev_name: String },
}

/// 核心录制管线处理器
struct MacroRecorder;
impl MacroRecorder {
    fn process(event: AppEvent, state: &AppState) {
        if let AppEvent::HardwareKeyPress { rdev_name } = event {
            if rdev_name == "Escape" {
                *state.capture_mode.lock().unwrap() = false;
                if let Some(s) = state.settings_holder.lock().unwrap().as_ref().and_then(|s| s.upgrade()) { s.set_capturing_mode(false); }
                if let Some(d) = state.dialog_holder.lock().unwrap().as_ref().and_then(|d| d.upgrade()) { d.hide().unwrap(); }
                *state.dialog_holder.lock().unwrap() = None;
                return;
            }

            // 同时写入 temp_config 和 config，确保主窗口瀑布流能立即看到新按键
            let new_key_cfg = {
                let mut tmp = state.temp_config.lock().unwrap();
                let margin = tmp.key_margin_width.max(10);
                let new_x = tmp.keys.iter()
                    .map(|k| k.x + k.width + margin)
                    .max()
                    .unwrap_or(margin);
                let new_y = tmp.keys.last().map(|k| k.y).unwrap_or(10);
                let key = KeyConfig {
                    rdev_key_name: rdev_name.clone(),
                    display_name: rdev_name.replace("Key", ""),
                    x: new_x,
                    y: new_y,
                    width: 80,
                    height: 80,
                    color_pressed: "#4A90E2FF".into(),
                };
                tmp.keys.push(key.clone());

                if let Some(s) = state.settings_holder.lock().unwrap().as_ref().and_then(|s| s.upgrade()) {
                    s.set_root_preview_keys(create_model(&tmp.keys));
                }
                key
            };

            // 同步到 config，使主窗口 LiveVisualizer 立即看到新按键
            state.config.lock().unwrap().keys.push(new_key_cfg);
        }
    }
}

/// 运行模式下实时渲染管线处理器
struct LiveVisualizer;
impl LiveVisualizer {
    fn process(event: AppEvent, state: &AppState, notes: &mut Vec<BarNote>, ui: &crate::MainWindow) {
        // MacroRecorder 添加新按键时会同步写入 config，因此只读 config 即可
        let cfg = state.config.lock().unwrap();
        match event {
            AppEvent::HardwareKeyPress { rdev_name } => {
                if let Some(key_cfg) = cfg.keys.iter().find(|k| k.rdev_key_name == rdev_name) {
                    for note in notes.iter_mut().filter(|n| n.rdev_key_name == rdev_name && n.is_growing) {
                        note.is_growing = false;
                    }
                    let (c_w, c_h) = calculate_window_size(&cfg);
                    let (start_x, start_y, note_w, note_h, vx, vy) = match cfg.flow_direction {
                        // ↑ 上：画布底部 = 0，从按键底部（= c_h - key_cfg.y）向上生长
                        1 => (key_cfg.x, c_h - key_cfg.y - key_cfg.height, key_cfg.width, 0, 0, -4),
                        // ← 左：从按键左侧（= c_w - key.w - key_cfg.x）向左生长
                        2 => (c_w - key_cfg.x - key_cfg.width, key_cfg.y, 0, key_cfg.height, -4, 0),
                        // → 右：从按键右侧（= key.x + key.w）向右生长
                        3 => (key_cfg.x + key_cfg.width, key_cfg.y, 0, key_cfg.height, 4, 0),
                        // ↓ 下：从按键底部（= key.y + key.h）向下生长（默认）
                        _ => (key_cfg.x, key_cfg.y + key_cfg.height, key_cfg.width, 0, 0, 4),
                    };
                    notes.push(BarNote {
                        rdev_key_name: rdev_name.clone(),
                        x: start_x,
                        width: note_w,
                        y: start_y,
                        height: note_h,
                        color: key_cfg.color_pressed.clone(),
                        is_growing: true,
                        vel_x: vx,
                        vel_y: vy,
                    });
                    update_key_visual_state(&ui.as_weak(), rdev_name.clone(), true);
                }
            }
            AppEvent::HardwareKeyRelease { rdev_name } => {
                for note in notes.iter_mut().filter(|n| n.rdev_key_name == rdev_name && n.is_growing) {
                    note.is_growing = false;
                }
                update_key_visual_state(&ui.as_weak(), rdev_name.clone(), false);
            }
        }
    }
}

/// 转换辅助函数
fn translate_event(raw: MyKeyEvent) -> AppEvent {
    match raw {
        MyKeyEvent::Press { rdev_name } => AppEvent::HardwareKeyPress { rdev_name },
        MyKeyEvent::Release { rdev_name } => AppEvent::HardwareKeyRelease { rdev_name },
    }
}

pub fn start_event_timer(
    rx: Receiver<MyKeyEvent>,
    state: AppState,
    ui_weak: slint::Weak<crate::MainWindow>,
) -> slint::Timer {
    let event_timer = slint::Timer::default();
    event_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(16),
        move || {
            let mut notes = state.active_notes.lock().unwrap();

            // 1. 消费和翻译硬件事件总线
            while let Ok(raw_event) = rx.try_recv() {
                let app_event = translate_event(raw_event);

                if *state.capture_mode.lock().unwrap() {
                    MacroRecorder::process(app_event, &state);
                } else if let Some(ui) = ui_weak.upgrade() {
                    LiveVisualizer::process(app_event, &state, &mut notes, &ui);
                }
            }

            // 2. 音符瀑布流物理生长步进循环
            if let Some(ui) = ui_weak.upgrade() {
                let cfg = state.config.lock().unwrap();
                for note in notes.iter_mut() {
                    if note.is_growing {
                        if cfg.flow_direction == 2 || cfg.flow_direction == 3 {
                            // ← 或 → 方向：宽度生长
                            note.width += 4;
                            if cfg.flow_direction == 2 {
                                // ←：向左生长，x 左移
                                note.x -= 4;
                            }
                        } else if cfg.flow_direction == 1 {
                            // ↑：向上生长，y 上移
                            note.height += 4;
                            note.y -= 4;
                        } else {
                            // ↓：向下生长（默认）
                            note.height += 4;
                        }
                    } else {
                        // 根据瀑布流方向移动
                        note.x += note.vel_x;
                        note.y += note.vel_y;
                    }
                }

                // 根据方向移除超出边界的音符（使用画布尺寸动态计算阈值）
                let cw = ui.get_window_width_px();
                let ch = ui.get_window_height_px();
                if cfg.flow_direction == 1 {
                    notes.retain(|n| n.y + n.height >= -ch / 2);
                } else if cfg.flow_direction == 2 {
                    notes.retain(|n| n.x + n.width >= -cw / 2);
                } else if cfg.flow_direction == 3 {
                    notes.retain(|n| n.x <= cw + cw / 2);
                } else {
                    notes.retain(|n| n.y <= ch + ch / 2);
                }
                ui.set_bar_notes(create_model(&notes));
            }
        },
    );
    event_timer
}