# Repository Guidelines

## 项目概述

VSRG-KeyVisualizer 是一个音游按键可视化工具，使用 **Rust + Slint 1.16** 构建。主窗口透明穿透显示按键和瀑布流，配置窗口用于图形化编辑按键布局。支持多 profile 配置管理。

## 核心架构

### 数据流全景

```
物理键盘 → [Windows: Raw Input API | Unix: rdev::listen()]
         → crossbeam_channel (MyKeyEvent)
         → events.rs::start_event_timer (16ms 轮询回调)
              ├─ capture_mode? → MacroRecorder: 向 temp_config + config 添加新键
              └─ 正常模式 → LiveVisualizer: 生成/更新/移动/回收 BarNote
                            → ui.set_bar_notes() 刷新 Slint 瀑布流模型
```

### 三大状态层

| 状态层 | 持有者 | 用途 |
|--------|--------|------|
| `config` | `AppState.config` | 运行时真实配置，主窗口瀑布流使用 |
| `temp_config` | `AppState.temp_config` | 配置窗口编辑中的临时副本，保存时覆盖 config |
| `selected_indices` | `AppState.selected_indices: HashSet<usize>` | 配置窗口多选集合 |

### 配置窗口事件分发

所有用户操作（点击、拖拽、SpinBox 编辑）统一走 **`UIAction` 枚举 → `dispatch()` 方法** 的单一路径：

1. Slint 前端 `callback` → Rust 端 `on_<name>` 绑定
2. Rust 端构造 `UIAction` 枚举 → 调用 `state.dispatch(action, &settings_weak)`
3. `dispatch()` 内部获取 `temp_config` 锁 → 执行操作 → 通过 `create_model_with_selection()` 刷新预览画布

| UIAction | 触发方式 | 行为 |
|----------|----------|------|
| `HitTestAndSelect { ctrl }` | 画布点击 | ctrl 切换多选，非 ctrl 单选（点击已选中保留选集） |
| `DragKeyOnCanvas` | 画布拖拽 | 锚点键走物理管线，增量同步到所有选中键 |
| `SpinBoxUpdateX/Y` | SpinBox 编辑 | 同拖拽逻辑，关闭磁吸吸附 |
| `BatchUpdateWidth/Height/Color/Opacity/BarWidthPercent` | 浮动参数面板 | 遍历所有选中键设置同一值 |
| `BatchDeleteKeys` | Delete 键/按钮 | 从大到小排序删除选中键 |

### 独立参数面板窗口 (`param_panel_window.rs`)

参数面板从设置窗口内浮动面板改为**独立无边框窗口**，通过 `no-frame: true` 的 Slint Window 实现。

#### 面板拖拽算法

标题栏 `TouchArea` 回调传递**屏幕坐标**（非面板局部坐标），Rust 端使用"固定偏移锁定法"：

```
on_drag_begin(mx, my):   // mx, my = 屏幕坐标 GetCursorPos
  offset_x = mx - window_x
  offset_y = my - window_y

on_drag_move(mx, my):    // mx, my = 屏幕坐标
  virtual_x = mx - offset_x
  virtual_y = my - offset_y
  // 检查是否在吸附范围内 → SNAPPED 或跟随
```

`offset` 在 `drag_begin` 记录后**永不重算**，消除指针漂移。

#### 吸附状态机

| 状态 | 条件 | 行为 |
|------|------|------|
| **SNAPPED** | `virtual_pos` 距 settings 窗口边界 < `SNAP_DISTANCE (50px)` | 后台线程锁定面板位置跟随 settings 窗口 |
| **DRAGGING** | 用户正在拖拽面板标题栏（`DRAG_STATE.started == true`） | 后台线程暂停 `SetWindowPos`，Slint 驱动位置 |
| **FOLLOWING** | 已脱离吸附范围但未在拖拽 | 后台线程以 `SetWindowPos` 设置 `virtual_pos` |

#### 后台轮询线程

```
spawn() → 每 16ms 循环:
  1. 检测 SETTINGS_HWND 是否已销毁 → 关闭面板
  2. 如果 DRAG_STATE.started → 跳过（用户正在拖拽）
  3. 如果 SNAPPED → 计算 settings 窗口右边界 + 偏移，SetWindowPos
  4. 否则 → 检查 virtual_pos，SetWindowPos
```

#### Show/Hide 面板

- 通过 `param_panel_holder`（`Arc<Mutex<Option<ParamPanelWindow>>`，强引用）管理生命周期
- **Show**：创建 `ParamPanelWindow` → 存储到 `param_panel_holder` → 调用 `.show()`
- **Hide**：调用 `.hide()`，后台线程跳过（`PARAM_PANEL_HWND` 为空）
- **设置窗口关闭时自动关闭面板**：后台线程检测 `SETTINGS_HWND` 有效性
- **防冻结**：通过 `SingleShotTimer` 延迟创建窗口，避免 Slint callback 递归

### 配置窗口布局

```
┌─────────────────────────────────────────────────────┐
│ [▶] (折叠时) 或 ProfilePanel (展开时)  │  Location Preview  [Show/Hide Panel] │
│                                                │  ┌─────────────────────┐        │
│                                                │  │                     │        │
│                                                │  │   SettingsPreview   │        │
│                                                │  │   (画布预览)        │        │
│                                                │  │                     │        │
│                                                │  └─────────────────────┘        │
│                                                │  [Delete Selected] [Close/Save] │
└─────────────────────────────────────────────────────┘
  ┌─── 独立参数面板窗口 ──────────────────────────┐
  │ ⋮⋮ Parameter Config                          │
  │ Key Properties (可折叠)                       │
  │ Global Settings (可折叠)                      │
  └──────────────────────────────────────────────┘
```

### 物理管线 (MovementPipeline)

```
transform_position():
  1. apply_grid_snap()       — 磁吸吸附（阈值 6px，磁滞 14px，跳过 skip_indices）
  2. apply_aabb_collision()  — AABB 刚体碰撞（margin 扩展盒；≥30 键启用空间哈希）
  3. apply_canvas_boundary() — 边界限幅（仅左/上，无右/下限制）
```

多选拖拽时 `skip_indices = selected_indices`，选中键之间不互碰。增量 `dx/dy` 统一应用到所有选中键（刚性整体移动），预扫所有选中键的边界，任一键碰边界则整组该轴停止。

### Profile 配置系统 (`configs.rs`)

```
configs/
├── .active                  # 记录当前激活的 profile 名（纯文本）
└── profiles/
    ├── default.json         # 默认 profile
    ├── 4K.json              # 4K 布局
    └── *.json               # 用户自定义 profile
```

- `configs::initialize()` — 启动时调用，迁移旧 `config.json` → `configs/profiles/default.json`
- `load_active_profile()` — 读取 `.active` → 加载对应 json
- `save_profile() / load_profile()` — 按名读写
- `switch_profile() / create_profile() / delete_profile()` — 管理操作（暂未接入 UI）

### 瀑布流音符生命周期

两阶段：

1. **Growing 阶段** (`is_growing == true`)：按键按下时创建 BarNote，从 0 开始在流向方向扩展至全长
2. **Traveling 阶段** (`is_growing == false`)：按键释放后按 `vel = ±flow_speed` 匀速移动，超出画布后回收

`front_line_emit` 开启时，所有音符从前排边缘统一发射（通过 `calc_front_line_edge()` 计算）。

## 项目结构

```
VSRG-KeyVisualizer/
├── src/                            # Rust 源代码
│   ├── main.rs                     # 入口、AppConfig/KeyConfig/BarNote 结构体、config I/O、
│   │                               #   MainWindow 回调绑定、Raw Input 消息窗口、窗口穿透与拖拽
│   ├── configs.rs                  # 多 profile 配置管理（目录结构、迁移、CRUD）
│   ├── events.rs                   # 16ms 定时器渲染循环、MacroRecorder、LiveVisualizer
│   ├── physics.rs                  # 磁吸吸附、AABB 碰撞、画布边界、MovementPipeline、空间哈希
│   ├── ri_table.rs                 # Windows Raw Input 虚拟键码 → rdev 字符串映射表
│   ├── state.rs                    # AppState（共享状态）、UIAction 枚举、dispatch 中央分发器
│   └── gui/
│       ├── settings_window.rs      # 配置窗口回调绑定（所有 on_<callback> 注册、config_dirty 标记）
│       └── param_panel_window.rs   # 独立参数面板窗口管理（拖拽、吸附跟随、后台轮询线程）
├── ui/                             # Slint 界面文件 (.slint)
│   ├── application.slint           # 组件聚合入口（export 所有窗口）
│   ├── key_data.slint              # KeyData 结构体定义（name/x/y/w/h/selected/anchor_ratio 等）
│   ├── main_window/
│   │   └── main_window.slint       # 主窗口布局（按键渲染、音符渲染、右键菜单 PopupWindow）
│   ├── settings/
│   │   ├── param_panel_window.slint# 独立参数面板窗口（无边框、可折叠 KeyProps/GlobalSettings）
│   │   ├── settings_window.slint   # 配置窗口布局（画布预览、profile 列表）
│   │   ├── settings_preview.slint  # 画布预览（按键渲染、选中、拖拽）
│   │   ├── key_props_panel.slint   # 按键属性面板（X/Y/W/H/Color/Opacity/BarW%）
│   │   ├── global_settings_panel.slint # 全局设置面板（颜色、流向、边界等）
│   │   └── profile_panel.slint     # Profile 列表（切换/重命名/新增/删除）
│   └── dialogs/
│       ├── dialogs.slint           # 确认删除、重命名对话框
│       └── key_capture_dialog.slint # 按键捕获弹窗（ESC 关闭、失去焦点关闭）
├── configs/                        # 配置系统目录
│   ├── .active                     # 当前激活 profile 名
│   └── profiles/                   # 各 profile JSON
├── Code_template/                  # 透明白板窗口模板（新窗口开发起点）
├── MD-PNG/                         # README 用到的截图
├── build.rs                        # Slint 编译脚本（slint_build::compile）
├── Cargo.toml                      # 依赖与平台条件编译
├── AGENTS.md                       # AI 代理开发指南
└── README.md / README-EN.md        # 使用说明
```

### 各文件职责

| 文件 | 职责 |
|------|------|
| `src/main.rs` | 入口、`AppConfig`/`KeyConfig`/`BarNote` 数据模型、config 读写、MainWindow 回调绑定、Raw Input 消息窗口&消息泵、窗口穿透/拖拽/位置恢复、`ToKeyData` trait、`hex_str_to_color`/`merge_alpha`/`split_alpha`、`PRIMARY_SCREEN_SIZE` OnceLock 缓存、`pub mod param_panel_window` 模块导出 |
| `src/configs.rs` | 多 profile 管理：目录初始化、config.json 迁移、`load_active_profile`/`save_profile`/`list_profiles` |
| `src/events.rs` | `start_event_timer` 16ms 定时器驱动渲染循环、`MacroRecorder` 捕获按键并写入配置、`LiveVisualizer` 音符生成/生长/移动/回收 |
| `src/physics.rs` | `MovementPipeline` 三阶段管线、`find_best_snap_skipping` 磁吸候选、`resolve_one_collision` AABB 碰撞解析、`spatial_hash` 空间哈希索引（≥30 键自动启用） |
| `src/state.rs` | `AppState` 全部共享状态（13 个 `Arc` 字段）、`UIAction` 枚举、`dispatch()` 中央分发器（选中管理/拖拽/批量编辑/删除）、`param_panel_holder` 强引用管理面板窗口生命周期 |
| `src/gui/settings_window.rs` | `setup_settings_window()` 配置窗口初始化、所有 Slint callback 绑定、`config_dirty` 脏标记追踪、`setup_param_panel_window()` 创建独立面板、保存/关闭按钮逻辑（脏则保存并停留，干净则关闭） |
| `src/gui/param_panel_window.rs` | 独立参数面板窗口生命周期管理：创建/显示/隐藏/关闭、SNAPPED 状态机（吸附/跟随/解除）、橡胶筋拖拽（virtual position 模型）、16ms 后台轮询线程（SetWindowPos 定位）、SETTINGS_HWND 检测拖拽自行解除吸附 |
| `ui/main_window/main_window.slint` | 透明窗口、`for key_info in keys` 按键渲染 + `for bar in bar_notes` 音符渲染、TouchArea 交互、右键菜单 PopupWindow |
| `ui/settings/param_panel_window.slint` | 独立参数面板窗口（no-frame、自定义 drag 回调、可折叠 KeyProps/GlobalSettings） |
| `ui/settings/settings_window.slint` | 配置窗口整体布局：左侧可折叠 Profile 列表 + 右侧画布预览 + `config_dirty` 属性 + 底部操作按钮（Save/Close 联动） |
| `ui/settings/settings_preview.slint` | 画布预览（按键渲染、touch 选中/拖拽、canvas_enabled 控制） |
| `ui/settings/key_props_slint` | 按键属性面板（X/Y/W/H/Color/Opacity/BarW%，统一竖列布局，可折叠） |
| `ui/settings/global_settings_panel.slint` | 全局设置面板（Key Color/Border/Front-Line/Flow Direction/Flow Speed/Boundary/Margin，统一竖列布局，默认折叠） |
| `ui/settings/profile_panel.slint` | Profile 列表管理（切换/重命名/新增/删除，宽 200px） |
| `ui/key_data.slint` | `KeyData { name, display_name, is_pressed, x, y, w, h, anchor_ratio_x/y, pressed_color, color_hex, selected }` |
| `ui/dialogs/dialogs.slint` | 确认删除对话框、重命名对话框 |
| `ui/dialogs/key_capture_dialog.slint` | 按键捕获弹窗，`init => fs.focus()` 聚焦，`changed focused` 失焦自动关闭 |

## 编译、测试与开发命令

**无特殊需要不自动构建发布版**

```bash
cargo build                  # 调试构建
cargo build --release        # 发布构建
cargo run --release          # 运行发布版
cargo test                   # 运行测试（当前 2 个，在 configs.rs）
```
> 如果编译输出极长（如大量警告）时，使用下面这个命令,只看末尾的“编译成功/失败”结论，减少刷屏
```bash
cargo build 2>&1 | Select-Object -Last 20 #将编译输出的全部信息（stdout + stderr）通过管道传给 PowerShell 的 Select-Object -Last N，只截取最后 N 行显示在终端
```

> **性能提示**：Debug 版本因 Slint 和锁竞争有明显渲染延迟，Release 版本流畅。开发时用 `cargo build`，测试时用 `cargo build --release && ./target/release/VSRG-KeyVisualizer.exe`。

VS Code 调试配置在 `.vscode/launch.json`：**Debug executable 'VSRG-KeyVisualizer'** 和 **Debug unit tests in executable 'VSRG-KeyVisualizer'**。

## 编码风格与命名约定

- 缩进使用 **4 空格**，遵循 Rust 2024 edition。
- Rust 代码按 `use` 导入 → 模块定义 → 常量 → 结构体 → 函数 顺序组织。
- 结构体字段和函数名使用 **snake_case**；类型和 trait 使用 **PascalCase**；常量使用 **SCREAMING_CASE**。
- Slint 属性名和回调名使用 **kebab-case**。Rust 端通过 `slint::include_modules!()` 导入，回调绑定使用 `on_<callback_name>`。
- 平台条件编译：`#[cfg(windows)]` 下 Raw Input API + win32 window manipulation；`#[cfg(unix)]` 下 `rdev` crate。
- 模块级静态量使用 `std::sync::OnceLock`（如 `PRIMARY_SCREEN_SIZE`）或 `AtomicBool`（如 `DRAG_ACTIVE`、`SNAPPED`、`POLLING_ACTIVE`、`DRAG_STATE`）。

## 锁约定与并发安全

- **锁顺序**：`dispatch()` 获取 `temp_config` 锁后不可再获取 `config` 锁；main window 回调获取 `config` 锁后不可获取 `temp_config` 锁。违者死锁。
- **短持有模式**：`selected_indices` 和 `drag_offset` 使用 `clone() + drop()`（在 `{ }` 块中获取锁并 clone，立即释放），避免嵌套锁。
- **跨线程通信**：`crossbeam_channel::unbounded<MyKeyEvent>` 用于键盘事件线程 → 主线程 16ms 轮询。
- **窗口弱引用**：`dialog_holder` / `settings_holder` 存 `slint::Weak<T>`，避免循环引用。**例外**：`param_panel_holder` 使用强引用 `ParamPanelWindow`（`Arc<Mutex<Option<ParamPanelWindow>>>`），防止函数返回时窗口被析构。
- **原子标志位**：`capture_mode` / `notes_dirty` / `SNAPPED` / `POLLING_ACTIVE` / `DRAG_STATE.started` 等用 `AtomicBool`，无锁读取。

## 测试指南

- 测试框架使用 `#[test]`。目前 `configs.rs` 有 2 个测试（`test_read_write_active`、`test_list_profiles_empty`）。
- `physics.rs` 为纯函数模块，非常适合加单元测试。
- `state.rs` 的 `dispatch()` 分支逻辑复杂，需补测。
- 命名风格：`test_<函数名>_<场景>`。

## 提交与 Pull Request 规范

- **提交信息使用中文**，简洁描述。如：`优化算法（physics.rs）`、`增加 profile 切换`、`ctrl多选拖动修复`。
- 一条提交对应一个逻辑变更。
- PR 描述说明改动模块、原因、UI 变更截图（如有）、性质（bugfix/feat/refactor）。

## 注意事项

- 项目使用 `#![windows_subsystem = "windows"]` + Cargo rustflags 双重隐藏控制台，`cargo run` 仍可输出日志。
- `tracing` + `tracing-subscriber` 用于日志，`main()` 中初始化 `tracing_subscriber::fmt()`。Release 构建自动使用 `INFO` 级别，Debug 使用 `DEBUG`。
- 物理引擎 (`physics.rs`) 在按键数 ≥ `INDEXING_THRESHOLD (30)` 时自动启用空间哈希索引，新增碰撞逻辑需同步更新索引。
- **`lazy_static` 已导入但未使用** — 实际使用 `OnceLock` 替代。
- **独立参数面板窗口拖拽**：使用 Slint `TouchArea` 回调传递屏幕坐标，`offset = cursor - window_position` 固定偏移锁定法。`DRAG_STATE` 静态原子标志控制后台线程暂停。**非** `HTCAPTION/SendMessageW` 方案。
- **多选删除**：`BatchDeleteKeys` 从大到小排序删除，避免索引偏移。
- **右键菜单**：`init => { fs.focus(); }` 打开即聚焦，`changed focused` 失焦自动关闭。
- **弹出窗口防重复**：创建设置窗口和按键捕获对话框前检查 holder。
- **无右/下边界限幅**：`apply_canvas_boundary()` 只限制左和上，按键可拖出画布右下。
- **模型重建**：每帧调用 `set_bar_notes(create_model(&notes))` 重建 VecModel，注意性能损耗。
- **profile 切换不回检查未保存变更**：直接丢弃 `temp_config`。
- **配置文件默认**：`panel_expanded: false`（参数面板默认隐藏），`profile_expanded: false`（Profile 列表默认折叠）。
- **UI 文件结构**：已按功能分类到 `ui/main_window/`、`ui/settings/`、`ui/dialogs/` 子目录，通过 `application.slint` 统一导出。
- **Profile 折叠**：左侧列表默认折叠（24px 竖条 + ▶ 图标），点击展开；展开后右侧容器边缘显示 ◀ 收缩按钮。由 `profile_expanded` 属性控制。
- **参数面板显隐**：通过「Show Panel」/「Hide Panel」按钮控制，按钮位于画布预览右上角。`panel_expanded` 属性控制整个 `param_panel` 是否在设置窗口内渲染。独立窗口通过 `param_panel_holder`（强引用）管理生命周期。
- **Save Config / Close 按钮**：按钮固定在画布预览下方靠左。`config_dirty` 控制按钮行为——脏时显示「Save Config」（primary 样式），点击保存配置并保持打开（按钮变为「Close」）；干净时显示「Close」，直接关闭窗口。
- **`config_dirty` 脏标记**：所有编辑 callback 调用 `mark_dirty()` 设置标志；保存操作清除标志。保存时若干净则关闭，若脏则保存后保持打开。
- **`key_handler`** 位于右侧容器根级（不依赖 `if` 条件块），确保键盘快捷键始终生效。
