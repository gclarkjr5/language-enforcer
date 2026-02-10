use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use le_core::{default_new_card, Language, Word};
use postgres::Client;
use postgres_native_tls::MakeTlsConnector;
use uuid::Uuid;

use crate::db::{Db, DbResult};

pub struct PostgresDb {
    client: Mutex<Client>,
}

fn sql_log_path() -> Option<String> {
    std::env::var("LOG_SQL_PATH").ok()
}

fn log_sql(query: &str, params: &[(&str, String)]) {
    let Some(path) = sql_log_path() else {
        return;
    };
    let mut line = String::new();
    line.push_str("[sql] ");
    line.push_str(query);
    for (name, value) in params {
        line.push_str("\n  - ");
        line.push_str(name);
        line.push_str(" = ");
        line.push_str(value);
    }
    line.push('\n');
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

impl PostgresDb {
    pub fn connect(url: &str, tls: MakeTlsConnector) -> DbResult<Self> {
        let client = Client::connect(url, tls)?;
        Ok(Self {
            client: Mutex::new(client),
        })
    }

    pub fn open(_path: &Path) -> DbResult<Self> {
        Err(crate::db::DbError::Config(
            "Postgres backend requires DATABASE_URL".to_string(),
        ))
    }
}

impl Db for PostgresDb {
    fn init(&self) -> DbResult<()> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| crate::db::DbError::Config("Postgres client lock poisoned".to_string()))?;
        client.batch_execute(
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
                word_id TEXT NOT NULL REFERENCES words(id),
                due_at TEXT NOT NULL,
                interval_days INTEGER NOT NULL,
                ease DOUBLE PRECISION NOT NULL,
                reps INTEGER NOT NULL,
                lapses INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS reviews (
                id TEXT PRIMARY KEY,
                card_id TEXT NOT NULL REFERENCES cards(id),
                grade INTEGER NOT NULL,
                reviewed_at TEXT NOT NULL
            );
            GRANT USAGE ON SCHEMA public TO authenticated;
            GRANT SELECT, INSERT, UPDATE, DELETE ON public.words TO authenticated;
            GRANT SELECT, INSERT, UPDATE, DELETE ON public.cards TO authenticated;
            GRANT SELECT, INSERT, UPDATE, DELETE ON public.reviews TO authenticated;
            ",
        )?;
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
        let language_value = format!("{:?}", word.language);
        let created_at = word.created_at.to_rfc3339();
        let due_at = card.due_at.to_rfc3339();
        let interval_days = card.interval_days;
        let ease = card.ease;
        let reps = card.reps;
        let lapses = card.lapses;

        let mut client = self
            .client
            .lock()
            .map_err(|_| crate::db::DbError::Config("Postgres client lock poisoned".to_string()))?;
        let translation = word.translation.clone();
        let chapter = word.chapter.clone();
        let group = word.group.clone();

        client
            .execute(
                "INSERT INTO words (id, text, language, translation, chapter, group_name, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &word.id.to_string(),
                    &word.text,
                    &language_value,
                    &translation,
                    &chapter,
                    &group,
                    &created_at,
                ],
            )
            .map_err(|err| {
                let message = format!("Postgres words insert failed: {err}");
                crate::db::log_error(&message);
                crate::db::DbError::Config(message)
            })?;

        log_sql(
            "INSERT INTO words (...) VALUES ($1..$7)",
            &[
                ("id", word.id.to_string()),
                ("text", word.text.clone()),
                ("language", language_value.clone()),
                ("translation", word.translation.clone().unwrap_or_default()),
                ("chapter", word.chapter.clone().unwrap_or_default()),
                ("group_name", word.group.clone().unwrap_or_default()),
                ("created_at", created_at.clone()),
            ],
        );

        log_sql(
            "INSERT INTO cards (...) VALUES ($1..$7)",
            &[
                ("id", card.id.to_string()),
                ("word_id", card.word_id.to_string()),
                ("due_at", due_at.clone()),
                ("interval_days", interval_days.to_string()),
                ("ease", ease.to_string()),
                ("reps", reps.to_string()),
                ("lapses", lapses.to_string()),
            ],
        );

        let card_id = card.id.to_string();
        let word_id = card.word_id.to_string();

        client
            .execute(
                "INSERT INTO cards (id, word_id, due_at, interval_days, ease, reps, lapses)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &card_id,
                    &word_id,
                    &due_at,
                    &interval_days,
                    &ease,
                    &reps,
                    &lapses,
                ],
            )
            .map_err(|err| {
                let message = format!("Postgres cards insert failed: {err}");
                crate::db::log_error(&message);
                crate::db::DbError::Config(message)
            })?;

        Ok(())
    }

    fn word_exists(&self, text: &str, language: Language) -> DbResult<bool> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| crate::db::DbError::Config("Postgres client lock poisoned".to_string()))?;
        let rows = client.query(
            "SELECT 1 FROM words WHERE lower(text) = lower($1) AND language = $2 LIMIT 1",
            &[&text, &format!("{:?}", language)],
        )?;
        Ok(!rows.is_empty())
    }

    fn load_all_words(&self) -> DbResult<Vec<Word>> {
        let mut words = Vec::new();
        let mut client = self
            .client
            .lock()
            .map_err(|_| crate::db::DbError::Config("Postgres client lock poisoned".to_string()))?;
        for row in client.query(
            "SELECT id, text, language, translation, chapter, group_name, sentence, created_at
             FROM words
             ORDER BY chapter, group_name, created_at",
            &[],
        )? {
            let language = match row.get::<_, String>(2).as_str() {
                "Dutch" => Language::Dutch,
                _ => Language::English,
            };
            let created_at = DateTime::parse_from_rfc3339(row.get::<_, String>(7).as_str())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            words.push(Word {
                id: Uuid::parse_str(row.get::<_, String>(0).as_str())
                    .unwrap_or_else(|_| Uuid::new_v4()),
                text: row.get(1),
                language,
                translation: row.get(3),
                chapter: row.get(4),
                group: row.get(5),
                sentence: row.get(6),
                created_at,
            });
        }
        Ok(words)
    }

    fn list_chapters(&self) -> DbResult<Vec<String>> {
        let mut chapters = Vec::new();
        let mut client = self
            .client
            .lock()
            .map_err(|_| crate::db::DbError::Config("Postgres client lock poisoned".to_string()))?;
        for row in client.query(
            "SELECT DISTINCT chapter
             FROM words
             WHERE chapter IS NOT NULL AND trim(chapter) != ''
             ORDER BY chapter",
            &[],
        )? {
            let value: String = row.get(0);
            chapters.push(value);
        }
        Ok(chapters)
    }

    fn last_group_for_chapter(&self, chapter: &str) -> DbResult<Option<String>> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| crate::db::DbError::Config("Postgres client lock poisoned".to_string()))?;
        let rows = client.query(
            "SELECT group_name
             FROM words
             WHERE chapter = $1 AND group_name IS NOT NULL AND trim(group_name) != ''
             ORDER BY created_at DESC
             LIMIT 1",
            &[&chapter],
        )?;
        Ok(rows.get(0).map(|row| row.get(0)))
    }

    fn delete_word(&self, word_id: Uuid) -> DbResult<()> {
        let id = word_id.to_string();
        let mut client = self
            .client
            .lock()
            .map_err(|_| crate::db::DbError::Config("Postgres client lock poisoned".to_string()))?;
        client.execute(
            "DELETE FROM reviews WHERE card_id IN (SELECT id FROM cards WHERE word_id = $1)",
            &[&id],
        )?;
        client.execute("DELETE FROM cards WHERE word_id = $1", &[&id])?;
        client.execute("DELETE FROM words WHERE id = $1", &[&id])?;
        Ok(())
    }

    fn delete_all_words(&self) -> DbResult<()> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| crate::db::DbError::Config("Postgres client lock poisoned".to_string()))?;
        client.batch_execute(
            "DELETE FROM reviews;
             DELETE FROM cards;
             DELETE FROM words;",
        )?;
        Ok(())
    }
}
