# Language Enforcer Review (Tauri + Svelte)

This is the cross‑platform flashcard review GUI.

## Setup

```sh
cd gui/frontend
npm install
```

## Dev (desktop)

In one terminal:

```sh
cd gui/frontend
npm run dev
```

In another terminal:

```sh
cd gui/src-tauri
cargo tauri dev
```

## Commands wired

Backend commands in `gui/src-tauri/src/main.rs`:

- `next_due_card`
- `grade_card`
- `counts`

The app uses the existing SQLite schema (`data/words.db`).

## UI behavior

- Shows the next due card
- Space/Enter reveals translation
- 1–4 grades the card

When you’re ready, we can add stats, filters by chapter/group, and session settings.

## iOS setup

export APPLE_DEVELOPMENT_TEAM=YOUR_TEAM_ID
cargo tauri ios init
cargo tauri ios build

running the simulator on a device:

- cargo tauri ios dev
