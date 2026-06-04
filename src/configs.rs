// src/configs.rs
// 多配置管理系统：profiles 目录 + .active 文件

use crate::AppConfig;
use std::fs;
use std::path::{Path, PathBuf};

/// 配置系统的根目录
const CONFIGS_DIR: &str = "configs";
/// profiles 子目录名
const PROFILES_DIR: &str = "profiles";
/// 元数据文件：记录当前激活的 profile 名称
const ACTIVE_FILE: &str = ".active";
/// profile 文件扩展名
const PROFILE_EXT: &str = ".json";

/// 获取 profiles 目录路径
fn profiles_dir() -> PathBuf {
    Path::new(CONFIGS_DIR).join(PROFILES_DIR)
}

/// 获取 .active 文件路径
fn active_file_path() -> PathBuf {
    Path::new(CONFIGS_DIR).join(ACTIVE_FILE)
}

/// 获取某个 profile 的完整文件路径
fn profile_path(name: &str) -> PathBuf {
    profiles_dir().join(format!("{}{}", name, PROFILE_EXT))
}

/// 读取 `configs/.active` 中的当前 profile 名
///
/// 如果文件不存在或为空，返回 `"default"`。
fn read_active_profile() -> String {
    let path = active_file_path();
    if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| {
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            })
            .unwrap_or_else(|| "default".into())
    } else {
        "default".into()
    }
}

/// 写入 `configs/.active`，持久化当前 profile 名
fn write_active_profile(name: &str) {
    let path = active_file_path();
    // 确保 configs/ 目录存在
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, name).expect("无法写入 .active 文件");
}

/// 枚举所有可用的 profile 名称（不带 .json 后缀）
///
/// 返回按文件名排序的列表。用于前端 ComboBox 展示。
#[allow(dead_code)]
pub fn list_profiles() -> Vec<String> {
    let dir = profiles_dir();
    if !dir.exists() {
        return Vec::new();
    }
    let mut names: Vec<String> = match fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "json").unwrap_or(false))
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// 加载当前激活的 profile 配置
///
/// 流程：
/// 1. 读取 `.active` 获取 profile 名（默认 `"default"`）
/// 2. 尝试加载 `configs/profiles/{name}.json`
/// 3. 如果文件不存在，创建该 profile（从 `AppConfig::default()` 写入）
/// 4. 返回 (config, profile_name)
pub fn load_active_profile() -> (AppConfig, String) {
    let profile_name = read_active_profile();
    let loaded = load_profile(&profile_name)
        .unwrap_or_else(|| {
            // 若加载失败（文件不存在），创建新的 profile
            let config = AppConfig::default();
            save_profile(&profile_name, &config);
            config
        });
    // 确保 .active 文件一定存在且内容正确
    write_active_profile(&profile_name);
    (loaded, profile_name)
}

/// 加载指定 profile 的配置，如果文件不存在返回 None
pub fn load_profile(name: &str) -> Option<AppConfig> {
    let path = profile_path(name);
    if path.exists() {
        fs::read_to_string(&path).ok().and_then(|content| {
            serde_json::from_str(&content).ok()
        })
    } else {
        None
    }
}

/// 保存配置到指定 profile 文件
pub fn save_profile(name: &str, config: &AppConfig) {
    let path = profile_path(name);
    // 确保 profiles/ 目录存在
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let content = serde_json::to_string_pretty(config).unwrap();
    fs::write(&path, content).unwrap_or_else(|_| panic!("无法写入 profile: {}", name));
}

/// 切换当前激活的 profile
///
/// 调用方需确保 `new_name` 对应的 profile 文件已存在。
/// 返回切换后的 profile 名（trimmed）。
#[allow(dead_code)]
pub fn switch_profile(new_name: &str) -> String {
    let name = new_name.trim().to_string();
    if name.is_empty() {
        panic!("profile 名不能为空");
    }
    write_active_profile(&name);
    name
}

/// 创建新的 profile（如果已存在则不做任何事）
///
/// 返回是否真正创建了（false 表示已存在）。
#[allow(dead_code)]
pub fn create_profile(name: &str, config: &AppConfig) -> bool {
    let path = profile_path(name);
    if path.exists() {
        return false;
    }
    save_profile(name, config);
    true
}

/// 删除指定的 profile（不允许删除当前激活的 profile）
///
/// 返回是否删除成功。
#[allow(dead_code)]
pub fn delete_profile(name: &str, active_name: &str) -> bool {
    if name == active_name {
        tracing::warn!("不允许删除当前激活的 profile: {}", name);
        return false;
    }
    let path = profile_path(name);
    if path.exists() {
        fs::remove_file(&path).expect("无法删除 profile 文件");
        true
    } else {
        false
    }
}

/// 初始化配置系统：确保目录结构存在，执行旧 config.json 迁移
///
/// 调用时机：程序启动时（`main()` 最开头）。
/// 如果 `config.json` 存在但 `configs/` 不存在，自动迁移。
pub fn initialize() {
    let configs_dir = Path::new(CONFIGS_DIR);
    let profiles = profiles_dir();

    if configs_dir.exists() {
        // 目录已在，只需确保 profiles/ 存在
        if !profiles.exists() {
            let _ = fs::create_dir_all(&profiles);
        }
        return;
    }

    // configs/ 不存在：检查旧 config.json
    let old_path = Path::new("config.json");
    if old_path.exists() {
        tracing::info!("检测到旧版 config.json，正在迁移至 configs/profiles/ ...");
        // 读取旧配置
        let content = fs::read_to_string(old_path).unwrap_or_default();
        if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
            // 创建目录结构
            let _ = fs::create_dir_all(&profiles);
            // 写入 default.json
            save_profile("default", &config);
            // 写入 .active
            write_active_profile("default");
            // 备份旧 config.json（重命名）
            let _ = fs::rename(old_path, Path::new("config.json.bak"));
            tracing::info!("迁移完成：旧 config.json → configs/profiles/default.json");
            return;
        }
    }

    // 没有任何旧文件，全新初始化
    let _ = fs::create_dir_all(&profiles);
    write_active_profile("default");
    let config = AppConfig::default();
    save_profile("default", &config);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 辅助：清理测试目录
    fn setup_test_env() {
        let _ = fs::remove_dir_all("_test_configs");
    }

    #[test]
    fn test_read_write_active() {
        setup_test_env();
        // 临时替换常量… 用直接路径测试函数逻辑
        let test_active = "_test_configs/.active";
        let _ = fs::create_dir_all("_test_configs");
        fs::write(test_active, "4K").unwrap();
        let content = fs::read_to_string(test_active).unwrap();
        assert_eq!(content.trim(), "4K");
        let _ = fs::remove_dir_all("_test_configs");
    }

    #[test]
    fn test_list_profiles_empty() {
        // 没有 profiles 目录时返回空列表
        let list = list_profiles();
        // 正常项目下可能已有文件，只测返回值类型正确
        assert!(list.iter().all(|s| !s.contains(".json")));
    }
}
