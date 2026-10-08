//! 백업과 되돌리기(복원).
//!
//! - 백업: `VACUUM INTO` 로 DB 전체를 backups\notebook-YYYYMMDD-HHMMSS-<종류>.db 에 복사한다.
//!   종류: auto(하루 한 번 자동), manual(지금 백업), before-restore(되돌리기 직전의 데이터)
//! - 되돌리기: 실행 중인 DB 를 건드리지 않도록 표식 파일만 남기고 프로그램을 다시 시작한다.
//!   다음 시작 때 DB 를 열기 전에 ① 백업 사본 검사 ② 지금 데이터를 before-restore 로 백업
//!   ③ 지금 DB 묶음(본 파일·-wal·-shm)을 옆으로 옮김 ④ 사본을 제자리에 넣는다.
//!   어느 단계든 실패하면 원래 DB 를 그대로 둔다.

use crate::db;
use crate::error::{AppError, AppResult};
use crate::logging;
use crate::paths::AppPaths;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub name: String,
    pub size: u64,
    /// 백업한 시각 'YYYY-MM-DD HH:MM'
    pub taken_at: String,
    /// auto | manual | before-restore
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BackupCounts {
    pub version: i64,
    pub notes: i64,
    pub students: i64,
    pub observations: i64,
    pub tasks: i64,
    pub events: i64,
    pub timetable_cells: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreOutcome {
    pub ok: bool,
    pub name: String,
    pub message: String,
    /// 되돌리기 직전 데이터를 담은 백업 파일 이름
    pub before: Option<String>,
}

/// 이번 시작 때 실행한 되돌리기 결과 (화면이 한 번 가져간다)
pub static LAST_RESTORE: Mutex<Option<RestoreOutcome>> = Mutex::new(None);

/// 우리가 만든 백업 파일 이름인지 (경로 구분자·상위 폴더 이동 차단)
pub fn valid_name(name: &str) -> bool {
    name.starts_with("notebook-")
        && name.ends_with(".db")
        && name.len() < 80
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !name.contains("..")
}

fn parse_name(name: &str) -> (String, String) {
    // notebook-20260930-153012-manual.db
    let stem = name.trim_start_matches("notebook-").trim_end_matches(".db");
    let mut parts = stem.splitn(3, '-');
    let (d, t, kind) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let taken = if d.len() == 8 && t.len() >= 4 {
        format!("{}-{}-{} {}:{}", &d[..4], &d[4..6], &d[6..8], &t[..2], &t[2..4])
    } else {
        String::new()
    };
    (taken, kind.to_string())
}

pub fn list(backups_dir: &Path) -> Vec<BackupFile> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(backups_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !valid_name(&name) {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            let (taken_at, kind) = parse_name(&name);
            out.push(BackupFile { name, size, taken_at, kind });
        }
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// 열린 연결로 백업 (실행 중 자동·수동 백업)
pub fn backup_with(conn: &Connection, backups_dir: &Path, kind: &str) -> AppResult<PathBuf> {
    std::fs::create_dir_all(backups_dir)?;
    let now = crate::util::now();
    let mut dest = backups_dir.join(format!("notebook-{}-{kind}.db", now.format("%Y%m%d-%H%M%S")));
    // 같은 초에 두 번 백업하면 뒤에 번호를 붙인다
    let mut n = 2;
    while dest.exists() {
        dest = backups_dir.join(format!("notebook-{}-{kind}_{n}.db", now.format("%Y%m%d-%H%M%S")));
        n += 1;
    }
    conn.execute("VACUUM INTO ?1", params![dest.to_string_lossy().to_string()])?;
    Ok(dest)
}

/// 표 하나라도 내용이 있는지 (기록 없이 명렬표·시간표만 쓰는 경우도 백업한다)
pub fn has_user_data(conn: &Connection) -> AppResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM notes) + (SELECT COUNT(*) FROM students) + (SELECT COUNT(*) FROM events)
              + (SELECT COUNT(*) FROM tasks) + (SELECT COUNT(*) FROM duties) + (SELECT COUNT(*) FROM timetable_cells)
              + (SELECT COUNT(*) FROM bell_schedules)",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// DB 파일 묶음(본 파일, -wal, -shm)
fn db_set(p: &Path) -> [PathBuf; 3] {
    let s = p.as_os_str().to_owned();
    let with = |suffix: &str| {
        let mut x = s.clone();
        x.push(suffix);
        PathBuf::from(x)
    };
    [p.to_path_buf(), with("-wal"), with("-shm")]
}

fn remove_db_files(p: &Path) {
    for f in db_set(p) {
        let _ = std::fs::remove_file(f);
    }
}

/// 이전 프로그램이 아직 끝나는 중이면 파일이 잠겨 있을 수 있어 잠깐씩 다시 시도한다
fn retry<T>(mut f: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut last = None;
    for _ in 0..50 {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("알 수 없는 오류")))
}

/// from 묶음을 to 묶음으로 옮긴다. 중간에 실패하면 옮긴 것을 되돌린다.
fn move_db_set(from: &Path, to: &Path) -> AppResult<()> {
    let (src, dst) = (db_set(from), db_set(to));
    let mut moved: Vec<usize> = Vec::new();
    for i in 0..3 {
        if !src[i].exists() {
            continue;
        }
        if let Err(e) = retry(|| std::fs::rename(&src[i], &dst[i])) {
            for j in moved.into_iter().rev() {
                let _ = std::fs::rename(&dst[j], &src[j]);
            }
            return Err(AppError::msg(format!("데이터 파일을 옮기지 못했습니다: {e}")));
        }
        moved.push(i);
    }
    Ok(())
}

/// 백업 사본이 이 프로그램의 온전한 DB 인지 검사하고 들어 있는 양을 센다
pub fn inspect_file(path: &Path) -> AppResult<BackupCounts> {
    let not_ours = || AppError::invalid("교무수첩 백업 파일이 아닙니다.");
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|_| not_ours())?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|_| not_ours())?;
    if version < 1 {
        return Err(not_ours());
    }
    let app: Option<String> =
        conn.query_row("SELECT value FROM app_meta WHERE key='app'", [], |r| r.get(0)).optional().unwrap_or(None);
    if !db::is_our_app(app.as_deref()) {
        return Err(not_ours());
    }
    if version > db::latest_version() {
        return Err(AppError::invalid("더 새로운 버전의 교무수첩에서 만든 백업입니다. 최신 버전 프로그램에서 되돌려 주세요."));
    }
    let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(AppError::invalid("백업 파일이 손상되어 되돌릴 수 없습니다."));
    }
    let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(0);
    Ok(BackupCounts {
        version,
        notes: count("SELECT COUNT(*) FROM notes"),
        students: count("SELECT COUNT(*) FROM students WHERE archived = 0"),
        observations: count("SELECT COUNT(*) FROM observations"),
        tasks: count("SELECT COUNT(*) FROM tasks"),
        events: count("SELECT COUNT(*) FROM events"),
        // 0.1 백업에는 시간표가 없다
        timetable_cells: count("SELECT COUNT(*) FROM timetable_cells"),
    })
}

fn backup_path(paths: &AppPaths, name: &str) -> AppResult<PathBuf> {
    if !valid_name(name) {
        return Err(AppError::invalid("백업 파일 이름이 올바르지 않습니다."));
    }
    let p = paths.backups_dir.join(name);
    if !p.is_file() {
        return Err(AppError::not_found("백업 파일을 찾지 못했습니다."));
    }
    Ok(p)
}

/// 화면의 확인 창용: 사본을 만들어 검사한다 (원본 백업 파일은 열지 않는다)
pub fn inspect(paths: &AppPaths, name: &str) -> AppResult<BackupCounts> {
    let src = backup_path(paths, name)?;
    let tmp = paths.backups_dir.join("inspect.tmp");
    remove_db_files(&tmp);
    std::fs::copy(&src, &tmp)?;
    let res = inspect_file(&tmp);
    remove_db_files(&tmp);
    res
}

/// 되돌리기 예약: 검사를 통과한 백업만 표식에 적는다
pub fn request(paths: &AppPaths, name: &str) -> AppResult<BackupCounts> {
    let counts = inspect(paths, name)?;
    std::fs::write(&paths.restore_marker, name)?;
    logging::warn("백업 되돌리기가 예약되었습니다 (다음 시작 때 실행)");
    Ok(counts)
}

/// 다음 시작 때 DB 를 열기 전에 호출한다
pub fn perform_restore(paths: &AppPaths) -> Option<RestoreOutcome> {
    if !paths.restore_marker.exists() {
        return None;
    }
    let name = std::fs::read_to_string(&paths.restore_marker).unwrap_or_default().trim().to_string();
    // 실패해도 시작할 때마다 반복하지 않도록 표식부터 지운다
    let _ = std::fs::remove_file(&paths.restore_marker);
    let outcome = match restore_now(paths, &name) {
        Ok(before) => {
            logging::info("백업 되돌리기 완료");
            let (taken_at, _) = parse_name(&name);
            RestoreOutcome { ok: true, message: format!("{taken_at}에 만든 백업의 상태로 돌아왔습니다."), name, before }
        }
        Err(e) => {
            logging::error(&format!("백업 되돌리기 실패: {e}"));
            RestoreOutcome {
                ok: false,
                message: format!("백업으로 되돌리지 못했습니다. 지금 데이터는 그대로입니다. ({e})"),
                name,
                before: None,
            }
        }
    };
    Some(outcome)
}

fn restore_now(paths: &AppPaths, name: &str) -> AppResult<Option<String>> {
    let src = backup_path(paths, name)?;
    std::fs::create_dir_all(&paths.data_dir)?;
    // ① 사본을 만들어 검사
    let incoming = paths.data_dir.join("notebook.db.restoring");
    remove_db_files(&incoming);
    std::fs::copy(&src, &incoming)?;
    if let Err(e) = inspect_file(&incoming) {
        remove_db_files(&incoming);
        return Err(e);
    }
    remove_db_files_except_main(&incoming);

    // ② 지금 데이터를 먼저 백업 (이전 프로그램이 끝나는 중이면 busy timeout 동안 기다린다)
    let mut before = None;
    if paths.db_path.exists() {
        let snap = (|| -> AppResult<PathBuf> {
            let c = Connection::open(&paths.db_path)?;
            c.busy_timeout(Duration::from_secs(10))?;
            backup_with(&c, &paths.backups_dir, "before-restore")
        })();
        match snap {
            Ok(p) => before = p.file_name().map(|n| n.to_string_lossy().to_string()),
            Err(e) => logging::warn(&format!("되돌리기 전 백업 실패, 기존 DB 를 notebook.db.replaced 로 보관합니다: {e}")),
        }
    }

    // ③ 지금 DB 묶음을 옆으로 옮기고 ④ 사본을 제자리에
    let replaced = paths.data_dir.join("notebook.db.replaced");
    remove_db_files(&replaced);
    if let Err(e) = move_db_set(&paths.db_path, &replaced) {
        remove_db_files(&incoming);
        return Err(e);
    }
    if let Err(e) = retry(|| std::fs::rename(&incoming, &paths.db_path)) {
        let _ = move_db_set(&replaced, &paths.db_path);
        remove_db_files(&incoming);
        return Err(AppError::msg(format!("백업 파일을 제자리에 넣지 못했습니다: {e}")));
    }
    if before.is_some() {
        remove_db_files(&replaced);
    }
    Ok(before)
}

/// 검사 때 생긴 -wal/-shm 만 지운다
fn remove_db_files_except_main(p: &Path) {
    for f in db_set(p).iter().skip(1) {
        let _ = std::fs::remove_file(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn note_texts(path: &Path) -> Vec<String> {
        let db = Db::open(path).unwrap();
        db.with(|c| {
            let mut st = c.prepare("SELECT raw_text FROM notes ORDER BY id")?;
            let v = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
            Ok(v)
        })
        .unwrap()
    }

    fn add_note(db: &Db, text: &str) {
        db.with(|c| {
            c.execute("INSERT INTO notes(raw_text, created_at, updated_at) VALUES (?1, 'x', 'x')", params![text])?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn names_are_checked() {
        assert!(valid_name("notebook-20260930-153012-manual.db"));
        assert!(valid_name("notebook-20260930-153012-before-restore.db"));
        assert!(!valid_name("..\\notebook-1.db"));
        assert!(!valid_name("notebook-../../x.db"));
        assert!(!valid_name("notebook-a/b.db"));
        assert!(!valid_name("other.db"));
        let (t, k) = parse_name("notebook-20260930-153012-before-restore.db");
        assert_eq!(t, "2026-09-30 15:30");
        assert_eq!(k, "before-restore");
    }

    #[test]
    fn user_data_includes_roster_and_timetable() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.with(|c| has_user_data(c)).unwrap());
        db.with(|c| {
            c.execute("INSERT INTO students(name, created_at, updated_at) VALUES ('홍길동', 'x', 'x')", [])?;
            Ok(())
        })
        .unwrap();
        assert!(db.with(|c| has_user_data(c)).unwrap());
    }

    #[test]
    fn restore_replaces_db_and_keeps_before_copy() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().join("teachers_note"));
        paths.ensure().unwrap();
        let backup_name;
        {
            let db = Db::open(&paths.db_path).unwrap();
            add_note(&db, "백업 전 기록");
            let p = db.with(|c| backup_with(c, &paths.backups_dir, "manual")).unwrap();
            backup_name = p.file_name().unwrap().to_string_lossy().to_string();
            add_note(&db, "백업 뒤에 쓴 기록");
        }
        let counts = request(&paths, &backup_name).unwrap();
        assert_eq!(counts.notes, 1);
        assert!(paths.restore_marker.exists());

        let out = perform_restore(&paths).unwrap();
        assert!(out.ok, "{}", out.message);
        assert!(!paths.restore_marker.exists());
        assert_eq!(note_texts(&paths.db_path), ["백업 전 기록"]);
        // 되돌리기 직전 데이터도 백업으로 남는다
        let before = out.before.unwrap();
        assert!(before.contains("before-restore"));
        let kept = inspect(&paths, &before).unwrap();
        assert_eq!(kept.notes, 2);
        assert!(!paths.data_dir.join("notebook.db.replaced").exists());
        assert!(!paths.data_dir.join("notebook.db.restoring").exists());
        // 표식이 없으면 아무 일도 없다
        assert!(perform_restore(&paths).is_none());
    }

    #[test]
    fn broken_or_foreign_backup_leaves_data_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().join("teachers_note"));
        paths.ensure().unwrap();
        {
            let db = Db::open(&paths.db_path).unwrap();
            add_note(&db, "지금 기록");
        }
        let fake = "notebook-20260101-000000-manual.db";
        std::fs::write(paths.backups_dir.join(fake), b"not a database at all").unwrap();
        assert!(request(&paths, fake).is_err());
        assert!(!paths.restore_marker.exists());
        // 표식을 직접 써도 시작할 때 다시 검사해서 거절한다
        std::fs::write(&paths.restore_marker, fake).unwrap();
        let out = perform_restore(&paths).unwrap();
        assert!(!out.ok);
        assert_eq!(note_texts(&paths.db_path), ["지금 기록"]);
        // 경로 이동 시도
        std::fs::write(&paths.restore_marker, "..\\..\\x.db").unwrap();
        assert!(!perform_restore(&paths).unwrap().ok);
        assert_eq!(note_texts(&paths.db_path), ["지금 기록"]);
    }

    #[test]
    fn upgrade_makes_before_update_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        {
            let c = Connection::open(&paths.db_path).unwrap();
            c.execute_batch(include_str!("../migrations/0001_init.sql")).unwrap();
            c.execute("INSERT INTO app_meta(key, value) VALUES ('app', ?1)", params![db::APP_ID]).unwrap();
            c.execute_batch("PRAGMA user_version = 1").unwrap();
            c.execute("INSERT INTO notes(raw_text, created_at, updated_at) VALUES ('옛 기록', 'x', 'x')", []).unwrap();
        }
        let db = Db::open_with_upgrade_backup(&paths.db_path, &paths.backups_dir).unwrap();
        let b = list(&paths.backups_dir);
        assert_eq!(b.len(), 1);
        assert!(b[0].kind.starts_with("before-update-v1"), "{}", b[0].kind);
        assert_eq!(inspect_file(&paths.backups_dir.join(&b[0].name)).unwrap().notes, 1);
        drop(db);
        // 이미 최신 형식이면 다시 만들지 않는다
        let _db = Db::open_with_upgrade_backup(&paths.db_path, &paths.backups_dir).unwrap();
        assert_eq!(list(&paths.backups_dir).len(), 1);
    }

    #[test]
    fn older_version_backup_is_migrated_after_restore() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().join("teachers_note"));
        paths.ensure().unwrap();
        {
            let db = Db::open(&paths.db_path).unwrap();
            add_note(&db, "지금 기록");
        }
        // 0.1 형식(버전 1) 백업을 만든다
        let name = "notebook-20260929-080000-auto.db";
        {
            let c = Connection::open(paths.backups_dir.join(name)).unwrap();
            c.execute_batch(include_str!("../migrations/0001_init.sql")).unwrap();
            c.execute("INSERT INTO app_meta(key, value) VALUES ('app', ?1)", params![db::APP_ID]).unwrap();
            c.execute("INSERT INTO notes(raw_text, created_at, updated_at) VALUES ('0.1 기록', 'x', 'x')", []).unwrap();
            c.execute_batch("PRAGMA user_version = 1").unwrap();
        }
        let counts = request(&paths, name).unwrap();
        assert_eq!(counts.version, 1);
        assert_eq!(counts.timetable_cells, 0);
        assert!(perform_restore(&paths).unwrap().ok);
        assert_eq!(note_texts(&paths.db_path), ["0.1 기록"]);
        let db = Db::open(&paths.db_path).unwrap();
        let v: i64 = db.with(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?)).unwrap();
        assert_eq!(v, db::latest_version());
    }
}
