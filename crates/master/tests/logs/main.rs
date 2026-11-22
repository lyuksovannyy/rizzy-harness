//! Master tests that read the process's log output, in one test program.
//!
//! `tracing` allows one global subscriber per process, so these files cannot
//! live in `tests/it`: they share one capture (`capture::logs`) instead.
//! Only files whose log assertions stay true with other tests' lines in the
//! buffer ("this secret appears nowhere", "this line appears") belong here;
//! a file that asserts "nothing was logged" or an exact line count must be a
//! test program of its own, like `tests/hooks_http.rs` and
//! `tests/forge_connections_http.rs`.

#[path = "../common/mod.rs"]
mod common;
#[path = "../fake_mcp/mod.rs"]
mod fake_mcp;

mod capture;

mod connector_calls_http;
mod connector_client;
mod custom_connectors_http;
mod doc_http;
mod forge_clone_http;
mod login_binding_http;
mod temp_passwords_http;
