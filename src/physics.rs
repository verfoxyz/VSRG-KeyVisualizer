// src/physics.rs
use crate::KeyConfig;

const SNAP_THRESHOLD: i32 = 6;    // 吸附触发阈值
const ESCAPE_THRESHOLD: i32 = 14; // 脱离吸附所需拉扯距离

pub struct MovementPipeline {
    pub canvas_w: i32,
    pub canvas_h: i32,
    pub margin: i32,
}

impl MovementPipeline {
    /// 核心暴露接口：原始鼠标输入通过过滤器，返回绝对安全的物理坐标
    pub fn transform_position(
        &self,
        moved_idx: usize,
        req_x: i32,
        req_y: i32,
        keys: &[KeyConfig],
        enable_snap: bool,
    ) -> (i32, i32) {
        // Filter 1: 磁性吸附状态机过滤
        let (x1, y1) = self.apply_grid_snap(moved_idx, req_x, req_y, keys, enable_snap);

        // Filter 2: 刚体碰撞阻挡过滤
        let (x2, y2) = self.apply_aabb_collision(moved_idx, x1, y1, keys);

        // Filter 3: 画布限幅边界过滤
        self.apply_canvas_boundary(moved_idx, x2, y2, keys)
    }

    // ==========================================================
    // Filter 1: “#” 字延长边磁性吸附状态机
    // ==========================================================
    fn apply_grid_snap(
        &self,
        moved_index: usize,
        pure_mouse_x: i32,
        pure_mouse_y: i32,
        keys: &[KeyConfig],
        enable_snap: bool,
    ) -> (i32, i32) {
        if !enable_snap {
            return (pure_mouse_x, pure_mouse_y);
        }

        let mut target_x = pure_mouse_x;
        let mut target_y = pure_mouse_y;
        let key_w = keys[moved_index].width;
        let key_h = keys[moved_index].height;

        for i in 0..keys.len() {
            if i == moved_index { continue; }

            let b = &keys[i];
            let spacing = self.margin;

            // X轴吸附逻辑
            let l_to_r = b.x + b.width + spacing;
            if (pure_mouse_x - l_to_r).abs() <= SNAP_THRESHOLD { target_x = l_to_r; }
            let r_to_l = b.x - key_w - spacing;
            if (pure_mouse_x - r_to_l).abs() <= SNAP_THRESHOLD { target_x = r_to_l; }
            let l_to_l = b.x;
            if (pure_mouse_x - l_to_l).abs() <= SNAP_THRESHOLD { target_x = l_to_l; }

            // 脱离拉扯判定
            if (pure_mouse_x - target_x).abs() > ESCAPE_THRESHOLD {
                target_x = pure_mouse_x;
            }

            // Y轴吸附逻辑
            let b_to_t = b.y + b.height + spacing;
            if (pure_mouse_y - b_to_t).abs() <= SNAP_THRESHOLD { target_y = b_to_t; }
            let t_to_b = b.y - key_h - spacing;
            if (pure_mouse_y - t_to_b).abs() <= SNAP_THRESHOLD { target_y = t_to_b; }
            let b_to_b = b.y;
            if (pure_mouse_y - b_to_b).abs() <= SNAP_THRESHOLD { target_y = b_to_b; }

            if (pure_mouse_y - target_y).abs() > ESCAPE_THRESHOLD {
                target_y = pure_mouse_y;
            }
        }

        (target_x, target_y)
    }

    // ==========================================================
    // Filter 2: AABB 刚体碰撞阻挡过滤器
    // ==========================================================
    fn apply_aabb_collision(
        &self,
        moved_index: usize,
        target_x: i32,
        target_y: i32,
        keys: &[KeyConfig],
    ) -> (i32, i32) {
        let key_w = keys[moved_index].width;
        let key_h = keys[moved_index].height;
        let margin = self.margin;

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

        (a_x1 + margin, a_y1 + margin)
    }

    // ==========================================================
    // Filter 3: 画布边缘强制限幅（仅限左边界和上边界，无右边界和下边界）
    // ==========================================================
    fn apply_canvas_boundary(
        &self,
        _idx: usize,
        x: i32,
        y: i32,
        _keys: &[KeyConfig],
    ) -> (i32, i32) {
        let margin = self.margin;

        let mut fx = x;
        let mut fy = y;

        // 仅限制左边界和上边界（不能移出画布左/上侧）
        if fx - margin < 0 { fx = margin; }
        if fy - margin < 0 { fy = margin; }
        // 右侧和底部不做限制，允许按键超出画布范围

        (fx, fy)
    }
}