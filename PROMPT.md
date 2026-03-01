Language Enforcer is a Rust + Tauri/Svelte flashcard/workflow suite with a Neon-backed auth/data proxy and AI sentence/question generator for Dutch-English practice.

## Acceptance Criteria
- The CLI TUI must load `data/words.db`, support importing via OCR, translating via configurable APIs, and let users review/save/delete cards.
- The GUI must present cards, grading controls, modals for fixing/adding words (and concepts), surface AI-generated sentences/questions, and keep a local SQLite mirror in sync with Neon once signed in.
- An Axum auth server must proxy Neon Auth/Data calls, template OpenAI prompts (generation/grade/question) at B1 level, and expose endpoints the GUI can call.
- Concepts stored in Neon (and mirrored locally) must influence all AI prompts and be editable via the desktop app.
- Documentation must clearly describe the repository layout, running instructions, and how data/sync flows work.

## Constraints
- The project must stay cross-platform (macOS/Linux) for TUI, and leverage Tauri for desktop/mobile with bundled web assets.
- All secrets and API keys should stay in environment variables (`.env` or CI) and should not be committed.
- SQLite mirrors should be self-initializing from `data/words.db`, and the GUI must keep them consistent with Neon via local commands (`add_word_local`, `delete_word_local`).
- AI prompts must aim for CEFR B1-level output and respect any supplied concept or context.

## Features to Replicate
- TUI review/import workflow with OCR, live translation hooks, and local review stats (cards, grades, sessions).
- GUI session flow with special AI cards (translate/create/question), toasts, modals for fix/add/delete operations, concept management, and Neon auth guard rails.
- Auth server with Neon Auth/Data proxy endpoints plus `/ai/generate-sentence`, `/ai/generate-question`, `/ai/grade-sentence`, all wired to OpenAI (model configurable via `OPENAI_MODEL`).
- Concept storage backed by a Neon `concepts` table and mirrored in the local DB, so the Tauri app can annotate/guide AI prompts.
- Deployment helpers (Dockerfile, Fly/Shuttle config) for the auth server and documentation for bundling assets.

## Running the System
- `cargo run -p tui`: launch the OCR/import/review CLI.
- `npm install` + `npm run dev -- --host 0.0.0.0` inside `gui/frontend`, then `cargo tauri dev`/`cargo tauri ios dev` for the GUI.
- `cargo run` (inside `auth-server`) with env vars for Neon URLs, PostgreSQL, `OPENAI_API_KEY`, `ALLOWED_ORIGIN`, etc.
- For signing in with the GUI, point `VITE_AUTH_SERVER_URL` at the auth server (local/ngrok/deployment) and keep the Neon session running.

## Files of Interest
- `tui/src/main.rs`, `tui/src/db/*`, `scripts/vision_ocr.swift` for CLI and OCR logic.
- `gui/frontend/src/App.svelte`, `gui/frontend/src/lib/auth.js`, `gui/src-tauri/src/lib.rs` for the Tauri UI and local sync commands.
- `auth-server/src/main.rs` for Neon proxy logic and AI prompts.
- `data/words.db` as the seeded SQLite deck; `data/config.toml` holds run-time tweaks.
