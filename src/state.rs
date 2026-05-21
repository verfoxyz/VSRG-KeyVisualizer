// src/state.rs
use std::sync::{Arc, Mutex};
use slint::Model;
use crate::{
    AppConfig, BarNote, SettingsWindow, KeyCaptureDialog,
    render_key_models,
};
use crate::physics::MovementPipeline;

/// 🌟 UI 意图枚举 —— 整个配置窗口所有用户操作的统一描述
pub enum UIAction {
    /// 点击画布坐标进行 hit-test 选中
    HitTestAndSelect {
        canvas_x: i32,
        canvas_y: i32,
    },
    /// 在画布上拖拽按键（mouse_x/y 是 raw 画布坐标，内部会减去 drag_offset）
    DragKeyOnCanvas {
        index: i32,
        mouse_x: i32,
        mouse_y: i32,
        canvas_w: i32,
        canvas_h: i32,
    },
    /// 右侧 SpinBox 手动修改 X 坐标
    SpinBoxUpdateX {
        index: i32,
        value: i32,
        canvas_w: i32,
        canvas_h: i32,
    },
    /// 右侧 SpinBox 手动修改 Y 坐标
    SpinBoxUpdateY {
        index: i32,
        value: i32,
        canvas_w: i32,
        canvas_h: i32,
    },
}

pub struct AppState {
    pub config: Arc<Mutex<AppConfig>>,
    pub temp_config: Arc<Mutex<AppConfig>>,
    pub active_notes: Arc<Mutex<Vec<BarNote>>>,
    pub capture_mode: Arc<Mutex<bool>>,
    pub dialog_holder: Arc<Mutex<Option<slint::Weak<KeyCaptureDialog>>>>,
    pub settings_holder: Arc<Mutex<Option<slint::Weak<SettingsWindow>>>>,
    /// 拖拽偏移：鼠标点击位置到按键左上角的偏移 (px)
    pub drag_offset: Arc<Mutex<(i32, i32)>>,
}

impl AppState {
    pub fn new(init_config: AppConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(init_config)),
            temp_config: Arc::new(Mutex::new(AppConfig::default())),
            active_notes: Arc::new(Mutex::new(Vec::new())),
            capture_mode: Arc::new(Mutex::new(false)),
            dialog_holder: Arc::new(Mutex::new(None)),
            settings_holder: Arc::new(Mutex::new(None)),
            drag_offset: Arc::new(Mutex::new((0, 0))),
        }
    }
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            temp_config: Arc::clone(&self.temp_config),
            active_notes: Arc::clone(&self.active_notes),
            capture_mode: Arc::clone(&self.capture_mode),
            dialog_holder: Arc::clone(&self.dialog_holder),
            settings_holder: Arc::clone(&self.settings_holder),
            drag_offset: Arc::clone(&self.drag_offset),
        }
    }
}

impl AppState {
    /// 🌟 中央唯一分发器 (Dispatcher)
    pub fn dispatch(&self, action: UIAction, settings_weak: &slint::Weak<SettingsWindow>) {
        let ui = match settings_weak.upgrade() {
            Some(window) => window,
            None => return,
        };

        let mut tmp = self.temp_config.lock().unwrap();

        match action {
            UIAction::HitTestAndSelect { canvas_x, canvas_y } => {
                // 1. 遍历所有按键做 AABB hit-test
                let margin = tmp.key_margin_width;
                let hit_idx = tmp.keys.iter().position(|k| {
                    canvas_x >= k.x - margin
                        && canvas_x <= k.x + k.width + margin
                        && canvas_y >= k.y - margin
                        && canvas_y <= k.y + k.height + margin
                });

                if let Some(idx) = hit_idx {
                    let i = idx as i32;
                    // 选中该按键
                    ui.set_selected_index(i);
                    ui.set_current_x(tmp.keys[idx].x);
                    ui.set_current_y(tmp.keys[idx].y);
                    ui.set_current_w(tmp.keys[idx].width);
                    ui.set_current_h(tmp.keys[idx].height);
                    ui.set_current_color(tmp.keys[idx].color_pressed.clone().into());

                    // 计算并保存拖拽偏移 = 点击坐标 - 按键左上角
                    let off_x = canvas_x - tmp.keys[idx].x;
                    let off_y = canvas_y - tmp.keys[idx].y;
                    *self.drag_offset.lock().unwrap() = (off_x, off_y);

                    // 回刷预览画布
                    ui.set_root_preview_keys(render_key_models(&tmp));
                } else {
                    // 点击空白区域 -> 取消选中
                    ui.set_selected_index(-1);
                }
            }

            UIAction::DragKeyOnCanvas { index, mouse_x, mouse_y, canvas_w, canvas_h } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }

                // 用存储的拖拽偏移将 raw 画布坐标转为按键目标坐标
                let (off_x, off_y) = *self.drag_offset.lock().unwrap();
                let target_x = mouse_x - off_x;
                let target_y = mouse_y - off_y;

                // 初始化物理管线
                let pipeline = MovementPipeline {
                    canvas_w,
                    canvas_h,
                    margin: tmp.key_margin_width,
                };

                // 穿过物理过滤器流
                let (real_x, real_y) = pipeline.transform_position(idx, target_x, target_y, &tmp.keys, true);

                // 更新核心状态
                tmp.keys[idx].x = real_x;
                tmp.keys[idx].y = real_y;

                // 触发数据回刷闭环
                self.sync_and_refresh_ui(idx, real_x, real_y, &ui, &tmp);
            }

            UIAction::SpinBoxUpdateX { index, value, canvas_w, canvas_h } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }

                let pipeline = MovementPipeline {
                    canvas_w,
                    canvas_h,
                    margin: tmp.key_margin_width,
                };

                // 穿过物理过滤器（手动 SpinBox 调整，关闭磁性吸附过滤）
                let (real_x, real_y) = pipeline.transform_position(idx, value, tmp.keys[idx].y, &tmp.keys, false);

                tmp.keys[idx].x = real_x;
                self.sync_and_refresh_ui(idx, real_x, real_y, &ui, &tmp);
            }

            UIAction::SpinBoxUpdateY { index, value, canvas_w, canvas_h } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }

                let pipeline = MovementPipeline {
                    canvas_w,
                    canvas_h,
                    margin: tmp.key_margin_width,
                };

                let (real_x, real_y) = pipeline.transform_position(idx, tmp.keys[idx].x, value, &tmp.keys, false);

                tmp.keys[idx].y = real_y;
                self.sync_and_refresh_ui(idx, real_x, real_y, &ui, &tmp);
            }
        }
    }

    /// 统一刷新回调：更新大画布模型 + 回刷前端属性面板，彻底防死锁回弹
    fn sync_and_refresh_ui(
        &self,
        _target_idx: usize,
        real_x: i32,
        real_y: i32,
        ui: &SettingsWindow,
        tmp: &AppConfig,
    ) {
        ui.set_root_preview_keys(render_key_models(tmp));
        ui.set_current_x(real_x);
        ui.set_current_y(real_y);
    }
}