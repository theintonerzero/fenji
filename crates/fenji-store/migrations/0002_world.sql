-- M4's world (docs/superpowers/specs/2026-10-13-m4-world-design.md §3.2): where each character
-- stands, as its scene, a point (x, y, z up) and a yaw in degrees. All five are null until the
-- character first enters, and are written together. Characters are created at the client's
-- creation level, 37; the ones M3 created at level 1 become 37 here.
ALTER TABLE characters ADD COLUMN scene INTEGER;
ALTER TABLE characters ADD COLUMN pos_x REAL;
ALTER TABLE characters ADD COLUMN pos_y REAL;
ALTER TABLE characters ADD COLUMN pos_z REAL;
ALTER TABLE characters ADD COLUMN yaw REAL;
UPDATE characters SET level = 37 WHERE level < 37;
