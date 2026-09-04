//! Startup "is there a newer release?" check (PM-14).
//!
//! A gpui global, seeded to `Unknown` and flipped to `Available` by a background
//! task that hits the GitHub API once. The About overlay and the status bar read
//! it; nothing blocks on it and any failure (offline) is silent.

use gpui::{App, Global};

use pm_core::update::Release;

pub enum UpdateStatus {
    /// The check hasn't finished (or hasn't run).
    Unknown,
    /// A newer release is published.
    Available(Release),
}

impl Global for UpdateStatus {}

impl UpdateStatus {
    /// Seed the global and kick off the one-shot check. Call once at startup.
    pub fn init(cx: &mut App) {
        cx.set_global(UpdateStatus::Unknown);
        cx.spawn(async move |cx| {
            let found = cx
                .background_executor()
                .spawn(async {
                    pm_core::update::latest()
                        .ok()
                        .filter(Release::is_newer_than_current)
                })
                .await;
            if let Some(rel) = found {
                let _ = cx.update(|cx| {
                    *cx.global_mut::<UpdateStatus>() = UpdateStatus::Available(rel);
                    cx.refresh_windows();
                });
            }
        })
        .detach();
    }

    /// The available release, if the check found one.
    pub fn available(cx: &App) -> Option<&Release> {
        match cx.try_global::<UpdateStatus>() {
            Some(UpdateStatus::Available(rel)) => Some(rel),
            _ => None,
        }
    }
}
