use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::params;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rusqlite::Connection;

pub fn start_watcher(db: Arc<Mutex<Connection>>) {
    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();

        let mut watcher = RecommendedWatcher::new(tx, Config::default()
            .with_poll_interval(Duration::from_secs(5)))
            .expect("Failed to create watcher");

        // Watch user home directory
        if let Some(home) = dirs::home_dir() {
            watcher.watch(&home, RecursiveMode::Recursive).ok();
        }

        // Watch desktop
        if let Some(desktop) = dirs::desktop_dir() {
            watcher.watch(&desktop, RecursiveMode::Recursive).ok();
        }

        for event in rx.into_iter().flatten() {
            match event.kind {
                // New file created → add to index
                EventKind::Create(_) => {
                    for path in &event.paths {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let path_str = path.to_string_lossy().to_string();
                        let kind = if path.is_dir() {
                            "folder"
                        } else if path_str.ends_with(".exe") || path_str.ends_with(".lnk") {
                            "app"
                        } else {
                            "file"
                        };

                        if let Ok(conn) = db.lock() {
                            conn.execute(
                                "INSERT OR IGNORE INTO files (name, path, kind) VALUES (?1, ?2, ?3)",
                                params![name, path_str, kind],
                            ).ok();
                        }
                    }
                }
                // File deleted → remove from index
                EventKind::Remove(_) => {
                    for path in &event.paths {
                        let path_str = path.to_string_lossy().to_string();
                        if let Ok(conn) = db.lock() {
                            conn.execute(
                                "DELETE FROM files WHERE path = ?1",
                                params![path_str],
                            ).ok();
                        }
                    }
                }
                _ => {}
            }
        }
    });
}
