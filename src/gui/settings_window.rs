// src/windows/settings_window.rs
use crate::calculate_window_size;
use crate::state::{AppState, UIAction};
use crate::{KeyCaptureDialog, SettingsWindow, hex_str_to_color, create_model, save_config};
use slint::ComponentHandle;

pub fn setup_settings_window(
    settings: SettingsWindow,
    state: AppState,
    main_ui_weak: slint::Weak<crate::MainWindow>,
) {
    let real_config = state.config.lock().unwrap().clone();
    *state.temp_config.lock().unwrap() = real_config.clone();

    // 1. 初始化渲染视图参数
    settings.set_global_grid_size(real_config.grid_size);
    settings.set_global_top_boundary(real_config.top_boundary);
    settings.set_global_border_width(real_config.global_border_width);
    settings.set_global_border_color(hex_str_to_color(&real_config.global_border_color));
    settings.set_key_margin_width(real_config.key_margin_width);
    settings.set_root_preview_keys(create_model(&real_config.keys));

    *state.settings_holder.lock().unwrap() = Some(settings.as_weak());

    // 2. 将前端视图回调安全投递至 Dispatcher
    let settings_weak = settings.as_weak();
    let state_clone = state.clone();
    settings.on_handle_canvas_pointer_down(move |x, y, _ctrl| {
        state_clone.dispatch(UIAction::HitTestAndSelect { canvas_x: x, canvas_y: y }, &settings_weak);
    });

    let settings_weak = settings.as_weak();
    let state_clone = state.clone();
    settings.on_update_key_position(move |index, x, y, cw, ch| {
        state_clone.dispatch(UIAction::DragKeyOnCanvas { index, mouse_x: x, mouse_y: y, canvas_w: cw, canvas_h: ch }, &settings_weak);
    });

    let settings_weak = settings.as_weak();
    let state_clone = state.clone();
    settings.on_update_key_position_x(move |index, val, cw, ch| {
        state_clone.dispatch(UIAction::SpinBoxUpdateX { index, value: val, canvas_w: cw, canvas_h: ch }, &settings_weak);
    });

    let settings_weak = settings.as_weak();
    let state_clone = state.clone();
    settings.on_update_key_position_y(move |index, val, cw, ch| {
        state_clone.dispatch(UIAction::SpinBoxUpdateY { index, value: val, canvas_w: cw, canvas_h: ch }, &settings_weak);
    });

    // 3. 全局基础配置保存与退出交互
    let tc = state.temp_config.clone();
    settings.on_grid_size_edited(move |sz| tc.lock().unwrap().grid_size = sz);
    let tc = state.temp_config.clone();
    settings.on_top_boundary_edited(move |bd| tc.lock().unwrap().top_boundary = bd);
    let tc = state.temp_config.clone();
    settings.on_key_margin_edited(move |margin| tc.lock().unwrap().key_margin_width = margin);

    let state_add = state.clone();
    let s_weak = settings.as_weak();
    settings.on_add_new_key(move || {
        *state_add.capture_mode.lock().unwrap() = true;
        if let Some(s) = s_weak.upgrade() { s.set_capturing_mode(true); }
        let capture_dialog = KeyCaptureDialog::new().unwrap();
        capture_dialog.show().unwrap();
        *state_add.dialog_holder.lock().unwrap() = Some(capture_dialog.as_weak());
    });

    // 4. 按键大小/颜色/删除操作
    let tc = state.temp_config.clone();
    settings.on_update_key_size(move |index, w, h| {
        let idx = index as usize;
        let mut tmp = tc.lock().unwrap();
        if idx < tmp.keys.len() {
            tmp.keys[idx].width = w;
            tmp.keys[idx].height = h;
        }
    });
    let tc = state.temp_config.clone();
    settings.on_update_key_color(move |index, color| {
        let idx = index as usize;
        let mut tmp = tc.lock().unwrap();
        if idx < tmp.keys.len() {
            tmp.keys[idx].color_pressed = color.to_string();
        }
    });
    let state_del = state.clone();
    let s_weak = settings.as_weak();
    settings.on_delete_key(move |index| {
        let idx = index as usize;
        let mut tmp = state_del.temp_config.lock().unwrap();
        if idx < tmp.keys.len() {
            tmp.keys.remove(idx);
        }
        if let Some(s) = s_weak.upgrade() {
            s.set_selected_index(-1);
            s.set_root_preview_keys(create_model(&tmp.keys));
        }
    });

    let state_save = state.clone();
    let s_weak = settings.as_weak();
    settings.on_save_config(move || {
        if let Some(s) = s_weak.upgrade() {
            let mut real = state_save.config.lock().unwrap();
            let tmp = state_save.temp_config.lock().unwrap();
            *real = tmp.clone();
            save_config(&real);

            if let Some(main_ui) = main_ui_weak.upgrade() {
                let (w, h) = calculate_window_size(&real);
                main_ui.window().set_size(slint::PhysicalSize::new(w as u32, h as u32));
                main_ui.set_keys(create_model(&real.keys));
                main_ui.set_global_border_width(real.global_border_width);
                main_ui.set_global_border_color(hex_str_to_color(&real.global_border_color));
                main_ui.set_key_margin_width(real.key_margin_width);
                main_ui.set_top_boundary_px(real.top_boundary);
            }
            s.hide().unwrap();
        }
    });
}