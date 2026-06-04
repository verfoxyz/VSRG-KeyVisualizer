// src/windows/settings_window.rs
use crate::calculate_window_size;
use crate::state::{AppState, UIAction};
use crate::{KeyCaptureDialog, SettingsWindow, hex_str_to_color, merge_alpha, split_alpha, create_model, compute_key_ratios, save_config_to_profile};
use slint::ComponentHandle;

pub fn setup_settings_window(
    settings: SettingsWindow,
    state: AppState,
    main_ui_weak: slint::Weak<crate::MainWindow>,
) {
    let real_config = state.config.lock().unwrap().clone();
    *state.temp_config.lock().unwrap() = real_config.clone();

    // 1. 初始化渲染视图参数
    settings.set_global_top_boundary(real_config.top_boundary);
    settings.set_global_border_width(real_config.global_border_width);
    settings.set_global_border_color(hex_str_to_color(&real_config.global_border_color));
    settings.set_global_key_color(hex_str_to_color(&real_config.global_key_color));
    settings.set_key_margin_width(real_config.key_margin_width);
    settings.set_root_preview_keys(create_model(&real_config.keys));

    // 初始化 Border Color 和 Key Color LineEdit 文本
    settings.set_global_border_color_hex(real_config.global_border_color.clone().into());
    // 从 #RRGGBBAA 中提取 #RRGGBB 和透明度百分比
    let (key_color_rgb, key_opacity_pct) = split_alpha(&real_config.global_key_color);
    settings.set_global_key_color_hex(key_color_rgb.into());
    settings.set_global_key_opacity_percent(key_opacity_pct);
    settings.set_flow_direction(real_config.flow_direction);
    settings.set_flow_speed(real_config.flow_speed);
    settings.set_front_line_emit(real_config.front_line_emit);

    *state.settings_holder.lock().unwrap() = Some(settings.as_weak());

    // 2. 将前端视图回调安全投递至 Dispatcher
    let settings_weak = settings.as_weak();
    let state_clone = state.clone();
    settings.on_handle_canvas_pointer_down(move |x, y, ctrl| {
        state_clone.dispatch(UIAction::HitTestAndSelect { canvas_x: x, canvas_y: y, ctrl }, &settings_weak);
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
    settings.on_top_boundary_edited(move |bd| tc.lock().unwrap().top_boundary = bd);
    let tc = state.temp_config.clone();
    settings.on_key_margin_edited(move |margin| tc.lock().unwrap().key_margin_width = margin);
    let tc = state.temp_config.clone();
    settings.on_border_color_edited(move |color| tc.lock().unwrap().global_border_color = color.to_string());
    let tc = state.temp_config.clone();
    settings.on_key_color_edited(move |color| {
        let mut tmp = tc.lock().unwrap();
        let (_, old_pct) = split_alpha(&tmp.global_key_color);
        tmp.global_key_color = merge_alpha(&color, old_pct);
    });
    let tc = state.temp_config.clone();
    settings.on_key_opacity_edited(move |pct| {
        let mut tmp = tc.lock().unwrap();
        let (rgb, _) = split_alpha(&tmp.global_key_color);
        tmp.global_key_color = merge_alpha(&rgb, pct);
    });

    let tc = state.temp_config.clone();
    settings.on_flow_direction_edited(move |dir| tc.lock().unwrap().flow_direction = dir);
    let tc = state.temp_config.clone();
    settings.on_flow_speed_edited(move |speed| tc.lock().unwrap().flow_speed = speed);
    let tc = state.temp_config.clone();
    settings.on_front_line_emit_toggled(move |val| tc.lock().unwrap().front_line_emit = val);

    let state_add = state.clone();
    let s_weak = settings.as_weak();
    settings.on_add_new_key(move || {
        // 检查是否已有按键捕获对话框，防止重复创建
        if let Some(holder) = state_add.dialog_holder.lock().unwrap().as_ref() {
            if let Some(existing) = holder.upgrade() {
                existing.show().unwrap();
                return;
            }
        }

        state_add.capture_mode.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(s) = s_weak.upgrade() { s.set_capturing_mode(true); }
        let capture_dialog = KeyCaptureDialog::new().unwrap();
        let dialog_weak = capture_dialog.as_weak();

        // 前端 ESC 关闭：避免后端处理
        let state_esc = state_add.clone();
        let s_esc = s_weak.clone();
        capture_dialog.on_escape_pressed(move || {
            state_esc.capture_mode.store(false, std::sync::atomic::Ordering::SeqCst);
            if let Some(s) = s_esc.upgrade() { s.set_capturing_mode(false); }
            if let Some(d) = dialog_weak.upgrade() { d.hide().unwrap(); }
            *state_esc.dialog_holder.lock().unwrap() = None;
        });

        capture_dialog.show().unwrap();
        *state_add.dialog_holder.lock().unwrap() = Some(capture_dialog.as_weak());
    });

    // 4. 按键大小/颜色/删除操作（通过 dispatch 实现多选批量编辑）
    let state_dispatch = state.clone();
    let s_weak = settings.as_weak();
    settings.on_update_key_size(move |index, w, h| {
        let s = match s_weak.upgrade() { Some(x) => x, None => return };
        state_dispatch.dispatch(UIAction::BatchUpdateWidth { index, value: w }, &s.as_weak());
        state_dispatch.dispatch(UIAction::BatchUpdateHeight { index, value: h }, &s.as_weak());
        if let Some(win) = s_weak.upgrade() {
            win.set_current_w(w);
            win.set_current_h(h);
        }
    });
    let state_dispatch = state.clone();
    let s_weak = settings.as_weak();
    settings.on_update_key_color(move |index, color| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(UIAction::BatchUpdateColor { index, color: color.to_string() }, &s.as_weak());
        }
    });
    let state_dispatch = state.clone();
    let s_weak = settings.as_weak();
    settings.on_update_key_opacity(move |index, pct| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(UIAction::BatchUpdateOpacity { index, pct }, &s.as_weak());
        }
    });
    let state_dispatch = state.clone();
    let s_weak = settings.as_weak();
    settings.on_update_key_bar_width_percent(move |index, pct| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(UIAction::BatchUpdateBarWidthPercent { index, pct }, &s.as_weak());
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

    // 多选删除（通过 dispatch）
    let state_del = state.clone();
    let s_weak = settings.as_weak();
    settings.on_delete_selected_keys(move || {
        if let Some(s) = s_weak.upgrade() {
            state_del.dispatch(UIAction::BatchDeleteKeys, &s.as_weak());
        }
    });

    let state_save = state.clone();
    let s_weak = settings.as_weak();
    settings.on_save_config(move || {
        if let Some(s) = s_weak.upgrade() {
            let mut real = state_save.config.lock().unwrap();
            let tmp = state_save.temp_config.lock().unwrap();
            
            // 保留窗口位置（temp_config 不包含窗口位置，从 real 继承）
            let saved_x = real.window_x;
            let saved_y = real.window_y;
            *real = tmp.clone();
            real.window_x = saved_x;
            real.window_y = saved_y;

            // 保存到当前激活的 profile
            let profile = state_save.current_profile.lock().unwrap().clone();
            save_config_to_profile(&profile, &real);

            // 重建按键位置缓存
            {
                let mut cache = state_save.key_positions.lock().unwrap();
                cache.clear();
                for k in &real.keys {
                    cache.push((k.rdev_key_name.clone(), k.x, k.y));
                }
            }
            state_save.notes_dirty.store(true, std::sync::atomic::Ordering::Relaxed);

            if let Some(main_ui) = main_ui_weak.upgrade() {
                let (w, h) = calculate_window_size(&real);
                
                // 1. 统一先更新基础属性（方向、边距等），让画布心里有数
                main_ui.set_global_border_width(real.global_border_width);
                main_ui.set_global_border_color(hex_str_to_color(&real.global_border_color));
                main_ui.set_global_key_color(hex_str_to_color(&real.global_key_color));
                main_ui.set_key_margin_width(real.key_margin_width);
                main_ui.set_top_boundary_px(real.top_boundary);
                main_ui.set_flow_direction(real.flow_direction);

                // 计算按键区域高度：最大物理 Y 范围 + 底部边距
                let max_bottom = real.keys.iter().map(|k| k.y + k.height).max().unwrap_or(0);
                let key_area_h = if max_bottom > 0 {
                    max_bottom + real.key_margin_width
                } else {
                    100
                };
                main_ui.set_key_area_height(key_area_h);

                // 2. 核心修复：先改变窗口的物理尺寸到目标大小
                main_ui.window().set_size(slint::PhysicalSize::new(w as u32, h as u32));
                
                // 3. 同步更新 UI 内部的像素宽高属性
                main_ui.set_window_width_px(w);
                main_ui.set_window_height_px(h);

                // 4. 严格使用最终的目标画布宽高 (w, h) 来计算按键比例锚点！
                // 避免使用临时的 safe_h 导致非对称方向切换时算错相对位置
                let key_model = create_model(&real.keys);
                compute_key_ratios(&key_model, w as f32, h as f32);
                
                // 5. 最后投递数据模型，触发 Slint 重新绘制
                main_ui.set_keys(key_model);
            }
            s.hide().unwrap();
        }
    });
}