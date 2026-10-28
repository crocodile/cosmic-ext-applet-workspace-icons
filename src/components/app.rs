// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use cctk::{
    cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1,
    cosmic_protocols::workspace::v2::client::zcosmic_workspace_handle_v2,
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
    cosmic_theme::palette::{IntoColor, Oklab, Srgb, Srgba},
    desktop::{IconSourceExt, fde},
    iced::core::{
        Background, Border, Color, Rectangle, Size,
        layout::{self as iced_layout, Layout as IcedLayout},
        renderer, svg,
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
    widget::{Id, autosize, container, divider, dropdown::popup_dropdown, mouse_area, toggler},
};

use crate::{
    config::{
        self, INACTIVE_PILL_BRIGHTNESS_STEP, MAX_INACTIVE_PILL_BRIGHTNESS_PERCENT,
        MAX_PILL_BORDER_WIDTH, MAX_PILL_SPACING_PERCENT, MAX_VISIBLE_ICONS,
        MIN_INACTIVE_PILL_BRIGHTNESS_PERCENT, MIN_PILL_BORDER_WIDTH, MIN_VISIBLE_ICONS,
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

#[cfg(test)]
#[path = "workspace_layout_tests.rs"]
mod workspace_layout_tests;

const SCROLL_RATE_LIMIT: Duration = Duration::from_millis(200);
const APP_ICON_SPACING: f32 = 4.0;
const APP_GROUP_LEADING_PADDING: f32 = 4.0;
const APP_GROUP_TRAILING_PADDING: f32 = 0.0;
const APP_GROUP_CROSS_AXIS_PADDING: f32 = 2.0;
const WORKSPACE_CONTENT_SPACING: f32 = 4.0;
const WORKSPACE_BUTTON_SPACING: f32 = 4.0;
const WORKSPACE_LIST_EDGE_PADDING: f32 = 2.0;
const WORKSPACE_CONTENT_PADDING: f32 = 8.0;
const WORKSPACE_DIVIDER_WIDTH: f32 = 1.0;
const MINIMIZED_ICON_OPACITY: f32 = 0.45;
const MAXIMIZED_HIGHLIGHT_SCALE: f32 = 1.28;
const MAXIMIZED_ICON_GLOW_OPACITY: f32 = 0.24;
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
<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <rect x="3" y="7" width="10" height="2" rx="1" fill="#000"/>
  <rect x="7" y="3" width="2" height="10" rx="1" fill="#000"/>
</svg>
"##;

const EMPTY_WORKSPACE_ICON_SVG: &[u8] = br##"
<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
  <circle cx="12" cy="12" r="4" fill="#000"/>
</svg>
"##;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceIndicator {
    Label,
    None,
    Plus,
    Empty,
}

impl WorkspaceIndicator {
    fn symbol_svg(self) -> &'static [u8] {
        match self {
            Self::Plus => INCREASE_ICON_SVG,
            Self::Empty => EMPTY_WORKSPACE_ICON_SVG,
            _ => unreachable!("only empty-workspace symbols have SVGs"),
        }
    }
}

fn workspace_svg(handle: svg::Handle, color: Color) -> svg::Svg {
    // Iced's SVG tint only applies RGB. Apply the theme alpha as image opacity.
    svg::Svg::new(handle)
        .color(Color { a: 1.0, ..color })
        .opacity(color.a)
}

struct WorkspaceSymbol {
    handle: svg::Handle,
    size: f32,
}

impl<Message> Widget<Message, Theme, cosmic::Renderer> for WorkspaceSymbol {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        limits: &iced_layout::Limits,
    ) -> iced_layout::Node {
        iced_layout::Node::new(limits.resolve(
            Length::Fixed(self.size),
            Length::Fixed(self.size),
            Size::ZERO,
        ))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut cosmic::Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: IcedLayout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use cosmic::iced::core::svg::Renderer as _;

        let bounds = layout.bounds();
        if let Some(clip) = bounds.intersection(viewport) {
            renderer.draw_svg(
                workspace_svg(self.handle.clone(), style.icon_color),
                bounds,
                clip,
            );
        }
    }
}

fn workspace_symbol_is_cutout(
    active: bool,
    inactive_pill_style: WorkspacePillStyle,
    hovered: bool,
) -> bool {
    !active && (inactive_pill_style == WorkspacePillStyle::Filled || hovered)
}

/// Reuse the solid symbol as a luminance mask so the panel shows through the fill.
fn workspace_cutout_svg(
    indicator: WorkspaceIndicator,
    icon_size: f32,
    size: Size,
    fill: Rectangle,
    radii: [f32; 4],
) -> String {
    let (canvas_width, canvas_height) = (size.width, size.height);
    let Size {
        width: w,
        height: h,
    } = fill.size();
    let max_radius = w.min(h) / 2.0;
    let [tl, tr, br, bl] = radii.map(|radius| radius.clamp(0.0, max_radius));
    let x = fill.x + (w - icon_size) / 2.0;
    let y = fill.y + (h - icon_size) / 2.0;
    let symbol = std::str::from_utf8(indicator.symbol_svg())
        .expect("embedded SVG is UTF-8")
        .replacen(
            "<svg ",
            &format!("<svg x=\"{x}\" y=\"{y}\" width=\"{icon_size}\" height=\"{icon_size}\" "),
            1,
        );
    format!(
        r##"<svg width="{canvas_width}" height="{canvas_height}"
    viewBox="0 0 {canvas_width} {canvas_height}" xmlns="http://www.w3.org/2000/svg">
  <defs><mask id="symbol" maskUnits="userSpaceOnUse" x="0" y="0"
      width="{canvas_width}" height="{canvas_height}">
    <rect width="{canvas_width}" height="{canvas_height}" fill="#fff"/>{symbol}
  </mask></defs>
  <g mask="url(#symbol)"><path transform="translate({origin_x} {origin_y})"
    d="M {tl} 0 H {} A {tr} {tr} 0 0 1 {w} {tr}
    V {} A {br} {br} 0 0 1 {} {h}
    H {bl} A {bl} {bl} 0 0 1 0 {}
    V {tl} A {tl} {tl} 0 0 1 {tl} 0 Z" fill="#000"/></g>
</svg>"##,
        w - tr,
        h - br,
        w - br,
        h - bl,
        origin_x = fill.x,
        origin_y = fill.y,
    )
}

fn workspace_cutout_image(
    indicator: WorkspaceIndicator,
    icon_size: f32,
    bounds: Rectangle,
    border: Border,
    color: Color,
    scale: f32,
) -> Option<(svg::Svg, Rectangle)> {
    // Match the native quad's inner fill; the urgent border stays outside it.
    let border_width = border
        .width
        .min(bounds.width / 2.0)
        .min(bounds.height / 2.0);
    let fill = bounds.shrink(border_width);
    if fill.width <= 0.0 || fill.height <= 0.0 {
        return None;
    }
    let max_radius = fill.width.min(fill.height) / 2.0;
    let radii = <[f32; 4]>::from(border.radius)
        .map(|radius| (radius - border_width / 2.0).clamp(0.0, max_radius) * scale);

    // SVG rasterization rounds its canvas size and position to physical pixels.
    // Pad that canvas outwards and keep the fractional pill position inside it.
    let left = (bounds.x * scale).floor();
    let top = (bounds.y * scale).floor();
    let width = ((bounds.x + bounds.width) * scale).ceil() - left;
    let height = ((bounds.y + bounds.height) * scale).ceil() - top;
    let x = fill.x * scale - left;
    let y = fill.y * scale - top;
    let source = workspace_cutout_svg(
        indicator,
        icon_size * scale,
        Size::new(width, height),
        Rectangle {
            x,
            y,
            width: fill.width * scale,
            height: fill.height * scale,
        },
        radii,
    );
    let image = workspace_svg(svg::Handle::from_memory(source.into_bytes()), color);
    let image_bounds = Rectangle {
        x: left / scale,
        y: top / scale,
        width: width / scale,
        height: height / scale,
    };
    Some((image, image_bounds))
}

/// Draw the usual pill border around either a solid fill or a fill with a hole.
struct WorkspacePillBackground<'a> {
    style: Box<dyn Fn(&Theme) -> container::Style + 'a>,
    cutout: Option<(WorkspaceIndicator, f32)>,
}

impl<Message> Widget<Message, Theme, cosmic::Renderer> for WorkspacePillBackground<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        limits: &iced_layout::Limits,
    ) -> iced_layout::Node {
        iced_layout::Node::new(limits.resolve(Length::Fill, Length::Fill, Size::ZERO))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &Theme,
        renderer_style: &renderer::Style,
        layout: IcedLayout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use cosmic::iced::core::{renderer::Renderer as _, svg::Renderer as _};

        let bounds = layout.bounds();
        if !bounds.intersects(viewport) {
            return;
        }
        let style = (self.style)(theme);
        let quad = renderer::Quad {
            bounds,
            border: style.border,
            shadow: style.shadow,
            snap: style.snap,
        };
        if let (Some((indicator, icon_size)), Some(Background::Color(color))) =
            (self.cutout, style.background)
        {
            if let Some((image, image_bounds)) = workspace_cutout_image(
                indicator,
                icon_size,
                bounds,
                style.border,
                color,
                renderer_style.scale_factor as f32,
            ) {
                renderer.with_layer(*viewport, |renderer| {
                    renderer.draw_svg(image, image_bounds, *viewport);
                });
            }
            // Separate layers preserve this order despite quad/image batching.
            if style.border.width > 0.0 {
                renderer.with_layer(*viewport, |renderer| {
                    renderer.fill_quad(quad, Color::TRANSPARENT);
                });
            }
            return;
        }
        renderer.fill_quad(
            quad,
            style
                .background
                .unwrap_or(Background::Color(Color::TRANSPARENT)),
        );
    }
}

fn workspace_indicator(
    show_numbers: bool,
    has_icons: bool,
    pinned: bool,
    is_last: bool,
    show_empty_dot: bool,
) -> WorkspaceIndicator {
    if show_numbers {
        WorkspaceIndicator::Label
    } else if has_icons {
        WorkspaceIndicator::None
    } else if !pinned && is_last {
        WorkspaceIndicator::Plus
    } else if show_empty_dot {
        WorkspaceIndicator::Empty
    } else {
        WorkspaceIndicator::None
    }
}

fn show_empty_workspace_dot(inactive_pill_style: WorkspacePillStyle, border_width: u8) -> bool {
    inactive_pill_style == WorkspacePillStyle::Outlined && border_width == 0
}

fn app_group_leading_padding(show_numbers: bool) -> f32 {
    if show_numbers {
        APP_GROUP_LEADING_PADDING
    } else {
        0.0
    }
}

fn workspace_major_size(
    base_size: f32,
    number_size: f32,
    app_group_size: f32,
    show_numbers: bool,
    edge_padding: f32,
) -> f32 {
    if app_group_size == 0.0 {
        return base_size;
    }
    edge_padding * 2.0
        + app_group_size
        + if show_numbers {
            number_size + WORKSPACE_CONTENT_SPACING * 2.0 + WORKSPACE_DIVIDER_WIDTH
        } else {
            0.0
        }
}

fn workspace_edge_padding(show_numbers: bool, cross_size: f32, largest_icon_slot: f32) -> f32 {
    if show_numbers {
        WORKSPACE_CONTENT_PADDING
    } else {
        // The same inset on either side of a single icon makes its pill square.
        ((cross_size - largest_icon_slot) / 2.0).max(0.0)
    }
}

fn workspace_pill_radii(
    theme_radii: [f32; 4],
    width: f32,
    height: f32,
    layout: Layout,
    cross_inset: f32,
) -> [f32; 4] {
    let (width, height) = match layout {
        Layout::Row => (width, height - cross_inset * 2.0),
        Layout::Column => (width - cross_inset * 2.0, height),
    };
    let max_radius = width.min(height).max(0.0) / 2.0;
    theme_radii.map(|radius| radius.min(max_radius))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PillStyleSelection {
    AllFilled,
    ActiveFilled,
    AllOutlined,
}

impl PillStyleSelection {
    const ALL: [Self; 3] = [Self::AllFilled, Self::ActiveFilled, Self::AllOutlined];

    fn styles(self) -> (WorkspacePillStyle, WorkspacePillStyle) {
        match self {
            Self::AllFilled => (WorkspacePillStyle::Filled, WorkspacePillStyle::Filled),
            Self::ActiveFilled => (WorkspacePillStyle::Filled, WorkspacePillStyle::Outlined),
            Self::AllOutlined => (WorkspacePillStyle::Outlined, WorkspacePillStyle::Outlined),
        }
    }

    fn label(self) -> String {
        match self {
            Self::AllFilled => crate::fl!("pill-style-filled"),
            Self::ActiveFilled => crate::fl!("pill-style-mixed"),
            Self::AllOutlined => crate::fl!("pill-style-outlined"),
        }
    }
}

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
        let normal = self
            .normal
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let hovered = self
            .hovered
            .as_widget_mut()
            .layout(&mut tree.children[1], renderer, limits);

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
        let child = if hovered { &self.hovered } else { &self.normal };

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

fn retain_transient_toplevel_placements(previous: &[ToplevelInfo], current: &mut [ToplevelInfo]) {
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

fn inactive_pill_brightness_percent(value: i8) -> i8 {
    value.clamp(
        MIN_INACTIVE_PILL_BRIGHTNESS_PERCENT,
        MAX_INACTIVE_PILL_BRIGHTNESS_PERCENT,
    )
}

fn inactive_pill_color(token: Srgba, brightness_percent: i8) -> Color {
    let brightness_percent = inactive_pill_brightness_percent(brightness_percent);
    if brightness_percent == 0 {
        // Avoid a colour-space round trip so 0% exactly matches COSMIC's token.
        return token.into();
    }

    // Adjust only perceptual lightness. Keep the button token's hue, chroma,
    // and alpha: its translucency is part of COSMIC's frosted-panel styling.
    let mut oklab: Oklab = Srgb::new(token.red, token.green, token.blue).into_color();
    let fraction = f32::from(brightness_percent) / 100.0;
    oklab.l = if fraction < 0.0 {
        oklab.l * (1.0 + fraction)
    } else {
        oklab.l + (1.0 - oklab.l) * fraction
    };
    let adjusted: Srgb = oklab.into_color();
    Srgba::new(
        adjusted.red.clamp(0.0, 1.0),
        adjusted.green.clamp(0.0, 1.0),
        adjusted.blue.clamp(0.0, 1.0),
        token.alpha,
    )
    .into()
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

fn workspace_overflow_text(
    applet: &cosmic::applet::Context,
    icon_size: f32,
    count: usize,
) -> cosmic::widget::Text<'static, Theme> {
    applet
        .text(format!("+{count}"))
        .size((icon_size * 0.55).max(10.0))
        .wrapping(cosmic::iced::core::text::Wrapping::None)
}

fn workspace_overflow_size(applet: &cosmic::applet::Context, icon_size: f32, count: usize) -> Size {
    if count == 0 {
        return Size::ZERO;
    }
    // This app uses Iced's CPU renderer. Reuse one per thread to lay out the
    // actual text widget, preserving COSMIC's font and absolute line height.
    // The widget sets its own font and size, so renderer defaults are unused.
    thread_local! {
        static MEASURER: cosmic::Renderer = cosmic::Renderer::new(
            cosmic::font::default(), cosmic::iced::Pixels(16.0),
        );
    }
    let mut text: Element<'_, Message> = workspace_overflow_text(applet, icon_size, count).into();
    let mut tree = Tree::new(&text);
    MEASURER.with(|renderer| {
        text.as_widget_mut()
            .layout(&mut tree, renderer, &iced_layout::Limits::NONE)
            .size()
    })
}

impl IcedWorkspacesApplet {
    fn inactive_pill_brightness_stepper(&self) -> Element<'_, Message> {
        let value = self.config.inactive_pill_brightness_percent;
        let decrement: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(DECREASE_ICON_SVG))
                .on_press_maybe((value > MIN_INACTIVE_PILL_BRIGHTNESS_PERCENT).then(|| {
                    Message::InactivePillBrightness(inactive_pill_brightness_percent(
                        value - INACTIVE_PILL_BRIGHTNESS_STEP,
                    ))
                }))
                .into();
        let increment: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(INCREASE_ICON_SVG))
                .on_press_maybe((value < MAX_INACTIVE_PILL_BRIGHTNESS_PERCENT).then(|| {
                    Message::InactivePillBrightness(inactive_pill_brightness_percent(
                        value + INACTIVE_PILL_BRIGHTNESS_STEP,
                    ))
                }))
                .into();
        let label = if value > 0 {
            format!("+{value}%")
        } else {
            format!("{value}%")
        };
        let value = container(self.core.applet.text(label).size(14))
            .center_x(Length::Fixed(48.0))
            .align_y(Alignment::Center);

        row![decrement, value, increment]
            .align_y(Alignment::Center)
            .into()
    }

    fn pill_border_width_stepper(&self) -> Element<'_, Message> {
        let value = self.config.pill_border_width;
        let decrement: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(DECREASE_ICON_SVG))
                .on_press_maybe(
                    (value > MIN_PILL_BORDER_WIDTH).then(|| Message::PillBorderWidth(value - 1)),
                )
                .into();
        let increment: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(INCREASE_ICON_SVG))
                .on_press_maybe(
                    (value < MAX_PILL_BORDER_WIDTH).then(|| Message::PillBorderWidth(value + 1)),
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
                    (value < MAX_PILL_SPACING_PERCENT).then(|| Message::PillSpacing(value + 1)),
                )
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
                    (value > MIN_VISIBLE_ICONS).then(|| Message::MaxVisibleIcons(value - 1)),
                )
                .into();
        let increment: Element<'_, Message> =
            cosmic::widget::button::icon(symbolic_svg_icon(INCREASE_ICON_SVG))
                .on_press_maybe(
                    (value < MAX_VISIBLE_ICONS).then(|| Message::MaxVisibleIcons(value + 1)),
                )
                .into();
        let value = container(self.core.applet.text(value.to_string()).size(14))
            .center_x(Length::Fixed(48.0))
            .align_y(Alignment::Center);

        row![decrement, value, increment]
            .align_y(Alignment::Center)
            .into()
    }

    fn workspace_pill_style(
        theme: &Theme,
        active: bool,
        urgent: bool,
        hovered: bool,
        pill_style: WorkspacePillStyle,
        inactive_pill_style: WorkspacePillStyle,
        outlined_border_width: f32,
        inactive_brightness_percent: i8,
    ) -> container::Style {
        let cosmic = theme.cosmic();
        let outlined_mode = if active {
            pill_style == WorkspacePillStyle::Outlined
        } else {
            inactive_pill_style == WorkspacePillStyle::Outlined
        };
        let component = if active {
            &cosmic.accent_button
        } else {
            &cosmic.button
        };
        let urgent = urgent && !active;
        let destructive = cosmic.destructive_button.base.into();

        let base = if active || urgent {
            component.base.into()
        } else {
            inactive_pill_color(component.base, inactive_brightness_percent)
        };
        let hover = if active || urgent {
            component.hover.into()
        } else {
            inactive_pill_color(component.hover, inactive_brightness_percent)
        };

        let background = match (outlined_mode, hovered) {
            (_, true) => Some(hover),
            (false, false) => Some(base),
            _ => None,
        };
        let text_color = if urgent {
            destructive
        } else if active && outlined_mode && !hovered {
            // An outlined active pill has no accent fill at rest. Keep application
            // icons readable against the panel background.
            theme.current_container().component.on.into()
        } else {
            component.on.into()
        };
        let border_color = match (outlined_mode, hovered, urgent) {
            (_, _, true) => destructive,
            (true, true, false) => hover,
            (true, false, false) => base,
            _ => Color::TRANSPARENT,
        };
        let border_width = match (outlined_mode, hovered, urgent) {
            (false, _, true) => URGENT_FILLED_BORDER_WIDTH,
            (true, _, true) => outlined_border_width.max(URGENT_FILLED_BORDER_WIDTH),
            (true, false, false) => outlined_border_width,
            _ => 0.0,
        };

        container::Style {
            background: background.map(Background::Color),
            border: Border {
                color: border_color,
                width: border_width,
                radius: cosmic.radius_xl().into(),
            },
            text_color: Some(text_color),
            icon_color: Some(text_color),
            ..Default::default()
        }
    }

    fn workspace_indicator_style(
        theme: &Theme,
        active: bool,
        pill_style: WorkspacePillStyle,
        hovered: bool,
    ) -> container::Style {
        if !active || pill_style != WorkspacePillStyle::Outlined {
            return container::Style::default();
        }

        let foreground = Self::outlined_active_foreground(theme, hovered);
        container::Style {
            text_color: Some(foreground),
            icon_color: Some(foreground),
            ..Default::default()
        }
    }

    fn workspace_symbol_style(pill: container::Style) -> container::Style {
        let color = if pill.background.is_none() {
            Some(pill.border.color)
        } else {
            pill.icon_color
        };
        container::Style {
            text_color: color,
            icon_color: color,
            ..Default::default()
        }
    }

    fn workspace_divider_style(
        theme: &Theme,
        active: bool,
        urgent: bool,
        pill_style: WorkspacePillStyle,
        hovered: bool,
    ) -> container::Style {
        let color = if active && pill_style == WorkspacePillStyle::Outlined {
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

    fn workspace_cross_axis_size(&self) -> f32 {
        let window_size = self.core.applet.suggested_window_size();
        if self.core.applet.is_horizontal() {
            window_size.1.get() as f32
        } else {
            window_size.0.get() as f32
        }
    }

    fn app_icon_size(&self) -> f32 {
        (self.workspace_cross_axis_size() * 0.52).max(16.0)
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

        let (visible_count, overflow_count) =
            visible_icon_counts(icons.len(), self.config.max_visible_icons);
        let visible_size = self.visible_icon_slot_sizes(icons).into_iter().sum::<f32>();
        let overflow_size = if overflow_count > 0 {
            let size =
                workspace_overflow_size(&self.core.applet, self.app_icon_size(), overflow_count);
            match self.layout {
                Layout::Row => size.width + APP_ICON_SPACING,
                Layout::Column => size.height + APP_ICON_SPACING,
            }
        } else {
            0.0
        };

        visible_size
            + visible_count.saturating_sub(1) as f32 * APP_ICON_SPACING
            + overflow_size
            + app_group_leading_padding(self.config.show_workspace_numbers)
            + APP_GROUP_TRAILING_PADDING
    }

    fn visible_icon_slot_sizes(&self, icons: &[WorkspaceIcon<'_>]) -> Vec<f32> {
        let (visible, _) = visible_icon_counts(icons.len(), self.config.max_visible_icons);
        let icon_size = self.app_icon_size();
        let uniform = !self.config.show_workspace_numbers
            && self.config.highlight_maximized_window_icons
            && icons.iter().take(visible).any(|icon| icon.maximized);
        icons
            .iter()
            .take(visible)
            .map(|icon| {
                Self::app_icon_slot_size(
                    icon_size,
                    uniform || (self.config.highlight_maximized_window_icons && icon.maximized),
                )
            })
            .collect()
    }

    fn workspace_button_major_size(&self, icons: &[WorkspaceIcon<'_>]) -> f32 {
        workspace_major_size(
            if self.config.show_workspace_numbers {
                self.suggested_button_size()
            } else {
                self.workspace_cross_axis_size()
            },
            self.number_section_major_size(true),
            self.app_group_major_size(icons),
            self.config.show_workspace_numbers,
            self.workspace_content_edge_padding(icons),
        )
    }

    fn workspace_content_edge_padding(&self, icons: &[WorkspaceIcon<'_>]) -> f32 {
        if icons.is_empty() {
            return 0.0;
        }
        let largest_slot = self
            .visible_icon_slot_sizes(icons)
            .into_iter()
            .fold(0.0_f32, f32::max);
        workspace_edge_padding(
            self.config.show_workspace_numbers,
            self.workspace_cross_axis_size(),
            largest_slot,
        )
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
        slot_size: f32,
    ) -> Element<'_, Message> {
        let opacity = if minimized {
            MINIMIZED_ICON_OPACITY
        } else {
            1.0
        };
        let handle = metadata.icon_source.as_cosmic_icon();
        let symbolic = handle.symbolic;

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

        if slot_size > icon_size {
            container(icon)
                .width(Length::Fixed(slot_size))
                .height(Length::Fixed(slot_size))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .class(ContainerClass::Custom(Box::new(move |_| {
                    if !maximized {
                        return container::Style::default();
                    }
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
        let pill_style = self.config.pill_style;
        let inactive_pill_style = self.config.inactive_pill_style;
        let outlined_border_width = f32::from(self.config.pill_border_width);
        let inactive_brightness_percent = self.config.inactive_pill_brightness_percent;
        let show_numbers = self.config.show_workspace_numbers;
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
            .zip(self.visible_icon_slot_sizes(icons))
            .map(|(icon, slot_size)| {
                self.app_icon(
                    icon.metadata,
                    icon_size,
                    self.config.dim_minimized_window_icons && icon.minimized,
                    self.config.highlight_maximized_window_icons && icon.maximized,
                    slot_size,
                )
            })
            .collect::<Vec<_>>();
        if overflow_count > 0 {
            let size = workspace_overflow_size(&self.core.applet, icon_size, overflow_count);
            icon_elements.push(
                container(
                    workspace_overflow_text(&self.core.applet, icon_size, overflow_count)
                        .width(size.width)
                        .height(size.height),
                )
                .class(ContainerClass::Custom(Box::new(move |theme| {
                    Self::workspace_indicator_style(theme, active, pill_style, hovered)
                })))
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

        let app_group: Element<'_, Message> = container(app_strip)
            .class(ContainerClass::Custom(Box::new(|_| {
                container::Style::default()
            })))
            .padding(oriented_padding(
                self.layout,
                app_group_leading_padding(self.config.show_workspace_numbers),
                APP_GROUP_TRAILING_PADDING,
                if show_numbers {
                    APP_GROUP_CROSS_AXIS_PADDING
                } else {
                    0.0
                },
            ))
            .into();
        let pinned = workspace
            .cosmic_state
            .contains(zcosmic_workspace_handle_v2::State::Pinned);
        // Use the complete monitor-local list, not the slice that fits on the panel.
        let is_last = self
            .workspaces
            .last()
            .is_some_and(|last| last.handle == workspace.handle);
        let indicator = workspace_indicator(
            self.config.show_workspace_numbers,
            !icons.is_empty(),
            pinned,
            is_last,
            show_empty_workspace_dot(inactive_pill_style, self.config.pill_border_width),
        );
        let cutout = matches!(
            indicator,
            WorkspaceIndicator::Plus | WorkspaceIndicator::Empty
        ) && workspace_symbol_is_cutout(active, inactive_pill_style, hovered);
        let content: Element<'_, Message> = match indicator {
            WorkspaceIndicator::None => app_group,
            WorkspaceIndicator::Plus | WorkspaceIndicator::Empty if cutout => {
                space::horizontal().into()
            }
            WorkspaceIndicator::Plus | WorkspaceIndicator::Empty => Element::new(WorkspaceSymbol {
                handle: svg::Handle::from_memory(indicator.symbol_svg()),
                size: f32::from(icon_size as u16),
            }),
            WorkspaceIndicator::Label => {
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
                let number =
                    container(number_text).class(ContainerClass::Custom(Box::new(move |theme| {
                        Self::workspace_indicator_style(theme, active, pill_style, hovered)
                    })));
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

                if icons.is_empty() {
                    number
                } else if horizontal {
                    let divider: Element<'_, Message> =
                        container(space::vertical().height(Length::Fixed(icon_size * 0.8)))
                            .width(Length::Fixed(WORKSPACE_DIVIDER_WIDTH))
                            .class(ContainerClass::Custom(Box::new(move |theme| {
                                Self::workspace_divider_style(
                                    theme, active, urgent, pill_style, hovered,
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
                                    theme, active, urgent, pill_style, hovered,
                                )
                            })))
                            .into();

                    column![number, divider, app_group]
                        .spacing(WORKSPACE_CONTENT_SPACING)
                        .align_x(Alignment::Center)
                        .into()
                }
            }
        };

        // Unfilled symbols match the outline; active filled symbols keep their foreground.
        let content = if matches!(
            indicator,
            WorkspaceIndicator::Plus | WorkspaceIndicator::Empty
        ) {
            container(content)
                .class(ContainerClass::Custom(Box::new(move |theme| {
                    Self::workspace_symbol_style(Self::workspace_pill_style(
                        theme,
                        active,
                        urgent,
                        hovered,
                        pill_style,
                        inactive_pill_style,
                        outlined_border_width,
                        inactive_brightness_percent,
                    ))
                })))
                .into()
        } else {
            content
        };

        let has_apps = !icons.is_empty();
        let content_edge_padding = self.workspace_content_edge_padding(icons);
        let layout = self.layout;
        let pill_background: Element<'_, Message> =
            container(Element::new(WorkspacePillBackground {
                cutout: cutout.then_some((indicator, f32::from(icon_size as u16))),
                style: Box::new(move |theme| {
                    let mut style = Self::workspace_pill_style(
                        theme,
                        active,
                        urgent,
                        hovered,
                        pill_style,
                        inactive_pill_style,
                        outlined_border_width,
                        inactive_brightness_percent,
                    );
                    if !show_numbers {
                        style.border.radius = workspace_pill_radii(
                            theme.cosmic().radius_xl(),
                            width,
                            height,
                            layout,
                            pill_cross_axis_inset,
                        )
                        .into();
                    }
                    style
                }),
            }))
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
                    pill_style,
                    inactive_pill_style,
                    outlined_border_width,
                    inactive_brightness_percent,
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
                oriented_padding(self.layout, content_edge_padding, content_edge_padding, 0.0)
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
        MAX_INACTIVE_PILL_BRIGHTNESS_PERCENT, MAX_VISIBLE_ICONS,
        MIN_INACTIVE_PILL_BRIGHTNESS_PERCENT, MIN_VISIBLE_ICONS, WorkspacePillStyle,
    };
    use cosmic::cosmic_theme::{
        CornerRadii, Roundness,
        palette::{IntoColor, Oklab, Srgb},
    };

    use super::{
        APP_GROUP_LEADING_PADDING, APP_GROUP_TRAILING_PADDING, APP_ICON_SPACING, AppMetadata,
        Background, Color, IcedWorkspacesApplet, Layout, MAX_PILL_BORDER_WIDTH,
        MIN_PILL_BORDER_WIDTH, Theme, URGENT_FILLED_BORDER_WIDTH, WORKSPACE_CONTENT_PADDING,
        WORKSPACE_CONTENT_SPACING, WORKSPACE_LIST_EDGE_PADDING, PillStyleSelection, WorkspaceApp,
        WorkspaceIndicator, WorkspaceWindowState, app_group_leading_padding, display_icons,
        inactive_pill_brightness_percent, inactive_pill_color, informative_titles,
        occupied_number_section_major_size, oriented_padding, pill_border_width,
        pill_spacing_percent, should_retain_toplevel_placement, visible_icon_counts,
        visible_icon_limit, workspace_cutout_svg, workspace_edge_padding, workspace_indicator,
        workspace_list_padding, workspace_major_size, workspace_number_font_size,
        workspace_overflow_size, workspace_overview_command, workspace_pill_radii,
        show_empty_workspace_dot, workspace_symbol_is_cutout, workspace_tooltip,
    };

    const TEST_OUTLINED_BORDER_WIDTH: f32 = 2.0;

    #[test]
    fn selects_workspace_labels_and_empty_placeholders() {
        // (has icons, pinned, last, expected when numbers are hidden)
        let cases = [
            (false, false, false, WorkspaceIndicator::Empty),
            (false, false, true, WorkspaceIndicator::Plus),
            (false, true, false, WorkspaceIndicator::Empty),
            (false, true, true, WorkspaceIndicator::Empty),
            (true, false, false, WorkspaceIndicator::None),
            (true, false, true, WorkspaceIndicator::None),
            (true, true, false, WorkspaceIndicator::None),
            (true, true, true, WorkspaceIndicator::None),
        ];
        for (has_icons, pinned, is_last, expected) in cases {
            assert_eq!(
                workspace_indicator(true, has_icons, pinned, is_last, true),
                WorkspaceIndicator::Label
            );
            assert_eq!(
                workspace_indicator(false, has_icons, pinned, is_last, true),
                expected
            );
        }
    }

    #[test]
    fn shows_empty_dot_only_for_zero_width_active_filled_or_all_outlined_pills() {
        for selection in PillStyleSelection::ALL {
            let (_, inactive_style) = selection.styles();
            for border_width in [0, 1, 2, 3] {
                let show_dot = show_empty_workspace_dot(inactive_style, border_width);
                assert_eq!(
                    show_dot,
                    selection != PillStyleSelection::AllFilled && border_width == 0
                );
                for pinned in [false, true] {
                    assert_eq!(
                        workspace_indicator(false, false, pinned, false, show_dot),
                        if show_dot {
                            WorkspaceIndicator::Empty
                        } else {
                            WorkspaceIndicator::None
                        }
                    );
                }
                assert_eq!(
                    workspace_indicator(false, false, false, true, show_dot),
                    WorkspaceIndicator::Plus
                );
                assert_eq!(
                    workspace_indicator(true, false, true, false, show_dot),
                    WorkspaceIndicator::Label
                );
            }
        }
    }

    #[test]
    fn round_single_icon_pills_are_circular_at_every_panel_size() {
        let theme_radii = CornerRadii::from(Roundness::Round).radius_xl;
        for cross_size in [24.0_f32, 32.0, 40.0, 56.0, 80.0] {
            let icon_size = (cross_size * 0.52).max(16.0);
            for maximized in [false, true] {
                let slot = IcedWorkspacesApplet::app_icon_slot_size(icon_size, maximized);
                let padding = workspace_edge_padding(false, cross_size, slot);
                let major = workspace_major_size(cross_size, 28.0, slot, false, padding);
                assert!((major - cross_size).abs() < 0.001);
                assert!((padding * 2.0 + slot - cross_size).abs() < 0.001);
                for layout in [Layout::Row, Layout::Column] {
                    let (width, height) = match layout {
                        Layout::Row => (major, cross_size),
                        Layout::Column => (cross_size, major),
                    };
                    for radius in workspace_pill_radii(theme_radii, width, height, layout, 0.0) {
                        assert!((radius - cross_size / 2.0).abs() < 0.001);
                    }
                }
            }
        }
        assert_eq!(app_group_leading_padding(false), APP_GROUP_TRAILING_PADDING);
    }

    #[test]
    fn additional_icons_and_overflow_extend_the_pill_without_changing_end_padding() {
        let cross_size = 40.0;
        let slot = 20.8;
        let padding = workspace_edge_padding(false, cross_size, slot);
        for additional_content in [
            slot + APP_ICON_SPACING,
            4.0 * (slot + APP_ICON_SPACING),
            workspace_overflow_size(&cosmic::applet::Context::default(), slot, 1).width
                + APP_ICON_SPACING,
        ] {
            let major =
                workspace_major_size(cross_size, 28.0, slot + additional_content, false, padding);
            assert!((major - cross_size - additional_content).abs() < 0.001);
        }
        // Icon-only mixed groups use the same enlarged slot for each frame.
        let highlighted = IcedWorkspacesApplet::app_icon_slot_size(slot, true);
        let padding = workspace_edge_padding(false, cross_size, highlighted);
        let major = workspace_major_size(
            cross_size,
            28.0,
            highlighted * 2.0 + APP_ICON_SPACING,
            false,
            padding,
        );
        assert!((major - (cross_size + APP_ICON_SPACING + highlighted)).abs() < 0.001);
    }

    #[test]
    fn numbered_layout_keeps_its_existing_spacing_and_empty_workspaces_keep_their_size() {
        let padding = workspace_edge_padding(true, 40.0, 20.8);
        assert_eq!(padding, 8.0);
        assert!((workspace_major_size(40.0, 28.0, 24.8, true, padding) - 77.8).abs() < 0.001);
        for show_numbers in [false, true] {
            assert_eq!(
                workspace_major_size(40.0, 28.0, 0.0, show_numbers, 0.0),
                40.0
            );
        }
    }

    #[test]
    fn capsule_radius_tracks_visible_bounds_in_both_orientations() {
        let theme_radii = CornerRadii::from(Roundness::Round).radius_xl;
        for (width, height, layout, inset, expected) in [
            (64.0, 40.0, Layout::Row, 0.0, 20.0),
            (40.0, 64.0, Layout::Column, 0.0, 20.0),
            (40.0, 40.0, Layout::Row, 4.0, 16.0),
            (40.0, 40.0, Layout::Column, 4.0, 16.0),
        ] {
            assert_eq!(
                workspace_pill_radii(theme_radii, width, height, layout, inset),
                [expected; 4],
            );
        }
    }

    #[test]
    fn hidden_numbers_preserve_slightly_round_and_square_theme_corners() {
        for (roundness, expected) in [(Roundness::SlightlyRound, 8.0), (Roundness::Square, 2.0)] {
            let theme_radii = CornerRadii::from(roundness).radius_xl;
            for cross_size in [24.0, 32.0, 40.0, 56.0, 80.0] {
                for major_size in [cross_size, cross_size * 3.0] {
                    for inset in [0.0, 4.0] {
                        for layout in [Layout::Row, Layout::Column] {
                            let (width, height) = match layout {
                                Layout::Row => (major_size, cross_size),
                                Layout::Column => (cross_size, major_size),
                            };
                            assert_eq!(
                                workspace_pill_radii(theme_radii, width, height, layout, inset),
                                [expected; 4],
                            );
                        }
                    }
                }
            }
        }
    }

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
        active_pill_style: WorkspacePillStyle,
        inactive_pill_style: WorkspacePillStyle,
    ) -> cosmic::widget::container::Style {
        IcedWorkspacesApplet::workspace_pill_style(
            theme,
            active,
            urgent,
            hovered,
            active_pill_style,
            inactive_pill_style,
            TEST_OUTLINED_BORDER_WIDTH,
            0,
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
        assert_eq!(WORKSPACE_CONTENT_PADDING, 8.0);
        assert_eq!(WORKSPACE_CONTENT_SPACING, 4.0);
        assert_eq!(WORKSPACE_CONTENT_SPACING + APP_GROUP_LEADING_PADDING, 8.0);
        assert_eq!(APP_ICON_SPACING, 4.0);
        assert_eq!(APP_GROUP_TRAILING_PADDING + WORKSPACE_CONTENT_PADDING, 8.0);
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
                WorkspacePillStyle::Outlined,
                WorkspacePillStyle::Outlined,
                3.0,
                0,
            );
            assert_eq!(style.border.width, 3.0);
        }
    }

    #[test]
    fn keeps_active_and_urgent_styles_independent_of_inactive_brightness() {
        let theme = Theme::default();

        for (active, urgent, hovered, outlined_mode) in [
            (true, false, false, WorkspacePillStyle::Filled),
            (true, false, true, WorkspacePillStyle::Outlined),
            (false, true, false, WorkspacePillStyle::Filled),
            (false, true, true, WorkspacePillStyle::Outlined),
        ] {
            let minimum_brightness = IcedWorkspacesApplet::workspace_pill_style(
                &theme,
                active,
                urgent,
                hovered,
                outlined_mode,
                outlined_mode,
                TEST_OUTLINED_BORDER_WIDTH,
                MIN_INACTIVE_PILL_BRIGHTNESS_PERCENT,
            );
            let maximum_brightness = IcedWorkspacesApplet::workspace_pill_style(
                &theme,
                active,
                urgent,
                hovered,
                outlined_mode,
                outlined_mode,
                TEST_OUTLINED_BORDER_WIDTH,
                MAX_INACTIVE_PILL_BRIGHTNESS_PERCENT,
            );

            assert_eq!(minimum_brightness, maximum_brightness);
        }
    }

    #[test]
    fn zero_brightness_uses_exact_cosmic_button_colors() {
        for theme in [Theme::dark(), Theme::light()] {
            let button = &theme.cosmic().button;

            for inactive_style in [WorkspacePillStyle::Filled, WorkspacePillStyle::Outlined] {
                for hovered in [false, true] {
                    let style = IcedWorkspacesApplet::workspace_pill_style(
                        &theme,
                        false,
                        false,
                        hovered,
                        WorkspacePillStyle::Filled,
                        inactive_style,
                        TEST_OUTLINED_BORDER_WIDTH,
                        0,
                    );
                    let expected = Color::from(if hovered { button.hover } else { button.base });
                    assert_eq!(
                        style.background,
                        (inactive_style == WorkspacePillStyle::Filled || hovered)
                            .then_some(Background::Color(expected))
                    );
                    if inactive_style == WorkspacePillStyle::Outlined && !hovered {
                        assert_eq!(style.border.color, expected);
                    }
                }
            }
        }
    }

    #[test]
    fn brightness_adjusts_lightness_and_preserves_each_button_alpha() {
        for theme in [Theme::dark(), Theme::light()] {
            for token in [theme.cosmic().button.base, theme.cosmic().button.hover] {
                let darker = inactive_pill_color(token, MIN_INACTIVE_PILL_BRIGHTNESS_PERCENT);
                let original = inactive_pill_color(token, 0);
                let lighter = inactive_pill_color(token, MAX_INACTIVE_PILL_BRIGHTNESS_PERCENT);
                let lightness = |color: Color| -> f32 {
                    let lab: Oklab = Srgb::new(color.r, color.g, color.b).into_color();
                    lab.l
                };

                assert!(lightness(darker) < lightness(original));
                assert!(lightness(original) < lightness(lighter));
                assert_eq!(darker.a, original.a);
                assert_eq!(lighter.a, original.a);
                assert_eq!(original.a, token.alpha);
            }
        }
    }

    #[test]
    fn nonzero_brightness_applies_to_both_inactive_pill_styles() {
        let theme = Theme::default();
        let button = &theme.cosmic().button;

        for brightness in [
            MIN_INACTIVE_PILL_BRIGHTNESS_PERCENT,
            MAX_INACTIVE_PILL_BRIGHTNESS_PERCENT,
        ] {
            for active_style in [WorkspacePillStyle::Filled, WorkspacePillStyle::Outlined] {
                for inactive_style in [WorkspacePillStyle::Filled, WorkspacePillStyle::Outlined] {
                    for hovered in [false, true] {
                        let style = IcedWorkspacesApplet::workspace_pill_style(
                            &theme,
                            false,
                            false,
                            hovered,
                            active_style,
                            inactive_style,
                            TEST_OUTLINED_BORDER_WIDTH,
                            brightness,
                        );
                        let token = if hovered { button.hover } else { button.base };
                        let expected = inactive_pill_color(token, brightness);

                        assert_eq!(
                            style.background,
                            (inactive_style == WorkspacePillStyle::Filled || hovered)
                                .then_some(Background::Color(expected))
                        );
                        if inactive_style == WorkspacePillStyle::Outlined && !hovered {
                            assert_eq!(style.border.color, expected);
                        }
                        assert_eq!(expected.a, token.alpha);
                    }
                }
            }
        }
    }

    #[test]
    fn brightness_is_clamped_to_seventy_five_percent_in_either_direction() {
        assert_eq!(inactive_pill_brightness_percent(i8::MIN), -75);
        assert_eq!(inactive_pill_brightness_percent(0), 0);
        assert_eq!(inactive_pill_brightness_percent(i8::MAX), 75);
    }

    #[test]
    fn matches_nonurgent_hovered_outlined_borders_to_their_backgrounds() {
        let theme = Theme::default();
        let styles = [
            test_pill_style(
                &theme,
                true,
                false,
                true,
                WorkspacePillStyle::Outlined,
                WorkspacePillStyle::Outlined,
            ),
            test_pill_style(
                &theme,
                false,
                false,
                true,
                WorkspacePillStyle::Filled,
                WorkspacePillStyle::Outlined,
            ),
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
            let style = test_pill_style(
                &theme,
                active,
                urgent,
                true,
                WorkspacePillStyle::Outlined,
                WorkspacePillStyle::Outlined,
            );
            assert!(style.background.is_some());
            assert_eq!(style.border.width, 0.0);
        }
    }

    #[test]
    fn gives_urgent_pills_a_destructive_text_and_border() {
        let theme = Theme::default();
        let destructive = Color::from(theme.cosmic().destructive_button.base);

        for outlined_mode in [WorkspacePillStyle::Filled, WorkspacePillStyle::Outlined] {
            for hovered in [false, true] {
                let style = test_pill_style(
                    &theme,
                    false,
                    true,
                    hovered,
                    WorkspacePillStyle::Filled,
                    outlined_mode,
                );

                assert_eq!(style.text_color, Some(destructive));
                assert_eq!(style.icon_color, Some(destructive));
                assert_eq!(style.border.color, destructive);
                let token = if hovered {
                    theme.cosmic().button.hover
                } else {
                    theme.cosmic().button.base
                };
                assert_eq!(
                    style.background,
                    (outlined_mode == WorkspacePillStyle::Filled || hovered)
                        .then_some(Background::Color(token.into()))
                );
                assert_eq!(
                    style.border.width,
                    if outlined_mode == WorkspacePillStyle::Outlined {
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
        let style = test_pill_style(
            &theme,
            true,
            false,
            false,
            WorkspacePillStyle::Filled,
            WorkspacePillStyle::Filled,
        );

        assert_eq!(
            style.background,
            Some(Background::Color(theme.cosmic().accent_button.base.into()))
        );
        assert_eq!(style.border.width, 0.0);
    }

    #[test]
    fn keeps_the_full_opacity_active_pill_when_hovered() {
        let theme = Theme::default();
        let style = test_pill_style(
            &theme,
            true,
            false,
            true,
            WorkspacePillStyle::Filled,
            WorkspacePillStyle::Filled,
        );

        assert_eq!(
            style.background,
            Some(Background::Color(theme.cosmic().accent_button.hover.into()))
        );
        assert_eq!(style.border.width, 0.0);
    }

    #[test]
    fn uses_an_accent_outline_in_outlined_mode() {
        let theme = Theme::default();
        let style = test_pill_style(
            &theme,
            true,
            false,
            false,
            WorkspacePillStyle::Outlined,
            WorkspacePillStyle::Filled,
        );

        assert_eq!(style.background, None);
        assert_eq!(style.border.color, theme.cosmic().accent_button.base.into());
        assert_eq!(style.border.width, TEST_OUTLINED_BORDER_WIDTH);
        let panel_foreground = theme.current_container().component.on.into();
        assert_eq!(style.text_color, Some(panel_foreground));
        assert_eq!(style.icon_color, Some(panel_foreground));
    }

    #[test]
    fn fills_the_active_outlined_pill_on_hover() {
        let theme = Theme::default();
        let style = test_pill_style(
            &theme,
            true,
            false,
            true,
            WorkspacePillStyle::Outlined,
            WorkspacePillStyle::Filled,
        );

        let expected = Color::from(theme.cosmic().accent_button.hover);
        assert_eq!(style.background, Some(Background::Color(expected)));
        assert_eq!(
            style.border.color,
            theme.cosmic().accent_button.hover.into()
        );
        assert_eq!(
            style.text_color,
            Some(theme.cosmic().accent_button.on.into())
        );
        assert_eq!(
            style.icon_color,
            Some(theme.cosmic().accent_button.on.into())
        );
        assert_eq!(style.border.width, 0.0);
    }

    #[test]
    fn gives_the_active_indicator_an_accent_text_color_in_outlined_mode() {
        let theme = Theme::default();
        let style = IcedWorkspacesApplet::workspace_indicator_style(
            &theme,
            true,
            WorkspacePillStyle::Outlined,
            false,
        );
        let expected = theme
            .cosmic()
            .accent_text
            .unwrap_or(theme.cosmic().accent_button.base);

        assert_eq!(style.text_color, Some(expected.into()));
        assert_eq!(style.icon_color, Some(expected.into()));
    }

    #[test]
    fn gives_the_active_indicator_on_accent_text_when_hovered() {
        let theme = Theme::default();
        let style = IcedWorkspacesApplet::workspace_indicator_style(
            &theme,
            true,
            WorkspacePillStyle::Outlined,
            true,
        );

        assert_eq!(
            style.text_color,
            Some(theme.cosmic().accent_button.on.into())
        );
        assert_eq!(
            style.icon_color,
            Some(theme.cosmic().accent_button.on.into())
        );
    }

    #[test]
    fn empty_workspace_symbols_match_the_outline_including_brightness_and_urgency() {
        for theme in [Theme::dark(), Theme::light()] {
            for active in [false, true] {
                for urgent in [false, true] {
                    for brightness in [-75, 0, 75] {
                        let pill = IcedWorkspacesApplet::workspace_pill_style(
                            &theme,
                            active,
                            urgent,
                            false,
                            WorkspacePillStyle::Outlined,
                            WorkspacePillStyle::Outlined,
                            0.0,
                            brightness,
                        );
                        let symbol = IcedWorkspacesApplet::workspace_symbol_style(pill);
                        assert_eq!(symbol.icon_color, Some(pill.border.color));
                        assert_eq!(symbol.text_color, Some(pill.border.color));
                    }
                }
            }
        }
    }

    #[test]
    fn renders_symbols_and_cutout_fills_with_the_same_opacity_as_native_pills() {
        use super::{
            ContainerClass, Element, IcedLayout, Length, Rectangle, Size, Tree,
            WorkspacePillBackground, WorkspaceSymbol, container, iced_layout, mouse, renderer, svg,
        };
        use cosmic::iced::{Font, Pixels, Point};
        use renderer::{Headless as _, Renderer as _};

        let mut renderer =
            futures::executor::block_on(<cosmic::Renderer as renderer::Headless>::new(
                Font::DEFAULT,
                Pixels(16.0),
                Some("tiny-skia"),
            ))
            .expect("software renderer");
        let viewport = Rectangle::with_size(Size::new(88.0, 40.0));
        let panel = Color::from_rgb8(61, 79, 111);

        for theme in [Theme::dark(), Theme::light()] {
            for brightness in [-75, 0, 75] {
                for indicator in [WorkspaceIndicator::Empty, WorkspaceIndicator::Plus] {
                    for cutout in [false, true] {
                        renderer.reset(viewport);
                        let style = if cutout {
                            WorkspacePillStyle::Filled
                        } else {
                            WorkspacePillStyle::Outlined
                        };
                        let pill = IcedWorkspacesApplet::workspace_pill_style(
                            &theme, false, false, false, style, style, 3.0, brightness,
                        );
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::with_size(Size::new(40.0, 40.0)),
                                border: pill.border,
                                ..Default::default()
                            },
                            pill.background
                                .unwrap_or(Background::Color(Color::TRANSPARENT)),
                        );
                        let mut visual: Element<'_, ()> = if cutout {
                            Element::new(WorkspacePillBackground {
                                style: Box::new(move |_| pill),
                                cutout: Some((indicator, 24.0)),
                            })
                        } else {
                            container(Element::new(WorkspaceSymbol {
                                handle: svg::Handle::from_memory(indicator.symbol_svg()),
                                size: 24.0,
                            }))
                            .center_x(Length::Fixed(40.0))
                            .center_y(Length::Fixed(40.0))
                            .class(ContainerClass::Custom(Box::new(move |_| {
                                IcedWorkspacesApplet::workspace_symbol_style(pill)
                            })))
                            .into()
                        };
                        let mut tree = Tree::new(&visual);
                        let layout = visual
                            .as_widget_mut()
                            .layout(
                                &mut tree,
                                &renderer,
                                &iced_layout::Limits::new(Size::ZERO, Size::new(40.0, 40.0)),
                            )
                            .move_to(Point::new(48.0, 0.0));
                        visual.as_widget().draw(
                            &tree,
                            &mut renderer,
                            &theme,
                            &renderer::Style::default(),
                            IcedLayout::new(&layout),
                            mouse::Cursor::Unavailable,
                            &viewport,
                        );
                        let pixels = renderer.screenshot(Size::new(88, 40), 1.0, panel);
                        let pixel =
                            |x: usize, y: usize| &pixels[(y * 88 + x) * 4..(y * 88 + x) * 4 + 4];
                        let (reference, actual) = if cutout {
                            (pixel(20, 20), pixel(53, 20))
                        } else {
                            (pixel(20, 1), pixel(68, 20))
                        };
                        for (expected, rendered) in reference.iter().zip(actual) {
                            assert!(
                                expected.abs_diff(*rendered) <= 1,
                                "pill={reference:?}, svg={actual:?}, cutout={cutout}"
                            );
                        }
                        if cutout {
                            assert_eq!(pixel(68, 20), panel.into_rgba8());
                        }
                    }
                }
            }
        }
    }

    fn compare_cutout_with_native_pill(
        theme: &Theme,
        pill: cosmic::widget::container::Style,
        bounds: cosmic::iced::Rectangle,
        scale: f32,
        indicator: WorkspaceIndicator,
    ) -> (Vec<u8>, Vec<u8>, usize) {
        use super::{Element, IcedLayout, Size, Tree, WorkspacePillBackground, mouse, renderer};
        use cosmic::iced::{Font, Pixels};
        use renderer::{Headless as _, Renderer as _};

        let mut renderer =
            futures::executor::block_on(<cosmic::Renderer as renderer::Headless>::new(
                Font::DEFAULT,
                Pixels(16.0),
                Some("tiny-skia"),
            ))
            .expect("software renderer");
        let viewport = cosmic::iced::Rectangle::with_size(Size::new(64.0, 64.0));
        let size = Size::new((64.0 * scale) as u32, (64.0 * scale) as u32);
        let panel = Color::from_rgb8(61, 79, 111);
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: pill.border,
                snap: pill.snap,
                ..Default::default()
            },
            pill.background
                .unwrap_or(Background::Color(Color::TRANSPARENT)),
        );
        let native = renderer.screenshot(size, scale, panel);
        renderer.reset(viewport);
        let visual: Element<'_, ()> = Element::new(WorkspacePillBackground {
            style: Box::new(move |_| pill),
            cutout: Some((indicator, 24.0)),
        });
        let tree = Tree::new(&visual);
        let layout = super::iced_layout::Node::new(bounds.size()).move_to(bounds.position());
        visual.as_widget().draw(
            &tree,
            &mut renderer,
            theme,
            &renderer::Style {
                scale_factor: f64::from(scale),
                ..Default::default()
            },
            IcedLayout::new(&layout),
            mouse::Cursor::Unavailable,
            &viewport,
        );
        let actual = renderer.screenshot(size, scale, panel);
        (native, actual, size.width as usize)
    }

    #[test]
    fn cutout_pills_preserve_urgent_border_colors() {
        for theme in [Theme::dark(), Theme::light()] {
            for (mode, hovered) in [
                (WorkspacePillStyle::Filled, false),
                (WorkspacePillStyle::Filled, true),
                (WorkspacePillStyle::Outlined, true),
            ] {
                let pill = test_pill_style(&theme, false, true, hovered, mode, mode);
                let bounds = cosmic::iced::Rectangle {
                    x: 8.0,
                    y: 8.0,
                    width: 40.0,
                    height: 40.0,
                };
                for indicator in [WorkspaceIndicator::Empty, WorkspaceIndicator::Plus] {
                    for scale in [1.0, 1.25, 1.5, 2.0] {
                        let (native, actual, width) =
                            compare_cutout_with_native_pill(&theme, pill, bounds, scale, indicator);
                        let start = (8.0 * scale) as usize;
                        let end = (48.0 * scale) as usize - 1;
                        let center = (28.0 * scale) as usize;
                        // Compare the middle of each straight border segment.
                        for (x, y) in [
                            (center, start),
                            (center, end),
                            (start, center),
                            (end, center),
                        ] {
                            let offset = (y * width + x) * 4;
                            assert_eq!(
                                &native[offset..offset + 4],
                                &actual[offset..offset + 4],
                                "urgent border changed for {mode:?} at ({x}, {y}), scale={scale}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cutout_pill_edges_follow_fractional_bounds_and_display_scale() {
        for theme in [Theme::dark(), Theme::light()] {
            for inset in [0.4_f32, 1.6, 4.0] {
                for horizontal in [false, true] {
                    let bounds = cosmic::iced::Rectangle {
                        x: 8.0 + if horizontal { 0.0 } else { inset },
                        y: 8.0 + if horizontal { inset } else { 0.0 },
                        width: 40.0 - if horizontal { 0.0 } else { inset * 2.0 },
                        height: 40.0 - if horizontal { inset * 2.0 } else { 0.0 },
                    };
                    let mut pill = IcedWorkspacesApplet::workspace_pill_style(
                        &theme,
                        false,
                        false,
                        false,
                        WorkspacePillStyle::Filled,
                        WorkspacePillStyle::Filled,
                        2.0,
                        75,
                    );
                    pill.border.radius = (bounds.width.min(bounds.height) / 2.0).into();
                    for scale in [1.0, 1.2, 1.25, 1.5, 1.75, 2.0] {
                        let (native, actual, width) = compare_cutout_with_native_pill(
                            &theme,
                            pill,
                            bounds,
                            scale,
                            WorkspaceIndicator::Empty,
                        );
                        for (index, (expected, rendered)) in native
                            .chunks_exact(4)
                            .zip(actual.chunks_exact(4))
                            .enumerate()
                        {
                            let x = (index % width) as f32 / scale;
                            let y = (index / width) as f32 / scale;
                            let center = bounds.center();
                            if (x - center.x).abs() < 13.0 && (y - center.y).abs() < 13.0 {
                                continue;
                            }
                            for (a, b) in expected.iter().zip(rendered) {
                                // Allow small differences in curve anti-aliasing, not lost rows.
                                assert!(
                                    a.abs_diff(*b) <= 8,
                                    "clipped or shifted edge at ({x}, {y}): native={expected:?}, \
                                     cutout={rendered:?}, inset={inset}, scale={scale}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn only_inactive_filled_pills_cut_out_their_symbols() {
        for (active, style, hovered, expected) in [
            (false, WorkspacePillStyle::Filled, false, true),
            (false, WorkspacePillStyle::Filled, true, true),
            (false, WorkspacePillStyle::Outlined, false, false),
            (false, WorkspacePillStyle::Outlined, true, true),
            (true, WorkspacePillStyle::Filled, false, false),
            (true, WorkspacePillStyle::Filled, true, false),
            (true, WorkspacePillStyle::Outlined, false, false),
            (true, WorkspacePillStyle::Outlined, true, false),
        ] {
            assert_eq!(workspace_symbol_is_cutout(active, style, hovered), expected);
        }
    }

    #[test]
    fn focused_filled_symbols_keep_their_existing_color() {
        for theme in [Theme::dark(), Theme::light()] {
            for active_style in [WorkspacePillStyle::Filled, WorkspacePillStyle::Outlined] {
                for hovered in [false, true] {
                    let pill = test_pill_style(
                        &theme,
                        true,
                        false,
                        hovered,
                        active_style,
                        WorkspacePillStyle::Outlined,
                    );
                    if pill.background.is_none() {
                        continue;
                    }
                    let symbol = IcedWorkspacesApplet::workspace_symbol_style(pill);
                    let previous = IcedWorkspacesApplet::workspace_indicator_style(
                        &theme,
                        true,
                        active_style,
                        hovered,
                    );
                    assert_eq!(symbol.icon_color, previous.icon_color.or(pill.icon_color));
                }
            }
        }
    }

    #[test]
    fn cutout_svg_has_a_transparent_symbol_and_preserves_pill_corners() {
        use resvg::{tiny_skia, usvg};

        for indicator in [WorkspaceIndicator::Empty, WorkspaceIndicator::Plus] {
            // Horizontal/vertical pills and every corner style.
            for (width, height) in [(48, 40), (40, 48), (40, 40)] {
                for radius in [0.0, 4.0, 20.0] {
                    let svg = workspace_cutout_svg(
                        indicator,
                        24.0,
                        super::Size::new(width as f32, height as f32),
                        super::Rectangle::with_size(super::Size::new(width as f32, height as f32)),
                        [radius; 4],
                    );
                    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())
                        .expect("valid cutout SVG");
                    let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
                    resvg::render(
                        &tree,
                        tiny_skia::Transform::identity(),
                        &mut pixmap.as_mut(),
                    );
                    assert_eq!(pixmap.pixel(width / 2, height / 2).unwrap().alpha(), 0);
                    assert_eq!(pixmap.pixel(2, height / 2).unwrap().alpha(), 255);
                    let corner_alpha = pixmap.pixel(0, 0).unwrap().alpha();
                    assert_eq!(corner_alpha, if radius == 0.0 { 255 } else { 0 });

                    let solid =
                        usvg::Tree::from_data(indicator.symbol_svg(), &usvg::Options::default())
                            .expect("valid symbol SVG");
                    let mut symbol = tiny_skia::Pixmap::new(width, height).unwrap();
                    let viewbox_size = match indicator {
                        WorkspaceIndicator::Plus => 16.0,
                        _ => 24.0,
                    };
                    let scale = 24.0 / viewbox_size;
                    let transform = tiny_skia::Transform::from_scale(scale, scale)
                        .post_translate((width as f32 - 24.0) / 2.0, (height as f32 - 24.0) / 2.0);
                    resvg::render(&solid, transform, &mut symbol.as_mut());
                    for y in height / 2 - 10..height / 2 + 10 {
                        for x in width / 2 - 10..width / 2 + 10 {
                            let sum = u16::from(pixmap.pixel(x, y).unwrap().alpha())
                                + u16::from(symbol.pixel(x, y).unwrap().alpha());
                            assert!(
                                (254..=256).contains(&sum),
                                "cutout must match the solid glyph"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn gives_the_active_divider_an_accent_color_in_outlined_mode() {
        let theme = Theme::default();
        let style = IcedWorkspacesApplet::workspace_divider_style(
            &theme,
            true,
            false,
            WorkspacePillStyle::Outlined,
            false,
        );
        let expected = theme
            .cosmic()
            .accent_text
            .unwrap_or(theme.cosmic().accent_button.base);

        assert_eq!(style.background, Some(Background::Color(expected.into())));
    }

    #[test]
    fn gives_the_active_divider_an_on_accent_color_when_hovered() {
        let theme = Theme::default();
        let style = IcedWorkspacesApplet::workspace_divider_style(
            &theme,
            true,
            false,
            WorkspacePillStyle::Outlined,
            true,
        );

        assert_eq!(
            style.background,
            Some(Background::Color(theme.cosmic().accent_button.on.into()))
        );
    }

    #[test]
    fn gives_the_urgent_divider_a_destructive_color() {
        let theme = Theme::default();

        for outlined_mode in [WorkspacePillStyle::Filled, WorkspacePillStyle::Outlined] {
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
    ShowWorkspaceNumbers(bool),
    MaxVisibleIcons(u8),
    PillStyle(usize),
    PillBorderWidth(u8),
    PillSpacing(u8),
    InactivePillBrightness(i8),
    ConfigUpdated(WorkspacesAppletConfig),
    Surface(surface::Action<Message>),
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
        config.inactive_pill_brightness_percent =
            inactive_pill_brightness_percent(config.inactive_pill_brightness_percent);
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
                    retain_transient_toplevel_placements(&self.toplevels, &mut snapshot.toplevels);
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
                                // No fixed size: a repeated request for the open popup would
                                // shrink it to that size under its corner radius (#52).
                                None,
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
            Message::ShowWorkspaceNumbers(enabled) => {
                self.config.show_workspace_numbers = enabled;
                self.write_config();
            }
            Message::MaxVisibleIcons(limit) => {
                self.config.max_visible_icons = visible_icon_limit(limit);
                self.write_config();
            }
            Message::PillStyle(index) => {
                if let Some(style) = PillStyleSelection::ALL.get(index) {
                    (self.config.pill_style, self.config.inactive_pill_style) = style.styles();
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
            Message::InactivePillBrightness(percent) => {
                self.config.inactive_pill_brightness_percent =
                    inactive_pill_brightness_percent(percent);
                self.write_config();
            }
            Message::ConfigUpdated(mut config) => {
                config.pill_border_width = pill_border_width(config.pill_border_width);
                config.pill_spacing_percent = pill_spacing_percent(config.pill_spacing_percent);
                config.max_visible_icons = visible_icon_limit(config.max_visible_icons);
                config.inactive_pill_brightness_percent =
                    inactive_pill_brightness_percent(config.inactive_pill_brightness_percent);
                self.config = config;
            }
            Message::Surface(a) => {
                return surface::surface_task(a);
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
                let tooltip = if w
                    .cosmic_state
                    .contains(zcosmic_workspace_handle_v2::State::Pinned)
                {
                    crate::fl!("pinned-empty-workspace", name = w.name.as_str())
                } else {
                    crate::fl!("empty-workspace", name = w.name.as_str())
                };
                self.core
                    .applet
                    .applet_tooltip(btn, tooltip, false, Message::Surface, None)
                    .into()
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

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        let spacing = theme::active().cosmic().spacing;
        let selected_pill_style = PillStyleSelection::ALL.iter().position(|style| {
            style.styles() == (self.config.pill_style, self.config.inactive_pill_style)
        });
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

        let outline_thickness: Element<'_, Message> = if self.config.pill_style
            == WorkspacePillStyle::Outlined
            || self.config.inactive_pill_style == WorkspacePillStyle::Outlined
        {
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
                toggler(self.config.show_workspace_numbers)
                    .on_toggle(Message::ShowWorkspaceNumbers)
                    .label(crate::fl!("show-workspace-numbers"))
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
                    row![
                        self.core.applet.text(crate::fl!("pill-style")).size(14),
                        space::horizontal(),
                        popup_dropdown(
                            PillStyleSelection::ALL
                                .map(PillStyleSelection::label)
                                .to_vec(),
                            selected_pill_style,
                            Message::PillStyle,
                            id,
                            Message::Surface,
                            |message| message,
                        )
                        .placeholder(crate::fl!("pill-style-custom"))
                    ]
                    .align_y(Alignment::Center),
                    outline_thickness,
                    row![
                        self.core.applet.text(crate::fl!("pill-spacing")).size(14),
                        space::horizontal(),
                        self.pill_spacing_stepper()
                    ]
                    .align_y(Alignment::Center),
                    row![
                        self.core
                            .applet
                            .text(crate::fl!("inactive-pill-brightness"))
                            .size(14),
                        space::horizontal(),
                        self.inactive_pill_brightness_stepper()
                    ]
                    .align_y(Alignment::Center),
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
