-- table fact_fts on fact_fts
CREATE VIRTUAL TABLE fact_fts USING fts5(
    id UNINDEXED, text, tokenize = 'unicode61 remove_diacritics 2'
);

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

