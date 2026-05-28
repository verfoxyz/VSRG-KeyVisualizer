// src/state.rs
use std::collections::HashSet;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use crate::{
    AppConfig, BarNote, SettingsWindow, KeyCaptureDialog,
    create_model_with_selection,
};
use crate::physics::MovementPipeline;

/// 🌟 UI 意图枚举 —— 整个配置窗口所有用户操作的统一描述
pub enum UIAction {
    /// 点击画布坐标进行 hit-test 选中
    HitTestAndSelect {
        canvas_x: i32,
        canvas_y: i32,
        ctrl: bool,
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
    /// 批量修改按键宽度
    BatchUpdateWidth {
        index: i32,
        value: i32,
    },
    /// 批量修改按键高度
    BatchUpdateHeight {
        index: i32,
        value: i32,
    },
    /// 批量修改按键颜色
    BatchUpdateColor {
        index: i32,
        color: String,
    },
    /// 批量修改按键透明度
    BatchUpdateOpacity {
        index: i32,
        pct: i32,
    },
    /// 批量修改瀑布流宽度百分比
    BatchUpdateBarWidthPercent {
        index: i32,
        pct: i32,
    },
}

pub struct AppState {
    pub config: Arc<Mutex<AppConfig>>,
    pub temp_config: Arc<Mutex<AppConfig>>,
    pub active_notes: Arc<Mutex<Vec<BarNote>>>,
    pub capture_mode: Arc<AtomicBool>,
    /// 按键位置缓存: rdev_key_name → (x, y)，每帧热路径 O(1) 查找
    pub key_positions: Arc<Mutex<Vec<(String, i32, i32)>>>,
    /// 脏标记：notes 是否有变化需要重建 UI 模型
    pub notes_dirty: Arc<AtomicBool>,
    pub dialog_holder: Arc<Mutex<Option<slint::Weak<KeyCaptureDialog>>>>,
    pub settings_holder: Arc<Mutex<Option<slint::Weak<SettingsWindow>>>>,
    /// 拖拽偏移：鼠标点击位置到按键左上角的偏移 (px)
    pub drag_offset: Arc<Mutex<(i32, i32)>>,
    /// Ctrl 多选集合（存储按键索引）
    pub selected_indices: Arc<Mutex<HashSet<usize>>>,
}

impl AppState {
    pub fn new(init_config: AppConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(init_config)),
            temp_config: Arc::new(Mutex::new(AppConfig::default())),
            active_notes: Arc::new(Mutex::new(Vec::new())),
            capture_mode: Arc::new(AtomicBool::new(false)),
            key_positions: Arc::new(Mutex::new(Vec::new())),
            notes_dirty: Arc::new(AtomicBool::new(true)),
            dialog_holder: Arc::new(Mutex::new(None)),
            settings_holder: Arc::new(Mutex::new(None)),
            drag_offset: Arc::new(Mutex::new((0, 0))),
            selected_indices: Arc::new(Mutex::new(HashSet::new())),
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
            key_positions: Arc::clone(&self.key_positions),
            notes_dirty: Arc::clone(&self.notes_dirty),
            dialog_holder: Arc::clone(&self.dialog_holder),
            settings_holder: Arc::clone(&self.settings_holder),
            drag_offset: Arc::clone(&self.drag_offset),
            selected_indices: Arc::clone(&self.selected_indices),
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
            UIAction::HitTestAndSelect { canvas_x, canvas_y, ctrl } => {
                // 1. 遍历所有按键做 AABB hit-test
                let margin = tmp.key_margin_width;
                let hit_idx = tmp.keys.iter().position(|k| {
                    canvas_x >= k.x - margin
                        && canvas_x <= k.x + k.width + margin
                        && canvas_y >= k.y - margin
                        && canvas_y <= k.y + k.height + margin
                });

                // 管理多选集合
                let mut sel = self.selected_indices.lock().unwrap();

                if let Some(idx) = hit_idx {
                    if ctrl {
                        // Ctrl+点击：切换该按键的选中状态
                        if sel.contains(&idx) {
                            sel.remove(&idx);
                        } else {
                            sel.insert(idx);
                        }
                    } else {
                        // 非 Ctrl：清空多选，仅选中当前按键
                        sel.clear();
                        sel.insert(idx);
                    }

                    // 用选中的最后一个（或当前点击的）填充右侧属性面板
                    let focus_idx = idx;
                    ui.set_selected_index(focus_idx as i32);
                    ui.set_current_x(tmp.keys[focus_idx].x);
                    ui.set_current_y(tmp.keys[focus_idx].y);
                    ui.set_current_w(tmp.keys[focus_idx].width);
                    ui.set_current_h(tmp.keys[focus_idx].height);
                    let (rgb, pct) = crate::split_alpha(&tmp.keys[focus_idx].color_pressed);
                    ui.set_current_color(rgb.into());
                    ui.set_current_opacity_percent(pct);
                    ui.set_current_bar_width_percent(tmp.keys[focus_idx].bar_width_percent);

                    // 计算并保存拖拽偏移 = 点击坐标 - 按键左上角
                    let off_x = canvas_x - tmp.keys[focus_idx].x;
                    let off_y = canvas_y - tmp.keys[focus_idx].y;
                    *self.drag_offset.lock().unwrap() = (off_x, off_y);

                    // 回刷预览画布（含多选高亮）
                    let model = create_model_with_selection(&tmp.keys, &sel);
                    ui.set_root_preview_keys(model);
                } else {
                    // 点击空白区域 -> 取消所有选中
                    sel.clear();
                    ui.set_selected_index(-1);
                    let model = create_model_with_selection(&tmp.keys, &sel);
                    ui.set_root_preview_keys(model);
                }
            }

            UIAction::DragKeyOnCanvas { index, mouse_x, mouse_y, canvas_w, canvas_h } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }

                // 获取多选集合作为物理管线的跳过集（选中的按键之间不互斥）
                let skip = self.selected_indices.lock().unwrap().clone();
                // 锁已通过 clone 释放，skip 可安全使用

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

                // 穿过物理过滤器流（跳过所有选中按键之间的碰撞/吸附）
                let (real_x, real_y) = pipeline.transform_position(idx, target_x, target_y, &tmp.keys, true, &skip);

                // 计算移动增量
                let dx = real_x - tmp.keys[idx].x;
                let dy = real_y - tmp.keys[idx].y;

                // 更新核心状态（当前拖拽的按键）
                tmp.keys[idx].x = real_x;
                tmp.keys[idx].y = real_y;

                // 同步移动所有其他选中的按键（使用相同的增量，同样跳过选中集）
                for &si in skip.iter() {
                    if si != idx && si < tmp.keys.len() {
                        let (sx, sy) = pipeline.transform_position(si, tmp.keys[si].x + dx, tmp.keys[si].y + dy, &tmp.keys, true, &skip);
                        tmp.keys[si].x = sx;
                        tmp.keys[si].y = sy;
                    }
                }

                // 触发数据回刷闭环（使用含多选的模型）
                let model = create_model_with_selection(&tmp.keys, &skip);
                ui.set_root_preview_keys(model);
                ui.set_current_x(real_x);
                ui.set_current_y(real_y);
            }

            UIAction::SpinBoxUpdateX { index, value, canvas_w, canvas_h } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }

                let skip = self.selected_indices.lock().unwrap().clone();

                let pipeline = MovementPipeline {
                    canvas_w,
                    canvas_h,
                    margin: tmp.key_margin_width,
                };

                // 穿过物理过滤器（手动 SpinBox 调整，关闭磁性吸附过滤）
                let (real_x, _real_y) = pipeline.transform_position(idx, value, tmp.keys[idx].y, &tmp.keys, false, &skip);

                let dx = real_x - tmp.keys[idx].x;
                tmp.keys[idx].x = real_x;

                // 同步移动所有选中的按键（仅 X）
                for &si in skip.iter() {
                    if si != idx && si < tmp.keys.len() {
                        let (sx, _) = pipeline.transform_position(si, tmp.keys[si].x + dx, tmp.keys[si].y, &tmp.keys, false, &skip);
                        tmp.keys[si].x = sx;
                    }
                }

                let model = create_model_with_selection(&tmp.keys, &skip);
                ui.set_root_preview_keys(model);
                ui.set_current_x(real_x);
            }

            UIAction::SpinBoxUpdateY { index, value, canvas_w, canvas_h } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }

                let skip = self.selected_indices.lock().unwrap().clone();

                let pipeline = MovementPipeline {
                    canvas_w,
                    canvas_h,
                    margin: tmp.key_margin_width,
                };

                let (_real_x, real_y) = pipeline.transform_position(idx, tmp.keys[idx].x, value, &tmp.keys, false, &skip);

                let dy = real_y - tmp.keys[idx].y;
                tmp.keys[idx].y = real_y;

                // 同步移动所有选中的按键（仅 Y）
                for &si in skip.iter() {
                    if si != idx && si < tmp.keys.len() {
                        let (_, sy) = pipeline.transform_position(si, tmp.keys[si].x, tmp.keys[si].y + dy, &tmp.keys, false, &skip);
                        tmp.keys[si].y = sy;
                    }
                }

                let model = create_model_with_selection(&tmp.keys, &skip);
                ui.set_root_preview_keys(model);
                ui.set_current_y(real_y);
            }

            // ===== 批量编辑：对所有选中的按键应用同一修改 =====
            UIAction::BatchUpdateWidth { index, value } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }
                tmp.keys[idx].width = value;

                let sel = self.selected_indices.lock().unwrap().clone();
                for &si in sel.iter() {
                    if si < tmp.keys.len() {
                        tmp.keys[si].width = value;
                    }
                }
                let model = create_model_with_selection(&tmp.keys, &sel);
                ui.set_root_preview_keys(model);
                ui.set_current_w(value);
            }

            UIAction::BatchUpdateHeight { index, value } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }
                tmp.keys[idx].height = value;

                let sel = self.selected_indices.lock().unwrap().clone();
                for &si in sel.iter() {
                    if si < tmp.keys.len() {
                        tmp.keys[si].height = value;
                    }
                }
                let model = create_model_with_selection(&tmp.keys, &sel);
                ui.set_root_preview_keys(model);
                ui.set_current_h(value);
            }

            UIAction::BatchUpdateColor { index, color } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }
                let (_, old_pct) = crate::split_alpha(&tmp.keys[idx].color_pressed);
                tmp.keys[idx].color_pressed = crate::merge_alpha(&color, old_pct);

                let sel = self.selected_indices.lock().unwrap().clone();
                for &si in sel.iter() {
                    if si < tmp.keys.len() {
                        let (_, p) = crate::split_alpha(&tmp.keys[si].color_pressed);
                        tmp.keys[si].color_pressed = crate::merge_alpha(&color, p);
                    }
                }
                let model = create_model_with_selection(&tmp.keys, &sel);
                ui.set_root_preview_keys(model);
                ui.set_current_color(color.into());
            }

            UIAction::BatchUpdateOpacity { index, pct } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }
                let (rgb, _) = crate::split_alpha(&tmp.keys[idx].color_pressed);
                tmp.keys[idx].color_pressed = crate::merge_alpha(&rgb, pct);

                let sel = self.selected_indices.lock().unwrap().clone();
                for &si in sel.iter() {
                    if si < tmp.keys.len() {
                        let (r, _) = crate::split_alpha(&tmp.keys[si].color_pressed);
                        tmp.keys[si].color_pressed = crate::merge_alpha(&r, pct);
                    }
                }
                let model = create_model_with_selection(&tmp.keys, &sel);
                ui.set_root_preview_keys(model);
                ui.set_current_opacity_percent(pct);
            }

            UIAction::BatchUpdateBarWidthPercent { index, pct } => {
                let idx = index as usize;
                if idx >= tmp.keys.len() { return; }
                tmp.keys[idx].bar_width_percent = pct;

                let sel = self.selected_indices.lock().unwrap().clone();
                for &si in sel.iter() {
                    if si < tmp.keys.len() {
                        tmp.keys[si].bar_width_percent = pct;
                    }
                }
                let model = create_model_with_selection(&tmp.keys, &sel);
                ui.set_root_preview_keys(model);
                ui.set_current_bar_width_percent(pct);
            }
        }
    }

}