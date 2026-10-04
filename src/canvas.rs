//! A little RGBA framebuffer the scene is painted into, then scaled up to the terminal.

use crate::sprites::{Eyes, SIZE, Sprite};

#[derive(Clone)]
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Normal,
    /// a washed-out, see-through outline: the seat is not running
    Ghost,
}

impl Canvas {
    pub fn new(w: u32, h: u32, color: [u8; 3]) -> Canvas {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
        }
        Canvas { w, h, rgba }
    }

    /// Paint one pixel; `alpha` 0..=255 blends over what is there. Off-canvas is ignored.
    pub fn put(&mut self, x: i32, y: i32, c: [u8; 3], alpha: u8) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        let a = alpha as u32;
        for (k, &ck) in c.iter().enumerate() {
            self.rgba[i + k] = ((ck as u32 * a + self.rgba[i + k] as u32 * (255 - a)) / 255) as u8;
        }
    }

    pub fn get(&self, x: i32, y: i32) -> [u8; 3] {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return [0, 0, 0];
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 3]) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.put(xx, yy, c, 255);
            }
        }
    }

    /// Draw `s` with its top-left at (x, y), optionally mirrored.
    pub fn sprite(&mut self, s: &Sprite, x: i32, y: i32, flip: bool, eyes: Eyes, look: Look) {
        for sy in 0..SIZE {
            for sx in 0..SIZE {
                let src_x = if flip { SIZE - 1 - sx } else { sx };
                let Some(c) = s.pixel(src_x, sy, eyes) else {
                    continue;
                };
                match look {
                    Look::Normal => self.put(x + sx as i32, y + sy as i32, c, 255),
                    Look::Ghost => {
                        let g = ((c[0] as u32 * 3 + c[1] as u32 * 6 + c[2] as u32) / 10) as u8;
                        let tint = [g / 2 + 70, g / 2 + 90, g / 2 + 130];
                        self.put(x + sx as i32, y + sy as i32, tint, 120);
                    }
                }
            }
        }
    }

    /// The rows `y0..y0+h` as a new canvas (clamped to the canvas).
    pub fn crop_rows(&self, y0: u32, h: u32) -> Canvas {
        let y0 = y0.min(self.h.saturating_sub(1));
        let h = h.min(self.h - y0).max(1);
        let start = (y0 * self.w * 4) as usize;
        Canvas {
            w: self.w,
            h,
            rgba: self.rgba[start..start + (h * self.w * 4) as usize].to_vec(),
        }
    }

    /// Scale up by whole numbers (nearest neighbour: crisp pixels) to exactly `w` x `h`, centred, over `bg`.
    pub fn scaled_to(&self, w: u32, h: u32, bg: [u8; 3]) -> Canvas {
        let k = (w / self.w.max(1)).min(h / self.h.max(1)).max(1);
        let mut out = Canvas::new(w, h, bg);
        let (ow, oh) = (self.w * k, self.h * k);
        let (ox, oy) = (w.saturating_sub(ow) / 2, h.saturating_sub(oh) / 2);
        for y in 0..oh.min(h) {
            for x in 0..ow.min(w) {
                let s = ((y / k * self.w + x / k) * 4) as usize;
                let d = (((y + oy) * w + x + ox) * 4) as usize;
                if d + 3 < out.rgba.len() {
                    out.rgba[d..d + 3].copy_from_slice(&self.rgba[s..s + 3]);
                }
            }
        }
        out
    }

    pub fn save_png(&self, path: &std::path::Path) -> anyhow::Result<()> {
        image::save_buffer(path, &self.rgba, self.w, self.h, image::ColorType::Rgba8)?;
        Ok(())
    }
}
