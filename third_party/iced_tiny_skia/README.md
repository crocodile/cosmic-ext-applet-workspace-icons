# Patched Iced software renderer

This is the `tiny_skia` crate from the COSMIC Iced fork:

- Iced: `229186b5180b6217d93866c912e6373ac378b231`
  ([original sources](https://github.com/pop-os/iced/tree/229186b5180b6217d93866c912e6373ac378b231/tiny_skia)).
- Bundled with libcosmic: `430f3d48a97d8779f9c1985473912e594ccca1ae`.
- License: MIT; the upstream license is preserved in `LICENSE`.

The root `Cargo.toml` patches only `iced_tiny_skia` to use this directory.
The `tiny-skia` library and the rest of Iced still come from their original
dependencies. Ordinary `cargo build`, `just build`, and `just install` use this
copy automatically.

## Local changes

Upstream issue: <https://github.com/pop-os/libcosmic/issues/1482>.

`src/vector.rs` preserves fractional SVG positions in the floating-point
transform. Its raster buffer dimensions are rounded upward, then the buffer is
scaled to the exact requested dimensions. This avoids truncating both position
and size before drawing.

`src/raster.rs` likewise translates bitmap images through the floating-point
transform before scaling, instead of converting their destination position into
integer source coordinates. This fixes the same shift for decoded PNGs and
other bitmap images. An unused duplicate filter-quality calculation from
upstream was also removed to avoid a build warning.

`Cargo.toml` expands the package metadata, dependency versions, and lints that
were inherited from the upstream workspace. `iced_debug` and `iced_graphics`
use the same libcosmic Git source as the app so renderer types stay compatible;
the root `Cargo.lock` pins the exact revision. The remaining renderer sources
and bundled font are unchanged.

## Verification and maintenance

Run from the repository root:

```sh
cargo test --locked --offline --test fractional_image_bounds
cargo test --locked --offline
cargo build --release --locked --offline
```

The regression tests render centred SVG and decoded bitmap fixtures through
the app's renderer and compare their artwork centres with a native quad. They
cover fractional sizes and origins, display scales from 100% to 200%, rotation,
and translucent images over a highlight. Both tests fail with the original
upstream drawing code.

When updating libcosmic, compare this crate with the newly bundled Iced renderer
and refresh its manifest as needed. Once the upstream fix is available, remove
the root Cargo patch, update the lockfile, and run the regression tests against
upstream before deleting this directory. Keep the tests to guard against the
bug returning.
