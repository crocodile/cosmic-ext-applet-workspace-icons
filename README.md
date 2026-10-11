# Workspace Icons

Workspace Icons is a third-party [COSMIC](https://en.wikipedia.org/wiki/COSMIC_desktop) panel applet based on the COSMIC
Numbered Workspaces applet. It shows application icons beside each workspace
number so you can see where windows are at a glance.

## Screenshots

<img src="resources/screenshots/workspace-icons-store-8.png" alt="Workspace Icons applet showing application icons beside workspace numbers" />

<br />

<img src="resources/screenshots/workspace-icons-store-5.png" alt="Workspace Icons settings popover with filled workspace pills" />

<br />

<img src="resources/screenshots/workspace-icons-store-2.png" alt="Outlined workspace pills with application names shown on hover" />

## Features

- Displays application icons beside each workspace number.
- Can show separate icons for each open window by disabling application grouping.
- Associates windows with the correct workspace and monitor.
- Configures up to sixteen visible icons per workspace before showing an overflow count.
- Dims minimised window icons.
- Highlights maximised window icons.
- Preserves workspace switching, scrolling, and workspace overview behavior.

## Pill Colors

Pill colors use semantic COSMIC theme tokens, so their rendered values follow
the user's current theme. In the table below, `mix(A, B, X)` is an opaque,
perceptual color interpolation from `A` at 0% toward `B` at 100%, `P` is the
configured inactive pill contrast, `H` is `P + 15` percentage points capped at
100%, and `W` is the configured outline thickness.

| State | Mode | Interaction | Fill | Border | Workspace number | Other foreground | Separator |
|---|---|---|---|---|---|---|---|
| Inactive | Filled | Resting | RGB mix of `current_container().base` and `current_container().component.border` at `P`, with `button.base` alpha | None | `button.on` | `button.on` | `current_container().divider` |
| Inactive | Filled | Hovered | RGB mix of `current_container().component.hover` and `current_container().component.border` at `H`, with `button.hover` alpha | None | `button.on` | `button.on` | `current_container().divider` |
| Inactive | Outlined | Resting | None | Resting inactive RGB mix with `button.base` alpha, width `W` | `button.on` | `button.on` | `current_container().divider` |
| Inactive | Outlined | Hovered | Hovered inactive RGB mix with `button.hover` alpha | None | `button.on` | `button.on` | `current_container().divider` |
| Active | Filled | Resting | `accent_button.base` | None | `accent_button.on` | `accent_button.on` | `current_container().divider` |
| Active | Filled | Hovered | `accent_button.hover` | None | `accent_button.on` | `accent_button.on` | `current_container().divider` |
| Active | Outlined | Resting | None | `accent_button.base`, width `W` | `accent_text`, falling back to `accent_button.base` | `current_container().component.on` | `accent_text`, falling back to `accent_button.base` |
| Active | Outlined | Hovered | `accent_button.hover` | None | `accent_button.on` | `accent_button.on` | `accent_button.on` |
| Urgent | Filled | Resting | `button.base` | `destructive_button.base`, 1 px | `destructive_button.base` | `destructive_button.base` | `destructive_button.base` |
| Urgent | Filled | Hovered | `button.hover` | `destructive_button.base`, 1 px | `destructive_button.base` | `destructive_button.base` | `destructive_button.base` |
| Urgent | Outlined | Resting | None | `destructive_button.base`, width `W` | `destructive_button.base` | `destructive_button.base` | `destructive_button.base` |
| Urgent | Outlined | Hovered | `button.hover` | `destructive_button.base`, width `W` | `destructive_button.base` | `destructive_button.base` | `destructive_button.base` |

Hovered outlined active and inactive pills remove their border after filling to
avoid a visible seam. Urgent pills keep their destructive border on hover. If a
workspace is both active and urgent, active styling takes precedence.

The inactive pill contrast control changes the RGB color while COSMIC's
standard `button.base` and `button.hover` tokens provide the alpha. This keeps
the full configurable color range while allowing transparent and frosted-glass
panels to remain visible through both filled pills and outlined pill borders.
At 0%, a resting pill uses the current container's base RGB; at 100%, it reaches
the component border RGB.

## Install From Source

Requires COSMIC Desktop development dependencies and a Rust toolchain.

```bash
git clone https://github.com/crocodile/cosmic-ext-applet-workspace-icons.git
cd cosmic-ext-applet-workspace-icons
just install
```

Then open COSMIC panel settings and add **Workspace Icons**.

## Testing The Flatpak Build

Workspace Icons is intended to be distributed as a Flatpak through the COSMIC
Store. Until then, the Flatpak packaging draft lives in
`packaging/io.github.crocodile.cosmic-ext-applet-workspace-icons/`.

Before testing the Flatpak, remove any source install first so COSMIC does not
show stale or duplicate applet entries:

```bash
just uninstall
```

Then install the local Flatpak build:

```bash
flatpak-builder --user --install --force-clean \
  build-dir/workspace-icons-flatpak \
  packaging/io.github.crocodile.cosmic-ext-applet-workspace-icons/io.github.crocodile.cosmic-ext-applet-workspace-icons.json
```

If the applet list does not refresh immediately, log out and back in. After
adding **Workspace Icons** to the panel, confirm that the Flatpak version is
running with:

```bash
flatpak ps
```

To remove the local Flatpak test install:

```bash
flatpak uninstall --user io.github.crocodile.cosmic-ext-applet-workspace-icons
```

## Development

```bash
just build
just install
just uninstall
cargo fmt --all -- --check
cargo check
cargo test
```

For a technical map of how this differs from COSMIC's Numbered Workspaces applet,
see [Changes From COSMIC Numbered Workspaces](docs/numbered-workspaces-changes.md).

## License

GPL-3.0-only.
