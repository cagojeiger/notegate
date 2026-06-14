-- Allow Unicode space and node names while keeping path/target delimiters reserved.

ALTER TABLE spaces DROP CONSTRAINT IF EXISTS spaces_name_check;
ALTER TABLE spaces ADD CONSTRAINT spaces_name_check CHECK (
    char_length(name) BETWEEN 1 AND 63
    AND name = btrim(name)
    AND name NOT IN ('.', '..')
    AND name NOT LIKE '%/%'
    AND name NOT LIKE '%:%'
    AND name !~ '[[:cntrl:]]'
);

ALTER TABLE nodes DROP CONSTRAINT IF EXISTS nodes_check1;
ALTER TABLE nodes DROP CONSTRAINT IF EXISTS nodes_name_format_check;
ALTER TABLE nodes ADD CONSTRAINT nodes_name_format_check CHECK (
    parent_id IS NULL
    OR (
        char_length(name) <= 128
        AND name = btrim(name)
        AND name !~ '[[:cntrl:]]'
    )
);
