//! Which animal a seat is. By default the role names the species; `~/.config/critters/animals.toml` overrides it.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub const ALL: [&str; 8] = [
    "owl", "beaver", "cat", "dog", "hawk", "heron", "fox", "otter",
];

/// Role words (matched anywhere in `pod.member`, in this order) and the animal they get.
const DEFAULT_RULES: [(&str, &str); 16] = [
    ("orchestrat", "owl"),
    ("lead", "owl"),
    ("builder", "beaver"),
    ("implement", "beaver"),
    ("developer", "beaver"),
    ("qa", "cat"),
    ("test", "cat"),
    ("secur", "dog"),
    ("review", "dog"),
    ("watcher", "hawk"),
    ("monitor", "heron"),
    ("oversight", "heron"),
    ("design", "fox"),
    ("advisor", "otter"),
    ("worker", "otter"),
    ("agent", "otter"),
];

/// User overrides: exact roles (`[roles]`, matched against the member name) and substrings (`[contains]`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Animals {
    roles: BTreeMap<String, String>,
    contains: BTreeMap<String, String>,
}

pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("critters").join("animals.toml"))
}

fn valid_species(s: &str) -> bool {
    ALL.contains(&s)
}

impl Animals {
    /// Parse `animals.toml`. Unknown species and malformed entries are reported and skipped; the rest still applies.
    pub fn parse(text: &str) -> (Animals, Vec<String>) {
        let mut a = Animals::default();
        let mut problems = Vec::new();
        let table: toml::Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                return (
                    a,
                    vec![format!("animals.toml is not valid TOML: {}", e.message())],
                );
            }
        };
        for (section, target) in [("roles", &mut a.roles), ("contains", &mut a.contains)] {
            let Some(t) = table.get(section).and_then(toml::Value::as_table) else {
                continue;
            };
            for (k, v) in t {
                match v.as_str() {
                    Some(s) if valid_species(s) => {
                        target.insert(k.to_lowercase(), s.to_string());
                    }
                    Some(s) => problems.push(format!(
                        "[{section}] {k}: '{s}' is not a critter (try: {})",
                        ALL.join(", ")
                    )),
                    None => problems.push(format!(
                        "[{section}] {k}: expected a critter name in quotes"
                    )),
                }
            }
        }
        (a, problems)
    }

    /// Load the user's file if there is one. A missing file is normal; a broken one is reported and ignored.
    pub fn load() -> (Animals, Vec<String>) {
        let Some(path) = config_path() else {
            return (Animals::default(), vec![]);
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => Animals::parse(&text),
            Err(_) => (Animals::default(), vec![]),
        }
    }

    /// The species for a seat. Stable: the same role always gets the same animal.
    pub fn species(&self, pod: &str, member: &str) -> &'static str {
        let member_l = member.to_lowercase();
        if let Some(s) = self.roles.get(&member_l) {
            return leak(s);
        }
        let pod_l = pod.to_lowercase();
        // the member says what a seat IS ("qa"); the pod only says where it sits ("implementation"), so the member wins
        for text in [&member_l, &pod_l] {
            for (word, s) in &self.contains {
                if text.contains(word.as_str()) {
                    return leak(s);
                }
            }
            for (word, s) in DEFAULT_RULES {
                if text.contains(word) {
                    return s;
                }
            }
        }
        // anything else: a stable pick from the pool by hash of the role name
        ALL[(fnv1a(member_l.as_bytes()) % ALL.len() as u32) as usize]
    }
}

fn leak(s: &str) -> &'static str {
    // species names are validated against ALL, so hand back the static one
    ALL.iter().find(|a| **a == s).copied().unwrap_or("owl")
}

pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = 0x811c9dc5u32;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roles_get_their_animals() {
        let a = Animals::default();
        for (pod, member, want) in [
            ("orchestration", "orchestrator", "owl"),
            ("implementation", "builder", "beaver"),
            ("implementation", "qa", "cat"),
            ("security", "reviewer", "dog"),
            ("security", "security-reviewer", "dog"),
            ("delivery", "pr-watcher", "hawk"),
            ("oversight", "monitor", "heron"),
            ("design", "ui-designer", "fox"),
            ("advisor", "lead", "owl"),
            ("queue", "worker", "otter"),
        ] {
            assert_eq!(a.species(pod, member), want, "{pod}.{member}");
        }
    }

    #[test]
    fn an_unknown_role_gets_a_stable_animal_from_the_pool() {
        let a = Animals::default();
        let x = a.species("zzz", "gizmo-tuner");
        assert!(ALL.contains(&x));
        for _ in 0..5 {
            assert_eq!(
                a.species("anything", "gizmo-tuner"),
                x,
                "the same role name always gets the same animal"
            );
        }
        // different roles are spread over the pool
        let picks: std::collections::BTreeSet<_> = (0..40)
            .map(|i| a.species("p", &format!("role-{i}")))
            .collect();
        assert!(picks.len() >= 5, "{picks:?}");
    }

    #[test]
    fn the_config_overrides_roles_and_substrings_and_reports_mistakes() {
        let (a, problems) = Animals::parse(
            "[roles]\nbuilder = \"otter\"\n[contains]\nwatch = \"fox\"\nbad = \"dragon\"\nnum = 3\n",
        );
        assert_eq!(a.species("implementation", "builder"), "otter");
        assert_eq!(a.species("delivery", "pr-watcher"), "fox");
        assert_eq!(
            a.species("implementation", "qa"),
            "cat",
            "unmentioned roles keep their default"
        );
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("dragon")));
        let (_, bad) = Animals::parse("this is = = not toml");
        assert!(bad[0].contains("not valid TOML"));
    }
}
