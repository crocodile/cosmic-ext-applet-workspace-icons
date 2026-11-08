use super::*;
use cctk::wayland_client::{Connection, Proxy};
use cosmic::applet::cosmic_panel_config::PanelSize;
use cosmic::iced::{Font, Pixels, Point};
use std::os::unix::net::UnixStream;

const SYMMETRIC_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<rect x="4" y="4" width="16" height="16" fill="#20db9a"/></svg>"##;

fn fixture_app(layout: Layout, panel_size: PanelSize, cross: f32) -> IcedWorkspacesApplet {
    let mut core = cosmic::app::Core::default();
    core.applet.anchor = match layout {
        Layout::Row => PanelAnchor::Top,
        Layout::Column => PanelAnchor::Left,
    };
    core.applet.size = cosmic::applet::Size::PanelSize(panel_size);
    core.applet.suggested_bounds = Some(Size::new(cross, cross));
    IcedWorkspacesApplet {
        core,
        workspaces: Vec::new(),
        toplevels: Vec::new(),
        output: None,
        locales: Vec::new(),
        desktop_entries: Vec::new(),
        app_metadata: HashMap::new(),
        workspace_tx: None,
        layout,
        scroll: DiscreteScrollState::default(),
        config: WorkspacesAppletConfig {
            max_visible_icons: 3,
            ..Default::default()
        },
        config_helper: None,
        popup: None,
    }
}

fn fixture_workspace() -> Workspace {
    let (client, _server) = UnixStream::pair().unwrap();
    let connection = Connection::from_socket(client).unwrap();
    Workspace {
        handle: ExtWorkspaceHandleV1::inert(connection.backend().downgrade()),
        cosmic_handle: None,
        name: "1".into(),
        coordinates: Vec::new(),
        state: ext_workspace_handle_v1::State::empty(),
        cosmic_state: zcosmic_workspace_handle_v2::State::empty(),
        capabilities: ext_workspace_handle_v1::WorkspaceCapabilities::empty(),
        cosmic_capabilities: zcosmic_workspace_handle_v2::WorkspaceCapabilities::empty(),
        tiling: None,
        id: None,
    }
}

fn renderer() -> cosmic::Renderer {
    cosmic::Renderer::new(Font::DEFAULT, Pixels(16.0))
}

fn major(rect: Rectangle, layout: Layout) -> f32 {
    match layout {
        Layout::Row => rect.width,
        Layout::Column => rect.height,
    }
}

fn start(rect: Rectangle, layout: Layout) -> f32 {
    match layout {
        Layout::Row => rect.x,
        Layout::Column => rect.y,
    }
}

fn leaves(layout: IcedLayout<'_>, result: &mut Vec<Rectangle>) {
    if layout.children().next().is_none() {
        result.push(layout.bounds());
    } else {
        for child in layout.children() {
            leaves(child, result);
        }
    }
}

struct Probe {
    button: Rectangle,
    paint: Rectangle,
    content: Rectangle,
    app_group: Rectangle,
    strip: Rectangle,
    frames: Vec<Rectangle>,
    items: Vec<Rectangle>,
}

fn probe(
    app: &IcedWorkspacesApplet,
    workspace: &Workspace,
    icons: &[WorkspaceIcon<'_>],
    hovered: bool,
    renderer: &cosmic::Renderer,
) -> Probe {
    let major = app.workspace_button_major_size(&workspace.name, icons);
    let cross = app.workspace_cross_axis_size();
    let (width, height) = match app.layout {
        Layout::Row => (major, cross),
        Layout::Column => (cross, major),
    };
    let visual = app.workspace_pill_visual(workspace, icons, width, height, hovered);
    let mut button: Element<'_, Message> = button(visual)
        .width(width)
        .height(height)
        .padding(0)
        .class(cosmic::theme::iced::Button::Transparent)
        .on_press(Message::WorkspaceOverview)
        .into();
    let mut tree = Tree::new(&button);
    let node = button
        .as_widget_mut()
        .layout(&mut tree, renderer, &iced_layout::Limits::NONE);
    let root = IcedLayout::new(&node);
    let stack = root.children().next().unwrap();
    let paint = stack
        .children()
        .next()
        .unwrap()
        .children()
        .next()
        .unwrap()
        .bounds();
    let content = stack.children().nth(1).unwrap().children().next().unwrap();
    let app_group = if app.config.show_workspace_numbers {
        content.children().nth(2).unwrap()
    } else {
        content
    };
    let strip = app_group.children().next().unwrap();
    let items: Vec<_> = strip.children().map(|item| item.bounds()).collect();
    let mut frames = Vec::new();
    let visible = visible_icon_counts(icons.len(), app.config.max_visible_icons).0;
    for item in strip.children().take(visible) {
        leaves(item, &mut frames);
    }
    Probe {
        button: root.bounds(),
        paint,
        content: content.bounds(),
        app_group: app_group.bounds(),
        strip: strip.bounds(),
        frames,
        items,
    }
}

fn assert_near(a: f32, b: f32, context: &str) {
    assert!((a - b).abs() < 0.003, "{context}: {a} != {b}");
}

#[test]
fn calculated_button_size_matches_actual_overflow_layout() {
    let temp = tempfile::tempdir().unwrap();
    let icon_path = temp.path().join("symmetric.svg");
    std::fs::write(&icon_path, SYMMETRIC_ICON).unwrap();
    let metadata = AppMetadata {
        name: "controlled".into(),
        icon_source: fde::IconSource::Path(icon_path),
    };
    let workspace = fixture_workspace();
    let renderer = renderer();
    for layout in [Layout::Row, Layout::Column] {
        for (preset, cross) in [
            (PanelSize::XS, 32.0),
            (PanelSize::S, 40.0),
            (PanelSize::M, 56.0),
            (PanelSize::XL, 80.0),
        ] {
            let mut app = fixture_app(layout, preset, cross);
            for numbers in [false, true] {
                app.config.show_workspace_numbers = numbers;
                for highlighted in [
                    [false, false, false],
                    [true, false, false],
                    [false, false, true],
                    [false, true, false],
                    [true, true, true],
                ] {
                    for overflow in [0, 1, 10, 100] {
                        let icons: Vec<_> = (0..3 + overflow)
                            .map(|index| WorkspaceIcon {
                                metadata: &metadata,
                                minimized: false,
                                maximized: highlighted[index.min(2)],
                            })
                            .collect();
                        for spacing in [0, 10] {
                            app.config.pill_spacing_percent = spacing;
                            for style in [WorkspacePillStyle::Filled, WorkspacePillStyle::Outlined]
                            {
                                app.config.pill_style = style;
                                app.config.inactive_pill_style = style;
                                for hovered in [false, true] {
                                    let p = probe(&app, &workspace, &icons, hovered, &renderer);
                                    assert_near(
                                        major(p.app_group, layout),
                                        major(p.strip, layout) + app_group_leading_padding(numbers),
                                        "app-group leading padding is explicit",
                                    );
                                    for pair in p.items.windows(2) {
                                        assert_near(
                                            start(pair[1], layout)
                                                - start(pair[0], layout)
                                                - major(pair[0], layout),
                                            APP_ICON_SPACING,
                                            "laid-out item gap",
                                        );
                                    }
                                    let actual = workspace_major_size(
                                        cross,
                                        app.number_section_major_size(true, &workspace.name),
                                        major(p.app_group, layout),
                                        numbers,
                                        app.workspace_content_edge_padding(&icons),
                                    );
                                    assert_near(
                                        major(p.button, layout),
                                        actual,
                                        &format!(
                                            "laid-out group mismatch: {layout:?}, C={cross}, numbers={numbers}, overflow={overflow}, highlighted={highlighted:?}"
                                        ),
                                    );
                                    assert_near(
                                        major(p.content, layout),
                                        major(p.button, layout)
                                            - 2.0 * app.workspace_content_edge_padding(&icons),
                                        "content fills the planned length",
                                    );
                                    for frame in &p.frames {
                                        assert_near(
                                            frame.width,
                                            app.app_icon_size(),
                                            "image frame width is retained",
                                        );
                                        assert_near(
                                            frame.height,
                                            app.app_icon_size(),
                                            "image frame height is retained",
                                        );
                                        let center = frame.center();
                                        let button_center = p.button.center();
                                        assert_near(
                                            if matches!(layout, Layout::Row) {
                                                center.y
                                            } else {
                                                center.x
                                            },
                                            if matches!(layout, Layout::Row) {
                                                button_center.y
                                            } else {
                                                button_center.x
                                            },
                                            "cross-axis frame centre",
                                        );
                                    }
                                    for slot in p.items.iter().take(p.frames.len()) {
                                        assert!(
                                            slot.x >= p.paint.x - 0.003
                                                && slot.y >= p.paint.y - 0.003
                                        );
                                        assert!(
                                            slot.x + slot.width
                                                <= p.paint.x + p.paint.width + 0.003
                                        );
                                        assert!(
                                            slot.y + slot.height
                                                <= p.paint.y + p.paint.height + 0.003
                                        );
                                    }
                                    let painted_cross = if matches!(layout, Layout::Row) {
                                        p.paint.height
                                    } else {
                                        p.paint.width
                                    };
                                    assert_near(
                                        painted_cross,
                                        cross * (1.0 - 2.0 * f32::from(spacing) / 100.0),
                                        "paint inset leaves click bounds unchanged",
                                    );
                                    if !numbers && overflow == 0 {
                                        let first = p.frames.first().unwrap();
                                        let last = p.frames.last().unwrap();
                                        assert_near(
                                            start(*first, layout),
                                            major(p.button, layout)
                                                - start(*last, layout)
                                                - major(*last, layout),
                                            "icon frame end margins",
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn highlights_beyond_the_visible_limit_do_not_change_visible_slots() {
    let metadata = AppMetadata {
        name: "controlled".into(),
        icon_source: fde::IconSource::default(),
    };
    let workspace = fixture_workspace();
    let renderer = renderer();
    for layout in [Layout::Row, Layout::Column] {
        for limit in [1, 2, 16] {
            let mut app = fixture_app(layout, PanelSize::S, 40.0);
            app.config.show_workspace_numbers = false;
            app.config.max_visible_icons = limit;
            let mut icons: Vec<_> = (0..limit)
                .map(|_| WorkspaceIcon {
                    metadata: &metadata,
                    minimized: false,
                    maximized: false,
                })
                .collect();
            let before = probe(&app, &workspace, &icons, false, &renderer);
            icons.push(WorkspaceIcon {
                metadata: &metadata,
                minimized: false,
                maximized: true,
            });
            let after = probe(&app, &workspace, &icons, false, &renderer);
            assert_eq!(before.frames.len(), after.frames.len());
            for (before, after) in before.frames.iter().zip(&after.frames) {
                assert_near(
                    before.x,
                    after.x,
                    "hidden highlight leaves frame x unchanged",
                );
                assert_near(
                    before.y,
                    after.y,
                    "hidden highlight leaves frame y unchanged",
                );
                assert_near(
                    before.width,
                    after.width,
                    "hidden highlight leaves frame width unchanged",
                );
                assert_near(
                    before.height,
                    after.height,
                    "hidden highlight leaves frame height unchanged",
                );
            }
            assert_near(
                app.workspace_content_edge_padding(&icons),
                9.6,
                "overflow-only highlights do not affect padding",
            );
            app.config.highlight_maximized_window_icons = false;
            icons[0].maximized = true;
            let disabled = probe(&app, &workspace, &icons, false, &renderer);
            for (before, after) in after.frames.iter().zip(&disabled.frames) {
                assert_near(
                    before.x,
                    after.x,
                    "disabled highlight leaves frame x unchanged",
                );
                assert_near(
                    before.y,
                    after.y,
                    "disabled highlight leaves frame y unchanged",
                );
            }
        }
    }
}

#[test]
fn overflow_grows_by_its_natural_text_extent_without_changing_end_padding() {
    let metadata = AppMetadata {
        name: "controlled".into(),
        icon_source: fde::IconSource::default(),
    };
    let workspace = fixture_workspace();
    let renderer = renderer();
    for layout in [Layout::Row, Layout::Column] {
        for (preset, cross, line_height) in [
            (PanelSize::XS, 32.0, 21.0),
            (PanelSize::S, 40.0, 21.0),
            (PanelSize::M, 56.0, 30.0),
            (PanelSize::L, 64.0, 36.0),
            (PanelSize::XL, 80.0, 43.0),
        ] {
            let mut app = fixture_app(layout, preset, cross);
            for numbers in [false, true] {
                app.config.show_workspace_numbers = numbers;
                for highlighted in [
                    [false, false, false],
                    [true, false, false],
                    [false, false, true],
                    [false, true, false],
                ] {
                    let icons: Vec<_> = highlighted
                        .iter()
                        .map(|maximized| WorkspaceIcon {
                            metadata: &metadata,
                            minimized: false,
                            maximized: *maximized,
                        })
                        .collect();
                    let before = probe(&app, &workspace, &icons, false, &renderer);
                    let padding = app.workspace_content_edge_padding(&icons);
                    for count in [1, 10, 100, 1000] {
                        let mut with_overflow = icons.clone();
                        with_overflow.extend((0..count).map(|_| icons[0]));
                        let after = probe(&app, &workspace, &with_overflow, false, &renderer);
                        let mut text: Element<'_, Message> =
                            workspace_overflow_text(&app.core.applet, app.app_icon_size(), count)
                                .into();
                        let mut tree = Tree::new(&text);
                        let natural = text
                            .as_widget_mut()
                            .layout(&mut tree, &renderer, &iced_layout::Limits::NONE)
                            .size();
                        assert_near(
                            natural.height,
                            line_height,
                            "COSMIC absolute line height survives font-size override",
                        );
                        let text_major = match layout {
                            Layout::Row => natural.width,
                            Layout::Column => natural.height,
                        };
                        assert_near(
                            major(after.button, layout) - major(before.button, layout),
                            text_major + APP_ICON_SPACING,
                            "button grows exactly by text plus intended gap",
                        );
                        assert_near(
                            major(*after.items.last().unwrap(), layout),
                            text_major,
                            "overflow widget agrees with natural text size",
                        );
                        assert_near(
                            app.workspace_content_edge_padding(&with_overflow),
                            padding,
                            "explicit padding remains stable",
                        );
                        assert_near(
                            start(after.content, layout),
                            start(before.content, layout),
                            "no new centring surplus at leading end",
                        );
                        assert_near(
                            major(after.button, layout)
                                - start(after.content, layout)
                                - major(after.content, layout),
                            major(before.button, layout)
                                - start(before.content, layout)
                                - major(before.content, layout),
                            "no new centring surplus at trailing end",
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "writes visual evidence; set WORKSPACE_LAYOUT_OUTPUT to an output directory"]
fn render_layout_samples_and_asset_bounds() {
    use cosmic::iced::core::renderer::{Headless as _, Renderer as _};
    use resvg::{tiny_skia, usvg};
    use serde_json::json;

    let output = std::path::PathBuf::from(
        std::env::var("WORKSPACE_LAYOUT_OUTPUT").expect("output directory"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let controlled = output.join("controlled-symmetric.svg");
    std::fs::write(&controlled, SYMMETRIC_ICON).unwrap();
    let asymmetric = output.join("controlled-asymmetric.svg");
    std::fs::write(
        &asymmetric,
        SYMMETRIC_ICON
            .replace("x=\"4\" y=\"4\"", "x=\"2\" y=\"6\"")
            .replace("width=\"16\" height=\"16\"", "width=\"12\" height=\"12\""),
    )
    .unwrap();
    let sources = [
        ("symmetric", controlled),
        ("asymmetric", asymmetric),
        (
            "cosmic-terminal",
            "/usr/share/icons/Cosmic/scalable/apps/utilities-terminal.svg".into(),
        ),
        (
            "brave",
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".local/share/icons/hicolor/scalable/apps/brave-browser-dark-red.svg"),
        ),
    ];
    let workspace = fixture_workspace();
    let mut renderer = renderer();
    let mut assets = Vec::new();
    let mut samples = Vec::new();
    let mut text_metrics = Vec::new();
    for (preset, cross) in [
        (PanelSize::XS, 32.0),
        (PanelSize::S, 40.0),
        (PanelSize::M, 56.0),
        (PanelSize::L, 64.0),
        (PanelSize::XL, 80.0),
    ] {
        let app = fixture_app(Layout::Row, preset, cross);
        for count in [1, 10, 100, 1000] {
            let size = workspace_overflow_size(&app.core.applet, app.app_icon_size(), count);
            text_metrics.push(json!({"cross":cross,"count":count,"font_size":(app.app_icon_size()*0.55).max(10.0),"width":size.width,"line_height":size.height}));
        }
    }
    for (name, source) in sources {
        if !source.exists() {
            assets.push(json!({"asset":name,"source":source,"available":false}));
            continue;
        }
        let data = std::fs::read(&source).unwrap();
        let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).unwrap();
        let mut pixmap = tiny_skia::Pixmap::new(240, 240).unwrap();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(
                240.0 / tree.size().width(),
                240.0 / tree.size().height(),
            ),
            &mut pixmap.as_mut(),
        );
        let mut ink = (240, 240, 0, 0);
        for y in 0..240 {
            for x in 0..240 {
                if pixmap.pixel(x, y).unwrap().alpha() > 16 {
                    ink = (
                        ink.0.min(x),
                        ink.1.min(y),
                        ink.2.max(x + 1),
                        ink.3.max(y + 1),
                    );
                }
            }
        }
        assets.push(json!({"asset":name,"source":source,"available":true,"alpha_envelope_at_240px":[ink.0,ink.1,ink.2,ink.3]}));
        let metadata = AppMetadata {
            name: name.into(),
            icon_source: fde::IconSource::Path(source.clone()),
        };
        for layout in [Layout::Row, Layout::Column] {
            for scale in [1.0, 1.25] {
                for variant in 0..3 {
                    let mut app = fixture_app(layout, PanelSize::S, 40.0);
                    app.config.show_workspace_numbers = variant == 2;
                    app.config.pill_spacing_percent = if variant == 0 { 0 } else { 10 };
                    app.config.pill_style = if variant == 0 {
                        WorkspacePillStyle::Filled
                    } else {
                        WorkspacePillStyle::Outlined
                    };
                    app.config.inactive_pill_style = app.config.pill_style;
                    let count = if variant == 0 { 3 } else { 13 };
                    let icons: Vec<_> = (0..count)
                        .map(|i| WorkspaceIcon {
                            metadata: &metadata,
                            minimized: false,
                            maximized: i == if variant == 0 { 0 } else { 2 },
                        })
                        .collect();
                    let hovered = variant == 2;
                    let p = probe(&app, &workspace, &icons, hovered, &renderer);
                    let mut visual = app.workspace_pill_visual(
                        &workspace,
                        &icons,
                        p.button.width,
                        p.button.height,
                        hovered,
                    );
                    let mut state = Tree::new(&visual);
                    let node = visual
                        .as_widget_mut()
                        .layout(&mut state, &renderer, &iced_layout::Limits::NONE)
                        .move_to(Point::new(4.0, 4.0));
                    let logical = Size::new(p.button.width + 8.0, p.button.height + 8.0);
                    let physical = Size::new(
                        (logical.width * scale).ceil() as u32,
                        (logical.height * scale).ceil() as u32,
                    );
                    let viewport = Rectangle::with_size(logical);
                    renderer.reset(viewport);
                    visual.as_widget().draw(
                        &state,
                        &mut renderer,
                        &Theme::dark(),
                        &renderer::Style {
                            scale_factor: f64::from(scale),
                            ..Default::default()
                        },
                        IcedLayout::new(&node),
                        mouse::Cursor::Unavailable,
                        &viewport,
                    );
                    let bytes = renderer.screenshot(physical, scale, Color::from_rgb8(35, 40, 49));
                    let stem = format!(
                        "{name}-{}-{}-v{variant}",
                        if matches!(layout, Layout::Row) {
                            "horizontal"
                        } else {
                            "vertical"
                        },
                        (scale * 100.0) as u32
                    );
                    let size =
                        tiny_skia::IntSize::from_wh(physical.width, physical.height).unwrap();
                    tiny_skia::Pixmap::from_vec(bytes.clone(), size)
                        .unwrap()
                        .save_png(output.join(format!("{stem}.png")))
                        .unwrap();
                    let mut ink_offsets = Vec::new();
                    for frame in &p.frames {
                        if name == "symmetric" {
                            let mut envelope = (physical.width, physical.height, 0, 0);
                            for y in ((frame.y + 4.0) * scale).floor() as u32
                                ..((frame.y + frame.height + 4.0) * scale).ceil() as u32
                            {
                                for x in ((frame.x + 4.0) * scale).floor() as u32
                                    ..((frame.x + frame.width + 4.0) * scale).ceil() as u32
                                {
                                    let offset = ((y * physical.width + x) * 4) as usize;
                                    let (r, g, b) =
                                        (bytes[offset], bytes[offset + 1], bytes[offset + 2]);
                                    if r < 80 && g > 160 && b > 100 && b < 200 {
                                        envelope = (
                                            envelope.0.min(x),
                                            envelope.1.min(y),
                                            envelope.2.max(x + 1),
                                            envelope.3.max(y + 1),
                                        );
                                    }
                                }
                            }
                            assert!(envelope.2 > envelope.0 && envelope.3 > envelope.1);
                            let dx = (envelope.0 + envelope.2) as f32 / 2.0
                                - (frame.center().x + 4.0) * scale;
                            let dy = (envelope.1 + envelope.3) as f32 / 2.0
                                - (frame.center().y + 4.0) * scale;
                            assert!(
                                dx.abs() <= 1.5 && dy.abs() <= 1.5,
                                "artwork offset exceeds raster rounding: {dx}, {dy}"
                            );
                            ink_offsets.push([dx, dy]);
                        }
                    }
                    renderer.with_layer(viewport, |renderer| {
                        for frame in &p.frames {
                            let bounds = Rectangle {
                                x: frame.x + 4.0,
                                y: frame.y + 4.0,
                                ..*frame
                            };
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds,
                                    border: Border {
                                        width: 0.6,
                                        color: Color::from_rgb8(54, 187, 255),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                                Color::TRANSPARENT,
                            );
                        }
                    });
                    tiny_skia::Pixmap::from_vec(
                        renderer.screenshot(physical, scale, Color::from_rgb8(35, 40, 49)),
                        size,
                    )
                    .unwrap()
                    .save_png(output.join(format!("{stem}-frames.png")))
                    .unwrap();
                    let rect = |r: Rectangle| json!([r.x, r.y, r.width, r.height]);
                    samples.push(json!({"image":format!("{stem}.png"),"asset":name,"orientation":format!("{layout:?}"),"scale":scale,"cross":40,"numbers":app.config.show_workspace_numbers,"spacing_percent":app.config.pill_spacing_percent,"style":format!("{:?}",app.config.inactive_pill_style),"hovered":hovered,"overflow":count-3,"button":rect(p.button),"paint":rect(p.paint),"slots":p.items.iter().copied().map(rect).collect::<Vec<_>>(),"frames":p.frames.iter().copied().map(rect).collect::<Vec<_>>(),"controlled_ink_offset_physical_pixels":ink_offsets}));
                }
            }
        }
    }
    let report = json!({"backend":"tiny-skia","font":format!("{:?}",cosmic::font::default()),"settings":"Dark theme; S panel; highlight enabled; 3 visible icons; first/last highlighted","assets":assets,"overflow_metrics":text_metrics,"samples":samples});
    std::fs::write(
        output.join("measurements.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "Visual samples and measurements written to {}",
        output.display()
    );
}
