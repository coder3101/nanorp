CREATE TABLE providers (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    provider_type TEXT NOT NULL,
    api_url       TEXT NOT NULL,
    api_key       TEXT,
    is_default    INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE characters (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    role          TEXT,
    personality   TEXT,
    system_prompt TEXT,
    greeting      TEXT,
    avatar_path   TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE chat_sessions (
    id           TEXT PRIMARY KEY,
    character_id TEXT NOT NULL,
    title        TEXT,
    last_message TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    FOREIGN KEY (character_id) REFERENCES characters(id) ON DELETE CASCADE
);

CREATE TABLE messages (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL,
    role        TEXT NOT NULL,
    content     TEXT NOT NULL,
    model_used  TEXT,
    provider_id TEXT,
    created_at  TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE SET NULL
);

CREATE TABLE attachments (
    id            TEXT PRIMARY KEY,
    message_id    TEXT NOT NULL,
    content_type  TEXT NOT NULL,
    file_path     TEXT NOT NULL,
    original_name TEXT,
    file_size     INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
