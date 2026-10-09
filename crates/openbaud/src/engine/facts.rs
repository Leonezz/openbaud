//! What a session knows about the device's state: facts established by
//! commands that declare `establishes`, and — for a fact no longer known —
//! why it was lost, so a precondition refusal can say what happened instead
//! of only that something is missing.

use serde::Serialize;
use std::collections::BTreeMap;

/// One fact: its value and the command (`device/command`) that set it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fact {
    pub value: String,
    pub by: String,
}

/// Where a fact stands on a session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FactState<'a> {
    Known(&'a Fact),
    /// Was known (or was being set) and is no longer — with the reason.
    Lost(&'a str),
    /// Nothing on this session ever established it.
    Never,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    known: BTreeMap<String, Fact>,
    lost: BTreeMap<String, String>,
}

impl Facts {
    pub fn get(&self, key: &str) -> FactState<'_> {
        if let Some(fact) = self.known.get(key) {
            FactState::Known(fact)
        } else if let Some(reason) = self.lost.get(key) {
            FactState::Lost(reason)
        } else {
            FactState::Never
        }
    }

    pub fn establish(&mut self, key: &str, value: &str, by: &str) {
        self.lost.remove(key);
        self.known.insert(
            key.to_string(),
            Fact {
                value: value.to_string(),
                by: by.to_string(),
            },
        );
    }

    /// The device may no longer be in the state this fact recorded.
    pub fn lose(&mut self, key: &str, reason: &str) {
        self.known.remove(key);
        self.lost.insert(key.to_string(), reason.to_string());
    }

    /// Nothing recorded can be trusted any more (e.g. arbitrary bytes were
    /// written): every known fact is lost, for the same reason.
    pub fn lose_all(&mut self, reason: &str) {
        let keys: Vec<String> = self.known.keys().cloned().collect();
        for key in keys {
            self.lose(&key, reason);
        }
    }

    pub fn known(&self) -> &BTreeMap<String, Fact> {
        &self.known
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fact_moves_between_known_lost_and_reestablished() {
        let mut facts = Facts::default();
        assert_eq!(facts.get("mode"), FactState::Never);

        facts.establish("mode", "MODBUS", "dev/set_mode");
        assert!(matches!(facts.get("mode"), FactState::Known(f) if f.value == "MODBUS"));

        facts.lose("mode", "dev/set_mode timed out");
        assert_eq!(facts.get("mode"), FactState::Lost("dev/set_mode timed out"));
        assert!(facts.known().is_empty());

        facts.establish("mode", "SETUP", "dev/escape");
        assert!(matches!(facts.get("mode"), FactState::Known(f) if f.by == "dev/escape"));
    }

    #[test]
    fn lose_all_keeps_the_reason_for_every_fact() {
        let mut facts = Facts::default();
        facts.establish("mode", "MODBUS", "dev/set_mode");
        facts.establish("profile", "pzem", "dev/select");
        facts.lose_all("a raw send wrote arbitrary bytes");
        assert_eq!(
            facts.get("mode"),
            FactState::Lost("a raw send wrote arbitrary bytes")
        );
        assert_eq!(
            facts.get("profile"),
            FactState::Lost("a raw send wrote arbitrary bytes")
        );
        assert_eq!(
            facts.get("other"),
            FactState::Never,
            "a fact never set stays never-set"
        );
    }
}
