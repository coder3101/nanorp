-- Timestamps alone do not totally order messages (several inserts can share
-- the same timestamp). Assign every existing message a per-session sequence,
-- preserving timestamp order and physical insertion order for timestamp ties.
ALTER TABLE messages ADD COLUMN sequence INTEGER NOT NULL DEFAULT 0;

WITH ordered AS (
    SELECT
        rowid AS message_rowid,
        ROW_NUMBER() OVER (
            PARTITION BY session_id
            ORDER BY created_at ASC, rowid ASC
        ) AS sequence
    FROM messages
)
UPDATE messages
SET sequence = (
    SELECT ordered.sequence
    FROM ordered
    WHERE ordered.message_rowid = messages.rowid
);

CREATE UNIQUE INDEX idx_messages_session_sequence
    ON messages(session_id, sequence);
