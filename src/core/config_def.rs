//! 配置数据结构定义
//!
//! 包含 AppConfig（全局配置）、KeyConfig（单个按键配置）以及 BarNote（瀑布流音符）、
//! MyKeyEvent（键盘事件枚举）等核心数据结构的定义与默认值。

use serde::{Deserialize, Serialize};

// ==================== 默认值常量 ====================

const DEFAULT_BORDER_WIDTH: i32 = 1;
const DEFAULT_BORDER_COLOR: &str = "#555555";
const DEFAULT_MARGIN_WIDTH: i32 = 10;
const DEFAULT_KEY_COLOR: &str = "#333333";
const DEFAULT_FLOW_DIRECTION: i32 = 0;
const DEFAULT_FLOW_SPEED: i32 = 4;
const DEFAULT_TOP_BOUNDARY: i32 = 0;

fn default_top_boundary() -> i32 {
    DEFAULT_TOP_BOUNDARY
}
fn default_border_width() -> i32 {
    DEFAULT_BORDER_WIDTH
}
fn default_border_color() -> String {
    DEFAULT_BORDER_COLOR.into()
}
fn default_margin_width() -> i32 {
    DEFAULT_MARGIN_WIDTH
}
fn default_key_color() -> String {
    DEFAULT_KEY_COLOR.into()
}
fn default_flow_direction() -> i32 {
    DEFAULT_FLOW_DIRECTION
}
fn default_flow_speed() -> i32 {
    DEFAULT_FLOW_SPEED
}
fn hundred() -> i32 {
    100
}

// ==================== 数据结构 ====================

/// 瀑布流音符
#[derive(Clone, Debug)]
pub struct BarNote {
    pub rdev_key_name: String,
    pub x: i32,
    pub width: i32,
    pub y: i32,
    pub height: i32,
    pub color: String,
    pub is_growing: bool,
    pub vel_x: i32, // 瀑布流方向 X 速度
    pub vel_y: i32, // 瀑布流方向 Y 速度
}

/// 键盘事件枚举
#[derive(Debug, Clone)]
pub enum MyKeyEvent {
    Press { rdev_name: String },
    Release { rdev_name: String },
}

/// 单个按键配置
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KeyConfig {
    pub rdev_key_name: String,
    pub display_name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub color_pressed: String,
    #[serde(default = "hundred")]
    pub bar_width_percent: i32,
}

/// 全局应用配置
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    #[serde(default = "default_top_boundary")]
    pub top_boundary: i32,

    #[serde(default = "default_border_width")]
    pub global_border_width: i32,
    #[serde(default = "default_border_color")]
    pub global_border_color: String,
    #[serde(default = "default_margin_width")]
    pub key_margin_width: i32,
    #[serde(default = "default_key_color")]
    pub global_key_color: String,
    #[serde(default = "default_flow_direction")]
    pub flow_direction: i32,

    #[serde(default = "default_flow_speed")]
    pub flow_speed: i32,

    #[serde(default)]
    pub front_line_emit: bool,

    #[serde(default)]
    pub window_x: Option<i32>,
    #[serde(default)]
    pub window_y: Option<i32>,

    pub keys: Vec<KeyConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            top_boundary: DEFAULT_TOP_BOUNDARY,
            global_border_width: DEFAULT_BORDER_WIDTH,
            global_border_color: DEFAULT_BORDER_COLOR.into(),
            key_margin_width: DEFAULT_MARGIN_WIDTH,
            global_key_color: DEFAULT_KEY_COLOR.into(),
            flow_direction: DEFAULT_FLOW_DIRECTION,
            flow_speed: DEFAULT_FLOW_SPEED,
            front_line_emit: false,
            window_x: None,
            window_y: None,
            keys: vec![KeyConfig {
                rdev_key_name: "KeyA".into(),
                display_name: "A".into(),
                x: 10,
                y: 10,
                width: 80,
                height: 80,
                color_pressed: "#4A90E2FF".into(),
                bar_width_percent: 100,
            }],
        }
    }
}
