//! How an agent looks in the UI: a flat blob shape with pill eyes in one
//! colour. The browser draws it and animates its expression; the server only
//! keeps the admin's choice (`[agent_avatars.<id>]` in config.toml) and
//! derives a stable default from the agent id for the rest (the built-in
//! agents have a hand-picked one).

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
    Cloud,
    Bean,
    Heart,
    Star,
    Flower,
    Ghost,
    Cat,
    Bunny,
    Mushroom,
    Leaf,
    Mug,
    Shield,
    Bubble,
    Hat,
    Rocket,
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

/// The UI accents' names (`prefs::ACCENTS`), and white, black, brown and
/// teal, which only avatars draw in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Graphite,
    White,
    Black,
    Brown,
    Teal,
}

const SHAPES: [Shape; 21] = [
    Shape::Circle,
    Shape::Triangle,
    Shape::Square,
    Shape::Hexagon,
    Shape::Diamond,
    Shape::Drop,
    Shape::Cloud,
    Shape::Bean,
    Shape::Heart,
    Shape::Star,
    Shape::Flower,
    Shape::Ghost,
    Shape::Cat,
    Shape::Bunny,
    Shape::Mushroom,
    Shape::Leaf,
    Shape::Mug,
    Shape::Shield,
    Shape::Bubble,
    Shape::Hat,
    Shape::Rocket,
];
/// Every colour except black, which is for a choice, not a default.
const ACCENTS: [Accent; 11] = [
    Accent::Blue,
    Accent::Purple,
    Accent::Pink,
    Accent::Red,
    Accent::Orange,
    Accent::Yellow,
    Accent::Green,
    Accent::Graphite,
    Accent::White,
    Accent::Brown,
    Accent::Teal,
];

/// The looks of the built-in agents (`crates/agents/builtin`), hand-picked.
const BUILTIN: [(&str, Shape, Accent); 8] = [
    ("general", Shape::Cloud, Accent::Blue),
    ("coder", Shape::Hexagon, Accent::Green),
    ("researcher", Shape::Mushroom, Accent::Brown),
    ("reviewer", Shape::Shield, Accent::Purple),
    ("scout", Shape::Rocket, Accent::Orange),
    ("smol", Shape::Bean, Accent::Pink),
    ("teamlead", Shape::Star, Accent::Yellow),
    ("writer", Shape::Bubble, Accent::Teal),
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
    /// The avatar an agent has until an admin picks one: a built-in agent's
    /// own, else derived from its id alone, so it never changes between
    /// restarts or versions. Every default has the tall pill eyes.
    pub fn default_for(agent_id: &str) -> Self {
        if let Some(&(_, shape, accent)) = BUILTIN.iter().find(|(id, ..)| *id == agent_id) {
            return Self {
                shape,
                eyes: Eyes::Long,
                accent,
            };
        }
        let h = fnv1a(agent_id);
        let pick = |n: usize, shift: u32| ((h >> shift) % n as u64) as usize;
        Self {
            shape: SHAPES[pick(SHAPES.len(), 0)],
            eyes: Eyes::Long,
            accent: ACCENTS[pick(ACCENTS.len(), 32)],
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
        let ids = ["my-agent", "helper", "docs", "ops", "data"];
        for id in ids {
            assert_eq!(AgentAvatar::default_for(id), AgentAvatar::default_for(id));
        }
        let looks: std::collections::HashSet<_> = ids
            .iter()
            .map(|id| serde_json::to_string(&AgentAvatar::default_for(id)).unwrap())
            .collect();
        assert!(looks.len() > 1, "different agents mostly look different");
        // Pinned values: derived from the id alone, whatever else changes.
        assert_eq!(
            serde_json::to_string(&AgentAvatar::default_for("my-agent")).unwrap(),
            r#"{"shape":"leaf","eyes":"long","accent":"green"}"#
        );
    }

    #[test]
    fn derived_defaults_use_every_shape_and_colour_but_black() {
        let (mut shapes, mut accents) = (
            std::collections::HashSet::new(),
            std::collections::HashSet::new(),
        );
        for n in 0..2000 {
            let a = AgentAvatar::default_for(&format!("agent-{n}"));
            assert_eq!(a.eyes, Eyes::Long, "the default eyes are the tall pills");
            shapes.insert(a.shape as u8);
            accents.insert(a.accent as u8);
        }
        assert_eq!(shapes.len(), SHAPES.len());
        assert_eq!(accents.len(), ACCENTS.len());
        assert!(!accents.contains(&(Accent::Black as u8)));
    }

    #[test]
    fn built_in_agents_have_their_own_picked_look() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../agents/builtin");
        let mut seen = 0;
        for f in std::fs::read_dir(dir).unwrap() {
            let path = f.unwrap().path();
            if path.extension().is_some_and(|e| e == "md") {
                let id = path.file_stem().unwrap().to_str().unwrap().to_string();
                assert!(
                    BUILTIN.iter().any(|(b, ..)| *b == id),
                    "{id} has no picked look"
                );
                seen += 1;
            }
        }
        assert_eq!(seen, BUILTIN.len(), "no stale entry in the table");
        let coder = AgentAvatar::default_for("coder");
        assert_eq!((coder.shape, coder.accent), (Shape::Hexagon, Accent::Green));
    }

    #[test]
    fn serde_names() {
        let a: AgentAvatar =
            serde_json::from_str(r#"{"shape":"drop","eyes":"sleepy","accent":"graphite"}"#)
                .unwrap();
        assert_eq!(a.shape, Shape::Drop);
        assert_eq!(a.accent, Accent::Graphite);
        let v: AgentAvatar =
            serde_json::from_str(r#"{"shape":"drop","eyes":"sleepy","accent":"purple"}"#).unwrap();
        assert_eq!(v.accent, Accent::Purple);
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"shape":"drop","eyes":"sleepy","accent":"purple"}"#
        );
        // Every name the browser draws, and nothing else.
        let shapes = [
            "circle", "triangle", "square", "hexagon", "diamond", "drop", "cloud", "bean", "heart",
            "star", "flower", "ghost", "cat", "bunny", "mushroom", "leaf", "mug", "shield",
            "bubble", "hat", "rocket",
        ];
        assert_eq!(shapes.len(), SHAPES.len());
        for s in shapes {
            let j = format!(r#"{{"shape":"{s}","eyes":"long","accent":"blue"}}"#);
            let a: AgentAvatar = serde_json::from_str(&j).unwrap();
            assert_eq!(serde_json::to_string(&a).unwrap(), j, "{s}");
        }
        for e in ["round", "long", "short", "wide", "sleepy"] {
            let s = format!(r#"{{"shape":"cloud","eyes":"{e}","accent":"blue"}}"#);
            assert!(serde_json::from_str::<AgentAvatar>(&s).is_ok(), "{e}");
        }
        for a in [
            "blue", "purple", "pink", "red", "orange", "yellow", "green", "graphite", "white",
            "black", "brown", "teal",
        ] {
            let s = format!(r#"{{"shape":"drop","eyes":"sleepy","accent":"{a}"}}"#);
            assert!(serde_json::from_str::<AgentAvatar>(&s).is_ok(), "{a}");
        }
        // The palette's earlier names are gone, not aliased (no
        // compatibility code before the first tagged release); a colour
        // outside the list, and a free one, are refused.
        for a in ["violet", "gray", "cyan", "#ff0000", "Teal", ""] {
            let s = format!(r#"{{"shape":"drop","eyes":"sleepy","accent":"{a}"}}"#);
            assert!(serde_json::from_str::<AgentAvatar>(&s).is_err(), "{a}");
        }
        for s in ["blob", "Cloud", "square ", "", "sun"] {
            let j = format!(r#"{{"shape":"{s}","eyes":"round","accent":"blue"}}"#);
            assert!(serde_json::from_str::<AgentAvatar>(&j).is_err(), "{s}");
        }
        assert!(
            serde_json::from_str::<AgentAvatar>(
                r#"{"shape":"circle","eyes":"round","accent":"blue","hat":1}"#
            )
            .is_err()
        );
    }
}
