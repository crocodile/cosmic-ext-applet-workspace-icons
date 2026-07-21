// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cctk::{
    cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1,
    sctk::reexports::{
        calloop::channel::SyncSender,
        protocols::ext::workspace::v1::client::ext_workspace_handle_v1::{
            self, ExtWorkspaceHandleV1,
        },
    },
    toplevel_info::ToplevelInfo,
    wayland_client::protocol::wl_output::WlOutput,
    workspace::Workspace,
};
use cosmic::{
    Element, Task, Theme, app,
    applet::{cosmic_panel_config::PanelAnchor, padded_control},
    cosmic_config::{Config, CosmicConfigEntry},
    desktop::{IconSourceExt, fde},
    iced::core::{Background, Border, Color},
    iced::{
        Alignment,
        Event::Mouse,
        Length, Limits, Padding, Subscription, event,
        mouse::{self, ScrollDelta},
        platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup},
        widget::{Image, Svg, button, column, row, space, stack},
        window,
    },
    scroll::DiscreteScrollState,
    surface, theme,
    theme::Container as ContainerClass,
    widget::{
        Id, autosize, container, divider, mouse_area, segmented_button, segmented_control, toggler,
    },
};

use crate::{
    config::{
        self, MAX_PILL_BORDER_WIDTH, MAX_PILL_SPACING_PERCENT, MIN_PILL_BORDER_WIDTH,
        WorkspacePillStyle, WorkspacesAppletConfig,
    },
    wayland::WorkspaceEvent,
    wayland_subscription::{WorkspacesUpdate, workspaces},
};

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    process::Command as ShellCommand,
    sync::LazyLock,
    time::Duration,
};

static AUTOSIZE_MAIN_ID: LazyLock<Id> = LazyLock::new(|| Id::new("autosize-main"));

const SCROLL_RATE_LIMIT: Duration = Duration::from_millis(200);
const MAX_VISIBLE_APPS: usize = 5;
const APP_ICON_SPACING: f32 = 4.0;
const APP_GROUP_LEADING_PADDING: f32 = 4.0;
const APP_GROUP_TRAILING_PADDING: f32 = 0.0;
const APP_GROUP_CROSS_AXIS_PADDING: f32 = 2.0;
const WORKSPACE_CONTENT_SPACING: f32 = 4.0;
const WORKSPACE_BUTTON_SPACING: f32 = 4.0;
const WORKSPACE_LIST_EDGE_PADDING: f32 = 2.0;
const WORKSPACE_LEADING_PADDING: f32 = 8.0;
const WORKSPACE_TRAILING_PADDING: f32 = 8.0;
const WORKSPACE_DIVIDER_WIDTH: f32 = 1.0;
const MINIMIZED_ICON_OPACITY: f32 = 0.45;
const MAXIMIZED_HIGHLIGHT_SCALE: f32 = 1.28;
const MAXIMIZED_ICON_GLOW_OPACITY: f32 = 0.24;
const INACTIVE_PILL_BACKGROUND_OPACITY: f32 = 0.55;
const INACTIVE_PILL_HOVER_BACKGROUND_OPACITY: f32 = 0.7;
const VERSION_TEXT_OPACITY: f32 = 0.45;
const XL_ICON_SIZE_THRESHOLD: f32 = 40.0;
const XL_WORKSPACE_NUMBER_FONT_SIZE: f32 = 33.0;
const DECREASE_ICON_SVG: &[u8] = br##"
<svg width="16" height="16" viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <rect x="3" y="7" width="10" height="2" rx="1" fill="#000"/>
</svg>
"##;
const INCREASE_ICON_SVG: &[u8] = br##"
<svg width="16" height="16" viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <rect x="3" y="7" width="10" height="2" rx="1" fill="#000"/>
  <rect x="7" y="3" width="2" height="10" rx="1" fill="#000"/>
</svg>
"##;

pub fn run() -> cosmic::iced::Result {
    cosmic::applet::run::<IcedWorkspacesApplet>(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Row,
    Column,
}

fn workspace_list_padding(layout: Layout) -> Padding {
    match layout {
        Layout::Row => Padding {
            right: WORKSPACE_LIST_EDGE_PADDING,
            left: WORKSPACE_LIST_EDGE_PADDING,
            ..Padding::ZERO
        },
        Layout::Column => Padding {
            top: WORKSPACE_LIST_EDGE_PADDING,
            bottom: WORKSPACE_LIST_EDGE_PADDING,
            ..Padding::ZERO
        },
    }
}

fn oriented_padding(layout: Layout, leading: f32, trailing: f32, cross_axis: f32) -> Padding {
    match layout {
        Layout::Row => Padding {
            top: cross_axis,
            right: trailing,
            bottom: cross_axis,
            left: leading,
        },
        Layout::Column => Padding {
            top: leading,
            right: cross_axis,
            bottom: trailing,
            left: cross_axis,
        },
    }
}

struct IcedWorkspacesApplet {
    core: cosmic::app::Core,
    workspaces: Vec<Workspace>,
    toplevels: Vec<ToplevelInfo>,
    output: Option<WlOutput>,
    locales: Vec<String>,
    desktop_entries: Vec<fde::DesktopEntry>,
    app_metadata: HashMap<String, AppMetadata>,
    workspace_tx: Option<SyncSender<WorkspaceEvent>>,
    layout: Layout,
    scroll: DiscreteScrollState,
    config: WorkspacesAppletConfig,
    config_helper: Option<Config>,
    pill_style_model: segmented_button::SingleSelectModel,
    popup: Option<window::Id>,
    hovered_workspace: Option<ExtWorkspaceHandleV1>,
}

struct AppMetadata {
    name: String,
    icon_source: fde::IconSource,
}

struct WorkspaceApp<'a> {
    app_id: &'a str,
    metadata: &'a AppMetadata,
    window_count: usize,
    minimized_count: usize,
    maximized_count: usize,
    minimized_titles: Vec<&'a str>,
}

impl WorkspaceApp<'_> {
    fn all_minimized(&self) -> bool {
        self.window_count > 0 && self.minimized_count == self.window_count
    }

    fn has_maximized(&self) -> bool {
        self.maximized_count > 0
    }
}

fn informative_titles<'a>(
    app_name: &str,
    titles: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    let app_name = app_name.trim();
    let mut informative = Vec::<&str>::new();

    for title in titles {
        let title = title.trim();
        if title.is_empty()
            || title.eq_ignore_ascii_case(app_name)
            || informative
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(title))
        {
            continue;
        }
        informative.push(title);
    }

    informative
}

fn pill_spacing_percent(value: u8) -> u8 {
    value.min(MAX_PILL_SPACING_PERCENT)
}

fn pill_border_width(value: u8) -> u8 {
    value.clamp(MIN_PILL_BORDER_WIDTH, MAX_PILL_BORDER_WIDTH)
}

fn occupied_number_section_major_size(base_size: f32, icon_size: f32) -> f32 {
    let preferred_size = (base_size * 0.65).max(20.0);
    preferred_size.min(icon_size.max(28.0))
}

fn workspace_number_font_size(icon_size: f32) -> Option<f32> {
    (icon_size >= XL_ICON_SIZE_THRESHOLD).then_some(XL_WORKSPACE_NUMBER_FONT_SIZE)
}

fn symbolic_svg_icon(bytes: &'static [u8]) -> cosmic::widget::icon::Handle {
    let mut handle = cosmic::widget::icon::from_svg_bytes(bytes);
    handle.symbolic = true;
    handle
}

fn pill_style_model(style: WorkspacePillStyle) -> segmented_button::SingleSelectModel {
    let mut model: segmented_button::SingleSelectModel = segmented_button::Model::builder()
        .insert(|button| {
            button
                .text(crate::fl!("pill-style-filled"))
                .data(WorkspacePillStyle::Filled)
        })
        .insert(|button| {
            button
                .text(crate::fl!("pill-style-outlined"))
                .data(WorkspacePillStyle::Outlined)
        })
        .build();
    model.activate_position(match style {
        WorkspacePillStyle::Filled => 0,
        WorkspacePillStyle::Outlined => 1,
    });
    model
}

impl IcedWorkspacesApplet {
    fn pill_border_width_stepper(&self) -> Element<'_, Message> {
        let value = self.config.pill_border_width;
        let decrement: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(DECREASE_ICON_SVG))
                .on_press_maybe(
                    (value > MIN_PILL_BORDER_WIDTH)
                        .then(|| Message::PillBorderWidth(value - 1)),
                )
                .into();
        let increment: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(INCREASE_ICON_SVG))
                .on_press_maybe(
                    (value < MAX_PILL_BORDER_WIDTH)
                        .then(|| Message::PillBorderWidth(value + 1)),
                )
                .into();
        let value = container(self.core.applet.text(format!("{value} px")).size(14))
            .center_x(Length::Fixed(48.0))
            .align_y(Alignment::Center);

        row![decrement, value, increment]
            .align_y(Alignment::Center)
            .into()
    }

    fn pill_spacing_stepper(&self) -> Element<'_, Message> {
        let value = self.config.pill_spacing_percent;
        let decrement: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(DECREASE_ICON_SVG))
                .on_press_maybe((value > 0).then(|| Message::PillSpacing(value - 1)))
                .into();
        let increment: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(INCREASE_ICON_SVG))
                .on_press_maybe(
                    (value < MAX_PILL_SPACING_PERCENT)
                        .then(|| Message::PillSpacing(value + 1)),
                )
                .into();
        let value = container(self.core.applet.text(format!("{value}%")).size(14))
            .center_x(Length::Fixed(48.0))
            .align_y(Alignment::Center);

        row![decrement, value, increment]
            .align_y(Alignment::Center)
            .into()
    }

    fn sync_pill_style_model(&mut self) {
        self.pill_style_model
            .activate_position(match self.config.pill_style {
                WorkspacePillStyle::Filled => 0,
                WorkspacePillStyle::Outlined => 1,
            });
    }

    fn workspace_pill_style(
        theme: &Theme,
        active: bool,
        urgent: bool,
        hovered: bool,
        outlined_mode: bool,
        outlined_border_width: f32,
    ) -> container::Style {
        let cosmic = theme.cosmic();
        let (background, text_color, border_color, border_width) = if active && outlined_mode {
            let component = &cosmic.accent_button;
            let border_color = Color::from(if hovered {
                component.hover
            } else {
                component.base
            });
            let background = hovered.then_some(Background::Color(border_color));
            (
                background,
                if hovered {
                    component.on.into()
                } else {
                    theme.current_container().component.on.into()
                },
                border_color,
                outlined_border_width,
            )
        } else if active {
            let component = &cosmic.accent_button;
            (
                Some(Background::Color(
                    if hovered {
                        component.hover
                    } else {
                        component.base
                    }
                    .into(),
                )),
                component.on.into(),
                Color::TRANSPARENT,
                0.0,
            )
        } else if !active && urgent {
            let color = Color::from(if hovered {
                theme.current_container().component.hover
            } else {
                cosmic.palette.neutral_3
            });
            (
                (!outlined_mode || hovered).then_some(Background::Color(color)),
                cosmic.destructive_button.base.into(),
                if outlined_mode {
                    color
                } else {
                    Color::TRANSPARENT
                },
                if outlined_mode {
                    outlined_border_width
                } else {
                    0.0
                },
            )
        } else {
            let component = &theme.current_container().component;
            let mut background = Color::from(component.hover);
            background.a = if hovered {
                INACTIVE_PILL_HOVER_BACKGROUND_OPACITY
            } else {
                INACTIVE_PILL_BACKGROUND_OPACITY
            };
            (
                (!outlined_mode || hovered).then_some(Background::Color(background)),
                component.on.into(),
                if outlined_mode {
                    background
                } else {
                    Color::TRANSPARENT
                },
                if outlined_mode {
                    outlined_border_width
                } else {
                    0.0
                },
            )
        };

        let border_color = if outlined_mode && hovered {
            match background.as_ref() {
                Some(Background::Color(color)) => *color,
                _ => border_color,
            }
        } else {
            border_color
        };

        container::Style {
            background,
            border: Border {
                color: border_color,
                width: border_width,
                radius: cosmic.radius_xl().into(),
                ..Default::default()
            },
            text_color: Some(text_color),
            icon_color: Some(text_color),
            ..Default::default()
        }
    }

    fn workspace_number_style(
        theme: &Theme,
        active: bool,
        outlined_mode: bool,
        hovered: bool,
    ) -> container::Style {
        if !active || !outlined_mode {
            return container::Style::default();
        }

        container::Style {
            text_color: Some(Self::outlined_active_foreground(theme, hovered).into()),
            ..Default::default()
        }
    }

    fn workspace_divider_style(
        theme: &Theme,
        active: bool,
        outlined_mode: bool,
        hovered: bool,
    ) -> container::Style {
        let color = if active && outlined_mode {
            Self::outlined_active_foreground(theme, hovered)
        } else {
            theme.current_container().divider.into()
        };

        container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        }
    }

    fn outlined_active_foreground(theme: &Theme, hovered: bool) -> Color {
        let cosmic = theme.cosmic();
        if hovered {
            cosmic.accent_button.on.into()
        } else {
            cosmic
                .accent_text
                .unwrap_or(cosmic.accent_button.base)
                .into()
        }
    }

    fn subtle_version_style(theme: &Theme) -> container::Style {
        let mut color = Color::from(theme.current_container().on);
        color.a *= VERSION_TEXT_OPACITY;

        container::Style {
            text_color: Some(color),
            ..Default::default()
        }
    }

    /// returns the index of the workspace button after which which must be moved to a popup
    /// if it exists.
    fn popup_index(&self) -> Option<usize> {
        let max_major_axis_len = self.core.applet.suggested_bounds.as_ref().map(|c| {
            // if we have a configure for width and height, we're in a overflow popup
            match self.core.applet.anchor {
                PanelAnchor::Top | PanelAnchor::Bottom => c.width as u32,
                PanelAnchor::Left | PanelAnchor::Right => c.height as u32,
            }
        })?;

        let mut used = WORKSPACE_LIST_EDGE_PADDING * 2.0;
        for (index, workspace) in self.workspaces.iter().enumerate() {
            if index > 0 {
                used += WORKSPACE_BUTTON_SPACING;
            }
            used += self.workspace_button_major_size(workspace);
            if used > max_major_axis_len as f32 {
                return Some(index.max(1));
            }
        }

        None
    }

    fn suggested_button_size(&self) -> f32 {
        (self.core.applet.suggested_size(true).0 + self.core.applet.suggested_padding(true).1 * 2)
            as f32
    }

    fn app_icon_size(&self) -> f32 {
        let window_size = self.core.applet.suggested_window_size();
        let cross_axis_size = if self.core.applet.is_horizontal() {
            window_size.1.get() as f32
        } else {
            window_size.0.get() as f32
        };

        (cross_axis_size * 0.52).max(16.0)
    }

    fn number_section_major_size(&self, has_apps: bool) -> f32 {
        let base_size = self.suggested_button_size();
        if has_apps {
            occupied_number_section_major_size(base_size, self.app_icon_size())
        } else {
            base_size
        }
    }

    fn app_icon_slot_size(icon_size: f32, highlighted: bool) -> f32 {
        if highlighted {
            icon_size * MAXIMIZED_HIGHLIGHT_SCALE
        } else {
            icon_size
        }
    }

    fn app_group_major_size(&self, apps: &[WorkspaceApp<'_>]) -> f32 {
        if apps.is_empty() {
            return 0.0;
        }

        let icon_size = self.app_icon_size();
        let visible_count = apps.len().min(MAX_VISIBLE_APPS);
        let visible_size = apps
            .iter()
            .take(visible_count)
            .map(|app| {
                Self::app_icon_slot_size(
                    icon_size,
                    self.config.highlight_maximized_window_icons && app.has_maximized(),
                )
            })
            .sum::<f32>();
        let overflow_size = if apps.len() > visible_count {
            self.app_icon_size() * 1.15 + APP_ICON_SPACING
        } else {
            0.0
        };

        visible_size
            + visible_count.saturating_sub(1) as f32 * APP_ICON_SPACING
            + overflow_size
            + APP_GROUP_LEADING_PADDING
            + APP_GROUP_TRAILING_PADDING
    }

    fn workspace_button_major_size(&self, workspace: &Workspace) -> f32 {
        let base_size = self.suggested_button_size();
        let apps = self.apps_for_workspace(workspace);
        if !apps.is_empty() {
            WORKSPACE_LEADING_PADDING
                + WORKSPACE_TRAILING_PADDING
                + self.number_section_major_size(true)
                + WORKSPACE_CONTENT_SPACING * 2.0
                + WORKSPACE_DIVIDER_WIDTH
                + self.app_group_major_size(&apps)
        } else {
            base_size
        }
    }

    fn update_desktop_entries(&mut self) {
        self.desktop_entries = fde::Iter::new(fde::default_paths())
            .filter_map(|path| fde::DesktopEntry::from_path(path, Some(&self.locales)).ok())
            .collect();
    }

    fn resolve_app_metadata(&mut self, app_id: &str) -> AppMetadata {
        let app_id_key = fde::unicase::Ascii::new(app_id);
        let mut desktop_entry = fde::find_app_by_id(&self.desktop_entries, app_id_key).cloned();

        if desktop_entry.is_none() {
            self.update_desktop_entries();
            desktop_entry = fde::find_app_by_id(&self.desktop_entries, app_id_key).cloned();
        }

        let desktop_entry =
            desktop_entry.unwrap_or_else(|| fde::DesktopEntry::from_appid(app_id.to_owned()));
        let name = desktop_entry
            .full_name(&self.locales)
            .unwrap_or(Cow::Borrowed(&desktop_entry.appid))
            .into_owned();
        let icon_source =
            fde::IconSource::from_unknown(desktop_entry.icon().unwrap_or(&desktop_entry.appid));

        AppMetadata { name, icon_source }
    }

    fn sync_app_metadata(&mut self) {
        let app_ids = self
            .toplevels
            .iter()
            .filter_map(|toplevel| (!toplevel.app_id.is_empty()).then_some(toplevel.app_id.clone()))
            .collect::<HashSet<_>>();

        self.app_metadata
            .retain(|app_id, _| app_ids.contains(app_id));

        for app_id in app_ids {
            if !self.app_metadata.contains_key(&app_id) {
                let metadata = self.resolve_app_metadata(&app_id);
                self.app_metadata.insert(app_id, metadata);
            }
        }
    }

    fn write_config(&self) {
        if let Some(helper) = &self.config_helper
            && let Err(err) = self.config.write_entry(helper)
        {
            tracing::error!(?err, "failed to write workspaces applet config");
        }
    }

    fn apps_for_workspace(&self, workspace: &Workspace) -> Vec<WorkspaceApp<'_>> {
        let mut apps = Vec::<WorkspaceApp<'_>>::new();

        for toplevel in &self.toplevels {
            if !toplevel.workspace.contains(&workspace.handle) {
                continue;
            }
            if let Some(output) = self.output.as_ref()
                && !toplevel.output.contains(output)
            {
                continue;
            }
            let Some(metadata) = self.app_metadata.get(&toplevel.app_id) else {
                continue;
            };
            let minimized = toplevel
                .state
                .contains(&zcosmic_toplevel_handle_v1::State::Minimized);
            let maximized = toplevel
                .state
                .contains(&zcosmic_toplevel_handle_v1::State::Maximized);

            if let Some(app) = apps.iter_mut().find(|app| app.app_id == toplevel.app_id) {
                app.window_count += 1;
                if minimized {
                    app.minimized_count += 1;
                    app.minimized_titles.push(&toplevel.title);
                }
                if maximized {
                    app.maximized_count += 1;
                }
            } else {
                apps.push(WorkspaceApp {
                    app_id: toplevel.app_id.as_str(),
                    metadata,
                    window_count: 1,
                    minimized_count: usize::from(minimized),
                    maximized_count: usize::from(maximized),
                    minimized_titles: minimized
                        .then_some(toplevel.title.as_str())
                        .into_iter()
                        .collect(),
                });
            }
        }

        apps
    }

    fn workspace_tooltip(&self, apps: &[WorkspaceApp<'_>]) -> String {
        let mut lines = Vec::new();

        for app in apps {
            let summary = if app.window_count > 1 {
                format!("{} ×{}", app.metadata.name, app.window_count)
            } else {
                app.metadata.name.clone()
            };
            if app.all_minimized() {
                lines.push(format!("{summary} (minimised)"));
            } else if app.minimized_count > 0 {
                lines.push(format!("{summary} ({} minimised)", app.minimized_count));
            } else {
                lines.push(summary);
            }

            lines.extend(
                informative_titles(&app.metadata.name, app.minimized_titles.iter().copied())
                    .into_iter()
                    .map(|title| {
                        if app.all_minimized() {
                            format!("  ↳ {title}")
                        } else {
                            format!("  ↳ {title} (minimised)")
                        }
                    }),
            );
        }

        lines.join("\n")
    }

    fn app_icon(
        &self,
        metadata: &AppMetadata,
        icon_size: f32,
        minimized: bool,
        maximized: bool,
    ) -> Element<'_, Message> {
        let opacity = if minimized {
            MINIMIZED_ICON_OPACITY
        } else {
            1.0
        };
        let handle = metadata.icon_source.as_cosmic_icon();
        let symbolic = handle.symbolic;
        let slot_size = Self::app_icon_slot_size(icon_size, maximized);

        let icon: Element<'_, Message> = match handle.data {
            cosmic::widget::icon::Data::Image(handle) => Image::new(handle)
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size))
                .opacity(opacity)
                .into(),
            cosmic::widget::icon::Data::Svg(handle) => Svg::<Theme>::new(handle)
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size))
                .symbolic(symbolic)
                .opacity(opacity)
                .into(),
        };

        if maximized {
            container(icon)
                .width(Length::Fixed(slot_size))
                .height(Length::Fixed(slot_size))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .class(ContainerClass::Custom(Box::new(move |_| {
                    let glow = Color {
                        a: MAXIMIZED_ICON_GLOW_OPACITY,
                        ..Color::WHITE
                    };

                    container::Style {
                        background: Some(Background::Color(glow)),
                        border: Border {
                            radius: (slot_size / 2.0).into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                })))
                .into()
        } else {
            icon
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        APP_GROUP_LEADING_PADDING, APP_GROUP_TRAILING_PADDING, APP_ICON_SPACING, Background, Color,
        INACTIVE_PILL_BACKGROUND_OPACITY, INACTIVE_PILL_HOVER_BACKGROUND_OPACITY,
        IcedWorkspacesApplet, Layout, MAX_PILL_BORDER_WIDTH, MIN_PILL_BORDER_WIDTH, Theme,
        WORKSPACE_CONTENT_SPACING, WORKSPACE_LEADING_PADDING, WORKSPACE_LIST_EDGE_PADDING,
        WORKSPACE_TRAILING_PADDING, informative_titles, occupied_number_section_major_size,
        oriented_padding, pill_border_width, pill_spacing_percent, workspace_list_padding,
        workspace_number_font_size,
    };

    const TEST_OUTLINED_BORDER_WIDTH: f32 = 2.0;

    fn test_pill_style(
        theme: &Theme,
        active: bool,
        urgent: bool,
        hovered: bool,
        outlined_mode: bool,
    ) -> cosmic::widget::container::Style {
        IcedWorkspacesApplet::workspace_pill_style(
            theme,
            active,
            urgent,
            hovered,
            outlined_mode,
            TEST_OUTLINED_BORDER_WIDTH,
        )
    }

    #[test]
    fn applies_outer_spacing_along_the_panel_axis() {
        let horizontal = workspace_list_padding(Layout::Row);
        assert_eq!(horizontal.left, WORKSPACE_LIST_EDGE_PADDING);
        assert_eq!(horizontal.right, WORKSPACE_LIST_EDGE_PADDING);
        assert_eq!(horizontal.top, 0.0);
        assert_eq!(horizontal.bottom, 0.0);

        let vertical = workspace_list_padding(Layout::Column);
        assert_eq!(vertical.top, WORKSPACE_LIST_EDGE_PADDING);
        assert_eq!(vertical.bottom, WORKSPACE_LIST_EDGE_PADDING);
        assert_eq!(vertical.left, 0.0);
        assert_eq!(vertical.right, 0.0);
    }

    #[test]
    fn rotates_leading_trailing_and_cross_axis_padding() {
        let horizontal = oriented_padding(Layout::Row, 5.0, 8.0, 2.0);
        assert_eq!(horizontal.top, 2.0);
        assert_eq!(horizontal.right, 8.0);
        assert_eq!(horizontal.bottom, 2.0);
        assert_eq!(horizontal.left, 5.0);

        let vertical = oriented_padding(Layout::Column, 5.0, 8.0, 2.0);
        assert_eq!(vertical.top, 5.0);
        assert_eq!(vertical.right, 2.0);
        assert_eq!(vertical.bottom, 8.0);
        assert_eq!(vertical.left, 2.0);
    }

    #[test]
    fn uses_a_balanced_four_eight_pixel_workspace_rhythm() {
        assert_eq!(WORKSPACE_LEADING_PADDING, 8.0);
        assert_eq!(WORKSPACE_CONTENT_SPACING, 4.0);
        assert_eq!(WORKSPACE_CONTENT_SPACING + APP_GROUP_LEADING_PADDING, 8.0);
        assert_eq!(APP_ICON_SPACING, 4.0);
        assert_eq!(APP_GROUP_TRAILING_PADDING + WORKSPACE_TRAILING_PADDING, 8.0);
    }

    #[test]
    fn keeps_inactive_pill_background_when_not_hovered() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, false, false, false, false);

        let Some(Background::Color(background)) = style.background else {
            panic!("inactive pill should have a solid translucent background");
        };
        let mut expected = Color::from(theme.current_container().component.hover);
        expected.a = INACTIVE_PILL_BACKGROUND_OPACITY;
        assert_eq!(background, expected);
    }

    #[test]
    fn gently_emphasizes_inactive_pill_background_when_hovered() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, false, false, true, false);

        let Some(Background::Color(background)) = style.background else {
            panic!("hovered inactive pill should have a solid translucent background");
        };
        let mut expected = Color::from(theme.current_container().component.hover);
        expected.a = INACTIVE_PILL_HOVER_BACKGROUND_OPACITY;
        assert_eq!(background, expected);
    }

    #[test]
    fn outlines_inactive_pills_in_outlined_mode() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, false, false, false, true);

        let mut expected = Color::from(theme.current_container().component.hover);
        expected.a = INACTIVE_PILL_BACKGROUND_OPACITY;
        assert_eq!(style.background, None);
        assert_eq!(style.border.color, expected);
        assert_eq!(style.border.width, TEST_OUTLINED_BORDER_WIDTH);
    }

    #[test]
    fn uses_the_selected_width_for_outlined_pills() {
        let theme = Theme::default();

        for (active, urgent, hovered) in [
            (true, false, false),
            (false, false, false),
            (false, true, false),
            (true, false, true),
        ] {
            let style = IcedWorkspacesApplet::workspace_pill_style(
                &theme, active, urgent, hovered, true, 3.0,
            );
            assert_eq!(style.border.width, 3.0);
        }
    }

    #[test]
    fn fills_inactive_outlined_pills_on_hover() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, false, false, true, true);

        let mut expected = Color::from(theme.current_container().component.hover);
        expected.a = INACTIVE_PILL_HOVER_BACKGROUND_OPACITY;
        assert_eq!(style.background, Some(Background::Color(expected)));
        assert_eq!(style.border.color, expected);
        assert_eq!(style.border.width, TEST_OUTLINED_BORDER_WIDTH);
    }

    #[test]
    fn matches_each_hovered_outlined_border_to_its_own_background() {
        let theme = Theme::default();
        let styles = [
            test_pill_style(&theme, true, false, true, true),
            test_pill_style(&theme, false, false, true, true),
            test_pill_style(&theme, false, true, true, true),
        ];

        for style in styles {
            let Some(Background::Color(background)) = style.background else {
                panic!("hovered outlined pill should have a solid background");
            };
            assert_eq!(style.border.color, background);
        }
    }

    #[test]
    fn restores_the_full_opacity_active_pill_when_outlined_mode_is_disabled() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, true, false, false, false);

        assert_eq!(
            style.background,
            Some(Background::Color(theme.cosmic().accent_button.base.into()))
        );
        assert_eq!(style.border.width, 0.0);
    }

    #[test]
    fn keeps_the_full_opacity_active_pill_when_hovered() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, true, false, true, false);

        assert_eq!(
            style.background,
            Some(Background::Color(theme.cosmic().accent_button.hover.into()))
        );
        assert_eq!(style.border.width, 0.0);
    }

    #[test]
    fn uses_an_accent_outline_in_outlined_mode() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, true, false, false, true);

        assert_eq!(style.background, None);
        assert_eq!(style.border.color, theme.cosmic().accent_button.base.into());
        assert_eq!(style.border.width, TEST_OUTLINED_BORDER_WIDTH);
    }

    #[test]
    fn fills_the_active_outlined_pill_on_hover() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, true, false, true, true);

        let expected = Color::from(theme.cosmic().accent_button.hover);
        assert_eq!(style.background, Some(Background::Color(expected)));
        assert_eq!(style.border.color, theme.cosmic().accent_button.hover.into());
        assert_eq!(style.text_color, Some(theme.cosmic().accent_button.on.into()));
        assert_eq!(style.border.width, TEST_OUTLINED_BORDER_WIDTH);
    }

    #[test]
    fn gives_the_active_number_an_accent_text_color_in_outlined_mode() {
        let theme = Theme::default();
        let style = IcedWorkspacesApplet::workspace_number_style(&theme, true, true, false);
        let expected = theme
            .cosmic()
            .accent_text
            .unwrap_or(theme.cosmic().accent_button.base);

        assert_eq!(style.text_color, Some(expected.into()));
    }

    #[test]
    fn gives_the_active_number_on_accent_text_when_hovered() {
        let theme = Theme::default();
        let style = IcedWorkspacesApplet::workspace_number_style(&theme, true, true, true);

        assert_eq!(style.text_color, Some(theme.cosmic().accent_button.on.into()));
    }

    #[test]
    fn gives_the_active_divider_an_accent_color_in_outlined_mode() {
        let theme = Theme::default();
        let style = IcedWorkspacesApplet::workspace_divider_style(&theme, true, true, false);
        let expected = theme
            .cosmic()
            .accent_text
            .unwrap_or(theme.cosmic().accent_button.base);

        assert_eq!(style.background, Some(Background::Color(expected.into())));
    }

    #[test]
    fn gives_the_active_divider_an_on_accent_color_when_hovered() {
        let theme = Theme::default();
        let style = IcedWorkspacesApplet::workspace_divider_style(&theme, true, true, true);

        assert_eq!(
            style.background,
            Some(Background::Color(theme.cosmic().accent_button.on.into()))
        );
    }

    #[test]
    fn hides_titles_that_repeat_the_application_name() {
        assert!(informative_titles("Surfshark", ["Surfshark"]).is_empty());
        assert!(informative_titles("Surfshark", [" surfshark "]).is_empty());
    }

    #[test]
    fn retains_distinct_window_titles_without_duplicates() {
        assert_eq!(
            informative_titles(
                "COSMIC Text Editor",
                ["notes.txt", "NOTES.TXT", "", "README.md"]
            ),
            ["notes.txt", "README.md"]
        );
    }

    #[test]
    fn caps_pill_spacing_to_the_supported_range() {
        assert_eq!(pill_spacing_percent(0), 0);
        assert_eq!(pill_spacing_percent(10), 10);
        assert_eq!(pill_spacing_percent(u8::MAX), 10);
    }

    #[test]
    fn supports_zero_width_and_clamps_pill_border_width_to_the_maximum() {
        assert_eq!(pill_border_width(0), MIN_PILL_BORDER_WIDTH);
        assert_eq!(pill_border_width(2), 2);
        assert_eq!(pill_border_width(u8::MAX), MAX_PILL_BORDER_WIDTH);
    }

    #[test]
    fn gives_workspace_numbers_more_room_as_icons_grow() {
        let small = occupied_number_section_major_size(40.0, 20.8);
        let large = occupied_number_section_major_size(64.0, 33.28);
        let extra_large = occupied_number_section_major_size(80.0, 41.6);

        assert!((small - 26.0).abs() < 0.001);
        assert!((large - 33.28).abs() < 0.001);
        assert!((extra_large - 41.6).abs() < 0.001);
    }

    #[test]
    fn enlarges_workspace_numbers_only_at_xl_icon_sizes() {
        assert_eq!(workspace_number_font_size(33.28), None);
        assert_eq!(workspace_number_font_size(39.99), None);
        assert_eq!(workspace_number_font_size(40.0), Some(33.0));
        assert_eq!(workspace_number_font_size(41.6), Some(33.0));
    }
}

#[derive(Debug, Clone)]
enum Message {
    WorkspaceUpdate(WorkspacesUpdate),
    WorkspacePressed(ExtWorkspaceHandleV1),
    WheelScrolled(ScrollDelta),
    WorkspaceOverview,
    TogglePopup,
    PopupClosed(window::Id),
    DimMinimizedWindowIcons(bool),
    HighlightMaximizedWindowIcons(bool),
    PillStyle(segmented_button::Entity),
    PillBorderWidth(u8),
    PillSpacing(u8),
    WorkspaceHovered(ExtWorkspaceHandleV1),
    WorkspaceUnhovered(ExtWorkspaceHandleV1),
    WorkspaceHoverCleared,
    ConfigUpdated(WorkspacesAppletConfig),
    Surface(surface::Action),
}

impl cosmic::Application for IcedWorkspacesApplet {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &'static str = config::APP_ID;

    fn init(core: cosmic::app::Core, _flags: Self::Flags) -> (Self, app::Task<Self::Message>) {
        let config_helper = Config::new(config::APP_ID, WorkspacesAppletConfig::VERSION).ok();
        let mut config = config_helper
            .as_ref()
            .map(|helper| {
                WorkspacesAppletConfig::get_entry(helper).unwrap_or_else(|(errors, config)| {
                    for err in errors {
                        tracing::error!(?err, "failed to load workspaces applet config entry");
                    }
                    config
                })
            })
            .unwrap_or_default();
        config.pill_border_width = pill_border_width(config.pill_border_width);
        config.pill_spacing_percent = pill_spacing_percent(config.pill_spacing_percent);
        let pill_style_model = pill_style_model(config.pill_style);

        let mut app = Self {
            layout: match &core.applet.anchor {
                PanelAnchor::Left | PanelAnchor::Right => Layout::Column,
                PanelAnchor::Top | PanelAnchor::Bottom => Layout::Row,
            },
            core,
            workspaces: Vec::new(),
            toplevels: Vec::new(),
            output: None,
            locales: fde::get_languages_from_env(),
            desktop_entries: Vec::new(),
            app_metadata: HashMap::new(),
            workspace_tx: Option::default(),
            scroll: DiscreteScrollState::default().rate_limit(Some(SCROLL_RATE_LIMIT)),
            config,
            config_helper,
            pill_style_model,
            popup: None,
            hovered_workspace: None,
        };
        app.update_desktop_entries();

        (app, Task::none())
    }

    fn core(&self) -> &cosmic::app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::app::Core {
        &mut self.core
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::WorkspaceUpdate(msg) => match msg {
                WorkspacesUpdate::Snapshot(mut snapshot) => {
                    snapshot
                        .workspaces
                        .retain(|w| !w.state.contains(ext_workspace_handle_v1::State::Hidden));
                    snapshot
                        .workspaces
                        .sort_by(|w1, w2| w1.coordinates.cmp(&w2.coordinates));
                    self.workspaces = snapshot.workspaces;
                    self.toplevels = snapshot.toplevels;
                    self.output = snapshot.output;
                    self.sync_app_metadata();
                }
                WorkspacesUpdate::Started(tx) => {
                    self.workspace_tx.replace(tx);
                }
                WorkspacesUpdate::Errored => {
                    // TODO
                }
            },
            Message::WorkspacePressed(id) => {
                if let Some(tx) = self.workspace_tx.as_mut() {
                    let _ = tx.try_send(WorkspaceEvent::Activate(id));
                }
            }
            Message::WheelScrolled(delta) => {
                let discrete_delta = self.scroll.update(delta);
                if discrete_delta.y != 0
                    && let Some(w_i) = self
                        .workspaces
                        .iter()
                        .position(|w| w.state.contains(ext_workspace_handle_v1::State::Active))
                {
                    let d_i = (w_i as isize - discrete_delta.y)
                        .rem_euclid(self.workspaces.len() as isize)
                        as usize;

                    if let Some(tx) = self.workspace_tx.as_mut() {
                        let _ = tx.try_send(WorkspaceEvent::Activate(
                            self.workspaces[d_i].handle.clone(),
                        ));
                    }
                }
            }
            Message::WorkspaceOverview => {
                let _ = ShellCommand::new("cosmic-workspaces").spawn();
            }
            Message::TogglePopup => {
                return if let Some(popup) = self.popup.take() {
                    destroy_popup(popup)
                } else {
                    let popup = window::Id::unique();
                    self.popup.replace(popup);
                    let popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        popup,
                        Some((1, 1)),
                        None,
                        None,
                    );

                    get_popup(popup_settings)
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
            Message::DimMinimizedWindowIcons(enabled) => {
                self.config.dim_minimized_window_icons = enabled;
                self.write_config();
            }
            Message::HighlightMaximizedWindowIcons(enabled) => {
                self.config.highlight_maximized_window_icons = enabled;
                self.write_config();
            }
            Message::PillStyle(entity) => {
                if let Some(style) = self
                    .pill_style_model
                    .data::<WorkspacePillStyle>(entity)
                    .copied()
                {
                    self.pill_style_model.activate(entity);
                    self.config.pill_style = style;
                    self.write_config();
                }
            }
            Message::PillBorderWidth(width) => {
                self.config.pill_border_width = pill_border_width(width);
                self.write_config();
            }
            Message::PillSpacing(percent) => {
                self.config.pill_spacing_percent = pill_spacing_percent(percent);
                self.write_config();
            }
            Message::WorkspaceHovered(workspace) => {
                self.hovered_workspace = Some(workspace);
            }
            Message::WorkspaceUnhovered(workspace) => {
                if self.hovered_workspace.as_ref() == Some(&workspace) {
                    self.hovered_workspace = None;
                }
            }
            Message::WorkspaceHoverCleared => {
                self.hovered_workspace = None;
            }
            Message::ConfigUpdated(mut config) => {
                config.pill_border_width = pill_border_width(config.pill_border_width);
                config.pill_spacing_percent = pill_spacing_percent(config.pill_spacing_percent);
                self.config = config;
                self.sync_pill_style_model();
            }
            Message::Surface(a) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(a),
                ));
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.workspaces.is_empty() {
            return row![].padding(8).into();
        }
        let suggested_window_size = self.core.applet.suggested_window_size();
        let popup_index = self.popup_index().unwrap_or(self.workspaces.len());

        let buttons = self.workspaces[..popup_index].iter().map(|w| {
            let horizontal = self.core.applet.is_horizontal();
            let active = w.state.contains(ext_workspace_handle_v1::State::Active);
            let urgent = w.state.contains(ext_workspace_handle_v1::State::Urgent);
            let apps = self.apps_for_workspace(w);
            let major_size = self.workspace_button_major_size(w);
            let (width, height) = if horizontal {
                (major_size, suggested_window_size.1.get() as f32)
            } else {
                (suggested_window_size.0.get() as f32, major_size)
            };
            let outlined_mode = self.config.pill_style == WorkspacePillStyle::Outlined;
            let outlined_border_width = f32::from(self.config.pill_border_width);
            let hovered = self.hovered_workspace.as_ref() == Some(&w.handle);
            let pill_cross_axis_inset = if horizontal {
                height * f32::from(self.config.pill_spacing_percent) / 100.0
            } else {
                width * f32::from(self.config.pill_spacing_percent) / 100.0
            };

            let visible_app_count = apps.len().min(MAX_VISIBLE_APPS);
            let icon_size = self.app_icon_size();
            let mut icons = apps
                .iter()
                .take(visible_app_count)
                .map(|app| {
                    self.app_icon(
                        app.metadata,
                        icon_size,
                        self.config.dim_minimized_window_icons && app.all_minimized(),
                        self.config.highlight_maximized_window_icons
                            && app.has_maximized(),
                    )
                })
                .collect::<Vec<_>>();
            if apps.len() > visible_app_count {
                icons.push(
                    self.core
                        .applet
                        .text(format!("+{}", apps.len() - visible_app_count))
                        .size((icon_size * 0.55).max(10.0))
                        .into(),
                );
            }
            let app_strip: Element<'_, Message> = if horizontal {
                row(icons)
                    .spacing(APP_ICON_SPACING)
                    .align_y(Alignment::Center)
                    .into()
            } else {
                column(icons)
                    .spacing(APP_ICON_SPACING)
                    .align_x(Alignment::Center)
                    .into()
            };

            let number_section_size = self.number_section_major_size(!apps.is_empty());
            let number_text = self.core.applet.text(&w.name).font(cosmic::font::bold());
            let number_text = if let Some(font_size) = workspace_number_font_size(icon_size) {
                number_text.size(font_size)
            } else {
                number_text
            }
            .width(Length::Fill)
            .height(Length::Fill)
            .center();
            let number = container(number_text).class(ContainerClass::Custom(Box::new(
                move |theme| {
                    Self::workspace_number_style(theme, active, outlined_mode, hovered)
                },
            )));
            let number: Element<'_, Message> = if horizontal {
                number
                    .width(Length::Fixed(number_section_size))
                    .height(Length::Fill)
            } else {
                number
                    .width(Length::Fill)
                    .height(Length::Fixed(number_section_size))
            }
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();

            let content: Element<'_, Message> = if apps.is_empty() {
                number
            } else {
                let app_group: Element<'_, Message> = container(app_strip)
                    .class(ContainerClass::Custom(Box::new(|_| {
                        container::Style::default()
                    })))
                    .padding(oriented_padding(
                        self.layout,
                        APP_GROUP_LEADING_PADDING,
                        APP_GROUP_TRAILING_PADDING,
                        APP_GROUP_CROSS_AXIS_PADDING,
                    ))
                    .into();

                if horizontal {
                    let divider: Element<'_, Message> =
                        container(space::vertical().height(Length::Fixed(icon_size * 0.8)))
                            .width(Length::Fixed(WORKSPACE_DIVIDER_WIDTH))
                            .class(ContainerClass::Custom(Box::new(move |theme| {
                                Self::workspace_divider_style(
                                    theme,
                                    active,
                                    outlined_mode,
                                    hovered,
                                )
                            })))
                            .into();

                    row![number, divider, app_group]
                        .spacing(WORKSPACE_CONTENT_SPACING)
                        .align_y(Alignment::Center)
                        .into()
                } else {
                    let divider: Element<'_, Message> =
                        container(space::horizontal().width(Length::Fixed(icon_size * 0.8)))
                            .height(Length::Fixed(WORKSPACE_DIVIDER_WIDTH))
                            .class(ContainerClass::Custom(Box::new(move |theme| {
                                Self::workspace_divider_style(
                                    theme,
                                    active,
                                    outlined_mode,
                                    hovered,
                                )
                            })))
                            .into();

                    column![number, divider, app_group]
                        .spacing(WORKSPACE_CONTENT_SPACING)
                        .align_x(Alignment::Center)
                        .into()
                }
            };

            let has_apps = !apps.is_empty();
            let pill_background: Element<'_, Message> = container(
                container(space::horizontal())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .class(ContainerClass::Custom(Box::new(move |theme| {
                        Self::workspace_pill_style(
                            theme,
                            active,
                            urgent,
                            hovered,
                            outlined_mode,
                            outlined_border_width,
                        )
                    }))),
            )
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .padding(oriented_padding(
                self.layout,
                0.0,
                0.0,
                pill_cross_axis_inset,
            ))
            .into();

            let pill_content: Element<'_, Message> = container(content)
                .class(ContainerClass::Custom(Box::new(move |theme| {
                    let pill_style = Self::workspace_pill_style(
                        theme,
                        active,
                        urgent,
                        hovered,
                        outlined_mode,
                        outlined_border_width,
                    );
                    container::Style {
                        text_color: pill_style.text_color,
                        icon_color: pill_style.icon_color,
                        ..Default::default()
                    }
                })))
                .width(Length::Fixed(width))
                .height(Length::Fixed(height))
                .padding(if has_apps {
                    oriented_padding(
                        self.layout,
                        WORKSPACE_LEADING_PADDING,
                        WORKSPACE_TRAILING_PADDING,
                        0.0,
                    )
                } else {
                    Padding::ZERO
                })
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .into();

            let btn = button(
                stack![pill_background, pill_content]
                    .width(Length::Fixed(width))
                    .height(Length::Fixed(height)),
            )
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .on_press(
                if active {
                    Message::WorkspaceOverview
                } else {
                    Message::WorkspacePressed(w.handle.clone())
                },
            )
            .padding(0)
            .class(cosmic::theme::iced::Button::Transparent);

            let workspace_button: Element<'_, Message> = if has_apps {
                let tooltip = self.workspace_tooltip(&apps);
                self.core
                    .applet
                    .applet_tooltip(btn, tooltip, false, Message::Surface, None)
                    .into()
            } else {
                btn.into()
            };

            mouse_area(workspace_button)
                .on_enter(Message::WorkspaceHovered(w.handle.clone()))
                .on_exit(Message::WorkspaceUnhovered(w.handle.clone()))
                .on_right_press(Message::TogglePopup)
                .into()
        });
        // TODO if there is a popup_index, create a button with a popup for the remaining workspaces
        // Should it appear on hover or on click?
        let layout_section: Element<_> = match self.layout {
            Layout::Row => row(buttons).spacing(WORKSPACE_BUTTON_SPACING).into(),
            Layout::Column => column(buttons).spacing(WORKSPACE_BUTTON_SPACING).into(),
        };
        let mut limits = Limits::NONE.min_width(1.).min_height(1.);
        if let Some(b) = self.core.applet.suggested_bounds {
            if b.width as i32 > 0 {
                limits = limits.max_width(b.width);
            }
            if b.height as i32 > 0 {
                limits = limits.max_height(b.height);
            }
        }

        autosize::autosize(
            container(layout_section).padding(workspace_list_padding(self.layout)),
            AUTOSIZE_MAIN_ID.clone(),
        )
        .limits(limits)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            self.core
                .watch_config::<WorkspacesAppletConfig>(config::APP_ID)
                .map(|update| {
                    for err in update.errors {
                        tracing::error!(?err, "failed to load workspaces applet config update");
                    }
                    Message::ConfigUpdated(update.config)
                }),
            workspaces().map(Message::WorkspaceUpdate),
            event::listen_with(|e, _, _| match e {
                Mouse(mouse::Event::WheelScrolled { delta }) => Some(Message::WheelScrolled(delta)),
                Mouse(mouse::Event::CursorLeft) => Some(Message::WorkspaceHoverCleared),
                _ => None,
            }),
        ])
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Message> {
        let spacing = theme::active().cosmic().spacing;
        let version = container(
            self.core
                .applet
                .text(format!(
                    "{} {}",
                    crate::fl!("version"),
                    env!("CARGO_PKG_VERSION")
                ))
                .size(11),
        )
        .width(Length::Fill)
        .align_x(Alignment::End)
        .padding([
            spacing.space_xxs,
            spacing.space_m,
            spacing.space_xxs,
            spacing.space_m,
        ])
        .class(ContainerClass::Custom(Box::new(Self::subtle_version_style)));

        let outline_thickness: Element<'_, Message> =
            if self.config.pill_style == WorkspacePillStyle::Outlined {
                row![
                    container(
                        self.core
                            .applet
                            .text(crate::fl!("pill-outline-thickness"))
                            .size(14)
                    )
                    .padding(Padding {
                        left: f32::from(spacing.space_s),
                        ..Padding::ZERO
                    }),
                    space::horizontal(),
                    self.pill_border_width_stepper()
                ]
                .align_y(Alignment::Center)
                .into()
            } else {
                space::vertical().height(Length::Fixed(0.0)).into()
            };

        let content = column![
            padded_control(
                toggler(self.config.dim_minimized_window_icons)
                    .on_toggle(Message::DimMinimizedWindowIcons)
                    .label(crate::fl!("dim-minimized-window-icons"))
                    .text_size(14)
                    .width(Length::Fill)
            ),
            padded_control(divider::horizontal::default())
                .padding([spacing.space_xxs, spacing.space_s]),
            padded_control(
                toggler(self.config.highlight_maximized_window_icons)
                    .on_toggle(Message::HighlightMaximizedWindowIcons)
                    .label(crate::fl!("highlight-maximized-window-icons"))
                    .text_size(14)
                    .width(Length::Fill)
            ),
            padded_control(divider::horizontal::default())
                .padding([spacing.space_xxs, spacing.space_s]),
            padded_control(
                column![
                    self.core.applet.text(crate::fl!("pill-style")).size(14),
                    segmented_control::horizontal(&self.pill_style_model)
                        .width(Length::Fill)
                        .on_activate(Message::PillStyle),
                    outline_thickness
                ]
                .spacing(spacing.space_xxs)
                .align_x(Alignment::Start)
            )
            .padding([0, spacing.space_m]),
            padded_control(divider::horizontal::default())
                .padding([spacing.space_xxs, spacing.space_s]),
            padded_control(
                row![
                    self.core
                        .applet
                        .text(crate::fl!("pill-spacing"))
                        .size(14),
                    space::horizontal(),
                    self.pill_spacing_stepper()
                ]
                .align_y(Alignment::Center)
            )
            .padding([0, spacing.space_m]),
            version
        ]
        .align_x(Alignment::Start)
        .padding([spacing.space_xxs, 0]);

        self.core.applet.popup_container(container(content)).into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
