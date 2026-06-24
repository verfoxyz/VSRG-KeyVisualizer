# Slint 开发技能（本项目定制版）

> 基于 Slint 官方 [ai-plugins/skills/slint](https://github.com/slint-ui/ai-plugins/tree/master/skills/slint) 适配

## 项目 Slint 版本

- **Slint 1.16.3**（见 `Cargo.lock`）
- 编译方式：`build.rs` 中 `slint_build::compile("ui/application.slint")`
- Rust 端导入：`slint::include_modules!()`

### 项目 .slint 文件结构

```
ui/
├── application.slint       # 入口，export 所有窗口
├── main_window.slint       # 主窗口（按键+音符渲染，右键菜单 PopupWindow）
├── settings_window.slint   # 配置窗口（画布预览+参数面板+profile 列表）
├── key_data.slint          # KeyData 结构体定义
├── key_capture_dialog.slint # 按键捕获弹窗
└── dialogs.slint           # 通用对话框
```

## 本项目 Slint 代码规范

1. **命名**：属性名和回调名使用 **kebab-case**；Rust 端通过 `on_<callback_name>` 绑定
2. **组件导出**：所有组件在 `application.slint` 中统一 `export`
3. **结构体共享**：`key_data.slint` 中的 `KeyData` 被多文件引用
4. **属性方向**：跨组件边界时显式使用 `in`/`out`/`in-out`/`private property`
5. **绑定 vs 赋值**：`name: expr;` 是响应式绑定；`name = expr;` 在回调内是命令式赋值
6. **字符串插值**：使用 `"Count: \{root.count}"`（反斜杠+花括号），不是 `${}` 或 `{}`
7. **元素控制流**：`if cond : Elem {}`；`for item[index] in model : Elem {}`
8. **双向绑定**：`a <=> b`

## 常用模式

### 回调绑定（Rust 端）

```rust
slint::include_modules!();
window.on_my_action({
    let weak = window.as_weak();
    move |val| {
        let window = weak.unwrap();
        // 处理回调
        "result".into()
    }
});
```

### 共享状态（Global）

```slint
export struct MyData { field: int }
export global AppState {
    in property <[MyData]> items;
    callback item-clicked(int);
}
```

```rust
ui.global::<AppState>().set_items(ModelRc::new(VecModel::from(vec)));
ui.global::<AppState>().on_item_clicked(move |i| { /* ... */ });
```

- `[T]` → `ModelRc<T>`（Rust）
- `string` ↔ `SharedString`；`length`/`float` ↔ `f32`；`int` ↔ `i32`
- kebab-case → snake_case：`row-clicked` → `on_row_clicked`

### 常见坑

1. **`Weak<T>` 循环引用**：窗口引用使用 `slint::Weak<T>` + `clone().unwrap()` 模式
2. **VecModel 刷新**：修改后调用 `set_xxx(create_model(&data))` 重建
3. **浮点数转换**：`Cannot convert float to length` → 用 `value * 1px` 或 `len / 1px`
4. **填充行为**：`Rectangle` 默认填充父容器；`Text`/`Image` 默认取 preferred 大小
5. **字符串插值**：必须用 `"\{expr}"`，不是 `${expr}` 或 `{expr}`
6. **enum 值**：内置枚举用小写 `PointerEventKind.down`，特殊键用 `Key.Escape`
