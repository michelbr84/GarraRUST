use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use tracing::{debug, warn};

/// A single incoming iMessage read from chat.db.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    pub rowid: i64,
    pub text: String,
    pub sender: String,
    pub timestamp: i64,
    pub group_name: Option<String>,
}

/// Read-only handle to `~/Library/Messages/chat.db`.
pub struct ChatDb {
    conn: Connection,
    last_seen_rowid: i64,
    path: PathBuf,
}

/// macOS Core Data epoch offset: seconds between Unix epoch (1970) and Apple epoch (2001).
const CORE_DATA_EPOCH_OFFSET: i64 = 978_307_200;

/// Convert a macOS Core Data nanosecond timestamp to Unix epoch seconds.
pub fn core_data_ns_to_unix(ns: i64) -> i64 {
    ns / 1_000_000_000 + CORE_DATA_EPOCH_OFFSET
}

/// Default path to the iMessage database.
pub fn default_chat_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join("Library/Messages/chat.db")
}

impl ChatDb {
    /// Open the chat database read-only and initialise `last_seen_rowid` to the
    /// current maximum so we only pick up messages arriving after startup.
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Self::open_connection(path)?;

        let max_rowid: i64 = conn
            .query_row("SELECT COALESCE(MAX(ROWID), 0) FROM message", [], |row| {
                row.get(0)
            })
            .map_err(|e| format!("failed to query max ROWID: {e}"))?;

        debug!(
            "opened chat.db at {}, last_seen_rowid = {max_rowid}",
            path.display()
        );

        Ok(Self {
            conn,
            last_seen_rowid: max_rowid,
            path: path.to_path_buf(),
        })
    }

    /// Open a read-only SQLite connection to the given path.
    fn open_connection(path: &Path) -> Result<Connection, String> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;

        Connection::open_with_flags(path, flags).map_err(|e| {
            format!(
                "failed to open chat.db at {}: {e}. \
                 Ensure Full Disk Access is granted to the terminal / GarraIA binary \
                 in System Settings → Privacy & Security → Full Disk Access.",
                path.display()
            )
        })
    }

    /// Attempt to reopen the database connection (e.g. after a lock error).
    ///
    /// Preserves the current `last_seen_rowid` cursor.
    pub fn reopen(&mut self) -> Result<(), String> {
        let conn = Self::open_connection(&self.path)?;
        self.conn = conn;
        debug!("reopened chat.db at {}", self.path.display());
        Ok(())
    }

    /// Poll for new incoming messages since the last poll.
    ///
    /// Returns messages ordered by date ascending, including both DMs and group chats.
    /// Group chat messages will have `group_name` set.
    /// Messages with attachments but no text get synthesized text like `[Attachment: file.heic]`.
    pub fn poll(&mut self) -> Result<Vec<IncomingMessage>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT m.ROWID, m.text, m.date, m.is_from_me, m.cache_roomnames, \
                        h.id AS sender_id, \
                        (SELECT GROUP_CONCAT(a.filename, ', ') \
                         FROM message_attachment_join maj \
                         JOIN attachment a ON maj.attachment_id = a.ROWID \
                         WHERE maj.message_id = m.ROWID) AS attachments \
                 FROM message m \
                 JOIN handle h ON m.handle_id = h.ROWID \
                 WHERE m.ROWID > ?1 AND m.is_from_me = 0 \
                 ORDER BY m.date ASC",
            )
            .map_err(|e| format!("failed to prepare poll query: {e}"))?;

        let rows = stmt
            .query_map([self.last_seen_rowid], |row| {
                let rowid: i64 = row.get(0)?;
                let text: Option<String> = row.get(1)?;
                let date: i64 = row.get(2)?;
                let cache_roomnames: Option<String> = row.get(4)?;
                let sender: String = row.get(5)?;
                let attachments: Option<String> = row.get(6)?;
                Ok((rowid, text, date, cache_roomnames, sender, attachments))
            })
            .map_err(|e| format!("failed to execute poll query: {e}"))?;

        let mut messages = Vec::new();
        for row in rows {
            match row {
                Ok((rowid, text, date, cache_roomnames, sender, attachments)) => {
                    if rowid > self.last_seen_rowid {
                        self.last_seen_rowid = rowid;
                    }

                    let resolved_text = match (text.as_deref(), attachments.as_deref()) {
                        (Some(t), _) if !t.is_empty() => t.to_string(),
                        (_, Some(att)) if !att.is_empty() => synthesize_attachment_text(att),
                        _ => continue, // no text and no attachments — skip
                    };

                    let group_name = cache_roomnames.filter(|r| !r.is_empty());

                    messages.push(IncomingMessage {
                        rowid,
                        text: resolved_text,
                        sender,
                        timestamp: core_data_ns_to_unix(date),
                        group_name,
                    });
                }
                Err(e) => {
                    warn!("imessage: error reading message row: {e}");
                }
            }
        }

        Ok(messages)
    }
}

/// Convert an attachment filename list into a human-readable placeholder.
///
/// Input: comma-separated full paths like `~/Library/Messages/Attachments/.../IMG_1234.heic`
/// Output: `[Attachment: IMG_1234.heic]` or `[Attachments: a.heic, b.png]`
fn synthesize_attachment_text(filenames: &str) -> String {
    let names: Vec<&str> = filenames
        .split(", ")
        .map(|f| {
            // Extract just the filename from the full path
            f.rsplit('/').next().unwrap_or(f)
        })
        .collect();

    if names.len() == 1 {
        format!("[Attachment: {}]", names[0])
    } else {
        format!("[Attachments: {}]", names.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_conversion() {
        // 2024-01-01 00:00:00 UTC in Core Data nanoseconds:
        // Unix timestamp for 2024-01-01 = 1704067200
        // Core Data seconds = 1704067200 - 978307200 = 725760000
        // Core Data nanoseconds = 725760000 * 1_000_000_000
        let core_data_ns: i64 = 725_760_000 * 1_000_000_000;
        let unix = core_data_ns_to_unix(core_data_ns);
        assert_eq!(unix, 1_704_067_200);
    }

    #[test]
    fn timestamp_zero_is_apple_epoch() {
        // Core Data timestamp 0 = 2001-01-01 00:00:00 UTC = Unix 978307200
        assert_eq!(core_data_ns_to_unix(0), CORE_DATA_EPOCH_OFFSET);
    }

    #[test]
    fn synthesize_single_attachment() {
        let result = synthesize_attachment_text("~/Library/Messages/Attachments/ab/IMG_1234.heic");
        assert_eq!(result, "[Attachment: IMG_1234.heic]");
    }

    #[test]
    fn synthesize_multiple_attachments() {
        let result = synthesize_attachment_text("~/path/a.heic, ~/path/b.png");
        assert_eq!(result, "[Attachments: a.heic, b.png]");
    }

    #[test]
    fn synthesize_bare_filename() {
        assert_eq!(
            synthesize_attachment_text("photo.jpg"),
            "[Attachment: photo.jpg]"
        );
    }

    // --- Mock chat.db integration tests ---

    /// Create an in-memory SQLite database with the iMessage schema subset we need.
    fn mock_chat_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE handle (
                ROWID INTEGER PRIMARY KEY,
                id TEXT NOT NULL
            );
            CREATE TABLE message (
                ROWID INTEGER PRIMARY KEY,
                text TEXT,
                date INTEGER NOT NULL,
                is_from_me INTEGER NOT NULL DEFAULT 0,
                cache_roomnames TEXT,
                handle_id INTEGER REFERENCES handle(ROWID)
            );
            CREATE TABLE attachment (
                ROWID INTEGER PRIMARY KEY,
                filename TEXT
            );
            CREATE TABLE message_attachment_join (
                message_id INTEGER REFERENCES message(ROWID),
                attachment_id INTEGER REFERENCES attachment(ROWID)
            );",
        )
        .unwrap();
        conn
    }

    /// Build a ChatDb from an in-memory connection for testing.
    fn chat_db_from_conn(conn: Connection) -> ChatDb {
        ChatDb {
            conn,
            last_seen_rowid: 0,
            path: PathBuf::from(":memory:"),
        }
    }

    #[test]
    fn poll_returns_dm_messages() {
        let conn = mock_chat_db();
        conn.execute(
            "INSERT INTO handle (ROWID, id) VALUES (1, '+15551234567')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (ROWID, text, date, is_from_me, handle_id) VALUES (1, 'hello', 725760000000000000, 0, 1)",
            [],
        )
        .unwrap();

        let mut db = chat_db_from_conn(conn);
        let msgs = db.poll().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "hello");
        assert_eq!(msgs[0].sender, "+15551234567");
        assert!(msgs[0].group_name.is_none());
        assert_eq!(msgs[0].rowid, 1);
    }

    #[test]
    fn poll_returns_group_messages_with_group_name() {
        let conn = mock_chat_db();
        conn.execute(
            "INSERT INTO handle (ROWID, id) VALUES (1, '+15551234567')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (ROWID, text, date, is_from_me, cache_roomnames, handle_id) \
             VALUES (1, 'group msg', 725760000000000000, 0, 'chat123456', 1)",
            [],
        )
        .unwrap();

        let mut db = chat_db_from_conn(conn);
        let msgs = db.poll().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "group msg");
        assert_eq!(msgs[0].group_name.as_deref(), Some("chat123456"));
    }

    #[test]
    fn poll_excludes_is_from_me() {
        let conn = mock_chat_db();
        conn.execute(
            "INSERT INTO handle (ROWID, id) VALUES (1, '+15551234567')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (ROWID, text, date, is_from_me, handle_id) VALUES (1, 'my msg', 725760000000000000, 1, 1)",
            [],
        )
        .unwrap();

        let mut db = chat_db_from_conn(conn);
        let msgs = db.poll().unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn poll_synthesizes_attachment_text() {
        let conn = mock_chat_db();
        conn.execute(
            "INSERT INTO handle (ROWID, id) VALUES (1, '+15551234567')",
            [],
        )
        .unwrap();
        // Message with NULL text but has an attachment
        conn.execute(
            "INSERT INTO message (ROWID, text, date, is_from_me, handle_id) VALUES (1, NULL, 725760000000000000, 0, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attachment (ROWID, filename) VALUES (1, '~/Library/Messages/Attachments/ab/IMG_1234.heic')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message_attachment_join (message_id, attachment_id) VALUES (1, 1)",
            [],
        )
        .unwrap();

        let mut db = chat_db_from_conn(conn);
        let msgs = db.poll().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "[Attachment: IMG_1234.heic]");
    }

    #[test]
    fn poll_skips_null_text_no_attachments() {
        let conn = mock_chat_db();
        conn.execute(
            "INSERT INTO handle (ROWID, id) VALUES (1, '+15551234567')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (ROWID, text, date, is_from_me, handle_id) VALUES (1, NULL, 725760000000000000, 0, 1)",
            [],
        )
        .unwrap();

        let mut db = chat_db_from_conn(conn);
        let msgs = db.poll().unwrap();
        assert!(msgs.is_empty());
        // But cursor should still advance
        assert_eq!(db.last_seen_rowid, 1);
    }

    #[test]
    fn poll_rowid_cursor_advances() {
        let conn = mock_chat_db();
        conn.execute(
            "INSERT INTO handle (ROWID, id) VALUES (1, '+15551234567')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (ROWID, text, date, is_from_me, handle_id) VALUES (1, 'first', 725760000000000000, 0, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (ROWID, text, date, is_from_me, handle_id) VALUES (2, 'second', 725760001000000000, 0, 1)",
            [],
        )
        .unwrap();

        let mut db = chat_db_from_conn(conn);
        let msgs = db.poll().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(db.last_seen_rowid, 2);

        // Second poll should return nothing
        let msgs = db.poll().unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn poll_text_with_attachment_uses_text() {
        let conn = mock_chat_db();
        conn.execute(
            "INSERT INTO handle (ROWID, id) VALUES (1, '+15551234567')",
            [],
        )
        .unwrap();
        // Message with both text AND attachment — text should win
        conn.execute(
            "INSERT INTO message (ROWID, text, date, is_from_me, handle_id) VALUES (1, 'look at this', 725760000000000000, 0, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attachment (ROWID, filename) VALUES (1, '~/path/photo.jpg')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message_attachment_join (message_id, attachment_id) VALUES (1, 1)",
            [],
        )
        .unwrap();

        let mut db = chat_db_from_conn(conn);
        let msgs = db.poll().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "look at this");
    }
}
