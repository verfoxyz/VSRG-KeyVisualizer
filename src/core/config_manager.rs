// src/core/config_manager.rs
//
// ConfigManager — 负责 AppConfig → UI State 的转换计算，
// 将 on_save_config 中的窗口计算、配置同步、位置恢复等逻辑解耦到此模块。

use crate::core::color::hex_str_to_color;
use crate::core::config_def::AppConfig;
use crate::platform::window::calculate_window_size;
use crate::ui::model::{compute_key_ratios, create_model};
use crate::MainWindow;
use slint::ComponentHandle;

/// 保存配置时的窗口与 UI 状态计算
pub struct ConfigManager;

impl ConfigManager {
    /// 将已保存的配置应用到主窗口 UI
    ///
    /// 调用者需确保 `config` 已写入 `AppState.config`。
    /// 此方法处理：基础属性更新、窗口尺寸/位置恢复、按键比例锚点计算、模型投递。
    pub fn apply_to_main_window(
        config: &AppConfig,
        main_ui: &MainWindow,
        primary_screen_size: Option<(u32, u32)>,
    ) {
        let (w, h) = calculate_window_size(config);

        // 1. 更新基础属性（方向、边距等），让画布心里有数
        main_ui.set_global_border_width(config.global_border_width);
        main_ui.set_global_border_color(hex_str_to_color(&config.global_border_color));
        main_ui.set_global_key_color(hex_str_to_color(&config.global_key_color));
        main_ui.set_key_margin_width(config.key_margin_width);
        main_ui.set_top_boundary_px(config.top_boundary);
        main_ui.set_flow_direction(config.flow_direction);

        // 2. 计算按键区域高度
        let max_bottom = config.keys.iter().map(|k| k.y + k.height).max().unwrap_or(0);
        let key_area_h = if max_bottom > 0 {
            max_bottom + config.key_margin_width
        } else {
            100
        };
        main_ui.set_key_area_height(key_area_h);

        // 3. 改变窗口的物理尺寸到目标大小
        main_ui.window().set_size(slint::PhysicalSize::new(w as u32, h as u32));

        // 4. 同步更新 UI 内部的像素宽高属性
        main_ui.set_window_width_px(w);
        main_ui.set_window_height_px(h);

        // 5. 用当前配置的窗口位置重新定位主窗口
        Self::restore_window_position(config, main_ui, primary_screen_size, w, h);

        // 6. 计算按键比例锚点并投递模型
        let key_model = create_model(&config.keys);
        compute_key_ratios(&key_model, w as f32, h as f32);
        main_ui.set_keys(key_model);
    }

    /// 恢复窗口位置，超出主显示器范围则重置居中
    fn restore_window_position(
        config: &AppConfig,
        main_ui: &MainWindow,
        primary_screen_size: Option<(u32, u32)>,
        win_w: i32,
        win_h: i32,
    ) {
        if let (Some(wx), Some(wy)) = (config.window_x, config.window_y) {
            let should_reset = if let Some((sw, sh)) = primary_screen_size {
                Self::is_center_outside(wx, wy, win_w as u32, win_h as u32, sw, sh)
            } else {
                wx < 0 || wy < 0
            };
            if should_reset {
                if let Some((sw, sh)) = primary_screen_size {
                    let cx = (sw.saturating_sub(win_w as u32) / 2) as i32;
                    let cy = (sh.saturating_sub(win_h as u32) / 2) as i32;
                    main_ui.window().set_position(slint::PhysicalPosition::new(cx, cy));
                }
            } else {
                main_ui.window().set_position(slint::PhysicalPosition::new(wx, wy));
            }
        }
    }

    fn is_center_outside(win_x: i32, win_y: i32, win_w: u32, win_h: u32, screen_w: u32, screen_h: u32) -> bool {
        let cx = win_x + (win_w / 2) as i32;
        let cy = win_y + (win_h / 2) as i32;
        cx < 0 || cy < 0 || cx as u32 > screen_w || cy as u32 > screen_h
    }
}
