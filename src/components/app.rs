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
    cosmic_theme::palette::{FromColor, Mix, Oklab, Srgb, Srgba},
    desktop::{IconSourceExt, fde},
    iced::core::{
        Background, Border, Color, Rectangle, Size,
        layout::{self as iced_layout, Layout as IcedLayout},
        renderer,
        widget::{Tree, Widget},
    },
    iced::{
        Alignment,
        Event::Mouse,
        Length, Limits, Padding, Subscription, event,
        mouse::{self, ScrollDelta},
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
        self, INACTIVE_PILL_CONTRAST_STEP_PERCENT, MAX_INACTIVE_PILL_CONTRAST_PERCENT,
        MAX_PILL_BORDER_WIDTH, MAX_PILL_SPACING_PERCENT, MAX_VISIBLE_ICONS, MIN_PILL_BORDER_WIDTH,
        MIN_VISIBLE_ICONS, WorkspacePillStyle, WorkspacesAppletConfig,
    },
    wayland::WorkspaceEvent,
    wayland_subscription::{WorkspacesUpdate, workspaces},
};

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::Path,
    process::Command as ShellCommand,
    sync::LazyLock,
    time::Duration,
};

static AUTOSIZE_MAIN_ID: LazyLock<Id> = LazyLock::new(|| Id::new("autosize-main"));

const SCROLL_RATE_LIMIT: Duration = Duration::from_millis(200);
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
const INACTIVE_PILL_HOVER_CONTRAST_INCREASE_PERCENT: u8 = 15;
const URGENT_FILLED_BORDER_WIDTH: f32 = 1.0;
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

/// Draws one of two equivalent visual trees based directly on cursor position.
///
/// Keeping hover selection inside the widget avoids application-level enter/exit
/// messages, which can be lost when sibling widgets capture the same cursor event.
struct HoverSwitch<'a, Message> {
    normal: Element<'a, Message>,
    hovered: Element<'a, Message>,
}

impl<Message> Widget<Message, Theme, cosmic::Renderer> for HoverSwitch<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.normal), Tree::new(&self.hovered)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [&mut self.normal, &mut self.hovered]);
    }

    fn size(&self) -> Size<Length> {
        self.normal.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.normal.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &iced_layout::Limits,
    ) -> iced_layout::Node {
        let normal = self.normal.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            limits,
        );
        let hovered = self.hovered.as_widget_mut().layout(
            &mut tree.children[1],
            renderer,
            limits,
        );

        iced_layout::Node::with_children(normal.size(), vec![normal, hovered])
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: IcedLayout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let hovered = cursor.is_over(layout.bounds());
        let child_index = usize::from(hovered);
        let child_layout = layout
            .children()
            .nth(child_index)
            .expect("hover switch should have normal and hovered layouts");
        let child = if hovered {
            &self.hovered
        } else {
            &self.normal
        };

        child.as_widget().draw(
            &tree.children[child_index],
            renderer,
            theme,
            style,
            child_layout,
            cursor,
            viewport,
        );
    }
}

fn hover_switch<'a, Message: 'a>(
    normal: impl Into<Element<'a, Message>>,
    hovered: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(HoverSwitch {
        normal: normal.into(),
        hovered: hovered.into(),
    })
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
}

struct AppMetadata {
    name: String,
    icon_source: fde::IconSource,
}

struct WorkspaceApp<'a> {
    app_id: &'a str,
    metadata: &'a AppMetadata,
    minimized_titles: Vec<&'a str>,
    windows: Vec<WorkspaceWindowState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkspaceWindowState {
    minimized: bool,
    maximized: bool,
}

#[derive(Clone, Copy)]
struct WorkspaceIcon<'a> {
    metadata: &'a AppMetadata,
    minimized: bool,
    maximized: bool,
}

impl WorkspaceApp<'_> {
    fn window_count(&self) -> usize {
        self.windows.len()
    }

    fn minimized_count(&self) -> usize {
        self.windows
            .iter()
            .filter(|window| window.minimized)
            .count()
    }

    fn all_minimized(&self) -> bool {
        !self.windows.is_empty() && self.windows.iter().all(|window| window.minimized)
    }

    fn has_maximized(&self) -> bool {
        self.windows.iter().any(|window| window.maximized)
    }
}

fn display_icons<'a>(
    apps: &'a [WorkspaceApp<'a>],
    show_one_icon_per_application: bool,
) -> Vec<WorkspaceIcon<'a>> {
    if show_one_icon_per_application {
        apps.iter()
            .map(|app| WorkspaceIcon {
                metadata: app.metadata,
                minimized: app.all_minimized(),
                maximized: app.has_maximized(),
            })
            .collect()
    } else {
        apps.iter()
            .flat_map(|app| {
                app.windows.iter().map(|window| WorkspaceIcon {
                    metadata: app.metadata,
                    minimized: window.minimized,
                    maximized: window.maximized,
                })
            })
            .collect()
    }
}

fn visible_icon_limit(value: u8) -> u8 {
    value.clamp(MIN_VISIBLE_ICONS, MAX_VISIBLE_ICONS)
}

fn visible_icon_counts(icon_count: usize, limit: u8) -> (usize, usize) {
    let visible = icon_count.min(usize::from(visible_icon_limit(limit)));
    (visible, icon_count.saturating_sub(visible))
}

fn workspace_tooltip(apps: &[WorkspaceApp<'_>]) -> String {
    let mut lines = Vec::new();

    for app in apps {
        let window_count = app.window_count();
        let minimized_count = app.minimized_count();
        let summary = if window_count > 1 {
            format!("{} ×{window_count}", app.metadata.name)
        } else {
            app.metadata.name.clone()
        };
        if app.all_minimized() {
            lines.push(format!("{summary} (minimised)"));
        } else if minimized_count > 0 {
            lines.push(format!("{summary} ({minimized_count} minimised)"));
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

fn should_retain_toplevel_placement(
    current_workspace_count: usize,
    previous_workspace_count: Option<usize>,
    sticky: bool,
) -> bool {
    current_workspace_count == 0
        && previous_workspace_count.is_some_and(|count| count > 0)
        && !sticky
}

fn retain_transient_toplevel_placements(
    previous: &[ToplevelInfo],
    current: &mut [ToplevelInfo],
) {
    for toplevel in current {
        // COSMIC temporarily removes a moved window from its workspace and
        // output until the grab ends. Keep its last placement across that gap.
        let sticky = toplevel
            .state
            .contains(&zcosmic_toplevel_handle_v1::State::Sticky);
        let previous = previous
            .iter()
            .find(|previous| previous.foreign_toplevel == toplevel.foreign_toplevel);

        if should_retain_toplevel_placement(
            toplevel.workspace.len(),
            previous.map(|previous| previous.workspace.len()),
            sticky,
        ) && let Some(previous) = previous
        {
            toplevel.workspace.clone_from(&previous.workspace);
            toplevel.output.clone_from(&previous.output);
        }
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

fn inactive_pill_contrast_percent(value: u8, hovered: bool) -> u8 {
    let value = value.min(MAX_INACTIVE_PILL_CONTRAST_PERCENT);
    if hovered {
        value
            .saturating_add(INACTIVE_PILL_HOVER_CONTRAST_INCREASE_PERCENT)
            .min(MAX_INACTIVE_PILL_CONTRAST_PERCENT)
    } else {
        value
    }
}

fn inactive_pill_contrast_color(start: Srgba, end: Srgba, percent: u8) -> Color {
    let percent = percent.min(MAX_INACTIVE_PILL_CONTRAST_PERCENT);
    if percent == 0 {
        return Color::from(start.color);
    }
    if percent == MAX_INACTIVE_PILL_CONTRAST_PERCENT {
        return Color::from(end.color);
    }

    let start = Oklab::from_color(start.color);
    let end = Oklab::from_color(end.color);
    let factor = f32::from(percent) / f32::from(MAX_INACTIVE_PILL_CONTRAST_PERCENT);
    Color::from(Srgb::from_color(start.mix(end, factor)))
}

fn workspace_overview_command(flatpak: bool) -> (&'static str, &'static [&'static str]) {
    if flatpak {
        ("flatpak-spawn", &["--host", "cosmic-workspaces"])
    } else {
        ("cosmic-workspaces", &[])
    }
}

fn launch_workspace_overview() {
    let flatpak =
        std::env::var_os("FLATPAK_ID").is_some() || std::path::Path::new("/.flatpak-info").exists();
    let (program, args) = workspace_overview_command(flatpak);

    match ShellCommand::new(program).args(args).spawn() {
        Ok(mut child) => {
            // Reap the launcher without blocking the applet's event loop.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(err) => tracing::error!(?err, program, "failed to launch workspace overview"),
    }
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

/// The file extensions icon theme directories recognize for icon files.
const ICON_FILE_EXTENSIONS: &[&str] = &["png", "svg", "svgz", "jpg", "jpeg", "xpm", "ico"];

/// Derive an icon-theme name from an icon value that names or points to a file,
/// such as `/home/user/.local/zed.app/share/icons/hicolor/512x512/apps/zed.png`.
/// Plain names without a file extension pass through unchanged.
fn icon_theme_name(icon: &str) -> &str {
    let file_name = Path::new(icon)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(icon);
    match file_name.rsplit_once('.') {
        Some((stem, extension))
            if ICON_FILE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) =>
        {
            stem
        }
        _ => file_name,
    }
}

/// Resolve a desktop entry's `Icon` value to an icon source, falling back to a
/// theme icon name when the value is a file path that cannot be loaded, such as
/// a sandboxed applet without access to the application's install prefix.
fn icon_source(icon: &str) -> fde::IconSource {
    match fde::IconSource::from_unknown(icon) {
        fde::IconSource::Name(name) => fde::IconSource::Name(icon_theme_name(&name).to_string()),
        source => source,
    }
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

    fn inactive_pill_contrast_stepper(&self) -> Element<'_, Message> {
        let value = self.config.inactive_pill_contrast_percent;
        let decrement: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(DECREASE_ICON_SVG))
                .on_press_maybe((value > 0).then(|| {
                    Message::InactivePillContrast(
                        value.saturating_sub(INACTIVE_PILL_CONTRAST_STEP_PERCENT),
                    )
                }))
                .into();
        let increment: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(INCREASE_ICON_SVG))
                .on_press_maybe((value < MAX_INACTIVE_PILL_CONTRAST_PERCENT).then(|| {
                    Message::InactivePillContrast(
                        value
                            .saturating_add(INACTIVE_PILL_CONTRAST_STEP_PERCENT)
                            .min(MAX_INACTIVE_PILL_CONTRAST_PERCENT),
                    )
                }))
                .into();
        let value = container(self.core.applet.text(format!("{value}%")).size(14))
            .center_x(Length::Fixed(48.0))
            .align_y(Alignment::Center);

        row![decrement, value, increment]
            .align_y(Alignment::Center)
            .into()
    }

    fn max_visible_icons_stepper(&self) -> Element<'_, Message> {
        let value = self.config.max_visible_icons;
        let decrement: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(DECREASE_ICON_SVG))
                .on_press_maybe(
                    (value > MIN_VISIBLE_ICONS)
                        .then(|| Message::MaxVisibleIcons(value - 1)),
                )
                .into();
        let increment: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(INCREASE_ICON_SVG))
                .on_press_maybe(
                    (value < MAX_VISIBLE_ICONS)
                        .then(|| Message::MaxVisibleIcons(value + 1)),
                )
                .into();
        let value = container(self.core.applet.text(value.to_string()).size(14))
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
        inactive_contrast_percent: u8,
    ) -> container::Style {
        let cosmic = theme.cosmic();
        let urgent = urgent && !active;
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
        } else if urgent {
            let color = Color::from(if hovered {
                theme.current_container().component.hover
            } else {
                cosmic.palette.neutral_3
            });
            let destructive = cosmic.destructive_button.base.into();
            (
                (!outlined_mode || hovered).then_some(Background::Color(color)),
                destructive,
                destructive,
                if outlined_mode {
                    outlined_border_width
                } else {
                    URGENT_FILLED_BORDER_WIDTH
                },
            )
        } else {
            let container = theme.current_container();
            let component = &container.component;
            let source = if hovered {
                component.hover
            } else {
                container.base
            };
            let background = inactive_pill_contrast_color(
                source,
                component.border,
                inactive_pill_contrast_percent(inactive_contrast_percent, hovered),
            );
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

        let border_color = if outlined_mode && hovered && !urgent {
            match background.as_ref() {
                Some(Background::Color(color)) => *color,
                _ => border_color,
            }
        } else {
            border_color
        };
        let border_width = if outlined_mode && hovered && !urgent {
            0.0
        } else {
            border_width
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
        urgent: bool,
        outlined_mode: bool,
        hovered: bool,
    ) -> container::Style {
        let color = if active && outlined_mode {
            Self::outlined_active_foreground(theme, hovered)
        } else if urgent && !active {
            theme.cosmic().destructive_button.base.into()
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
            let apps = self.apps_for_workspace(workspace);
            let icons = display_icons(&apps, self.config.show_one_icon_per_application);
            used += self.workspace_button_major_size(&icons);
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

    fn app_group_major_size(&self, icons: &[WorkspaceIcon<'_>]) -> f32 {
        if icons.is_empty() {
            return 0.0;
        }

        let icon_size = self.app_icon_size();
        let (visible_count, overflow_count) =
            visible_icon_counts(icons.len(), self.config.max_visible_icons);
        let visible_size = icons
            .iter()
            .take(visible_count)
            .map(|icon| {
                Self::app_icon_slot_size(
                    icon_size,
                    self.config.highlight_maximized_window_icons && icon.maximized,
                )
            })
            .sum::<f32>();
        let overflow_size = if overflow_count > 0 {
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

    fn workspace_button_major_size(&self, icons: &[WorkspaceIcon<'_>]) -> f32 {
        let base_size = self.suggested_button_size();
        if !icons.is_empty() {
            WORKSPACE_LEADING_PADDING
                + WORKSPACE_TRAILING_PADDING
                + self.number_section_major_size(true)
                + WORKSPACE_CONTENT_SPACING * 2.0
                + WORKSPACE_DIVIDER_WIDTH
                + self.app_group_major_size(icons)
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
        let icon_source = icon_source(desktop_entry.icon().unwrap_or(&desktop_entry.appid));

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
                app.windows.push(WorkspaceWindowState {
                    minimized,
                    maximized,
                });
                if minimized {
                    app.minimized_titles.push(&toplevel.title);
                }
            } else {
                apps.push(WorkspaceApp {
                    app_id: toplevel.app_id.as_str(),
                    metadata,
                    minimized_titles: minimized
                        .then_some(toplevel.title.as_str())
                        .into_iter()
                        .collect(),
                    windows: vec![WorkspaceWindowState {
                        minimized,
                        maximized,
                    }],
                });
            }
        }

        apps
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

    fn workspace_pill_visual<'a>(
        &'a self,
        workspace: &'a Workspace,
        icons: &[WorkspaceIcon<'_>],
        width: f32,
        height: f32,
        hovered: bool,
    ) -> Element<'a, Message> {
        let horizontal = self.core.applet.is_horizontal();
        let active = workspace
            .state
            .contains(ext_workspace_handle_v1::State::Active);
        let urgent = workspace
            .state
            .contains(ext_workspace_handle_v1::State::Urgent);
        let outlined_mode = self.config.pill_style == WorkspacePillStyle::Outlined;
        let outlined_border_width = f32::from(self.config.pill_border_width);
        let pill_cross_axis_inset = if horizontal {
            height * f32::from(self.config.pill_spacing_percent) / 100.0
        } else {
            width * f32::from(self.config.pill_spacing_percent) / 100.0
        };

        let (visible_icon_count, overflow_count) =
            visible_icon_counts(icons.len(), self.config.max_visible_icons);
        let icon_size = self.app_icon_size();
        let mut icon_elements = icons
            .iter()
            .take(visible_icon_count)
            .map(|icon| {
                self.app_icon(
                    icon.metadata,
                    icon_size,
                    self.config.dim_minimized_window_icons && icon.minimized,
                    self.config.highlight_maximized_window_icons && icon.maximized,
                )
            })
            .collect::<Vec<_>>();
        if overflow_count > 0 {
            icon_elements.push(
                self.core
                    .applet
                    .text(format!("+{overflow_count}"))
                    .size((icon_size * 0.55).max(10.0))
                    .into(),
            );
        }
        let app_strip: Element<'_, Message> = if horizontal {
            row(icon_elements)
                .spacing(APP_ICON_SPACING)
                .align_y(Alignment::Center)
                .into()
        } else {
            column(icon_elements)
                .spacing(APP_ICON_SPACING)
                .align_x(Alignment::Center)
                .into()
        };

        let number_section_size = self.number_section_major_size(!icons.is_empty());
        let number_text = self
            .core
            .applet
            .text(&workspace.name)
            .font(cosmic::font::bold());
        let number_text = if let Some(font_size) = workspace_number_font_size(icon_size) {
            number_text.size(font_size)
        } else {
            number_text
        }
        .width(Length::Fill)
        .height(Length::Fill)
        .center();
        let number = container(number_text).class(ContainerClass::Custom(Box::new(
            move |theme| Self::workspace_number_style(theme, active, outlined_mode, hovered),
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

        let content: Element<'_, Message> = if icons.is_empty() {
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
                                urgent,
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
                                urgent,
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

        let has_apps = !icons.is_empty();
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
                        self.config.inactive_pill_contrast_percent,
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
                    self.config.inactive_pill_contrast_percent,
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

        stack![pill_background, pill_content]
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{
        DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT, MAX_VISIBLE_ICONS, MIN_VISIBLE_ICONS,
    };

    use super::{
        APP_GROUP_LEADING_PADDING, APP_GROUP_TRAILING_PADDING, APP_ICON_SPACING, Background, Color,
        IcedWorkspacesApplet, Layout, MAX_INACTIVE_PILL_CONTRAST_PERCENT, MAX_PILL_BORDER_WIDTH,
        MIN_PILL_BORDER_WIDTH, Theme, URGENT_FILLED_BORDER_WIDTH, WORKSPACE_CONTENT_SPACING,
        WORKSPACE_LEADING_PADDING, WORKSPACE_LIST_EDGE_PADDING, WORKSPACE_TRAILING_PADDING,
        AppMetadata, WorkspaceApp, WorkspaceWindowState, display_icons, fde, icon_source,
        icon_theme_name, inactive_pill_contrast_color, inactive_pill_contrast_percent,
        informative_titles, occupied_number_section_major_size, oriented_padding, pill_border_width,
        pill_spacing_percent, should_retain_toplevel_placement, visible_icon_counts,
        visible_icon_limit, workspace_list_padding, workspace_number_font_size,
        workspace_overview_command, workspace_tooltip,
    };

    const TEST_OUTLINED_BORDER_WIDTH: f32 = 2.0;

    fn test_app<'a>(
        app_id: &'a str,
        metadata: &'a AppMetadata,
        windows: Vec<WorkspaceWindowState>,
    ) -> WorkspaceApp<'a> {
        WorkspaceApp {
            app_id,
            metadata,
            minimized_titles: Vec::new(),
            windows,
        }
    }

    fn test_metadata(name: &str) -> AppMetadata {
        AppMetadata {
            name: name.to_owned(),
            icon_source: cosmic::desktop::fde::IconSource::from_unknown(name),
        }
    }

    #[test]
    fn derives_theme_icon_name_from_absolute_path() {
        assert_eq!(
            icon_theme_name("/home/user/.local/zed.app/share/icons/hicolor/512x512/apps/zed.png",),
            "zed",
        );
    }

    #[test]
    fn derives_theme_icon_name_from_relative_image_path() {
        assert_eq!(icon_theme_name("images/zed.svg"), "zed");
        assert_eq!(icon_theme_name("./zed.png"), "zed");
    }

    #[test]
    fn keeps_plain_icon_names_unchanged() {
        assert_eq!(icon_theme_name("zed"), "zed");
        assert_eq!(icon_theme_name("org.gnome.Weather"), "org.gnome.Weather");
        assert_eq!(
            icon_theme_name("application-x-executable"),
            "application-x-executable",
        );
    }

    #[test]
    fn falls_back_to_theme_name_for_unloadable_icon_paths() {
        let source =
            icon_source("/home/user/.local/zed.app/share/icons/hicolor/512x512/apps/zed.png");
        assert_eq!(source, fde::IconSource::Name("zed".to_string()));
    }

    #[test]
    fn keeps_loadable_absolute_paths_as_paths() {
        let existing = std::env::current_dir().expect("current directory");
        let source = icon_source(existing.to_str().expect("utf-8 path"));
        assert!(matches!(source, fde::IconSource::Path(_)));
    }

    #[test]
    fn groups_multiple_windows_into_one_icon_by_default() {
        let metadata = test_metadata("Browser");
        let apps = [test_app(
            "browser",
            &metadata,
            vec![
                WorkspaceWindowState {
                    minimized: true,
                    maximized: false,
                },
                WorkspaceWindowState {
                    minimized: false,
                    maximized: true,
                },
            ],
        )];

        let icons = display_icons(&apps, true);

        assert_eq!(icons.len(), 1);
        assert!(!icons[0].minimized);
        assert!(icons[0].maximized);
    }

    #[test]
    fn expands_windows_and_preserves_each_windows_state() {
        let browser = test_metadata("Browser");
        let editor = test_metadata("Editor");
        let apps = [
            test_app(
                "browser",
                &browser,
                vec![
                    WorkspaceWindowState {
                        minimized: true,
                        maximized: false,
                    },
                    WorkspaceWindowState {
                        minimized: false,
                        maximized: true,
                    },
                ],
            ),
            test_app(
                "editor",
                &editor,
                vec![WorkspaceWindowState {
                    minimized: false,
                    maximized: false,
                }],
            ),
        ];

        let icons = display_icons(&apps, false);

        assert_eq!(icons.len(), 3);
        assert_eq!(icons[0].metadata.name, "Browser");
        assert_eq!((icons[0].minimized, icons[0].maximized), (true, false));
        assert_eq!(icons[1].metadata.name, "Browser");
        assert_eq!((icons[1].minimized, icons[1].maximized), (false, true));
        assert_eq!(icons[2].metadata.name, "Editor");
    }

    #[test]
    fn limits_icon_slots_and_reports_the_remaining_count() {
        assert_eq!(visible_icon_counts(0, 5), (0, 0));
        assert_eq!(visible_icon_counts(5, 5), (5, 0));
        assert_eq!(visible_icon_counts(8, 5), (5, 3));
        assert_eq!(visible_icon_counts(8, 3), (3, 5));
        assert_eq!(visible_icon_counts(20, 16), (16, 4));
    }

    #[test]
    fn clamps_visible_icon_limit_to_the_supported_range() {
        assert_eq!(visible_icon_limit(0), MIN_VISIBLE_ICONS);
        assert_eq!(visible_icon_limit(5), 5);
        assert_eq!(visible_icon_limit(u8::MAX), MAX_VISIBLE_ICONS);
    }

    #[test]
    fn keeps_tooltips_grouped_when_icons_are_expanded_per_window() {
        let browser = test_metadata("Browser");
        let apps = [test_app(
            "browser",
            &browser,
            vec![
                WorkspaceWindowState {
                    minimized: false,
                    maximized: false,
                },
                WorkspaceWindowState {
                    minimized: false,
                    maximized: false,
                },
            ],
        )];

        assert_eq!(display_icons(&apps, false).len(), 2);
        assert_eq!(workspace_tooltip(&apps), "Browser ×2");
    }

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
            DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT,
        )
    }

    #[test]
    fn retains_only_transient_non_sticky_toplevel_placements() {
        assert!(should_retain_toplevel_placement(0, Some(1), false));
        assert!(!should_retain_toplevel_placement(1, Some(1), false));
        assert!(!should_retain_toplevel_placement(0, Some(0), false));
        assert!(!should_retain_toplevel_placement(0, None, false));
        assert!(!should_retain_toplevel_placement(0, Some(1), true));
    }

    fn test_pill_style_with_contrast(
        theme: &Theme,
        hovered: bool,
        outlined_mode: bool,
        contrast_percent: u8,
    ) -> cosmic::widget::container::Style {
        IcedWorkspacesApplet::workspace_pill_style(
            theme,
            false,
            false,
            hovered,
            outlined_mode,
            TEST_OUTLINED_BORDER_WIDTH,
            contrast_percent,
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
            panic!("inactive pill should have a solid opaque background");
        };
        let container = theme.current_container();
        let component = &container.component;
        let expected = inactive_pill_contrast_color(
            container.base,
            component.border,
            DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT,
        );
        assert_eq!(background, expected);
        assert_eq!(background.a, 1.0);
    }

    #[test]
    fn gently_emphasizes_inactive_pill_background_when_hovered() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, false, false, true, false);

        let Some(Background::Color(background)) = style.background else {
            panic!("hovered inactive pill should have a solid opaque background");
        };
        let component = &theme.current_container().component;
        let expected = inactive_pill_contrast_color(
            component.hover,
            component.border,
            inactive_pill_contrast_percent(DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT, true),
        );
        assert_eq!(background, expected);
        assert_eq!(background.a, 1.0);
    }

    #[test]
    fn outlines_inactive_pills_in_outlined_mode() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, false, false, false, true);

        let container = theme.current_container();
        let component = &container.component;
        let expected = inactive_pill_contrast_color(
            container.base,
            component.border,
            DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT,
        );
        assert_eq!(style.background, None);
        assert_eq!(style.border.color, expected);
        assert_eq!(style.border.color.a, 1.0);
        assert_eq!(style.border.width, TEST_OUTLINED_BORDER_WIDTH);
    }

    #[test]
    fn uses_the_selected_width_for_outlined_pills() {
        let theme = Theme::default();

        for (active, urgent, hovered) in [
            (true, false, false),
            (false, false, false),
            (false, true, false),
        ] {
            let style = IcedWorkspacesApplet::workspace_pill_style(
                &theme,
                active,
                urgent,
                hovered,
                true,
                3.0,
                DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT,
            );
            assert_eq!(style.border.width, 3.0);
        }
    }

    #[test]
    fn fills_inactive_outlined_pills_on_hover() {
        let theme = Theme::default();
        let style = test_pill_style(&theme, false, false, true, true);

        let component = &theme.current_container().component;
        let expected = inactive_pill_contrast_color(
            component.hover,
            component.border,
            inactive_pill_contrast_percent(DEFAULT_INACTIVE_PILL_CONTRAST_PERCENT, true),
        );
        assert_eq!(style.background, Some(Background::Color(expected)));
        assert_eq!(style.border.color, expected);
        assert_eq!(style.border.width, 0.0);
    }

    #[test]
    fn applies_configured_contrast_to_filled_and_outlined_inactive_pills() {
        let theme = Theme::default();
        let container = theme.current_container();
        let component = &container.component;

        for outlined_mode in [false, true] {
            for (configured, resting_contrast, hovered_contrast) in
                [(55, 55, 70), (90, 90, 100), (0, 0, 15)]
            {
                let resting =
                    test_pill_style_with_contrast(&theme, false, outlined_mode, configured);
                let hovered =
                    test_pill_style_with_contrast(&theme, true, outlined_mode, configured);

                let resting_color = inactive_pill_contrast_color(
                    container.base,
                    component.border,
                    resting_contrast,
                );
                let hovered_color = inactive_pill_contrast_color(
                    component.hover,
                    component.border,
                    hovered_contrast,
                );

                if outlined_mode {
                    assert_eq!(resting.background, None);
                    assert_eq!(resting.border.color, resting_color);
                } else {
                    assert_eq!(resting.background, Some(Background::Color(resting_color)));
                }
                assert_eq!(hovered.background, Some(Background::Color(hovered_color)));
                if outlined_mode {
                    assert_eq!(hovered.border.color, hovered_color);
                }
                assert_eq!(resting_color.a, 1.0);
                assert_eq!(hovered_color.a, 1.0);
            }
        }
    }

    #[test]
    fn uses_opaque_theme_tokens_at_the_contrast_endpoints() {
        let theme = Theme::default();
        let container = theme.current_container();
        let component = &container.component;

        let minimum =
            inactive_pill_contrast_color(container.base, component.border, 0);
        let maximum =
            inactive_pill_contrast_color(container.base, component.border, 100);

        assert_eq!(minimum, Color::from(container.base.color));
        assert_eq!(maximum, Color::from(component.border.color));
        assert_eq!(minimum.a, 1.0);
        assert_eq!(maximum.a, 1.0);
    }

    #[test]
    fn derives_hover_contrast_from_the_configured_resting_contrast() {
        assert_eq!(inactive_pill_contrast_percent(55, false), 55);
        assert_eq!(inactive_pill_contrast_percent(55, true), 70);
        assert_eq!(inactive_pill_contrast_percent(90, false), 90);
        assert_eq!(inactive_pill_contrast_percent(90, true), 100);
        assert_eq!(inactive_pill_contrast_percent(0, false), 0);
        assert_eq!(inactive_pill_contrast_percent(0, true), 15);
    }

    #[test]
    fn clamps_inactive_pill_contrast_to_one_hundred_percent() {
        assert_eq!(inactive_pill_contrast_percent(100, false), 100);
        assert_eq!(
            inactive_pill_contrast_percent(u8::MAX, false),
            MAX_INACTIVE_PILL_CONTRAST_PERCENT
        );
        assert_eq!(inactive_pill_contrast_percent(u8::MAX, true), 100);
    }

    #[test]
    fn keeps_active_and_urgent_styles_independent_of_inactive_contrast() {
        let theme = Theme::default();

        for (active, urgent, hovered, outlined_mode) in [
            (true, false, false, false),
            (true, false, true, true),
            (false, true, false, false),
            (false, true, true, true),
        ] {
            let minimum_contrast = IcedWorkspacesApplet::workspace_pill_style(
                &theme,
                active,
                urgent,
                hovered,
                outlined_mode,
                TEST_OUTLINED_BORDER_WIDTH,
                0,
            );
            let maximum_contrast = IcedWorkspacesApplet::workspace_pill_style(
                &theme,
                active,
                urgent,
                hovered,
                outlined_mode,
                TEST_OUTLINED_BORDER_WIDTH,
                100,
            );

            assert_eq!(minimum_contrast, maximum_contrast);
        }
    }

    #[test]
    fn matches_nonurgent_hovered_outlined_borders_to_their_backgrounds() {
        let theme = Theme::default();
        let styles = [
            test_pill_style(&theme, true, false, true, true),
            test_pill_style(&theme, false, false, true, true),
        ];

        for style in styles {
            let Some(Background::Color(background)) = style.background else {
                panic!("hovered outlined pill should have a solid background");
            };
            assert_eq!(style.border.color, background);
        }
    }

    #[test]
    fn removes_the_redundant_outline_when_an_outlined_pill_is_filled_on_hover() {
        let theme = Theme::default();

        for (active, urgent) in [(true, false), (false, false)] {
            let style = test_pill_style(&theme, active, urgent, true, true);
            assert!(style.background.is_some());
            assert_eq!(style.border.width, 0.0);
        }
    }

    #[test]
    fn gives_urgent_pills_a_destructive_text_and_border() {
        let theme = Theme::default();
        let destructive = Color::from(theme.cosmic().destructive_button.base);

        for outlined_mode in [false, true] {
            for hovered in [false, true] {
                let style = test_pill_style(&theme, false, true, hovered, outlined_mode);

                assert_eq!(style.text_color, Some(destructive));
                assert_eq!(style.icon_color, Some(destructive));
                assert_eq!(style.border.color, destructive);
                assert_eq!(
                    style.border.width,
                    if outlined_mode {
                        TEST_OUTLINED_BORDER_WIDTH
                    } else {
                        URGENT_FILLED_BORDER_WIDTH
                    }
                );
            }
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
        assert_eq!(style.border.width, 0.0);
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
        let style =
            IcedWorkspacesApplet::workspace_divider_style(&theme, true, false, true, false);
        let expected = theme
            .cosmic()
            .accent_text
            .unwrap_or(theme.cosmic().accent_button.base);

        assert_eq!(style.background, Some(Background::Color(expected.into())));
    }

    #[test]
    fn gives_the_active_divider_an_on_accent_color_when_hovered() {
        let theme = Theme::default();
        let style =
            IcedWorkspacesApplet::workspace_divider_style(&theme, true, false, true, true);

        assert_eq!(
            style.background,
            Some(Background::Color(theme.cosmic().accent_button.on.into()))
        );
    }

    #[test]
    fn gives_the_urgent_divider_a_destructive_color() {
        let theme = Theme::default();

        for outlined_mode in [false, true] {
            for hovered in [false, true] {
                let style = IcedWorkspacesApplet::workspace_divider_style(
                    &theme,
                    false,
                    true,
                    outlined_mode,
                    hovered,
                );

                assert_eq!(
                    style.background,
                    Some(Background::Color(
                        theme.cosmic().destructive_button.base.into()
                    ))
                );
            }
        }
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
    fn launches_the_overview_directly_outside_flatpak() {
        assert_eq!(
            workspace_overview_command(false),
            ("cosmic-workspaces", &[][..])
        );
    }

    #[test]
    fn launches_the_host_overview_from_flatpak() {
        assert_eq!(
            workspace_overview_command(true),
            ("flatpak-spawn", &["--host", "cosmic-workspaces"][..])
        );
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
    ShowOneIconPerApplication(bool),
    MaxVisibleIcons(u8),
    PillStyle(segmented_button::Entity),
    PillBorderWidth(u8),
    PillSpacing(u8),
    InactivePillContrast(u8),
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
                let (config, errors) = WorkspacesAppletConfig::load(helper);
                for err in errors {
                    tracing::error!(?err, "failed to load workspaces applet config entry");
                }
                config
            })
            .unwrap_or_default();
        config.pill_border_width = pill_border_width(config.pill_border_width);
        config.pill_spacing_percent = pill_spacing_percent(config.pill_spacing_percent);
        config.max_visible_icons = visible_icon_limit(config.max_visible_icons);
        config.inactive_pill_contrast_percent =
            inactive_pill_contrast_percent(config.inactive_pill_contrast_percent, false);
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
                    retain_transient_toplevel_placements(
                        &self.toplevels,
                        &mut snapshot.toplevels,
                    );
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
                launch_workspace_overview();
            }
            Message::TogglePopup => {
                return if let Some(popup) = self.popup.take() {
                    surface::surface_task(surface::action::destroy_popup(popup))
                } else {
                    surface::surface_task(surface::action::app_popup(
                        |_| Default::default(),
                        |app: &mut Self| {
                            let popup = window::Id::unique();
                            app.popup.replace(popup);
                            app.core.applet.get_popup_settings(
                                app.core.main_window_id().unwrap(),
                                popup,
                                Some((1, 1)),
                                None,
                                None,
                            )
                        },
                        None,
                    ))
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
            Message::ShowOneIconPerApplication(enabled) => {
                self.config.show_one_icon_per_application = enabled;
                self.write_config();
            }
            Message::MaxVisibleIcons(limit) => {
                self.config.max_visible_icons = visible_icon_limit(limit);
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
            Message::InactivePillContrast(percent) => {
                self.config.inactive_pill_contrast_percent =
                    inactive_pill_contrast_percent(percent, false);
                self.write_config();
            }
            Message::ConfigUpdated(mut config) => {
                config.pill_border_width = pill_border_width(config.pill_border_width);
                config.pill_spacing_percent = pill_spacing_percent(config.pill_spacing_percent);
                config.max_visible_icons = visible_icon_limit(config.max_visible_icons);
                config.inactive_pill_contrast_percent =
                    inactive_pill_contrast_percent(config.inactive_pill_contrast_percent, false);
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
            let apps = self.apps_for_workspace(w);
            let icons = display_icons(&apps, self.config.show_one_icon_per_application);
            let major_size = self.workspace_button_major_size(&icons);
            let (width, height) = if horizontal {
                (major_size, suggested_window_size.1.get() as f32)
            } else {
                (suggested_window_size.0.get() as f32, major_size)
            };
            let has_apps = !icons.is_empty();
            let normal = self.workspace_pill_visual(w, &icons, width, height, false);
            let hovered = self.workspace_pill_visual(w, &icons, width, height, true);
            let btn = button(hover_switch(normal, hovered))
                .width(Length::Fixed(width))
                .height(Length::Fixed(height))
                .on_press(if active {
                    Message::WorkspaceOverview
                } else {
                    Message::WorkspacePressed(w.handle.clone())
                })
                .padding(0)
                .class(cosmic::theme::iced::Button::Transparent);

            let workspace_button: Element<'_, Message> = if has_apps {
                let tooltip = workspace_tooltip(&apps);
                self.core
                    .applet
                    .applet_tooltip(btn, tooltip, false, Message::Surface, None)
                    .into()
            } else {
                btn.into()
            };

            mouse_area(workspace_button)
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
                    self.core
                        .applet
                        .text(crate::fl!("pill-outline-thickness"))
                        .size(14),
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
            padded_control(
                toggler(self.config.highlight_maximized_window_icons)
                    .on_toggle(Message::HighlightMaximizedWindowIcons)
                    .label(crate::fl!("highlight-maximized-window-icons"))
                    .text_size(14)
                    .width(Length::Fill)
            ),
            padded_control(
                toggler(self.config.show_one_icon_per_application)
                    .on_toggle(Message::ShowOneIconPerApplication)
                    .label(crate::fl!("show-one-icon-per-application"))
                    .text_size(14)
                    .width(Length::Fill)
            ),
            padded_control(
                row![
                    self.core
                        .applet
                        .text(crate::fl!("max-visible-icons"))
                        .size(14),
                    space::horizontal(),
                    self.max_visible_icons_stepper()
                ]
                .align_y(Alignment::Center)
            ),
            padded_control(divider::horizontal::default())
                .padding([spacing.space_xxs, spacing.space_s]),
            padded_control(
                column![
                    column![
                        self.core.applet.text(crate::fl!("pill-style")).size(14),
                        segmented_control::horizontal(&self.pill_style_model)
                            .width(Length::Fill)
                            .on_activate(Message::PillStyle),
                        outline_thickness
                    ]
                    .spacing(spacing.space_xxs)
                    .align_x(Alignment::Start),
                    row![
                        self.core
                            .applet
                            .text(crate::fl!("pill-spacing"))
                            .size(14),
                        space::horizontal(),
                        self.pill_spacing_stepper()
                    ]
                    .align_y(Alignment::Center),
                    row![
                        self.core
                            .applet
                            .text(crate::fl!("inactive-pill-contrast"))
                            .size(14),
                        space::horizontal(),
                        self.inactive_pill_contrast_stepper()
                    ]
                    .align_y(Alignment::Center)
                ]
                .spacing(spacing.space_xxs)
                .align_x(Alignment::Start)
            ),
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
