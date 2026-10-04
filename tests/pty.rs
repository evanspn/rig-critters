//! The real binary in a real pseudo-terminal.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const BIN: &str = env!("CARGO_BIN_EXE_critters");

fn run(
    args: &[&str],
    envs: &[(&str, &str)],
    rows: u16,
    cols: u16,
    wait: Duration,
    keys: &[u8],
) -> (u32, String) {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(BIN);
    cmd.args(args);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("TMUX");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let (tx, rx) = std::sync::mpsc::channel();
    let reply = writer.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 65536];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            // a real terminal answers a cursor-position query
            if buf[..n].windows(4).any(|w| w == b"\x1b[6n") {
                let mut w = reply.lock().unwrap();
                let _ = w.write_all(b"\x1b[1;1R");
                let _ = w.flush();
            }
            let _ = tx.send(buf[..n].to_vec());
        }
    });
    std::thread::sleep(wait);
    {
        let mut w = writer.lock().unwrap();
        w.write_all(keys).unwrap();
        w.flush().unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut out = Vec::new();
    let status = loop {
        while let Ok(c) = rx.try_recv() {
            out.extend(c);
        }
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            child.kill().ok();
            panic!(
                "did not exit after the keys; output: {:?}",
                String::from_utf8_lossy(&out)
                    .chars()
                    .take(300)
                    .collect::<String>()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    std::thread::sleep(Duration::from_millis(100));
    while let Ok(c) = rx.try_recv() {
        out.extend(c);
    }
    (
        status.exit_code(),
        String::from_utf8_lossy(&out).into_owned(),
    )
}

#[test]
fn the_demo_in_half_blocks_takes_keys_exits_zero_and_gives_the_terminal_back() {
    // Tab, arrows, p, ?, any key to close help, +, -, q
    let (code, out) = run(
        &["--demo", "--protocol", "halfblocks"],
        &[],
        40,
        130,
        Duration::from_millis(1500),
        b"\t\x1b[C\x1b[Bp?x+-q",
    );
    assert_eq!(code, 0, "{out:?}");
    assert!(
        out.contains("\x1b[?1049h") && out.contains("\x1b[?1049l"),
        "alternate screen entered and left"
    );
    assert!(
        out.contains("\x1b[?1000h") && out.contains("\x1b[?1000l"),
        "mouse on then off"
    );
    assert!(out.contains("\x1b[?25h"), "cursor shown again");
    assert!(out.contains('\u{2580}') && out.contains("workshop-demo"));
    assert!(!out.contains("panicked"));
}

#[test]
fn kitty_mode_sends_one_image_id_below_the_text_and_deletes_it_on_exit() {
    let (code, out) = run(
        &["--demo", "--protocol", "kitty"],
        &[],
        40,
        130,
        Duration::from_millis(1500),
        b"q",
    );
    assert_eq!(code, 0, "{out:?}");
    assert!(out.contains("\x1b_Ga=T,f=24,o=z,") && out.contains("i=7777") && out.contains("z=-1"));
    let delete = out
        .rfind("\x1b_Ga=d,d=I,i=7777")
        .expect("the image is deleted on exit");
    assert!(delete < out.rfind("\x1b[?1049l").unwrap());
}

#[test]
fn with_no_rig_on_the_machine_it_shows_a_calm_sleeping_scene() {
    let (code, out) = run(
        &["--protocol", "halfblocks"],
        &[("CRITTERS_RIG_BIN", "/nonexistent/rig")],
        30,
        100,
        Duration::from_millis(2500),
        b"q",
    );
    assert_eq!(code, 0, "{out:?}");
    assert!(out.contains("daemon asleep"), "{out:?}");
}

#[test]
fn ctrl_c_quits_cleanly_and_tiny_terminals_do_not_panic() {
    let (code, out) = run(
        &["--demo", "--protocol", "halfblocks"],
        &[],
        24,
        80,
        Duration::from_millis(1000),
        &[3],
    );
    assert_eq!(code, 0, "{out:?}");
    assert!(out.contains("\x1b[?1049l"));
    for (rows, cols) in [(1, 1), (3, 10), (5, 20), (9, 40)] {
        let (code, out) = run(
            &["--demo", "--protocol", "halfblocks"],
            &[],
            rows,
            cols,
            Duration::from_millis(700),
            b"q",
        );
        assert_eq!(code, 0, "{rows}x{cols}: {out:?}");
        assert!(!out.contains("panicked"), "{rows}x{cols}");
    }
}

#[test]
fn the_sprite_sheet_and_still_commands_write_pngs() {
    let td = tempfile::tempdir().unwrap();
    let sheet = td.path().join("sheet.png");
    let o = std::process::Command::new(BIN)
        .args(["sprites", "--out"])
        .arg(&sheet)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let img = image::open(&sheet).unwrap();
    assert!(img.width() > 100 && img.height() > 100);
    let still = td.path().join("still.png");
    let o = std::process::Command::new(BIN)
        .args(["still", "--out"])
        .arg(&still)
        .output()
        .unwrap();
    assert!(o.status.success());
    assert!(image::open(&still).is_ok());
}
