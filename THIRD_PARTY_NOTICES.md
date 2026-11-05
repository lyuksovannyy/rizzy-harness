# Third-party notices

## caveman (adapted text)

`crates/agent-native/src/style.rs` contains answer-style rules adapted from
the "caveman" skill (`skills/caveman/SKILL.md`, `skills/ultracave/SKILL.md`)
by Julius Brussee, https://github.com/JuliusBrussee/caveman, licensed under
the Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0).
Changes: shortened, reworded, rules merged and renumbered, harness-specific
parts added. "Caveman" is a trademark of its owner; this project is not
affiliated with or endorsed by it.

## Ideas, no code copied

- Shell output filtering (`crates/tools/src/filter.rs`): ideas from rtk
  (https://github.com/rtk-ai/rtk), whose CLI is Apache-2.0. No code copied.
- Masking only older tool outputs and keeping the cached prefix stable: ideas
  from headroom (https://github.com/headroomlabs-ai/headroom, Apache-2.0) and
  "The Complexity Trap" (JetBrains Research, MIT). No code copied.
- Repository map: the approach of aider's repo map (tree-sitter tags ranked by
  PageRank), reimplemented.
