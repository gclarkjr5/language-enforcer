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
5. Set `AUTH_SERVER_URL` (default `http://127.0.0.1:8787`) so the CLI can reach the auth
   server’s AI endpoints, then press `Ctrl+k` from the main menu to batch up to ten existing
   translations to `/ai/cleanup`. You’ll be prompted to accept/reject/skip each AI
   suggestion before it updates your local SQLite mirror (and later syncs to Neon).

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
3. The same service powers the TUI cleanup command with `/ai/cleanup`, so keep your
   OpenAI config in sync so the CLI can fetch translation suggestions from the same
   model.

### Database migrations

Run `scripts/migrate-cleanup-column.sh --sqlite data/words.db` (or point `--postgres`
at your Neon URL) before launching the updated app if you already have a populated
`words` table. The script will skip existing columns and only add `cleanup_at` when needed.

## Data flow and storage

- `data/words.db` seeds the GUI/TUI SQLite mirror; the CLI/Tauri apps copy it on startup.
- The GUI keeps Postgres in sync by invoking the Neon Data API via the auth server when
  signed in, while `add_word_local`/`delete_word_local` keep the local DB consistent.
- `concepts` are stored in both Neon and the local `concepts` table so the GUI can pick
  a random construction per card; the snapshot fetch (`fetchDataApiSnapshot`) now also
  pulls `/concepts` to keep the mirror in sync.
- The GUI/Tauri review queue now stays on a persistent 10-card batch (tracked in the
  local SQLite `batch_meta` table), repeating those cards until the batch hits the mastery
  heuristics before moving to a new batch; the weighted scheduler will still drip older
  cards back in if their ease drops.
- The auth server also exposes `/ai/cleanup`, which the CLI uses to ask OpenAI for
  translation edits such as missing articles, alternate meanings, or more natural phrasing
  before writing the changes locally.
- AI prompts live in `auth-server/src/main.rs` and expect the front-end to provide the
  current word, optional translation hint, and any selected concept.
- `cleanup_at` keeps track of reviewed translations so the cleanup workflow can skip
  repetitions; new databases include it automatically, and `scripts/migrate-cleanup-column.sh`
  can be run to add the column to older SQLite or Postgres instances before switching to
  the updated code.
- `notes` replaces the old `sentence` column in both local and Neon mirrors so the
  AI cleanup notes are stored alongside translations; the migration script also renames
  the column when necessary.

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
- Keep `AUTH_SERVER_URL` synced to the same host whenever you run the TUI cleanup flow.
- Run `cargo fmt` regularly—the Rust workspace prefers the canonical formatting.
- Run `cargo test` to exercise the Rust tests across the workspace after major changes.
- Run `npm run build` inside `gui/frontend` whenever you merge frontend work so the static bundle stays current.
- Changes to `gui/frontend` require `npm run build` (and the packaged assets are bundled
  by Tauri during `cargo tauri ios/dev build`).
