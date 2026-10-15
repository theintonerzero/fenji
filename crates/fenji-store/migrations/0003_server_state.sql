-- M4 hosting (docs/superpowers/specs/2026-10-14-m4-hosting-design.md §3.9, §8 HA1): what the
-- server keeps of its own state across a restart, one text value per key. `maintenance` holds the
-- maintenance mode's JSON (its message and times) while it's on, and `motd` the MOTD the host set
-- over the config file's.
CREATE TABLE server_state (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;
