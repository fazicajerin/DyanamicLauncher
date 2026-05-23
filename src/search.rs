use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rusqlite::{Connection, Result, params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use walkdir::WalkDir;
use rayon::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub name:       String,
    pub path:       String,
    pub kind:       ResultKind,
    pub score:      i64,
    pub open_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ResultKind {
    App,
    File,
    Folder,
}

impl ResultKind {
    pub fn icon(&self) -> &'static str {
        match self {
            ResultKind::App    => "🚀",
            ResultKind::File   => "📄",
            ResultKind::Folder => "📁",
        }
    }
}

pub struct SearchEngine {
    db:      Arc<Mutex<Connection>>,
    matcher: SkimMatcherV2,
}

impl SearchEngine {
    pub fn new() -> Result<Self> {
        let db_path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("seno")
            .join("index.db");

        std::fs::create_dir_all(db_path.parent().unwrap()).ok();
        let conn = Connection::open(&db_path)?;

        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS files (
                id         INTEGER PRIMARY KEY,
                name       TEXT NOT NULL,
                path       TEXT NOT NULL UNIQUE,
                kind       TEXT NOT NULL,
                open_count INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_name ON files(name);
        ")?;

        Ok(Self {
            db:      Arc::new(Mutex::new(conn)),
            matcher: SkimMatcherV2::default(),
        })
    }

    // ── Build index (runs once in background) ─────────────────────────────
    pub fn build_index(&self) {
        let roots = get_search_roots();
        let entries: Vec<(String, String, String)> = roots
            .par_iter()
            .flat_map(|root| {
                WalkDir::new(root)
                    .max_depth(8)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        let name = e.file_name().to_string_lossy().to_lowercase();
                        // Skip system/hidden dirs
                        !name.starts_with('.')
                            && !matches!(
                                name.as_str(),
                                "windows" | "$recycle.bin" | "system volume information"
                                    | "__pycache__" | "node_modules" | "target"
                            )
                    })
                    .map(|e| {
                        let path = e.path().to_string_lossy().to_string();
                        let name = e.file_name().to_string_lossy().to_string();
                        let kind = if e.file_type().is_dir() {
                            "folder"
                        } else if path.ends_with(".exe") || path.ends_with(".lnk") {
                            "app"
                        } else {
                            "file"
                        };
                        (name, path, kind.to_string())
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        if let Ok(mut conn) = self.db.lock() {
            let tx = conn.transaction().unwrap();
            for (name, path, kind) in &entries {
                tx.execute(
                    "INSERT OR IGNORE INTO files (name, path, kind) VALUES (?1, ?2, ?3)",
                    params![name, path, kind],
                ).ok();
            }
            tx.commit().ok();
        }
    }

    // ── Fuzzy search ──────────────────────────────────────────────────────
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        if query.trim().is_empty() {
            return vec![];
        }

        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT name, path, kind, open_count FROM files LIMIT 5000")
            .unwrap();

        let rows: Vec<(String, String, String, u32)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let mut results: Vec<SearchResult> = rows
            .into_iter()
            .filter_map(|(name, path, kind, open_count)| {
                self.matcher
                    .fuzzy_match(&name.to_lowercase(), &query.to_lowercase())
                    .map(|score| {
                        let kind = match kind.as_str() {
                            "app"    => ResultKind::App,
                            "folder" => ResultKind::Folder,
                            _        => ResultKind::File,
                        };
                        // Boost score by open count (smart ranking)
                        let boosted = score + (open_count as i64 * 5);
                        // Extra boost for apps
                        let boosted = if kind == ResultKind::App { boosted + 20 } else { boosted };
                        SearchResult { name, path, kind, score: boosted, open_count }
                    })
            })
            .collect();

        results.sort_by(|a, b| b.score.cmp(&a.score));
        results.truncate(10);
        results
    }

    // ── Record open (for smart ranking) ───────────────────────────────────
    pub fn record_open(&self, path: &str) {
        if let Ok(conn) = self.db.lock() {
            conn.execute(
                "UPDATE files SET open_count = open_count + 1 WHERE path = ?1",
                params![path],
            ).ok();
        }
    }

    pub fn db(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.db)
    }
}

// ── Decide where to search ─────────────────────────────────────────────────
fn get_search_roots() -> Vec<PathBuf> {
    let mut roots = vec![];

    // Always include user home
    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    }

    // Start menu apps
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Ok(pd) = std::env::var("PROGRAMDATA") {
        roots.push(PathBuf::from(pd).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }

    // Common drives on Windows
    #[cfg(target_os = "windows")]
    for letter in b'C'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        if std::path::Path::new(&drive).exists() {
            roots.push(PathBuf::from(drive));
        }
    }

    roots
}
