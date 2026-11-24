//! The master's HTTP and link tests, in one test program.
//!
//! Every test program links the whole server, so one file per program cost
//! 78 links of a very large binary after each edit. The files are modules
//! here; each keeps its own helpers (they stay private to their module).
//!
//! Not in here, on purpose:
//! - `connector_memory`, `connector_call_memory`, `connector_stall` and
//!   `forge_repositories_http` measure the process (resident memory, thread
//!   counts, stalls): they prove the 50 MB invariant and need a process of
//!   their own.
//! - Tests that install the global `tracing` subscriber to read the log:
//!   `tests/logs` shares one capture among the files whose assertions hold
//!   with other tests' lines in the buffer; `hooks_http`,
//!   `forge_connections_http` and `forge_repositories_http` keep their own.

#[path = "../common/mod.rs"]
mod common;
#[path = "../fake_mcp/mod.rs"]
mod fake_mcp;

mod accounts_http;
mod agent_edits_http;
mod agent_roles_http;
mod agent_switch_http;
mod agents_http;
mod approval_answers_http;
mod approval_mode_http;
mod artifacts_http;
mod attached_http;
mod attention_http;
mod background_stop_http;
mod bg_output_http;
mod blocking_io_http;
mod board_http;
mod callbacks_http;
mod chat_diff_http;
mod coding_memory_http;
mod compact_http;
mod deny_always_http;
mod doc_http;
mod doc_page_edit_http;
mod doc_preview_http;
mod exec_levels_http;
mod exec_mode_http;
mod exec_switch_http;
mod favicon_http;
mod files_http;
mod folder_file_http;
mod folder_page_edit_http;
mod folder_preview_http;
mod fork_http;
mod git_state_http;
mod handoff_http;
mod hardening_http;
mod hostile_node;
mod images_http;
mod incognito_http;
mod link_hardening;
mod live_audience_http;
mod live_audience_projects_http;
mod local_node_http;
mod login_limits_http;
mod made_http;
mod media_http;
mod member_walk_http;
mod memory_http;
mod memory_places_http;
mod mirror;
mod model_cache_http;
mod orchestration;
mod pdf_http;
mod peers_http;
mod pins_http;
mod plugins_http;
mod prices_http;
mod profile_http;
mod project_folder_http;
mod project_handoff_http;
mod project_mcp_http;
mod project_slug_parity;
mod projects_http;
mod provider_http;
mod recall_http;
mod remote_nodes;
mod rename_http;
mod replies_http;
mod retry_http;
mod rewind_http;
mod route_walk_http;
mod routines_http;
mod seams_http;
mod search_http;
mod server_limits;
mod settings_http;
mod skills_menu_http;
mod step_limit_http;
mod storage_http;
mod stored_skills_http;
mod turn_diff_http;
mod uploads_http;
mod usage_http;
mod version_refusal;
mod voice_http;
mod workspace_agents_http;
mod workspaces_http;
mod workspaces_resolve_http;
mod ws_sessions_http;
