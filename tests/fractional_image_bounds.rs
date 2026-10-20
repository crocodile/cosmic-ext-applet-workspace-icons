//! Regression coverage for https://github.com/pop-os/libcosmic/issues/1482.
//! Exercise the renderer selected by the app's Cargo dependency graph.

use cosmic::iced::core::{
    Color, Font, Image, Pixels, Point, Radians, Rectangle, Size, image, renderer, svg,
};
use renderer::{Headless as _, Renderer as _};

const VIEWPORT: f32 = 96.0;

enum TestImage {
    Svg(svg::Svg),
    Bitmap(Image),
}

impl TestImage {
    fn draw(
        &self,
        renderer: &mut cosmic::Renderer,
        bounds: Rectangle,
        rotation: f32,
        opacity: f32,
    ) {
        let clip = Rectangle::with_size(Size::new(VIEWPORT, VIEWPORT));
        match self {
            Self::Svg(svg) => svg::Renderer::draw_svg(
                renderer,
                svg.clone().rotation(Radians(rotation)).opacity(opacity),
                bounds,
                clip,
            ),
            Self::Bitmap(image) => image::Renderer::draw_image(
                renderer,
                image.clone().rotation(Radians(rotation)).opacity(opacity),
                bounds,
                clip,
            ),
        }
    }
}

fn green_centroid(pixels: &[u8], width: usize) -> (f64, f64) {
    let mut weight = 0.0;
    let mut x_sum = 0.0;
    let mut y_sum = 0.0;
    for (index, rgba) in pixels.chunks_exact(4).enumerate() {
        // Ignore the neutral highlight, measuring only the green artwork.
        let green = f64::from(rgba[1].saturating_sub(rgba[0].max(rgba[2])));
        weight += green;
        x_sum += (index % width) as f64 * green;
        y_sum += (index / width) as f64 * green;
    }
    assert!(weight > 0.0, "image must actually render");
    (x_sum / weight + 0.5, y_sum / weight + 0.5)
}

fn assert_centered(image: TestImage) {
    let mut renderer = futures::executor::block_on(<cosmic::Renderer as renderer::Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    let viewport = Rectangle::with_size(Size::new(VIEWPORT, VIEWPORT));

    for panel_size in [32.0_f32, 40.0, 56.0, 64.0, 80.0] {
        let icon_size = (panel_size * 0.52).max(16.0);
        for scale in [1.0_f32, 1.2, 1.25, 1.5, 1.75, 2.0] {
            let physical_size = (VIEWPORT * scale) as u32;
            let size = Size::new(physical_size, physical_size);
            for origin in [Point::ORIGIN, Point::new(4.3, 4.7)] {
                for highlighted in [false, true] {
                    let slot_size = if highlighted {
                        icon_size * 1.28
                    } else {
                        panel_size
                    };
                    let bounds = Rectangle {
                        x: origin.x + (slot_size - icon_size) / 2.0,
                        y: origin.y + (slot_size - icon_size) / 2.0,
                        width: icon_size,
                        height: icon_size,
                    };
                    let opacity = if highlighted { 0.45 } else { 1.0 };
                    for rotation in [0.0, std::f32::consts::FRAC_PI_2] {
                        // A native quad provides an independent rasterized reference.
                        // Both fixtures contain the same centred 16/24 green square.
                        renderer.reset(viewport);
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: bounds.x + icon_size / 6.0,
                                    y: bounds.y + icon_size / 6.0,
                                    width: icon_size * 2.0 / 3.0,
                                    height: icon_size * 2.0 / 3.0,
                                },
                                ..Default::default()
                            },
                            Color::from_rgba(0.0, 1.0, 0.0, opacity),
                        );
                        let reference = renderer.screenshot(size, scale, Color::BLACK);
                        let expected = green_centroid(&reference, physical_size as usize);

                        renderer.reset(viewport);
                        if highlighted {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle::new(origin, Size::new(slot_size, slot_size)),
                                    ..Default::default()
                                },
                                Color::from_rgba(1.0, 1.0, 1.0, 0.24),
                            );
                        }
                        image.draw(&mut renderer, bounds, rotation, opacity);
                        let pixels = renderer.screenshot(size, scale, Color::BLACK);
                        let actual = green_centroid(&pixels, physical_size as usize);
                        let dx = actual.0 - expected.0;
                        let dy = actual.1 - expected.1;
                        // Allow half a physical pixel plus 0.01px for 8-bit
                        // blending/centroid quantization; reject a 1px shift.
                        assert!(
                            dx.abs() <= 0.51 && dy.abs() <= 0.51,
                            "panel={panel_size}, scale={scale}, origin={origin:?}, \
                             highlighted={highlighted}, rotation={rotation}: \
                             expected={expected:?}, actual={actual:?}, delta=({dx}, {dy})",
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn fractional_svg_bounds_keep_the_artwork_centered() {
    let handle = svg::Handle::from_memory(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
            <rect x="4" y="4" width="16" height="16" fill="lime"/>
        </svg>"#
            .as_slice(),
    );
    assert_centered(TestImage::Svg(svg::Svg::new(handle)));
}

#[test]
fn fractional_bitmap_bounds_keep_the_artwork_centered() {
    let mut pixels = vec![0; 24 * 24 * 4];
    for y in 4..20 {
        for x in 4..20 {
            let offset = (y * 24 + x) * 4;
            pixels[offset..offset + 4].copy_from_slice(&[0, 255, 0, 255]);
        }
    }
    let handle = image::Handle::from_rgba(24, 24, pixels);
    assert_centered(TestImage::Bitmap(Image::new(handle)));
}
