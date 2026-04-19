//! On-disk layout persistence. Writes a small JSON file next to the user's
//! XDG config dir so pane positions/sizes/z-order survive across runs.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::workspace::{Pane, Workspace};

#[derive(Debug, Serialize, Deserialize)]
struct Layout {
    panes: Vec<PaneRecord>,
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

fn layout_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".config");
    p.push("clogger");
    p.push("gui-layout.json");
    Some(p)
}

pub fn save(ws: &Workspace) {
    let Some(path) = layout_path() else {
        return;
    };
    let layout = Layout {
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
    };
    let Ok(json) = serde_json::to_string_pretty(&layout) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, json);
    tracing::debug!(path = %path.display(), "saved gui layout");
}

pub fn load(ws: &mut Workspace) {
    let Some(path) = layout_path() else {
        return;
    };
    let Ok(json) = fs::read_to_string(&path) else {
        return;
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
    tracing::debug!(path = %path.display(), panes = ws.panes.len(), "loaded gui layout");
}
