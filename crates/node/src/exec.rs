//! The execution modes this node offers (O13, N.10a;
//! docs/execution-modes.md): computed once at start from the node's own
//! self-test and its `[execution]` table, never from anything received.
//! The master's request only chooses inside it.

use harness_core::policy::ExecMode;
pub use harness_protocol::{
    GAP_HOST_NET_OPEN, GAP_HOST_NETNS, GAP_SANDBOX_NETNS, GAP_SANDBOX_PROC, GAP_SANDBOX_SOCKETS,
};

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

    /// What a started sandbox lets this node offer (N.10a): `sandbox` where
    /// its containment works, `host` where `[execution] host = true` and
    /// the Landlock sandbox does not refuse the node; with every gap the
    /// containment line names, so the chat says it too (reviews T8B-3, L-d,
    /// L-e). Decided once at start from the node's own self-test and
    /// node.toml.
    pub fn from_start(s: &harness_tools::sandbox::Started, policy: &crate::NodePolicy) -> Self {
        use harness_tools::sandbox::{Confinement, Network};
        let sandbox = match s.sandbox_refusal() {
            Some(why) => Err(why.to_string()),
            None => Ok(()),
        };
        let host = match (&s.uncontained, policy.execution.host) {
            (Confinement::Refused(why), _) => Err(why.to_string()),
            (_, false) => Err(host_off()),
            (_, true) => Ok(()),
        };
        let mut gaps = Vec::new();
        if sandbox.is_ok() {
            if s.sandbox_netns_gap() {
                gaps.push(GAP_SANDBOX_NETNS);
            }
            if s.landlock_abi < harness_tools::sandbox::ABI_PATHNAME_SOCKETS {
                gaps.push(GAP_SANDBOX_SOCKETS);
            }
            if s.sandbox_form() == Some(harness_tools::contain::Form::SharedProc) {
                gaps.push(GAP_SANDBOX_PROC);
            }
        }
        if host.is_ok() && policy.sandbox.network == Network::Off {
            // Landlock stops TCP only where it is there and enforces it.
            gaps.push(if s.host_tcp_stopped() {
                GAP_HOST_NETNS
            } else {
                GAP_HOST_NET_OPEN
            });
        }
        Self::new(sandbox, host, gaps)
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
    /// it, else refused; without one, `sandbox` (review L-a: `host` is only
    /// ever given when asked for by name, never as a default). What the
    /// master asks for never widens what is offered.
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
            None if self.offers(ExecMode::Sandbox) => Ok(ExecMode::Sandbox),
            None => Err(format!(
                "this node offers only the host mode (commands without the sandbox), and it \
                 must be asked for by name; the sandbox mode: {}",
                self.why_not(ExecMode::Sandbox).unwrap_or_default()
            )),
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

    /// The confinement the backend gets for `mode` (review L-f): `c` where
    /// this node offers the mode, else one that refuses every command with
    /// the reason. So a session whose log says a mode the node does not
    /// offer can start nothing, even if a check before its turn were missed.
    pub fn confinement_for(
        &self,
        mode: ExecMode,
        c: harness_tools::sandbox::Confinement,
    ) -> harness_tools::sandbox::Confinement {
        match self.why_not(mode) {
            None => c,
            Some(why) => harness_tools::sandbox::Confinement::Refused(why.into()),
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
        // Review L-a: never `host` by default.
        let e = host.choose(None).unwrap_err();
        assert!(
            e.contains("only the host mode") && e.contains("by name"),
            "{e}"
        );
        assert_eq!(host.choose(Some(ExecMode::Host)).unwrap(), ExecMode::Host);
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

    /// Review L-f: the backend's confinement for a mode the node does not
    /// offer refuses every command, whatever the session's log says.
    #[test]
    fn a_mode_not_offered_gets_a_confinement_that_runs_nothing() {
        use harness_tools::sandbox::Confinement;
        let o = ExecOffers::default();
        let dir = tempfile::tempdir().unwrap();
        let shell = |c: &Confinement| {
            c.shell(
                std::ffi::OsStr::new("bash"),
                &[],
                dir.path(),
                None,
                &Default::default(),
                &[],
                Default::default(),
            )
            .map(|_| ())
        };
        let host = o.confinement_for(ExecMode::Host, Confinement::Unconfined);
        let e = shell(&host).unwrap_err();
        assert!(e.contains("[execution] host = false"), "{e}");
        let sandbox = o.confinement_for(ExecMode::Sandbox, Confinement::Unconfined);
        assert!(shell(&sandbox).is_ok());
    }

    fn started(
        confinement: harness_tools::sandbox::Confinement,
        uncontained: harness_tools::sandbox::Confinement,
        abi: u32,
    ) -> harness_tools::sandbox::Started {
        harness_tools::sandbox::Started {
            confinement,
            uncontained,
            refusal: None,
            summary: String::new(),
            contain_summary: String::new(),
            contain_level: harness_tools::sandbox::ContainLevel::Ok,
            view_summary: String::new(),
            tmp_root: None,
            landlock_abi: abi,
        }
    }

    fn landlock(
        contain: Option<harness_tools::contain::Form>,
        network: harness_tools::sandbox::Network,
    ) -> harness_tools::sandbox::Confinement {
        use harness_tools::sandbox::{Confinement, Sandbox, Settings};
        let settings = Settings {
            network,
            ..Default::default()
        };
        let mut sb = Sandbox::new("/bin/true".into(), &settings, None, &[]).unwrap();
        if let Some(form) = contain {
            sb = sb.with_containment(
                harness_tools::contain::Contain::new("/usr/bin/bwrap".into(), form)
                    .without_network_namespace(),
            );
        }
        Confinement::Confined {
            sandbox: std::sync::Arc::new(sb),
            tmp: None,
        }
    }

    /// Review L-d: with the network off, a `host` chat is told TCP is
    /// stopped only where Landlock is there and enforces it; where nothing
    /// applies it (no Landlock, `mode = "off"`) it is told everything goes
    /// out.
    #[test]
    fn a_host_chat_is_told_whether_anything_stops_its_network() {
        use harness_tools::sandbox::{Confinement, Network};
        let mut p = crate::NodePolicy::default();
        p.execution.host = true;
        p.sandbox.network = Network::Off;
        let refused = || Confinement::Refused("no sandbox here".into());
        for (uncontained, abi, want) in [
            (landlock(None, Network::Off), 6, GAP_HOST_NETNS),
            (landlock(None, Network::Off), 0, GAP_HOST_NET_OPEN),
            (Confinement::Unconfined, 0, GAP_HOST_NET_OPEN),
        ] {
            let o = ExecOffers::from_start(&started(refused(), uncontained, abi), &p);
            assert_eq!(o.capabilities(), ["exec:host", want], "abi {abi}");
        }
    }

    /// Review L-e: every gap the containment line names reaches the hello:
    /// below Landlock ABI 9 the socket gap, the shared-`/proc` form, the
    /// network namespace missing with the network off.
    #[test]
    fn the_hello_announces_every_gap_of_the_sandbox() {
        use harness_tools::contain::Form;
        use harness_tools::sandbox::Network;
        let mut p = crate::NodePolicy::default();
        p.sandbox.network = Network::Off;
        let s = started(
            landlock(Some(Form::SharedProc), Network::Off),
            landlock(None, Network::Off),
            8,
        );
        assert_eq!(
            ExecOffers::from_start(&s, &p).capabilities(),
            [
                "exec:sandbox",
                GAP_SANDBOX_NETNS,
                GAP_SANDBOX_SOCKETS,
                GAP_SANDBOX_PROC
            ]
        );
        p.sandbox.network = Network::On;
        let s = started(
            landlock(Some(Form::Full), Network::On),
            landlock(None, Network::On),
            9,
        );
        assert_eq!(
            ExecOffers::from_start(&s, &p).capabilities(),
            ["exec:sandbox"]
        );
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
