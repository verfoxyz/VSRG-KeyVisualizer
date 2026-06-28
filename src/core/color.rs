//! 颜色工具函数
//!
//! 提供十六进制颜色字符串与 Slint 颜色之间的转换。
//! 支持 #RGB、#RRGGBB、#RRGGBBAA 三种格式。

/// 默认按键颜色（当配置文件中颜色解析失败时使用）
const DEFAULT_KEY_COLOR: u32 = 0x4A90E2;

/// 辅助：将单个 hex 字符重复两次组成一个字节（如 'A' → 0xAA）
fn hex_dup(c: u8) -> u8 {
    let v = (c as char).to_digit(16).unwrap_or(0) as u8;
    v * 16 + v
}

/// 将十六进制颜色字符串解析为 slint::Color
/// 支持 #RGB、#RRGGBB、#RRGGBBAA 三种格式
pub fn hex_str_to_color(hex_str: &str) -> slint::Color {
    let clean: String = hex_str
        .trim_start_matches('#')
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();

    let (r, g, b, a) = match clean.len() {
        // #RGB -> #RRGGBB（每位重复）
        3 => {
            let b = clean.as_bytes();
            (hex_dup(b[0]), hex_dup(b[1]), hex_dup(b[2]), 255)
        }
        // #RRGGBB
        6 => (
            u8::from_str_radix(&clean[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&clean[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&clean[4..6], 16).unwrap_or(0),
            255,
        ),
        // #RRGGBBAA（取前 8 位）
        8.. => (
            u8::from_str_radix(&clean[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&clean[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&clean[4..6], 16).unwrap_or(0),
            u8::from_str_radix(&clean[6..8], 16).unwrap_or(255),
        ),
        // 输入无效或为空 → 返回默认颜色（不透明）
        _ => (
            ((DEFAULT_KEY_COLOR >> 16) & 0xFF) as u8,
            ((DEFAULT_KEY_COLOR >> 8) & 0xFF) as u8,
            (DEFAULT_KEY_COLOR & 0xFF) as u8,
            255,
        ),
    };

    slint::Color::from_argb_encoded(
        ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32),
    )
}

/// 将 #RRGGBB 和透明度百分比(0-100)合并为 #RRGGBBAA
pub fn merge_alpha(hex_rgb: &str, opacity_pct: i32) -> String {
    let clean: String = hex_rgb
        .trim_start_matches('#')
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    // 取前 6 位 hex，不足右补 '0'
    let hex6 = if clean.len() >= 6 {
        clean[..6].to_string()
    } else {
        format!("{:0<6}", clean)
    };
    let a = ((opacity_pct.clamp(1, 100) * 255) / 100).clamp(1, 255);
    format!("#{}{:02X}", hex6, a)
}

/// 从 #RRGGBBAA 中提取 #RRGGBB 和透明度百分比(0-100)
pub fn split_alpha(hex_with_alpha: &str) -> (String, i32) {
    let clean: String = hex_with_alpha
        .trim_start_matches('#')
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    let len = clean.len();
    if len >= 8 {
        // 完整 #RRGGBBAA
        let rgb = format!("#{}", &clean[..6]);
        let a = u8::from_str_radix(&clean[6..8], 16).unwrap_or(255);
        let pct = ((a as i32) * 100 / 255).clamp(0, 100);
        (rgb, pct)
    } else if len >= 6 {
        // 只有 #RRGGBB，无透明度
        (format!("#{}", &clean[..6]), 100)
    } else {
        ("#333333".into(), 100)
    }
}
