//! Reading the rig, read-only: `rig ps --nodes -A --json` every second or two, `rig spec show <id> --json` for the edges (cached).
//! Both run as a fixed argv, without a shell, with a timeout and an output cap. Nothing else is ever executed or read: no queue
//! rows, no transcripts, no message bodies.

use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::model::{self, RigScene, Snapshot, Value};

const RUN_TIMEOUT: Duration = Duration::from_secs(8);
const OUTPUT_CAP: usize = 2 * 1024 * 1024;
const EDGE_TTL: Duration = Duration::from_secs(60);

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Run an argv (never a shell) and parse its stdout as JSON. `None` on any failure.
pub fn run_json(bin: &str, args: &[&str]) -> Option<Value> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout)
            .take(OUTPUT_CAP as u64 + 1)
            .read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + RUN_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = reader.join().ok()?;
    if !status.success() || out.len() > OUTPUT_CAP {
        return None;
    }
    serde_json_from(&out)
}

fn serde_json_from(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(bytes).ok()
}

/// The daemon payload plus the per-rig-id edges -> the snapshot. Pure, so a hostile payload can be tested.
pub fn snapshot_from(
    payload: &Value,
    edges_by_rig: &BTreeMap<String, Vec<(String, String, String)>>,
    now: i64,
) -> Snapshot {
    let mut rigs: Vec<RigScene> = model::build_scenes(payload, now);
    let names = model::rig_names(payload);
    for scene in &mut rigs {
        let raw: Vec<_> = names
            .iter()
            .filter(|(_, n)| *n == scene.name)
            .flat_map(|(id, _)| edges_by_rig.get(id).cloned().unwrap_or_default())
            .collect();
        model::attach_edges(scene, &raw);
    }
    Snapshot {
        rigs,
        asleep: false,
    }
}

/// Poll in a background thread; the receiver gets a fresh snapshot every `interval`. A missing `rig` or a sleeping daemon
/// produces `asleep` snapshots, never an error.
pub fn spawn_live(interval: Duration) -> Receiver<Snapshot> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let bin = std::env::var("CRITTERS_RIG_BIN").unwrap_or_else(|_| "rig".into());
        let mut edges: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
        let mut edges_at: Option<Instant> = None;
        let mut edge_ids: Vec<String> = Vec::new();
        loop {
            let started = Instant::now();
            let snap = match run_json(&bin, &["ps", "--nodes", "-A", "--json"]) {
                Some(payload) => {
                    let ids = model::rig_ids(&payload);
                    if edges_at.is_none_or(|t| t.elapsed() > EDGE_TTL) || ids != edge_ids {
                        edges.clear();
                        for id in &ids {
                            // the id was validated against a strict pattern before it reaches an argv
                            if let Some(spec) = run_json(&bin, &["spec", "show", id, "--json"]) {
                                edges.insert(id.clone(), model::spec_edges(&spec));
                            }
                        }
                        edge_ids = ids;
                        edges_at = Some(Instant::now());
                    }
                    snapshot_from(&payload, &edges, unix_now())
                }
                None => Snapshot {
                    rigs: vec![],
                    asleep: true,
                },
            };
            if tx.send(snap).is_err() {
                return;
            }
            std::thread::sleep(interval.saturating_sub(started.elapsed()));
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload() -> Value {
        json!([
            {"rigId": "01M40XXBQ7V40B658XN1VBS6DV", "rigName": "kernel", "logicalId": "advisor.lead", "canonicalSessionName": "advisor-lead@kernel",
             "runtime": "claude-code", "lifecycleState": "running", "agentActivity": {"state": "needs_input", "reason": "selection_prompt"},
             "assignedWorkCount": 2, "pendingWorkCount": 1, "lastActivity": "2026-10-04T12:27:11.411Z",
             "cwd": "/Users/someone/secret/place", "resumeTokenPresent": true, "transcriptPath": "/x/y", "token": "sk-should-never-appear"},
            {"rigId": "01M40XXBQ7V40B658XN1VBS6DV", "rigName": "kernel", "logicalId": "queue.worker", "canonicalSessionName": "queue-worker@kernel",
             "runtime": "terminal", "lifecycleState": "stopped", "agentActivity": {"state": "unknown"}, "pendingWorkCount": 0}
        ])
    }

    #[test]
    fn a_daemon_payload_becomes_seats_and_extra_fields_are_dropped() {
        let snap = snapshot_from(
            &payload(),
            &BTreeMap::new(),
            model::epoch_secs("2026-10-04T12:27:21.000Z").unwrap(),
        );
        assert_eq!(snap.rigs.len(), 1);
        let s = &snap.rigs[0].seats[0];
        assert_eq!(
            (
                s.name.as_str(),
                s.rig.as_str(),
                s.pod.as_str(),
                s.member.as_str()
            ),
            ("advisor-lead@kernel", "kernel", "advisor", "lead")
        );
        assert_eq!(
            (
                s.activity.as_str(),
                s.lifecycle.as_str(),
                s.assigned,
                s.pending,
                s.idle_secs
            ),
            ("needs_input", "running", 2, 1, Some(10))
        );
        // the Debug text of everything we kept contains none of the hostile extras
        let all = format!("{snap:?}");
        for leaked in [
            "/Users/",
            "secret",
            "sk-should",
            "transcript",
            "resumeToken",
            "selection_prompt",
        ] {
            assert!(!all.contains(leaked), "{leaked} leaked: {all}");
        }
    }

    #[test]
    fn hostile_values_are_rejected_not_displayed() {
        let evil = json!([
            {"canonicalSessionName": "bad name with spaces and \u{1b}[31mescapes", "logicalId": "a.b", "lifecycleState": "running"},
            {"canonicalSessionName": "ok@r", "logicalId": "p.m", "rigName": "x".repeat(500), "pendingWorkCount": 1e12, "assignedWorkCount": -5,
             "lastActivity": "not a date", "agentActivity": {"state": "<script>"}, "runtime": 7},
            "just a string", 42, null, {"logicalId": ["array"]}
        ]);
        let snap = snapshot_from(&evil, &BTreeMap::new(), 0);
        // the first entry falls back to its (valid) logical id; the second keeps only what is valid
        let seats: Vec<_> = snap.rigs.iter().flat_map(|r| r.seats.iter()).collect();
        assert!(
            seats
                .iter()
                .all(|s| !s.name.contains(' ') && !s.name.contains('\u{1b}'))
        );
        let ok = seats.iter().find(|s| s.name == "ok@r").unwrap();
        assert_eq!((ok.pending, ok.assigned, ok.idle_secs), (9999, 0, None));
        assert_eq!(ok.activity, "unknown", "an invalid state is not echoed");
        assert_eq!(ok.runtime, "unknown");
        assert_eq!(
            ok.rig, "r",
            "an over-long rig name falls back to the one in the session name"
        );
    }

    #[test]
    fn edges_need_both_ends_and_a_known_kind_and_are_deduplicated() {
        let spec = json!({
            "edges": [
                {"kind": "delegates_to", "from": "advisor.lead", "to": "queue.worker"},
                {"kind": "delegates_to", "from": "advisor.lead", "to": "queue.worker"},
                {"kind": "teleports_to", "from": "advisor.lead", "to": "queue.worker"},
                {"kind": "delegates_to", "from": "advisor.lead", "to": "nobody.here"},
                {"kind": "delegates_to", "from": "advisor.lead", "to": "advisor.lead"}
            ],
            "pods": [{"id": "queue", "edges": [{"kind": "escalates_to", "from": "worker", "to": "worker"}]}]
        });
        let mut by = BTreeMap::new();
        by.insert(
            "01M40XXBQ7V40B658XN1VBS6DV".to_string(),
            model::spec_edges(&spec),
        );
        let snap = snapshot_from(&payload(), &by, 0);
        assert_eq!(
            snap.rigs[0].edges,
            vec![model::Edge {
                from: "advisor-lead@kernel".into(),
                to: "queue-worker@kernel".into(),
                kind: "delegates_to".into()
            }]
        );
    }

    #[test]
    fn rig_ids_must_match_the_strict_pattern_before_they_can_reach_an_argv() {
        let p = json!([{"rigId": "01M40XXBQ7V40B658XN1VBS6DV"}, {"rigId": "short"}, {"rigId": "01m40xxbq7v40b658xn1vbs6dv"}, {"rigId": "01M40XXBQ7V40B658XN1VBS6D;"}, {"rigId": 5}]);
        assert_eq!(
            model::rig_ids(&p),
            vec!["01M40XXBQ7V40B658XN1VBS6DV".to_string()]
        );
    }

    #[test]
    fn caps_hold() {
        let many: Vec<Value> = (0..200).map(|i| json!({"canonicalSessionName": format!("s{i}@r{}", i % 40), "logicalId": format!("p.m{i}"), "rigName": format!("r{}", i % 40)})).collect();
        let snap = snapshot_from(&Value::Array(many), &BTreeMap::new(), 0);
        assert!(snap.rigs.len() <= model::MAX_RIGS);
        assert!(snap.rigs.iter().map(|r| r.seats.len()).sum::<usize>() <= model::MAX_SEATS);
    }

    #[test]
    fn a_missing_rig_binary_is_not_an_error() {
        assert!(run_json("/nonexistent/definitely-not-rig", &["ps"]).is_none());
    }
}
