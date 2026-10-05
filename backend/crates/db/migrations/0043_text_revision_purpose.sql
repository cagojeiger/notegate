-- Invocation purpose belongs to the resulting body, not the body it replaced.
-- Existing revisions have no trustworthy purpose linkage; leave them NULL.
ALTER TABLE text_objects ADD COLUMN revision_purpose TEXT CHECK (char_length(revision_purpose) <= 200);
ALTER TABLE text_revisions ADD COLUMN purpose TEXT CHECK (char_length(purpose) <= 200);
