// src/physics.rs
use crate::KeyConfig;
use std::collections::HashMap;

const SNAP_THRESHOLD: i32 = 6;    // 吸附触发阈值
const ESCAPE_THRESHOLD: i32 = 14; // 脱离吸附所需拉扯距离
const INDEXING_THRESHOLD: usize = 30; // 超过此数量启用空间索引

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
        // Filter 1: 磁性吸附
        let (x1, y1) = self.apply_grid_snap(moved_idx, req_x, req_y, keys, enable_snap);

        // Filter 2: 刚体碰撞阻挡
        let (x2, y2) = self.apply_aabb_collision(moved_idx, x1, y1, keys);

        // Filter 3: 画布限幅边界
        self.apply_canvas_boundary(moved_idx, x2, y2, keys)
    }

    // ==========================================================
    // Filter 1: “#” 字延长边磁性吸附
    // 修复：选择距离鼠标坐标最近的合法吸附点，脱离阈值统一在最后判定
    // ==========================================================
    fn apply_grid_snap(
        &self,
        moved_index: usize,
        mx: i32,
        my: i32,
        keys: &[KeyConfig],
        enable_snap: bool,
    ) -> (i32, i32) {
        if !enable_snap {
            return (mx, my);
        }

        let key_w = keys[moved_index].width;
        let key_h = keys[moved_index].height;
        let spacing = self.margin;

        // 收集所有候选吸附位置，选择最近的
        let (best_x, snap_x) = self.find_best_snap(
            mx,
            moved_index,
            keys,
            SNAP_THRESHOLD,
            |k: &KeyConfig| -> [i32; 4] {
                // 为 X 方向生成 4 个吸附候选值
                [
                    k.x,                                  // 左对齐
                    k.x + k.width + spacing,               // 右侧 + 间距
                    k.x - key_w - spacing,                 // 左侧 - 按键宽 - 间距
                    k.x + k.width - key_w,                 // 右对齐（移动键右缘对齐固定键右缘）
                ]
            },
        );

        let (best_y, snap_y) = self.find_best_snap(
            my,
            moved_index,
            keys,
            SNAP_THRESHOLD,
            |k: &KeyConfig| -> [i32; 4] {
                [
                    k.y,
                    k.y + k.height + spacing,
                    k.y - key_h - spacing,
                    k.y + k.height - key_h,
                ]
            },
        );

        // 统一脱离判定：如果最近吸附点离鼠标太远，则取消吸附
        let fx = if snap_x && (mx - best_x).abs() <= ESCAPE_THRESHOLD { best_x } else { mx };
        let fy = if snap_y && (my - best_y).abs() <= ESCAPE_THRESHOLD { best_y } else { my };
        (fx, fy)
    }

    /// 在指定轴上查找距离鼠标坐标最近的吸附候选点
    fn find_best_snap<const N: usize>(
        &self,
        mouse_pos: i32,
        moved_index: usize,
        keys: &[KeyConfig],
        threshold: i32,
        candidates_fn: impl Fn(&KeyConfig) -> [i32; N],
    ) -> (i32, bool) {
        let mut best = mouse_pos;
        let mut best_dist = threshold; // 超过阈值就不吸附
        let mut found = false;

        for (i, k) in keys.iter().enumerate() {
            if i == moved_index { continue; }
            for &cand in candidates_fn(k).iter() {
                let d = (mouse_pos - cand).abs();
                if d <= best_dist {
                    best_dist = d;
                    best = cand;
                    found = true;
                }
            }
        }

        (best, found)
    }

    // ==========================================================
    // Filter 2: AABB 刚体碰撞阻挡
    // 按键数 ≥ INDEXING_THRESHOLD 时启用空间哈希索引
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

        let mut ax1 = target_x - margin;
        let mut ax2 = target_x + key_w + margin;
        let mut ay1 = target_y - margin;
        let mut ay2 = target_y + key_h + margin;

        if keys.len() >= INDEXING_THRESHOLD {
            self.apply_aabb_collision_indexed(moved_index, &mut ax1, &mut ax2, &mut ay1, &mut ay2, keys);
        } else {
            self.apply_aabb_collision_bruteforce(moved_index, &mut ax1, &mut ax2, &mut ay1, &mut ay2, keys);
        }

        (ax1 + margin, ay1 + margin)
    }

    fn apply_aabb_collision_bruteforce(
        &self,
        moved_index: usize,
        ax1: &mut i32,
        ax2: &mut i32,
        ay1: &mut i32,
        ay2: &mut i32,
        keys: &[KeyConfig],
    ) {
        let margin = self.margin;
        for (i, b) in keys.iter().enumerate() {
            if i == moved_index { continue; }
            self.resolve_one_collision(b, margin, ax1, ax2, ay1, ay2);
        }
    }

    fn apply_aabb_collision_indexed(
        &self,
        moved_index: usize,
        ax1: &mut i32,
        ax2: &mut i32,
        ay1: &mut i32,
        ay2: &mut i32,
        keys: &[KeyConfig],
    ) {
        let margin = self.margin;
        let cell_size = 100; // 空间哈希网格单元大小

        // 构建空间哈希：将按键映射到网格
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, b) in keys.iter().enumerate() {
            if i == moved_index { continue; }
            let cx = b.x / cell_size;
            let cy = b.y / cell_size;
            grid.entry((cx, cy)).or_default().push(i);
        }

        // 移动键所在网格及其相邻 8 格
        let gx = (*ax1 + *ax2) / 2 / cell_size;
        let gy = (*ay1 + *ay2) / 2 / cell_size;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(indices) = grid.get(&(gx + dx, gy + dy)) {
                    for &i in indices {
                        let b = &keys[i];
                        self.resolve_one_collision(b, margin, ax1, ax2, ay1, ay2);
                    }
                }
            }
        }
    }

    /// 解析单个按键与移动键的碰撞
    #[inline(always)]
    fn resolve_one_collision(
        &self,
        b: &KeyConfig,
        margin: i32,
        ax1: &mut i32,
        ax2: &mut i32,
        ay1: &mut i32,
        ay2: &mut i32,
    ) {
        let bx1 = b.x - margin;
        let bx2 = b.x + b.width + margin;
        let by1 = b.y - margin;
        let by2 = b.y + b.height + margin;

        if *ax1 < bx2 && *ax2 > bx1 && *ay1 < by2 && *ay2 > by1 {
            let overlap_w = (*ax2).min(bx2) - (*ax1).max(bx1);
            let overlap_h = (*ay2).min(by2) - (*ay1).max(by1);

            if overlap_w < overlap_h {
                let center_a = *ax1 + (*ax2 - *ax1) / 2;
                let center_b = bx1 + (bx2 - bx1) / 2;
                if center_a < center_b {
                    *ax1 -= overlap_w;
                } else {
                    *ax1 += overlap_w;
                }
                *ax2 = *ax1 + (b.width) + margin * 2;
            } else {
                let center_a = *ay1 + (*ay2 - *ay1) / 2;
                let center_b = by1 + (by2 - by1) / 2;
                if center_a < center_b {
                    *ay1 -= overlap_h;
                } else {
                    *ay1 += overlap_h;
                }
                *ay2 = *ay1 + (b.height) + margin * 2;
            }
        }
    }

    // ==========================================================
    // Filter 3: 画布边缘强制限幅（仅限左边界和上边界）
    // ==========================================================
    fn apply_canvas_boundary(
        &self,
        _idx: usize,
        x: i32,
        y: i32,
        _keys: &[KeyConfig],
    ) -> (i32, i32) {
        let margin = self.margin;
        let fx = if x - margin < 0 { margin } else { x };
        let fy = if y - margin < 0 { margin } else { y };
        (fx, fy)
    }
}