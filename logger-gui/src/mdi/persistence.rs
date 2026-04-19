//! On-disk layout persistence. Writes a small JSON file next to the user's
//! XDG config dir so pane positions/sizes/z-order survive across runs.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::workspace::{Pane, Workspace};

/// Saved outer-window geometry. Loaded before `iced::application(...).run()`
/// so the first render already appears where the user last left it.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Layout {
    /// `None` means no saved window geometry — iced falls back to its
    /// default size/position on first run. Older layout files without
    /// this field deserialize as `None` via `#[serde(default)]`.
    #[serde(default)]
    window: Option<WindowState>,
    #[serde(default)]
    panes: Vec<PaneRecord>,
    /// UI scale factor. 1.0 = native size; >1 enlarges; <1 shrinks. Applied
    /// via `iced::application::scale_factor`, so every widget (text,
    /// padding, borders) scales together.
    #[serde(default)]
    font_scale: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PaneRecord {
    id: u32,
    title: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    z: u32,
    #[serde(default = "default_true")]
    visible: bool,
}

fn default_true() -> bool {
    true
}

/// Where the layout JSON lives. Uses platform-appropriate config dirs via
/// the `dirs` crate: `$XDG_CONFIG_HOME/clogger` (or `$HOME/.config/clogger`)
/// on Linux, `%APPDATA%\clogger` on Windows, `~/Library/Application Support/
/// clogger` on macOS. Previous versions only checked `$HOME`, which broke
/// persistence entirely on Windows.
pub fn layout_path() -> Option<PathBuf> {
    let mut p = dirs::config_dir().or_else(|| {
        // Fallback for environments where dirs returns None (e.g. $HOME
        // unset on a Linux CI runner). Try $HOME/.config manually.
        std::env::var_os("HOME").map(|h| {
            let mut p = PathBuf::from(h);
            p.push(".config");
            p
        })
    })?;
    p.push("clogger");
    p.push("gui-layout.json");
    Some(p)
}

pub fn save(ws: &Workspace, font_scale: f32) {
    let Some(path) = layout_path() else {
        tracing::warn!(
            "cannot save gui layout: no config dir available (platform missing $HOME / $APPDATA?)"
        );
        return;
    };
    let layout = Layout {
        window: ws.saved_window(),
        panes: ws
            .panes
            .iter()
            .map(|p| PaneRecord {
                id: p.id,
                title: p.title.clone(),
                x: p.pos.x,
                y: p.pos.y,
                w: p.size.width,
                h: p.size.height,
                z: p.z,
                visible: p.visible,
            })
            .collect(),
        // Persist 1.0 explicitly rather than null so the file is
        // self-describing once saved.
        font_scale: Some(font_scale),
    };
    let json = match serde_json::to_string_pretty(&layout) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!(err = %e, "serializing gui layout failed");
            return;
        }
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            tracing::warn!(path = %parent.display(), err = %e, "mkdir for gui layout failed");
            return;
        }
    }
    if let Err(e) = fs::write(&path, json) {
        tracing::warn!(path = %path.display(), err = %e, "writing gui layout failed");
        return;
    }
    tracing::trace!(path = %path.display(), panes = ws.panes.len(), "saved gui layout");
}

/// Read just the window geometry from the layout file — called before iced
/// starts up so we can pass a `window::Settings` with the right pos/size.
/// Returns `None` if the file doesn't exist, can't be parsed, or has no
/// saved window record yet.
pub fn load_window_state() -> Option<WindowState> {
    let path = layout_path()?;
    let json = fs::read_to_string(&path).ok()?;
    let layout: Layout = serde_json::from_str(&json).ok()?;
    layout.window
}

/// Read just the saved UI scale factor. Called before iced starts so the
/// initial `scale_factor` callback returns the right value on first paint.
pub fn load_font_scale() -> Option<f32> {
    let path = layout_path()?;
    let json = fs::read_to_string(&path).ok()?;
    let layout: Layout = serde_json::from_str(&json).ok()?;
    layout.font_scale
}

pub fn load(ws: &mut Workspace) {
    let Some(path) = layout_path() else {
        tracing::info!(
            "no config dir available; running with default gui layout"
        );
        return;
    };
    let json = match fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(path = %path.display(), "no existing gui layout; will create on first save");
            return;
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), err = %e, "reading gui layout failed");
            return;
        }
    };
    let Ok(layout) = serde_json::from_str::<Layout>(&json) else {
        tracing::warn!(path = %path.display(), "gui layout file unparseable; ignoring");
        return;
    };
    let mut max_z = 0;
    ws.panes = layout
        .panes
        .into_iter()
        .map(|r| {
            max_z = max_z.max(r.z);
            Pane {
                id: r.id,
                title: r.title,
                pos: iced::Point::new(r.x, r.y),
                size: iced::Size::new(r.w, r.h),
                z: r.z,
                visible: r.visible,
                body: String::new(),
            }
        })
        .collect();
    ws.set_next_z(max_z);
    if let Some(w) = layout.window {
        ws.set_saved_window(w);
    }
    tracing::info!(path = %path.display(), panes = ws.panes.len(), "loaded gui layout");
}
