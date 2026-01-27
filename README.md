# Language Enforcer (TUI)

## Run

1. Install Rust (stable).
2. From the repo root:

```sh
cargo run -p tui
```

Data lives in `data/` (`data/words.db`, `data/config.toml`).

## Import Vocabulary Images (Vision OCR)

You can import a photo of a vocabulary list and have it grouped by headings, translated,
and saved with chapter/group metadata from inside the TUI.

TUI:

- Press `i` from the menu (or `Ctrl+O`) and select an image from `img/`.
- Enter the chapter manually in the import screen before previewing.
- If you leave it blank, you'll be prompted to pick an existing chapter.

Notes:
- OCR is currently implemented via macOS Vision in `scripts/vision_ocr.swift`.
- The OCR provider is intentionally pluggable so other engines can be added later.

## Review List Controls

- `Ctrl+V` open review list
- `Up/Down` or `j/k` move selection
- `d` delete selected entry (with confirmation)
- `D` delete all entries (with confirmation)
- `q` return

## Translation Backend (DeepL example)

The app stores translations locally so reviews work offline. Live, debounced translation
in the Add Word screen pulls from a translation API using environment variables like in `ptrui`:

```sh
TRANSLATION_API_URL="https://api-free.deepl.com/v2/translate" \
TRANSLATION_API_KEY="YOUR_DEEPL_API_KEY" \
TRANSLATION_API_AUTH_HEADER="DeepL-Auth-Key" \
cargo run -p tui
```

Notes:
- `TRANSLATION_API_URL` is required for live translation.
- `TRANSLATION_API_KEY` and `TRANSLATION_API_AUTH_HEADER` are optional; if you use
  DeepL, set the header to `DeepL-Auth-Key`.
