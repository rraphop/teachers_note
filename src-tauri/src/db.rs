//! SQLite 연결과 마이그레이션.
//! WAL, 외래키, busy timeout 을 켜고, 쓰기 트랜잭션은 짧게 유지한다.

use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub const APP_ID: &str = "local-teacher-notebook";

/// (버전, SQL). 새 마이그레이션은 끝에 추가만 한다.
const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_init.sql"))];

pub fn latest_version() -> i64 {
    MIGRATIONS.last().map(|m| m.0).unwrap_or(0)
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure(&conn)?;
        migrate(&conn)?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> AppResult<Db> {
        let conn = Connection::open_in_memory()?;
        configure(&conn)?;
        migrate(&conn)?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        // 다른 스레드가 패닉으로 잠금을 남겨도 DB 연결 자체는 계속 쓸 수 있다
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> AppResult<T>) -> AppResult<T> {
        let mut g = self.lock();
        f(&mut g)
    }
}

pub fn configure(conn: &Connection) -> AppResult<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    // 메모리 DB 는 WAL 을 쓰지 않는다
    let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(())
}

pub fn migrate(conn: &Connection) -> AppResult<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version == 0 {
        let tables: i64 =
            conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |r| {
                r.get(0)
            })?;
        if tables > 0 {
            return Err(AppError::msg(
                "데이터 폴더에 이 프로그램이 만들지 않은 데이터베이스가 있습니다. 기존 파일을 보호하기 위해 열지 않았습니다.",
            ));
        }
    } else {
        let app: Option<String> = conn
            .query_row("SELECT value FROM app_meta WHERE key='app'", [], |r| r.get(0))
            .optional()
            .unwrap_or(None);
        if app.as_deref() != Some(APP_ID) {
            return Err(AppError::msg(
                "데이터 폴더에 다른 프로그램의 데이터베이스가 있습니다. 기존 파일을 보호하기 위해 열지 않았습니다.",
            ));
        }
        if version > latest_version() {
            return Err(AppError::msg(
                "더 새로운 버전의 교무수첩에서 만든 데이터입니다. 최신 버전 프로그램으로 실행해 주세요.",
            ));
        }
    }
    for (v, sql) in MIGRATIONS {
        if *v <= version {
            continue;
        }
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let res = (|| -> AppResult<()> {
            conn.execute_batch(sql)?;
            if *v == 1 {
                conn.execute("INSERT INTO app_meta(key, value) VALUES ('app', ?1)", params![APP_ID])?;
                conn.execute(
                    "INSERT INTO app_meta(key, value) VALUES ('created_at', ?1)",
                    params![crate::util::fmt_dt(crate::util::now())],
                )?;
            }
            conn.execute_batch(&format!("PRAGMA user_version = {v}"))?;
            Ok(())
        })();
        match res {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(conn.query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get(0)).optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete_setting(conn: &Connection, key: &str) -> AppResult<()> {
    conn.execute("DELETE FROM settings WHERE key=?1", params![key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("notebook.db");
        {
            let db = Db::open(&path).unwrap();
            db.with(|c| {
                c.execute(
                    "INSERT INTO notes(raw_text, created_at, updated_at) VALUES ('테스트 원문', '2026-09-29T09:00:00', '2026-09-29T09:00:00')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        }
        // 다시 열어도 마이그레이션이 반복되지 않고 데이터가 남아 있다
        let db = Db::open(&path).unwrap();
        let (v, n, mode): (i64, i64, String) = db
            .with(|c| {
                let v = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                let n = c.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?;
                let mode = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
                Ok((v, n, mode))
            })
            .unwrap();
        assert_eq!(v, latest_version());
        assert_eq!(n, 1);
        assert_eq!(mode.to_lowercase(), "wal");
        let fk: i64 = db.with(|c| Ok(c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?)).unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn refuses_foreign_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE something(x); INSERT INTO something VALUES (1);").unwrap();
        }
        assert!(Db::open(&path).is_err());
        // 원래 파일은 그대로 남아 있다
        let c = Connection::open(&path).unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM something", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn fts5_trigram_available() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            c.execute(
                "INSERT INTO notes(raw_text, created_at, updated_at) VALUES ('오늘 3교시 2-1 김민수 지각', '2026-09-29T09:00:00', '2026-09-29T09:00:00')",
                [],
            )?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM notes_fts WHERE notes_fts MATCH '\"김민수\"'", [], |r| r.get(0))?;
            assert_eq!(n, 1);
            c.execute("UPDATE notes SET summary='지각 기록' WHERE id=1", [])?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM notes_fts WHERE notes_fts MATCH '\"지각 기록\"'", [], |r| r.get(0))?;
            assert_eq!(n, 1);
            c.execute("DELETE FROM notes WHERE id=1", [])?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM notes_fts WHERE notes_fts MATCH '\"김민수\"'", [], |r| r.get(0))?;
            assert_eq!(n, 0);
            Ok(())
        })
        .unwrap();
    }
}
