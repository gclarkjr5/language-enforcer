use std::path::PathBuf;

use chrono::{DateTime, Utc};
use le_core::{schedule_sm2, Card};
use native_tls::TlsConnector;
use postgres::Client;
use postgres_native_tls::MakeTlsConnector;
use rand::seq::SliceRandom;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::path::BaseDirectory;
use tauri::{command, Manager, State};
use tauri::Emitter;
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

#[derive(Debug, Deserialize)]
struct CorrectionInput {
    word_id: String,
    text: Option<String>,
    translation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WordRow {
    id: String,
    text: String,
    language: String,
    translation: Option<String>,
    chapter: Option<String>,
    group_name: Option<String>,
    sentence: Option<String>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct CardRow {
    id: String,
    word_id: String,
    due_at: String,
    interval_days: i32,
    ease: f64,
    reps: i32,
    lapses: i32,
}

#[derive(Debug, Deserialize)]
struct ReviewRow {
    id: String,
    card_id: String,
    grade: i32,
    reviewed_at: String,
}

#[derive(Debug, Deserialize)]
struct DataApiSnapshot {
    words: Vec<WordRow>,
    cards: Vec<CardRow>,
    reviews: Vec<ReviewRow>,
}

#[derive(Default)]
struct ReviewState {
    queue: Vec<String>,
    session_limit: usize,
}

fn find_seed_db(app: &tauri::AppHandle) -> Option<PathBuf> {
    let candidates = [
        app.path().resolve("words.db", BaseDirectory::Resource).ok(),
        app.path().resolve("data/words.db", BaseDirectory::Resource).ok(),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Ok(mut dir) = std::env::current_dir() {
        for _ in 0..5 {
            let candidate = dir.join("data/words.db");
            if candidate.exists() {
                return Some(candidate);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

fn app_db_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("./data"));
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let db_path = dir.join("words.db");


    if !db_path.exists() && let Some(seed) = find_seed_db(app) {
        std::fs::copy(&seed, &db_path).map_err(|err| err.to_string())?;
    }
    Ok(db_path)
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

fn postgres_url() -> Result<String, String> {
    std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required for Postgres sync".to_string())
}

fn open_postgres() -> Result<Client, String> {
    let url = postgres_url()?;
    let connector = TlsConnector::new().map_err(|err| err.to_string())?;
    let connector = MakeTlsConnector::new(connector);
    Client::connect(&url, connector).map_err(|err| err.to_string())
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

fn log_error(message: &str) {
    let Some(path) = sql_log_path() else {
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
    let db_path = app_db_path(&app)?;
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
    let db_path = app_db_path(&app)?;
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
    let db_path = app_db_path(&app)?;
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
    let mut path = app_db_path(&app)?;
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
fn apply_correction(app: tauri::AppHandle, input: CorrectionInput) -> Result<(), String> {
    if input.text.is_none() && input.translation.is_none() {
        return Ok(());
    }

    let mut client = open_postgres()?;
    let affected = match (input.text.as_ref(), input.translation.as_ref()) {
        (Some(text), Some(translation)) => {
            log_sql(
                "UPDATE words SET text = $1, translation = $2 WHERE id = $3",
                &[
                    ("text", text.to_string()),
                    ("translation", translation.to_string()),
                    ("id", input.word_id.clone()),
                ],
            );
            client.execute(
                "UPDATE words SET text = $1, translation = $2 WHERE id = $3",
                &[text, translation, &input.word_id],
            )
        }
        (Some(text), None) => {
            log_sql(
                "UPDATE words SET text = $1 WHERE id = $2",
                &[("text", text.to_string()), ("id", input.word_id.clone())],
            );
            client.execute("UPDATE words SET text = $1 WHERE id = $2", &[text, &input.word_id])
        }
        (None, Some(translation)) => {
            log_sql(
                "UPDATE words SET translation = $1 WHERE id = $2",
                &[
                    ("translation", translation.to_string()),
                    ("id", input.word_id.clone()),
                ],
            );
            client.execute(
                "UPDATE words SET translation = $1 WHERE id = $2",
                &[translation, &input.word_id],
            )
        }
        (None, None) => Ok(0),
    }
    .map_err(|err| err.to_string())?;

    if affected == 0 {
        return Err("Word not found in Postgres".to_string());
    }

    let db_path = app_db_path(&app)?;
    let conn = open_db(&db_path).map_err(|err| err.to_string())?;
    if let Some(text) = input.text.as_ref() {
        conn.execute(
            "UPDATE words SET text = ?1 WHERE id = ?2",
            params![text, &input.word_id],
        )
        .map_err(|err| err.to_string())?;
    }
    if let Some(translation) = input.translation.as_ref() {
        conn.execute(
            "UPDATE words SET translation = ?1 WHERE id = ?2",
            params![translation, &input.word_id],
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[command]
fn apply_correction_local(app: tauri::AppHandle, input: CorrectionInput) -> Result<(), String> {
    if input.text.is_none() && input.translation.is_none() {
        return Ok(());
    }
    let db_path = app_db_path(&app)?;
    let conn = open_db(&db_path).map_err(|err| err.to_string())?;
    if let Some(text) = input.text.as_ref() {
        conn.execute(
            "UPDATE words SET text = ?1 WHERE id = ?2",
            params![text, &input.word_id],
        )
        .map_err(|err| err.to_string())?;
    }
    if let Some(translation) = input.translation.as_ref() {
        conn.execute(
            "UPDATE words SET translation = ?1 WHERE id = ?2",
            params![translation, &input.word_id],
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[command]
fn refresh_from_postgres(
    app: tauri::AppHandle,
    state: State<'_, Mutex<ReviewState>>,
) -> Result<(i64, i64, i64), String> {
    let mut client = open_postgres()?;
    let db_path = app_db_path(&app)?;
    let mut conn = open_db(&db_path).map_err(|err| err.to_string())?;

    let tx = conn.transaction().map_err(|err| {
        let message = format!("refresh_from_postgres: begin transaction failed: {err}");
        log_error(&message);
        message
    })?;
    let query = "DELETE FROM reviews; DELETE FROM cards; DELETE FROM words;";
    log_sql(query, &[]);
    tx.execute_batch(query).map_err(|err| {
        let message = format!("refresh_from_postgres: clear sqlite tables failed: {err}");
        log_error(&message);
        message
    })?;

    let mut word_count = 0i64;
    let mut card_count = 0i64;
    let mut review_count = 0i64;

    log_sql(
        "SELECT id, text, language, translation, chapter, group_name, sentence, created_at FROM words",
        &[],
    );
    let word_rows = client
        .query(
            "SELECT id, text, language, translation, chapter, group_name, sentence, created_at FROM words",
            &[],
        )
        .map_err(|err| {
            let message = format!("refresh_from_postgres: select words failed: {err}");
            log_error(&message);
            message
        })?;
    for row in word_rows {
        tx.execute(
            "INSERT INTO words (id, text, language, translation, chapter, group_name, sentence, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, String>(2),
                row.get::<_, Option<String>>(3),
                row.get::<_, Option<String>>(4),
                row.get::<_, Option<String>>(5),
                row.get::<_, Option<String>>(6),
                row.get::<_, String>(7),
            ],
        )
        .map_err(|err| {
            let message = format!("refresh_from_postgres: insert word failed: {err}");
            log_error(&message);
            message
        })?;
        word_count += 1;
    }

    log_sql(
        "SELECT id, word_id, due_at, interval_days, ease, reps, lapses FROM cards",
        &[],
    );
    let card_rows = client
        .query(
            "SELECT id, word_id, due_at, interval_days, ease, reps, lapses FROM cards",
            &[],
        )
        .map_err(|err| {
            let message = format!("refresh_from_postgres: select cards failed: {err}");
            log_error(&message);
            message
        })?;
    for row in card_rows {
        tx.execute(
            "INSERT INTO cards (id, word_id, due_at, interval_days, ease, reps, lapses, seen_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            params![
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, String>(2),
                row.get::<_, i32>(3),
                row.get::<_, f64>(4),
                row.get::<_, i32>(5),
                row.get::<_, i32>(6),
            ],
        )
        .map_err(|err| {
            let message = format!("refresh_from_postgres: insert card failed: {err}");
            log_error(&message);
            message
        })?;
        card_count += 1;
    }

    log_sql("SELECT id, card_id, grade, reviewed_at FROM reviews", &[]);
    let review_rows = client
        .query("SELECT id, card_id, grade, reviewed_at FROM reviews", &[])
        .map_err(|err| {
            let message = format!("refresh_from_postgres: select reviews failed: {err}");
            log_error(&message);
            message
        })?;
    for row in review_rows {
        tx.execute(
            "INSERT INTO reviews (id, card_id, grade, reviewed_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, i32>(2),
                row.get::<_, String>(3),
            ],
        )
        .map_err(|err| {
            let message = format!("refresh_from_postgres: insert review failed: {err}");
            log_error(&message);
            message
        })?;
        review_count += 1;
    }

    tx.commit().map_err(|err| {
        let message = format!("refresh_from_postgres: commit failed: {err}");
        log_error(&message);
        message
    })?;

    if let Ok(mut guard) = state.lock() {
        guard.queue.clear();
    }

    Ok((word_count, card_count, review_count))
}

#[command]
fn refresh_from_data_api(
    app: tauri::AppHandle,
    state: State<'_, Mutex<ReviewState>>,
    snapshot: DataApiSnapshot,
) -> Result<(i64, i64, i64), String> {
    let db_path = app_db_path(&app)?;
    let mut conn = open_db(&db_path).map_err(|err| err.to_string())?;

    let tx = conn.transaction().map_err(|err| {
        let message = format!("refresh_from_data_api: begin transaction failed: {err}");
        log_error(&message);
        message
    })?;
    let query = "DELETE FROM reviews; DELETE FROM cards; DELETE FROM words;";
    log_sql(query, &[]);
    tx.execute_batch(query).map_err(|err| {
        let message = format!("refresh_from_data_api: clear sqlite tables failed: {err}");
        log_error(&message);
        message
    })?;

    for row in &snapshot.words {
        tx.execute(
            "INSERT INTO words (id, text, language, translation, chapter, group_name, sentence, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.id,
                row.text,
                row.language,
                row.translation,
                row.chapter,
                row.group_name,
                row.sentence,
                row.created_at,
            ],
        )
        .map_err(|err| {
            let message = format!("refresh_from_data_api: insert word failed: {err}");
            log_error(&message);
            message
        })?;
    }

    for row in &snapshot.cards {
        tx.execute(
            "INSERT INTO cards (id, word_id, due_at, interval_days, ease, reps, lapses, seen_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            params![
                row.id,
                row.word_id,
                row.due_at,
                row.interval_days,
                row.ease,
                row.reps,
                row.lapses,
            ],
        )
        .map_err(|err| {
            let message = format!("refresh_from_data_api: insert card failed: {err}");
            log_error(&message);
            message
        })?;
    }

    for row in &snapshot.reviews {
        tx.execute(
            "INSERT INTO reviews (id, card_id, grade, reviewed_at) VALUES (?1, ?2, ?3, ?4)",
            params![row.id, row.card_id, row.grade, row.reviewed_at],
        )
        .map_err(|err| {
            let message = format!("refresh_from_data_api: insert review failed: {err}");
            log_error(&message);
            message
        })?;
    }

    tx.commit().map_err(|err| {
        let message = format!("refresh_from_data_api: commit failed: {err}");
        log_error(&message);
        message
    })?;

    if let Ok(mut guard) = state.lock() {
        guard.queue.clear();
    }

    Ok((
        snapshot.words.len() as i64,
        snapshot.cards.len() as i64,
        snapshot.reviews.len() as i64,
    ))
}

#[command]
fn counts(app: tauri::AppHandle) -> Result<(i64, i64), String> {
    let db_path = app_db_path(&app)?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(Mutex::new(ReviewState {
            queue: Vec::new(),
            session_limit: 10,
        }))
        .invoke_handler(tauri::generate_handler![
            start_session,
            next_due_card,
            grade_card,
            report_issue,
            apply_correction,
            apply_correction_local,
            refresh_from_postgres,
            refresh_from_data_api,
            counts
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let tauri::RunEvent::Opened { urls } = event {
            let payload: Vec<String> = urls.into_iter().map(|url| url.to_string()).collect();
            let _ = app.emit("deep-link", payload);
        }
    });
}
