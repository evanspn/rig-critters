//! The terminal UI: tabs (one per rig), the scene (a pixel picture), a side panel for the selected seat, and a footer.

use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use ratatui::Frame as UiFrame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::canvas::Canvas;
use crate::model::{Edge, Mood, RigScene, Seat, Snapshot, mood};
use crate::scene::{self, SLOT_H, SLOT_W, SeatView};
use crate::species::Animals;
use crate::sprites::{self, Sprite};
use crate::termimg::{self, Protocol};

const KITTY_ID: u32 = 7777;
const KITTY_MAX: (u32, u32) = (960, 540);
const SIDE_WIDTH: u16 = 34;

pub struct Options {
    pub demo: bool,
    pub protocol: Option<Protocol>,
    pub fps: u32,
    pub no_tint: bool,
    pub zoom: Option<u32>,
}

/// Where the picture goes and how logical pixels map to screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// the picture's cells
    pub image: Rect,
    /// the picture's pixel size
    pub px: (u32, u32),
    pub zoom: u32,
    /// logical canvas size
    pub logical: (u32, u32),
    /// centering offset of the scaled canvas inside `px`
    pub offset: (u32, u32),
    /// logical rows scrolled off the top (when the seats do not all fit)
    pub scroll: i32,
}

impl Plan {
    /// Image pixels per cell.
    pub fn cell(&self) -> (f32, f32) {
        (
            self.px.0 as f32 / self.image.width.max(1) as f32,
            self.px.1 as f32 / self.image.height.max(1) as f32,
        )
    }

    /// The cell holding logical pixel (lx, ly).
    pub fn logical_to_cell(&self, lx: i32, ly: i32) -> (i32, i32) {
        let (cw, ch) = self.cell();
        let px = self.offset.0 as f32 + lx as f32 * self.zoom as f32;
        let py = self.offset.1 as f32 + (ly - self.scroll) as f32 * self.zoom as f32;
        (
            (px / cw).floor() as i32 + self.image.x as i32,
            (py / ch).floor() as i32 + self.image.y as i32,
        )
    }

    /// The logical pixel under screen cell (col, row).
    pub fn cell_to_logical(&self, col: u16, row: u16) -> (i32, i32) {
        let (cw, ch) = self.cell();
        let px = (col.saturating_sub(self.image.x)) as f32 * cw + cw / 2.0;
        let py = (row.saturating_sub(self.image.y)) as f32 * ch + ch / 2.0;
        (
            ((px - self.offset.0 as f32) / self.zoom as f32).floor() as i32,
            ((py - self.offset.1 as f32) / self.zoom as f32).floor() as i32 + self.scroll,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Areas {
    pub tabs: Rect,
    pub pane: Rect,
    pub image: Rect,
    pub side: Rect,
    pub footer: Rect,
}

/// The layout for a terminal of `area`. Pure, never panics on tiny sizes.
pub fn layout(area: Rect) -> Areas {
    let tabs_h = if area.height >= 6 { 1 } else { 0 };
    let footer_h = if area.height >= 8 { 1 } else { 0 };
    let [tabs, body, footer] = Layout::vertical([
        Constraint::Length(tabs_h),
        Constraint::Min(0),
        Constraint::Length(footer_h),
    ])
    .areas(area);
    let (pane, side) = if body.width >= SIDE_WIDTH + 30 {
        let [a, b] =
            Layout::horizontal([Constraint::Min(10), Constraint::Length(SIDE_WIDTH)]).areas(body);
        (a, b)
    } else {
        (body, Rect::new(body.x, body.y, 0, 0))
    };
    let image = if pane.width >= 4 && pane.height >= 3 {
        Block::bordered().inner(pane)
    } else {
        Rect::new(pane.x, pane.y, 0, 0)
    };
    Areas {
        tabs,
        pane,
        image,
        side,
        footer,
    }
}

pub fn plan_for(
    proto: Protocol,
    image: Rect,
    cell_px: (f32, f32),
    want_zoom: u32,
    n_seats: usize,
    selected: Option<usize>,
) -> Plan {
    let (cols, rows) = (image.width.max(1) as u32, image.height.max(1) as u32);
    let px = match proto {
        Protocol::HalfBlocks => (cols, rows * 2),
        _ => {
            let (w, h) = (cols as f32 * cell_px.0, rows as f32 * cell_px.1);
            let k = (KITTY_MAX.0 as f32 / w)
                .min(KITTY_MAX.1 as f32 / h)
                .min(1.0);
            (((w * k) as u32).max(8), ((h * k) as u32).max(8))
        }
    };
    let zoom = scene::fit_zoom(px, n_seats.max(1), want_zoom.max(1));
    let logical = ((px.0 / zoom).max(1), (px.1 / zoom).max(1));
    let offset = ((px.0 - logical.0 * zoom) / 2, (px.1 - logical.1 * zoom) / 2);
    let scroll = scene::scroll_for(logical.0 as i32, n_seats.max(1), logical.1 as i32, selected);
    Plan {
        image,
        px,
        zoom,
        logical,
        offset,
        scroll,
    }
}

pub struct App {
    pub sprites: Vec<Sprite>,
    pub animals: Animals,
    pub problems: Vec<String>,
    pub snapshot: Snapshot,
    pub received: Instant,
    pub rx: Option<Receiver<Snapshot>>,
    pub demo: bool,
    pub proto: Protocol,
    pub note: Option<String>,
    pub truecolor: bool,
    pub cell_px: (f32, f32),
    pub rig: usize,
    pub sel_name: Option<String>,
    pub want_zoom: u32,
    pub paused: bool,
    pub help: bool,
    pub quit: bool,
    pub t: f32,
    pub hour: f32,
    pub tabs: Vec<(Rect, usize)>,
    pub plan: Option<Plan>,
    pub canvas: Option<Canvas>,
    kitty_shown: bool,
}

fn style_dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn cream() -> Color {
    Color::Rgb(244, 236, 214)
}

pub fn mood_name(m: Mood) -> &'static str {
    match m {
        Mood::Working => "working",
        Mood::Napping => "napping (idle)",
        Mood::Alert => "needs attention",
        Mood::Parked => "parked / waiting",
        Mood::Ghost => "not running",
        Mood::Unknown => "state unknown",
    }
}

pub fn ago(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

impl App {
    pub fn new(opts: &Options, proto: Protocol, note: Option<String>) -> App {
        let (animals, problems) = Animals::load();
        App {
            sprites: sprites::builtin(),
            animals,
            problems,
            snapshot: Snapshot::default(),
            received: Instant::now(),
            rx: None,
            demo: opts.demo,
            proto,
            note,
            truecolor: truecolor_supported(),
            cell_px: (14.0, 28.0),
            rig: 0,
            sel_name: None,
            want_zoom: opts
                .zoom
                .unwrap_or(if proto == Protocol::HalfBlocks { 1 } else { 4 }),
            paused: false,
            help: false,
            quit: false,
            t: 0.0,
            hour: 14.0,
            tabs: Vec::new(),
            plan: None,
            canvas: None,
            kitty_shown: false,
        }
    }

    pub fn scene(&self) -> Option<&RigScene> {
        self.snapshot
            .rigs
            .get(self.rig.min(self.snapshot.rigs.len().saturating_sub(1)))
    }

    pub fn views(&self) -> Vec<SeatView<'_>> {
        self.scene()
            .map(|s| {
                s.seats
                    .iter()
                    .map(|seat| SeatView {
                        seat,
                        species: self.animals.species(&seat.pod, &seat.member),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Index of the selected seat in the current scene (the first one if the remembered seat is gone).
    pub fn selected(&self) -> Option<usize> {
        let s = self.scene()?;
        if s.seats.is_empty() {
            return None;
        }
        Some(
            self.sel_name
                .as_ref()
                .and_then(|n| s.seats.iter().position(|x| &x.name == n))
                .unwrap_or(0),
        )
    }

    fn select_index(&mut self, i: usize) {
        if let Some(s) = self.scene()
            && let Some(seat) = s.seats.get(i)
        {
            self.sel_name = Some(seat.name.clone());
        }
    }

    pub fn set_snapshot(&mut self, snap: Snapshot) {
        self.snapshot = snap;
        self.received = Instant::now();
        if self.rig >= self.snapshot.rigs.len() {
            self.rig = 0;
        }
    }

    /// Paint the scene for `image` cells into `self.canvas` (at the picture's pixel size).
    pub fn prepare(&mut self, image: Rect) {
        let n = self.scene().map_or(0, |s| s.seats.len());
        let plan = plan_for(
            self.proto,
            image,
            self.cell_px,
            self.want_zoom,
            n,
            self.selected(),
        );
        // paint everything, then show the part that fits
        let full_h = plan
            .logical
            .1
            .max(scene::content_height(plan.logical.0 as i32, n.max(1)) as u32);
        let mut logical = Canvas::new(plan.logical.0, full_h, [0, 0, 0]);
        if self.snapshot.asleep || n == 0 {
            scene::draw_asleep(&mut logical, &self.sprites, self.t);
        } else if let Some(sc) = self.scene() {
            let views: Vec<SeatView> = sc
                .seats
                .iter()
                .map(|s| SeatView {
                    seat: s,
                    species: self.animals.species(&s.pod, &s.member),
                })
                .collect();
            scene::draw_rig(
                &mut logical,
                &self.sprites,
                &views,
                &sc.edges,
                self.selected(),
                self.t,
                self.hour,
            );
        }
        let view = logical.crop_rows(plan.scroll as u32, plan.logical.1);
        self.canvas = Some(view.scaled_to(plan.px.0, plan.px.1, [18, 16, 26]));
        self.plan = Some(plan);
    }

    // ---- input ----------------------------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.help {
            self.help = false;
            return;
        }
        let n = self.scene().map_or(0, |s| s.seats.len());
        let per_row = self
            .plan
            .map_or(1, |p| scene::grid(p.logical.0 as i32, n).0)
            .max(1);
        let cur = self.selected().unwrap_or(0);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('p') => self.paused = !self.paused,
            KeyCode::Char('+') | KeyCode::Char('=') => self.want_zoom = (self.want_zoom + 1).min(8),
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.want_zoom = self.want_zoom.saturating_sub(1).max(1)
            }
            KeyCode::Tab => self.switch_rig(1),
            KeyCode::BackTab => self.switch_rig(-1),
            KeyCode::Right | KeyCode::Char('l') if n > 0 => self.select_index((cur + 1) % n),
            KeyCode::Left | KeyCode::Char('h') if n > 0 => self.select_index((cur + n - 1) % n),
            KeyCode::Down | KeyCode::Char('j') if n > 0 => {
                self.select_index((cur + per_row).min(n - 1))
            }
            KeyCode::Up | KeyCode::Char('k') if n > 0 => {
                self.select_index(cur.saturating_sub(per_row))
            }
            _ => {}
        }
    }

    fn switch_rig(&mut self, by: isize) {
        let n = self.snapshot.rigs.len() as isize;
        if n > 0 {
            self.rig = (self.rig as isize + by).rem_euclid(n) as usize;
            self.sel_name = None;
        }
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        if !matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) || self.help {
            return;
        }
        let hit = |r: &Rect| {
            m.column >= r.x && m.column < r.x + r.width && m.row >= r.y && m.row < r.y + r.height
        };
        if let Some((_, i)) = self.tabs.iter().find(|(r, _)| hit(r)).copied() {
            self.rig = i;
            self.sel_name = None;
            return;
        }
        if let Some(plan) = self.plan
            && hit(&plan.image)
        {
            let (lx, ly) = plan.cell_to_logical(m.column, m.row);
            let n = self.scene().map_or(0, |s| s.seats.len());
            if let Some(i) = scene::slot_at(plan.logical.0 as i32, n, lx, ly) {
                self.select_index(i);
            }
        }
    }

    // ---- drawing ----------------------------------------------------------------------------------

    pub fn draw(&mut self, f: &mut UiFrame) {
        let a = layout(f.area());
        self.draw_tabs(f, a.tabs);
        self.draw_pane(f, &a);
        self.draw_side(f, a.side);
        self.draw_footer(f, a.footer);
        if self.help {
            self.draw_help(f);
        }
    }

    fn draw_tabs(&mut self, f: &mut UiFrame, r: Rect) {
        self.tabs.clear();
        if r.height == 0 || r.width == 0 {
            return;
        }
        let mut x = r.x;
        let mut spans: Vec<Span> = vec![Span::styled(
            " critters ",
            Style::default().fg(cream()).add_modifier(Modifier::BOLD),
        )];
        x += 10;
        if self.snapshot.rigs.is_empty() {
            spans.push(Span::styled(
                if self.snapshot.asleep {
                    "daemon asleep"
                } else {
                    "no rigs yet"
                },
                style_dim(),
            ));
        }
        for (i, rig) in self.snapshot.rigs.iter().enumerate() {
            let label = format!(" {} ", rig.name);
            let w = label.chars().count() as u16;
            let on = i == self.rig;
            spans.push(Span::styled(
                label,
                if on {
                    Style::default().fg(Color::Black).bg(cream())
                } else {
                    style_dim()
                },
            ));
            self.tabs
                .push((Rect::new(x, r.y, w.min(r.right().saturating_sub(x)), 1), i));
            x += w;
        }
        f.render_widget(Paragraph::new(Line::from(spans)), r);
    }

    fn draw_pane(&mut self, f: &mut UiFrame, a: &Areas) {
        if a.pane.width == 0 || a.pane.height == 0 {
            return;
        }
        let title = match self.scene() {
            Some(s) => format!(" {} ", s.name),
            None => " ".into(),
        };
        f.render_widget(
            Block::bordered().title(title).border_style(style_dim()),
            a.pane,
        );
        if a.image.width == 0 || a.image.height == 0 {
            return;
        }
        self.prepare(a.image);
        let plan = self.plan.expect("prepare sets the plan");
        match self.proto {
            Protocol::HalfBlocks => {
                if let Some(c) = &self.canvas {
                    let truecolor = self.truecolor;
                    let buf = f.buffer_mut();
                    paint_half_blocks(buf, a.image, c, truecolor);
                }
            }
            _ => f.render_widget(Clear, a.image),
        }
        self.draw_labels(f, &plan);
        if self.snapshot.asleep || self.scene().is_none_or(|s| s.seats.is_empty()) {
            let msg = if self.snapshot.asleep {
                "daemon asleep: no rig answers (is `rig` installed and its daemon running?)"
            } else {
                "no seats yet"
            };
            let w = (msg.chars().count() as u16).min(a.image.width);
            let r = Rect::new(
                a.image.x + (a.image.width - w) / 2,
                a.image.y + a.image.height.saturating_sub(2),
                w,
                1,
            );
            f.render_widget(Paragraph::new(msg).style(Style::default().fg(cream())), r);
        }
    }

    fn draw_labels(&self, f: &mut UiFrame, plan: &Plan) {
        let views = self.views();
        let n = views.len();
        let sel = self.selected();
        let buf = f.buffer_mut();
        for (i, v) in views.iter().enumerate() {
            let (sx, sy) = scene::slot_origin(plan.logical.0 as i32, n, i);
            let m = mood(&v.seat.lifecycle, &v.seat.activity);
            let mut text = v.seat.member.clone();
            if v.seat.pending > 0 {
                text = format!("{text} [{}]", v.seat.pending);
            }
            let max = ((SLOT_W as f32 * plan.zoom as f32) / plan.cell().0) as usize;
            let max = max.max(4);
            if text.chars().count() > max {
                text = text.chars().take(max.saturating_sub(1)).collect::<String>() + "~";
            }
            let (col, row) = plan.logical_to_cell(sx + SLOT_W / 2, sy + SLOT_H - 5);
            let start = col - text.chars().count() as i32 / 2;
            let style = match (sel == Some(i), m) {
                (true, _) if self.proto == Protocol::HalfBlocks => Style::default()
                    .fg(Color::Black)
                    .bg(cream())
                    .add_modifier(Modifier::BOLD),
                (true, _) => Style::default()
                    .fg(Color::Rgb(255, 250, 230))
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                (_, Mood::Ghost) => Style::default().fg(Color::DarkGray),
                (_, Mood::Alert) => Style::default().fg(Color::Rgb(240, 110, 110)),
                _ => Style::default().fg(cream()),
            };
            for (k, ch) in text.chars().enumerate() {
                let x = start + k as i32;
                if x < plan.image.x as i32
                    || x >= plan.image.right() as i32
                    || row < plan.image.y as i32
                    || row >= plan.image.bottom() as i32
                {
                    continue;
                }
                if let Some(cell) = buf.cell_mut((x as u16, row as u16)) {
                    if self.proto == Protocol::HalfBlocks {
                        // keep the picture's color behind the letter
                        let bg = cell.bg;
                        cell.set_char(ch).set_style(style);
                        if sel != Some(i) {
                            cell.set_bg(bg);
                        }
                    } else {
                        cell.set_char(ch).set_style(style);
                    }
                }
            }
        }
    }

    fn draw_side(&self, f: &mut UiFrame, r: Rect) {
        if r.width < 10 || r.height < 4 {
            return;
        }
        let block = Block::bordered().title(" seat ").border_style(style_dim());
        let inner = block.inner(r);
        f.render_widget(block, r);
        let lines = match (self.scene(), self.selected()) {
            (Some(s), Some(i)) => {
                let seat = &s.seats[i];
                let species = self.animals.species(&seat.pod, &seat.member);
                seat_details(seat, species, &s.edges, self.received.elapsed().as_secs())
            }
            _ => vec![Line::styled("nothing selected", style_dim())],
        };
        f.render_widget(
            Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
            inner,
        );
    }

    fn draw_footer(&self, f: &mut UiFrame, r: Rect) {
        if r.height == 0 {
            return;
        }
        let mut spans = vec![Span::styled(
            "tab rig  \u{2190}\u{2191}\u{2192}\u{2193} / click seat  +/- zoom  p pause  ? help  q quit",
            style_dim(),
        )];
        if self.paused {
            spans.push(Span::styled(
                "   PAUSED",
                Style::default().fg(Color::Yellow),
            ));
        }
        if self.demo {
            spans.push(Span::styled("   demo", Style::default().fg(Color::Cyan)));
        }
        if let Some(n) = &self.note {
            spans.push(Span::styled(
                format!("   {n}"),
                Style::default().fg(Color::Yellow),
            ));
        }
        if let Some(p) = self.problems.first() {
            spans.push(Span::styled(
                format!("   animals.toml: {p}"),
                Style::default().fg(Color::Red),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), r);
    }

    fn draw_help(&self, f: &mut UiFrame) {
        let area = f.area();
        let lines = help_lines();
        let (w, h) = (
            64.min(area.width),
            (lines.len() as u16 + 2).min(area.height),
        );
        if w < 20 || h < 5 {
            return;
        }
        let r = Rect::new(
            area.x + (area.width - w) / 2,
            area.y + (area.height - h) / 2,
            w,
            h,
        );
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(lines).block(
                Block::bordered()
                    .title(" help (any key closes) ")
                    .border_style(Style::default().fg(cream())),
            ),
            r,
        );
    }

    // ---- the picture over the link ----------------------------------------------------------------------

    /// Send the picture for kitty/sixel after Ratatui's cells (outside its buffer, so nothing flickers).
    pub fn emit_image(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        if self.proto == Protocol::HalfBlocks {
            return Ok(());
        }
        let (Some(plan), Some(c)) = (self.plan, self.canvas.as_ref()) else {
            return Ok(());
        };
        if plan.image.width == 0 || plan.image.height == 0 || self.help {
            if self.kitty_shown && self.proto == Protocol::Kitty {
                out.write_all(&termimg::kitty_delete(KITTY_ID))?;
                self.kitty_shown = false;
            }
            return Ok(());
        }
        let rgb = termimg::rgba_to_rgb(&c.rgba);
        write!(out, "\x1b7\x1b[{};{}H", plan.image.y + 1, plan.image.x + 1)?;
        match self.proto {
            Protocol::Kitty => {
                out.write_all(&termimg::kitty_image(
                    &rgb,
                    c.w,
                    c.h,
                    plan.image.width,
                    plan.image.height,
                    KITTY_ID,
                    -1,
                ))?;
                self.kitty_shown = true;
            }
            _ => out.write_all(&termimg::sixel(&rgb, c.w, c.h))?,
        }
        out.write_all(b"\x1b8")?;
        out.flush()
    }
}

/// Fill `area` with half-block characters showing `c` (its pixel size must be cols x 2*rows).
pub fn paint_half_blocks(buf: &mut Buffer, area: Rect, c: &Canvas, truecolor: bool) {
    let color = |p: [u8; 3]| {
        if truecolor {
            Color::Rgb(p[0], p[1], p[2])
        } else {
            Color::Indexed(termimg::nearest_256(p))
        }
    };
    for cy in 0..area.height {
        for cx in 0..area.width {
            let (up, down) = termimg::half_block_pair(&c.rgba, c.w, c.h, cx as u32, cy as u32);
            if let Some(cell) = buf.cell_mut((area.x + cx, area.y + cy)) {
                cell.set_char('\u{2580}')
                    .set_fg(color(up))
                    .set_bg(color(down));
            }
        }
    }
}

pub fn seat_details(
    seat: &Seat,
    species: &str,
    edges: &[Edge],
    since_poll_secs: u64,
) -> Vec<Line<'static>> {
    let m = mood(&seat.lifecycle, &seat.activity);
    let dim = style_dim();
    let kv =
        |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<9}"), dim), Span::raw(v)]);
    let mut lines = vec![
        Line::from(Span::styled(
            seat.member.clone(),
            Style::default().fg(cream()).add_modifier(Modifier::BOLD),
        )),
        Line::styled(seat.name.clone(), dim),
        Line::raw(""),
        kv("critter", species.to_string()),
        kv("runtime", seat.runtime.clone()),
        kv("state", mood_name(m).to_string()),
        kv("daemon", format!("{} / {}", seat.lifecycle, seat.activity)),
        kv(
            "queue",
            format!("{} pending, {} assigned", seat.pending, seat.assigned),
        ),
        kv(
            "active",
            seat.idle_secs
                .map_or("unknown".into(), |s| ago(s + since_poll_secs)),
        ),
    ];
    let outgoing: Vec<&Edge> = edges.iter().filter(|e| e.from == seat.name).collect();
    let incoming: Vec<&Edge> = edges.iter().filter(|e| e.to == seat.name).collect();
    let short = |n: &str| n.split('@').next().unwrap_or(n).to_string();
    if !outgoing.is_empty() || !incoming.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("neighbors", dim));
    }
    for e in outgoing {
        lines.push(Line::from(format!(
            "\u{2192} {} ({})",
            short(&e.to),
            e.kind.replace('_', " ")
        )));
    }
    for e in incoming {
        lines.push(Line::from(format!(
            "\u{2190} {} ({})",
            short(&e.from),
            e.kind.replace('_', " ")
        )));
    }
    lines
}

pub fn help_lines() -> Vec<Line<'static>> {
    let dim = style_dim();
    let row = |a: &str, b: &str| {
        Line::from(vec![
            Span::styled(format!("{a:<22}"), Style::default().fg(cream())),
            Span::styled(b.to_string(), dim),
        ])
    };
    vec![
        Line::from(Span::styled(
            "keys",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        row("Tab / Shift-Tab", "next / previous rig"),
        row("arrows, h j k l", "select a seat (or click it)"),
        row("+ / -", "zoom the pixels"),
        row("p", "pause the animation"),
        row("? / q / Esc", "help / quit"),
        Line::raw(""),
        Line::from(Span::styled(
            "what the animals are doing",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        row("typing, sparks", "working"),
        row("curled up, zzz", "idle"),
        row("! bubble", "needs input or has failed"),
        row("little sign", "parked / waiting"),
        row("? bubble", "state unknown"),
        row("see-through ghost", "not running"),
        row("stack of crates", "queued work (count = pending)"),
        row("glitter around it", "active in the last few seconds"),
        row(
            "rope + hopping crate",
            "a handoff to a seat with queued work",
        ),
        Line::raw(""),
        Line::styled("species: ~/.config/critters/animals.toml", dim),
    ]
}

fn truecolor_supported() -> bool {
    std::env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit")
        || matches!(
            std::env::var("TERM_PROGRAM").as_deref(),
            Ok("ghostty" | "WezTerm" | "iTerm.app" | "vscode")
        )
}

fn cell_pixels() -> (f32, f32) {
    match ratatui::crossterm::terminal::window_size() {
        Ok(ws) if ws.width > 0 && ws.height > 0 && ws.columns > 0 && ws.rows > 0 => (
            ws.width as f32 / ws.columns as f32,
            ws.height as f32 / ws.rows as f32,
        ),
        _ => (14.0, 28.0),
    }
}

fn local_hour() -> f32 {
    use chrono::Timelike;
    let n = chrono::Local::now();
    n.hour() as f32 + n.minute() as f32 / 60.0
}

// ---- running ----------------------------------------------------------------------------------------------

/// Run until the user quits. The terminal is restored on every exit path, including a panic.
pub fn run(opts: Options) -> Result<()> {
    let (proto, note) = match opts.protocol {
        Some(p) => (p, None),
        None => termimg::detect(&|k| std::env::var(k).ok()),
    };
    let mut app = App::new(&opts, proto, note);
    if !opts.demo {
        app.rx = Some(crate::source::spawn_live(Duration::from_millis(1500)));
        app.snapshot = Snapshot {
            rigs: vec![],
            asleep: false,
        };
    } else {
        app.snapshot = crate::demo::snapshot(0.0);
    }
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_extras(proto);
        previous(info);
    }));
    let result = event_loop(&mut terminal, &mut app, &opts);
    let _ = restore_extras(proto);
    ratatui::restore();
    result
}

fn restore_extras(proto: Protocol) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    if proto == Protocol::Kitty {
        out.write_all(&termimg::kitty_delete(KITTY_ID))?;
    }
    execute!(out, DisableMouseCapture, ratatui::crossterm::cursor::Show)
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    opts: &Options,
) -> Result<()> {
    let interval = Duration::from_secs_f32(1.0 / opts.fps.clamp(1, 30) as f32);
    let mut next_frame = Instant::now();
    let mut last = Instant::now();
    let mut last_size = (0u16, 0u16);
    let mut hour_at = Instant::now() - Duration::from_secs(100);
    while !app.quit {
        let mut wait = next_frame.saturating_duration_since(Instant::now());
        next_frame = Instant::now().max(next_frame) + interval;
        while event::poll(wait).map_err(|e| anyhow!("terminal input failed: {e}"))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => app.on_key(k),
                Event::Mouse(m) => app.on_mouse(m),
                _ => {}
            }
            wait = Duration::ZERO;
        }
        let size = terminal.size()?;
        if (size.width, size.height) != last_size {
            last_size = (size.width, size.height);
            app.cell_px = cell_pixels();
            terminal.clear()?;
        }
        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.2);
        last = now;
        if !app.paused {
            app.t += dt;
            if app.demo {
                app.snapshot = crate::demo::snapshot(app.t);
            }
        }
        if let Some(rx) = &app.rx {
            let mut newest = None;
            while let Ok(s) = rx.try_recv() {
                newest = Some(s);
            }
            if let Some(s) = newest {
                app.set_snapshot(s);
            }
        }
        if !opts.no_tint && hour_at.elapsed() > Duration::from_secs(30) {
            app.hour = local_hour();
            hour_at = Instant::now();
        }
        terminal.draw(|f| app.draw(f))?;
        app.emit_image(terminal.backend_mut())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app(proto: Protocol) -> App {
        let opts = Options {
            demo: true,
            protocol: Some(proto),
            fps: 12,
            no_tint: true,
            zoom: None,
        };
        let mut a = App::new(&opts, proto, None);
        a.animals = Animals::default();
        a.problems.clear();
        a.snapshot = crate::demo::snapshot(3.0);
        a
    }

    fn render(a: &mut App, w: u16, h: u16) -> Buffer {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        t.backend().buffer().clone()
    }

    fn text_of(buf: &Buffer) -> String {
        let w = buf.area.width as usize;
        buf.content()
            .chunks(w.max(1))
            .map(|r| r.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_screen_shows_tabs_labels_the_picture_the_side_panel_and_the_keys() {
        let mut a = app(Protocol::HalfBlocks);
        let t = text_of(&render(&mut a, 200, 70));
        for want in [
            "critters",
            "workshop-demo",
            "studio-demo",
            "orchestrator",
            "builder",
            "qa",
            "pr-watcher",
            "reviewer",
            "critter",
            "owl",
            "runtime",
            "state",
            "queue",
            "neighbors",
            "delegates to",
            "tab rig",
            "q quit",
            "demo",
        ] {
            assert!(t.contains(want), "missing '{want}':\n{t}");
        }
        let buf = render(&mut a, 200, 70);
        let blocks = buf
            .content()
            .iter()
            .filter(|c| c.symbol() == "\u{2580}")
            .count();
        assert!(blocks > 2000, "the picture is half-blocks: {blocks}");
        assert!(a.tabs.len() == 2);
    }

    #[test]
    fn keys_switch_rigs_and_move_the_selection() {
        let mut a = app(Protocol::HalfBlocks);
        let _ = render(&mut a, 200, 70);
        let key = |a: &mut App, c: KeyCode| a.on_key(KeyEvent::new(c, KeyModifiers::NONE));
        assert_eq!(a.selected(), Some(0));
        key(&mut a, KeyCode::Right);
        assert_eq!(a.selected(), Some(1));
        key(&mut a, KeyCode::Left);
        key(&mut a, KeyCode::Left);
        assert_eq!(a.selected(), Some(4), "wraps");
        key(&mut a, KeyCode::Tab);
        assert_eq!(a.rig, 1);
        assert_eq!(a.selected(), Some(0), "a new rig starts at its first seat");
        let t = text_of(&render(&mut a, 200, 70));
        assert!(t.contains("designer") && t.contains("librarian"), "{t}");
        key(&mut a, KeyCode::BackTab);
        assert_eq!(a.rig, 0);
        key(&mut a, KeyCode::Char('p'));
        assert!(a.paused);
        key(&mut a, KeyCode::Char('+'));
        key(&mut a, KeyCode::Char('-'));
        key(&mut a, KeyCode::Char('?'));
        assert!(text_of(&render(&mut a, 200, 70)).contains("what the animals are doing"));
        key(&mut a, KeyCode::Char('x'));
        assert!(!a.help, "any key closes the help");
        assert!(!a.quit);
        key(&mut a, KeyCode::Char('q'));
        assert!(a.quit);
    }

    #[test]
    fn clicking_a_seat_or_a_tab_selects_it() {
        let mut a = app(Protocol::HalfBlocks);
        let _ = render(&mut a, 200, 70);
        let plan = a.plan.unwrap();
        let views_n = a.scene().unwrap().seats.len();
        // click the middle of the third seat's slot
        let (sx, sy) = scene::slot_origin(plan.logical.0 as i32, views_n, 2);
        let (col, row) = plan.logical_to_cell(sx + SLOT_W / 2, sy + 20);
        a.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col as u16,
            row: row as u16,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(a.selected(), Some(2));
        assert!(
            text_of(&render(&mut a, 200, 70)).contains("cat"),
            "the side panel follows the click"
        );
        // the second tab
        let tab = a.tabs[1].0;
        a.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tab.x + 1,
            row: tab.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(a.rig, 1);
    }

    #[test]
    fn a_daemon_that_is_asleep_shows_a_calm_scene() {
        let mut a = app(Protocol::HalfBlocks);
        a.snapshot = Snapshot {
            rigs: vec![],
            asleep: true,
        };
        let t = text_of(&render(&mut a, 100, 30));
        assert!(t.contains("daemon asleep"), "{t}");
        a.snapshot = Snapshot {
            rigs: vec![],
            asleep: false,
        };
        assert!(text_of(&render(&mut a, 100, 30)).contains("no seats yet"));
    }

    #[test]
    fn nothing_panics_at_any_size_with_any_protocol() {
        for proto in [Protocol::HalfBlocks, Protocol::Kitty, Protocol::Sixel] {
            let mut a = app(proto);
            for (w, h) in [
                (0, 0),
                (1, 1),
                (2, 2),
                (5, 3),
                (10, 3),
                (20, 5),
                (40, 8),
                (65, 9),
                (80, 24),
                (200, 60),
            ] {
                let _ = render(&mut a, w, h);
                a.help = true;
                let _ = render(&mut a, w, h);
                a.help = false;
            }
        }
        // many seats
        let mut a = app(Protocol::HalfBlocks);
        let seat = a.snapshot.rigs[0].seats[0].clone();
        a.snapshot.rigs[0].seats = (0..64)
            .map(|i| Seat {
                name: format!("s{i}@w"),
                member: format!("m{i}"),
                ..seat.clone()
            })
            .collect();
        let _ = render(&mut a, 160, 50);
    }

    #[test]
    fn side_panel_lists_the_facts_and_the_neighbors() {
        let a = app(Protocol::HalfBlocks);
        let sc = a.scene().unwrap();
        let builder = sc.seats.iter().find(|s| s.member == "builder").unwrap();
        let text: String = seat_details(builder, "beaver", &sc.edges, 0)
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        for want in [
            "builder",
            "beaver",
            "claude-code",
            "working",
            "pending",
            "neighbors",
            "qa (delegates to)",
            "orchestrator (delegates to)",
            "pr-watcher (can observe)",
        ] {
            assert!(text.contains(want), "missing '{want}':\n{text}");
        }
        assert_eq!(ago(5), "5s ago");
        assert_eq!(ago(125), "2m ago");
        assert_eq!(ago(7200), "2h ago");
        assert_eq!(ago(200000), "2d ago");
    }

    #[test]
    fn the_kitty_picture_is_sent_below_the_text_with_one_image_id_and_removed_when_hidden() {
        let mut a = app(Protocol::Kitty);
        let _ = render(&mut a, 200, 70);
        let mut out = Vec::new();
        a.emit_image(&mut out).unwrap();
        let s = String::from_utf8_lossy(&out).to_string();
        assert!(
            s.contains("\x1b_Ga=T,f=24,o=z,")
                && s.contains(&format!("i={KITTY_ID}"))
                && s.contains("z=-1"),
            "{}",
            &s[..120.min(s.len())]
        );
        assert!(
            s.starts_with("\x1b7\x1b[") && s.ends_with("\x1b8"),
            "the cursor is saved and restored around it"
        );
        a.help = true;
        let mut out = Vec::new();
        a.emit_image(&mut out).unwrap();
        assert_eq!(
            out,
            termimg::kitty_delete(KITTY_ID),
            "help hides the picture"
        );
        // half-blocks never write escapes outside the buffer
        let mut b = app(Protocol::HalfBlocks);
        let _ = render(&mut b, 200, 70);
        let mut out = Vec::new();
        b.emit_image(&mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn the_layout_and_plan_keep_logical_and_cell_coordinates_consistent() {
        let a = layout(Rect::new(0, 0, 200, 70));
        assert!(a.side.width == SIDE_WIDTH && a.tabs.height == 1 && a.footer.height == 1);
        let plan = plan_for(Protocol::Kitty, a.image, (14.0, 28.0), 4, 5, None);
        assert!(plan.px.0 <= 960 && plan.px.1 <= 540);
        for (lx, ly) in [(5, 20), (100, 60), (180, 100)] {
            let (c, r) = plan.logical_to_cell(lx, ly);
            let (bx, by) = plan.cell_to_logical(c as u16, r as u16);
            assert!(
                (bx - lx).abs() <= (plan.cell().0 / plan.zoom as f32).ceil() as i32 + 1,
                "x {lx} -> {c} -> {bx}"
            );
            assert!(
                (by - ly).abs() <= (plan.cell().1 / plan.zoom as f32).ceil() as i32 + 1,
                "y {ly} -> {r} -> {by}"
            );
        }
    }

    #[test]
    fn when_the_seats_do_not_fit_the_view_scrolls_to_the_selected_one() {
        let mut a = app(Protocol::HalfBlocks);
        let first = text_of(&render(&mut a, 130, 40));
        assert!(first.contains("orchestrator"), "{first}");
        assert!(
            !first.contains("reviewer"),
            "the last seat is below the view at first:\n{first}"
        );
        let last = a.snapshot.rigs[0].seats.len() - 1;
        a.select_index(last);
        let t = text_of(&render(&mut a, 130, 40));
        assert!(
            t.contains("reviewer"),
            "selecting a seat scrolls it into view:\n{t}"
        );
        assert!(a.plan.unwrap().scroll > 0);
        // clicking it still finds it
        let plan = a.plan.unwrap();
        let n = a.scene().unwrap().seats.len();
        let (sx, sy) = scene::slot_origin(plan.logical.0 as i32, n, 0);
        let (col, row) = plan.logical_to_cell(sx + SLOT_W / 2, sy + 20);
        assert!(
            row < plan.image.y as i32 || a.plan.unwrap().scroll > sy,
            "the first seat is scrolled off"
        );
        let _ = col;
    }
}
