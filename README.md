# Workspace Icons

Workspace Icons is a third-party COSMIC panel applet based on the COSMIC
Numbered Workspaces applet. It shows application icons beside each workspace
number so you can see where windows are at a glance.

## Features

- Displays application icons beside each workspace number.
- Associates windows with the correct workspace and monitor.
- Shows an overflow count when a workspace has many applications.
- Dims application icons when all windows for that app are minimized.
- Highlights icons for apps with maximized windows.
- Preserves workspace switching, scrolling, and workspace overview behavior.

## Install From Source

Requires COSMIC Desktop development dependencies and a Rust toolchain.

```bash
git clone https://github.com/crocodile/cosmic-ext-applet-workspace-icons.git
cd cosmic-ext-applet-workspace-icons
just install
```

Then open COSMIC panel settings and add **Workspace Icons**.

## Development

```bash
just build
just install
just uninstall
cargo fmt --all -- --check
cargo check
cargo test
```

## License

GPL-3.0-only.
