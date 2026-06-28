//! 窗口管理工具函数
//!
//! 包含窗口穿透、位置恢复、尺寸计算、SafeHWND 封装等功能。

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use tracing;

use crate::core::config_def::AppConfig;

/// 线程安全的 HWND 包装（HWND 本身是 `*mut c_void`，不自动实现 Send）
#[derive(Clone, Copy)]
pub struct SafeHWND(pub windows::Win32::Foundation::HWND);
unsafe impl Send for SafeHWND {}
unsafe impl Sync for SafeHWND {}

/// 存储主窗口 HWND，供拖动回调使用
pub static MAIN_HWND: std::sync::Mutex<Option<SafeHWND>> = std::sync::Mutex::new(None);

#[cfg(windows)]
pub fn make_window_clickthrough(window: &winit::window::Window) {
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongW, HWND_TOPMOST, LWA_COLORKEY, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SetLayeredWindowAttributes, SetWindowLongW, SetWindowPos,
        WS_EX_LAYERED,
    };

    let hwnd = match window.window_handle().unwrap().as_raw() {
        RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut c_void),
        _ => return,
    };

    unsafe {
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);

        // 1. 添加分层样式（LWA_COLORKEY 需要 WS_EX_LAYERED）
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);

        // 2. 激活颜色键穿透（黑色像素 → 透明 → 点击穿透到下层窗口）
        if SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY).is_err() {
            tracing::error!("SetLayeredWindowAttributes 失败");
        }

        // 3. ⚠️ 关键：SetWindowPos 刷新窗口非客户区，使 WS_EX_LAYERED 生效
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }

    tracing::debug!("[clickthrough] 主窗口 HWND 已存储");
    *MAIN_HWND.lock().unwrap_or_else(|e| e.into_inner()) = Some(SafeHWND(hwnd));
}

/// 根据按键布局计算窗口尺寸
pub fn calculate_window_size(config: &AppConfig) -> (i32, i32) {
    let (width, height) = if config.keys.is_empty() {
        (1200, 500)
    } else {
        let max_right = config
            .keys
            .iter()
            .map(|k| k.x + k.width)
            .max()
            .unwrap_or(0);
        let max_bottom = config
            .keys
            .iter()
            .map(|k| k.y + k.height)
            .max()
            .unwrap_or(0);

        // 基础尺寸：按键布局占用的最小矩形
        let base_w = max_right + config.key_margin_width;
        let base_h = max_bottom + config.key_margin_width;

        match config.flow_direction {
            // ↓ 向下：高度增加 top_boundary 用于音符向下流动
            0 => (base_w, base_h + config.top_boundary),
            // ↑ 向上：高度增加 top_boundary 用于音符向上流动
            1 => (base_w, base_h + config.top_boundary),
            // ← 向左：宽度增加 top_boundary 用于音符向左流动
            2 => (base_w + config.top_boundary, base_h),
            // → 向右：宽度增加 top_boundary 用于音符向右流动
            3 => (base_w + config.top_boundary, base_h),
            _ => (base_w, base_h + config.top_boundary),
        }
    };

    (width, height)
}

/// 获取窗口尺寸（u32 版本，用于 winit API）
pub fn get_window_size(cfg: &AppConfig) -> (u32, u32) {
    let (w, h) = calculate_window_size(cfg);
    (w as u32, h as u32)
}

/// 检查窗口中心点是否在主显示器的可见范围内
/// 如果中心点超出屏幕范围，返回 true（需要重置位置）
fn is_window_center_outside(
    win_x: i32,
    win_y: i32,
    win_w: u32,
    win_h: u32,
    screen_w: u32,
    screen_h: u32,
) -> bool {
    let cx = win_x + (win_w / 2) as i32;
    let cy = win_y + (win_h / 2) as i32;
    cx < 0 || cy < 0 || cx as u32 > screen_w || cy as u32 > screen_h
}

/// 检查窗口中心点是否在任意显示器的可见范围内
/// 如果所有显示器都不包含中心点，返回 true（需要重置位置）
fn is_window_center_outside_all_monitors(
    win_x: i32,
    win_y: i32,
    win_w: u32,
    win_h: u32,
    monitors: &[(u32, u32)],
) -> bool {
    if monitors.is_empty() {
        return true;
    }
    !monitors.iter().any(|&(sw, sh)| {
        !is_window_center_outside(win_x, win_y, win_w, win_h, sw, sh)
    })
}

/// 从 AppConfig 恢复窗口位置（支持多显示器）
/// 规则：计算窗口中心点，如果中心点超出任意显示器范围则重置居中
pub fn restore_window_position(winit_window: &winit::window::Window, cfg: &AppConfig) {
    let (saved_x, saved_y) = match (cfg.window_x, cfg.window_y) {
        (Some(x), Some(y)) => (x, y),
        _ => {
            // 无保存记录，居中显示
            if let Some(monitor) = winit_window.primary_monitor() {
                let size = monitor.size();
                let win_size = calculate_window_size(cfg);
                let cx = (size.width.saturating_sub(win_size.0 as u32) / 2) as i32;
                let cy = (size.height.saturating_sub(win_size.1 as u32) / 2) as i32;
                winit_window.set_outer_position(winit::dpi::PhysicalPosition::new(cx, cy));
            }
            return;
        }
    };

    // 计算窗口尺寸
    let (win_w, win_h) = get_window_size(cfg);

    // 收集所有显示器的尺寸
    let monitors: Vec<(u32, u32)> = winit_window
        .available_monitors()
        .map(|m| {
            let size = m.size();
            (size.width, size.height)
        })
        .collect();

    if is_window_center_outside_all_monitors(saved_x, saved_y, win_w, win_h, &monitors) {
        // 中心点超出所有显示器，重置到主显示器居中
        if let Some(monitor) = winit_window.primary_monitor() {
            let size = monitor.size();
            let cx = (size.width.saturating_sub(win_w) / 2) as i32;
            let cy = (size.height.saturating_sub(win_h) / 2) as i32;
            winit_window.set_outer_position(winit::dpi::PhysicalPosition::new(cx, cy));
        }
    } else {
        // 中心点在某个显示器内 → 直接恢复位置
        winit_window.set_outer_position(winit::dpi::PhysicalPosition::new(saved_x, saved_y));
    }
}
