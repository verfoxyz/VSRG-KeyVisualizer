#[cfg(windows)]
pub fn win_vkey_to_rdev_str(vkey: u16) -> &'static str {
    match vkey {
        // 1. 特殊功能键：直接返回静态字符串切片
        0x1B => "Escape",
        0x20 => "Space",
        0x0D => "Return",
        0x08 => "Backspace",
        0x09 => "Tab",
        0x25 => "LeftArrow",
        0x26 => "UpArrow",
        0x27 => "RightArrow",
        0x28 => "DownArrow",
        0xBA => "SemiColon",
        0xBB => "Plus",
        0xBC => "Comma",
        0xBD => "Minus",
        0xBE => "Period",
        0xBF => "Slash",
        0xC0 => "BackQuote",
        0xDB => "OpenBracket",
        0xDC => "BackSlash",
        0xDD => "CloseBracket",
        0xDE => "Quote",
        0x2E => "Delete",

        // 2. A-Z (0x41 - 0x5A)：范围匹配 + 数组偏移查表
        0x41..=0x5A => {
            const ALPHA: [&str; 26] = [
                "KeyA", "KeyB", "KeyC", "KeyD","KeyE","KeyF","KeyG", "KeyH", 
                "KeyI", "KeyJ", "KeyK", "KeyL","KeyM", "KeyN", "KeyO", "KeyP", 
                "KeyQ", "KeyR","KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX",
                "KeyY", "KeyZ",
            ];
            ALPHA[(vkey - 0x41) as usize]
        }

        // 3. 0-9 (0x30 - 0x39)：范围匹配 + 数组偏移查表
        0x30..=0x39 => {
            const NUM: [&str; 10] = [
                "Num0", "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9",
            ];
            NUM[(vkey - 0x30) as usize]
        }

        // 4. F1-F12 (0x70 - 0x7B)：同理
        0x70..=0x7B => {
            const FUNC: [&str; 12] = [
                "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
            ];
            FUNC[(vkey - 0x70) as usize]
        }

        // 5. 兜底
        _ => "Unknown",
    }
}