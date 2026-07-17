// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cosmic_config::{CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "io.github.crocodile.cosmic-ext-applet-workspace-icons";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
#[serde(default)]
pub struct WorkspacesAppletConfig {
    pub dim_minimized_window_icons: bool,
    pub highlight_maximized_window_icons: bool,
    pub active_workspace_padding: u8,
}

impl Default for WorkspacesAppletConfig {
    fn default() -> Self {
        Self {
            dim_minimized_window_icons: true,
            highlight_maximized_window_icons: true,
            active_workspace_padding: 1,
        }
    }
}
