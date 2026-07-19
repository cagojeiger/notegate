-- New users default to the lowest product tier when inserted outside the
-- application account provisioning path. Existing user tiers are unchanged.

ALTER TABLE users ALTER COLUMN tier SET DEFAULT 'tier0';
