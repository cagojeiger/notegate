-- Align the stored text line cap with the full-document editor read cap.

ALTER TABLE text_objects DROP CONSTRAINT IF EXISTS text_objects_line_count_check;
ALTER TABLE text_objects ADD CONSTRAINT text_objects_line_count_check CHECK (
    line_count >= 0 AND line_count <= 5000
);
