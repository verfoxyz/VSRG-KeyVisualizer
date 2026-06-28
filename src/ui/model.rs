//! Slint 模型工具函数
//!
//! 提供 ToKeyData trait、create_model、compute_key_ratios、update_key_visual_state 等
//! 用于将 Rust 数据结构转换为 Slint KeyData 模型的通用工具。

use slint::{Model, ModelRc, VecModel};
use std::collections::HashMap;
use std::rc::Rc;

use crate::core::color::hex_str_to_color;
use crate::core::config_def::{BarNote, KeyConfig};
use crate::KeyData;

/// 统一转换 trait：将不同类型转换为 KeyData
pub trait ToKeyData {
    fn to_key_data(&self) -> KeyData;
}

impl ToKeyData for BarNote {
    fn to_key_data(&self) -> KeyData {
        KeyData {
            name: self.rdev_key_name.clone().into(),
            display_name: "".into(),
            is_pressed: false,
            x: self.x as f32,
            y: self.y as f32,
            w: self.width as f32,
            h: self.height as f32,
            anchor_ratio_x: 0.0,
            anchor_ratio_y: 0.0,
            pressed_color: hex_str_to_color(&self.color),
            color_hex: self.color.clone().into(),
            selected: false,
        }
    }
}

impl ToKeyData for KeyConfig {
    fn to_key_data(&self) -> KeyData {
        KeyData {
            name: self.rdev_key_name.clone().into(),
            display_name: self.display_name.clone().into(),
            is_pressed: false,
            x: self.x as f32,
            y: self.y as f32,
            w: self.width as f32,
            h: self.height as f32,
            anchor_ratio_x: 0.0,
            anchor_ratio_y: 0.0,
            pressed_color: hex_str_to_color(&self.color_pressed),
            color_hex: self.color_pressed.clone().into(),
            selected: false,
        }
    }
}

/// 按键专用：为 KeyData 模型计算比例锚点（相对于画布宽高）
pub fn compute_key_ratios(model: &ModelRc<KeyData>, canvas_w: f32, canvas_h: f32) {
    let cw = canvas_w.max(1.0);
    let ch = canvas_h.max(1.0);
    for i in 0..model.row_count() {
        let mut d = model.row_data(i).unwrap();
        d.anchor_ratio_x = d.x / cw;
        d.anchor_ratio_y = d.y / ch;
        model.set_row_data(i, d);
    }
}

/// 通用渲染函数：将任意实现了 `ToKeyData` 的切片转换为 `ModelRc<KeyData>`
pub fn create_model<T: ToKeyData>(items: &[T]) -> ModelRc<KeyData> {
    let data: Vec<KeyData> = items.iter().map(|i| i.to_key_data()).collect();
    Rc::new(VecModel::from(data)).into()
}

/// 带多选高亮的渲染函数：根据 selected_indices 设置每个 KeyData 的 selected 字段
pub fn create_model_with_selection<T: ToKeyData>(
    items: &[T],
    selected: &std::collections::HashSet<usize>,
) -> ModelRc<KeyData> {
    let data: Vec<KeyData> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut kd = item.to_key_data();
            kd.selected = selected.contains(&i);
            kd
        })
        .collect();
    Rc::new(VecModel::from(data)).into()
}

/// 按键索引缓存：rdev_key_name → 模型索引
pub type KeyIndexMap = HashMap<String, usize>;

/// 从按键配置构建按键索引映射
pub fn build_key_index_map(keys: &[KeyConfig]) -> KeyIndexMap {
    keys.iter()
        .enumerate()
        .map(|(i, k)| (k.rdev_key_name.clone(), i))
        .collect()
}

/// 更新按键视觉状态（按下/释放高亮）
/// 使用预构建的 index_map 实现 O(1) 查找
pub fn update_key_visual_state(
    ui_weak: &slint::Weak<crate::MainWindow>,
    key_name: &str,
    is_pressed: bool,
    index_map: &KeyIndexMap,
) {
    if let Some(ui) = ui_weak.upgrade()
        && let Some(&idx) = index_map.get(key_name) {
            let model = ui.get_keys();
            if let Some(mut data) = model.row_data(idx) {
                data.is_pressed = is_pressed;
                model.set_row_data(idx, data);
            }
        }
}
