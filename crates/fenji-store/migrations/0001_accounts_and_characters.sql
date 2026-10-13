-- M3 (docs/superpowers/specs/2026-10-12-m3-login-lobby-design.md §4). Times are Unix milliseconds.
-- Invite codes, session tokens and game tokens are SHA-256 hashes, never the secrets.
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_phc TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_sign_in_at INTEGER
) STRICT;

CREATE TABLE invites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code_hash BLOB NOT NULL UNIQUE,
    label TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER,
    revoked_at INTEGER,
    -- Set to null if the account is deleted; redeemed_at keeps the invite spent.
    redeemed_by INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    redeemed_at INTEGER
) STRICT;

CREATE TABLE sessions (
    token_hash BLOB PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE TABLE game_tokens (
    token_hash BLOB PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    used_at INTEGER
) STRICT;

-- AUTOINCREMENT: a deleted character's pid is never handed out again.
CREATE TABLE characters (
    pid INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    vocation INTEGER NOT NULL,
    skeleton INTEGER NOT NULL,
    level INTEGER NOT NULL DEFAULT 1,
    appearance BLOB NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX characters_by_account ON characters(account_id);
CREATE INDEX sessions_by_account ON sessions(account_id);
CREATE INDEX game_tokens_by_account ON game_tokens(account_id);
