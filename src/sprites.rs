//! The critters' pixel art: 16x16 palette-indexed sprites kept as plain text in `assets/sprites.txt` (a format a web page
//! can parse too).

use std::collections::HashMap;

pub const SIZE: usize = 16;
const SOURCE: &str = include_str!("../assets/sprites.txt");

#[derive(Clone, Debug)]
pub struct Sprite {
    pub name: String,
    palette: HashMap<char, [u8; 3]>,
    rows: Vec<Vec<char>>,
}

/// How to draw one pixel of a sprite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eyes {
    Open,
    Closed,
}

impl Sprite {
    /// The color at (x, y) or `None` where the sprite is transparent.
    pub fn pixel(&self, x: usize, y: usize, eyes: Eyes) -> Option<[u8; 3]> {
        let c = *self.rows.get(y)?.get(x)?;
        if c == '.' {
            return None;
        }
        let key = if eyes == Eyes::Closed && (c == 'w' || c == 'e') {
            'l'
        } else {
            c
        };
        self.palette.get(&key).copied()
    }

    pub fn palette_color(&self, key: char) -> Option<[u8; 3]> {
        self.palette.get(&key).copied()
    }
}

fn hex(s: &str) -> Result<[u8; 3], String> {
    let h = s
        .strip_prefix('#')
        .ok_or_else(|| format!("'{s}' is not #rrggbb"))?;
    if h.len() != 6 {
        return Err(format!("'{s}' is not #rrggbb"));
    }
    let n = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| format!("'{s}' is not #rrggbb"))
    };
    Ok([n(0)?, n(2)?, n(4)?])
}

/// Parse the sprite file format.
pub fn parse(text: &str) -> Result<Vec<Sprite>, String> {
    let mut out = Vec::new();
    let mut cur: Option<Sprite> = None;
    for (n, line) in text.lines().enumerate() {
        let at = |m: &str| format!("line {}: {m}", n + 1);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix("sprite ") {
            if cur.is_some() {
                return Err(at("a sprite starts before the last one ended"));
            }
            cur = Some(Sprite {
                name: name.trim().to_string(),
                palette: HashMap::new(),
                rows: Vec::new(),
            });
        } else if line == "end" {
            let s = cur.take().ok_or_else(|| at("'end' without a sprite"))?;
            if s.rows.len() != SIZE {
                return Err(at(&format!(
                    "{} has {} rows, not {SIZE}",
                    s.name,
                    s.rows.len()
                )));
            }
            out.push(s);
        } else if let Some(rest) = line.strip_prefix("pal ") {
            let s = cur.as_mut().ok_or_else(|| at("'pal' outside a sprite"))?;
            let mut parts = rest.split_whitespace();
            let key = parts
                .next()
                .and_then(|k| (k.chars().count() == 1).then(|| k.chars().next()))
                .flatten();
            let color = parts
                .next()
                .ok_or_else(|| at("pal needs a key and a color"))?;
            s.palette.insert(
                key.ok_or_else(|| at("a palette key is one character"))?,
                hex(color).map_err(|e| at(&e))?,
            );
        } else {
            let s = cur
                .as_mut()
                .ok_or_else(|| at("pixel row outside a sprite"))?;
            if line.chars().count() != SIZE {
                return Err(at(&format!(
                    "a row has {} characters, not {SIZE}",
                    line.chars().count()
                )));
            }
            for c in line.chars() {
                if c != '.' && !s.palette.contains_key(&c) {
                    return Err(at(&format!("'{c}' is not in the palette of {}", s.name)));
                }
            }
            s.rows.push(line.chars().collect());
        }
    }
    if cur.is_some() {
        return Err("the last sprite has no 'end'".into());
    }
    Ok(out)
}

/// The built-in critters.
pub fn builtin() -> Vec<Sprite> {
    parse(SOURCE).expect("assets/sprites.txt is valid")
}

pub fn find<'a>(all: &'a [Sprite], name: &str) -> Option<&'a Sprite> {
    all.iter().find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_critter_is_a_complete_16_by_16_sprite() {
        let all = builtin();
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            crate::species::ALL,
            "one sprite per species, in the species order"
        );
        for s in &all {
            let opaque = (0..SIZE)
                .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
                .filter(|&(x, y)| s.pixel(x, y, Eyes::Open).is_some())
                .count();
            assert!(
                (90..=200).contains(&opaque),
                "{} fills {opaque} of 256 pixels",
                s.name
            );
            // the eyes close: some pixels change when they do
            let changed = (0..SIZE)
                .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
                .filter(|&(x, y)| s.pixel(x, y, Eyes::Open) != s.pixel(x, y, Eyes::Closed))
                .count();
            assert!(changed >= 2, "{} has no eyes to close", s.name);
            for key in ['k', 'b', 'd', 'l'] {
                assert!(
                    s.palette_color(key).is_some(),
                    "{} lacks the '{key}' color",
                    s.name
                );
            }
        }
    }

    #[test]
    fn the_critters_look_different_from_each_other() {
        let all = builtin();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                let diff = (0..SIZE)
                    .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
                    .filter(|&(x, y)| a.pixel(x, y, Eyes::Open) != b.pixel(x, y, Eyes::Open))
                    .count();
                assert!(
                    diff > 50,
                    "{} and {} differ in only {diff} pixels",
                    a.name,
                    b.name
                );
            }
        }
    }

    #[test]
    fn malformed_sprite_files_are_reported_with_a_line() {
        assert!(
            parse("sprite x\npal a #112233\nend\n")
                .unwrap_err()
                .contains("0 rows")
        );
        let row = "a".repeat(16);
        let ok = format!(
            "sprite x\npal a #112233\n{}end\n",
            format!("{row}\n").repeat(16)
        );
        assert_eq!(parse(&ok).unwrap().len(), 1);
        assert!(
            parse(&ok.replace("#112233", "112233"))
                .unwrap_err()
                .contains("not #rrggbb")
        );
        assert!(
            parse(&ok.replace(&row, "aaa"))
                .unwrap_err()
                .contains("characters")
        );
        assert!(
            parse(&ok.replace("pal a", "pal b"))
                .unwrap_err()
                .contains("not in the palette")
        );
        assert!(parse("sprite x\n").unwrap_err().contains("no 'end'"));
        assert!(
            parse("pal a #112233\n")
                .unwrap_err()
                .contains("outside a sprite")
        );
    }
}
