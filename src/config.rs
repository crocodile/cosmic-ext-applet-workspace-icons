// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic_config::{
    Config, ConfigGet, CosmicConfigEntry, Error, cosmic_config_derive::CosmicConfigEntry,
};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "io.github.crocodile.cosmic-ext-applet-workspace-icons";
pub const MIN_PILL_BORDER_WIDTH: u8 = 0;
pub const DEFAULT_PILL_BORDER_WIDTH: u8 = 2;
pub const MAX_PILL_BORDER_WIDTH: u8 = 3;
pub const MAX_PILL_SPACING_PERCENT: u8 = 10;
pub const MIN_VISIBLE_ICONS: u8 = 1;
pub const DEFAULT_VISIBLE_ICONS: u8 = 5;
pub const MAX_VISIBLE_ICONS: u8 = 16;
pub const DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT: u8 = 25;
pub const MAX_INACTIVE_PILL_CONTRAST_PERCENT: u8 = 100;
pub const INACTIVE_PILL_CONTRAST_STEP_PERCENT: u8 = 5;

const LEGACY_INACTIVE_PILL_OPACITY_KEY: &str = "inactive_pill_opacity_percent";
const INACTIVE_PILL_CONTRAST_KEY: &str = "inactive_pill_contrast_percent";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkspacePillStyle {
    #[default]
    Filled,
    Outlined,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, CosmicConfigEntry)]
#[version = 3]
#[serde(default)]
pub struct WorkspacesAppletConfig {
    pub dim_minimized_window_icons: bool,
    pub highlight_maximized_window_icons: bool,
    pub show_one_icon_per_application: bool,
    pub max_visible_icons: u8,
    pub pill_style: WorkspacePillStyle,
    pub pill_border_width: u8,
    pub pill_spacing_percent: u8,
    pub inactive_pill_contrast_percent: u8,
    pub show_inactive_pill_background: bool,
}

impl Default for WorkspacesAppletConfig {
    fn default() -> Self {
        Self {
            dim_minimized_window_icons: true,
            highlight_maximized_window_icons: true,
            show_one_icon_per_application: true,
            max_visible_icons: DEFAULT_VISIBLE_ICONS,
            pill_style: WorkspacePillStyle::Filled,
            pill_border_width: DEFAULT_PILL_BORDER_WIDTH,
            pill_spacing_percent: 0,
            inactive_pill_contrast_percent: DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT,
            show_inactive_pill_background: true,
        }
    }
}

impl WorkspacesAppletConfig {
    pub fn load(config: &Config) -> (Self, Vec<Error>) {
        let (mut entry, mut errors) = match Self::get_entry(config) {
            Ok(entry) => (entry, Vec::new()),
            Err((errors, entry)) => (entry, errors),
        };

        let needs_migration = matches!(
            config.get::<u8>(INACTIVE_PILL_CONTRAST_KEY),
            Err(Error::NotFound | Error::NoConfigDirectory)
        );

        if needs_migration {
            match config.get_local::<u8>(LEGACY_INACTIVE_PILL_OPACITY_KEY) {
                Ok(percent) => {
                    entry.inactive_pill_contrast_percent =
                        percent.min(MAX_INACTIVE_PILL_CONTRAST_PERCENT);
                }
                Err(error) if error.is_err() => errors.push(error),
                Err(_) => {}
            }
        }

        let clamped_contrast = entry
            .inactive_pill_contrast_percent
            .min(MAX_INACTIVE_PILL_CONTRAST_PERCENT);
        let needs_clamping = clamped_contrast != entry.inactive_pill_contrast_percent;
        entry.inactive_pill_contrast_percent = clamped_contrast;

        if (needs_migration || needs_clamping)
            && let Err(error) = entry.write_entry(config)
        {
            errors.push(error);
        }

        errors.retain(Error::is_err);
        (entry, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        APP_ID, DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT, DEFAULT_PILL_BORDER_WIDTH,
        DEFAULT_VISIBLE_ICONS, MAX_INACTIVE_PILL_CONTRAST_PERCENT, WorkspacePillStyle,
        WorkspacesAppletConfig,
    };
    use cosmic_config::{Config, ConfigGet, ConfigSet, CosmicConfigEntry};

    #[test]
    fn uses_filled_pills_by_default() {
        assert_eq!(
            WorkspacesAppletConfig::default().pill_style,
            WorkspacePillStyle::Filled
        );
    }

    #[test]
    fn uses_a_two_pixel_pill_border_by_default() {
        assert_eq!(
            WorkspacesAppletConfig::default().pill_border_width,
            DEFAULT_PILL_BORDER_WIDTH
        );
    }

    #[test]
    fn uses_twenty_five_percent_inactive_pill_contrast_by_default() {
        assert_eq!(
            WorkspacesAppletConfig::default().inactive_pill_contrast_percent,
            DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT
        );
    }

    #[test]
    fn shows_one_icon_per_application_by_default() {
        assert!(WorkspacesAppletConfig::default().show_one_icon_per_application);
    }

    #[test]
    fn shows_five_icons_before_overflow_by_default() {
        assert_eq!(
            WorkspacesAppletConfig::default().max_visible_icons,
            DEFAULT_VISIBLE_ICONS
        );
    }

    #[test]
    fn supplies_the_default_contrast_when_deserializing_an_older_config() {
        let config: WorkspacesAppletConfig = serde_json::from_str(
            r#"{
                "dim_minimized_window_icons": true,
                "highlight_maximized_window_icons": true,
                "pill_style": "Filled",
                "pill_border_width": 2,
                "pill_spacing_percent": 0
            }"#,
        )
        .expect("an older config should deserialize using defaults for missing fields");

        assert_eq!(
            config.inactive_pill_contrast_percent,
            DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT
        );
        assert!(config.show_one_icon_per_application);
        assert_eq!(config.max_visible_icons, DEFAULT_VISIBLE_ICONS);
    }

    #[test]
    fn loads_an_older_cosmic_config_with_application_grouping_enabled() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("version three config");
        config
            .set(
                "inactive_pill_contrast_percent",
                DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT,
            )
            .expect("existing contrast setting");

        let (loaded, errors) = WorkspacesAppletConfig::load(&config);

        assert!(errors.is_empty());
        assert!(loaded.show_one_icon_per_application);
        assert_eq!(loaded.max_visible_icons, DEFAULT_VISIBLE_ICONS);
    }

    #[test]
    fn migrates_the_version_two_opacity_percentage_to_contrast() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let old_config = Config::with_custom_path(APP_ID, 2, directory.path().to_path_buf())
            .expect("version two config");
        old_config
            .set("inactive_pill_opacity_percent", 90_u8)
            .expect("legacy opacity setting");

        let new_config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("version three config");
        let (loaded, errors) = WorkspacesAppletConfig::load(&new_config);

        assert!(errors.is_empty());
        assert_eq!(loaded.inactive_pill_contrast_percent, 90);
        assert_eq!(
            new_config
                .get_local::<u8>("inactive_pill_contrast_percent")
                .expect("migrated contrast setting"),
            90
        );
    }

    #[test]
    fn initializes_missing_contrast_with_the_default() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("version three config");

        let (loaded, errors) = WorkspacesAppletConfig::load(&config);

        assert!(errors.is_empty());
        assert_eq!(
            loaded.inactive_pill_contrast_percent,
            DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT
        );
        assert_eq!(
            config
                .get_local::<u8>("inactive_pill_contrast_percent")
                .expect("initialized contrast setting"),
            DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT
        );
    }

    #[test]
    fn clamps_migrated_contrast_to_one_hundred_percent() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let old_config = Config::with_custom_path(APP_ID, 2, directory.path().to_path_buf())
            .expect("version two config");
        old_config
            .set("inactive_pill_opacity_percent", u8::MAX)
            .expect("legacy opacity setting");

        let new_config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("version three config");
        let (loaded, errors) = WorkspacesAppletConfig::load(&new_config);

        assert!(errors.is_empty());
        assert_eq!(
            loaded.inactive_pill_contrast_percent,
            MAX_INACTIVE_PILL_CONTRAST_PERCENT
        );
    }

    #[test]
    fn clamps_and_persists_invalid_version_three_contrast() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("version three config");
        config
            .set("inactive_pill_contrast_percent", u8::MAX)
            .expect("invalid contrast setting");

        let (loaded, errors) = WorkspacesAppletConfig::load(&config);

        assert!(errors.is_empty());
        assert_eq!(
            loaded.inactive_pill_contrast_percent,
            MAX_INACTIVE_PILL_CONTRAST_PERCENT
        );
        assert_eq!(
            config
                .get_local::<u8>("inactive_pill_contrast_percent")
                .expect("clamped contrast setting"),
            MAX_INACTIVE_PILL_CONTRAST_PERCENT
        );
    }
}
