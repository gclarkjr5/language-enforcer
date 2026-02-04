use std::collections::HashSet;
use std::path::Path;

use chrono::{DateTime, Utc};
use le_core::{default_new_card, Language, Word};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::db::{Db, DbResult};

pub struct SqliteDb {
    conn: Connection,
}

impl SqliteDb {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }

    fn ensure_word_columns(&self) -> rusqlite::Result<()> {
        let mut stmt = self.conn.prepare("PRAGMA table_info(words)")?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut existing = HashSet::new();
        for column in columns {
            existing.insert(column?);
        }

        let mut missing = Vec::new();
        if !existing.contains("translation") {
            missing.push("ALTER TABLE words ADD COLUMN translation TEXT");
        }
        if !existing.contains("chapter") {
            missing.push("ALTER TABLE words ADD COLUMN chapter TEXT");
        }
        if !existing.contains("group_name") {
            missing.push("ALTER TABLE words ADD COLUMN group_name TEXT");
        }
        for stmt in missing {
            self.conn.execute(stmt, [])?;
        }
        Ok(())
    }
}

impl Db for SqliteDb {
    fn init(&self) -> DbResult<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS words (
                id TEXT PRIMARY KEY,
                text TEXT NOT NULL,
                language TEXT NOT NULL,
                translation TEXT,
                chapter TEXT,
                group_name TEXT,
                sentence TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cards (
                id TEXT PRIMARY KEY,
                word_id TEXT NOT NULL,
                due_at TEXT NOT NULL,
                interval_days INTEGER NOT NULL,
                ease REAL NOT NULL,
                reps INTEGER NOT NULL,
                lapses INTEGER NOT NULL,
                FOREIGN KEY(word_id) REFERENCES words(id)
            );
            CREATE TABLE IF NOT EXISTS reviews (
                id TEXT PRIMARY KEY,
                card_id TEXT NOT NULL,
                grade INTEGER NOT NULL,
                reviewed_at TEXT NOT NULL,
                FOREIGN KEY(card_id) REFERENCES cards(id)
            );",
        )?;
        self.ensure_word_columns()?;
        Ok(())
    }

    fn save_word(
        &self,
        text: &str,
        translation: &str,
        language: Language,
        chapter: Option<&str>,
        group: Option<&str>,
    ) -> DbResult<()> {
        let now = Utc::now();
        let word = Word {
            id: Uuid::new_v4(),
            text: text.to_string(),
            translation: Some(translation.to_string()),
            chapter: chapter.map(|value| value.to_string()),
            group: group.map(|value| value.to_string()),
            language,
            sentence: None,
            created_at: now,
        };

        let card = default_new_card(word.id, now);

        self.conn.execute(
            "INSERT INTO words (id, text, language, translation, chapter, group_name, sentence, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                word.id.to_string(),
                word.text,
                format!("{:?}", word.language),
                word.translation,
                word.chapter,
                word.group,
                word.sentence,
                word.created_at.to_rfc3339()
            ],
        )?;

        self.conn.execute(
            "INSERT INTO cards (id, word_id, due_at, interval_days, ease, reps, lapses) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                card.id.to_string(),
                card.word_id.to_string(),
                card.due_at.to_rfc3339(),
                card.interval_days,
                card.ease,
                card.reps,
                card.lapses
            ],
        )?;

        Ok(())
    }

    fn word_exists(&self, text: &str, language: Language) -> DbResult<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM words WHERE lower(text) = lower(?1) AND language = ?2 LIMIT 1")?;
        let mut rows = stmt.query(params![text, format!("{:?}", language)])?;
        Ok(rows.next()?.is_some())
    }

    fn load_all_words(&self) -> DbResult<Vec<Word>> {
        let mut words = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, text, language, translation, chapter, group_name, sentence, created_at
             FROM words
             ORDER BY chapter, group_name, created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            let language = match row.get::<_, String>(2)?.as_str() {
                "Dutch" => Language::Dutch,
                _ => Language::English,
            };
            let created_at = DateTime::parse_from_rfc3339(row.get::<_, String>(7)?.as_str())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(Word {
                id: Uuid::parse_str(row.get::<_, String>(0)?.as_str())
                    .unwrap_or_else(|_| Uuid::new_v4()),
                text: row.get(1)?,
                language,
                translation: row.get(3)?,
                chapter: row.get(4)?,
                group: row.get(5)?,
                sentence: row.get(6)?,
                created_at,
            })
        })?;

        for word in rows {
            words.push(word?);
        }

        Ok(words)
    }

    fn list_chapters(&self) -> DbResult<Vec<String>> {
        let mut chapters = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT chapter
             FROM words
             WHERE chapter IS NOT NULL AND trim(chapter) != ''
             ORDER BY chapter",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            chapters.push(row?);
        }
        Ok(chapters)
    }

    fn last_group_for_chapter(&self, chapter: &str) -> DbResult<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT group_name
             FROM words
             WHERE chapter = ?1 AND group_name IS NOT NULL AND trim(group_name) != ''
             ORDER BY created_at DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![chapter])?;
        if let Some(row) = rows.next()? {
            let value: String = row.get(0)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn delete_word(&self, word_id: Uuid) -> DbResult<()> {
        let id = word_id.to_string();
        self.conn.execute(
            "DELETE FROM reviews WHERE card_id IN (SELECT id FROM cards WHERE word_id = ?1)",
            params![id],
        )?;
        self.conn.execute("DELETE FROM cards WHERE word_id = ?1", params![word_id.to_string()])?;
        self.conn.execute("DELETE FROM words WHERE id = ?1", params![word_id.to_string()])?;
        Ok(())
    }

    fn delete_all_words(&self) -> DbResult<()> {
        self.conn.execute_batch(
            "DELETE FROM reviews;
             DELETE FROM cards;
             DELETE FROM words;",
        )?;
        Ok(())
    }
}
