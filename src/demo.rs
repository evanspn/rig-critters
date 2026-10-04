//! `critters --demo`: made-up seats whose moods rotate, so the scene can be seen without a rig (and screenshotted).

use crate::model::{Edge, RigScene, Seat, Snapshot};

struct Fake {
    pod: &'static str,
    member: &'static str,
    runtime: &'static str,
    /// (lifecycle, activity, pending) shown in turn
    states: &'static [(&'static str, &'static str, u32)],
    period: f32,
    phase: usize,
}

const WORKSHOP: &[Fake] = &[
    Fake {
        pod: "orchestration",
        member: "orchestrator",
        runtime: "claude-code",
        states: &[
            ("running", "running", 0),
            ("running", "idle", 0),
            ("running", "needs_input", 0),
        ],
        period: 9.0,
        phase: 0,
    },
    Fake {
        pod: "implementation",
        member: "builder",
        runtime: "claude-code",
        states: &[
            ("running", "running", 3),
            ("running", "running", 5),
            ("running", "running", 1),
            ("running", "idle", 0),
        ],
        period: 7.0,
        phase: 0,
    },
    Fake {
        pod: "implementation",
        member: "qa",
        runtime: "claude-code",
        states: &[
            ("running", "idle", 0),
            ("running", "running", 0),
            ("running", "running", 2),
        ],
        period: 8.0,
        phase: 1,
    },
    Fake {
        pod: "delivery",
        member: "pr-watcher",
        runtime: "claude-code",
        states: &[
            ("running", "idle", 0),
            ("running", "idle", 0),
            ("running", "running", 0),
        ],
        period: 10.0,
        phase: 2,
    },
    Fake {
        pod: "security",
        member: "reviewer",
        runtime: "claude-code",
        states: &[
            ("running", "blocked", 0),
            ("running", "idle", 0),
            ("running", "running", 0),
        ],
        period: 11.0,
        phase: 0,
    },
];

const STUDIO: &[Fake] = &[
    Fake {
        pod: "design",
        member: "designer",
        runtime: "codex",
        states: &[("running", "running", 2), ("running", "idle", 0)],
        period: 8.0,
        phase: 0,
    },
    Fake {
        pod: "advisor",
        member: "lead",
        runtime: "claude-code",
        states: &[("running", "idle", 0), ("running", "running", 0)],
        period: 12.0,
        phase: 1,
    },
    Fake {
        pod: "oversight",
        member: "monitor",
        runtime: "claude-code",
        states: &[
            ("running", "needs_input", 0),
            ("running", "idle", 0),
            ("running", "running", 0),
        ],
        period: 6.0,
        phase: 0,
    },
    Fake {
        pod: "queue",
        member: "worker",
        runtime: "terminal",
        states: &[
            ("stopped", "unknown", 0),
            ("stopped", "unknown", 0),
            ("running", "running", 4),
        ],
        period: 14.0,
        phase: 0,
    },
    Fake {
        pod: "research",
        member: "librarian",
        runtime: "claude-code",
        states: &[("running", "unknown", 0), ("running", "idle", 0)],
        period: 9.0,
        phase: 1,
    },
];

fn scene(name: &str, fakes: &[Fake], t: f32) -> RigScene {
    let seats: Vec<Seat> = fakes
        .iter()
        .map(|f| {
            let i = ((t / f.period) as usize + f.phase) % f.states.len();
            let (lifecycle, activity, pending) = f.states[i];
            Seat {
                name: format!("{}-{}@{}", f.pod, f.member, name),
                rig: name.to_string(),
                pod: f.pod.to_string(),
                member: f.member.to_string(),
                runtime: f.runtime.to_string(),
                activity: activity.to_string(),
                lifecycle: lifecycle.to_string(),
                assigned: pending,
                pending,
                idle_secs: Some(if activity == "running" { 2 } else { 120 }),
            }
        })
        .collect();
    let by = |pod: &str, member: &str| {
        seats
            .iter()
            .find(|s| s.pod == pod && s.member == member)
            .map(|s| s.name.clone())
    };
    let mut edges = Vec::new();
    let mut add = |kind: &str, a: (&str, &str), b: (&str, &str)| {
        if let (Some(from), Some(to)) = (by(a.0, a.1), by(b.0, b.1)) {
            edges.push(Edge {
                from,
                to,
                kind: kind.into(),
            });
        }
    };
    add(
        "delegates_to",
        ("orchestration", "orchestrator"),
        ("implementation", "builder"),
    );
    add(
        "delegates_to",
        ("implementation", "builder"),
        ("implementation", "qa"),
    );
    add(
        "delegates_to",
        ("implementation", "qa"),
        ("delivery", "pr-watcher"),
    );
    add(
        "delegates_to",
        ("implementation", "builder"),
        ("security", "reviewer"),
    );
    add(
        "can_observe",
        ("delivery", "pr-watcher"),
        ("implementation", "builder"),
    );
    add("delegates_to", ("advisor", "lead"), ("design", "designer"));
    add(
        "escalates_to",
        ("oversight", "monitor"),
        ("advisor", "lead"),
    );
    add("delegates_to", ("advisor", "lead"), ("queue", "worker"));
    RigScene {
        name: name.to_string(),
        seats,
        edges,
    }
}

/// The demo snapshot at `t` seconds.
pub fn snapshot(t: f32) -> Snapshot {
    Snapshot {
        rigs: vec![
            scene("workshop-demo", WORKSHOP, t),
            scene("studio-demo", STUDIO, t),
        ],
        asleep: false,
    }
}
