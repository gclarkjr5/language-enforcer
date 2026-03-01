# Language Enforcer

Language Enforcer is a Rust + Svelte/Tauri stack that keeps a local SuperMemo-style
flashcard database synchronized with Neon/Postgres, layers in AI-assisted sentence
practice, and provides both a command-line TUI workflow and a polished iOS/desktop GUI.

## Repository layout

- `tui/`: the command-line review interface and import workflows (OCR, translation, NeDB
  sync scripts). Run it with `cargo run -p tui`.
- `gui/`: the Tauri-powered desktop/ mobile UI plus Svelte frontend under `gui/frontend`.
  Bundled with a local SQLite mirror of the deck and integrations to the auth server.
- `auth-server/`: lightweight Axum service acting as a proxy between the GUI and Neon
  Auth/Data APIs; also hosts the OpenAI prompts for sentence generation/checking.
- `core/`, `scripts/`, `data/`, etc.: shared logic, helper scripts (Vision OCR, migrations),
  and the seeded SQLite `data/words.db`.

## Running the key pieces

### TUI review flow

1. Install Rust 1.72+ (stable channel).
2. Seed `data/words.db` by copying `data/words.db` or creating a new one.
3. Run `cargo run -p tui` from the repo root and follow the on-screen menu (press `i`
   to load OCR imports, `Ctrl+V` to open the review list, etc.).
4. Configure `TRANSLATION_API_*` env vars when you want live translations during imports
   (e.g., DeepL via `TRANSLATION_API_URL=https://api-free.deepl.com/v2/translate`).

### GUI & mobile app

1. Install Node 18+, run `npm install` inside `gui/frontend`.
2. Start the dev workflow with `npm run dev -- --host 0.0.0.0` and keep Vite running.
3. Run `cargo tauri ios dev` (or `cargo tauri dev` on desktop) to launch the Tauri shell
   that points to the Vite server.
4. The GUI expects `auth-server` to be running locally (see below) and will fall back
   to the bundled SQLite when offline.

### Auth server

1. Copy `.env.example` (or set envs manually). Required values include:
   - `NEON_AUTH_URL`, `NEON_DATA_API_URL`, `DATABASE_URL`, `BIND_ADDR`.
   - `OPENAI_API_KEY` (+ `OPENAI_MODEL`) for sentence/question generation.
   - `ALLOWED_ORIGIN` (ngrok or local URL for the GUI).
2. From `auth-server/`, run `cargo run` (or use the included Docker/Fly configs for
   deployment). It proxies sign-in/sign-up calls and exposes `/ai/*` endpoints used by
   the GUI’s sentence/question flows.

## Data flow and storage

- `data/words.db` seeds the GUI/TUI SQLite mirror; the CLI/Tauri apps copy it on startup.
- The GUI keeps Postgres in sync by invoking the Neon Data API via the auth server when
  signed in, while `add_word_local`/`delete_word_local` keep the local DB consistent.
- `concepts` are stored in both Neon and the local `concepts` table so the GUI can pick
  a random construction per card; the snapshot fetch (`fetchDataApiSnapshot`) now also
  pulls `/concepts` to keep the mirror in sync.
- AI prompts live in `auth-server/src/main.rs` and expect the front-end to provide the
  current word, optional translation hint, and any selected concept.

> **Neon schema note:** create a `concepts` table in your Neon database so these
> entries are shared across devices:
>
> ```sql
> CREATE TABLE IF NOT EXISTS concepts (
>   id TEXT PRIMARY KEY,
>   name TEXT NOT NULL UNIQUE,
>   created_at TEXT NOT NULL
> );
> ```

## Tips

- Use `ngrok http 8787` (or a deployed host) and point `VITE_AUTH_SERVER_URL` at it when
  testing sign-in/AI features from iOS or the bundled app.
- Run `cargo fmt` regularly—the Rust workspace prefers the canonical formatting.
- Changes to `gui/frontend` require `npm run build` (and the packaged assets are bundled
  by Tauri during `cargo tauri ios/dev build`).
