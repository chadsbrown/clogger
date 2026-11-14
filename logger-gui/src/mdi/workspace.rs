use iced::widget::{button, column, container, mouse_area, row, stack, text, Space};
use iced::{
    event, mouse, window, Border, Color, Element, Event, Length, Padding, Point, Size,
    Subscription, Theme,
};

use super::positioned::Positioned;
use crate::panes::style::{self, Icon};
use crate::panes::widgets::{self, ButtonVariant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HEdge {
    None,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VEdge {
    None,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeEdges {
    h: HEdge,
    v: VEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Move,
    Resize(ResizeEdges),
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
    StartResize(u32, ResizeEdges),
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
/// Title-bar height. It was 20px, which squeezed 11px text against the
/// edges. 26px gives the title room and a comfortable drag target.
const TITLE_H: f32 = 26.0;
/// The pane frame's border width. Pane content is inset by exactly this
/// much, so nothing inside (title tint, hairline, canvas) can paint over
/// the border.
const FRAME_BORDER: f32 = 1.0;
// Thickness of the perimeter resize zones (top/bottom strips and left/right strips).
const EDGE: f32 = 6.0;
// Length of each corner zone along the edge it sits on. Slightly larger than EDGE
// so the corner's diagonal-resize area dominates over the straight-edge zones at
// the meeting points.
const CORNER: f32 = 12.0;
const TOOLBAR_H: f32 = 34.0;
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
            Message::StartResize(id, edges) => {
                if let Some(p) = self.panes.iter().find(|p| p.id == id) {
                    self.drag = Some(DragState {
                        pane: id,
                        kind: DragKind::Resize(edges),
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
                            DragKind::Resize(edges) => {
                                let (new_x, new_w) = match edges.h {
                                    HEdge::None => (d.start_pos.x, d.start_size.width),
                                    HEdge::Right => {
                                        (d.start_pos.x, (d.start_size.width + dx).max(MIN_W))
                                    }
                                    HEdge::Left => {
                                        let new_w = (d.start_size.width - dx).max(MIN_W);
                                        let used = d.start_size.width - new_w;
                                        (d.start_pos.x + used, new_w)
                                    }
                                };
                                let (new_y, new_h) = match edges.v {
                                    VEdge::None => (d.start_pos.y, d.start_size.height),
                                    VEdge::Bottom => {
                                        (d.start_pos.y, (d.start_size.height + dy).max(MIN_H))
                                    }
                                    VEdge::Top => {
                                        let new_h = (d.start_size.height - dy).max(MIN_H);
                                        let used = d.start_size.height - new_h;
                                        (d.start_pos.y + used, new_h)
                                    }
                                };
                                pane.size = Size::new(new_w, new_h);
                                pane.pos = Self::clamp_pos(
                                    window,
                                    Point::new(new_x, new_y),
                                    pane.size,
                                );
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
            items.push(toolbar_button(p));
        }

        // A neutral strip on the pane surface with a hairline beneath.
        // Earlier versions used `background.strong` plus a filled primary
        // pill for every visible pane, which made the toolbar the
        // brightest thing on screen.
        column![
            container(
                row(items)
                    .spacing(style::SP_1)
                    .align_y(iced::alignment::Vertical::Center),
            )
            .padding([0.0, style::SP_3])
            .width(Length::Fill)
            .height(TOOLBAR_H - 1.0)
            .align_y(iced::alignment::Vertical::Center)
            .style(|t: &Theme| container::Style {
                background: Some(style::surface_pane(t).into()),
                ..container::Style::default()
            }),
            widgets::hairline(),
        ]
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
        let background: Element<Message> = container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|t: &Theme| {
                let bg = style::surface_canvas(t);
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

/// A toolbar toggle for one pane. Visible panes show body-colored text
/// over a 2px accent underline; hidden panes show muted text and no
/// underline. This is the familiar tab idiom, and it keeps the toolbar
/// calm.
fn toolbar_button(pane: &Pane) -> Element<'_, Message> {
    let visible = pane.visible;
    let label = text(pane.title.clone())
        .size(style::TEXT_LABEL)
        .font(if visible {
            style::FONT_UI_MEDIUM
        } else {
            style::FONT_UI
        });
    let btn = button(label)
        .padding([style::SP_2, style::SP_3 + 2.0])
        .on_press(Message::Toggle(pane.id))
        .style(move |t: &Theme, status| {
            let mut s = widgets::button_style(ButtonVariant::Ghost)(t, status);
            if !matches!(status, button::Status::Hovered) {
                s.text_color = if visible {
                    style::text_color(t)
                } else {
                    style::very_muted_color(t)
                };
            }
            s
        });
    let underline = container(Space::new().height(2))
        .width(Length::Fill)
        .height(2)
        .style(move |t: &Theme| container::Style {
            background: visible.then(|| style::accent_color(t).into()),
            border: Border {
                radius: 1.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    column![btn, underline]
        .width(Length::Shrink)
        .align_x(iced::alignment::Horizontal::Center)
        .into()
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
    // The title bar sits on the pane surface with a hairline below. Focus
    // shows as a faint accent tint on the title bar, accent title text, an
    // accent border and a deeper shadow. Earlier versions filled the whole
    // focused title bar with primary, which competed with the entry pane's
    // focus cues.
    //
    // A separate 2px accent stripe was tried and dropped. A 2px-tall shape
    // can't carry the frame's 5px inner corner radius, so it broke the
    // rounded top-left corner. The full-width hairline also painted over
    // the 1px accent border, which left a notch just below the title.
    let title_text = container(
        text(pane.title.to_uppercase())
            .size(style::TEXT_TINY)
            .font(style::FONT_UI_SEMIBOLD)
            .style(move |t: &Theme| {
                if focused {
                    style::accent(t)
                } else {
                    style::muted(t)
                }
            }),
    )
    .padding(Padding {
        left: style::SP_4 - 2.0,
        ..Padding::ZERO
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .align_y(iced::alignment::Vertical::Center);

    let drag_handle = mouse_area(title_text)
        .on_press(wrap(Message::StartMove(pane.id)))
        .interaction(mouse::Interaction::Grab);

    let close_btn: Element<'a, M> = container(
        widgets::icon_button(
            Icon::X,
            style::TEXT_LABEL,
            ButtonVariant::GhostDanger,
            wrap(Message::Close(pane.id)),
        )
        .padding(3),
    )
    .padding(Padding {
        right: style::SP_2,
        ..Padding::ZERO
    })
    .height(Length::Fill)
    .align_y(iced::alignment::Vertical::Center)
    .into();

    let inner_radius = style::RADIUS_MD - FRAME_BORDER;
    let title_bar = column![
        container(
            row![drag_handle, close_btn]
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Center),
        )
        .width(Length::Fill)
        .height(TITLE_H - FRAME_BORDER - 1.0)
        .style(move |t: &Theme| container::Style {
            background: focused.then(|| style::accent_tint(t).into()),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: iced::border::Radius {
                    top_left: inner_radius,
                    top_right: inner_radius,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            },
            ..container::Style::default()
        }),
        widgets::hairline(),
    ];

    // Wrap the body so a click anywhere inside raises this pane. The body
    // itself is already in host-message space; the click emits an MDI
    // Message via `wrap`.
    let body_area = mouse_area(body).on_press(wrap(Message::PaneClicked(pane.id)));

    let pane_main: Element<'a, M> = container(column![title_bar, body_area].spacing(0))
        .padding(FRAME_BORDER)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |t: &Theme| container::Style {
            background: Some(style::surface_pane(t).into()),
            shadow: if focused {
                style::shadow_focus(t)
            } else {
                style::shadow_rest(t)
            },
            border: Border {
                color: if focused {
                    style::focused_border_color(t)
                } else {
                    style::border_color(t)
                },
                width: FRAME_BORDER,
                radius: style::RADIUS_MD.into(),
            },
            ..container::Style::default()
        })
        .into();

    // 8 invisible resize zones layered on top of the pane body. The middle cell
    // is a plain Space (no mouse_area), so events at the body fall through to
    // pane_main below — that's what keeps the body clickable for raise-on-press.
    let perimeter = perimeter_overlay(pane.id, wrap);

    stack![pane_main, perimeter]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn perimeter_overlay<'a, M>(pane_id: u32, wrap: fn(Message) -> M) -> Element<'a, M>
where
    M: 'a + Clone,
{
    use iced::Length::{Fill, Fixed};

    let nw = corner_zone(
        pane_id,
        ResizeEdges { h: HEdge::Left, v: VEdge::Top },
        mouse::Interaction::ResizingDiagonallyDown,
        wrap,
    );
    let ne = corner_zone(
        pane_id,
        ResizeEdges { h: HEdge::Right, v: VEdge::Top },
        mouse::Interaction::ResizingDiagonallyUp,
        wrap,
    );
    let sw = corner_zone(
        pane_id,
        ResizeEdges { h: HEdge::Left, v: VEdge::Bottom },
        mouse::Interaction::ResizingDiagonallyUp,
        wrap,
    );
    let se = corner_zone(
        pane_id,
        ResizeEdges { h: HEdge::Right, v: VEdge::Bottom },
        mouse::Interaction::ResizingDiagonallyDown,
        wrap,
    );

    let n_edge = horizontal_edge(
        pane_id,
        ResizeEdges { h: HEdge::None, v: VEdge::Top },
        wrap,
    );
    let s_edge = horizontal_edge(
        pane_id,
        ResizeEdges { h: HEdge::None, v: VEdge::Bottom },
        wrap,
    );
    let w_edge = vertical_edge(
        pane_id,
        ResizeEdges { h: HEdge::Left, v: VEdge::None },
        wrap,
    );
    let e_edge = vertical_edge(
        pane_id,
        ResizeEdges { h: HEdge::Right, v: VEdge::None },
        wrap,
    );

    let middle: Element<'a, M> = Space::new().width(Fill).height(Fill).into();

    column![
        row![nw, n_edge, ne].height(Fixed(EDGE)),
        row![w_edge, middle, e_edge].height(Fill),
        row![sw, s_edge, se].height(Fixed(EDGE)),
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

fn corner_zone<'a, M>(
    pane_id: u32,
    edges: ResizeEdges,
    interaction: mouse::Interaction,
    wrap: fn(Message) -> M,
) -> Element<'a, M>
where
    M: 'a + Clone,
{
    mouse_area(
        Space::new()
            .width(Length::Fixed(CORNER))
            .height(Length::Fixed(EDGE)),
    )
    .on_press(wrap(Message::StartResize(pane_id, edges)))
    .interaction(interaction)
    .into()
}

fn horizontal_edge<'a, M>(
    pane_id: u32,
    edges: ResizeEdges,
    wrap: fn(Message) -> M,
) -> Element<'a, M>
where
    M: 'a + Clone,
{
    mouse_area(
        Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(EDGE)),
    )
    .on_press(wrap(Message::StartResize(pane_id, edges)))
    .interaction(mouse::Interaction::ResizingVertically)
    .into()
}

fn vertical_edge<'a, M>(
    pane_id: u32,
    edges: ResizeEdges,
    wrap: fn(Message) -> M,
) -> Element<'a, M>
where
    M: 'a + Clone,
{
    mouse_area(
        Space::new()
            .width(Length::Fixed(EDGE))
            .height(Length::Fill),
    )
    .on_press(wrap(Message::StartResize(pane_id, edges)))
    .interaction(mouse::Interaction::ResizingHorizontally)
    .into()
}
