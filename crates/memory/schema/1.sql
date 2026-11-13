-- table fact_fts on fact_fts
CREATE VIRTUAL TABLE fact_fts USING fts5(
    id UNINDEXED, text, tokenize = 'unicode61 remove_diacritics 2'
);

-- table fact_fts_config on fact_fts_config
CREATE TABLE 'fact_fts_config'(k PRIMARY KEY, v) WITHOUT ROWID;

-- table fact_fts_content on fact_fts_content
CREATE TABLE 'fact_fts_content'(id INTEGER PRIMARY KEY, c0, c1);

-- table fact_fts_data on fact_fts_data
CREATE TABLE 'fact_fts_data'(id INTEGER PRIMARY KEY, block BLOB);

-- table fact_fts_docsize on fact_fts_docsize
CREATE TABLE 'fact_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);

-- table fact_fts_idx on fact_fts_idx
CREATE TABLE 'fact_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;

-- table fact_indexed on fact_indexed
CREATE TABLE fact_indexed (
    id    TEXT NOT NULL,
    model TEXT NOT NULL,
    PRIMARY KEY (id, model)
);

-- table fact_vec on fact_vec
CREATE TABLE fact_vec (
    id    TEXT NOT NULL,
    model TEXT NOT NULL,
    vec   BLOB NOT NULL,
    PRIMARY KEY (id, model)
);

-- table facts on facts
CREATE TABLE facts (
    id             TEXT PRIMARY KEY,
    owner_id       TEXT,
    text           TEXT NOT NULL,
    kind           TEXT NOT NULL,
    status         TEXT NOT NULL,
    source_session TEXT,
    dedupe_key     TEXT UNIQUE,
    created_ms     INTEGER NOT NULL,
    updated_ms     INTEGER NOT NULL
);

-- index facts_owner on facts
CREATE INDEX facts_owner ON facts(owner_id);

-- index sqlite_autoindex_fact_indexed_1 on fact_indexed
-- (created by SQLite)

-- index sqlite_autoindex_fact_vec_1 on fact_vec
-- (created by SQLite)

-- index sqlite_autoindex_facts_1 on facts
-- (created by SQLite)

-- index sqlite_autoindex_facts_2 on facts
-- (created by SQLite)

