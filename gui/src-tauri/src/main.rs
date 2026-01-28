#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use le_core::{schedule_sm2, Card, Language, Word};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use rand::seq::SliceRandom;
use std::sync::Mutex;
use tauri::{command, Manager, State};
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct ReviewItem {
    card_id: String,
    word_id: String,
    text: String,
    translation: Option<String>,
    language: String,
    due_at: String,
    chapter: Option<String>,
    group: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GradeInput {
    card_id: String,
    grade: u8,
}

#[derive(Debug, Deserialize, Serialize)]
struct ReportInput {
    card_id: String,
    word_id: String,
    text: String,
    translation: Option<String>,
    note: Option<String>,
    reported_at: String,
}

#[derive(Default)]
struct ReviewState {
    queue: Vec<String>,
    session_limit: usize,
}

fn app_db_path(app: &tauri::AppHandle) -> PathBuf {
    let local = PathBuf::from("/Users/6148139/personal/language-enforcer/data/words.db");
    if local.exists() {
        return local;
    }
    let mut dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("./data"));
    if dir.ends_with("data") {
        return dir.join("words.db");
    }
    dir.push("language-enforcer");
    dir.push("words.db");
    dir
}

fn open_db(path: &PathBuf) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
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
            seen_count INTEGER NOT NULL DEFAULT 0,
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
    ensure_seen_count(&conn)?;
    Ok(conn)
}

fn ensure_seen_count(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(cards)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == "seen_count" {
            return Ok(());
        }
    }
    conn.execute("ALTER TABLE cards ADD COLUMN seen_count INTEGER NOT NULL DEFAULT 0", [])?;
    Ok(())
}

#[command]
fn start_session(app: tauri::AppHandle, state: State<'_, Mutex<ReviewState>>) -> Result<(), String> {
    let db_path = app_db_path(&app);
    let conn = open_db(&db_path).map_err(|err| err.to_string())?;
    let now = Utc::now().to_rfc3339();
    let mut stmt = conn
        .prepare(
            "SELECT id FROM cards
             WHERE due_at <= ?1
             ORDER BY due_at",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![now], |row| row.get::<_, String>(0))
        .map_err(|err| err.to_string())?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(|err| err.to_string())?);
    }
    let mut rng = rand::thread_rng();
    ids.shuffle(&mut rng);
    let mut guard = state.lock().map_err(|_| "Failed to lock review state".to_string())?;
    let limit = guard.session_limit;
    guard.queue = ids.into_iter().take(limit).collect();
    Ok(())
}

#[command]
fn next_due_card(app: tauri::AppHandle, state: State<'_, Mutex<ReviewState>>) -> Result<Option<ReviewItem>, String> {
    let db_path = app_db_path(&app);
    let conn = open_db(&db_path).map_err(|err| err.to_string())?;
    let mut guard = state.lock().map_err(|_| "Failed to lock review state".to_string())?;
    let Some(card_id) = guard.queue.pop() else {
        return Ok(None);
    };
    drop(guard);

    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.word_id, c.due_at,
                    w.text, w.translation, w.language, w.chapter, w.group_name
             FROM cards c
             JOIN words w ON w.id = c.word_id
             WHERE c.id = ?1
             LIMIT 1",
        )
        .map_err(|err| err.to_string())?;
    let mut rows = stmt
        .query(params![card_id])
        .map_err(|err| err.to_string())?;
    if let Some(row) = rows.next().map_err(|err| err.to_string())? {
        let item = ReviewItem {
            card_id: row.get::<_, String>(0).map_err(|err| err.to_string())?,
            word_id: row.get::<_, String>(1).map_err(|err| err.to_string())?,
            due_at: row.get::<_, String>(2).map_err(|err| err.to_string())?,
            text: row.get::<_, String>(3).map_err(|err| err.to_string())?,
            translation: row.get::<_, Option<String>>(4).map_err(|err| err.to_string())?,
            language: row.get::<_, String>(5).map_err(|err| err.to_string())?,
            chapter: row.get::<_, Option<String>>(6).map_err(|err| err.to_string())?,
            group: row.get::<_, Option<String>>(7).map_err(|err| err.to_string())?,
        };
        Ok(Some(item))
    } else {
        Ok(None)
    }
}

#[command]
fn grade_card(app: tauri::AppHandle, input: GradeInput, state: State<'_, Mutex<ReviewState>>) -> Result<(), String> {
    let db_path = app_db_path(&app);
    let conn = open_db(&db_path).map_err(|err| err.to_string())?;
    let now = Utc::now();

    let mut stmt = conn
        .prepare(
            "SELECT id, word_id, due_at, interval_days, ease, reps, lapses
             FROM cards WHERE id = ?1",
        )
        .map_err(|err| err.to_string())?;
    let mut rows = stmt
        .query(params![input.card_id])
        .map_err(|err| err.to_string())?;
    let row = rows.next().map_err(|err| err.to_string())?;
    let Some(row) = row else { return Ok(()); };

    let mut card = Card {
        id: Uuid::parse_str(&row.get::<_, String>(0).map_err(|err| err.to_string())?)
            .map_err(|err| err.to_string())?,
        word_id: Uuid::parse_str(&row.get::<_, String>(1).map_err(|err| err.to_string())?)
            .map_err(|err| err.to_string())?,
        due_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(2).map_err(|err| err.to_string())?)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|err| err.to_string())?,
        interval_days: row.get(3).map_err(|err| err.to_string())?,
        ease: row.get(4).map_err(|err| err.to_string())?,
        reps: row.get(5).map_err(|err| err.to_string())?,
        lapses: row.get(6).map_err(|err| err.to_string())?,
    };

    schedule_sm2(&mut card, input.grade, now);

    conn.execute(
        "UPDATE cards SET due_at = ?1, interval_days = ?2, ease = ?3, reps = ?4, lapses = ?5 WHERE id = ?6",
        params![
            card.due_at.to_rfc3339(),
            card.interval_days,
            card.ease,
            card.reps,
            card.lapses,
            card.id.to_string()
        ],
    )
    .map_err(|err| err.to_string())?;

    conn.execute(
        "INSERT INTO reviews (id, card_id, grade, reviewed_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            Uuid::new_v4().to_string(),
            card.id.to_string(),
            input.grade,
            now.to_rfc3339()
        ],
    )
    .map_err(|err| err.to_string())?;

    conn.execute(
        "UPDATE cards SET seen_count = seen_count + 1 WHERE id = ?1",
        params![card.id.to_string()],
    )
    .map_err(|err| err.to_string())?;

    if let Ok(mut guard) = state.lock() {
        guard.queue.retain(|id| id != &input.card_id);
    }

    Ok(())
}

#[command]
fn report_issue(app: tauri::AppHandle, input: ReportInput) -> Result<(), String> {
    let mut path = app_db_path(&app);
    path.pop();
    path.push("reported_issues.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    let line = serde_json::to_string(&input).map_err(|err| err.to_string())?;
    use std::io::Write;
    writeln!(file, "{}", line).map_err(|err| err.to_string())?;
    Ok(())
}

#[command]
fn counts(app: tauri::AppHandle) -> Result<(i64, i64), String> {
    let db_path = app_db_path(&app);
    let conn = open_db(&db_path).map_err(|err| err.to_string())?;
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM cards", [], |row| row.get(0))
        .map_err(|err| err.to_string())?;
    let due: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cards WHERE due_at <= ?1",
            params![Utc::now().to_rfc3339()],
            |row| row.get(0),
        )
        .map_err(|err| err.to_string())?;
    Ok((due, total))
}

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(ReviewState {
            queue: Vec::new(),
            session_limit: 10,
        }))
        .invoke_handler(tauri::generate_handler![
            start_session,
            next_due_card,
            grade_card,
            report_issue,
            counts
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
