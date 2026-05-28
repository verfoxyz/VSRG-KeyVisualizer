// src/events.rs
use crossbeam_channel::Receiver;
use slint::ComponentHandle;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use crate::state::AppState;
use crate::{MyKeyEvent, BarNote, KeyConfig, create_model, update_key_visual_state};

/// 核心录制管线处理器
struct MacroRecorder;
impl MacroRecorder {
    fn process(event: (&str, bool), state: &AppState) {
        let (rdev_name, is_press) = event;
        if !is_press { return; }
        if rdev_name == "Escape" {
            state.capture_mode.store(false, Ordering::SeqCst);
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
                rdev_key_name: rdev_name.to_string(),
                display_name: rdev_name.replace("Key", ""),
                x: new_x,
                y: new_y,
                width: 80,
                height: 80,
                color_pressed: "#4A90E2FF".into(),
                bar_width_percent: 100,
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

/// 运行模式下实时渲染管线处理器
struct LiveVisualizer;
impl LiveVisualizer {
    fn process(event: (&str, bool), state: &AppState, notes: &mut Vec<BarNote>, ui: &crate::MainWindow) {
        let (rdev_name, is_press) = event;
        let cfg = state.config.lock().unwrap();
        if is_press {
            if let Some(key_cfg) = cfg.keys.iter().find(|k| k.rdev_key_name == rdev_name) {
                for note in notes.iter_mut().filter(|n| n.rdev_key_name == rdev_name && n.is_growing) {
                    note.is_growing = false;
                }
                let speed = cfg.flow_speed.max(1);
                let pct = key_cfg.bar_width_percent.max(10).min(100);
                let bar_w = key_cfg.width * pct / 100;
                let bar_h = key_cfg.height * pct / 100;
                let (start_x, start_y, note_w, note_h, vx, vy) = match cfg.flow_direction {
                    1 => (key_cfg.x, key_cfg.y + cfg.top_boundary, bar_w, 0, 0, -speed),
                    2 => (key_cfg.x + cfg.top_boundary, key_cfg.y, 0, bar_h, -speed, 0),
                    3 => (key_cfg.x + key_cfg.width, key_cfg.y, 0, bar_h, speed, 0),
                    _ => (key_cfg.x, key_cfg.y + key_cfg.height, bar_w, 0, 0, speed),
                };
                notes.push(BarNote {
                    rdev_key_name: rdev_name.to_string(),
                    x: start_x,
                    width: note_w,
                    y: start_y,
                    height: note_h,
                    color: key_cfg.color_pressed.clone(),
                    is_growing: true,
                    vel_x: vx,
                    vel_y: vy,
                });
                update_key_visual_state(&ui.as_weak(), rdev_name.to_string(), true);
            }
        } else {
            for note in notes.iter_mut().filter(|n| n.rdev_key_name == rdev_name && n.is_growing) {
                note.is_growing = false;
            }
            update_key_visual_state(&ui.as_weak(), rdev_name.to_string(), false);
        }
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
            let mut local_dirty = false;

            // 1. 消费硬件事件（内联翻译，避免枚举分配）
            while let Ok(raw_event) = rx.try_recv() {
                let (name, is_press) = match raw_event {
                    MyKeyEvent::Press { rdev_name } => (rdev_name, true),
                    MyKeyEvent::Release { rdev_name } => (rdev_name, false),
                };

                if state.capture_mode.load(Ordering::Relaxed) {
                    MacroRecorder::process((&name, is_press), &state);
                } else if let Some(ui) = ui_weak.upgrade() {
                    LiveVisualizer::process((&name, is_press), &state, &mut notes, &ui);
                    local_dirty = true;
                }
            }

            // 2. 音符瀑布流物理生长步进循环
            if let Some(ui) = ui_weak.upgrade() {
                let cfg = state.config.lock().unwrap();
                let speed = cfg.flow_speed.max(1);
                let flow_dir = cfg.flow_direction;

                for note in notes.iter_mut() {
                    if note.is_growing {
                        if flow_dir == 2 || flow_dir == 3 {
                            note.width += speed;
                            if flow_dir == 2 { note.x -= speed; }
                        } else if flow_dir == 1 {
                            note.height += speed;
                            note.y -= speed;
                        } else {
                            note.height += speed;
                        }
                    } else {
                        note.x += note.vel_x;
                        note.y += note.vel_y;
                    }
                }

                // 缓存窗口尺寸（减少 FFI 调用），触发脏标记
                let cw = ui.get_window_width_px();
                let ch = ui.get_window_height_px();
                let old_len = notes.len();
                match flow_dir {
                    1 => notes.retain(|n| n.y + n.height >= -ch / 2),
                    2 => notes.retain(|n| n.x + n.width >= -cw / 2),
                    3 => notes.retain(|n| n.x <= cw + cw / 2),
                    _ => notes.retain(|n| n.y <= ch + ch / 2),
                }
                if notes.len() != old_len { local_dirty = true; }

                // 用预构建的 HashMap O(1) 查找按键位置
                let pos_cache = state.key_positions.lock().unwrap();
                let pos_map: HashMap<String, (i32, i32)> = pos_cache.iter()
                    .map(|(k, x, y)| (k.clone(), (*x, *y)))
                    .collect();
                drop(pos_cache);

                let use_x = flow_dir == 2 || flow_dir == 3;
                notes.sort_by(|a, b| {
                    let pa = pos_map.get(a.rdev_key_name.as_str()).copied().unwrap_or((0, 0));
                    let pb = pos_map.get(b.rdev_key_name.as_str()).copied().unwrap_or((0, 0));
                    let va = if use_x { pa.0 } else { pa.1 };
                    let vb = if use_x { pb.0 } else { pb.1 };
                    match flow_dir {
                        0 => vb.cmp(&va),
                        1 => va.cmp(&vb),
                        2 => va.cmp(&vb),
                        3 => vb.cmp(&va),
                        _ => va.cmp(&vb),
                    }
                });

                // 有音符就每帧更新模型（音符位置/尺寸持续变化），
                // 无音符时跳过重建以节省开销
                if !notes.is_empty() || local_dirty || state.notes_dirty.load(Ordering::Relaxed) {
                    ui.set_bar_notes(create_model(&notes));
                    state.notes_dirty.store(false, Ordering::Relaxed);
                }
            }
        },
    );
    event_timer
}