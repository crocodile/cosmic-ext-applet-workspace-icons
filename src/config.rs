// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic_config::{CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "io.github.crocodile.cosmic-ext-applet-workspace-icons";
pub const MAX_PILL_SPACING_PERCENT: u8 = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, CosmicConfigEntry)]
#[version = 2]
#[serde(default)]
pub struct WorkspacesAppletConfig {
    pub dim_minimized_window_icons: bool,
    pub highlight_maximized_window_icons: bool,
    pub pill_spacing_percent: u8,
}

impl Default for WorkspacesAppletConfig {
    fn default() -> Self {
        Self {
            dim_minimized_window_icons: true,
            highlight_maximized_window_icons: true,
            pill_spacing_percent: 0,
        }
    }
}
