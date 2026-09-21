// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic_config::{
    Config, CosmicConfigEntry, Error, cosmic_config_derive::CosmicConfigEntry,
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
    pub dim_minimized_window_icons: bool,
    pub highlight_maximized_window_icons: bool,
    pub show_one_icon_per_application: bool,
    pub max_visible_icons: u8,
    pub pill_style: WorkspacePillStyle,
    pub pill_border_width: u8,
    pub pill_spacing_percent: u8,
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
        }
    }
}

impl WorkspacesAppletConfig {
    pub fn load(config: &Config) -> (Self, Vec<Error>) {
        match Self::get_entry(config) {
            Ok(entry) => (entry, Vec::new()),
            Err((mut errors, entry)) => {
                errors.retain(Error::is_err);
                (entry, errors)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        APP_ID, DEFAULT_PILL_BORDER_WIDTH,
        DEFAULT_VISIBLE_ICONS, WorkspacePillStyle,
        WorkspacesAppletConfig,
    };
    use cosmic_config::{Config, CosmicConfigEntry};

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
}
