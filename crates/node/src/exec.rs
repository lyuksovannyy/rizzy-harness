//! The execution modes this node offers (O13, N.10a;
//! docs/execution-modes.md): computed once at start from the node's own
//! self-test and its `[execution]` table, never from anything received.
//! The master's request only chooses inside it.

use harness_core::policy::ExecMode;
pub use harness_protocol::{GAP_HOST_NETNS, GAP_SANDBOX_NETNS};

/// Where `[execution] host = true` is explained: every refusal that names
/// it points there.
pub const HOST_README: &str = "README, \"Execution modes\"";

/// What this node offers, with why not where it does not.
#[derive(Debug, Clone)]
pub struct ExecOffers {
    sandbox: Result<(), String>,
    host: Result<(), String>,
    /// Capability strings of the offered modes' gaps
    /// (`harness_protocol::GAP_*`).
    gaps: Vec<&'static str>,
}

impl Default for ExecOffers {
    /// What a runtime offers before (or without) a start-up self-test:
    /// `sandbox` only, its commands run as the backend's confinement says.
    /// The binary always replaces it with [`ExecOffers::new`] from its
    /// self-test (`cli/src/main.rs`, `start_sandbox`); this default is for
    /// tests and embedders that start no sandbox.
    fn default() -> Self {
        Self {
            sandbox: Ok(()),
            host: Err(host_off()),
            gaps: Vec::new(),
        }
    }
}

/// Why `host` is not offered on a node that did not turn it on.
pub fn host_off() -> String {
    format!(
        "the host mode is off on this node ([execution] host = false in node.toml, the \
         default; {HOST_README})"
    )
}

impl ExecOffers {
    /// `sandbox` and `host` each offered (`Ok`) or not, with why not.
    pub fn new(
        sandbox: Result<(), String>,
        host: Result<(), String>,
        gaps: Vec<&'static str>,
    ) -> Self {
        Self {
            sandbox,
            host,
            gaps,
        }
    }

    fn of(&self, mode: ExecMode) -> &Result<(), String> {
        match mode {
            ExecMode::Sandbox => &self.sandbox,
            ExecMode::Host => &self.host,
        }
    }

    pub fn offers(&self, mode: ExecMode) -> bool {
        self.of(mode).is_ok()
    }

    /// The modes offered, most confined first.
    pub fn offered(&self) -> Vec<ExecMode> {
        ExecMode::ALL
            .into_iter()
            .filter(|m| self.offers(*m))
            .collect()
    }

    /// Why `mode` is not offered, or `None` where it is.
    pub fn why_not(&self, mode: ExecMode) -> Option<&str> {
        self.of(mode).as_ref().err().map(String::as_str)
    }

    /// The plain refusal of a node that offers no mode, or `None`.
    pub fn none_offered(&self) -> Option<String> {
        if !self.offered().is_empty() {
            return None;
        }
        let (s, h) = (
            self.why_not(ExecMode::Sandbox).unwrap_or_default(),
            self.why_not(ExecMode::Host).unwrap_or_default(),
        );
        // The Landlock sandbox required and not working refuses both, with
        // its own advice (`[sandbox] mode`).
        if s == h {
            return Some(s.to_string());
        }
        Some(format!(
            "this node offers no way to run commands: install bubblewrap, or set \
             `[execution] host = true` in node.toml and read what that means ({HOST_README}). \
             The sandbox mode: {s}. The host mode: {h}"
        ))
    }

    /// The mode of a new agentic session: `requested` if this node offers
    /// it, else refused; without one, the most confined mode offered. What
    /// the master asks for never widens what is offered.
    pub fn choose(&self, requested: Option<ExecMode>) -> Result<ExecMode, String> {
        if let Some(why) = self.none_offered() {
            return Err(why);
        }
        match requested {
            Some(m) => match self.why_not(m) {
                None => Ok(m),
                Some(why) => Err(format!(
                    "this node does not offer the {} mode: {why}",
                    m.id()
                )),
            },
            None => Ok(self.offered()[0]),
        }
    }

    /// Before every turn, callback, restore and delegation of an agentic
    /// session: its recorded mode is still offered. A session whose mode is
    /// no longer offered (bubblewrap gone since, `host` turned off) runs
    /// nothing; it is never moved to another mode, in either direction.
    pub fn check_session(&self, recorded: Option<ExecMode>) -> Result<(), String> {
        let Some(m) = recorded else {
            return Err(
                "this chat has no execution mode in its log, so it runs nothing here: start a \
                 new chat"
                    .into(),
            );
        };
        match self.why_not(m) {
            None => Ok(()),
            Some(why) => Err(format!(
                "this chat runs in the {} mode, which this node no longer offers ({why}). It \
                 runs nothing here, and its mode is never changed: start a new chat, or ask the \
                 node's admin",
                m.id()
            )),
        }
    }

    /// What the hello announces: one `exec:<mode>` per mode offered, then
    /// the gaps.
    pub fn capabilities(&self) -> Vec<String> {
        self.offered()
            .into_iter()
            .map(harness_protocol::exec_capability)
            .chain(self.gaps.iter().map(|g| g.to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn off(why: &str) -> Result<(), String> {
        Err(why.into())
    }

    #[test]
    fn a_node_offering_nothing_refuses_with_both_reasons() {
        let o = ExecOffers::new(off("bubblewrap not found"), Err(host_off()), vec![]);
        assert!(o.offered().is_empty());
        for ask in [None, Some(ExecMode::Sandbox), Some(ExecMode::Host)] {
            let e = o.choose(ask).unwrap_err();
            assert!(
                e.contains("this node offers no way to run commands: install bubblewrap"),
                "{e}"
            );
            assert!(e.contains("[execution] host = true"), "{e}");
            assert!(e.contains("bubblewrap not found"), "{e}");
        }
        assert!(o.capabilities().is_empty());
        // One cause for both (the Landlock sandbox required): said once.
        let o = ExecOffers::new(off("set [sandbox] mode"), off("set [sandbox] mode"), vec![]);
        assert_eq!(o.choose(None).unwrap_err(), "set [sandbox] mode");
    }

    #[test]
    fn the_request_chooses_only_inside_what_is_offered() {
        let both = ExecOffers::new(Ok(()), Ok(()), vec![]);
        assert_eq!(
            both.choose(None).unwrap(),
            ExecMode::Sandbox,
            "most confined"
        );
        assert_eq!(both.choose(Some(ExecMode::Host)).unwrap(), ExecMode::Host);
        let sandbox = ExecOffers::default();
        let e = sandbox.choose(Some(ExecMode::Host)).unwrap_err();
        assert!(e.contains("does not offer the host mode"), "{e}");
        assert!(e.contains("[execution] host = false"), "{e}");
        let host = ExecOffers::new(off("bubblewrap not found"), Ok(()), vec![]);
        assert_eq!(host.choose(None).unwrap(), ExecMode::Host);
        let e = host.choose(Some(ExecMode::Sandbox)).unwrap_err();
        assert!(
            e.contains("does not offer the sandbox mode: bubblewrap"),
            "{e}"
        );
    }

    #[test]
    fn a_session_whose_mode_is_gone_runs_nothing_and_is_never_moved() {
        let host_only = ExecOffers::new(off("bubblewrap not found"), Ok(()), vec![]);
        let e = host_only
            .check_session(Some(ExecMode::Sandbox))
            .unwrap_err();
        assert!(
            e.contains("runs in the sandbox mode") && e.contains("no longer"),
            "{e}"
        );
        assert!(host_only.check_session(Some(ExecMode::Host)).is_ok());
        let sandbox_only = ExecOffers::default();
        let e = sandbox_only
            .check_session(Some(ExecMode::Host))
            .unwrap_err();
        assert!(e.contains("runs in the host mode"), "{e}");
        assert!(sandbox_only.check_session(None).is_err());
    }

    #[test]
    fn the_hello_names_the_modes_and_the_gaps() {
        let o = ExecOffers::new(
            Ok(()),
            Ok(()),
            vec![
                harness_protocol::GAP_SANDBOX_NETNS,
                harness_protocol::GAP_HOST_NETNS,
            ],
        );
        assert_eq!(
            o.capabilities(),
            [
                "exec:sandbox",
                "exec:host",
                "gap:sandbox:netns",
                "gap:host:netns"
            ]
        );
    }
}
