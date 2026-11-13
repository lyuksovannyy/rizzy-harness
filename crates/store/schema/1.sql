-- table artifact_fts on artifact_fts
CREATE VIRTUAL TABLE artifact_fts USING fts5(artifact_id UNINDEXED, name, filenames, notes,
    tokenize = 'unicode61 remove_diacritics 2');

-- table artifact_fts_config on artifact_fts_config
CREATE TABLE 'artifact_fts_config'(k PRIMARY KEY, v) WITHOUT ROWID;

-- table artifact_fts_content on artifact_fts_content
CREATE TABLE 'artifact_fts_content'(id INTEGER PRIMARY KEY, c0, c1, c2, c3);

-- table artifact_fts_data on artifact_fts_data
CREATE TABLE 'artifact_fts_data'(id INTEGER PRIMARY KEY, block BLOB);

-- table artifact_fts_docsize on artifact_fts_docsize
CREATE TABLE 'artifact_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);

-- table artifact_fts_idx on artifact_fts_idx
CREATE TABLE 'artifact_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;

-- table artifact_projects on artifact_projects
CREATE TABLE artifact_projects (
    artifact_id TEXT PRIMARY KEY REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    project_id  TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE);

-- table artifact_versions on artifact_versions
CREATE TABLE artifact_versions (
    version_id  TEXT PRIMARY KEY,
    artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,
    status      TEXT NOT NULL,
    error       TEXT,
    blob_id     TEXT REFERENCES blobs(blob_id) ON DELETE CASCADE,
    size        INTEGER NOT NULL,
    sha256      TEXT NOT NULL,
    filename    TEXT NOT NULL,
    packed      TEXT,
    by_user     INTEGER NOT NULL DEFAULT 0,
    session_id  TEXT, call_id TEXT,
    stage_id    TEXT NOT NULL,
    node_id     TEXT NOT NULL,
    meta        TEXT NOT NULL DEFAULT '{}',
    notes       TEXT NOT NULL DEFAULT '',
    pinned      INTEGER NOT NULL DEFAULT 0,
    created_ms  INTEGER NOT NULL,
    ended_ms    INTEGER,
    UNIQUE (artifact_id, seq), UNIQUE (node_id, stage_id)
);

-- index artifact_versions_blob on artifact_versions
CREATE INDEX artifact_versions_blob ON artifact_versions(blob_id);

-- index artifact_versions_pending on artifact_versions
CREATE INDEX artifact_versions_pending ON artifact_versions(status) WHERE status = 'pending';

-- index artifact_versions_session on artifact_versions
CREATE INDEX artifact_versions_session ON artifact_versions(session_id);

-- table artifacts on artifacts
CREATE TABLE artifacts (
    artifact_id  TEXT PRIMARY KEY,
    workspace_id TEXT REFERENCES workspaces(workspace_id) ON DELETE SET NULL,
    owner_id     TEXT REFERENCES users(user_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    visibility   TEXT NOT NULL DEFAULT 'private',
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);

-- index artifacts_owner on artifacts
CREATE INDEX artifacts_owner ON artifacts(owner_id, workspace_id);

-- index artifacts_ws on artifacts
CREATE INDEX artifacts_ws ON artifacts(workspace_id, updated_ms DESC);

-- table attachments on attachments
CREATE TABLE attachments (
    attachment_id TEXT PRIMARY KEY,
    owner_id      TEXT,
    name          TEXT NOT NULL,
    mime          TEXT NOT NULL,
    text          TEXT NOT NULL,
    truncated     INTEGER NOT NULL,
    created_ms    INTEGER NOT NULL
);

-- table audit on audit
CREATE TABLE audit (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_ms  INTEGER NOT NULL,
    kind   TEXT NOT NULL,
    detail TEXT NOT NULL
);

-- table blob_previews on blob_previews
CREATE TABLE blob_previews (
    blob_id     TEXT PRIMARY KEY REFERENCES blobs(blob_id) ON DELETE CASCADE,
    width       INTEGER, height INTEGER,
    duration_ms INTEGER,
    thumb_mime  TEXT,
    thumb       BLOB,
    no_thumb    INTEGER NOT NULL DEFAULT 0,
    updated_ms  INTEGER NOT NULL
);

-- table blobs on blobs
CREATE TABLE blobs (
    blob_id    TEXT PRIMARY KEY,
    owner_id   TEXT REFERENCES users(user_id) ON DELETE CASCADE,
    sha256     TEXT NOT NULL,
    size       INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    format     TEXT NOT NULL,
    mime       TEXT NOT NULL,
    display    TEXT NOT NULL,
    key        BLOB NOT NULL,
    created_ms INTEGER NOT NULL
);

-- index blobs_owner on blobs
CREATE INDEX blobs_owner ON blobs(owner_id);

-- index blobs_owner_sha on blobs
CREATE UNIQUE INDEX blobs_owner_sha ON blobs(COALESCE(owner_id, ''), sha256);

-- table board_cards on board_cards
CREATE TABLE board_cards (
    card_id    TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE,
    column_id  TEXT NOT NULL REFERENCES board_columns(column_id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    notes      TEXT NOT NULL DEFAULT '',
    position   INTEGER NOT NULL,
    session_id TEXT,
    created_by TEXT,
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL
);

-- index board_cards_column on board_cards
CREATE INDEX board_cards_column ON board_cards(column_id);

-- table board_columns on board_columns
CREATE TABLE board_columns (
    column_id  TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    position   INTEGER NOT NULL,
    -- Cards here are finished: left out of the model's context.
    done       INTEGER NOT NULL DEFAULT 0
);

-- index board_columns_project on board_columns
CREATE INDEX board_columns_project ON board_columns(project_id);

-- table board_templates on board_templates
CREATE TABLE board_templates (
    template_id  TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- JSON array of {"name", "done"}.
    columns      TEXT NOT NULL,
    created_by   TEXT,
    created_ms   INTEGER NOT NULL
);

-- index board_templates_ws on board_templates
CREATE INDEX board_templates_ws ON board_templates(workspace_id);

-- table chat_fts on chat_fts
CREATE VIRTUAL TABLE chat_fts USING fts5(
    session_id UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- table chat_fts_config on chat_fts_config
CREATE TABLE 'chat_fts_config'(k PRIMARY KEY, v) WITHOUT ROWID;

-- table chat_fts_content on chat_fts_content
CREATE TABLE 'chat_fts_content'(id INTEGER PRIMARY KEY, c0, c1, c2);

-- table chat_fts_data on chat_fts_data
CREATE TABLE 'chat_fts_data'(id INTEGER PRIMARY KEY, block BLOB);

-- table chat_fts_docsize on chat_fts_docsize
CREATE TABLE 'chat_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);

-- table chat_fts_idx on chat_fts_idx
CREATE TABLE 'chat_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;

-- table deleted_sessions on deleted_sessions
CREATE TABLE deleted_sessions (
    session_id TEXT PRIMARY KEY,
    ts_ms      INTEGER NOT NULL,
    -- The node that owned it, so only that node is told it was deleted
    -- (NULL when the mirror never had a row for it).
    node_id    TEXT
);

-- table events on events
CREATE TABLE events (
    session_id TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    ts_ms      INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    payload    TEXT NOT NULL,
    PRIMARY KEY (session_id, seq)
) WITHOUT ROWID;

-- table join_tokens on join_tokens
CREATE TABLE join_tokens (
    token_hash TEXT PRIMARY KEY,
    expires_ms INTEGER NOT NULL,
    created_by TEXT
);

-- table kv on kv
CREATE TABLE kv (
    k TEXT PRIMARY KEY,
    v TEXT NOT NULL
);

-- table media_fts on media_fts
CREATE VIRTUAL TABLE media_fts USING fts5(media_id UNINDEXED, name,
    tokenize = 'unicode61 remove_diacritics 2');

-- table media_fts_config on media_fts_config
CREATE TABLE 'media_fts_config'(k PRIMARY KEY, v) WITHOUT ROWID;

-- table media_fts_content on media_fts_content
CREATE TABLE 'media_fts_content'(id INTEGER PRIMARY KEY, c0, c1);

-- table media_fts_data on media_fts_data
CREATE TABLE 'media_fts_data'(id INTEGER PRIMARY KEY, block BLOB);

-- table media_fts_docsize on media_fts_docsize
CREATE TABLE 'media_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);

-- table media_fts_idx on media_fts_idx
CREATE TABLE 'media_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;

-- table media_items on media_items
CREATE TABLE media_items (
    media_id     TEXT PRIMARY KEY,
    blob_id      TEXT NOT NULL REFERENCES blobs(blob_id) ON DELETE CASCADE,
    owner_id     TEXT REFERENCES users(user_id) ON DELETE CASCADE,
    workspace_id TEXT REFERENCES workspaces(workspace_id) ON DELETE SET NULL,
    name         TEXT NOT NULL,
    origin       TEXT NOT NULL,
    session_id   TEXT,
    artifact_id  TEXT,
    version_id   TEXT,
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);

-- index media_items_artifact on media_items
CREATE UNIQUE INDEX media_items_artifact ON media_items(artifact_id) WHERE artifact_id IS NOT NULL;

-- index media_items_owner on media_items
CREATE INDEX media_items_owner ON media_items(owner_id, created_ms DESC);

-- index media_items_session on media_items
CREATE INDEX media_items_session ON media_items(session_id);

-- table media_uses on media_uses
CREATE TABLE media_uses (
    media_id   TEXT NOT NULL REFERENCES media_items(media_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    PRIMARY KEY (media_id, session_id)
);

-- index media_uses_session on media_uses
CREATE INDEX media_uses_session ON media_uses(session_id);

-- table nodes on nodes
CREATE TABLE nodes (
    node_id    TEXT PRIMARY KEY,
    public_key TEXT NOT NULL UNIQUE,
    hostname   TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    revoked_ms INTEGER
);

-- table projects on projects
CREATE TABLE projects (
    project_id   TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    icon         TEXT NOT NULL,
    accent       TEXT NOT NULL,
    instructions TEXT NOT NULL DEFAULT '',
    -- JSON object; ProjectSettings in the master.
    settings     TEXT NOT NULL DEFAULT '{}',
    created_by   TEXT,
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);

-- index projects_ws on projects
CREATE INDEX projects_ws ON projects(workspace_id);

-- table routine_dedupe on routine_dedupe
CREATE TABLE routine_dedupe (
    routine_id TEXT NOT NULL REFERENCES routines(routine_id) ON DELETE CASCADE,
    key        TEXT NOT NULL,                 -- SHA-256 hex of the header value
    run_id     TEXT NOT NULL,
    expires_ms INTEGER NOT NULL,
    PRIMARY KEY (routine_id, key)
);

-- table routine_runs on routine_runs
CREATE TABLE routine_runs (
    run_id       TEXT PRIMARY KEY,            -- new_id()
    routine_id   TEXT NOT NULL REFERENCES routines(routine_id) ON DELETE CASCADE,
    trigger_id   TEXT,
    trigger_kind TEXT NOT NULL,               -- schedule | webhook | manual
    scheduled_ms INTEGER,
    started_ms   INTEGER NOT NULL,
    ended_ms     INTEGER,
    status       TEXT NOT NULL,   -- pending|running|waiting|completed|error|cancelled|interrupted|timed_out|failed|skipped|blocked|missed
    reason       TEXT,
    session_id   TEXT UNIQUE,                 -- no FK: the mirror may lag; cleared by delete_session
    chat_deleted INTEGER NOT NULL DEFAULT 0,  -- 1: its chat was deleted (by the owner or by retention)
    turn_id      TEXT,
    waiting_since_ms INTEGER,
    prompt       TEXT,                        -- rendered, <= 32 KiB; cleared when the chat is deleted
    event        TEXT,                        -- JSON, <= 16 KiB; cleared when the chat is deleted
    untrusted    INTEGER NOT NULL DEFAULT 0,
    warnings     TEXT,                        -- cleared when the chat is deleted
    usage        TEXT,
    attempt      INTEGER NOT NULL DEFAULT 1,
    repeats      INTEGER NOT NULL DEFAULT 1,  -- coalesced skipped/missed
    seen         INTEGER NOT NULL DEFAULT 0,  -- the owner viewed the run's chat or its attention row
    promoted     INTEGER NOT NULL DEFAULT 0   -- the owner prompted in the run's chat
);

-- index routine_runs_active on routine_runs
CREATE INDEX routine_runs_active ON routine_runs(status)
    WHERE status IN ('pending','running','waiting');

-- index routine_runs_routine on routine_runs
CREATE INDEX routine_runs_routine ON routine_runs(routine_id, started_ms DESC);

-- table routine_triggers on routine_triggers
CREATE TABLE routine_triggers (
    trigger_id  TEXT PRIMARY KEY,             -- random_hex(16); also a webhook's public id
    routine_id  TEXT NOT NULL REFERENCES routines(routine_id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,                -- schedule | webhook (later sources add names, §15.2)
    config      TEXT NOT NULL,                -- JSON (8.3)
    next_run_ms INTEGER,                      -- schedules only; NULL = nothing due
    secret_hash TEXT,                         -- webhook: SHA-256 hex of the bearer secret
    last_fired_ms INTEGER
);

-- index routine_triggers_due on routine_triggers
CREATE INDEX routine_triggers_due ON routine_triggers(next_run_ms) WHERE next_run_ms IS NOT NULL;

-- index routine_triggers_routine on routine_triggers
CREATE INDEX routine_triggers_routine ON routine_triggers(routine_id);

-- table routine_untrusted_chats on routine_untrusted_chats
CREATE TABLE routine_untrusted_chats (
    session_id TEXT PRIMARY KEY
) WITHOUT ROWID;

-- table routines on routines
CREATE TABLE routines (
    routine_id   TEXT PRIMARY KEY,                                                     -- new_id()
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    owner_id     TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    paused_reason TEXT,                       -- set by the master: blocked | failures | cap
    spec         TEXT NOT NULL,               -- JSON RoutineSpec (8.3)
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);

-- index routines_owner on routines
CREATE INDEX routines_owner ON routines(owner_id, workspace_id);

-- table seen on seen
CREATE TABLE seen (
    user_id    TEXT NOT NULL,
    session_id TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    PRIMARY KEY (user_id, session_id)
);

-- table session_images on session_images
CREATE TABLE session_images (
    session_id TEXT NOT NULL, sha256 TEXT NOT NULL, mime TEXT NOT NULL, name TEXT NOT NULL,
    bytes BLOB NOT NULL, PRIMARY KEY (session_id, sha256));

-- table session_projects on session_projects
CREATE TABLE session_projects (
    session_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE,
    added_ms   INTEGER NOT NULL
);

-- index session_projects_project on session_projects
CREATE INDEX session_projects_project ON session_projects(project_id);

-- table session_workspaces on session_workspaces
CREATE TABLE session_workspaces (
    session_id   TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE
);

-- index session_workspaces_ws on session_workspaces
CREATE INDEX session_workspaces_ws ON session_workspaces(workspace_id);

-- table sessions on sessions
CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    node_id    TEXT NOT NULL,
    title      TEXT NOT NULL DEFAULT '',
    cwd        TEXT NOT NULL DEFAULT '',
    model      TEXT NOT NULL DEFAULT '',
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL,
    head_seq   INTEGER NOT NULL,
    running    INTEGER NOT NULL DEFAULT 0,
    incognito  INTEGER NOT NULL DEFAULT 0,
    owner_id   TEXT,
    agent_id   TEXT,
    profile    TEXT,
    parent_id  TEXT,
    -- Attention: approvals waiting, or the last turn ended with a question.
    pending_approvals INTEGER NOT NULL DEFAULT 0,
    last_asks  INTEGER NOT NULL DEFAULT 0,
    asks_user  INTEGER NOT NULL DEFAULT 0,
    attention_seq INTEGER NOT NULL DEFAULT 0,
    -- Set when a second writer sent other content under this seq (PLAN.md
    -- §3): every later append is refused until the session is deleted.
    diverged_seq INTEGER
);

-- index sessions_owner on sessions
CREATE INDEX sessions_owner ON sessions(owner_id, updated_ms);

-- index sqlite_autoindex_artifact_projects_1 on artifact_projects
-- (created by SQLite)

-- index sqlite_autoindex_artifact_versions_1 on artifact_versions
-- (created by SQLite)

-- index sqlite_autoindex_artifact_versions_2 on artifact_versions
-- (created by SQLite)

-- index sqlite_autoindex_artifact_versions_3 on artifact_versions
-- (created by SQLite)

-- index sqlite_autoindex_artifacts_1 on artifacts
-- (created by SQLite)

-- index sqlite_autoindex_attachments_1 on attachments
-- (created by SQLite)

-- index sqlite_autoindex_blob_previews_1 on blob_previews
-- (created by SQLite)

-- index sqlite_autoindex_blobs_1 on blobs
-- (created by SQLite)

-- index sqlite_autoindex_board_cards_1 on board_cards
-- (created by SQLite)

-- index sqlite_autoindex_board_columns_1 on board_columns
-- (created by SQLite)

-- index sqlite_autoindex_board_templates_1 on board_templates
-- (created by SQLite)

-- index sqlite_autoindex_deleted_sessions_1 on deleted_sessions
-- (created by SQLite)

-- index sqlite_autoindex_join_tokens_1 on join_tokens
-- (created by SQLite)

-- index sqlite_autoindex_kv_1 on kv
-- (created by SQLite)

-- index sqlite_autoindex_media_items_1 on media_items
-- (created by SQLite)

-- index sqlite_autoindex_media_uses_1 on media_uses
-- (created by SQLite)

-- index sqlite_autoindex_nodes_1 on nodes
-- (created by SQLite)

-- index sqlite_autoindex_nodes_2 on nodes
-- (created by SQLite)

-- index sqlite_autoindex_projects_1 on projects
-- (created by SQLite)

-- index sqlite_autoindex_routine_dedupe_1 on routine_dedupe
-- (created by SQLite)

-- index sqlite_autoindex_routine_runs_1 on routine_runs
-- (created by SQLite)

-- index sqlite_autoindex_routine_runs_2 on routine_runs
-- (created by SQLite)

-- index sqlite_autoindex_routine_triggers_1 on routine_triggers
-- (created by SQLite)

-- index sqlite_autoindex_routines_1 on routines
-- (created by SQLite)

-- index sqlite_autoindex_seen_1 on seen
-- (created by SQLite)

-- index sqlite_autoindex_session_images_1 on session_images
-- (created by SQLite)

-- index sqlite_autoindex_session_projects_1 on session_projects
-- (created by SQLite)

-- index sqlite_autoindex_session_workspaces_1 on session_workspaces
-- (created by SQLite)

-- index sqlite_autoindex_sessions_1 on sessions
-- (created by SQLite)

-- index sqlite_autoindex_ui_sessions_1 on ui_sessions
-- (created by SQLite)

-- index sqlite_autoindex_users_1 on users
-- (created by SQLite)

-- index sqlite_autoindex_users_2 on users
-- (created by SQLite)

-- index sqlite_autoindex_workspace_members_1 on workspace_members
-- (created by SQLite)

-- index sqlite_autoindex_workspaces_1 on workspaces
-- (created by SQLite)

-- index sqlite_autoindex_workspaces_2 on workspaces
-- (created by SQLite)

-- table sqlite_sequence on sqlite_sequence
CREATE TABLE sqlite_sequence(name,seq);

-- table ui_sessions on ui_sessions
CREATE TABLE ui_sessions (
    token_hash   TEXT PRIMARY KEY,
    bind_hash    TEXT NOT NULL,
    user_id      TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    created_ms   INTEGER NOT NULL,
    last_seen_ms INTEGER NOT NULL,
    expires_ms   INTEGER NOT NULL
);

-- index ui_sessions_expires on ui_sessions
CREATE INDEX ui_sessions_expires ON ui_sessions(expires_ms);

-- index ui_sessions_user on ui_sessions
CREATE INDEX ui_sessions_user ON ui_sessions(user_id);

-- table users on users
CREATE TABLE users (
    user_id       TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL,
    created_ms    INTEGER NOT NULL,
    disabled      INTEGER NOT NULL DEFAULT 0,
    can_agentic   INTEGER NOT NULL DEFAULT 0,
    must_change_password INTEGER NOT NULL DEFAULT 0
);

-- table workspace_members on workspace_members
CREATE TABLE workspace_members (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    user_id      TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    role_id      TEXT NOT NULL,          -- owner | admin | member | guest
    joined_ms    INTEGER NOT NULL,
    -- The member's own choice to share their facts (personal memory) with this
    -- workspace's chats: recall and proposals. Every insert says it (1 in
    -- Personal, 0 elsewhere); the default only covers an insert that forgets,
    -- and then it shares nothing.
    use_memory   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (workspace_id, user_id)
);

-- index workspace_members_user on workspace_members
CREATE INDEX workspace_members_user ON workspace_members(user_id);

-- table workspaces on workspaces
CREATE TABLE workspaces (
    workspace_id   TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    personal_owner TEXT UNIQUE REFERENCES users(user_id) ON DELETE CASCADE,
    created_ms     INTEGER NOT NULL,
    -- Workspace-tier settings (JSON object; WorkspaceSettings in the master).
    settings       TEXT NOT NULL DEFAULT '{}'
);

