// d:\code\KeyTick\src\physics.rs
use crate::KeyConfig;

/// 固体阻挡算法 + “#”型延长边磁性捕获状态机
pub fn handle_key_movement(
    moved_index: usize,
    pure_mouse_x: i32, // 前端传来的：完全由鼠标位置减去点击位置得到的纯轨迹X
    pure_mouse_y: i32, // 完全由鼠标位置减去点击位置得到的纯轨迹Y
    margin: i32,
    keys: &mut [KeyConfig],
    canvas_w: i32,
    canvas_h: i32,
) {
    if moved_index >= keys.len() {
        return;
    }

    let key_w = keys[moved_index].width;
    let key_h = keys[moved_index].height;

    // ==========================================
    // 配置物理参数
    // ==========================================
    let snap_threshold = 6;      // 吸附触发阈值（靠近到6px内啪地吸附）
    let escape_threshold = 14;   // 脱离吸附所需的“拉扯距离”（必须拉开14px才能解除捕获）

    // 默认情况下，按键应该完全跟随鼠标的纯轨迹
    let mut target_x = pure_mouse_x;
    let mut target_y = pure_mouse_y;

    let mut x_snapped = false;
    let mut y_snapped = false;

    // ==========================================
    // 阶段 1: “#” 字延长边磁性捕获状态机
    // ==========================================
    // 遍历所有其他静态元素，检测它们的延长边
    for i in 0..keys.len() {
        if i == moved_index { continue; }

        let b_x = keys[i].x;
        let b_y = keys[i].y;
        let b_w = keys[i].width;
        let b_h = keys[i].height;

        // ------------- X 轴方向的延长线吸附检测 -------------
        if !x_snapped {
            // 可能性 1: 拖动元件的左边缘 靠近 静态元件的右边缘 (加上视觉margin)
            let line_left_to_right = b_x + b_w + margin * 2;
            let dist_left_to_right = pure_mouse_x - line_left_to_right;
            
            // 之前的实现每次循环都覆盖 target_x，导致后面的非吸附元素洗掉了前面的吸附结果
            // 这里使用过往状态锁（如果拉扯没到逃逸阈值就保持锁死，或者进入吸附范围则触发吸附）
            if dist_left_to_right.abs() <= snap_threshold {
                target_x = line_left_to_right;
                x_snapped = true;
            }

            // 可能性 2: 拖动元件的右边缘 靠近 静态元件的左边缘
            if !x_snapped {
                let line_right_to_left = b_x - key_w - margin * 2;
                let dist_right_to_left = pure_mouse_x - line_right_to_left;
                if dist_right_to_left.abs() <= snap_threshold {
                    target_x = line_right_to_left;
                    x_snapped = true;
                }
            }

            // 可能性 3: 左对齐延长线 (X 坐标相同)
            if !x_snapped {
                let dist_align_left = pure_mouse_x - b_x;
                if dist_align_left.abs() <= snap_threshold {
                    target_x = b_x;
                    x_snapped = true;
                }
            }
        }

        // ------------- Y 轴方向的延长线吸附检测 -------------
        if !y_snapped {
            // 可能性 1: 拖动元件的上边缘 靠近 静态元件的下边缘
            let line_top_to_bottom = b_y + b_h + margin * 2;
            let dist_top_to_bottom = pure_mouse_y - line_top_to_bottom;
            if dist_top_to_bottom.abs() <= snap_threshold {
                target_y = line_top_to_bottom;
                y_snapped = true;
            }

            // 可能性 2: 拖动元件的下边缘 靠近 静态元件的上边缘
            if !y_snapped {
                let line_bottom_to_top = b_y - key_h - margin * 2;
                let dist_bottom_to_top = pure_mouse_y - line_bottom_to_top;
                if dist_bottom_to_top.abs() <= snap_threshold {
                    target_y = line_bottom_to_top;
                    y_snapped = true;
                }
            }

            // 可能性 3: 上对齐延长线 (Y 坐标相同)
            if !y_snapped {
                let dist_align_top = pure_mouse_y - b_y;
                if dist_align_top.abs() <= snap_threshold {
                    target_y = b_y;
                    y_snapped = true;
                }
            }
        }
    }

    // 处理磁性粘滞带来的逃逸惩罚（如果没有真正拉开 escape_threshold 距离，维持在吸附位置）
    if !x_snapped {
        for i in 0..keys.len() {
            if i == moved_index { continue; }
            let b_x = keys[i].x;
            let b_w = keys[i].width;

            let l_r = b_x + b_w + margin * 2;
            if (pure_mouse_x - l_r).abs() < escape_threshold && (keys[moved_index].x - l_r).abs() <= 1 {
                target_x = l_r;
                break;
            }
            let r_l = b_x - key_w - margin * 2;
            if (pure_mouse_x - r_l).abs() < escape_threshold && (keys[moved_index].x - r_l).abs() <= 1 {
                target_x = r_l;
                break;
            }
            if (pure_mouse_x - b_x).abs() < escape_threshold && (keys[moved_index].x - b_x).abs() <= 1 {
                target_x = b_x;
                break;
            }
        }
    }

    if !y_snapped {
        for i in 0..keys.len() {
            if i == moved_index { continue; }
            let b_y = keys[i].y;
            let b_h = keys[i].height;

            let t_b = b_y + b_h + margin * 2;
            if (pure_mouse_y - t_b).abs() < escape_threshold && (keys[moved_index].y - t_b).abs() <= 1 {
                target_y = t_b;
                break;
            }
            let b_t = b_y - key_h - margin * 2;
            if (pure_mouse_y - b_t).abs() < escape_threshold && (keys[moved_index].y - b_t).abs() <= 1 {
                target_y = b_t;
                break;
            }
            if (pure_mouse_y - b_y).abs() < escape_threshold && (keys[moved_index].y - b_y).abs() <= 1 {
                target_y = b_y;
                break;
            }
        }
    }

    // ==========================================
    // 阶段 2: 画布物理边界裁剪
    // ==========================================
    if target_x - margin < 0 { target_x = margin; }
    if target_x + key_w + margin > canvas_w { target_x = canvas_w - key_w - margin; }
    if target_y - margin < 0 { target_y = margin; }
    if target_y + key_h + margin > canvas_h { target_y = canvas_h - key_h - margin; }

    // ==========================================
    // 阶段 3: AABB 刚体阻挡（确保在吸附或自由滑动时都无法穿透实体）
    // ==========================================
    let mut a_x1 = target_x - margin;
    let mut a_x2 = target_x + key_w + margin;
    let mut a_y1 = target_y - margin;
    let mut a_y2 = target_y + key_h + margin;

    for i in 0..keys.len() {
        if i == moved_index { continue; }

        let b_x1 = keys[i].x - margin;
        let b_x2 = keys[i].x + keys[i].width + margin;
        let b_y1 = keys[i].y - margin;
        let b_y2 = keys[i].y + keys[i].height + margin;

        let overlap_x = a_x1 < b_x2 && a_x2 > b_x1;
        let overlap_y = a_y1 < b_y2 && a_y2 > b_y1;

        if overlap_x && overlap_y {
            let overlap_w = a_x2.min(b_x2) - a_x1.max(b_x1);
            let overlap_h = a_y2.min(b_y2) - a_y1.max(b_y1);

            if overlap_w < overlap_h {
                let center_a_x = a_x1 + (a_x2 - a_x1) / 2;
                let center_b_x = b_x1 + (b_x2 - b_x1) / 2;
                if center_a_x < center_b_x {
                    a_x1 -= overlap_w;
                } else {
                    a_x1 += overlap_w;
                }
                a_x2 = a_x1 + key_w + margin * 2;
            } else {
                let center_a_y = a_y1 + (a_y2 - a_y1) / 2;
                let center_b_y = b_y1 + (b_y2 - b_y1) / 2;
                if center_a_y < center_b_y {
                    a_y1 -= overlap_h;
                } else {
                    a_y1 += overlap_h;
                }
                a_y2 = a_y1 + key_h + margin * 2;
            }
        }
    }

    // 回写最终计算出的安全物理整数坐标
    keys[moved_index].x = a_x1 + margin;
    keys[moved_index].y = a_y1 + margin;
}