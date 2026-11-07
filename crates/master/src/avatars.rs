//! How an agent looks in the UI: a shape with eyes in one of the accents.
//! The browser draws it and animates its expression; the server only keeps
//! the admin's choice (`[agent_avatars.<id>]` in config.toml) and derives a
//! stable default from the agent id for the rest.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Circle,
    Triangle,
    Square,
    Hexagon,
    Diamond,
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Eyes {
    Round,
    Long,
    Short,
    Wide,
    Sleepy,
}

/// The same names as the UI accents (`prefs::ACCENTS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    Blue,
    Violet,
    Green,
    Orange,
    Pink,
    Gray,
}

const SHAPES: [Shape; 6] = [
    Shape::Circle,
    Shape::Triangle,
    Shape::Square,
    Shape::Hexagon,
    Shape::Diamond,
    Shape::Drop,
];
const EYES: [Eyes; 5] = [
    Eyes::Round,
    Eyes::Long,
    Eyes::Short,
    Eyes::Wide,
    Eyes::Sleepy,
];
const ACCENTS: [Accent; 6] = [
    Accent::Blue,
    Accent::Violet,
    Accent::Green,
    Accent::Orange,
    Accent::Pink,
    Accent::Gray,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAvatar {
    pub shape: Shape,
    pub eyes: Eyes,
    pub accent: Accent,
}

/// FNV-1a 64-bit: the same on every build and platform, unlike std's hasher.
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

impl AgentAvatar {
    /// The avatar an agent has until an admin picks one: derived from its
    /// id alone, so it never changes between restarts or versions.
    pub fn default_for(agent_id: &str) -> Self {
        let h = fnv1a(agent_id);
        let pick = |n: usize, shift: u32| ((h >> shift) % n as u64) as usize;
        Self {
            shape: SHAPES[pick(SHAPES.len(), 0)],
            eyes: EYES[pick(EYES.len(), 21)],
            accent: ACCENTS[pick(ACCENTS.len(), 42)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_stable_and_varied() {
        // Pinned: a change here changes every unconfigured agent's look.
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
        let ids = ["general", "coder", "writer", "researcher", "analyst"];
        for id in ids {
            assert_eq!(AgentAvatar::default_for(id), AgentAvatar::default_for(id));
        }
        let looks: std::collections::HashSet<_> = ids
            .iter()
            .map(|id| serde_json::to_string(&AgentAvatar::default_for(id)).unwrap())
            .collect();
        assert!(looks.len() > 1, "different agents mostly look different");
    }

    #[test]
    fn serde_names() {
        let a: AgentAvatar =
            serde_json::from_str(r#"{"shape":"drop","eyes":"sleepy","accent":"gray"}"#).unwrap();
        assert_eq!(a.shape, Shape::Drop);
        assert!(
            serde_json::from_str::<AgentAvatar>(
                r#"{"shape":"star","eyes":"round","accent":"blue"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<AgentAvatar>(
                r#"{"shape":"circle","eyes":"round","accent":"blue","hat":1}"#
            )
            .is_err()
        );
    }
}
