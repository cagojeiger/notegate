CREATE OR REPLACE FUNCTION mark_link_graph_space_pending()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.op_type = 'item.update'
       AND NEW.metadata -> 'name_changed' = 'false'::jsonb THEN
        RETURN NEW;
    END IF;

    INSERT INTO link_graph_space_states (
        space_id,
        available_at,
        pending_since_event_id
    ) VALUES (
        NEW.space_id,
        clock_timestamp() + interval '5 minutes',
        NEW.id
    )
    ON CONFLICT (space_id) DO UPDATE
    SET available_at = GREATEST(
            COALESCE(link_graph_space_states.available_at, '-infinity'::timestamptz),
            clock_timestamp() + interval '5 minutes'
        ),
        pending_since_event_id = COALESCE(
            link_graph_space_states.pending_since_event_id,
            EXCLUDED.pending_since_event_id
        );

    RETURN NEW;
END;
$$;
