//! Painting a rig: a cozy little workshop with one desk per seat, an animal at each desk, and ropes between them.
//!
//! All coordinates are logical pixels (one sprite pixel = one logical pixel); the app scales the canvas up.

use crate::canvas::{Canvas, Look};
use crate::model::{Edge, Mood, Seat, mood};
use crate::species::fnv1a;
use crate::sprites::{Eyes, SIZE, Sprite, find};

pub const SLOT_W: i32 = 44;
pub const SLOT_H: i32 = 56;
/// Height of the back wall.
const WALL_H: i32 = 12;

/// How many slots fit across `w` and how many rows `n` seats need.
pub fn grid(w: i32, n: usize) -> (usize, usize) {
    let per_row = ((w / SLOT_W).max(1)) as usize;
    (per_row, n.div_ceil(per_row).max(1))
}

/// Top-left of seat `i`'s slot, centering each row.
pub fn slot_origin(w: i32, n: usize, i: usize) -> (i32, i32) {
    let (per_row, _) = grid(w, n);
    let row = i / per_row;
    let in_row = if (row + 1) * per_row <= n {
        per_row
    } else {
        n - row * per_row
    };
    let left = (w - in_row as i32 * SLOT_W) / 2;
    (
        left + (i % per_row) as i32 * SLOT_W,
        WALL_H + row as i32 * SLOT_H,
    )
}

/// The height of everything for `n` seats in a canvas `w` wide.
pub fn content_height(w: i32, n: usize) -> i32 {
    let (_, rows) = grid(w, n);
    WALL_H + rows as i32 * SLOT_H + 4
}

/// How far to scroll (in logical pixels) so seat `selected` is on screen in a viewport `vh` tall.
pub fn scroll_for(w: i32, n: usize, vh: i32, selected: Option<usize>) -> i32 {
    let total = content_height(w, n);
    if total <= vh {
        return 0;
    }
    let mid = selected.map_or(0, |i| slot_origin(w, n, i).1 + SLOT_H / 2);
    (mid - vh / 2).clamp(0, total - vh)
}

/// The slot (seat index) at logical pixel (x, y), if any.
pub fn slot_at(w: i32, n: usize, x: i32, y: i32) -> Option<usize> {
    (0..n).find(|&i| {
        let (sx, sy) = slot_origin(w, n, i);
        x >= sx && x < sx + SLOT_W && y >= sy && y < sy + SLOT_H
    })
}

/// The largest zoom (screen pixels per logical pixel), at most `want`, at which `n` seats fit a pane of `px` pixels.
pub fn fit_zoom(px: (u32, u32), n: usize, want: u32) -> u32 {
    for z in (1..=want.max(1)).rev() {
        let w = (px.0 / z) as i32;
        let h = (px.1 / z) as i32;
        let (_, rows) = grid(w, n);
        if WALL_H + rows as i32 * SLOT_H <= h {
            return z;
        }
    }
    1
}

pub struct SeatView<'a> {
    pub seat: &'a Seat,
    pub species: &'static str,
}

const WOOD: [u8; 3] = [122, 90, 60];
const WOOD_LIGHT: [u8; 3] = [150, 114, 78];
const WOOD_DARK: [u8; 3] = [84, 60, 42];
const CRATE: [u8; 3] = [196, 146, 78];
const CRATE_DARK: [u8; 3] = [132, 92, 48];
const CREAM: [u8; 3] = [244, 236, 214];

/// Sky and window colors for a local hour 0..24.
pub fn sky(hour: f32) -> ([u8; 3], bool) {
    let h = hour.rem_euclid(24.0);
    if (7.0..17.0).contains(&h) {
        ([120, 178, 226], false)
    } else if (17.0..19.5).contains(&h) {
        ([226, 138, 96], false)
    } else if (5.5..7.0).contains(&h) {
        ([232, 170, 136], false)
    } else {
        ([22, 24, 52], true)
    }
}

fn shade(c: [u8; 3], k: f32) -> [u8; 3] {
    [
        (c[0] as f32 * k).min(255.0) as u8,
        (c[1] as f32 * k).min(255.0) as u8,
        (c[2] as f32 * k).min(255.0) as u8,
    ]
}

pub fn draw_background(c: &mut Canvas, hour: f32) {
    let night = if sky(hour).1 { 0.62 } else { 1.0 };
    let (w, h) = (c.w as i32, c.h as i32);
    // back wall
    for y in 0..WALL_H.min(h) {
        c.rect(
            0,
            y,
            w,
            1,
            shade([66, 54, 78], night * (0.9 + 0.1 * y as f32 / WALL_H as f32)),
        );
    }
    c.rect(0, WALL_H - 1, w, 1, shade([44, 34, 54], night));
    // planks
    for y in WALL_H..h {
        let plank = ((y - WALL_H) / 7) % 2 == 0;
        let base = if plank { [92, 70, 58] } else { [84, 63, 52] };
        c.rect(0, y, w, 1, shade(base, night));
    }
    for y in (WALL_H..h).step_by(7) {
        c.rect(0, y, w, 1, shade([64, 46, 40], night));
    }
    // a window every so often along the wall
    let (sky_c, dark) = sky(hour);
    let mut x = 10;
    while x + 14 < w {
        c.rect(x - 1, 1, 16, 9, shade([44, 34, 54], 1.0));
        c.rect(x, 2, 14, 7, sky_c);
        if dark {
            for (sx, sy) in [(2, 3), (7, 4), (11, 3), (5, 6)] {
                c.put(x + sx, sy, [250, 244, 200], 255);
            }
            c.rect(x + 9, 3, 2, 2, [244, 238, 206]);
        } else if (7.0..17.0).contains(&hour) {
            c.rect(x + 2, 4, 5, 1, [244, 248, 255]);
            c.rect(x + 3, 3, 3, 1, [244, 248, 255]);
        }
        c.rect(x + 6, 2, 1, 7, [44, 34, 54]);
        x += 44;
    }
}

/// A dotted run of pixels along `pts`; returns nothing, the points are drawn every 3rd one skipped.
fn dotted(c: &mut Canvas, pts: &[(i32, i32)], color: [u8; 3]) {
    for (n, &(x, y)) in pts.iter().enumerate() {
        if n % 3 != 2 {
            c.put(x, y, color, 255);
        }
    }
}

/// Every pixel from `a` to `b` along a straight horizontal or vertical run (excluding `a`).
fn run(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((b.0 - a.0).signum(), (b.1 - a.1).signum());
    let n = (b.0 - a.0).abs().max((b.1 - a.1).abs());
    (1..=n).map(|k| (a.0 + dx * k, a.1 + dy * k)).collect()
}

/// The rope between two desks: down from the first, along a channel in the floor, then to the second. Each rope gets its own
/// channel so ropes between the same neighbours do not draw over each other.
pub fn rope_path(a: (i32, i32), b: (i32, i32), channel: i32) -> Vec<(i32, i32)> {
    let mut pts = vec![a];
    let corner1 = (a.0, channel);
    let corner2 = (b.0, channel);
    for target in [corner1, corner2, b] {
        let from = *pts.last().expect("starts non-empty");
        pts.extend(run(from, target));
    }
    pts
}

fn edge_color(kind: &str) -> [u8; 3] {
    match kind {
        "delegates_to" => [236, 196, 110],
        "escalates_to" => [226, 120, 110],
        "can_observe" => [130, 180, 210],
        "collaborates_with" => [150, 210, 150],
        _ => [190, 170, 220],
    }
}

fn tiny_crate(c: &mut Canvas, x: i32, y: i32) {
    c.rect(x, y, 4, 4, CRATE);
    c.rect(x, y, 4, 1, CRATE_DARK);
    c.rect(x, y + 3, 4, 1, CRATE_DARK);
    c.put(x + 1, y + 1, CRATE_DARK, 255);
    c.put(x + 2, y + 2, CRATE_DARK, 255);
}

/// Ropes between desks, with a crate hopping along while the receiving seat has work waiting.
pub fn draw_edges(c: &mut Canvas, w: i32, views: &[SeatView], edges: &[Edge], t: f32) {
    let n = views.len();
    let index = |name: &str| views.iter().position(|v| v.seat.name == name);
    for (k, e) in edges.iter().enumerate() {
        let (Some(ia), Some(ib)) = (index(&e.from), index(&e.to)) else {
            continue;
        };
        let (ax, ay) = slot_origin(w, n, ia);
        let (bx, by) = slot_origin(w, n, ib);
        let channel = ay + 46 + (k as i32 % 4) * 2;
        // start under the first desk; end under the second desk, or above its head when it is on a lower row
        let start = (ax + SLOT_W / 2, ay + 44);
        let end = if by > ay {
            (bx + SLOT_W / 2, by + 9)
        } else {
            (bx + SLOT_W / 2, by + 44)
        };
        let channel = if by > ay {
            ay + SLOT_H - 4 + (k as i32 % 3) * 2
        } else {
            channel
        };
        let pts = rope_path(start, end, channel);
        dotted(c, &pts, edge_color(&e.kind));
        if views[ib].seat.pending > 0 && pts.len() > 4 {
            let phase = (t * 0.45 + (fnv1a(e.from.as_bytes()) % 100) as f32 / 100.0).fract();
            let i = ((pts.len() - 1) as f32 * phase) as usize;
            let (x, y) = pts[i.min(pts.len() - 1)];
            tiny_crate(c, x - 2, y - 5 - ((t * 8.0) as i32 % 2));
        }
    }
}

const FONT_Z: [&str; 4] = ["###", "..#", ".#.", "###"];
const FONT_BANG: [&str; 7] = [
    "..#..", "..#..", "..#..", "..#..", ".....", "..#..", "..#..",
];
const FONT_Q: [&str; 7] = [
    ".###.", "#...#", "....#", "..##.", "..#..", ".....", "..#..",
];

fn glyph(c: &mut Canvas, rows: &[&str], x: i32, y: i32, color: [u8; 3]) {
    for (dy, row) in rows.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            if ch == '#' {
                c.put(x + dx as i32, y + dy as i32, color, 255);
            }
        }
    }
}

fn bubble(c: &mut Canvas, x: i32, y: i32, rows: &[&str], ink: [u8; 3], fill: [u8; 3]) {
    // a rounded 9x11 bubble with a tail
    c.rect(x + 1, y, 7, 11, fill);
    c.rect(x, y + 1, 9, 9, fill);
    c.put(x + 2, y + 11, fill, 255);
    c.put(x + 1, y + 12, fill, 255);
    glyph(c, rows, x + 2, y + 2, ink);
}

fn desk(c: &mut Canvas, cx: i32, sy: i32, alpha: u8) {
    let top = sy + 30;
    let draw = |c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, col: [u8; 3]| {
        for yy in y..y + h {
            for xx in x..x + w {
                c.put(xx, yy, col, alpha);
            }
        }
    };
    draw(c, cx - 15, top, 30, 3, WOOD_LIGHT);
    draw(c, cx - 15, top + 3, 30, 2, WOOD);
    draw(c, cx - 13, top + 5, 3, 8, WOOD_DARK);
    draw(c, cx + 10, top + 5, 3, 8, WOOD_DARK);
}

fn keyboard(c: &mut Canvas, cx: i32, sy: i32, working: bool, frame: u32, seed: u32) {
    let (x, y) = (cx - 6, sy + 28);
    c.rect(x, y, 12, 2, [58, 56, 74]);
    c.rect(x, y + 2, 12, 1, [40, 38, 54]);
    if working {
        for k in 0..12 {
            if (k as u32 + frame * 5 + seed).is_multiple_of(4) {
                c.put(x + k, y, [190, 232, 255], 255);
            }
        }
    } else {
        for k in (0..12).step_by(3) {
            c.put(x + k, y, [96, 96, 120], 255);
        }
    }
}

fn crates(c: &mut Canvas, cx: i32, sy: i32, pending: u32) {
    if pending == 0 {
        return;
    }
    // a pyramid beside the desk: 3 on the bottom, 2, then 1
    let shown = pending.min(6);
    let base = (cx + 17, sy + 43);
    let mut placed = 0;
    for (row, count) in [(0, 3), (1, 2), (2, 1)] {
        for k in 0..count {
            if placed >= shown {
                break;
            }
            let x = base.0 + k * 5 + row * 2 + 1;
            let y = base.1 - 5 * (row + 1) + 5;
            tiny_crate(c, x - 1, y - 4);
            placed += 1;
        }
    }
    if pending > 6 {
        // a "+" tag for more than fit
        c.rect(base.0 + 14, sy + 36, 3, 1, CREAM);
        c.rect(base.0 + 15, sy + 35, 1, 3, CREAM);
    }
}

fn sign(c: &mut Canvas, cx: i32, sy: i32) {
    let (x, y) = (cx - 27, sy + 30);
    c.rect(x + 3, y + 6, 2, 8, WOOD_DARK);
    c.rect(x, y, 9, 7, WOOD_LIGHT);
    c.rect(x, y, 9, 1, WOOD);
    c.rect(x, y + 6, 9, 1, WOOD);
    c.rect(x + 2, y + 2, 1, 3, CREAM);
    c.rect(x + 5, y + 2, 1, 3, CREAM);
}

/// Paint one seat. `frame` ticks at ~6 per second; `t` is seconds.
#[allow(clippy::too_many_arguments)]
pub fn draw_seat(
    c: &mut Canvas,
    sprites: &[Sprite],
    view: &SeatView,
    origin: (i32, i32),
    t: f32,
    selected: bool,
) {
    let frame = (t * 6.0) as u32;
    let seed = fnv1a(view.seat.name.as_bytes());
    let m = mood(&view.seat.lifecycle, &view.seat.activity);
    let (sx, sy) = origin;
    let cx = sx + SLOT_W / 2;
    let Some(sprite) = find(sprites, view.species).or_else(|| sprites.first()) else {
        return;
    };

    if m == Mood::Ghost {
        desk(c, cx, sy, 70);
        let float = ((t * 1.6 + seed as f32).sin() * 1.5).round() as i32;
        c.sprite(
            sprite,
            cx - 8,
            sy + 10 + float,
            false,
            Eyes::Open,
            Look::Ghost,
        );
    } else {
        desk(c, cx, sy, 255);
        // the critter
        let (mut ax, mut ay) = (cx - 8, sy + 14);
        let mut eyes = Eyes::Open;
        match m {
            Mood::Working => ay -= (frame % 2) as i32, // a nod with every keystroke
            Mood::Napping => {
                eyes = Eyes::Closed;
                ay += 1 + ((t * 0.8 + seed as f32).sin() > 0.0) as i32; // slow breathing
            }
            Mood::Alert => ax += if frame.is_multiple_of(2) { 0 } else { 1 },
            Mood::Parked => {
                eyes = if ((t * 0.5) as u32).is_multiple_of(6) {
                    Eyes::Closed
                } else {
                    Eyes::Open
                }
            }
            Mood::Unknown | Mood::Ghost => {}
        }
        // an occasional blink for the awake ones
        if matches!(m, Mood::Working | Mood::Alert | Mood::Unknown)
            && ((t * 2.0 + (seed % 7) as f32) as u32).is_multiple_of(9)
            && frame.is_multiple_of(3)
        {
            eyes = Eyes::Closed;
        }
        c.sprite(sprite, ax, ay, false, eyes, Look::Normal);
        keyboard(c, cx, sy, m == Mood::Working, frame, seed % 7);
        if m == Mood::Working {
            // paws on the keys, alternating, in the critter's own colors
            let paw = sprite
                .palette_color('d')
                .or_else(|| sprite.palette_color('b'))
                .unwrap_or([200, 160, 120]);
            let (lx, rx) = (cx - 5, cx + 3);
            let (ly, ry) = if frame.is_multiple_of(2) {
                (sy + 26, sy + 27)
            } else {
                (sy + 27, sy + 26)
            };
            c.rect(lx, ly, 3, 2, paw);
            c.rect(rx, ry, 3, 2, paw);
            // sparks
            for k in 0..3u32 {
                let phase = (t * 4.0 + k as f32 * 0.37 + (seed % 5) as f32) % 1.0;
                let px = cx - 7 + ((k * 7 + seed) % 15) as i32;
                let py = sy + 26 - (phase * 9.0) as i32;
                let a = (255.0 * (1.0 - phase)) as u8;
                c.put(px, py, [255, 226, 120], a);
            }
        }
        match m {
            Mood::Napping => {
                for k in 0..3 {
                    let phase = (t * 0.7 + k as f32 / 3.0) % 1.0;
                    let a = (255.0 * (1.0 - phase * 0.8)) as u8;
                    let mut z = Canvas::new(3, 4, [0, 0, 0]);
                    glyph(&mut z, &FONT_Z, 0, 0, [220, 226, 255]);
                    let (zx, zy) = (
                        cx + 6 + (phase * 6.0) as i32 + k * 2,
                        sy + 12 - (phase * 12.0) as i32,
                    );
                    for yy in 0..4 {
                        for xx in 0..3 {
                            if z.get(xx, yy) != [0, 0, 0] {
                                c.put(zx + xx, zy + yy, [220, 226, 255], a);
                            }
                        }
                    }
                }
            }
            Mood::Alert => {
                let bounce = if frame.is_multiple_of(2) { 0 } else { -1 };
                bubble(c, cx + 5, sy - 1 + bounce, &FONT_BANG, [210, 40, 50], CREAM);
            }
            Mood::Unknown => bubble(c, cx + 5, sy + 1, &FONT_Q, [90, 100, 150], [206, 214, 236]),
            Mood::Parked => sign(c, cx, sy),
            _ => {}
        }
        crates(c, cx, sy, view.seat.pending);
        // recent activity: sparkles circling the critter
        if view.seat.idle_secs.is_some_and(|s| s < 8) && m != Mood::Ghost {
            for k in 0..4u32 {
                let ang = (t * 2.4 + k as f32 * 1.57 + seed as f32) % std::f32::consts::TAU;
                let (px, py) = (
                    cx + (ang.cos() * 12.0) as i32,
                    sy + 20 + (ang.sin() * 9.0) as i32,
                );
                if !(frame + k).is_multiple_of(3) {
                    c.put(px, py, [255, 244, 190], 230);
                }
            }
        }
    }
    if selected {
        let blink = ((t * 2.0) as u32).is_multiple_of(2);
        let col = if blink { CREAM } else { [196, 186, 160] };
        let (x0, y0, x1, y1) = (sx + 1, sy + 1, sx + SLOT_W - 2, sy + SLOT_H - 2);
        for (cx_, cy_, dx, dy) in [
            (x0, y0, 1, 1),
            (x1, y0, -1, 1),
            (x0, y1, 1, -1),
            (x1, y1, -1, -1),
        ] {
            for k in 0..5 {
                c.put(cx_ + dx * k, cy_, col, 255);
                c.put(cx_, cy_ + dy * k, col, 255);
            }
        }
    }
}

/// The calm scene shown when the daemon cannot be reached: a moonlit empty desk and a sleepy "z".
pub fn draw_asleep(c: &mut Canvas, sprites: &[Sprite], t: f32) {
    draw_background(c, 23.0);
    let cx = c.w as i32 / 2;
    let sy = (c.h as i32 - SLOT_H) / 2 + 6;
    desk(c, cx, sy, 255);
    if let Some(s) = find(sprites, "owl") {
        let breathe = ((t * 0.8).sin() > 0.0) as i32;
        c.sprite(
            s,
            cx - 8,
            sy + 15 + breathe,
            false,
            Eyes::Closed,
            Look::Normal,
        );
    }
    keyboard(c, cx, sy, false, 0, 0);
    for k in 0..3 {
        let phase = (t * 0.5 + k as f32 / 3.0) % 1.0;
        let a = (255.0 * (1.0 - phase)) as u8;
        for (dy, row) in FONT_Z.iter().enumerate() {
            for (dx, ch) in row.chars().enumerate() {
                if ch == '#' {
                    c.put(
                        cx + 7 + (phase * 8.0) as i32 + k * 2 + dx as i32,
                        sy + 10 - (phase * 12.0) as i32 + dy as i32,
                        [220, 226, 255],
                        a,
                    );
                }
            }
        }
    }
}

/// Everything for one rig.
pub fn draw_rig(
    c: &mut Canvas,
    sprites: &[Sprite],
    views: &[SeatView],
    edges: &[Edge],
    selected: Option<usize>,
    t: f32,
    hour: f32,
) {
    draw_background(c, hour);
    let w = c.w as i32;
    draw_edges(c, w, views, edges, t);
    for (i, v) in views.iter().enumerate() {
        draw_seat(
            c,
            sprites,
            v,
            slot_origin(w, views.len(), i),
            t,
            selected == Some(i),
        );
    }
}

pub const SPRITE_SIZE: i32 = SIZE as i32;

/// Every critter in every mood, for judging the art: one row per species; columns are working (two frames), napping, alert,
/// parked, unknown, ghost.
pub fn draw_sheet(sprites: &[Sprite]) -> Canvas {
    let moods: [(&str, &str, f32); 7] = [
        ("running", "running", 0.0),
        ("running", "running", 0.17),
        ("running", "idle", 0.0),
        ("running", "needs_input", 0.0),
        ("running", "blocked", 0.0),
        ("running", "unknown", 0.0),
        ("stopped", "unknown", 0.0),
    ];
    let (cols, rows) = (moods.len() as i32, sprites.len() as i32);
    let mut c = Canvas::new(
        (cols * SLOT_W) as u32,
        (rows * (SLOT_H - 8) + 4) as u32,
        [0, 0, 0],
    );
    draw_background(&mut c, 14.0);
    for (r, sp) in sprites.iter().enumerate() {
        for (k, (life, act, t)) in moods.iter().enumerate() {
            let seat = Seat {
                name: format!("{}@sheet", sp.name),
                rig: "sheet".into(),
                pod: "p".into(),
                member: sp.name.clone(),
                runtime: "claude-code".into(),
                activity: (*act).into(),
                lifecycle: (*life).into(),
                assigned: u32::from(k == 1),
                pending: if k == 1 { 3 } else { 0 },
                idle_secs: if k == 0 { Some(1) } else { Some(500) },
            };
            let species = sprites
                .iter()
                .find(|s| s.name == sp.name)
                .map_or("owl", |_| "owl");
            let _ = species;
            let view = SeatView {
                seat: &seat,
                species: crate::species::ALL
                    .iter()
                    .copied()
                    .find(|a| *a == sp.name)
                    .unwrap_or("owl"),
            };
            draw_seat(
                &mut c,
                sprites,
                &view,
                (k as i32 * SLOT_W, r as i32 * (SLOT_H - 8) - 6),
                1.0 + *t,
                false,
            );
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Seat, Snapshot};

    fn seat(i: usize, pending: u32, life: &str, act: &str) -> Seat {
        Seat {
            name: format!("p{i}-m{i}@r"),
            rig: "r".into(),
            pod: format!("p{i}"),
            member: format!("m{i}"),
            runtime: "claude-code".into(),
            activity: act.into(),
            lifecycle: life.into(),
            assigned: pending,
            pending,
            idle_secs: Some(1),
        }
    }

    #[test]
    fn slots_are_centered_rows_that_never_overlap() {
        for (w, n) in [(240, 5), (160, 7), (44, 3), (10, 2), (400, 1), (176, 8)] {
            let origins: Vec<_> = (0..n).map(|i| slot_origin(w, n, i)).collect();
            for (i, a) in origins.iter().enumerate() {
                for b in &origins[i + 1..] {
                    assert!(
                        a.0 + SLOT_W <= b.0
                            || b.0 + SLOT_W <= a.0
                            || a.1 + SLOT_H <= b.1
                            || b.1 + SLOT_H <= a.1,
                        "{a:?} overlaps {b:?} (w {w}, n {n})"
                    );
                }
            }
            // every slot finds itself again
            for (i, o) in origins.iter().enumerate() {
                assert_eq!(slot_at(w, n, o.0 + 3, o.1 + 3), Some(i));
            }
        }
        assert_eq!(slot_at(240, 5, 2, 2), None, "the wall is not a seat");
    }

    #[test]
    fn the_zoom_shrinks_until_every_seat_fits() {
        assert_eq!(fit_zoom((960, 540), 5, 4), 4);
        assert!(
            fit_zoom((960, 540), 40, 4) < 4,
            "many seats need smaller pixels"
        );
        assert_eq!(fit_zoom((10, 10), 3, 4), 1, "never below 1");
    }

    #[test]
    fn a_rope_is_one_connected_run_of_pixels() {
        let p = rope_path((10, 10), (60, 14), 20);
        assert_eq!(p[0], (10, 10));
        assert_eq!(*p.last().unwrap(), (60, 14));
        for w in p.windows(2) {
            assert_eq!(
                (w[1].0 - w[0].0).abs() + (w[1].1 - w[0].1).abs(),
                1,
                "{w:?}"
            );
        }
    }

    #[test]
    fn every_mood_paints_something_and_the_scene_survives_odd_sizes() {
        let sprites = crate::sprites::builtin();
        let states = [
            ("running", "running"),
            ("running", "idle"),
            ("running", "needs_input"),
            ("running", "blocked"),
            ("stopped", "unknown"),
            ("running", "weird"),
        ];
        let seats: Vec<Seat> = states
            .iter()
            .enumerate()
            .map(|(i, (l, a))| seat(i, i as u32 * 2, l, a))
            .collect();
        let views: Vec<SeatView> = seats
            .iter()
            .map(|s| SeatView {
                seat: s,
                species: "owl",
            })
            .collect();
        for (w, h) in [(240u32, 135u32), (100, 60), (44, 70), (10, 10), (1, 1)] {
            let mut c = Canvas::new(w, h, [0, 0, 0]);
            draw_rig(&mut c, &sprites, &views, &[], Some(2), 3.3, 14.0);
            draw_rig(&mut c, &sprites, &[], &[], None, 0.0, 3.0);
        }
        // each mood looks different from a plain background at the same moment
        let mut plain = Canvas::new(60, 70, [0, 0, 0]);
        draw_background(&mut plain, 14.0);
        let mut pics = Vec::new();
        for (l, a) in states {
            let s = seat(0, 0, l, a);
            let mut c = plain.clone();
            draw_seat(
                &mut c,
                &sprites,
                &SeatView {
                    seat: &s,
                    species: "cat",
                },
                (8, 12),
                3.3,
                false,
            );
            assert_ne!(c.rgba, plain.rgba, "{l}/{a} draws nothing");
            pics.push(c.rgba);
        }
        for i in 0..pics.len() {
            for j in i + 1..pics.len() {
                assert_ne!(pics[i], pics[j], "moods {i} and {j} look identical");
            }
        }
        // time moves the animation, and the same time repeats exactly
        let s = seat(0, 0, "running", "running");
        let draw = |t: f32| {
            let mut c = plain.clone();
            draw_seat(
                &mut c,
                &sprites,
                &SeatView {
                    seat: &s,
                    species: "beaver",
                },
                (8, 12),
                t,
                false,
            );
            c.rgba
        };
        assert_ne!(draw(1.0), draw(1.2));
        assert_eq!(draw(1.0), draw(1.0));
    }

    #[test]
    fn crates_stand_for_pending_work() {
        let sprites = crate::sprites::builtin();
        let mut plain = Canvas::new(60, 70, [0, 0, 0]);
        draw_background(&mut plain, 14.0);
        let count = |pending: u32| {
            let s = seat(0, pending, "running", "idle");
            let mut c = plain.clone();
            draw_seat(
                &mut c,
                &sprites,
                &SeatView {
                    seat: &s,
                    species: "cat",
                },
                (8, 12),
                0.0,
                false,
            );
            c.rgba.chunks(4).filter(|p| p[..3] == CRATE).count()
        };
        assert_eq!(count(0), 0);
        assert!(count(1) > 0 && count(3) > count(1) && count(6) > count(3));
        let _ = Snapshot::default();
    }

    #[test]
    fn the_sky_follows_the_clock() {
        assert!(!sky(12.0).1);
        assert!(sky(23.0).1 && sky(3.0).1);
        assert_ne!(sky(12.0).0, sky(18.0).0);
    }
}
