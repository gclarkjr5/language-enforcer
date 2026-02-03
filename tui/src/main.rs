use std::fs;
use std::io;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use std::process::Command;

use arboard::Clipboard;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use directories::ProjectDirs;
use le_core::{Language, SessionConfig, Word};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod db;
use crate::db::Db;

const TICK_MS: u64 = 100;
const TRANSLATE_DEBOUNCE_MS: u64 = 400;

fn main() -> io::Result<()> {
    let data_dir = ProjectDirs::from("com", "languageenforcer", "Language Enforcer")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("./data"));
    fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join("words.db");
    let config_path = data_dir.join("config.toml");

    let db = Db::open(&db_path).expect("Error connecting to db");
    db.init().expect("Error initializing db");

    let config = load_config(&config_path)?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let translation_api = TranslationApi::from_env().ok().map(Arc::new);
    let (translation_tx, translation_rx) = mpsc::channel();
    let mut app = App::new(config.session, translation_api, translation_tx, translation_rx);

    let res = run_app(&mut terminal, &db, &mut app);

    disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("error: {err}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    db: &Db,
    app: &mut App,
) -> io::Result<()> {
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, app))?;

        let timeout = TICK_MS.saturating_sub(last_tick.elapsed().as_millis() as u64);
        if event::poll(Duration::from_millis(timeout))? {
            if let Event::Key(key) = event::read()? {
                if handle_key(db, app, key)? {
                    return Ok(());
                }
            }
        }

        if last_tick.elapsed() >= Duration::from_millis(TICK_MS) {
            app.tick();
            last_tick = Instant::now();
        }
    }
}

fn handle_key(db: &Db, app: &mut App, key: KeyEvent) -> io::Result<bool> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }
    if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('a') => {
                app.mode = Mode::AddWord;
                return Ok(false);
            }
            KeyCode::Char('v') => {
                match start_review_list(db, app) {
                    Ok(()) => app.mode = Mode::ReviewList,
                    Err(err) => {
                        app.set_message(format!("Failed to load review list: {err}"));
                        app.mode = Mode::Message;
                    }
                }
                return Ok(false);
            }
            KeyCode::Char('i') => {
                app.start_import();
                return Ok(false);
            }
            KeyCode::Char('o') => {
                app.start_import();
                return Ok(false);
            }
            KeyCode::Char('o') => {
                app.start_import();
                return Ok(false);
            }
            _ => {}
        }
    }

    match app.mode {
        Mode::Menu => handle_menu_key(db, app, key),
        Mode::AddWord => handle_add_key(db, app, key),
        Mode::ReviewList => handle_review_list_key(db, app, key),
        Mode::Import => handle_import_key(db, app, key),
        Mode::ImportPreview => handle_import_preview_key(db, app, key),
        Mode::ChapterSelect => handle_chapter_select_key(db, app, key),
        Mode::Confirm => handle_confirm_key(db, app, key),
        Mode::Message => {
            app.mode = Mode::AddWord;
            Ok(false)
        }
    }
}

fn handle_menu_key(db: &Db, app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Char('q') => Ok(true),
        KeyCode::Char('a') => {
            app.start_add(None);
            Ok(false)
        }
        KeyCode::Char('c') => {
            let mut clipboard = Clipboard::new().ok();
            let text = clipboard
                .as_mut()
                .and_then(|cb| cb.get_text().ok())
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            if text.is_none() {
                app.set_message("Clipboard is empty or unavailable".to_string());
                app.mode = Mode::Message;
            } else {
                app.start_add(text);
            }
            Ok(false)
        }
        KeyCode::Char('v') => {
            match start_review_list(db, app) {
                Ok(()) => app.mode = Mode::ReviewList,
                Err(err) => {
                    app.set_message(format!("Failed to load review list: {err}"));
                    app.mode = Mode::Message;
                }
            }
            Ok(false)
        }
        KeyCode::Char('i') => {
            app.start_import();
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_review_list_key(_db: &Db, app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.mode = Mode::AddWord;
            Ok(false)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.review_list_move(-1);
            Ok(false)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.review_list_move(1);
            Ok(false)
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            app.toggle_review_group();
            Ok(false)
        }
        KeyCode::Char('d') => {
            if let Some(word) = app.current_review_word() {
                let message = format!(
                    "WARNING: Delete '{}' and its translation? This cannot be undone. (y/n)",
                    word.text
                );
                app.set_confirm(ConfirmAction::DeleteWord(word.id), message);
            }
            Ok(false)
        }
        KeyCode::Char('D') => {
            if !app.review_list.is_empty() {
                let message = "WARNING: Delete ALL words and translations? This cannot be undone. (y/n)".to_string();
                app.set_confirm(ConfirmAction::DeleteAll, message);
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_confirm_key(db: &Db, app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(action) = app.confirm_action.take() {
                app.confirm_message = None;
                let result = match action {
                    ConfirmAction::DeleteWord(word_id) => db.delete_word(word_id),
                    ConfirmAction::DeleteAll => db.delete_all_words(),
                };
                if let Err(err) = result {
                    app.set_message(format!("Delete failed: {err}"));
                    app.mode = Mode::Message;
                } else {
                    if let Err(err) = reload_review_list(db, app) {
                        app.set_message(format!("Failed to load review list: {err}"));
                        app.mode = Mode::Message;
                    } else {
                        app.mode = Mode::ReviewList;
                    }
                }
            } else {
                app.mode = Mode::ReviewList;
            }
            Ok(false)
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.confirm_action = None;
            app.confirm_message = None;
            app.mode = Mode::ReviewList;
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_add_key(db: &Db, app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.reset_add_fields();
            Ok(false)
        }
        KeyCode::Tab => {
            app.toggle_add_field();
            Ok(false)
        }
        KeyCode::Enter => {
            let text = app.active_input().trim();
            if text.is_empty() {
                app.set_message("Word cannot be empty".to_string());
                return Ok(false);
            }

            let translation = app.inactive_input().trim();
            if translation.is_empty() {
                app.set_message("Translation cannot be empty".to_string());
                return Ok(false);
            }

            match db.word_exists(text, app.active_language()) {
                Ok(true) => {
                    app.set_message("Word already exists".to_string());
                    return Ok(false);
                }
                Ok(false) => {}
                Err(err) => {
                    app.set_message(format!("Failed to check duplicates: {err}"));
                    return Ok(false);
                }
            }

            if let Err(err) = db.save_word(
                text,
                translation,
                app.active_language(),
                Some("Manual"),
                Some("Vocabulaire"),
            ) {
                app.set_message(format!("Failed to save word: {err}"));
            } else {
                app.set_message("Word saved".to_string());
                app.clear_add_inputs();
            }
            Ok(false)
        }
        KeyCode::Backspace => {
            app.pop_add_char();
            Ok(false)
        }
        KeyCode::Char(ch) => {
            app.push_add_char(ch);
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_import_key(db: &Db, app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::AddWord;
            Ok(false)
        }
        KeyCode::Tab => {
            app.toggle_import_field();
            Ok(false)
        }
        KeyCode::Enter => {
            let chapter = app.import_chapter.trim();
            if app.import_images.is_empty() {
                app.set_message("No images found in img/".to_string());
                return Ok(false);
            }
            let image_name = match app.import_images.get(app.import_selection) {
                Some(name) => name.clone(),
                None => {
                    app.set_message("No image selected".to_string());
                    return Ok(false);
                }
            };
            if chapter.is_empty() {
                let chapters = db.list_chapters()
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
                if chapters.is_empty() {
                    app.set_message("No existing chapters found. Enter a chapter first.".to_string());
                    return Ok(false);
                }
                app.chapter_select_list = chapters;
                app.chapter_select_index = 0;
                app.import_pending_image = Some(image_name);
                app.mode = Mode::ChapterSelect;
                return Ok(false);
            }
            let image_path = PathBuf::from("img").join(&image_name);
            let initial_group = db.last_group_for_chapter(chapter)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            match run_ocr(OcrProviderKind::Vision, &image_path) {
                Ok(lines) => match parse_grouped_items(&lines, initial_group) {
                    Ok(items) => {
                        app.import_preview_items = items;
                        app.import_preview_scroll = 0;
                        app.import_preview_path = Some(image_name);
                        app.mode = Mode::ImportPreview;
                    }
                    Err(err) => app.set_message(format!("Preview failed: {err}")),
                },
                Err(err) => app.set_message(format!("Preview failed: {err}")),
            }
            Ok(false)
        }
        KeyCode::Backspace => {
            app.pop_import_char();
            Ok(false)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.import_field == ImportField::List && !app.import_images.is_empty() {
                app.import_selection = app.import_selection.saturating_sub(1);
            }
            Ok(false)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.import_field == ImportField::List && !app.import_images.is_empty() {
                let max = app.import_images.len().saturating_sub(1);
                app.import_selection = (app.import_selection + 1).min(max);
            }
            Ok(false)
        }
        KeyCode::Char(ch) => {
            app.push_import_char(ch);
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_import_preview_key(db: &Db, app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            app.mode = Mode::Import;
            Ok(false)
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let chapter = app.import_chapter.trim();
            let Some(image_name) = app.import_preview_path.clone() else {
                app.set_message("Missing preview image".to_string());
                app.mode = Mode::Import;
                return Ok(false);
            };
            let image_path = PathBuf::from("img").join(image_name);
            let Some(api) = app.translation_api.as_deref() else {
                app.set_message("Missing TRANSLATION_API_URL for translation".to_string());
                return Ok(false);
            };
            let initial_group = db.last_group_for_chapter(chapter)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            match import_from_image(
                db,
                api,
                &image_path,
                chapter,
                OcrProviderKind::Vision,
                initial_group,
            ) {
                Ok(count) => {
                    app.set_message(format!("Imported {count} words"));
                    app.mode = Mode::Message;
                }
                Err(err) => {
                    app.set_message(format!("Import failed: {err}"));
                    app.mode = Mode::Import;
                }
            }
            Ok(false)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.import_preview_scroll = app.import_preview_scroll.saturating_sub(1);
            Ok(false)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.import_preview_scroll = app.import_preview_scroll.saturating_add(1);
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_chapter_select_key(db: &Db, app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            app.mode = Mode::Import;
            Ok(false)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.chapter_select_index = app.chapter_select_index.saturating_sub(1);
            Ok(false)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if !app.chapter_select_list.is_empty() {
                let max = app.chapter_select_list.len().saturating_sub(1);
                app.chapter_select_index = (app.chapter_select_index + 1).min(max);
            }
            Ok(false)
        }
        KeyCode::Enter => {
            let Some(image_name) = app.import_pending_image.clone() else {
                app.set_message("Missing pending image".to_string());
                app.mode = Mode::Import;
                return Ok(false);
            };
            let Some(chapter) = app.chapter_select_list.get(app.chapter_select_index).cloned() else {
                app.set_message("No chapter selected".to_string());
                app.mode = Mode::Import;
                return Ok(false);
            };
            app.import_chapter = chapter.clone();
            let image_path = PathBuf::from("img").join(&image_name);
            let initial_group = db.last_group_for_chapter(&chapter)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            match run_ocr(OcrProviderKind::Vision, &image_path) {
                Ok(lines) => match parse_grouped_items(&lines, initial_group) {
                    Ok(items) => {
                        app.import_preview_items = items;
                        app.import_preview_scroll = 0;
                        app.import_preview_path = Some(image_name);
                        app.import_pending_image = None;
                        app.mode = Mode::ImportPreview;
                    }
                    Err(err) => app.set_message(format!("Preview failed: {err}")),
                },
                Err(err) => app.set_message(format!("Preview failed: {err}")),
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn ui(frame: &mut ratatui::Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)].as_ref())
        .split(frame.size());

    match app.mode {
        Mode::AddWord => render_add(frame, app, chunks[0]),
        Mode::Menu => frame.render_widget(render_menu(app), chunks[0]),
        Mode::ReviewList => render_review_list(frame, app, chunks[0]),
        Mode::Import => render_import(frame, app, chunks[0]),
        Mode::ImportPreview => render_import_preview(frame, app, chunks[0]),
        Mode::ChapterSelect => render_chapter_select(frame, app, chunks[0]),
        Mode::Confirm => frame.render_widget(render_confirm(app), chunks[0]),
        Mode::Message => frame.render_widget(render_message(app), chunks[0]),
    }
    frame.render_widget(render_footer(app), chunks[1]);
}

fn render_menu(app: &App) -> Paragraph<'_> {
    let mut text = Text::default();
    text.lines.push(Line::from("Language Enforcer"));
    text.lines.push(Line::from(""));
    text.lines.push(Line::from("a - add word"));
    text.lines.push(Line::from("c - add from clipboard"));
    text.lines.push(Line::from("i - import image"));
    text.lines.push(Line::from("v - review list"));
    text.lines.push(Line::from("q - quit"));

    Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Menu"))
        .wrap(Wrap { trim: true })
}

fn render_add(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(3)].as_ref())
        .split(area);

    let mut text = Text::default();
    text.lines.push(Line::from("Add Word"));
    if let Some(message) = &app.message {
        text.lines.push(Line::from(""));
        text.lines.push(Line::from(Span::styled(
            message,
            Style::default().add_modifier(Modifier::BOLD),
        )));
    }

    let header = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Add"))
        .wrap(Wrap { trim: false });
    frame.render_widget(header, chunks[0]);

    let boxes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[1]);

    let active_style = Style::default()
        .fg(Color::Blue)
        .add_modifier(Modifier::BOLD);
    let dutch_active = app.add_field == AddField::Dutch;
    let english_active = app.add_field == AddField::English;

    let dutch = Paragraph::new(app.dutch_input.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Dutch")
                .border_style(if dutch_active {
                    active_style
                } else {
                    Style::default()
                }),
        )
        .wrap(Wrap { trim: false });
    let english = Paragraph::new(app.english_input.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("English")
                .border_style(if english_active {
                    active_style
                } else {
                    Style::default()
                }),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(dutch, boxes[0]);
    frame.render_widget(english, boxes[1]);
}

fn render_import(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(3)].as_ref())
        .split(area);

    let mut text = Text::default();
    text.lines.push(Line::from("Import Image"));
    if let Some(message) = &app.message {
        text.lines.push(Line::from(""));
        text.lines.push(Line::from(Span::styled(
            message,
            Style::default().add_modifier(Modifier::BOLD),
        )));
    }

    let header = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Import"))
        .wrap(Wrap { trim: false });
    frame.render_widget(header, chunks[0]);

    let boxes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)].as_ref())
        .split(chunks[1]);

    let active_style = Style::default()
        .fg(Color::Blue)
        .add_modifier(Modifier::BOLD);
    let chapter_active = app.import_field == ImportField::Chapter;
    let list_active = app.import_field == ImportField::List;

    let chapter = Paragraph::new(app.import_chapter.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Chapter")
                .border_style(if chapter_active {
                    active_style
                } else {
                    Style::default()
                }),
        )
        .wrap(Wrap { trim: false });

    let mut list_text = Text::default();
    if app.import_images.is_empty() {
        list_text.lines.push(Line::from("No images found in img/"));
    } else {
        let available_lines = boxes[1].height.saturating_sub(2) as usize;
        let total = app.import_images.len();
        let mut start = app.import_selection.saturating_sub(available_lines / 2);
        if available_lines > 0 && start + available_lines > total {
            start = total.saturating_sub(available_lines);
        }
        let end = (start + available_lines).min(total);
        for (idx, name) in app.import_images[start..end].iter().enumerate() {
            let global_idx = start + idx;
            let line = format!(
                "{} {}",
                if global_idx == app.import_selection { ">" } else { " " },
                name
            );
            if global_idx == app.import_selection {
                list_text.lines.push(Line::from(Span::styled(
                    line,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
            } else {
                list_text.lines.push(Line::from(line));
            }
        }
    }

    let list = Paragraph::new(list_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Images (img/)")
                .border_style(if list_active {
                    active_style
                } else {
                    Style::default()
                }),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(chapter, boxes[0]);
    frame.render_widget(list, boxes[1]);
}

fn render_import_preview(frame: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(3)].as_ref())
        .split(area);

    let mut header = Text::default();
    header.lines.push(Line::from("Import Preview"));
    if let Some(path) = &app.import_preview_path {
        header.lines.push(Line::from(format!("Image: {}", path)));
    }
    if !app.import_chapter.trim().is_empty() {
        header.lines.push(Line::from(format!("Chapter: {}", app.import_chapter)));
    }
    header.lines.push(Line::from(format!(
        "Items: {}",
        app.import_preview_items.len()
    )));

    let header_widget = Paragraph::new(header)
        .block(Block::default().borders(Borders::ALL).title("Preview"))
        .wrap(Wrap { trim: false });
    frame.render_widget(header_widget, chunks[0]);

    let lines = build_preview_lines(&app.import_preview_items);
    if lines.is_empty() {
        let empty = Paragraph::new("No items parsed.")
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false });
        frame.render_widget(empty, chunks[1]);
        return;
    }

    let body_height = chunks[1].height.saturating_sub(2) as usize;
    let min_col_width = 30u16;
    let max_cols = (chunks[1].width / min_col_width).max(1) as usize;
    let per_page = body_height.saturating_mul(max_cols).max(1);
    let max_start = lines.len().saturating_sub(per_page);
    if app.import_preview_scroll > max_start {
        app.import_preview_scroll = max_start;
    }
    let start = app.import_preview_scroll;
    let end = (start + per_page).min(lines.len());
    let page_lines = &lines[start..end];

    let col_count = ((page_lines.len() + body_height.saturating_sub(1)) / body_height).max(1);
    let col_count = col_count.min(max_cols);
    let constraints = vec![Constraint::Ratio(1, col_count as u32); col_count];
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(chunks[1]);

    for (col_idx, col_area) in cols.iter().enumerate() {
        let mut col_text = Text::default();
        let start_idx = col_idx * body_height;
        let end_idx = (start_idx + body_height).min(page_lines.len());
        for line in &page_lines[start_idx..end_idx] {
            col_text.lines.push(Line::from(line.clone()));
        }
        let widget = Paragraph::new(col_text)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, *col_area);
    }
}

fn render_chapter_select(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut text = Text::default();
    text.lines.push(Line::from("Select Chapter"));
    text.lines.push(Line::from(""));
    if app.chapter_select_list.is_empty() {
        text.lines.push(Line::from("No chapters available."));
    } else {
        let available = area.height.saturating_sub(4) as usize;
        let total = app.chapter_select_list.len();
        let mut start = app.chapter_select_index.saturating_sub(available / 2);
        if available > 0 && start + available > total {
            start = total.saturating_sub(available);
        }
        let end = (start + available).min(total);
        for (idx, chapter) in app.chapter_select_list[start..end].iter().enumerate() {
            let global_idx = start + idx;
            let line = format!(
                "{} {}",
                if global_idx == app.chapter_select_index { ">" } else { " " },
                chapter
            );
            if global_idx == app.chapter_select_index {
                text.lines.push(Line::from(Span::styled(
                    line,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
            } else {
                text.lines.push(Line::from(line));
            }
        }
    }

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Chapter"))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_message(app: &App) -> Paragraph<'_> {
    let message = app.message.clone().unwrap_or_else(|| "".to_string());
    Paragraph::new(message)
        .block(Block::default().borders(Borders::ALL).title("Message"))
        .wrap(Wrap { trim: true })
}

fn render_review_list(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut text = Text::default();
    text.lines.push(Line::from("Review List"));
    text.lines.push(Line::from(""));
    let items = app.review_list_items();
    if items.is_empty() {
        text.lines.push(Line::from("No words saved yet"));
    } else {
        let available_lines = area.height.saturating_sub(2) as usize;
        let item_lines = available_lines.saturating_sub(2);
        let total = items.len();
        let mut start = app.review_list_selection.saturating_sub(item_lines / 2);
        if item_lines > 0 && start + item_lines > total {
            start = total.saturating_sub(item_lines);
        }
        let end = (start + item_lines).min(total);
        for (idx, item) in items[start..end].iter().enumerate() {
            let global_idx = start + idx;
            let (line, is_group) = match item {
                ReviewListItem::Group { key, count, collapsed } => {
                    let marker = if *collapsed { "[+]" } else { "[-]" };
                    (format!("{} {} {} ({})", if global_idx == app.review_list_selection { ">" } else { " " }, marker, key, count), true)
                }
                ReviewListItem::Word { index } => {
                    let word = &app.review_list[*index];
                    let translation = word.translation.as_deref().unwrap_or("?");
                    (
                        format!(
                            "{}   [{}] {} -> {}",
                            if global_idx == app.review_list_selection { ">" } else { " " },
                            language_label(word.language),
                            word.text,
                            translation
                        ),
                        false,
                    )
                }
            };
            let styled = if global_idx == app.review_list_selection {
                let mut style = Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD);
                if is_group {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                Line::from(Span::styled(line, style))
            } else {
                Line::from(line)
            };
            text.lines.push(styled);
        }
    }

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Review"))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_confirm(app: &App) -> Paragraph<'_> {
    let message = app
        .confirm_message
        .as_deref()
        .unwrap_or("WARNING: This action cannot be undone. (y/n)");
    let mut text = Text::default();
    text.lines.push(Line::from(message));
    Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Confirm"))
        .wrap(Wrap { trim: true })
}

fn render_footer(app: &App) -> Paragraph<'_> {
    let info = match app.mode {
        Mode::Menu => "a add | c clipboard | i import | v review list | q quit | Ctrl+A add | Ctrl+O import | Ctrl+V list | Ctrl+Q quit",
        Mode::AddWord => {
            "Enter save | Tab switch | Esc clear | Ctrl+A add | Ctrl+O import | Ctrl+V list | Ctrl+Q quit"
        }
        Mode::ReviewList => "Up/Down or j/k move | Enter/Space toggle | d delete | D delete all | q back | Ctrl+A add | Ctrl+O import | Ctrl+V list | Ctrl+Q quit",
        Mode::Import => "Up/Down or j/k move | Tab focus | Enter preview | Esc cancel",
        Mode::ImportPreview => "Up/Down or j/k scroll | y confirm import | n back | Esc back",
        Mode::ChapterSelect => "Up/Down or j/k move | Enter select | Esc back",
        Mode::Confirm => "y confirm | n cancel",
        Mode::Message => "Any key back | Ctrl+A add | Ctrl+O import | Ctrl+V list | Ctrl+Q quit",
    };

    Paragraph::new(info).block(Block::default().borders(Borders::ALL).title("Control Command Center"))
}

fn language_label(language: Language) -> &'static str {
    match language {
        Language::Dutch => "Dutch",
        Language::English => "English",
    }
}

#[derive(Debug)]
struct App {
    mode: Mode,
    dutch_input: String,
    english_input: String,
    add_field: AddField,
    import_chapter: String,
    import_field: ImportField,
    import_images: Vec<String>,
    import_selection: usize,
    import_preview_items: Vec<ImportItem>,
    import_preview_scroll: usize,
    import_preview_path: Option<String>,
    import_pending_image: Option<String>,
    chapter_select_list: Vec<String>,
    chapter_select_index: usize,
    message: Option<String>,
    confirm_message: Option<String>,
    confirm_action: Option<ConfirmAction>,
    review_list: Vec<Word>,
    review_list_selection: usize,
    review_list_collapsed: HashSet<String>,
    session_config: SessionConfig,
    translation_api: Option<Arc<TranslationApi>>,
    translation_tx: Sender<TranslationResult>,
    translation_rx: Receiver<TranslationResult>,
    translation_in_flight: bool,
    pending_translation: Option<PendingTranslation>,
    last_edit_field: Option<AddField>,
    last_edit_dutch_at: Option<Instant>,
    last_edit_english_at: Option<Instant>,
    last_translated_dutch_source: Option<String>,
    last_translated_english_source: Option<String>,
}

impl App {
    fn new(
        session_config: SessionConfig,
        translation_api: Option<Arc<TranslationApi>>,
        translation_tx: Sender<TranslationResult>,
        translation_rx: Receiver<TranslationResult>,
    ) -> Self {
        Self {
            mode: Mode::AddWord,
            dutch_input: String::new(),
            english_input: String::new(),
            add_field: AddField::Dutch,
            import_chapter: String::new(),
            import_field: ImportField::Chapter,
            import_images: Vec::new(),
            import_selection: 0,
            import_preview_items: Vec::new(),
            import_preview_scroll: 0,
            import_preview_path: None,
            import_pending_image: None,
            chapter_select_list: Vec::new(),
            chapter_select_index: 0,
            message: None,
            confirm_message: None,
            confirm_action: None,
            review_list: Vec::new(),
            review_list_selection: 0,
            review_list_collapsed: HashSet::new(),
            session_config,
            translation_api,
            translation_tx,
            translation_rx,
            translation_in_flight: false,
            pending_translation: None,
            last_edit_field: None,
            last_edit_dutch_at: None,
            last_edit_english_at: None,
            last_translated_dutch_source: None,
            last_translated_english_source: None,
        }
    }

    fn tick(&mut self) {
        self.process_translation();
    }

    fn set_message(&mut self, message: String) {
        self.message = Some(message);
    }

    fn set_confirm(&mut self, action: ConfirmAction, message: String) {
        self.confirm_action = Some(action);
        self.confirm_message = Some(message);
        self.mode = Mode::Confirm;
    }

    fn start_add(&mut self, prefilling: Option<String>) {
        self.reset_add();
        if let Some(text) = prefilling {
            *self.active_input_mut() = text;
            self.mark_edit(self.add_field);
        }
        self.mode = Mode::AddWord;
    }

    fn start_import(&mut self) {
        self.import_chapter.clear();
        self.import_field = ImportField::Chapter;
        self.import_images = list_import_images();
        self.import_selection = 0;
        self.import_preview_items.clear();
        self.import_preview_scroll = 0;
        self.import_preview_path = None;
        self.import_pending_image = None;
        self.chapter_select_list.clear();
        self.chapter_select_index = 0;
        self.mode = Mode::Import;
    }

    fn reset_add(&mut self) {
        self.reset_add_fields();
        self.dutch_input.clear();
        self.english_input.clear();
        self.add_field = AddField::Dutch;
    }

    fn reset_add_fields(&mut self) {
        self.dutch_input.clear();
        self.english_input.clear();
        self.message = None;
        self.reset_translation_state();
    }

    fn toggle_add_field(&mut self) {
        self.add_field = match self.add_field {
            AddField::Dutch => AddField::English,
            AddField::English => AddField::Dutch,
        };
    }

    fn toggle_import_field(&mut self) {
        self.import_field = match self.import_field {
            ImportField::Chapter => ImportField::List,
            ImportField::List => ImportField::Chapter,
        };
    }

    fn push_add_char(&mut self, ch: char) {
        self.active_input_mut().push(ch);
        self.mark_edit(self.add_field);
    }

    fn pop_add_char(&mut self) {
        self.active_input_mut().pop();
        self.mark_edit(self.add_field);
    }

    fn push_import_char(&mut self, ch: char) {
        if self.import_field == ImportField::Chapter {
            self.import_chapter.push(ch);
        }
    }

    fn pop_import_char(&mut self) {
        if self.import_field == ImportField::Chapter {
            self.import_chapter.pop();
        }
    }

    fn active_input(&self) -> &str {
        match self.add_field {
            AddField::Dutch => &self.dutch_input,
            AddField::English => &self.english_input,
        }
    }

    fn active_input_mut(&mut self) -> &mut String {
        match self.add_field {
            AddField::Dutch => &mut self.dutch_input,
            AddField::English => &mut self.english_input,
        }
    }

    fn inactive_input(&self) -> &str {
        match self.add_field {
            AddField::Dutch => &self.english_input,
            AddField::English => &self.dutch_input,
        }
    }

    fn active_language(&self) -> Language {
        match self.add_field {
            AddField::Dutch => Language::Dutch,
            AddField::English => Language::English,
        }
    }

    fn clear_add_inputs(&mut self) {
        self.dutch_input.clear();
        self.english_input.clear();
        self.reset_translation_state();
    }

    fn review_list_move(&mut self, delta: i32) {
        let items = self.review_list_items();
        if items.is_empty() {
            return;
        }
        let len = items.len() as i32;
        let mut idx = self.review_list_selection as i32 + delta;
        if idx < 0 {
            idx = 0;
        } else if idx >= len {
            idx = len - 1;
        }
        self.review_list_selection = idx as usize;
    }

    fn current_review_word(&self) -> Option<&Word> {
        let items = self.review_list_items();
        let item = items.get(self.review_list_selection)?;
        match item {
            ReviewListItem::Word { index } => self.review_list.get(*index),
            _ => None,
        }
    }

    fn toggle_review_group(&mut self) {
        let items = self.review_list_items();
        let item = match items.get(self.review_list_selection) {
            Some(item) => item,
            None => return,
        };
        if let ReviewListItem::Group { key, .. } = item {
            if self.review_list_collapsed.contains(key) {
                self.review_list_collapsed.remove(key);
            } else {
                self.review_list_collapsed.insert(key.clone());
            }
            let new_items = self.review_list_items();
            if new_items.is_empty() {
                self.review_list_selection = 0;
            } else if self.review_list_selection >= new_items.len() {
                self.review_list_selection = new_items.len() - 1;
            }
        }
    }

    fn review_list_items(&self) -> Vec<ReviewListItem> {
        if self.review_list.is_empty() {
            return Vec::new();
        }
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for (idx, word) in self.review_list.iter().enumerate() {
            let key = review_group_key(word);
            if let Some((last_key, items)) = groups.last_mut() {
                if *last_key == key {
                    items.push(idx);
                    continue;
                }
            }
            groups.push((key, vec![idx]));
        }

        let mut items = Vec::new();
        for (key, indices) in groups {
            let collapsed = self.review_list_collapsed.contains(&key);
            let count = indices.len();
            items.push(ReviewListItem::Group {
                key: key.clone(),
                count,
                collapsed,
            });
            if !collapsed {
                for index in indices {
                    items.push(ReviewListItem::Word { index });
                }
            }
        }
        items
    }

    fn mark_edit(&mut self, field: AddField) {
        let now = Instant::now();
        self.last_edit_field = Some(field);
        match field {
            AddField::Dutch => self.last_edit_dutch_at = Some(now),
            AddField::English => self.last_edit_english_at = Some(now),
        }
    }

    fn reset_translation_state(&mut self) {
        self.translation_in_flight = false;
        self.pending_translation = None;
        self.last_edit_field = None;
        self.last_edit_dutch_at = None;
        self.last_edit_english_at = None;
        self.last_translated_dutch_source = None;
        self.last_translated_english_source = None;
    }

    fn process_translation(&mut self) {
        loop {
            match self.translation_rx.try_recv() {
                Ok(result) => {
                    self.translation_in_flight = false;
                    self.apply_translation_result(result);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        if self.translation_in_flight || self.translation_api.is_none() || self.mode != Mode::AddWord {
            return;
        }

        let field = match self.last_edit_field {
            Some(field) => field,
            None => return,
        };

        let (source_text, direction, last_edit_at, last_translated_source) = match field {
            AddField::Dutch => (
                self.dutch_input.clone(),
                TranslateDirection::DutchToEnglish,
                self.last_edit_dutch_at,
                self.last_translated_dutch_source.as_deref(),
            ),
            AddField::English => (
                self.english_input.clone(),
                TranslateDirection::EnglishToDutch,
                self.last_edit_english_at,
                self.last_translated_english_source.as_deref(),
            ),
        };

        let Some(last_edit_at) = last_edit_at else {
            return;
        };

        if last_edit_at.elapsed() < Duration::from_millis(TRANSLATE_DEBOUNCE_MS) {
            return;
        }

        let source_trimmed = source_text.trim();
        if source_trimmed.is_empty() {
            return;
        }

        if last_translated_source == Some(source_trimmed) {
            return;
        }

        let api = match &self.translation_api {
            Some(api) => Arc::clone(api),
            None => return,
        };
        let tx = self.translation_tx.clone();
        let source_owned = source_trimmed.to_string();
        let started_at = Instant::now();
        self.translation_in_flight = true;
        self.pending_translation = Some(PendingTranslation {
            direction,
            source_text: source_owned.clone(),
            started_at,
        });

        thread::spawn(move || {
            let (source_lang, target_lang) = direction.language_codes();
            let result = translate_via_api(&api, &source_owned, source_lang, target_lang);
            let _ = tx.send(TranslationResult {
                direction,
                source_text: source_owned,
                started_at,
                result,
            });
        });
    }

    fn apply_translation_result(&mut self, result: TranslationResult) {
        let Some(pending) = self.pending_translation.take() else {
            return;
        };

        if pending.direction != result.direction || pending.source_text != result.source_text {
            return;
        }

        let target_was_edited = match result.direction {
            TranslateDirection::DutchToEnglish => self.last_edit_english_at,
            TranslateDirection::EnglishToDutch => self.last_edit_dutch_at,
        }
        .map(|edited_at| edited_at > pending.started_at)
        .unwrap_or(false);

        if target_was_edited {
            return;
        }

        let current_source = match result.direction {
            TranslateDirection::DutchToEnglish => self.dutch_input.trim(),
            TranslateDirection::EnglishToDutch => self.english_input.trim(),
        };

        if current_source != pending.source_text {
            return;
        }

        match result.result {
            Ok(translated) => {
                match result.direction {
                    TranslateDirection::DutchToEnglish => {
                        self.english_input = translated;
                        self.last_translated_dutch_source = Some(pending.source_text);
                    }
                    TranslateDirection::EnglishToDutch => {
                        self.dutch_input = translated;
                        self.last_translated_english_source = Some(pending.source_text);
                    }
                }
            }
            Err(err) => {
                self.set_message(format!("Translation failed: {err}"));
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Menu,
    AddWord,
    ReviewList,
    Confirm,
    Import,
    ImportPreview,
    ChapterSelect,
    Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddField {
    Dutch,
    English,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportField {
    Chapter,
    List,
}

#[derive(Debug, Clone, Copy)]
enum ConfirmAction {
    DeleteWord(Uuid),
    DeleteAll,
}

#[derive(Debug, Clone, Copy)]
enum OcrProviderKind {
    Vision,
}

#[derive(Debug, Deserialize, Clone)]
struct OcrLine {
    text: String,
    bbox: OcrBBox,
    confidence: f32,
}

#[derive(Debug, Deserialize, Clone)]
struct OcrBBox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Debug, Clone)]
struct LineEntry {
    text: String,
    x: f32,
    y_top: f32,
    height: f32,
}

#[derive(Debug, Clone)]
struct ColumnBucket {
    center: f32,
    lines: Vec<LineEntry>,
}

impl ColumnBucket {
    fn new(entry: LineEntry) -> Self {
        Self {
            center: entry.x,
            lines: vec![entry],
        }
    }

    fn add(&mut self, entry: LineEntry) {
        let count = self.lines.len() as f32;
        self.center = (self.center * count + entry.x) / (count + 1.0);
        self.lines.push(entry);
    }
}

#[derive(Debug, Clone)]
struct ImportItem {
    text: String,
    group: String,
}

#[derive(Debug, Clone)]
enum ReviewListItem {
    Group { key: String, count: usize, collapsed: bool },
    Word { index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranslateDirection {
    DutchToEnglish,
    EnglishToDutch,
}

impl TranslateDirection {
    fn language_codes(self) -> (&'static str, &'static str) {
        match self {
            TranslateDirection::DutchToEnglish => ("NL", "EN"),
            TranslateDirection::EnglishToDutch => ("EN", "NL"),
        }
    }
}

#[derive(Debug)]
struct PendingTranslation {
    direction: TranslateDirection,
    source_text: String,
    started_at: Instant,
}

#[derive(Debug)]
struct TranslationResult {
    direction: TranslateDirection,
    source_text: String,
    started_at: Instant,
    result: Result<String, String>,
}

#[derive(Debug)]
struct TranslationApi {
    client: reqwest::blocking::Client,
    url: String,
    auth_header: Option<String>,
    auth_value: Option<String>,
}

impl TranslationApi {
    fn from_env() -> Result<Self, String> {
        let url = std::env::var("TRANSLATION_API_URL")
            .map_err(|_| "Missing TRANSLATION_API_URL environment variable".to_string())?;
        let auth_key = std::env::var("TRANSLATION_API_KEY").ok();
        let auth_header = std::env::var("TRANSLATION_API_AUTH_HEADER").ok();

        let (header_name, header_value) = match auth_key {
            Some(key) => {
                let header = auth_header.unwrap_or_else(|| "Authorization".to_string());
                let value = if header.eq_ignore_ascii_case("Authorization") {
                    format!("DeepL-Auth-Key {}", key)
                } else {
                    key
                };
                (Some(header), Some(value))
            }
            None => (None, None),
        };

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|err| format!("Failed to build HTTP client: {err}"))?;

        Ok(Self {
            client,
            url,
            auth_header: header_name,
            auth_value: header_value,
        })
    }
}

#[derive(Debug, Serialize)]
struct TranslateRequest<'a> {
    text: Vec<&'a str>,
    source_lang: &'a str,
    target_lang: &'a str,
}

#[derive(Debug, Deserialize)]
struct TranslateResponse {
    translations: Vec<TranslationItem>,
}

#[derive(Debug, Deserialize)]
struct TranslationItem {
    text: String,
}

fn translate_via_api(
    api: &TranslationApi,
    text: &str,
    source_lang: &str,
    target_lang: &str,
) -> Result<String, String> {
    let translations = translate_batch_via_api(api, &[text], source_lang, target_lang)?;
    translations
        .into_iter()
        .next()
        .ok_or_else(|| "API response missing translations".to_string())
}

fn translate_batch_via_api(
    api: &TranslationApi,
    texts: &[&str],
    source_lang: &str,
    target_lang: &str,
) -> Result<Vec<String>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let payload = TranslateRequest {
        text: texts.to_vec(),
        source_lang,
        target_lang,
    };
    let mut request = api.client.post(&api.url).json(&payload);
    if let (Some(header), Some(value)) = (&api.auth_header, &api.auth_value) {
        request = request.header(header, value);
    }
    let response = request
        .send()
        .map_err(|err| format!("Failed to call translation API: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(format!("Translation API error ({status}): {body}"));
    }

    let response: TranslateResponse = response
        .json()
        .map_err(|err| format!("Invalid API response: {err}"))?;
    if response.translations.len() != texts.len() {
        return Err("Translation API response count mismatch".to_string());
    }
    Ok(response.translations.into_iter().map(|item| item.text).collect())
}

#[derive(Debug, Serialize, Deserialize)]
struct ConfigFile {
    session: SessionConfig,
}

fn load_config(path: &Path) -> io::Result<ConfigFile> {
    if path.exists() {
        let content = fs::read_to_string(path)?;
        let cfg: ConfigFile = toml::from_str(&content).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        Ok(cfg)
    } else {
        let cfg = ConfigFile {
            session: SessionConfig::default(),
        };
        let content = toml::to_string_pretty(&cfg)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        fs::write(path, content)?;
        Ok(cfg)
    }
}


fn start_review_list(db: &Db, app: &mut App) -> rusqlite::Result<()> {
    app.review_list = db.load_all_words()?;
    app.review_list_selection = 0;
    Ok(())
}

fn reload_review_list(db: &Db, app: &mut App) -> rusqlite::Result<()> {
    let words = db.load_all_words()?;
    app.review_list = words;
    if app.review_list.is_empty() {
        app.review_list_selection = 0;
    } else {
        let items = app.review_list_items();
        if items.is_empty() {
            app.review_list_selection = 0;
        } else if app.review_list_selection >= items.len() {
            app.review_list_selection = items.len() - 1;
        }
    }
    Ok(())
}


fn review_group_key(word: &Word) -> String {
    let chapter = word.chapter.as_deref().unwrap_or("Unassigned");
    let group = word.group.as_deref().unwrap_or("Ungrouped");
    if chapter.is_empty() && group.is_empty() {
        "Ungrouped".to_string()
    } else if chapter.is_empty() {
        group.to_string()
    } else if group.is_empty() {
        chapter.to_string()
    } else {
        format!("{chapter} / {group}")
    }
}

fn list_import_images() -> Vec<String> {
    let mut images = Vec::new();
    let Ok(entries) = fs::read_dir("img") else {
        return images;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|value| value.to_str()) {
            let ext = ext.to_ascii_lowercase();
            if ext == "jpg" || ext == "jpeg" || ext == "png" {
                if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
                    images.push(name.to_string());
                }
            }
        }
    }
    images.sort();
    images
}

fn build_preview_lines(items: &[ImportItem]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut last_group: Option<&str> = None;
    for item in items {
        if last_group != Some(item.group.as_str()) {
            last_group = Some(item.group.as_str());
            lines.push(format!("[{}]", item.group));
        }
        lines.push(format!("  - {}", item.text));
    }
    lines
}

fn import_from_image(
    db: &Db,
    api: &TranslationApi,
    image_path: &Path,
    chapter: &str,
    provider: OcrProviderKind,
    initial_group: Option<String>,
) -> Result<usize, String> {
    let lines = run_ocr(provider, image_path)?;
    let items = parse_grouped_items(&lines, initial_group)?;
    if items.is_empty() {
        return Ok(0);
    }

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    let chunk_size = 25usize;
    let mut index = 0usize;
    while index < items.len() {
        let end = (index + chunk_size).min(items.len());
        let chunk = &items[index..end];
        let texts: Vec<&str> = chunk.iter().map(|item| item.text.as_str()).collect();
        let translations = translate_batch_via_api(api, &texts, "NL", "EN")?;
        for (item, translation) in chunk.iter().zip(translations) {
            if db
                .word_exists(&item.text, Language::Dutch)
                .map_err(|err| format!("Failed to check duplicates: {err}"))?
            {
                skipped += 1;
                continue;
            }
            db.save_word(
                &item.text,
                &translation,
                Language::Dutch,
                Some(chapter),
                Some(&item.group),
            )
            .map_err(|err| format!("Failed to save word: {err}"))?;
            inserted += 1;
        }
        index = end;
    }

    if skipped > 0 {
        println!("Skipped {skipped} duplicate words.");
    }
    Ok(inserted)
}

fn run_ocr(provider: OcrProviderKind, image_path: &Path) -> Result<Vec<OcrLine>, String> {
    match provider {
        OcrProviderKind::Vision => run_vision_ocr(image_path),
    }
}

fn run_vision_ocr(image_path: &Path) -> Result<Vec<OcrLine>, String> {
    if !cfg!(target_os = "macos") {
        return Err("Vision OCR is only supported on macOS".to_string());
    }
    let script_path = PathBuf::from("scripts/vision_ocr.swift");
    if !script_path.exists() {
        return Err("Missing scripts/vision_ocr.swift".to_string());
    }

    let output = Command::new("swift")
        .arg(script_path)
        .arg("--image")
        .arg(image_path)
        .output()
        .map_err(|err| format!("Failed to run vision OCR: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Vision OCR failed: {stderr}"));
    }

    serde_json::from_slice::<Vec<OcrLine>>(&output.stdout)
        .map_err(|err| format!("Failed to parse OCR output: {err}"))
}

fn parse_grouped_items(lines: &[OcrLine], initial_group: Option<String>) -> Result<Vec<ImportItem>, String> {
    let mut entries: Vec<LineEntry> = lines
        .iter()
        .filter_map(|line| {
            let text = line.text.trim();
            if text.is_empty() {
                return None;
            }
            if looks_like_chapter_line(text) || looks_like_page_number(text) {
                return None;
            }
            let x = line.bbox.x;
            let y_top = 1.0 - (line.bbox.y + line.bbox.h);
            Some(LineEntry {
                text: text.to_string(),
                x,
                y_top,
                height: line.bbox.h,
            })
        })
        .collect();

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let median_height = median(entries.iter().map(|entry| entry.height).collect());
    let columns = split_into_columns(&mut entries);

    let mut current_group: Option<String> = initial_group;
    let mut items = Vec::new();
    for mut column in columns {
        column.sort_by(|a, b| a.y_top.partial_cmp(&b.y_top).unwrap_or(std::cmp::Ordering::Equal));
        for entry in column {
            let normalized = normalize_item_text(&entry.text);
            if normalized.is_empty() {
                continue;
            }
            if is_heading(&entry, median_height) {
                current_group = Some(normalize_heading(&normalized));
                continue;
            }
            let group = current_group.clone().unwrap_or_else(|| "Ungrouped".to_string());
            items.push(ImportItem {
                text: normalized,
                group,
            });
        }
    }

    Ok(items)
}

 

fn split_into_columns(entries: &mut [LineEntry]) -> Vec<Vec<LineEntry>> {
    let mut columns: Vec<ColumnBucket> = Vec::new();
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    let threshold = 0.08;

    for entry in sorted {
        let mut best_index: Option<usize> = None;
        let mut best_distance = f32::MAX;
        for (idx, column) in columns.iter().enumerate() {
            let distance = (entry.x - column.center).abs();
            if distance < best_distance {
                best_distance = distance;
                best_index = Some(idx);
            }
        }
        if let Some(idx) = best_index {
            if best_distance <= threshold {
                columns[idx].add(entry);
                continue;
            }
        }
        columns.push(ColumnBucket::new(entry));
    }

    columns.sort_by(|a, b| a.center.partial_cmp(&b.center).unwrap_or(std::cmp::Ordering::Equal));
    columns.into_iter().map(|column| column.lines).collect()
}

fn is_heading(entry: &LineEntry, median_height: f32) -> bool {
    let text = entry.text.trim();
    if text.is_empty() {
        return false;
    }
    if text.contains(',') || text.contains('-') || text.contains('(') || text.contains(')') {
        return false;
    }
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_uppercase() {
        return false;
    }
    if chars.clone().any(|c| c.is_uppercase()) {
        return false;
    }
    if median_height > 0.0 {
        if text.contains(' ') {
            return entry.height >= median_height * 1.15;
        }
        return entry.height >= median_height * 0.8;
    }
    false
}

fn normalize_heading(text: &str) -> String {
    text.trim_end_matches(':').trim().to_string()
}

fn normalize_item_text(text: &str) -> String {
    let trimmed = text.trim();
    let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    let trimmed = trimmed.strip_prefix("• ").unwrap_or(trimmed);
    trimmed.trim().replace('.', ",")
}

fn looks_like_chapter_line(text: &str) -> bool {
    let lowered = text.to_lowercase();
    if lowered.contains("hoofdstuk") || lowered.contains("chapter") || lowered.contains("hoolastuk") {
        return true;
    }
    if lowered.starts_with("hoo") && lowered.contains("stuk") {
        return true;
    }
    false
}

fn looks_like_page_number(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if trimmed.len() <= 3 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    false
}

fn median(mut values: Vec<f32>) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}
