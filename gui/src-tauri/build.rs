fn main() {
    ensure_seed_db();
    tauri_build::build();
}

fn ensure_seed_db() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let data_dir = std::path::Path::new(&manifest_dir).join("../../data");
    if std::fs::create_dir_all(&data_dir).is_err() {
        return;
    }
    let db_path = data_dir.join("words.db");
    if !db_path.exists() {
        let _ = std::fs::File::create(db_path);
    }
}
