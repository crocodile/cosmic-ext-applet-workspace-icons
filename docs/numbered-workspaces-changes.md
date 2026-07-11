# Changes From COSMIC Numbered Workspaces

This document explains how this applet differs from the original COSMIC
Numbered Workspaces applet.

Baseline used for this review:

- Upstream applet: `pop-os/cosmic-applets/cosmic-applet-workspaces`
- This repo: `cosmic-ext-applet-workspace-icons` at `669d76e`
- Important note: this repository's first commit already contains most of the
  Workspace Icons rewrite. Later commits mostly add polish, screenshots,
  README updates, and Flatpak packaging.

## Short Version

The original Numbered Workspaces applet showed one button per workspace. Each
button contained only the workspace number/name. It subscribed to workspace
Wayland events, let you click to activate another workspace, opened the
workspace overview when clicking the active workspace, and supported scrolling
between workspaces.

Workspace Icons keeps that base behavior, then adds a second layer of context:
it watches open windows, groups them by workspace and monitor, resolves each
window's application icon, and renders those icons beside the workspace number.

The main new behavior is:

- Show application icons beside each workspace number.
- Group multiple windows from the same app into one icon with a count in the
  tooltip.
- Dim an app icon when all windows for that app are minimized.
- Highlight an app icon when that app has a maximized window.
- Show overflow text when a workspace has more app groups than fit in the
  compact strip.
- Add a small settings popup for the minimized/maximized icon behavior.
- Package the applet as a standalone third-party applet with its own app ID,
  metadata, install commands, and Flatpak draft.

## Commit Timeline

The local history is short:

- `8a7882c` - initial standalone Workspace Icons import. This already contains
  the main changes from Numbered Workspaces: icon lookup, toplevel tracking,
  grouping, settings, new app ID, resources, and install commands.
- `1d152bd` - metadata polish, screenshots, and icon scaling polish.
- `5bd9f5e` - first Flatpak packaging draft.
- `65dadc7` - Flatpak testing workflow in the README.
- `b1070c4` - README link for COSMIC.
- `669d76e` - Flatpak wrapper and permissions for resolving host app icons.

## File-by-file Map

This is the fast orientation map:

- `src/components/app.rs` is where almost all user-visible behavior lives:
  state, icon lookup, grouping, tooltips, settings popup, button sizing, and
  rendering.
- `src/wayland.rs` collects raw workspace and window data from Wayland.
- `src/wayland_subscription.rs` bridges the Wayland thread into the COSMIC
  application subscription system.
- `src/config.rs` defines the standalone app ID and persistent settings.
- `src/main.rs` is only startup glue.
- `src/lib.rs` wires localization and app startup together.
- `src/localize.rs` loads Fluent translations.
- `i18n/en/cosmic_ext_applet_workspace_icons.ftl` contains the new English app
  name and settings labels.
- `Cargo.toml` makes the applet a standalone Rust package instead of a member
  of the upstream COSMIC applets workspace.
- `resources/*` provides desktop/appstream/icon assets.
- `justfile` provides local build/install/uninstall commands.
- `packaging/*` contains the Flatpak draft and its icon-resolution wrapper.
- `src/colors.rs` is present but not compiled today.

## Project Identity

Files:

- `Cargo.toml`
- `src/config.rs`
- `src/main.rs`
- `resources/io.github.crocodile.cosmic-ext-applet-workspace-icons.desktop`
- `resources/io.github.crocodile.cosmic-ext-applet-workspace-icons.metainfo.xml`
- `resources/io.github.crocodile.cosmic-ext-applet-workspace-icons.svg`
- `justfile`

The applet is no longer identified as the built-in COSMIC workspaces applet.
It now has its own package name, executable name, repository metadata, app ID,
desktop entry, appstream metadata, and icon.

The old app ID was:

```text
com.system76.CosmicWorkspacesApplet
```

The new app ID is:

```text
io.github.crocodile.cosmic-ext-applet-workspace-icons
```

That matters because COSMIC config, desktop discovery, applet discovery, and
Flatpak packaging all use the app ID as identity. Keeping this separate avoids
colliding with System76's built-in applet.

`src/main.rs` is still intentionally tiny. It initializes tracing, logs the
version, and calls the library's `run()` function.

## Dependency Shape

File:

- `Cargo.toml`

The upstream applet lives inside the larger `pop-os/cosmic-applets` workspace,
so many dependencies are inherited from that workspace.

This repo is standalone, so `Cargo.toml` pins the dependencies directly. That
is why you see explicit dependencies for:

- `libcosmic`
- `cosmic-config`
- `cctk`
- `serde`
- `tokio`
- `futures`
- `i18n-embed`
- `rust-embed`
- tracing crates

The important functional additions are:

- `cosmic-config` and `serde` for persistent user settings.
- `cctk`/COSMIC protocol access for workspace and toplevel state.
- `libcosmic` desktop/icon APIs for resolving app IDs into display names and
  icons.

The patch sections keep the Smithay/COSMIC protocol dependency versions aligned
with the current COSMIC stack.

## Configuration

Files:

- `src/config.rs`
- `src/components/app.rs`
- `i18n/en/cosmic_ext_applet_workspace_icons.ftl`

The original applet had no user settings. Workspace Icons adds
`WorkspacesAppletConfig` with two booleans:

- `dim_minimized_window_icons`
- `highlight_maximized_window_icons`

Both default to `true`.

`src/components/app.rs` loads the config during app initialization:

```rust
let config_helper = Config::new(config::APP_ID, WorkspacesAppletConfig::VERSION).ok();
let config = config_helper
    .as_ref()
    .and_then(|helper| WorkspacesAppletConfig::get_entry(helper).ok())
    .unwrap_or_default();
```

The app also watches for external config changes through
`watch_config::<WorkspacesAppletConfig>()`. That means if COSMIC config changes
outside this process, the applet can update without a restart.

Right-clicking the applet opens a settings popup. The popup contains two
toggles:

- Dim minimized window icons.
- Highlight maximized window icons.

When either toggle changes, `write_config()` persists it through
`cosmic-config`.

## Wayland Data Model

Files:

- `src/wayland.rs`
- `src/wayland_subscription.rs`
- `src/components/app.rs`

The original applet only needed workspace data. Its subscription could send
roughly this shape:

```rust
Vec<Workspace>
```

Workspace Icons needs to know which windows are on which workspace, so the
Wayland layer now sends a richer snapshot:

```rust
pub struct WorkspaceSnapshot {
    pub workspaces: Vec<Workspace>,
    pub toplevels: Vec<ToplevelInfo>,
    pub output: Option<WlOutput>,
}
```

The new `toplevels` field is the important part. A `ToplevelInfo` represents
an open top-level window and includes data such as:

- app ID
- title
- workspace handles
- output handles
- minimized/maximized state

`output` is also included so the applet can avoid showing windows from another
monitor's panel instance.

## Toplevel Window Tracking

File:

- `src/wayland.rs`

The Wayland state now owns a `ToplevelInfoState`:

```rust
toplevel_info_state: ToplevelInfoState,
```

It also implements `ToplevelInfoHandler`.

This is the core change that lets the applet react when windows open, close,
move, minimize, maximize, or change workspaces. The handler sends a fresh
snapshot on:

- new toplevel window
- updated toplevel window
- closed toplevel window

For closed windows, the code does one careful thing: it removes the just-closed
toplevel from the snapshot before sending it. The comment explains why: the
toolkit callback runs immediately before the toolkit removes that window from
its own internal state. Without this manual removal, the UI could briefly show
an icon for a window that has already closed.

## Monitor Filtering

Files:

- `src/wayland.rs`
- `src/components/app.rs`

The original applet already cared about the panel output so each monitor could
show the correct workspace group.

Workspace Icons extends that idea to windows too. In `apps_for_workspace()`,
each toplevel is ignored unless it belongs to the current workspace and, when
an output is known, the current panel output:

```rust
if let Some(output) = self.output.as_ref()
    && !toplevel.output.contains(output)
{
    continue;
}
```

That prevents app icons from one monitor leaking into another monitor's
workspace applet.

## App Metadata And Icon Lookup

File:

- `src/components/app.rs`

The original applet did not need desktop entries or icons. Workspace Icons adds
these fields to app state:

```rust
locales: Vec<String>,
desktop_entries: Vec<fde::DesktopEntry>,
app_metadata: HashMap<String, AppMetadata>,
```

`update_desktop_entries()` loads `.desktop` files from the normal XDG desktop
entry paths:

```rust
self.desktop_entries = fde::Iter::new(fde::default_paths())
    .filter_map(|path| fde::DesktopEntry::from_path(path, Some(&self.locales)).ok())
    .collect();
```

`resolve_app_metadata()` maps a Wayland app ID to:

- a localized display name
- a COSMIC icon source

If an app cannot be found in the desktop entries, the code falls back to a
synthetic desktop entry based on the app ID. That gives the UI something
reasonable to display even for apps without a clean desktop file match.

`sync_app_metadata()` keeps the cache trimmed to only app IDs currently present
in open toplevel windows. This avoids accumulating stale icon metadata as apps
open and close.

## Grouping Windows Into Workspace Apps

File:

- `src/components/app.rs`

Workspace Icons introduces a derived view model:

```rust
struct WorkspaceApp<'a> {
    app_id: &'a str,
    metadata: &'a AppMetadata,
    window_count: usize,
    minimized_count: usize,
    maximized_count: usize,
    minimized_titles: Vec<&'a str>,
}
```

This is not raw Wayland data. It is UI-friendly data built from toplevel
windows.

`apps_for_workspace()` loops over every toplevel and groups windows by app ID
for a specific workspace. While grouping, it counts:

- how many windows that app has on the workspace
- how many are minimized
- how many are maximized
- which minimized window titles are worth showing in a tooltip

This is why one icon can represent several windows from the same app.

## Tooltip Cleanup

File:

- `src/components/app.rs`

The helper `informative_titles()` removes tooltip titles that would be noisy:

- empty titles
- titles that repeat the app name
- duplicate titles, case-insensitively

There are two focused unit tests for this helper:

- `hides_titles_that_repeat_the_application_name`
- `retains_distinct_window_titles_without_duplicates`

The tooltip then shows a compact summary such as:

```text
COSMIC Terminal x2
Firefox (minimised)
  -> Release notes (minimised)
```

In the actual UI the arrow is rendered as a Unicode arrow. The example above
uses ASCII so this document stays plain-text friendly.

## Workspace Button Sizing

File:

- `src/components/app.rs`

The original applet could treat every workspace button as a fixed-size square
or rectangle because it only rendered a number.

Workspace Icons has variable-width buttons when icons are visible. Several
helpers were added for this:

- `suggested_button_size()`
- `app_icon_size()`
- `number_section_width()`
- `app_icon_slot_size()`
- `app_group_width()`
- `workspace_button_major_size()`

The applet still uses COSMIC's suggested panel size, but now calculates how
wide each workspace button needs to be based on:

- the number area
- the divider
- visible app icons
- icon spacing
- overflow indicator
- extra slot size for maximized highlights

Later commit `1d152bd` removed the upper clamp on icon size. Before that,
horizontal icons were capped at `24px` and vertical icons at `14px`. Now they
can grow with larger panels while still keeping a minimum size.

## Overflow Calculation

File:

- `src/components/app.rs`

The original `popup_index()` estimated overflow by dividing available panel
space by one fixed button width.

That does not work once some workspace buttons become wider because they have
icons. Workspace Icons changes overflow calculation to walk the workspace list
and add each button's actual major-axis size:

```rust
used += self.workspace_button_major_size(workspace);
if used > max_major_axis_len as f32 {
    return Some(index.max(1));
}
```

The `index.max(1)` part preserves at least one visible workspace button.

There is still a TODO in `view()` for rendering hidden overflow workspaces in
a popup. The applet calculates the split point, but the overflow UI itself is
not complete yet.

## Rendering App Icons

File:

- `src/components/app.rs`

The original button content was just bold workspace text.

Workspace Icons builds richer content:

- workspace number/name
- vertical divider
- row of app icons
- overflow marker if there are too many app groups

For horizontal panels, the layout is:

```text
[ workspace number | app icon app icon +N ]
```

For vertical panels, the number stays primary and a smaller app strip appears
under it:

```text
[ number ]
[ icons  ]
```

Only a limited number of app groups are shown:

- horizontal panel: up to `MAX_VISIBLE_APPS`, currently `5`
- vertical panel: up to `2`

If there are more app groups than visible slots, horizontal layout shows `+N`
and vertical layout shows an ellipsis.

## Minimized And Maximized States

Files:

- `src/components/app.rs`
- `src/config.rs`

`WorkspaceApp::all_minimized()` returns true when every window in that app
group is minimized. If the dimming setting is enabled, `app_icon()` renders
that app icon with lower opacity:

```rust
const MINIMIZED_ICON_OPACITY: f32 = 0.45;
```

`WorkspaceApp::has_maximized()` returns true when any window in that app group
is maximized. If the highlighting setting is enabled, horizontal panels render
the icon in a slightly larger slot with a soft circular background:

```rust
const MAXIMIZED_HIGHLIGHT_SCALE: f32 = 1.28;
const MAXIMIZED_ICON_GLOW_OPACITY: f32 = 0.24;
```

The maximized highlight is only applied in horizontal layout. Vertical layout
keeps icons smaller to preserve panel space.

## Button Styling

File:

- `src/components/app.rs`

The original applet styled active and urgent workspaces, and otherwise used a
plain background.

Workspace Icons keeps active and urgent behavior, then adds a subtle visual
treatment for inactive workspaces that contain apps:

- a `small_widget` background
- a `divider` border

Inactive empty workspaces remain visually lighter.

This makes occupied workspaces easier to scan without stealing the stronger
active-workspace styling.

## Preserved Workspace Controls

File:

- `src/components/app.rs`

These behaviors are intentionally preserved from Numbered Workspaces:

- hidden workspaces are filtered out
- workspaces are sorted by coordinates
- clicking an inactive workspace activates it
- clicking the active workspace launches `cosmic-workspaces`
- mouse wheel scrolling moves between workspaces
- active workspace uses the primary button style
- urgent workspace uses the urgent/destructive text treatment
- row/column layout follows the panel anchor
- autosize limits still respect panel suggested bounds

So the applet is not a new workspace switcher from scratch. It is the original
workspace switcher with an added icon-aware view model and renderer.

## Settings Popup

File:

- `src/components/app.rs`

The original applet had no popup window.

Workspace Icons adds:

- `popup: Option<window::Id>`
- `Message::TogglePopup`
- `Message::PopupClosed`
- `view_window()`

Right-clicking a workspace button toggles the popup. Closing the popup clears
the stored popup ID. `view_window()` builds the popup content using COSMIC
applet popup styling and two `toggler` controls.

## Localization

Files:

- `src/localize.rs`
- `i18n/en/cosmic_ext_applet_workspace_icons.ftl`
- `i18n/*/cosmic_ext_applet_workspace_icons.ftl`

The localization loader is still the same basic RustEmbed/i18n-embed setup,
but the fallback English file now defines the Workspace Icons name and settings
labels.

One thing to know: most non-English `.ftl` files still appear to contain the
old upstream `cosmic-applet-workspaces` key. Since the new popup labels are
only present in English right now, non-English users will likely fall back for
the new strings until those translations are filled in.

## Packaging And Installation

Files:

- `justfile`
- `README.md`
- `packaging/io.github.crocodile.cosmic-ext-applet-workspace-icons/io.github.crocodile.cosmic-ext-applet-workspace-icons.json`
- `packaging/io.github.crocodile.cosmic-ext-applet-workspace-icons/cargo-sources.json`
- `packaging/io.github.crocodile.cosmic-ext-applet-workspace-icons/cosmic-ext-applet-workspace-icons-wrapper`

The `justfile` gives simple local development commands:

- `just build`
- `just install`
- `just uninstall`

The Flatpak manifest adds a draft build for COSMIC Store-style distribution.
It builds the Rust binary offline, installs desktop/appstream/icon resources,
and grants read access to host application/icon directories so the applet can
resolve icons for apps installed outside the sandbox.

The wrapper script is part of the Flatpak icon-resolution fix. Instead of
running the Rust binary directly, Flatpak installs it as:

```text
/app/bin/cosmic-ext-applet-workspace-icons-bin
```

Then the command users run is a shell wrapper at:

```text
/app/bin/cosmic-ext-applet-workspace-icons
```

The wrapper sets:

- `XDG_DATA_HOME`
- `XDG_DATA_DIRS`

That gives the desktop-entry scanner the right host and sandbox paths when the
applet is running inside Flatpak.

Commit `669d76e` also expands Flatpak filesystem access for app/icon discovery:

- system Flatpak exports
- user Flatpak exports
- Flatpak app install data
- local desktop entries
- local icon themes
- Snap desktop/icon paths
- host OS data

Without those paths, the applet could know a window's app ID but fail to find
the matching desktop file or icon.

## Metadata And Documentation Polish

Files:

- `README.md`
- `resources/screenshots/workspace-icons-panel-1.png`
- `resources/screenshots/workspace-icons-panel-2.png`
- `resources/io.github.crocodile.cosmic-ext-applet-workspace-icons.metainfo.xml`

Later commits added:

- screenshots to the README
- screenshot metadata to appstream
- clearer description text
- a release entry for `0.1.0`
- Flatpak testing instructions
- a link explaining COSMIC

These are not core behavior changes, but they matter for distribution and for
people understanding what the applet does before installing it.

## Unused Or Inherited Code

File:

- `src/colors.rs`

`src/colors.rs` appears to be inherited or experimental theme-color parsing
code. It is not currently included from `src/lib.rs`, and the dependencies it
mentions are not listed in `Cargo.toml`, so it is not part of the compiled
applet today.

In other words: it is present in the repository, but not part of the current
Workspace Icons behavior.

## Mental Model

The easiest way to understand the new applet is as a pipeline:

```text
Wayland workspaces + Wayland toplevel windows
    -> WorkspaceSnapshot
    -> app metadata/icon lookup
    -> grouped WorkspaceApp values per workspace
    -> workspace buttons with number + icon strip + tooltip
```

Most of the code exists to keep those stages separate:

- `wayland.rs` knows about Wayland protocols and raw workspace/window state.
- `wayland_subscription.rs` bridges that Wayland thread into iced/COSMIC
  subscriptions.
- `components/app.rs` owns UI state, config, icon lookup, grouping, rendering,
  and user interaction.
- `config.rs` defines persistent user settings and app identity.
- packaging/resource files make the standalone applet installable and
  discoverable.

## Biggest Behavioral Differences

If you are comparing this to Numbered Workspaces, these are the changes that
matter most:

1. The applet is now a standalone third-party applet with its own identity.
2. The Wayland layer now tracks windows, not just workspaces.
3. The UI now groups windows by app and workspace.
4. Desktop files are scanned so Wayland app IDs can become real names/icons.
5. Workspace buttons are variable-width on horizontal panels.
6. App icons visually communicate minimized and maximized state.
7. A settings popup controls those two visual states.
8. Flatpak support needs extra host desktop/icon paths because icon lookup
   crosses the sandbox boundary.

## Source Links

- Upstream COSMIC applets repo:
  <https://github.com/pop-os/cosmic-applets>
- Upstream workspaces applet:
  <https://github.com/pop-os/cosmic-applets/tree/master/cosmic-applet-workspaces>
- This applet repository:
  <https://github.com/crocodile/cosmic-ext-applet-workspace-icons>
