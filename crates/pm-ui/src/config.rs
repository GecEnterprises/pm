//! The live user config: a gpui global wrapping [`pm_core::Config`].
//!
//! Loaded once at startup ([`ConfigStore::init`]). Read it with
//! [`ConfigStore::get`]; change it with [`ConfigStore::update`], which persists
//! to `~/.pm/config.json` and repaints every window. Modelled on Zed's
//! `ThemeSettings` global + `update_settings_file`.

use gpui::{App, Global};

use pm_core::Config;

pub struct ConfigStore {
    config: Config,
}

impl Global for ConfigStore {}

impl ConfigStore {
    /// Load `~/.pm/config.json` into the global. Call once, before opening
    /// windows. Writes the default file on first run so it's discoverable and
    /// hand-editable.
    pub fn init(cx: &mut App) {
        let config = Config::load();
        if pm_core::config::config_path().is_some_and(|p| !p.exists()) {
            let _ = config.save();
        }
        if let Some(p) = pm_core::config::config_path() {
            eprintln!("pm: config {} (ui_scale {})", p.display(), config.ui_scale());
        }
        cx.set_global(ConfigStore { config });
    }

    /// The current config. Falls back to defaults if [`init`](Self::init) never ran.
    pub fn get(cx: &App) -> Config {
        cx.try_global::<ConfigStore>()
            .map(|s| s.config.clone())
            .unwrap_or_default()
    }

    /// Mutate the config, persist it, and repaint all windows.
    pub fn update(cx: &mut App, f: impl FnOnce(&mut Config)) {
        if cx.try_global::<ConfigStore>().is_none() {
            cx.set_global(ConfigStore { config: Config::default() });
        }
        let store = cx.global_mut::<ConfigStore>();
        f(&mut store.config);
        let config = store.config.clone();
        if let Err(e) = config.save() {
            eprintln!("pm: saving config failed: {e}");
        }
        cx.refresh_windows();
    }
}
