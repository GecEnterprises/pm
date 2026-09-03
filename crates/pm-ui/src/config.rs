//! The live user config: a gpui global wrapping [`pm_core::Config`].
//!
//! Loaded once at startup ([`ConfigStore::init`]). Read it with
//! [`ConfigStore::get`]; change it with [`ConfigStore::update`], which persists
//! to `~/.pm/config.json` and repaints every window. Modelled on Zed's
//! `ThemeSettings` global + `update_settings_file`.
//!
//! When the config can't be loaded or written, the store keeps working from
//! memory and stashes a one-shot [`take_alert`](ConfigStore::take_alert)
//! message that the view layer shows in a native dialog.

use gpui::{App, Global, PromptLevel, Window};

use pm_core::Config;

/// How durable this session's config is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Persistence {
    /// Reads and writes both work.
    Live,
    /// Read fine, but writes fail — session changes are in-memory only.
    ReadOnly,
    /// Couldn't load the config at all — running on defaults.
    Failed,
}

pub struct ConfigStore {
    config: Config,
    persistence: Persistence,
    /// `(title, detail)` for a native dialog the app should show once.
    alert: Option<(String, String)>,
}

impl Global for ConfigStore {}

fn config_path_str() -> String {
    pm_core::config::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.pm/config.json".to_string())
}

impl ConfigStore {
    /// Load `~/.pm/config.json` into the global. Call once, before opening
    /// windows.
    pub fn init(cx: &mut App) {
        let path_str = config_path_str();

        let (config, persistence, alert) = match Config::try_load() {
            Err(why) => (
                Config::default(),
                Persistence::Failed,
                Some((
                    "pm failed to initialize config".to_string(),
                    format!(
                        "{why}.\n\npm is starting with default settings. Changes you make this \
                         session apply in memory only and will not be saved to {path_str}."
                    ),
                )),
            ),
            Ok(config) => {
                if Self::config_writable(&config) {
                    (config, Persistence::Live, None)
                } else {
                    (
                        config,
                        Persistence::ReadOnly,
                        Some((
                            "pm can't save its config".to_string(),
                            format!(
                                "pm read your configuration, but it can't write to {path_str}.\n\n\
                                 Settings you change this session apply in memory only and will \
                                 be lost on restart."
                            ),
                        )),
                    )
                }
            }
        };

        eprintln!(
            "pm: config {path_str} (ui_scale {}, {})",
            config.ui_scale(),
            match persistence {
                Persistence::Live => "persistent",
                Persistence::ReadOnly => "read-only",
                Persistence::Failed => "defaults",
            }
        );
        cx.set_global(ConfigStore { config, persistence, alert });
    }

    /// Can we write the config file? Probes without truncating an existing file;
    /// creates it on first run.
    fn config_writable(config: &Config) -> bool {
        let Some(path) = pm_core::config::config_path() else {
            return false;
        };
        if path.exists() {
            std::fs::OpenOptions::new().append(true).open(&path).is_ok()
        } else {
            config.save_to(&path).is_ok()
        }
    }

    /// The current config. Falls back to defaults if [`init`](Self::init) never ran.
    pub fn get(cx: &App) -> Config {
        cx.try_global::<ConfigStore>()
            .map(|s| s.config.clone())
            .unwrap_or_default()
    }

    /// Mutate the config, persist it if we can, and repaint all windows.
    pub fn update(cx: &mut App, f: impl FnOnce(&mut Config)) {
        if !cx.has_global::<ConfigStore>() {
            cx.set_global(ConfigStore {
                config: Config::default(),
                persistence: Persistence::Live,
                alert: None,
            });
        }
        let store = cx.global_mut::<ConfigStore>();
        f(&mut store.config);
        if store.persistence == Persistence::Live {
            if let Err(e) = store.config.save() {
                store.persistence = Persistence::ReadOnly;
                store.alert = Some((
                    "pm can't save its config".to_string(),
                    format!(
                        "Writing {} failed ({e}).\n\nSettings you change now apply in memory only \
                         and will be lost on restart.",
                        config_path_str()
                    ),
                ));
            }
        }
        cx.refresh_windows();
    }

    /// A native-dialog message the app should present, consumed on read.
    pub fn take_alert(cx: &mut App) -> Option<(String, String)> {
        if cx.has_global::<ConfigStore>() {
            cx.global_mut::<ConfigStore>().alert.take()
        } else {
            None
        }
    }
}

/// Show any pending config alert in a native dialog. Call once per window,
/// after it's created; it's a one-shot and self-clears.
pub fn present_config_alert(window: &mut Window, cx: &mut App) {
    if let Some((title, detail)) = ConfigStore::take_alert(cx) {
        let recv = window.prompt(PromptLevel::Warning, &title, Some(&detail), &["OK"], cx);
        // The platform runs the dialog from its own task; we don't need the
        // clicked-button index, so drop the receiver explicitly.
        drop(recv);
    }
}
