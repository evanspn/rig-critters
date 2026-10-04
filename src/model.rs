//! What the tool knows about the agents, and how it learns it safely.
//!
//! Everything comes from `rig ps --nodes -A --json` and `rig spec show <rigId> --json` and is rebuilt FIELD BY FIELD from a
//! short allowlist (names, state, runtime, counts, timestamps, edges). Whatever else the daemon sends (paths, tokens, titles,
//! queue text) is dropped by construction. Queue rows and transcripts are never read.

pub use serde_json::Value;

pub const MAX_SEATS: usize = 64;
pub const MAX_RIGS: usize = 16;
pub const MAX_EDGES: usize = 96;
const MAX_COUNT: u32 = 9999;
/// rigspec-schema VALID_EDGE_KINDS: anything else is dropped, not drawn.
pub const EDGE_KINDS: [&str; 5] = [
    "delegates_to",
    "spawned_by",
    "can_observe",
    "collaborates_with",
    "escalates_to",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    /// `pod-member@rig`
    pub name: String,
    pub rig: String,
    /// the part of the logical id before the dot
    pub pod: String,
    /// the part after the dot: what the seat is for ("builder", "qa", ...)
    pub member: String,
    pub runtime: String,
    pub activity: String,
    pub lifecycle: String,
    pub assigned: u32,
    pub pending: u32,
    /// seconds since the last activity, when the daemon said
    pub idle_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RigScene {
    pub name: String,
    pub seats: Vec<Seat>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub rigs: Vec<RigScene>,
    /// the daemon (or `rig` itself) could not be reached
    pub asleep: bool,
}

/// What a seat looks like it is doing; drives the animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Working,
    Napping,
    /// needs input or has failed: an "!" bubble
    Alert,
    /// waiting on something: a little sign
    Parked,
    /// not running
    Ghost,
    Unknown,
}

/// Map the daemon's lifecycle and activity strings to a mood. Anything unrecognised is `Unknown` (a question mark), never a guess.
pub fn mood(lifecycle: &str, activity: &str) -> Mood {
    if lifecycle != "running" {
        return Mood::Ghost;
    }
    match activity {
        "running" | "working" | "busy" | "active" => Mood::Working,
        "idle" => Mood::Napping,
        "needs_input" | "attention" | "error" | "failed" | "stuck" => Mood::Alert,
        "blocked" | "parked" | "waiting" => Mood::Parked,
        _ => Mood::Unknown,
    }
}

fn name_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.@:/+-".contains(c))
}

fn text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| name_ok(s)).map(str::to_string)
}

fn count(v: &Value) -> u32 {
    match v {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
            .map_or(0, |n| n.min(MAX_COUNT as u64) as u32),
        _ => 0,
    }
}

fn iso_ok(s: &str) -> bool {
    // 2026-10-04T12:27:11.411Z
    let b = s.as_bytes();
    s.len() >= 20
        && s.len() <= 32
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && s.ends_with('Z')
        && s.chars().all(|c| c.is_ascii_digit() || "-T:.Z".contains(c))
}

/// Seconds since the Unix epoch of an ISO-8601 UTC timestamp.
pub fn epoch_secs(iso: &str) -> Option<i64> {
    if !iso_ok(iso) {
        return None;
    }
    let n = |a: usize, b: usize| iso.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (
        n(0, 4)?,
        n(5, 7)?,
        n(8, 10)?,
        n(11, 13)?,
        n(14, 16)?,
        n(17, 19)?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    // days from civil (Howard Hinnant)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

/// One daemon node -> a seat, or `None` to drop it. `now` is the Unix time used to turn the timestamp into an age.
pub fn sanitize_seat(entry: &Value, now: i64) -> Option<Seat> {
    let obj = entry.as_object()?;
    let logical = obj.get("logicalId").and_then(text);
    let name = obj
        .get("canonicalSessionName")
        .and_then(text)
        .or_else(|| logical.clone())?;
    let (pod, member) = match logical.as_deref().and_then(|l| l.split_once('.')) {
        Some((p, m)) => (p.to_string(), m.to_string()),
        None => (
            String::new(),
            logical
                .clone()
                .unwrap_or_else(|| name.split('@').next().unwrap_or(&name).to_string()),
        ),
    };
    let rig = obj
        .get("rigName")
        .and_then(text)
        .unwrap_or_else(|| name.split('@').nth(1).unwrap_or("rig").to_string());
    let activity = obj
        .get("agentActivity")
        .and_then(|a| a.get("state"))
        .and_then(text)
        .unwrap_or_else(|| "unknown".into());
    let idle_secs = obj
        .get("lastActivity")
        .and_then(Value::as_str)
        .and_then(epoch_secs)
        .map(|t| (now - t).max(0) as u64);
    Some(Seat {
        name,
        rig,
        pod,
        member,
        runtime: obj
            .get("runtime")
            .and_then(text)
            .unwrap_or_else(|| "unknown".into()),
        activity,
        lifecycle: obj
            .get("lifecycleState")
            .and_then(text)
            .unwrap_or_else(|| "unknown".into()),
        assigned: obj.get("assignedWorkCount").map_or(0, count),
        pending: obj.get("pendingWorkCount").map_or(0, count),
        idle_secs,
    })
}

fn entries(payload: &Value) -> &[Value] {
    // `rig ps --nodes -A --json` is a bare list; a single-rig form wraps it in {"entries": [...]}
    let list = payload.get("entries").unwrap_or(payload);
    list.as_array().map_or(&[], |a| a.as_slice())
}

/// Group the daemon payload into one scene per rig (edges are added separately).
pub fn build_scenes(payload: &Value, now: i64) -> Vec<RigScene> {
    let mut scenes: Vec<RigScene> = Vec::new();
    let mut total = 0;
    for e in entries(payload) {
        if total >= MAX_SEATS {
            break;
        }
        let Some(seat) = sanitize_seat(e, now) else {
            continue;
        };
        total += 1;
        if let Some(i) = scenes.iter().position(|s| s.name == seat.rig) {
            scenes[i].seats.push(seat);
        } else if scenes.len() < MAX_RIGS {
            scenes.push(RigScene {
                name: seat.rig.clone(),
                seats: vec![seat],
                edges: vec![],
            });
        }
    }
    scenes
}

/// Distinct, strictly valid rig ids from the daemon's own output (they go into an argv, so they are re-checked).
pub fn rig_ids(payload: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in entries(payload) {
        if let Some(id) = e.get("rigId").and_then(Value::as_str)
            && valid_rig_id(id)
            && !out.iter().any(|x| x == id)
        {
            out.push(id.to_string());
        }
    }
    out.truncate(MAX_RIGS);
    out
}

pub fn valid_rig_id(id: &str) -> bool {
    id.len() == 26
        && id
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
}

/// Map a rig id to the rig name the seats carry, from the same payload.
pub fn rig_names(payload: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for e in entries(payload) {
        let id = e
            .get("rigId")
            .and_then(Value::as_str)
            .filter(|i| valid_rig_id(i));
        let name = e.get("rigName").and_then(text);
        if let (Some(id), Some(name)) = (id, name)
            && !out.iter().any(|(i, _)| i == id)
        {
            out.push((id.to_string(), name));
        }
    }
    out
}

/// A rig spec -> `(kind, from, to)` on logical ids (`pod.member`). Only these three values are read from the spec.
pub fn spec_edges(spec: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut add = |kind: Option<&str>, from: Option<&str>, to: Option<&str>, pod: Option<&str>| {
        let (Some(kind), Some(from), Some(to)) = (kind, from, to) else {
            return;
        };
        if !EDGE_KINDS.contains(&kind) {
            return;
        }
        let (from, to) = match pod {
            Some(p) => (format!("{p}.{from}"), format!("{p}.{to}")),
            None => (from.to_string(), to.to_string()),
        };
        if name_ok(&from) && name_ok(&to) {
            out.push((kind.to_string(), from, to));
        }
    };
    if let Some(edges) = spec.get("edges").and_then(Value::as_array) {
        for e in edges {
            add(
                e.get("kind").and_then(Value::as_str),
                e.get("from").and_then(Value::as_str),
                e.get("to").and_then(Value::as_str),
                None,
            );
        }
    }
    if let Some(pods) = spec.get("pods").and_then(Value::as_array) {
        for p in pods {
            let Some(pid) = p.get("id").and_then(Value::as_str).filter(|i| name_ok(i)) else {
                continue;
            };
            if let Some(edges) = p.get("edges").and_then(Value::as_array) {
                for e in edges {
                    add(
                        e.get("kind").and_then(Value::as_str),
                        e.get("from").and_then(Value::as_str),
                        e.get("to").and_then(Value::as_str),
                        Some(pid),
                    );
                }
            }
        }
    }
    out
}

/// Put the logical-id edges of one rig onto its drawn seats: an edge survives only when both ends are seats in the scene.
pub fn attach_edges(scene: &mut RigScene, raw: &[(String, String, String)]) {
    let find = |logical: &str| {
        scene
            .seats
            .iter()
            .find(|s| format!("{}.{}", s.pod, s.member) == logical)
            .map(|s| s.name.clone())
    };
    let mut edges: Vec<Edge> = Vec::new();
    for (kind, from, to) in raw {
        if let (Some(a), Some(b)) = (find(from), find(to)) {
            let e = Edge {
                from: a,
                to: b,
                kind: kind.clone(),
            };
            if e.from != e.to && !edges.contains(&e) {
                edges.push(e);
            }
        }
        if edges.len() >= MAX_EDGES {
            break;
        }
    }
    scene.edges = edges;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_map_to_animations() {
        for (life, act, want) in [
            ("running", "running", Mood::Working),
            ("running", "working", Mood::Working),
            ("running", "idle", Mood::Napping),
            ("running", "needs_input", Mood::Alert),
            ("running", "error", Mood::Alert),
            ("running", "blocked", Mood::Parked),
            ("running", "parked", Mood::Parked),
            ("running", "unknown", Mood::Unknown),
            ("running", "something-new", Mood::Unknown),
            ("stopped", "running", Mood::Ghost),
            ("terminated", "idle", Mood::Ghost),
            ("unknown", "unknown", Mood::Ghost),
        ] {
            assert_eq!(mood(life, act), want, "{life}/{act}");
        }
    }

    #[test]
    fn timestamps_parse_to_epoch_seconds() {
        assert_eq!(epoch_secs("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(epoch_secs("2000-03-01T00:00:00Z"), Some(951868800));
        assert_eq!(epoch_secs("2026-10-04T12:27:11.411Z"), Some(1791116831));
        assert_eq!(epoch_secs("not a date"), None);
        assert_eq!(epoch_secs("2026-13-04T12:27:11.411Z"), None);
        assert_eq!(epoch_secs("2026-10-04T12:27:11.411+02:00"), None);
    }
}
