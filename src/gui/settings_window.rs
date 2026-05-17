// src/windows/settings_window.rs
use std::sync::{Arc, Mutex};
use slint::{ComponentHandle, Model};
use crate::state::AppState;
use crate::{SettingsWindow, KeyCaptureDialog, render_key_models, hex_str_to_color, apply_snapping, save_config};

pub fn setup_settings_window(
    settings: SettingsWindow,
    state: AppState,
    main_ui_weak: slint::Weak<crate::MainWindow>,
) {
    let real_config = state.config.lock().unwrap().clone();
    *state.temp_config.lock().unwrap() = real_config.clone();

    // 1. 初始化视图属性
    settings.set_global_grid_size(real_config.grid_size);
    settings.set_global_top_boundary(real_config.top_boundary);
    settings.set_global_border_width(real_config.global_border_width);
    settings.set_global_border_color(hex_str_to_color(&real_config.global_border_color));
    settings.set_root_preview_keys(render_key_models(&real_config));

    *state.settings_holder.lock().unwrap() = Some(settings.as_weak());

    // 2. 绑定各项编辑回调
    let tc = state.temp_config.clone();
    settings.on_global_border_edited(move |width, color| {
        let mut tmp = tc.lock().unwrap();
        tmp.global_border_width = width;
        tmp.global_border_color = color.to_string();
    });

    let selected_indices = Arc::new(Mutex::new(Vec::<usize>::new()));
    let settings_weak = settings.as_weak();

    let tc = state.temp_config.clone();
    let s_weak = settings_weak.clone();
    let sel_idx = selected_indices.clone();
    settings.on_update_key_config(move |index, x, y, w, h, color| {
        let idx = index as usize;
        let mut tmp = tc.lock().unwrap();
        if idx >= tmp.keys.len() { return; }

        let g_size = tmp.grid_size;
        let snapped_x = apply_snapping(x, g_size) as f32;
        let snapped_y = apply_snapping(y, g_size) as f32;
        
        let select_set = sel_idx.lock().unwrap();
        let targets = if select_set.contains(&idx) { select_set.clone() } else { vec![idx] };

        if let Some(s) = s_weak.upgrade() {
            let model = s.get_root_preview_keys();
            for &target_idx in &targets {
                if target_idx < tmp.keys.len() {
                    let k = &mut tmp.keys[target_idx];
                    k.x = snapped_x;
                    k.y = snapped_y;
                    k.width = w as f32;
                    k.height = h as f32;
                    k.color_pressed = color.to_string();

                    if let Some(mut data) = model.row_data(target_idx) {
                        data.x = snapped_x;
                        data.y = snapped_y;
                        data.w = w as f32;
                        data.h = h as f32;
                        data.color_hex = color.clone();
                        model.set_row_data(target_idx, data);
                    }
                }
            }
        }
    });

    let s_weak = settings_weak.clone();
    let sel_idx = selected_indices.clone();
    settings.on_key_clicked_in_settings(move |index, ctrl_pressed| {
        let idx = index as usize;
        if let Some(s) = s_weak.upgrade() {
            let mut select_set = sel_idx.lock().unwrap();
            if ctrl_pressed {
                if let Some(pos) = select_set.iter().position(|&i| i == idx) { select_set.remove(pos); }
                else { select_set.push(idx); }
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

    let tc = state.temp_config.clone();
    let s_weak = settings_weak.clone();
    settings.on_delete_key(move |index| {
        let mut tmp = tc.lock().unwrap();
        if (index as usize) < tmp.keys.len() {
            tmp.keys.remove(index as usize);
            let new_model = render_key_models(&tmp);
            if let Some(s) = s_weak.upgrade() { s.set_root_preview_keys(new_model); }
        }
    });

    let s_weak = settings_weak.clone();
    let state_save = state.clone();
    let m_ui_weak = main_ui_weak.clone();
    settings.on_save_config_clicked(move || {
        if let Some(s) = s_weak.upgrade() {
            let  tmp = state_save.temp_config.lock().unwrap();
            let mut real = state_save.config.lock().unwrap();
            *real = tmp.clone();
            save_config(&real);

            if let Some(main_ui) = m_ui_weak.upgrade() {
                main_ui.set_keys(render_key_models(&real));
                main_ui.set_global_border_width(real.global_border_width);
                main_ui.set_global_border_color(hex_str_to_color(&real.global_border_color));
                main_ui.set_top_boundary_px(real.top_boundary);
            }
            s.hide().unwrap();
        }
    });

    let tc = state.temp_config.clone();
    settings.on_grid_size_edited(move |sz| tc.lock().unwrap().grid_size = sz);
    let tc = state.temp_config.clone();
    settings.on_top_boundary_edited(move |bd| tc.lock().unwrap().top_boundary = bd);

    let state_add = state.clone();
    let s_weak = settings_weak.clone();
    settings.on_add_new_key(move || {
        *state_add.capture_mode.lock().unwrap() = true;
        if let Some(s) = s_weak.upgrade() { s.set_capturing_mode(true); }
        let capture_dialog = KeyCaptureDialog::new().unwrap();
        capture_dialog.show().unwrap();
        *state_add.dialog_holder.lock().unwrap() = Some(capture_dialog.as_weak());
    });

    let s_weak = settings_weak.clone();
    let state_close = state.clone();
    settings.on_close_clicked(move || {
        if let Some(s) = s_weak.upgrade() { s.hide().unwrap(); }
        *state_close.settings_holder.lock().unwrap() = None;
    });

    settings.show().unwrap();
}