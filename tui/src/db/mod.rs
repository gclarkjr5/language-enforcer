mod postgres;
mod sqlite;

use std::error::Error;
use std::fmt;
use std::path::Path;

use dotenvy::dotenv;
use le_core::{Language, Word};
use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use uuid::Uuid;

pub type DbResult<T> = Result<T, DbError>;

fn log_path() -> Option<String> {
    std::env::var("LOG_SQL_PATH").ok()
}

pub fn log_error(message: &str) {
    let Some(path) = log_path() else {
        return;
    };
    let mut line = String::new();
    line.push_str("[error] ");
    line.push_str(message);
    line.push('\n');
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    Postgres(::postgres::Error),
    Config(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Sqlite(err) => write!(f, "{err}"),
            DbError::Postgres(err) => write!(f, "{err}"),
            DbError::Config(err) => write!(f, "{err}"),
        }
    }
}

impl Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(err: rusqlite::Error) -> Self {
        DbError::Sqlite(err)
    }
}

impl From<::postgres::Error> for DbError {
    fn from(err: ::postgres::Error) -> Self {
        DbError::Postgres(err)
    }
}

pub trait Db {
    fn init(&self) -> DbResult<()>;
    fn save_word(
        &self,
        text: &str,
        translation: &str,
        language: Language,
        chapter: Option<&str>,
        group: Option<&str>,
    ) -> DbResult<()>;
    fn word_exists(&self, text: &str, language: Language) -> DbResult<bool>;
    fn load_all_words(&self) -> DbResult<Vec<Word>>;
    fn list_chapters(&self) -> DbResult<Vec<String>>;
    fn last_group_for_chapter(&self, chapter: &str) -> DbResult<Option<String>>;
    fn delete_word(&self, word_id: Uuid) -> DbResult<()>;
    fn delete_all_words(&self) -> DbResult<()>;
}

pub fn get_db_backend(path: &Path) -> DbResult<Box<dyn Db>> {
    let backend = std::env::var("BACKEND").expect("Must define a BACKEND. postgres/sqlite");
    match backend.as_str() {
        "sqlite" => Ok(Box::new(sqlite::SqliteDb::open(path)?)),
        "postgres" => {
            let url = std::env::var("DATABASE_URL")
                .map_err(|_| DbError::Config("DATABASE_URL is required for postgres".to_string()))?;
            let connector = TlsConnector::new()
                .map_err(|err| DbError::Config(format!("Failed to create TLS connector: {err}")))?;
            let connector = MakeTlsConnector::new(connector);
            Ok(Box::new(postgres::PostgresDb::connect(&url, connector)?))
        }
        other => Err(DbError::Config(format!("Unknown BACKEND '{other}'"))),
    }
}

pub use postgres::PostgresDb;
pub use sqlite::SqliteDb;
