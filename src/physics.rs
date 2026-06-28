// src/physics.rs
use crate::core::config_def::KeyConfig;
use std::collections::HashMap;

/// AABB 碰撞盒（含 margin 扩展），用于碰撞检测管线的中间传递
struct CollisionBox {
    x1: i32,
    x2: i32,
    y1: i32,
    y2: i32,
}

impl CollisionBox {
    fn new(x: i32, y: i32, w: i32, h: i32, margin: i32) -> Self {
        Self {
            x1: x - margin,
            x2: x + w + margin,
            y1: y - margin,
            y2: y + h + margin,
        }
    }

    /// 还原为不含 margin 的原始坐标
    fn into_origin(self, margin: i32) -> (i32, i32) {
        (self.x1 + margin, self.y1 + margin)
    }
}

pub struct MovementPipeline {
    pub canvas_w: i32,
    pub canvas_h: i32,
    pub margin: i32,
    /// 吸附触发阈值（像素）
    pub snap_threshold: i32,
    /// 脱离吸附所需拉扯距离（像素）
    pub escape_threshold: i32,
    /// 空间哈希网格单元尺寸（像素）
    pub cell_size: i32,
    /// 超过此数量启用空间哈希索引
    pub hash_threshold: usize,
}

impl Default for MovementPipeline {
    fn default() -> Self {
        Self {
            canvas_w: 0,
            canvas_h: 0,
            margin: 0,
            snap_threshold: 6,
            escape_threshold: 14,
            cell_size: 100,
            hash_threshold: 30,
        }
    }
}

impl MovementPipeline {
    /// 核心暴露接口：原始鼠标输入通过过滤器，返回绝对安全的物理坐标
    /// `skip_indices` — 碰撞和吸附时忽略这些索引（用于多选拖拽，选中按键间不互斥）
    pub fn transform_position(
        &self,
        moved_idx: usize,
        req_x: i32,
        req_y: i32,
        keys: &[KeyConfig],
        enable_snap: bool,
        skip_indices: &std::collections::HashSet<usize>,
    ) -> (i32, i32) {
        // Filter 1: 磁性吸附
        let (x1, y1) = self.apply_grid_snap(moved_idx, req_x, req_y, keys, enable_snap, skip_indices);

        // Filter 2: 刚体碰撞阻挡
        let (x2, y2) = self.apply_aabb_collision(moved_idx, x1, y1, keys, skip_indices);

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
        skip_indices: &std::collections::HashSet<usize>,
    ) -> (i32, i32) {
        if !enable_snap {
            return (mx, my);
        }

        let key_w = keys[moved_index].width;
        let key_h = keys[moved_index].height;
        let spacing = self.margin;

        // 收集所有候选吸附位置，选择最近的
        let (best_x, snap_x) = self.find_best_snap_skipping(
            mx,
            moved_index,
            keys,
            self.snap_threshold,
            skip_indices,
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

        let (best_y, snap_y) = self.find_best_snap_skipping(
            my,
            moved_index,
            keys,
            self.snap_threshold,
            skip_indices,
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
        let fx = if snap_x && (mx - best_x).abs() <= self.escape_threshold { best_x } else { mx };
        let fy = if snap_y && (my - best_y).abs() <= self.escape_threshold { best_y } else { my };
        (fx, fy)
    }

    /// 在指定轴上查找距离鼠标坐标最近的吸附候选点（跳过 skip_indices 中的按键）
    /// 候选点数量固定为 4（左/右/上/下四种对齐方式）
    fn find_best_snap_skipping(
        &self,
        mouse_pos: i32,
        moved_index: usize,
        keys: &[KeyConfig],
        threshold: i32,
        skip_indices: &std::collections::HashSet<usize>,
        candidates_fn: impl Fn(&KeyConfig) -> [i32; 4],
    ) -> (i32, bool) {
        let mut best = mouse_pos;
        let mut best_dist = threshold;
        let mut found = false;

        for (i, k) in keys.iter().enumerate() {
            if i == moved_index || skip_indices.contains(&i) { continue; }
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
    // 按键数 ≥ hash_threshold 时启用空间哈希索引
    // ==========================================================
    fn apply_aabb_collision(
        &self,
        moved_index: usize,
        target_x: i32,
        target_y: i32,
        keys: &[KeyConfig],
        skip_indices: &std::collections::HashSet<usize>,
    ) -> (i32, i32) {
        let key_w = keys[moved_index].width;
        let key_h = keys[moved_index].height;
        let margin = self.margin;

        let mut cb = CollisionBox::new(target_x, target_y, key_w, key_h, margin);

        if keys.len() >= self.hash_threshold {
            self.apply_aabb_collision_indexed(moved_index, &mut cb, keys, skip_indices);
        } else {
            self.apply_aabb_collision_bruteforce(moved_index, &mut cb, keys, skip_indices);
        }

        cb.into_origin(margin)
    }

    /// 暴力遍历碰撞检测（用于按键数 < hash_threshold 的情况）
    fn apply_aabb_collision_bruteforce(
        &self,
        moved_index: usize,
        cb: &mut CollisionBox,
        keys: &[KeyConfig],
        skip_indices: &std::collections::HashSet<usize>,
    ) {
        let margin = self.margin;
        let mov_w = keys[moved_index].width;
        let mov_h = keys[moved_index].height;
        for (i, b) in keys.iter().enumerate() {
            if i == moved_index || skip_indices.contains(&i) { continue; }
            self.resolve_one_collision(b, margin, cb, mov_w, mov_h);
        }
    }

    /// 空间哈希加速碰撞检测（用于按键数 ≥ hash_threshold 的情况）
    fn apply_aabb_collision_indexed(
        &self,
        moved_index: usize,
        cb: &mut CollisionBox,
        keys: &[KeyConfig],
        skip_indices: &std::collections::HashSet<usize>,
    ) {
        let margin = self.margin;
        let mov_w = keys[moved_index].width;
        let mov_h = keys[moved_index].height;
        let cell_size = self.cell_size;

        // 构建空间哈希：将按键映射到网格
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, b) in keys.iter().enumerate() {
            if i == moved_index || skip_indices.contains(&i) { continue; }
            let cx = b.x / cell_size;
            let cy = b.y / cell_size;
            grid.entry((cx, cy)).or_default().push(i);
        }

        // 移动键所在网格及其相邻 8 格
        let gx = (cb.x1 + cb.x2) / 2 / cell_size;
        let gy = (cb.y1 + cb.y2) / 2 / cell_size;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(indices) = grid.get(&(gx + dx, gy + dy)) {
                    for &i in indices {
                        let b = &keys[i];
                        self.resolve_one_collision(b, margin, cb, mov_w, mov_h);
                    }
                }
            }
        }
    }

    /// 解析单个按键与移动键的碰撞
    ///
    /// 碰撞策略：计算两个 AABB 在 X 和 Y 方向的重叠量，
    /// 选择重叠较小的方向作为推开方向（即沿最短路径推开），
    /// 通过中心点位置决定向左/右或上/下推开。
    #[inline(always)]
    fn resolve_one_collision(
        &self,
        b: &KeyConfig,
        margin: i32,
        cb: &mut CollisionBox,
        mov_w: i32,
        mov_h: i32,
    ) {
        let bx1 = b.x - margin;
        let bx2 = b.x + b.width + margin;
        let by1 = b.y - margin;
        let by2 = b.y + b.height + margin;

        if cb.x1 < bx2 && cb.x2 > bx1 && cb.y1 < by2 && cb.y2 > by1 {
            let overlap_w = cb.x2.min(bx2) - cb.x1.max(bx1);
            let overlap_h = cb.y2.min(by2) - cb.y1.max(by1);

            if overlap_w < overlap_h {
                let center_a = cb.x1 + (cb.x2 - cb.x1) / 2;
                let center_b = bx1 + (bx2 - bx1) / 2;
                if center_a < center_b {
                    cb.x1 -= overlap_w;
                } else {
                    cb.x1 += overlap_w;
                }
                cb.x2 = cb.x1 + mov_w + margin * 2;
            } else {
                let center_a = cb.y1 + (cb.y2 - cb.y1) / 2;
                let center_b = by1 + (by2 - by1) / 2;
                if center_a < center_b {
                    cb.y1 -= overlap_h;
                } else {
                    cb.y1 += overlap_h;
                }
                cb.y2 = cb.y1 + mov_h + margin * 2;
            }
        }
    }

    // ==========================================================
    // Filter 3: 画布边缘强制限幅
    // 限制左/上边界（不能小于 margin），同时限制右/下边界
    // （按键 + margin 不能超出 canvas_w/canvas_h）
    // ==========================================================
    fn apply_canvas_boundary(
        &self,
        idx: usize,
        x: i32,
        y: i32,
        keys: &[KeyConfig],
    ) -> (i32, i32) {
        let margin = self.margin;
        let key_w = keys[idx].width;
        let key_h = keys[idx].height;

        // 左/上边界限幅
        let fx = if x - margin < 0 { margin } else { x };
        let fy = if y - margin < 0 { margin } else { y };

        // 右/下边界限幅（按键右/下边缘 + margin 不超出画布）
        let fx = if fx + key_w + margin > self.canvas_w { self.canvas_w - key_w - margin } else { fx };
        let fy = if fy + key_h + margin > self.canvas_h { self.canvas_h - key_h - margin } else { fy };

        (fx, fy)
    }
}