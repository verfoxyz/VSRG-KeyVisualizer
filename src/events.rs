// src/events.rs
use crossbeam_channel::Receiver;
use slint::ComponentHandle;
use crate::state::AppState;
use crate::{MyKeyEvent, BarNote, KeyConfig, render_key_models, render_bar_models, update_key_visual_state};

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
                    color_pressed: "#4A90E2".into(),
                };
                tmp.keys.push(key.clone());

                if let Some(s) = state.settings_holder.lock().unwrap().as_ref().and_then(|s| s.upgrade()) {
                    s.set_root_preview_keys(render_key_models(&tmp));
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
                    notes.push(BarNote {
                        rdev_key_name: rdev_name.clone(),
                        x: key_cfg.x,
                        width: key_cfg.width,
                        y: key_cfg.y + key_cfg.height,
                        height: 0,
                        color: key_cfg.color_pressed.clone(),
                        is_growing: true,
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
                let top_boundary = cfg.top_boundary;

                let speed = 4;

                for note in notes.iter_mut() {
                    if note.is_growing {
                        note.height += speed;
                    } else {
                        // 非生长阶段：音符向上移动（物理 Y 递增 = 向上）
                        note.y += speed;
                    }
                }

                // 音符物理 Y 超过此值时移除 = 音符底部移出窗口顶部
                let window_top_phys = cfg.keys.iter()
                    .map(|k| k.y + k.height)
                    .max()
                    .unwrap_or(0)
                    + top_boundary;
                notes.retain(|note| note.y < window_top_phys);
                ui.set_bar_notes(render_bar_models(&notes));
            }
        },
    );
    event_timer
}