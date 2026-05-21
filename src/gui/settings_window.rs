// src/windows/settings_window.rs
use crate::calculate_window_size;
use crate::physics::handle_key_movement;
use crate::state::AppState;
use crate::{
    KeyCaptureDialog, SettingsWindow, apply_snapping, hex_str_to_color, render_key_models,
    save_config,
};
use slint::{ComponentHandle, Model};
use std::sync::{Arc, Mutex};

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
    settings.set_key_margin_width(real_config.key_margin_width);
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

    // 0. 处理画布点击事件 - 命中测试（判断点中了哪个按键）
    let tc_for_hit_test = state.temp_config.clone();
    let s_weak_for_hit_test = settings_weak.clone();
    let sel_idx_for_hit_test = selected_indices.clone();
    settings.on_handle_canvas_pointer_down(move |click_canvas_x, click_canvas_y| {
        let mut tmp = tc_for_hit_test.lock().unwrap();

        // 倒序遍历按键，做碰撞检测看点中了谁（后绘制的按键优先）
        for (idx, key) in tmp.keys.iter().enumerate().rev() {
            // 考虑边距的碰撞检测
            let key_x_start = key.x - tmp.key_margin_width;
            let key_x_end = key.x + key.width + tmp.key_margin_width;
            let key_y_start = key.y - tmp.key_margin_width;
            let key_y_end = key.y + key.height + tmp.key_margin_width;

            if click_canvas_x >= key_x_start && click_canvas_x <= key_x_end
                && click_canvas_y >= key_y_start && click_canvas_y <= key_y_end
            {
                // 更新选中状态
                if let Some(s) = s_weak_for_hit_test.upgrade() {
                    let mut select_set = sel_idx_for_hit_test.lock().unwrap();
                    select_set.clear();
                    select_set.push(idx);

                    s.set_selected_index(idx as i32);
                    s.set_has_active_selection(true);

                    // 核心：记录点击处距离该按键左上角的相对偏移
                    s.set_click_offset_x(click_canvas_x - key.x);
                    s.set_click_offset_y(click_canvas_y - key.y);

                    // 同步配置面板的数值
                    s.set_current_x(key.x);
                    s.set_current_y(key.y);
                    s.set_current_w(key.width);
                    s.set_current_h(key.height);
                    s.set_current_color(key.color_pressed.clone().into());
                }
                return;
            }
        }

        // 如果没有点中任何按键，清除选择状态
        if let Some(s) = s_weak_for_hit_test.upgrade() {
            sel_idx_for_hit_test.lock().unwrap().clear();
            s.set_selected_index(-1);
            s.set_has_active_selection(false);
        }
    });

    // 1. 处理位置更新（集成固体阻挡算法）
    let tc = state.temp_config.clone();
    let s_weak = settings_weak.clone();
    let sel_idx = selected_indices.clone();

    settings.on_update_key_position(move |index, x, y, current_canvas_w, current_canvas_h| {
        let idx = index as usize;
        let mut tmp = tc.lock().unwrap();
        if idx >= tmp.keys.len() {
            return;
        }

        let g_size = tmp.grid_size;
        let snapped_x = apply_snapping(x, g_size);
        let snapped_y = apply_snapping(y, g_size);

        let select_set = sel_idx.lock().unwrap();
        let targets = if select_set.contains(&idx) {
            select_set.clone()
        } else {
            vec![idx]
        };

        // 动态将前端由于窗口改变产生的长宽转换为物理像素 i32
        let canvas_w = current_canvas_w as i32;
        let canvas_h = current_canvas_h as i32;

        if let Some(s) = s_weak.upgrade() {
            let model = s.get_root_preview_keys();

            for &target_idx in &targets {
                if target_idx < tmp.keys.len() {
                    // 使用固体阻挡算法计算安全位置
                    // 确保直接把前端发来的连续原始坐标无损传给物理引擎即可
                    handle_key_movement(
                        target_idx,
                        x, // 此时这里的 x 和 y 就是没有被吸附污染过的纯鼠标位置 pure_mouse_x
                        y, // pure_mouse_y
                        tmp.key_margin_width,
                        &mut tmp.keys,
                        canvas_w,
                        canvas_h,
                    );

                    // 从数据源获取计算后的安全坐标
                    let k = &tmp.keys[target_idx];

                    if let Some(mut data) = model.row_data(target_idx) {
                        data.x = k.x as f32;
                        data.y = k.y as f32;
                        model.set_row_data(target_idx, data);
                    }

                    // ✨ 如果更新的是当前主选中的按键，顺便同步刷新右下角配置面板的 SpinBox 联动数值
                    if target_idx == idx {
                        s.set_current_x(k.x);
                        s.set_current_y(k.y);
                    }
                }
            }
        }
    });

    // 2. 处理尺寸更新
    let tc = state.temp_config.clone();
    let s_weak = settings_weak.clone();
    let sel_idx = selected_indices.clone();
    settings.on_update_key_size(move |index, w, h| {
        let idx = index as usize;
        let mut tmp = tc.lock().unwrap();
        if idx >= tmp.keys.len() {
            return;
        }

        let select_set = sel_idx.lock().unwrap();
        let targets = if select_set.contains(&idx) {
            select_set.clone()
        } else {
            vec![idx]
        };

        if let Some(s) = s_weak.upgrade() {
            let model = s.get_root_preview_keys();
            for &target_idx in &targets {
                if target_idx < tmp.keys.len() {
                    let k = &mut tmp.keys[target_idx];
                    // 只更新尺寸属性，确保最小值为1
                    k.width = std::cmp::max(1, w);
                    k.height = std::cmp::max(1, h);

                    if let Some(mut data) = model.row_data(target_idx) {
                        data.w = k.width as f32;
                        data.h = k.height as f32;
                        model.set_row_data(target_idx, data);
                    }
                }
            }
        }
    });

    // 3. 处理颜色更新
    let tc = state.temp_config.clone();
    let s_weak = settings_weak.clone();
    let sel_idx = selected_indices.clone();
    settings.on_update_key_color(move |index, color| {
        let idx = index as usize;
        let mut tmp = tc.lock().unwrap();
        if idx >= tmp.keys.len() {
            return;
        }

        let select_set = sel_idx.lock().unwrap();
        let targets = if select_set.contains(&idx) {
            select_set.clone()
        } else {
            vec![idx]
        };

        if let Some(s) = s_weak.upgrade() {
            let model = s.get_root_preview_keys();
            for &target_idx in &targets {
                if target_idx < tmp.keys.len() {
                    let k = &mut tmp.keys[target_idx];
                    // 只更新颜色属性
                    k.color_pressed = color.to_string();

                    if let Some(mut data) = model.row_data(target_idx) {
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

    let tc = state.temp_config.clone();
    let s_weak = settings_weak.clone();
    settings.on_delete_key(move |index| {
        let mut tmp = tc.lock().unwrap();
        if (index as usize) < tmp.keys.len() {
            tmp.keys.remove(index as usize);
            let new_model = render_key_models(&tmp);
            if let Some(s) = s_weak.upgrade() {
                s.set_root_preview_keys(new_model);
            }
        }
    });

    let s_weak = settings_weak.clone();
    let state_save = state.clone();
    let m_ui_weak = main_ui_weak.clone();
    settings.on_save_config_clicked(move || {
        if let Some(s) = s_weak.upgrade() {
            let tmp = state_save.temp_config.lock().unwrap();
            let mut real = state_save.config.lock().unwrap();
            *real = tmp.clone();
            save_config(&real);

            if let Some(main_ui) = m_ui_weak.upgrade() {
                let (width, height) = calculate_window_size(&real);
                main_ui.set_keys(render_key_models(&real));
                main_ui.set_window_width_px(width);
                main_ui.set_window_height_px(height);
                main_ui.set_global_border_width(real.global_border_width);
                main_ui.set_global_border_color(hex_str_to_color(&real.global_border_color));
                main_ui.set_key_margin_width(real.key_margin_width);
                main_ui.set_top_boundary_px(real.top_boundary);
            }
            s.hide().unwrap();
        }
    });

    let tc = state.temp_config.clone();
    settings.on_grid_size_edited(move |sz| tc.lock().unwrap().grid_size = sz);
    let tc = state.temp_config.clone();
    settings.on_top_boundary_edited(move |bd| tc.lock().unwrap().top_boundary = bd);
    let tc = state.temp_config.clone();
    settings.on_key_margin_edited(move |margin| tc.lock().unwrap().key_margin_width = margin);

    let state_add = state.clone();
    let s_weak = settings_weak.clone();
    settings.on_add_new_key(move || {
        *state_add.capture_mode.lock().unwrap() = true;
        if let Some(s) = s_weak.upgrade() {
            s.set_capturing_mode(true);
        }
        let capture_dialog = KeyCaptureDialog::new().unwrap();
        capture_dialog.show().unwrap();
        *state_add.dialog_holder.lock().unwrap() = Some(capture_dialog.as_weak());
    });

    let s_weak = settings_weak.clone();
    let state_close = state.clone();
    settings.on_close_clicked(move || {
        if let Some(s) = s_weak.upgrade() {
            s.hide().unwrap();
        }
        *state_close.settings_holder.lock().unwrap() = None;
    });

    settings.show().unwrap();
}