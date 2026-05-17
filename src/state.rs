// src/state.rs
use std::sync::{Arc, Mutex};
//use slint::ComponentHandle;
use crate::{AppConfig, BarNote, MainWindow, SettingsWindow, KeyCaptureDialog};

pub struct AppState {
    pub config: Arc<Mutex<AppConfig>>,
    pub temp_config: Arc<Mutex<AppConfig>>,
    pub active_notes: Arc<Mutex<Vec<BarNote>>>,
    pub capture_mode: Arc<Mutex<bool>>,
    pub dialog_holder: Arc<Mutex<Option<slint::Weak<KeyCaptureDialog>>>>,
    pub settings_holder: Arc<Mutex<Option<slint::Weak<SettingsWindow>>>>,
}

impl AppState {
    pub fn new(init_config: AppConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(init_config)),
            temp_config: Arc::new(Mutex::new(AppConfig::default())),
            active_notes: Arc::new(Mutex::new(Vec::new())),
            capture_mode: Arc::new(Mutex::new(false)),
            dialog_holder: Arc::new(Mutex::new(None)),
            settings_holder: Arc::new(Mutex::new(None)),
        }
    }
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            temp_config: Arc::clone(&self.temp_config),
            active_notes: Arc::clone(&self.active_notes),
            capture_mode: Arc::clone(&self.capture_mode),
            dialog_holder: Arc::clone(&self.dialog_holder),
            settings_holder: Arc::clone(&self.settings_holder),
        }
    }
}