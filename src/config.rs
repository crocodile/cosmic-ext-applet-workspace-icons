// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic_config::{CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "io.github.crocodile.cosmic-ext-applet-workspace-icons";
pub const MIN_PILL_BORDER_WIDTH: u8 = 0;
pub const DEFAULT_PILL_BORDER_WIDTH: u8 = 2;
pub const MAX_PILL_BORDER_WIDTH: u8 = 3;
pub const MAX_PILL_SPACING_PERCENT: u8 = 10;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkspacePillStyle {
    #[default]
    Filled,
    Outlined,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, CosmicConfigEntry)]
#[version = 2]
#[serde(default)]
pub struct WorkspacesAppletConfig {
    pub dim_minimized_window_icons: bool,
    pub highlight_maximized_window_icons: bool,
    pub pill_style: WorkspacePillStyle,
    pub pill_border_width: u8,
    pub pill_spacing_percent: u8,
}

impl Default for WorkspacesAppletConfig {
    fn default() -> Self {
        Self {
            dim_minimized_window_icons: true,
            highlight_maximized_window_icons: true,
            pill_style: WorkspacePillStyle::Filled,
            pill_border_width: DEFAULT_PILL_BORDER_WIDTH,
            pill_spacing_percent: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PILL_BORDER_WIDTH, WorkspacePillStyle, WorkspacesAppletConfig};

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
}
