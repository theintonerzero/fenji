-- M4 hosting (docs/superpowers/specs/2026-10-14-m4-hosting-design.md §8, HA5): ops, the accounts
-- that are admins besides those `[access].admins` names, and bans, which refuse an account's
-- sign-ins and game tickets with the host's reason. Both go with their account. `since` is in
-- Unix milliseconds.
CREATE TABLE ops (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE
) STRICT;

CREATE TABLE bans (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    reason TEXT NOT NULL,
    since INTEGER NOT NULL
) STRICT;
