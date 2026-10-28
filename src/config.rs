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
pub const MIN_INACTIVE_PILL_BRIGHTNESS_PERCENT: i8 = -75;
pub const MAX_INACTIVE_PILL_BRIGHTNESS_PERCENT: i8 = 75;
pub const INACTIVE_PILL_BRIGHTNESS_STEP: i8 = 5;
pub const MIN_VISIBLE_ICONS: u8 = 1;
pub const DEFAULT_VISIBLE_ICONS: u8 = 5;
pub const MAX_VISIBLE_ICONS: u8 = 16;
const INACTIVE_PILL_STYLE_KEY: &str = "inactive_pill_style";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkspacePillStyle {
    #[default]
    Filled,
    Outlined,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, CosmicConfigEntry)]
#[version = 4]
#[serde(default)]
pub struct WorkspacesAppletConfig {
    pub show_workspace_numbers: bool,
    pub show_app_icons: bool,
    pub dim_minimized_window_icons: bool,
    pub highlight_maximized_window_icons: bool,
    pub show_one_icon_per_application: bool,
    pub max_visible_icons: u8,
    pub pill_style: WorkspacePillStyle,
    pub inactive_pill_style: WorkspacePillStyle,
    pub pill_border_width: u8,
    pub pill_spacing_percent: u8,
    pub inactive_pill_brightness_percent: i8,
}

impl Default for WorkspacesAppletConfig {
    fn default() -> Self {
        Self {
            show_workspace_numbers: true,
            show_app_icons: true,
            dim_minimized_window_icons: true,
            highlight_maximized_window_icons: true,
            show_one_icon_per_application: true,
            max_visible_icons: DEFAULT_VISIBLE_ICONS,
            pill_style: WorkspacePillStyle::Filled,
            inactive_pill_style: WorkspacePillStyle::Filled,
            pill_border_width: DEFAULT_PILL_BORDER_WIDTH,
            pill_spacing_percent: 0,
            inactive_pill_brightness_percent: 0,
        }
    }
}

impl WorkspacesAppletConfig {
    pub fn load(config: &Config) -> (Self, Vec<Error>) {
        let (mut entry, mut errors) = match Self::get_entry(config) {
            Ok(entry) => (entry, Vec::new()),
            Err((errors, entry)) => (entry, errors),
        };

        let needs_pill_style_migration = matches!(
            config.get::<WorkspacePillStyle>(INACTIVE_PILL_STYLE_KEY),
            Err(Error::NotFound | Error::NoConfigDirectory)
        );

        if needs_pill_style_migration {
            entry.inactive_pill_style = entry.pill_style;
            if let Err(error) = entry.write_entry(config) {
                errors.push(error);
            }
        }

        errors.retain(Error::is_err);
        (entry, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        APP_ID, DEFAULT_PILL_BORDER_WIDTH, DEFAULT_VISIBLE_ICONS, WorkspacePillStyle,
        WorkspacesAppletConfig,
    };
    use cosmic_config::{Config, ConfigGet, ConfigSet, CosmicConfigEntry};

    #[test]
    fn preserves_number_visibility_when_loading_old_settings() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("config");
        config.set("max_visible_icons", 8_u8).unwrap();

        let (loaded, errors) = WorkspacesAppletConfig::load(&config);

        assert!(errors.is_empty());
        assert!(loaded.show_workspace_numbers);
        assert_eq!(loaded.max_visible_icons, 8);
        let deserialized: WorkspacesAppletConfig = serde_json::from_str("{}").unwrap();
        assert!(deserialized.show_workspace_numbers);
    }

    #[test]
    fn persists_both_number_visibility_settings() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("config");
        for enabled in [false, true] {
            let entry = WorkspacesAppletConfig {
                show_workspace_numbers: enabled,
                ..Default::default()
            };
            entry.write_entry(&config).unwrap();
            let (loaded, errors) = WorkspacesAppletConfig::load(&config);
            assert!(errors.is_empty());
            assert_eq!(loaded.show_workspace_numbers, enabled);
        }
    }

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
    fn uses_zero_inactive_pill_brightness_by_default() {
        assert_eq!(
            WorkspacesAppletConfig::default().inactive_pill_brightness_percent,
            0
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
    fn loads_an_older_cosmic_config_with_application_grouping_enabled() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("version three config");

        let (loaded, errors) = WorkspacesAppletConfig::load(&config);

        assert!(errors.is_empty());
        assert!(loaded.show_one_icon_per_application);
        assert_eq!(loaded.max_visible_icons, DEFAULT_VISIBLE_ICONS);
    }

    #[test]
    fn migrates_an_existing_outlined_style_to_both_pills() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("version three config");
        config
            .set("pill_style", WorkspacePillStyle::Outlined)
            .expect("existing outlined style");

        let (loaded, errors) = WorkspacesAppletConfig::load(&config);

        assert!(errors.is_empty());
        assert_eq!(loaded.pill_style, WorkspacePillStyle::Outlined);
        assert_eq!(loaded.inactive_pill_style, WorkspacePillStyle::Outlined);
        assert_eq!(
            config
                .get_local::<WorkspacePillStyle>("inactive_pill_style")
                .expect("migrated inactive style"),
            WorkspacePillStyle::Outlined
        );
    }

    #[test]
    fn preserves_an_explicit_mixed_style() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("version three config");
        config
            .set("pill_style", WorkspacePillStyle::Filled)
            .expect("active style");
        config
            .set("inactive_pill_style", WorkspacePillStyle::Outlined)
            .expect("inactive style");

        let (loaded, errors) = WorkspacesAppletConfig::load(&config);

        assert!(errors.is_empty());
        assert_eq!(loaded.pill_style, WorkspacePillStyle::Filled);
        assert_eq!(loaded.inactive_pill_style, WorkspacePillStyle::Outlined);
    }

    #[test]
    fn retains_version_three_settings_but_resets_old_contrast_to_zero_brightness() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let old_config = Config::with_custom_path(APP_ID, 3, directory.path().to_path_buf())
            .expect("version three config");
        old_config
            .set("dim_minimized_window_icons", false)
            .expect("old dimming preference");
        old_config
            .set("max_visible_icons", 8_u8)
            .expect("old icon limit");
        old_config
            .set("pill_style", WorkspacePillStyle::Filled)
            .expect("old active style");
        old_config
            .set("inactive_pill_style", WorkspacePillStyle::Outlined)
            .expect("old inactive style");
        old_config
            .set("inactive_pill_contrast_percent", 90_u8)
            .expect("old contrast preference");

        let new_config = Config::with_custom_path(
            APP_ID,
            WorkspacesAppletConfig::VERSION,
            directory.path().to_path_buf(),
        )
        .expect("version four config");
        let (loaded, errors) = WorkspacesAppletConfig::load(&new_config);

        assert!(errors.is_empty());
        assert!(!loaded.dim_minimized_window_icons);
        assert_eq!(loaded.max_visible_icons, 8);
        assert_eq!(loaded.pill_style, WorkspacePillStyle::Filled);
        assert_eq!(loaded.inactive_pill_style, WorkspacePillStyle::Outlined);
        assert_eq!(loaded.inactive_pill_brightness_percent, 0);
    }
}
