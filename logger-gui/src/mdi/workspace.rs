use iced::widget::{button, column, container, mouse_area, row, stack, text, Space};
use iced::{
    event, mouse, window, Border, Color, Element, Event, Length, Point, Size, Subscription, Theme,
};

use super::positioned::Positioned;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Move,
    Resize,
}

#[derive(Debug, Clone, Copy)]
struct DragState {
    pane: u32,
    kind: DragKind,
    anchor: Point,
    start_pos: Point,
    start_size: Size,
}

#[derive(Debug)]
pub struct Pane {
    pub id: u32,
    pub title: String,
    pub pos: Point,
    pub size: Size,
    pub z: u32,
    /// Hidden panes stay in the workspace but are not rendered or hit-tested.
    /// The toolbar toggles them back into view at their last position/size.
    pub visible: bool,
    /// Content shown in the pane body. The host updates this with whatever
    /// derived state belongs in this pane (e.g. focused radio's freq).
    /// Not persisted — recomputed each session.
    pub body: String,
}

#[derive(Debug, Default)]
pub struct Workspace {
    pub panes: Vec<Pane>,
    next_z: u32,
    next_id: u32,
    drag: Option<DragState>,
    cursor: Point,
    window: Size,
    /// Top-left of the OS window in screen coordinates. Only populated
    /// from `Event::Window::Moved`; initially `None` so we don't overwrite
    /// saved state with zero before the first Moved event fires.
    window_pos: Option<Point>,
    focused: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum Message {
    StartMove(u32),
    StartResize(u32),
    PaneClicked(u32),
    Close(u32),
    Toggle(u32),
    CursorMoved(Point),
    MouseReleased,
    WindowResized(Size),
    WindowMoved(Point),
}

const MIN_W: f32 = 140.0;
const MIN_H: f32 = 80.0;
const TITLE_H: f32 = 24.0;
const RESIZE: f32 = 14.0;
const TOOLBAR_H: f32 = 32.0;
// Keep at least this much of a pane visible when dragging.
const VISIBLE_MARGIN: f32 = 60.0;

impl Workspace {
    pub fn add(&mut self, title: impl Into<String>, pos: Point, size: Size) -> u32 {
        self.next_id += 1;
        self.next_z += 1;
        let id = self.next_id;
        let pane = Pane {
            id,
            title: title.into(),
            pos,
            size,
            z: self.next_z,
            visible: true,
            body: String::new(),
        };
        self.panes.push(pane);
        self.focused = Some(id);
        id
    }

    /// Persistence calls this after replacing `panes` so subsequent `add()`s
    /// don't collide with restored ids/z values.
    pub(super) fn set_next_z(&mut self, z: u32) {
        self.next_z = z;
        self.next_id = self.panes.iter().map(|p| p.id).max().unwrap_or(0);
        self.focused = self
            .panes
            .iter()
            .filter(|p| p.visible)
            .max_by_key(|p| p.z)
            .map(|p| p.id);
    }

    /// Look up a pane id by title (used by the host to address panes by name
    /// after restoring layout from disk).
    pub fn id_by_title(&self, title: &str) -> Option<u32> {
        self.panes.iter().find(|p| p.title == title).map(|p| p.id)
    }

    /// Replace a pane's body content (for hosts using the plain-string body
    /// path; first-class panes inject their own `Element` via the view
    /// closure and don't need this).
    #[allow(dead_code)]
    pub fn set_body(&mut self, id: u32, body: impl Into<String>) {
        if let Some(p) = self.panes.iter_mut().find(|p| p.id == id) {
            p.body = body.into();
        }
    }

    fn raise_and_focus(&mut self, id: u32) {
        self.next_z += 1;
        if let Some(p) = self.panes.iter_mut().find(|p| p.id == id) {
            p.z = self.next_z;
        }
        self.focused = Some(id);
    }

    fn clamp_pos(window: Size, pos: Point, size: Size) -> Point {
        let max_x = (window.width - VISIBLE_MARGIN).max(0.0);
        // Workspace area starts below the toolbar, so the floor for pos.y is 0
        // (coordinates are relative to the workspace stack, not the window).
        let max_y = (window.height - TOOLBAR_H - TITLE_H).max(0.0);
        let min_x = -(size.width - VISIBLE_MARGIN).max(0.0);
        Point::new(pos.x.clamp(min_x, max_x), pos.y.clamp(0.0, max_y))
    }

    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::StartMove(id) => {
                if let Some(p) = self.panes.iter().find(|p| p.id == id) {
                    self.drag = Some(DragState {
                        pane: id,
                        kind: DragKind::Move,
                        anchor: self.cursor,
                        start_pos: p.pos,
                        start_size: p.size,
                    });
                }
                self.raise_and_focus(id);
            }
            Message::StartResize(id) => {
                if let Some(p) = self.panes.iter().find(|p| p.id == id) {
                    self.drag = Some(DragState {
                        pane: id,
                        kind: DragKind::Resize,
                        anchor: self.cursor,
                        start_pos: p.pos,
                        start_size: p.size,
                    });
                }
                self.raise_and_focus(id);
            }
            Message::PaneClicked(id) => self.raise_and_focus(id),
            Message::Close(id) => {
                if let Some(p) = self.panes.iter_mut().find(|p| p.id == id) {
                    p.visible = false;
                }
                if self.focused == Some(id) {
                    self.focused = self
                        .panes
                        .iter()
                        .filter(|p| p.visible)
                        .max_by_key(|p| p.z)
                        .map(|p| p.id);
                }
                if self.drag.map(|d| d.pane) == Some(id) {
                    self.drag = None;
                }
            }
            Message::Toggle(id) => {
                let became_visible = if let Some(p) = self.panes.iter_mut().find(|p| p.id == id) {
                    p.visible = !p.visible;
                    p.visible
                } else {
                    false
                };
                if became_visible {
                    self.raise_and_focus(id);
                } else if self.focused == Some(id) {
                    self.focused = self
                        .panes
                        .iter()
                        .filter(|p| p.visible)
                        .max_by_key(|p| p.z)
                        .map(|p| p.id);
                }
            }
            Message::CursorMoved(p) => {
                self.cursor = p;
                if let Some(d) = self.drag {
                    let window = self.window;
                    if let Some(pane) = self.panes.iter_mut().find(|x| x.id == d.pane) {
                        let dx = p.x - d.anchor.x;
                        let dy = p.y - d.anchor.y;
                        match d.kind {
                            DragKind::Move => {
                                let raw = Point::new(d.start_pos.x + dx, d.start_pos.y + dy);
                                pane.pos = Self::clamp_pos(window, raw, pane.size);
                            }
                            DragKind::Resize => {
                                pane.size = Size::new(
                                    (d.start_size.width + dx).max(MIN_W),
                                    (d.start_size.height + dy).max(MIN_H),
                                );
                                pane.pos = Self::clamp_pos(window, pane.pos, pane.size);
                            }
                        }
                    }
                }
            }
            Message::MouseReleased => self.drag = None,
            Message::WindowResized(size) => {
                self.window = size;
                for pane in self.panes.iter_mut() {
                    pane.pos = Self::clamp_pos(size, pane.pos, pane.size);
                }
            }
            Message::WindowMoved(pos) => {
                self.window_pos = Some(pos);
            }
        }
    }

    /// Current OS-window geometry, for persistence. Returns `None` until
    /// at least one `WindowMoved` or `WindowResized` event has been seen.
    pub fn saved_window(&self) -> Option<crate::mdi::WindowState> {
        // Size must be non-zero — iced may report (0,0) between sizing
        // events on some platforms; don't persist garbage.
        if self.window.width <= 0.0 || self.window.height <= 0.0 {
            return None;
        }
        let pos = self.window_pos?;
        Some(crate::mdi::WindowState {
            x: pos.x,
            y: pos.y,
            w: self.window.width,
            h: self.window.height,
        })
    }

    /// Seed window geometry from a restored layout so `saved_window` has
    /// something to return even before the first live window event.
    pub fn set_saved_window(&mut self, state: crate::mdi::WindowState) {
        self.window = Size::new(state.w, state.h);
        self.window_pos = Some(Point::new(state.x, state.y));
    }

    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|ev, _status, _id| match ev {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorMoved(position))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Some(Message::MouseReleased)
            }
            Event::Window(window::Event::Resized(size)) => Some(Message::WindowResized(size)),
            Event::Window(window::Event::Moved(pos)) => Some(Message::WindowMoved(pos)),
            _ => None,
        })
    }

    /// Top-level render. `body_for` yields the interior content of each pane
    /// in the host's message space; `wrap` lifts internal MDI messages into
    /// that same space so host and MDI messages coexist in one tree.
    /// `status_bar` is a fixed-height element appended below the workspace
    /// (pass `Space::new(0, 0)` to omit).
    pub fn view<'a, M, F>(
        &'a self,
        body_for: F,
        status_bar: Element<'a, M>,
        wrap: fn(Message) -> M,
    ) -> Element<'a, M>
    where
        M: 'a + Clone,
        F: Fn(&'a Pane) -> Element<'a, M>,
    {
        let toolbar: Element<'a, Message> = self.toolbar_view();
        let workspace: Element<'a, M> = self.workspace_view(body_for, wrap);
        column![toolbar.map(wrap), workspace, status_bar]
            .spacing(0)
            .into()
    }

    fn toolbar_view(&self) -> Element<'_, Message> {
        // Panes are listed in creation order (by id) so the toolbar layout
        // stays stable even as z-order shuffles.
        let mut by_id: Vec<&Pane> = self.panes.iter().collect();
        by_id.sort_by_key(|p| p.id);

        let mut items: Vec<Element<Message>> = Vec::with_capacity(by_id.len());
        for p in by_id {
            items.push(toolbar_button(p).into());
        }

        container(
            row(items)
                .spacing(4)
                .align_y(iced::alignment::Vertical::Center),
        )
        .padding([4, 8])
        .width(Length::Fill)
        .height(TOOLBAR_H)
        .style(|t: &Theme| {
            let pal = t.extended_palette().background.strong;
            container::Style {
                background: Some(pal.color.into()),
                border: Border {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
    }

    fn workspace_view<'a, M, F>(
        &'a self,
        body_for: F,
        wrap: fn(Message) -> M,
    ) -> Element<'a, M>
    where
        M: 'a + Clone,
        F: Fn(&'a Pane) -> Element<'a, M>,
    {
        let mut order: Vec<&Pane> = self.panes.iter().filter(|p| p.visible).collect();
        order.sort_by_key(|p| p.z);

        let mut layers: Vec<Element<'a, M>> = Vec::with_capacity(order.len() + 1);
        let background: Element<Message> = container(Space::new(Length::Fill, Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|t: &Theme| {
                let bg = t.extended_palette().background.weak.color;
                container::Style {
                    background: Some(bg.into()),
                    ..container::Style::default()
                }
            })
            .into();
        layers.push(background.map(wrap));

        for pane in order {
            let focused = self.focused == Some(pane.id);
            let body = body_for(pane);
            let rendered = pane_view(pane, focused, body, wrap);
            layers.push(Positioned::new(pane.pos, pane.size, rendered).into());
        }

        stack(layers).width(Length::Fill).height(Length::Fill).into()
    }
}

fn toolbar_button(pane: &Pane) -> iced::widget::Button<'_, Message> {
    let visible = pane.visible;
    let label = text(pane.title.clone()).size(12);
    button(label)
        .padding([4, 10])
        .on_press(Message::Toggle(pane.id))
        .style(move |t: &Theme, status| {
            let palette = t.extended_palette();
            let (bg, fg) = if visible {
                let p = palette.primary.strong;
                (p.color, p.text)
            } else {
                let p = palette.background.weak;
                (p.color, p.text)
            };
            let bg = match status {
                button::Status::Hovered => {
                    if visible {
                        palette.primary.base.color
                    } else {
                        palette.background.base.color
                    }
                }
                _ => bg,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: fg,
                border: Border {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.0),
                    width: 0.0,
                    radius: 3.0.into(),
                },
                ..button::Style::default()
            }
        })
}

fn pane_view<'a, M>(
    pane: &'a Pane,
    focused: bool,
    body: Element<'a, M>,
    wrap: fn(Message) -> M,
) -> Element<'a, M>
where
    M: 'a + Clone,
{
    let title_text = container(text(pane.title.clone()).size(13))
        .padding([2, 8])
        .width(Length::Fill)
        .height(TITLE_H)
        .style(move |t: &Theme| {
            let pal = if focused {
                t.extended_palette().primary.strong
            } else {
                t.extended_palette().background.strong
            };
            container::Style {
                background: Some(pal.color.into()),
                text_color: Some(pal.text),
                ..container::Style::default()
            }
        });

    let drag_handle = mouse_area(title_text)
        .on_press(wrap(Message::StartMove(pane.id)))
        .interaction(mouse::Interaction::Grab);

    let close_btn = button(text("✕").size(12))
        .padding([0, 6])
        .on_press(wrap(Message::Close(pane.id)))
        .style(|t: &Theme, status| {
            let pal = t.extended_palette().background.strong;
            let bg = match status {
                button::Status::Hovered => t.extended_palette().danger.base.color,
                _ => pal.color,
            };
            let fg = match status {
                button::Status::Hovered => t.extended_palette().danger.base.text,
                _ => pal.text,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: fg,
                border: Border::default(),
                ..button::Style::default()
            }
        });

    let title_bar = row![drag_handle, close_btn]
        .height(TITLE_H)
        .align_y(iced::alignment::Vertical::Center);

    // Wrap the body so a click anywhere inside raises this pane. The body
    // itself is already in host-message space; the click emits an MDI
    // Message via `wrap`.
    let body_area = mouse_area(body).on_press(wrap(Message::PaneClicked(pane.id)));

    let resize_handle = mouse_area(
        container(text("◢").size(12).color(Color::from_rgb(0.6, 0.6, 0.6)))
            .padding(0)
            .width(RESIZE)
            .height(RESIZE)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center),
    )
    .on_press(wrap(Message::StartResize(pane.id)))
    .interaction(mouse::Interaction::ResizingDiagonallyDown);

    let footer = row![Space::new(Length::Fill, RESIZE), resize_handle];

    let border_color = if focused {
        Color::from_rgb(0.45, 0.55, 0.85)
    } else {
        Color::from_rgb(0.35, 0.35, 0.35)
    };
    let border_width = if focused { 1.5 } else { 1.0 };

    container(column![title_bar, body_area, footer].spacing(0))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |t: &Theme| {
            let pal = t.extended_palette().background.base;
            container::Style {
                background: Some(pal.color.into()),
                border: Border {
                    color: border_color,
                    width: border_width,
                    radius: 3.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}
