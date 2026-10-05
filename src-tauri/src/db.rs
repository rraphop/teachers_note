//! SQLite 연결과 마이그레이션.
//! WAL, 외래키, busy timeout 을 켜고, 쓰기 트랜잭션은 짧게 유지한다.

use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub const APP_ID: &str = "local-teacher-notebook";

/// (버전, SQL). 새 마이그레이션은 끝에 추가만 한다.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_init.sql")),
    (2, include_str!("../migrations/0002_timetable_roster.sql")),
    (3, include_str!("../migrations/0003_timetable_from_notes.sql")),
    (4, include_str!("../migrations/0004_neis_roster_tracks.sql")),
    (5, include_str!("../migrations/0005_neis_colors.sql")),
    (6, include_str!("../migrations/0006_record_areas.sql")),
    (7, include_str!("../migrations/0007_tt_color_scopes.sql")),
    (8, include_str!("../migrations/0008_memos.sql")),
];

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

    /// 새 버전이 DB 형식을 올리기 전에 지금 데이터를 '업데이트 전' 백업으로 남기고 연다.
    /// (형식 올리기는 트랜잭션이라 실패하면 그대로지만, 새 버전에서 문제가 생겨도 되돌릴 수 있게)
    pub fn open_with_upgrade_backup(path: &Path, backups_dir: &Path) -> AppResult<Db> {
        if path.exists() {
            if let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX) {
                let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
                let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(0);
                if v > 0 && v < latest_version() {
                    match crate::backup::backup_with(&conn, backups_dir, &format!("before-update-v{v}")) {
                        Ok(p) => crate::logging::info(&format!("업데이트 전 백업: {}", p.display())),
                        Err(e) => crate::logging::error(&format!("업데이트 전 백업 실패 (계속 진행): {e}")),
                    }
                }
            }
        }
        Db::open(path)
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

    /// 종료·재시작 직전에 DB 파일을 닫는다 (WAL 을 정리하고, 다음 실행이 곧바로 파일을 옮길 수 있게).
    /// 이후의 호출은 빈 메모리 DB 로 가서 실패할 뿐 파일을 건드리지 않는다.
    pub fn close(&self) {
        let mut g = self.lock();
        if let Ok(mem) = Connection::open_in_memory() {
            let old = std::mem::replace(&mut *g, mem);
            let _ = old.close();
        }
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
    // 표를 다시 만드는 마이그레이션(영역 값 제한 넓히기 등)이 연결된 기록을 지우지 않도록 외래키를 끄고 실행하고,
    // 커밋 전에 외래키가 모두 맞는지 검사한다. (외래키 설정은 트랜잭션 밖에서만 바꿀 수 있다)
    let pending = MIGRATIONS.iter().any(|(v, _)| *v > version);
    if pending {
        conn.execute_batch("PRAGMA foreign_keys = OFF")?;
    }
    let result = run_migrations(conn, version);
    if pending {
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
    }
    result
}

fn run_migrations(conn: &Connection, version: i64) -> AppResult<()> {
    for (v, sql) in MIGRATIONS {
        if *v <= version {
            continue;
        }
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let res = (|| -> AppResult<()> {
            conn.execute_batch(sql)?;
            let broken: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
            if broken > 0 {
                return Err(AppError::msg(format!("데이터 형식을 올리다 연결이 맞지 않는 자료 {broken}건을 찾아 멈추었습니다. 지금 데이터는 그대로입니다.")));
            }
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
    fn migration_6_rebuilds_tables_and_keeps_links() {
        // 0.8(v5) DB 에 관찰·초안·근거 연결을 넣고 0.9(v6)로 올린다 — 표를 다시 만들어도 연결이 남아야 한다
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("n.db");
        {
            let c = Connection::open(&path).unwrap();
            configure(&c).unwrap();
            for (v, sql) in MIGRATIONS.iter().take(5) {
                c.execute_batch(sql).unwrap();
                if *v == 1 {
                    c.execute("INSERT INTO app_meta(key, value) VALUES ('app', ?1)", params![APP_ID]).unwrap();
                }
            }
            c.execute_batch("PRAGMA user_version = 5").unwrap();
            c.execute_batch(
                "INSERT INTO notes(id, raw_text, created_at, updated_at) VALUES (1, '1-1 홍길동 발표함', 'x', 'x');
                 INSERT INTO students(id, name, created_at, updated_at) VALUES (1, '홍길동', 'x', 'x');
                 INSERT INTO observations(id, note_id, student_id, area, content, observed_on, source, created_at, updated_at)
                   VALUES (7, 1, 1, 'subject', '발표함', '2026-09-29', 'ai', 'x', 'x');
                 INSERT INTO record_drafts(id, student_id, area, body, generated_by, reviewed, created_at, updated_at)
                   VALUES (3, 1, 'subject', '발표함.', 'template', 1, 'x', 'x');
                 INSERT INTO record_draft_sources(draft_id, observation_id) VALUES (3, 7);",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        db.with(|c| {
            assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?, latest_version());
            assert_eq!(c.query_row("SELECT COUNT(*) FROM record_draft_sources WHERE draft_id=3 AND observation_id=7", [], |r| r.get::<_, i64>(0))?, 1);
            assert_eq!(c.query_row("SELECT subject || excluded FROM observations WHERE id=7", [], |r| r.get::<_, String>(0))?, "0");
            assert_eq!(c.query_row("SELECT reviewed FROM record_drafts WHERE id=3", [], |r| r.get::<_, i64>(0))?, 1);
            // 새 영역 값과 외래키가 모두 동작한다
            c.execute(
                "INSERT INTO observations(student_id, area, content, observed_on, source, created_at, updated_at) VALUES (1, 'free_semester', '주제선택 발표', '2026-09-30', 'manual', 'x', 'x')",
                [],
            )?;
            assert_eq!(c.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))?, 1);
            c.execute("DELETE FROM observations WHERE id=7", [])?;
            assert_eq!(c.query_row("SELECT COUNT(*) FROM record_draft_sources", [], |r| r.get::<_, i64>(0))?, 0, "근거 연결은 관찰을 지우면 함께 지워진다");
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn migration_7_keeps_colors_and_allows_subject_scope() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("n.db");
        {
            let c = Connection::open(&path).unwrap();
            configure(&c).unwrap();
            for (v, sql) in MIGRATIONS.iter().take(6) {
                c.execute_batch(sql).unwrap();
                if *v == 1 {
                    c.execute("INSERT INTO app_meta(key, value) VALUES ('app', ?1)", params![APP_ID]).unwrap();
                }
            }
            c.execute_batch("PRAGMA user_version = 6; INSERT INTO timetable_colors(scope, target, color) VALUES ('class', '1-1', 'blue'), ('room', '과학실', 'green');").unwrap();
            assert!(c.execute("INSERT INTO timetable_colors(scope, target, color) VALUES ('subject', '국어', 'red')", []).is_err(), "v6 에서는 교과 색을 넣을 수 없다");
        }
        let db = Db::open(&path).unwrap();
        db.with(|c| {
            assert_eq!(c.query_row("SELECT COUNT(*) FROM timetable_colors", [], |r| r.get::<_, i64>(0))?, 2);
            c.execute("INSERT INTO timetable_colors(scope, target, color) VALUES ('subject', '국어', 'red'), ('teacher', '김선생', 'teal')", [])?;
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn migration_8_adds_memos_and_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("n.db");
        {
            let c = Connection::open(&path).unwrap();
            configure(&c).unwrap();
            for (v, sql) in MIGRATIONS.iter().take(7) {
                c.execute_batch(sql).unwrap();
                if *v == 1 {
                    c.execute("INSERT INTO app_meta(key, value) VALUES ('app', ?1)", params![APP_ID]).unwrap();
                }
            }
            c.execute_batch("PRAGMA user_version = 7; INSERT INTO timetable_colors(scope, target, color) VALUES ('subject', '국어', 'red');").unwrap();
        }
        let db = Db::open(&path).unwrap();
        db.with(|c| {
            assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?, 8);
            assert_eq!(c.query_row("SELECT COUNT(*) FROM timetable_colors", [], |r| r.get::<_, i64>(0))?, 1);
            assert_eq!(c.query_row("SELECT COUNT(*) FROM memos", [], |r| r.get::<_, i64>(0))?, 0);
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn migration_2_merges_duty_fields() {
        // 0.1 DB 를 만들고 업무 세 칸을 채운 뒤 0.2 로 올린다
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("n.db");
        {
            let c = Connection::open(&path).unwrap();
            configure(&c).unwrap();
            c.execute_batch(MIGRATIONS[0].1).unwrap();
            c.execute("INSERT INTO app_meta(key, value) VALUES ('app', ?1)", params![APP_ID]).unwrap();
            c.execute_batch("PRAGMA user_version = 1").unwrap();
            c.execute(
                "INSERT INTO duties(name, detail, guide, keywords, created_at, updated_at) VALUES ('교무기획','학사일정 관리','학사력 작성·변경','수업일수','x','x'), ('정보','','','정보공시','x','x')",
                [],
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let rows: Vec<(String, String, String)> = db
            .with(|c| {
                let mut st = c.prepare("SELECT detail, guide, keywords FROM duties ORDER BY id")?;
                let r = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<Vec<_>, _>>()?;
                Ok(r)
            })
            .unwrap();
        assert_eq!(rows[0].0, "학사일정 관리, 학사력 작성·변경, 수업일수");
        assert_eq!(rows[1].0, "정보공시");
        assert!(rows.iter().all(|r| r.1.is_empty() && r.2.is_empty()));
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
                "INSERT INTO notes(raw_text, created_at, updated_at) VALUES ('오늘 3교시 2-1 홍길동 지각', '2026-09-29T09:00:00', '2026-09-29T09:00:00')",
                [],
            )?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM notes_fts WHERE notes_fts MATCH '\"홍길동\"'", [], |r| r.get(0))?;
            assert_eq!(n, 1);
            c.execute("UPDATE notes SET summary='지각 기록' WHERE id=1", [])?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM notes_fts WHERE notes_fts MATCH '\"지각 기록\"'", [], |r| r.get(0))?;
            assert_eq!(n, 1);
            c.execute("DELETE FROM notes WHERE id=1", [])?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM notes_fts WHERE notes_fts MATCH '\"홍길동\"'", [], |r| r.get(0))?;
            assert_eq!(n, 0);
            Ok(())
        })
        .unwrap();
    }
}
