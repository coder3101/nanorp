-- SQLite indexes nothing on its own: not the columns you sort by, and not
-- foreign keys. Listing conversations scanned and sorted all of `chat_sessions`,
-- and opening one scanned all of `messages` (then all of `attachments`).

CREATE INDEX idx_chat_sessions_updated_at ON chat_sessions(updated_at DESC);
CREATE INDEX idx_chat_sessions_character_id ON chat_sessions(character_id);
CREATE INDEX idx_messages_session_created ON messages(session_id, created_at);
CREATE INDEX idx_attachments_message_id ON attachments(message_id);
