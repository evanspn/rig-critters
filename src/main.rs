use std::path::PathBuf;

use clap::{Parser, Subcommand};
use critters::canvas::Canvas;
use critters::sprites;

#[derive(Parser)]
#[command(
    name = "critters",
    version,
    about = "Your OpenRig agents as little pixel animals, working"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// Show made-up seats instead of reading `rig` (to see it without a rig)
    #[arg(long)]
    demo: bool,
    /// How the picture reaches the terminal (default: detected)
    #[arg(long, value_enum, default_value_t = ProtocolArg::Auto)]
    protocol: ProtocolArg,
    /// Frames per second (max 30)
    #[arg(long, default_value_t = 12)]
    fps: u32,
    /// Do not tint the room by the time of day
    #[arg(long)]
    no_tint: bool,
    /// Pixel zoom (default: 4 with graphics, 1 with half-blocks)
    #[arg(long)]
    zoom: Option<u32>,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum ProtocolArg {
    Auto,
    Kitty,
    Sixel,
    Halfblocks,
}

#[derive(Subcommand)]
enum Cmd {
    /// Write a PNG still of the demo scene (used for the README screenshots)
    Still {
        #[arg(long, default_value = "critters-still.png")]
        out: PathBuf,
        /// which demo rig: 0 or 1
        #[arg(long, default_value_t = 0)]
        rig: usize,
        /// seconds into the demo
        #[arg(long, default_value_t = 3.0)]
        time: f32,
        #[arg(long, default_value_t = 6)]
        scale: u32,
        /// local hour for the window and lighting (0-24)
        #[arg(long, default_value_t = 14.0)]
        hour: f32,
        /// select this seat (index)
        #[arg(long)]
        select: Option<usize>,
    },
    /// Write an animated GIF of the demo scene (used for the README)
    Gif {
        #[arg(long, default_value = "critters-demo.gif")]
        out: PathBuf,
        #[arg(long, default_value_t = 0)]
        rig: usize,
        #[arg(long, default_value_t = 4.0)]
        seconds: f32,
        #[arg(long, default_value_t = 10)]
        fps: u32,
        #[arg(long, default_value_t = 3)]
        scale: u32,
        #[arg(long, default_value_t = 14.0)]
        hour: f32,
        /// start this many seconds into the demo
        #[arg(long, default_value_t = 2.0)]
        start: f32,
    },
    /// Write a sprite sheet PNG of every critter in every state
    Sprites {
        #[arg(long, default_value = "critters-sheet.png")]
        out: PathBuf,
        #[arg(long, default_value_t = 6)]
        scale: u32,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::Sprites { out, scale }) => {
            let all = sprites::builtin();
            let sheet = critters::scene::draw_sheet(&all);
            sheet
                .scaled_to(sheet.w * scale, sheet.h * scale, [0, 0, 0])
                .save_png(&out)?;
            println!("wrote {}", out.display());
            Ok(())
        }
        Some(Cmd::Gif {
            out,
            rig,
            seconds,
            fps,
            scale,
            hour,
            start,
        }) => {
            let all = sprites::builtin();
            let animals = critters::species::Animals::default();
            let file = std::fs::File::create(&out)?;
            let (w, h) = (
                (5 * critters::scene::SLOT_W + 8) as u32,
                (critters::scene::SLOT_H + 16) as u32,
            );
            let mut enc = gif::Encoder::new(file, (w * scale) as u16, (h * scale) as u16, &[])?;
            enc.set_repeat(gif::Repeat::Infinite)?;
            let frames = (seconds * fps as f32) as u32;
            for n in 0..frames {
                let t = start + n as f32 / fps as f32;
                let snap = critters::demo::snapshot(t);
                let scene = snap
                    .rigs
                    .get(rig)
                    .ok_or_else(|| anyhow::anyhow!("the demo has rigs 0 and 1"))?;
                let views: Vec<critters::scene::SeatView> = scene
                    .seats
                    .iter()
                    .map(|s| critters::scene::SeatView {
                        seat: s,
                        species: animals.species(&s.pod, &s.member),
                    })
                    .collect();
                let mut c = Canvas::new(w, h, [0, 0, 0]);
                critters::scene::draw_rig(&mut c, &all, &views, &scene.edges, None, t, hour);
                let mut big = c.scaled_to(w * scale, h * scale, [0, 0, 0]);
                let mut frame = gif::Frame::from_rgba_speed(
                    (w * scale) as u16,
                    (h * scale) as u16,
                    &mut big.rgba,
                    10,
                );
                frame.delay = (100 / fps) as u16;
                enc.write_frame(&frame)?;
            }
            println!("wrote {} ({frames} frames)", out.display());
            Ok(())
        }
        Some(Cmd::Still {
            out,
            rig,
            time,
            scale,
            hour,
            select,
        }) => {
            let all = sprites::builtin();
            let animals = critters::species::Animals::default();
            let snap = critters::demo::snapshot(time);
            let scene = snap
                .rigs
                .get(rig)
                .ok_or_else(|| anyhow::anyhow!("the demo has rigs 0 and 1"))?;
            let views: Vec<critters::scene::SeatView> = scene
                .seats
                .iter()
                .map(|s| critters::scene::SeatView {
                    seat: s,
                    species: animals.species(&s.pod, &s.member),
                })
                .collect();
            let mut c = Canvas::new(
                (5 * critters::scene::SLOT_W + 8) as u32,
                (critters::scene::SLOT_H + 16) as u32,
                [0, 0, 0],
            );
            critters::scene::draw_rig(&mut c, &all, &views, &scene.edges, select, time, hour);
            c.scaled_to(c.w * scale, c.h * scale, [0, 0, 0])
                .save_png(&out)?;
            println!("wrote {}", out.display());
            Ok(())
        }
        None => {
            use critters::termimg::Protocol;
            let protocol = match cli.protocol {
                ProtocolArg::Auto => None,
                ProtocolArg::Kitty => Some(Protocol::Kitty),
                ProtocolArg::Sixel => Some(Protocol::Sixel),
                ProtocolArg::Halfblocks => Some(Protocol::HalfBlocks),
            };
            critters::app::run(critters::app::Options {
                demo: cli.demo,
                protocol,
                fps: cli.fps,
                no_tint: cli.no_tint,
                zoom: cli.zoom,
            })
        }
    }
}
