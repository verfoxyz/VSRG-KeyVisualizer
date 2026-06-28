// src/gui/param_panel_window.rs
use crate::core::color::split_alpha;
use crate::ui::state::{AppState, UIAction};
use crate::{ParamPanelWindow, SettingsWindow};
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing;

/// 存储 ParamPanelWindow 的 HWND，用于窗口拖动
#[cfg(windows)]
static PARAM_PANEL_HWND: std::sync::Mutex<Option<crate::SafeHWND>> = std::sync::Mutex::new(None);
/// 存储 SettingsWindow 的 HWND，用于后台线程轮询实时跟随
#[cfg(windows)]
static SETTINGS_HWND: std::sync::Mutex<Option<crate::SafeHWND>> = std::sync::Mutex::new(None);

/// 吸附距离（像素）— 面板距设置窗口右侧多少像素内触发重新吸附
const SNAP_DISTANCE: i32 = 30;
/// 吸附后跟随时的间距
const SNAP_GAP: i32 = 4;
/// 吸附状态：true = 吸附并跟随设置窗口；false = 独立移动
static SNAPPED: AtomicBool = AtomicBool::new(true);
/// 上一帧设置窗口的 X 坐标（用于区分"设置窗口移动"vs"用户拖动面板"）
static PREV_SETTINGS_X: std::sync::Mutex<Option<i32>> = std::sync::Mutex::new(None);
/// 拖拽状态
static DRAG_STATE: std::sync::Mutex<Option<DragInfo>> = std::sync::Mutex::new(None);

struct DragInfo {
    offset_x: i32,      // 按下时光标屏幕 x - 窗口 x (恒定偏移)
    offset_y: i32,      // 按下时光标屏幕 y - 窗口 y
    click_ofs_x: i32,   // 光标在窗口内的相对 x（用于吸附时检测光标距离）
    click_ofs_y: i32,   // 光标在窗口内的相对 y
    started: bool,      // 是否已超过阈值正式开始拖动
}

/// 获取光标屏幕坐标（Win32 GetCursorPos）
#[cfg(windows)]
fn get_cursor_pos() -> (i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    unsafe {
        let mut pt: windows::Win32::Foundation::POINT = std::mem::zeroed();
        if GetCursorPos(&mut pt).is_ok() {
            (pt.x, pt.y)
        } else {
            (0, 0)
        }
    }
}

/// 后台轮询线程活跃标志（面板关闭时置 false，线程自行退出）
static POLLING_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 配置参数面板独立窗口
pub fn setup_param_panel_window(
    panel: ParamPanelWindow,
    state: AppState,
    settings_weak: slint::Weak<SettingsWindow>,
) {
    use i_slint_backend_winit::WinitWindowAccessor;

    tracing::debug!("[PARAM-PANEL] setup_param_panel_window: showing panel...");
    // 重置吸附状态（跨面板生命周期）
    SNAPPED.store(true, Ordering::Relaxed);
    panel.show().unwrap();
    tracing::debug!("[PARAM-PANEL] panel.show() OK");

    // ===== 1. 绑定窗口拖拽（纯状态机：drag_begin 固定 offset，drag_move 不重算） =====
    {
        let panel_weak = panel.as_weak();
        let s_weak = settings_weak.clone();

        panel.on_drag_begin(move |_mx, _my| {
            let p = match panel_weak.upgrade() {
                Some(p) => p,
                None => return,
            };
            let pos = p.window().position();
            let (sx, sy) = get_cursor_pos();
            let ofs_x = sx - pos.x;
            let ofs_y = sy - pos.y;
            let mut state = DRAG_STATE.lock().unwrap();
            *state = Some(DragInfo {
                offset_x: ofs_x,
                offset_y: ofs_y,
                click_ofs_x: ofs_x,
                click_ofs_y: ofs_y,
                started: false,
            });
            tracing::debug!(
                "[PARAM-PANEL] drag_begin: offset=({},{}), window=({},{}), cursor=({},{})",
                ofs_x, ofs_y, pos.x, pos.y, sx, sy
            );
        });

        let panel_weak2 = panel.as_weak();
        let s_weak2 = settings_weak.clone();
        panel.on_drag_move(move |_mx, _my| {
            let p = match panel_weak2.upgrade() {
                Some(p) => p,
                None => return,
            };
            let mut state = DRAG_STATE.lock().unwrap();
            let info = match state.as_mut() {
                Some(info) => info,
                None => return,
            };
            let (sx, sy) = get_cursor_pos();

            // ═══ 1. 虚拟位置：始终跟随鼠标（不漂移） ═══
            let virtual_x = sx - info.offset_x;
            let virtual_y = sy - info.offset_y;

            // ═══ 2. 判断虚拟位置是否进入吸附范围 ═══
            let mut snap_pos = None;
            if let Some(s) = s_weak2.upgrade() {
                let sw = s.window().position();
                let sw_w = s.window().size().width as i32;
                let sx_p = sw.x + sw_w + SNAP_GAP;
                let sy_p = sw.y;
                if (virtual_x - sx_p).abs().max((virtual_y - sy_p).abs()) <= SNAP_DISTANCE {
                    snap_pos = Some((sx_p, sy_p));
                }
            }

            // ═══ 3. 根据状态设置窗口位置 ═══
            if let Some((sx_p, sy_p)) = snap_pos {
                SNAPPED.store(true, Ordering::Relaxed);
                p.window().set_position(slint::PhysicalPosition::new(sx_p, sy_p));
            } else {
                if SNAPPED.load(Ordering::Relaxed) {
                    tracing::debug!(
                        "[PARAM-PANEL] unsnapped: virtual=({},{}), exceeded snap range",
                        virtual_x, virtual_y
                    );
                }
                SNAPPED.store(false, Ordering::Relaxed);
                p.window().set_position(slint::PhysicalPosition::new(virtual_x, virtual_y));
            }
            info.started = true;
        });

        panel.on_drag_end(move || {
            let mut state = DRAG_STATE.lock().unwrap();
            *state = None;
            tracing::debug!("[PARAM-PANEL] drag_end");
        });

        tracing::debug!("[PARAM-PANEL] drag callbacks bound (fixed-offset mode)");
    }

    // ===== 2. 获取 Win32 HWND 并设置窗口子类化实现实时跟随 =====
    {
        tracing::debug!("[PARAM-PANEL] spawning async HWND acquisition...");
        let panel_weak = panel.as_weak();
        let s_weak = settings_weak.clone();
        slint::spawn_local(async move {
            tracing::debug!("[PARAM-PANEL] async HWND acquisition started");
            let p = match panel_weak.upgrade() {
                Some(p) => p,
                None => {
                    tracing::warn!("[PARAM-PANEL] async HWND: panel_weak expired");
                    return;
                }
            };

            let p_win = match p.window().winit_window().await {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!("[PARAM-PANEL] panel winit_window() failed: {:?}", e);
                    return;
                }
            };

            tracing::debug!("[PARAM-PANEL] panel winit window acquired");

            #[cfg(windows)]
            {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                use windows::Win32::Foundation::HWND;
                use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SetWindowPos, SWP_NOSIZE, SWP_NOACTIVATE, SWP_NOZORDER};

                // 获取面板 HWND
                if let Ok(handle) = p_win.window_handle() {
                    if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                        let hwnd = HWND(win32_handle.hwnd.get() as *mut std::ffi::c_void);
                        *PARAM_PANEL_HWND.lock().unwrap() = Some(crate::SafeHWND(hwnd));
                        tracing::debug!("[PARAM-PANEL] panel HWND stored for window drag");
                    }
                }

                // 获取设置窗口 HWND
                if let Some(s) = s_weak.upgrade() {
                    if let Ok(s_win) = s.window().winit_window().await {
                        if let Ok(handle) = s_win.window_handle() {
                            if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                                let s_hwnd = HWND(win32_handle.hwnd.get() as *mut std::ffi::c_void);
                                *SETTINGS_HWND.lock().unwrap() = Some(crate::SafeHWND(s_hwnd));
                                tracing::debug!("[PARAM-PANEL] settings HWND stored");
                            }
                        }
                    }
                }

                // ⭐ 启动后台轮询线程：在 Win32 拖拽模态循环期间实时跟随
                let Some(panel_safe) = *PARAM_PANEL_HWND.lock().unwrap() else { return; };
                let Some(settings_safe) = *SETTINGS_HWND.lock().unwrap() else { return; };
                {
                    POLLING_ACTIVE.store(true, Ordering::Relaxed);
                    // 用 raw isize 跨线程传 HWND，避免 HWND/*mut c_void 非 Send
                    let p_raw = panel_safe.0 .0 as isize;
                    let s_raw = settings_safe.0 .0 as isize;
                    std::thread::spawn(move || {
                        let mut prev_settings_left: i32 = i32::MIN;
                        loop {
                            if !POLLING_ACTIVE.load(Ordering::Relaxed) { break; }

                            unsafe {
                                let p_hwnd = windows::Win32::Foundation::HWND(p_raw as *mut std::ffi::c_void);
                                let s_hwnd = windows::Win32::Foundation::HWND(s_raw as *mut std::ffi::c_void);
                                let mut s_rect: windows::Win32::Foundation::RECT = std::mem::zeroed();
                                let mut p_rect: windows::Win32::Foundation::RECT = std::mem::zeroed();
                                if GetWindowRect(s_hwnd, &mut s_rect).is_err()
                                    || GetWindowRect(p_hwnd, &mut p_rect).is_err()
                                {
                                    std::thread::sleep(std::time::Duration::from_millis(16));
                                    continue;
                                }

                                // 用户活跃拖拽中 → 跳过位置同步（由 drag_move 负责）
                                if DRAG_STATE.lock().unwrap().is_some() {
                                    std::thread::sleep(std::time::Duration::from_millis(16));
                                    continue;
                                }

                                if !SNAPPED.load(Ordering::Relaxed) {
                                    std::thread::sleep(std::time::Duration::from_millis(50));
                                    continue;
                                }

                                let sw_w = s_rect.right - s_rect.left;
                                let target_x = s_rect.left + sw_w + SNAP_GAP as i32;
                                let target_y = s_rect.top;
                                let panel_dx = p_rect.left - target_x;

                                // 检测活跃拖拽：面板远离目标(> SNAP_DISTANCE)但设置窗口没动
                                if prev_settings_left != i32::MIN {
                                    let settings_dx = (s_rect.left - prev_settings_left).abs();
                                    if panel_dx.abs() > SNAP_DISTANCE && settings_dx < 5 {
                                        SNAPPED.store(false, Ordering::Relaxed);
                                        tracing::debug!(
                                            "[PARAM-PANEL] background: drag detected, unsnapped"
                                        );
                                        prev_settings_left = s_rect.left;
                                        std::thread::sleep(std::time::Duration::from_millis(50));
                                        continue;
                                    }
                                }
                                prev_settings_left = s_rect.left;

                                // 位置同步
                                if panel_dx != 0 || p_rect.top != target_y {
                                    let _ = SetWindowPos(
                                        p_hwnd,
                                        None,
                                        target_x,
                                        target_y,
                                        0,
                                        0,
                                        SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
                                    );
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_millis(16));
                        }
                        tracing::debug!("[PARAM-PANEL] follow polling thread exited");
                    });
                    tracing::debug!("[PARAM-PANEL] background follow polling thread started (16ms)");
                }
            }
        })
        .unwrap();
        tracing::debug!("[PARAM-PANEL] spawn_local queued");
    }

    // ===== 3. 放置面板窗口到设置窗口右侧 =====
    {
        let panel_weak = panel.as_weak();
        let s_weak = settings_weak.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
            tracing::debug!("[PARAM-PANEL] initial position timer fired");
            if let (Some(p), Some(s)) = (panel_weak.upgrade(), s_weak.upgrade()) {
                let sw_pos = s.window().position();
                let sw_size = s.window().size();
                let target_x = sw_pos.x + sw_size.width as i32 + SNAP_GAP;
                let target_y = sw_pos.y;
                p.window().set_position(slint::PhysicalPosition::new(target_x, target_y));
                tracing::debug!("[PARAM-PANEL] initial position set to ({}, {})", target_x, target_y);
            } else {
                tracing::warn!("[PARAM-PANEL] initial position: weak refs expired");
            }
        });
    }

    // ===== 4. 定期同步：属性同步 + 吸附跟随 =====
    {
        let panel_weak = panel.as_weak();
        let s_weak = settings_weak.clone();
        let follow_timer = Box::new(slint::Timer::default());
        follow_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(30),
            move || {
                let (p, s) = match (panel_weak.upgrade(), s_weak.upgrade()) {
                    (Some(p), Some(s)) => (p, s),
                    _ => return,
                };

                // 跳过不可见窗口
                if !p.window().is_visible() || !s.window().is_visible() {
                    return;
                }

                // === 属性同步 ===
                p.set_selected_index(s.get_selected_index());
                p.set_current_x(s.get_current_x());
                p.set_current_y(s.get_current_y());
                p.set_current_w(s.get_current_w());
                p.set_current_h(s.get_current_h());
                p.set_current_color(s.get_current_color());
                p.set_current_opacity_percent(s.get_current_opacity_percent());
                p.set_current_bar_width_percent(s.get_current_bar_width_percent());
                p.set_global_key_color_hex(s.get_global_key_color_hex());
                p.set_global_key_opacity_percent(s.get_global_key_opacity_percent());
                p.set_global_border_color_hex(s.get_global_border_color_hex());
                p.set_front_line_emit(s.get_front_line_emit());
                p.set_flow_direction(s.get_flow_direction());
                p.set_flow_speed(s.get_flow_speed());
                p.set_global_top_boundary(s.get_global_top_boundary());
                p.set_key_margin_width(s.get_key_margin_width());

                // === SNAPPED 状态管理 + OS 拖拽检测（不移动位置，由后台线程负责） ===
                let sw_pos = s.window().position();
                let sw_size = s.window().size();
                let sw_right_edge = sw_pos.x + sw_size.width as i32;
                let pp_pos = p.window().position();
                let gap = pp_pos.x - sw_right_edge;
                let snapped = SNAPPED.load(Ordering::Relaxed);

                // 更新跟踪（所有分支都需要）
                let prev_sw_x = PREV_SETTINGS_X.lock().unwrap().replace(sw_pos.x);

                if snapped {
                    // ⭐ 检测用户是否通过 OS 标题栏拖动了面板
                    // 判断依据：gap 很大（> SNAP_DISTANCE），但设置窗口 X 坐标几乎没变
                    if gap > SNAP_DISTANCE {
                        if let Some(prev_x) = prev_sw_x {
                            let settings_dx = (sw_pos.x - prev_x).abs();
                            if settings_dx < 5 {
                                // 设置窗口没动但面板跑了 → 用户拖了面板 → 脱离吸附
                                SNAPPED.store(false, Ordering::Relaxed);
                                tracing::debug!(
                                    "[PARAM-PANEL] OS title-bar drag detected: gap={}, settings_dx={}, UNSNAPPED",
                                    gap, settings_dx
                                );
                                return;
                            }
                        }
                    }
                } else if gap >= 0 && gap <= SNAP_DISTANCE {
                    // 游离状态但接近 → 检查光标距离后决定是否重新吸附
                    // 如果吸附后光标在面板上的位置与光标实际位置差距过大则不吸附
                    let should_snap = {
                        let drag = DRAG_STATE.lock().unwrap();
                        if let Some(info) = drag.as_ref() {
                            let (cx, cy) = get_cursor_pos();
                            // 吸附后光标会位于: snap_target + click_ofs
                            let expected_cx = sw_right_edge + SNAP_GAP + info.click_ofs_x;
                            let expected_cy = sw_pos.y + info.click_ofs_y;
                            let dist = (cx - expected_cx).abs().max((cy - expected_cy).abs());
                            dist <= SNAP_DISTANCE
                        } else {
                            // 没有活跃拖拽 → 无条件吸附
                            true
                        }
                    };
                    if !should_snap {
                        tracing::trace!("[PARAM-PANEL] skip re-snap: cursor too far from expected snap position");
                        return;
                    }
                    SNAPPED.store(true, Ordering::Relaxed);
                    tracing::debug!("[PARAM-PANEL] RE-SNAPPED by follow timer (gap={})", gap);
                }
            },
        );
        Box::leak(follow_timer);
        tracing::debug!("[PARAM-PANEL] follow/sync timer started (30ms)");
    }

    // ===== 5. 定期检测设置窗口是否关闭 =====
    {
        let panel_weak = panel.as_weak();
        let s_weak = settings_weak.clone();
        let holder_weak = state.param_panel_holder.clone();
        let close_check_timer = Box::new(slint::Timer::default());
        close_check_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(100),
            move || {
                // 如果面板已经不可见，静默跳过（避免隐藏后还持续输出日志）
                let panel = match panel_weak.upgrade() {
                    Some(p) => p,
                    None => return,
                };
                if !panel.window().is_visible() {
                    return;
                }

                let s = match s_weak.upgrade() {
                    Some(s) => s,
                    None => {
                        // 设置窗口已销毁 → 关闭面板并清理 holder
                        tracing::debug!("[PARAM-PANEL] close_check: settings destroyed, hiding panel");
                        panel.hide().unwrap();
                        *holder_weak.lock().unwrap() = None;
                        POLLING_ACTIVE.store(false, Ordering::Relaxed);
                        return;
                    }
                };
                // 如果设置窗口不可见了，也关闭面板
                if !s.window().is_visible() {
                    tracing::debug!("[PARAM-PANEL] close_check: settings window not visible, hiding panel");
                    panel.hide().unwrap();
                    *holder_weak.lock().unwrap() = None;
                    POLLING_ACTIVE.store(false, Ordering::Relaxed);
                    // 更新设置窗口的 panel_window_open 标记
                    s.set_panel_window_open(false);
                }
            },
        );
        Box::leak(close_check_timer);
        tracing::debug!("[PARAM-PANEL] close-check timer started (100ms)");
    }

    // ===== 6. 绑定按键属性编辑回调（通过 dispatch 回传） =====
    let state_dispatch = state.clone();
    let s_weak = settings_weak.clone();
    panel.on_update_key_position_x(move |index, val, _cw, _ch| {
        if let Some(s) = s_weak.upgrade() {
            let (cw, ch) = get_preview_size_from_settings(&s);
            state_dispatch.dispatch(
                UIAction::SpinBoxUpdateX {
                    index,
                    value: val,
                    canvas_w: cw,
                    canvas_h: ch,
                },
                &s.as_weak(),
            );
        }
    });

    let state_dispatch = state.clone();
    let s_weak = settings_weak.clone();
    panel.on_update_key_position_y(move |index, val, _cw, _ch| {
        if let Some(s) = s_weak.upgrade() {
            let (cw, ch) = get_preview_size_from_settings(&s);
            state_dispatch.dispatch(
                UIAction::SpinBoxUpdateY {
                    index,
                    value: val,
                    canvas_w: cw,
                    canvas_h: ch,
                },
                &s.as_weak(),
            );
        }
    });

    let state_dispatch = state.clone();
    let s_weak = settings_weak.clone();
    panel.on_update_key_size(move |index, w, h| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(UIAction::BatchUpdateWidth { index, value: w }, &s.as_weak());
            state_dispatch.dispatch(UIAction::BatchUpdateHeight { index, value: h }, &s.as_weak());
            s.set_current_w(w);
            s.set_current_h(h);
        }
    });

    let state_dispatch = state.clone();
    let s_weak = settings_weak.clone();
    panel.on_update_key_color(move |index, color| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(
                UIAction::BatchUpdateColor {
                    index,
                    color: color.to_string(),
                },
                &s.as_weak(),
            );
        }
    });

    let state_dispatch = state.clone();
    let s_weak = settings_weak.clone();
    panel.on_update_key_opacity(move |index, pct| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(UIAction::BatchUpdateOpacity { index, pct }, &s.as_weak());
        }
    });

    let state_dispatch = state.clone();
    let s_weak = settings_weak.clone();
    panel.on_update_key_bar_width_percent(move |index, pct| {
        if let Some(s) = s_weak.upgrade() {
            state_dispatch.dispatch(
                UIAction::BatchUpdateBarWidthPercent { index, pct },
                &s.as_weak(),
            );
        }
    });

    // ===== 7. 绑定全局设置回调 =====
    let tc = state.temp_config.clone();
    panel.on_top_boundary_edited(move |bd| tc.lock().unwrap().top_boundary = bd);
    let tc = state.temp_config.clone();
    panel.on_key_margin_edited(move |margin| tc.lock().unwrap().key_margin_width = margin);
    let tc = state.temp_config.clone();
    panel.on_border_color_edited(move |color| tc.lock().unwrap().global_border_color = color.to_string());

    let tc = state.temp_config.clone();
    panel.on_key_color_edited(move |color| {
        let mut tmp = tc.lock().unwrap();
        let (_, old_pct) = split_alpha(&tmp.global_key_color);
        tmp.global_key_color = crate::merge_alpha(&color, old_pct);
    });

    let tc = state.temp_config.clone();
    panel.on_key_opacity_edited(move |pct| {
        let mut tmp = tc.lock().unwrap();
        let (rgb, _) = split_alpha(&tmp.global_key_color);
        tmp.global_key_color = crate::merge_alpha(&rgb, pct);
    });

    let tc = state.temp_config.clone();
    panel.on_flow_direction_edited(move |dir| tc.lock().unwrap().flow_direction = dir);
    let tc = state.temp_config.clone();
    panel.on_flow_speed_edited(move |speed| tc.lock().unwrap().flow_speed = speed);
    let tc = state.temp_config.clone();
    panel.on_front_line_emit_toggled(move |val| tc.lock().unwrap().front_line_emit = val);

    // ⭐ 存储面板窗口强引用到 state（防止函数返回后 panel 被 drop 销毁）
    tracing::debug!("[PARAM-PANEL] storing panel strong ref in param_panel_holder");
    *state.param_panel_holder.lock().unwrap() = Some(panel);
    tracing::debug!("[PARAM-PANEL] setup_param_panel_window COMPLETE");
}

/// 从 SettingsWindow 获取预览画布尺寸
fn get_preview_size_from_settings(s: &SettingsWindow) -> (i32, i32) {
    // 无法直接从 Slint 外部获取组件尺寸，使用设置的窗口尺寸估算
    let win_size = s.window().size();
    // 预览区域 ≈ 窗口宽度减去左侧 profile 面板（~200px）和各种边距（~60px）
    let cw = (win_size.width as i32 - 260).max(200);
    let ch = (win_size.height as i32 - 160).max(100);
    (cw, ch)
}
