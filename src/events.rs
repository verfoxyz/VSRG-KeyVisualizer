// src/events.rs
use crossbeam_channel::Receiver;
use slint::ComponentHandle;
use crate::state::AppState;
use crate::{MyKeyEvent, BarNote, KeyConfig, render_key_models, render_bar_models, update_key_visual_state};

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
            
            while let Ok(event) = rx.try_recv() {
                let is_capturing = *state.capture_mode.lock().unwrap();
                if is_capturing {
                    handle_capture_event(&event, &state);
                    continue;
                }

                let cfg = state.config.lock().unwrap();
                match event {
                    MyKeyEvent::Press { rdev_name } => {
                        if let Some(key_cfg) = cfg.keys.iter().find(|k| k.rdev_key_name == rdev_name) {
                            for note in notes.iter_mut().filter(|n| n.rdev_key_name == rdev_name && n.is_growing) {
                                note.is_growing = false;
                            }
                            notes.push(BarNote {
                                rdev_key_name: rdev_name.clone(),
                                x: key_cfg.x, width: key_cfg.width,
                                y: key_cfg.y + key_cfg.height, height: 0.0,
                                color: key_cfg.color_pressed.clone(), is_growing: true,
                            });
                            update_key_visual_state(&ui_weak, rdev_name, true);
                        }
                    }
                    MyKeyEvent::Release { rdev_name } => {
                        if cfg.keys.iter().any(|k| k.rdev_key_name == rdev_name) {
                            for note in notes.iter_mut().filter(|n| n.rdev_key_name == rdev_name) { note.is_growing = false; }
                            update_key_visual_state(&ui_weak, rdev_name, false);
                        }
                    }
                }
            }

            // 更新下落音符
            for note in notes.iter_mut() {
                if note.is_growing { note.height += 6.0; } else { note.y += 6.0; }
            }
            let max_height = 200.0 + state.config.lock().unwrap().top_boundary as f32;
            notes.retain(|note| note.is_growing || (note.y - note.height) < max_height);

            if let Some(main_ui) = ui_weak.upgrade() {
                main_ui.set_bar_notes(render_bar_models(&notes));
            }
        },
    );
    event_timer
}

fn handle_capture_event(event: &MyKeyEvent, state: &AppState) {
    if let MyKeyEvent::Press { rdev_name } = event {
        if rdev_name == "Escape" {
            *state.capture_mode.lock().unwrap() = false;
            if let Some(s) = state.settings_holder.lock().unwrap().as_ref().and_then(|s| s.upgrade()) { s.set_capturing_mode(false); }
            if let Some(d) = state.dialog_holder.lock().unwrap().as_ref().and_then(|d| d.upgrade()) { d.hide().unwrap(); }
            *state.dialog_holder.lock().unwrap() = None;
            return;
        }

        let mut tmp = state.temp_config.lock().unwrap();
        let spawn_x = (tmp.keys.len() * 90) as f32 + 10.0;
        tmp.keys.push(KeyConfig {
            rdev_key_name: rdev_name.clone(),
            display_name: rdev_name.replace("Key", ""),
            x: spawn_x, y: 10.0, width: 80.0, height: 80.0,
            color_pressed: "#4A90E2".into(),
        });

        let new_model = render_key_models(&tmp);
        if let Some(s) = state.settings_holder.lock().unwrap().as_ref().and_then(|s| s.upgrade()) {
            s.set_root_preview_keys(new_model);
            s.set_capturing_mode(false);
        }
        if let Some(d) = state.dialog_holder.lock().unwrap().as_ref().and_then(|d| d.upgrade()) { d.hide().unwrap(); }
        *state.dialog_holder.lock().unwrap() = None;
        *state.capture_mode.lock().unwrap() = false;
    }
}