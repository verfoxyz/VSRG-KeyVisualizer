// src/windows/settings_window.rs
use crate::core::color::{hex_str_to_color, merge_alpha, split_alpha};
use crate::core::config_manager::ConfigManager;
use crate::configs;
use crate::gui::param_panel_window::setup_param_panel_window;
use crate::ui::model::create_model;
use crate::ui::state::{AppState, UIAction};
use crate::{KeyCaptureDialog, ParamPanelWindow, SettingsWindow, save_config_to_profile};
use i_slint_backend_winit::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, VecModel, SharedString};
use std::rc::Rc;
use std::sync::OnceLock;

/// 缓存主显示器尺寸（宽、高），首次通过 winit 异步获取后写入
pub(super) static PRIMARY_SCREEN_SIZE: OnceLock<(u32, u32)> = OnceLock::new();

pub fn setup_settings_window(
    settings: SettingsWindow,
    state: AppState,
    main_ui_weak: slint::Weak<crate::MainWindow>,
) {
    let real_config = state.config.lock().unwrap().clone();
    *state.temp_config.lock().unwrap() = real_config.clone();

    // 异步获取主显示器尺寸并缓存（仅首次）
    if PRIMARY_SCREEN_SIZE.get().is_none() {
        let main_weak_clone = main_ui_weak.clone();
        slint::spawn_local(async move {
            if let Some(main_ui) = main_weak_clone.upgrade()
                && let Ok(winit_window) = main_ui.window().winit_window().await
                    && let Some(monitor) = winit_window.primary_monitor() {
                        let size = monitor.size();
                        let _ = PRIMARY_SCREEN_SIZE.set((size.width, size.height));
                    }
        }).unwrap();
    }

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

    // ====== 初始化 Profile 列表 ======
    {
        let profiles = configs::list_profiles();
        tracing::debug!("[PROFILES] list_profiles() returned {} items: {:?}", profiles.len(), profiles);
        let string_list: Vec<SharedString> = profiles.iter().map(|s| SharedString::from(s.as_str())).collect();
        tracing::debug!("[PROFILES] SharedString list has {} items: {:?}", string_list.len(), string_list);
        let model: ModelRc<SharedString> = Rc::new(VecModel::from(string_list)).into();
        tracing::debug!("[PROFILES] model row_count: {}", model.row_count());
        settings.set_profile_list(model);
        // 立即读回验证
        let readback = settings.get_profile_list();
        tracing::debug!("[PROFILES] readback row_count: {}", readback.row_count());
    }

    // 设置当前激活的 profile 名和选中索引
    {
        let current = state.current_profile.lock().unwrap().clone();
        tracing::debug!("[PROFILES] current_profile = {:?}", current);
        settings.set_current_profile_name(current.as_str().into());
        let profiles = configs::list_profiles();
        tracing::debug!("[PROFILES] for index lookup: profiles = {:?}", profiles);
        if let Some(idx) = profiles.iter().position(|p| p == &current) {
            tracing::debug!("[PROFILES] setting profile_selected = {}", idx);
            settings.set_profile_selected(idx as i32);
        } else {
            tracing::warn!("[PROFILES] current profile {:?} not found in list!", current);
        }
    }

    // Profile 选择回调：切换配置（不关闭窗口，不更新主窗口）
    let settings_weak_switch = settings.as_weak();
    let state_switch = state.clone();
    settings.on_switch_to_profile(move |index| {
        let profiles = configs::list_profiles();
        if index < 0 || index as usize >= profiles.len() { return; }
        let name = profiles[index as usize].clone();
        if name == *state_switch.current_profile.lock().unwrap() {
            return; // 选中的就是当前配置，不切换
        }
        // 放弃当前 temp_config 的未保存修改，直接加载新配置到 temp_config
        configs::switch_profile(&name);
        let new_config = configs::load_profile(&name).unwrap_or_default();
        // 只更新 temp_config（预览用）和 current_profile，不动 config（等保存才更新主窗口）
        *state_switch.temp_config.lock().unwrap() = new_config;
        *state_switch.current_profile.lock().unwrap() = name;
        // 刷新配置窗口预览画布
        if let Some(s) = settings_weak_switch.upgrade() {
            let tmp = state_switch.temp_config.lock().unwrap();
            s.set_root_preview_keys(create_model(&tmp.keys));
            s.set_global_top_boundary(tmp.top_boundary);
            s.set_global_border_width(tmp.global_border_width);
            let (key_color_rgb, key_opacity_pct) = split_alpha(&tmp.global_key_color);
            s.set_global_key_color_hex(key_color_rgb.into());
            s.set_global_key_opacity_percent(key_opacity_pct);
            s.set_global_border_color_hex(tmp.global_border_color.clone().into());
            s.set_key_margin_width(tmp.key_margin_width);
            s.set_flow_direction(tmp.flow_direction);
            s.set_flow_speed(tmp.flow_speed);
            s.set_front_line_emit(tmp.front_line_emit);
            s.set_current_profile_name(state_switch.current_profile.lock().unwrap().clone().into());
            // 清空选中状态
            s.set_selected_index(-1);
            *state_switch.selected_indices.lock().unwrap() = std::collections::HashSet::new();
        }
    });

    // "新增配置"回调：用当前配置创建新 profile
    let settings_weak_create = settings.as_weak();
    let state_create = state.clone();
    settings.on_add_new_profile(move || {
        let profiles = configs::list_profiles();
        // 找一个不重复的名字：New Profile, New Profile 2, ...
        let mut new_name = "New Profile".to_string();
        let mut counter = 1;
        while profiles.contains(&new_name) {
            counter += 1;
            new_name = format!("New Profile {}", counter);
        }
        // 用当前 temp_config 的内容创建新 profile
        let cfg = state_create.temp_config.lock().unwrap().clone();
        configs::create_profile(&new_name, &cfg);
        // 刷新 profile 列表
        let updated = configs::list_profiles();
        let model: ModelRc<SharedString> = Rc::new(VecModel::from(
            updated.iter().map(|s| SharedString::from(s.as_str())).collect::<Vec<_>>()
        )).into();
        if let Some(s) = settings_weak_create.upgrade() {
            s.set_profile_list(model);
        }
    });

    // 删除 profile 回调（预删除：加入待删列表，UI 移除，保存时才真正执行）
    let settings_weak_delete = settings.as_weak();
    let state_delete = state.clone();
    settings.on_delete_profile(move |index| {
        let profiles = configs::list_profiles();
        if index < 0 || index as usize >= profiles.len() { return; }
        let name = &profiles[index as usize];
        let active = state_delete.current_profile.lock().unwrap().clone();
        let is_active = *name == active;

        // 加入待删列表
        state_delete.pending_deletions.lock().unwrap().push(name.clone());

        // 如果删除的是当前激活配置，切换到下一个
        if is_active {
            let next = if index + 1 < profiles.len() as i32 {
                Some(profiles[(index + 1) as usize].clone())
            } else if profiles.len() > 1 {
                Some(profiles[(index - 1) as usize].clone())
            } else {
                None
            };
            if let Some(ref next) = next {
                *state_delete.current_profile.lock().unwrap() = next.clone();
                if let Some(cfg) = configs::load_profile(next) {
                    *state_delete.temp_config.lock().unwrap() = cfg;
                }
            }
        }
        // 刷新 UI 列表：从真实列表中移除已标记删除的项
        let updated: Vec<String> = configs::list_profiles().into_iter()
            .filter(|p| !state_delete.pending_deletions.lock().unwrap().contains(p))
            .collect();
        let model: ModelRc<SharedString> = Rc::new(VecModel::from(
            updated.iter().map(|s| SharedString::from(s.as_str())).collect::<Vec<_>>()
        )).into();
        if let Some(s) = settings_weak_delete.upgrade() {
            s.set_profile_list(model);
            let tmp = state_delete.temp_config.lock().unwrap();
            s.set_root_preview_keys(create_model(&tmp.keys));
            s.set_selected_index(-1);
            *state_delete.selected_indices.lock().unwrap() = std::collections::HashSet::new();
            s.set_current_profile_name(state_delete.current_profile.lock().unwrap().clone().into());
        }
    });

    // 重命名 profile 回调
    let settings_weak_rename = settings.as_weak();
    let state_rename = state.clone();
    settings.on_rename_profile(move |index, new_name| {
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() { return; }
        let profiles = configs::list_profiles();
        if index < 0 || index as usize >= profiles.len() { return; }
        let old_name = &profiles[index as usize];
        if old_name == &new_name { return; }
        if configs::rename_profile(old_name, &new_name) {
            // 如果重命名的是当前激活的配置，更新 current_profile
            {
                let mut cur = state_rename.current_profile.lock().unwrap();
                if *cur == *old_name {
                    *cur = new_name.clone();
                    configs::switch_profile(&new_name);
                }
            }
            // 刷新列表
            let updated = configs::list_profiles();
            let model: ModelRc<SharedString> = Rc::new(VecModel::from(
                updated.iter().map(|s| SharedString::from(s.as_str())).collect::<Vec<_>>()
            )).into();
            if let Some(s) = settings_weak_rename.upgrade() {
                s.set_profile_list(model);
                s.set_current_profile_name(state_rename.current_profile.lock().unwrap().clone().into());
                // 更新选中索引
                if let Some(idx) = updated.iter().position(|p| p == &new_name) {
                    s.set_profile_selected(idx as i32);
                }
            }
        }
    });

    // 2. 将前端视图回调安全投递至 Dispatcher
    let settings_weak = settings.as_weak();
    let state_clone = state.clone();
    settings.on_handle_canvas_pointer_down(move |x, y, ctrl| {
        state_clone.dispatch(UIAction::HitTestAndSelect { canvas_x: x, canvas_y: y, ctrl }, &settings_weak);
    });

    let settings_weak = settings.as_weak();
    let s_dirty = settings.as_weak();
    let state_clone = state.clone();
    settings.on_update_key_position(move |index, x, y, cw, ch| {
        state_clone.dispatch(UIAction::DragKeyOnCanvas { index, mouse_x: x, mouse_y: y, canvas_w: cw, canvas_h: ch }, &settings_weak);
        if let Some(s) = s_dirty.upgrade() { s.set_config_dirty(true); }
    });

    let settings_weak = settings.as_weak();
    let s_dirty = settings.as_weak();
    let state_clone = state.clone();
    settings.on_update_key_position_x(move |index, val, cw, ch| {
        state_clone.dispatch(UIAction::SpinBoxUpdateX { index, value: val, canvas_w: cw, canvas_h: ch }, &settings_weak);
        if let Some(s) = s_dirty.upgrade() { s.set_config_dirty(true); }
    });

    let settings_weak = settings.as_weak();
    let s_dirty = settings.as_weak();
    let state_clone = state.clone();
    settings.on_update_key_position_y(move |index, val, cw, ch| {
        state_clone.dispatch(UIAction::SpinBoxUpdateY { index, value: val, canvas_w: cw, canvas_h: ch }, &settings_weak);
        if let Some(s) = s_dirty.upgrade() { s.set_config_dirty(true); }
    });

    // 3. 全局基础配置保存与退出交互（所有编辑操作标记 dirty）
    let mark = |s: &slint::Weak<SettingsWindow>| {
        if let Some(ui) = s.upgrade() { ui.set_config_dirty(true); }
    };
    let tc = state.temp_config.clone();
    let s = settings.as_weak();
    settings.on_top_boundary_edited(move |bd| { tc.lock().unwrap().top_boundary = bd; mark(&s); });
    let tc = state.temp_config.clone();
    let s = settings.as_weak();
    settings.on_key_margin_edited(move |margin| { tc.lock().unwrap().key_margin_width = margin; mark(&s); });
    let tc = state.temp_config.clone();
    let s = settings.as_weak();
    settings.on_border_color_edited(move |color| { tc.lock().unwrap().global_border_color = color.to_string(); mark(&s); });
    let tc = state.temp_config.clone();
    let s = settings.as_weak();
    settings.on_key_color_edited(move |color| {
        let mut tmp = tc.lock().unwrap();
        let (_, old_pct) = split_alpha(&tmp.global_key_color);
        tmp.global_key_color = merge_alpha(&color, old_pct);
        mark(&s);
    });
    let tc = state.temp_config.clone();
    let s = settings.as_weak();
    settings.on_key_opacity_edited(move |pct| {
        let mut tmp = tc.lock().unwrap();
        let (rgb, _) = split_alpha(&tmp.global_key_color);
        tmp.global_key_color = merge_alpha(&rgb, pct);
        mark(&s);
    });
    let tc = state.temp_config.clone();
    let s = settings.as_weak();
    settings.on_flow_direction_edited(move |dir| { tc.lock().unwrap().flow_direction = dir; mark(&s); });
    let tc = state.temp_config.clone();
    let s = settings.as_weak();
    settings.on_flow_speed_edited(move |speed| { tc.lock().unwrap().flow_speed = speed; mark(&s); });
    let tc = state.temp_config.clone();
    let s = settings.as_weak();
    settings.on_front_line_emit_toggled(move |val| { tc.lock().unwrap().front_line_emit = val; mark(&s); });

    let state_add = state.clone();
    let s_weak = settings.as_weak();
    settings.on_add_new_key(move || {
        // 检查是否已有按键捕获对话框，防止重复创建
        if let Some(holder) = state_add.dialog_holder.lock().unwrap().as_ref()
            && let Some(existing) = holder.upgrade() {
                existing.show().unwrap();
                return;
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
        if let Some(win) = s_weak.upgrade() {
            state_dispatch.dispatch(UIAction::BatchUpdateWidth { index, value: w }, &win.as_weak());
            state_dispatch.dispatch(UIAction::BatchUpdateHeight { index, value: h }, &win.as_weak());
            win.set_current_w(w);
            win.set_current_h(h);
            win.set_config_dirty(true);
        }
    });
    let state_dispatch = state.clone();
    let s_weak = settings.as_weak();
    settings.on_update_key_color(move |index, color| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(UIAction::BatchUpdateColor { index, color: color.to_string() }, &s.as_weak());
            s.set_config_dirty(true);
        }
    });
    let state_dispatch = state.clone();
    let s_weak = settings.as_weak();
    settings.on_update_key_opacity(move |index, pct| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(UIAction::BatchUpdateOpacity { index, pct }, &s.as_weak());
            s.set_config_dirty(true);
        }
    });
    let state_dispatch = state.clone();
    let s_weak = settings.as_weak();
    settings.on_update_key_bar_width_percent(move |index, pct| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(UIAction::BatchUpdateBarWidthPercent { index, pct }, &s.as_weak());
            s.set_config_dirty(true);
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
            s.set_config_dirty(true);
        }
    });

    // 多选删除（通过 dispatch）
    let state_del = state.clone();
    let s_weak = settings.as_weak();
    settings.on_delete_selected_keys(move || {
        if let Some(s) = s_weak.upgrade() {
            state_del.dispatch(UIAction::BatchDeleteKeys, &s.as_weak());
            s.set_config_dirty(true);
        }
    });

    // 5. 独立参数面板窗口开关（延迟创建避免 Slint 回调内重入卡死）
    {
        let state_toggle = state.clone();
        let s_weak = settings.as_weak();
        settings.on_toggle_param_panel(move || {
            let s = match s_weak.upgrade() {
                Some(s) => s,
                None => {
                    tracing::error!("[PARAM-PANEL] toggle: settings_weak 已失效");
                    return;
                }
            };

            tracing::debug!("[PARAM-PANEL] toggle_param_panel 被触发");

            // 检查是否已有面板窗口（强引用直判）
            let mut holder = state_toggle.param_panel_holder.lock().unwrap();
            if holder.is_some() {
                // 面板已存在：关闭并销毁它
                tracing::debug!("[PARAM-PANEL] close existing panel window");
                if let Some(ref panel) = *holder {
                    panel.hide().unwrap();
                }
                *holder = None;
                s.set_panel_window_open(false);
                return;
            }
            drop(holder); // 提前释放锁，避免后续创建过程中持有锁

            tracing::debug!("[PARAM-PANEL] scheduling deferred panel creation via timer");

            // ⭐ 延迟到下一帧事件循环再创建窗口，避免 Slint 回调内重入
            let state_create = state_toggle.clone();
            let s_weak_create = s_weak.clone();
            // SingleShot 定时器：触发一次后自动停止，使用 slint::Timer::single_shot 无需手动管理
            slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                tracing::debug!("[PARAM-PANEL] deferred creation timer fired");
                let s = match s_weak_create.upgrade() {
                    Some(s) => s,
                    None => {
                        tracing::error!("[PARAM-PANEL] deferred: settings_weak expired");
                        return;
                    }
                };

                if let Ok(panel) = ParamPanelWindow::new() {
                    tracing::debug!("[PARAM-PANEL] ParamPanelWindow::new() OK");
                    // 从 settings 窗口同步属性到面板窗口
                    panel.set_selected_index(s.get_selected_index());
                    panel.set_current_x(s.get_current_x());
                    panel.set_current_y(s.get_current_y());
                    panel.set_current_w(s.get_current_w());
                    panel.set_current_h(s.get_current_h());
                    panel.set_current_color(s.get_current_color());
                    panel.set_current_opacity_percent(s.get_current_opacity_percent());
                    panel.set_current_bar_width_percent(s.get_current_bar_width_percent());
                    panel.set_global_key_color_hex(s.get_global_key_color_hex());
                    panel.set_global_key_opacity_percent(s.get_global_key_opacity_percent());
                    panel.set_global_border_color_hex(s.get_global_border_color_hex());
                    panel.set_front_line_emit(s.get_front_line_emit());
                    panel.set_flow_direction(s.get_flow_direction());
                    panel.set_flow_speed(s.get_flow_speed());
                    panel.set_global_top_boundary(s.get_global_top_boundary());
                    panel.set_key_margin_width(s.get_key_margin_width());

                    tracing::debug!("[PARAM-PANEL] calling setup_param_panel_window...");
                    setup_param_panel_window(panel, state_create.clone(), s.as_weak());
                    s.set_panel_window_open(true);
                    tracing::debug!("[PARAM-PANEL] panel window setup complete");
                } else {
                    tracing::error!("[PARAM-PANEL] ParamPanelWindow::new() failed");
                }
            });
        });
    }

    let state_save = state.clone();
    let s_weak = settings.as_weak();
    settings.on_save_config(move || {
        let s = match s_weak.upgrade() {
            Some(s) => s,
            None => return,
        };

        // 未修改：直接关闭
        if !s.get_config_dirty() {
            if let Err(e) = s.hide() {
                tracing::error!("隐藏设置窗口失败: {}", e);
            }
            return;
        }

        // ===== 1. 持久化配置 =====
        {
            let mut real = state_save.config.lock().unwrap_or_else(|e| e.into_inner());
            let tmp = state_save.temp_config.lock().unwrap_or_else(|e| e.into_inner());
            *real = tmp.clone();

            let profile =
                state_save.current_profile.lock().unwrap_or_else(|e| e.into_inner()).clone();
            save_config_to_profile(&profile, &real);

            // 重建按键位置缓存
            let mut cache = state_save.key_positions.lock().unwrap_or_else(|e| e.into_inner());
            cache.clear();
            for k in &real.keys {
                cache.push((k.rdev_key_name.clone(), k.x, k.y));
            }
        }

        s.set_config_dirty(false);
        state_save.notes_dirty.store(true, std::sync::atomic::Ordering::Relaxed);

        // ===== 2. 通过 ConfigManager 更新主窗口 =====
        if let Some(main_ui) = main_ui_weak.upgrade() {
            let cfg = state_save.config.lock().unwrap_or_else(|e| e.into_inner());
            ConfigManager::apply_to_main_window(&cfg, &main_ui, PRIMARY_SCREEN_SIZE.get().copied());
        }

        // ===== 3. 执行待删除的 profile =====
        let pending = state_save.pending_deletions.lock().unwrap().clone();
        for del_name in &pending {
            if *del_name != *state_save.current_profile.lock().unwrap() {
                configs::delete_profile(del_name, del_name);
            }
        }
        state_save.pending_deletions.lock().unwrap().clear();
    });
}