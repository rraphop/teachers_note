//! 기술 로그 (logs\app-YYYYMMDD.log).
//! 학생 이름, 기록 원문, AI 프롬프트·응답은 절대 쓰지 않는다. 기록 번호·오류 종류·소요 시간만 남긴다.

use once_cell::sync::OnceCell;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static DIR: OnceCell<PathBuf> = OnceCell::new();
static LOCK: Mutex<()> = Mutex::new(());

pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    // 14일 지난 로그는 지운다
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(14 * 24 * 3600);
        for e in entries.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).map(|t| t < cutoff).unwrap_or(false);
            if old && e.file_name().to_string_lossy().starts_with("app-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let _ = DIR.set(dir);
}

fn write(level: &str, msg: &str) {
    let Some(dir) = DIR.get() else { return };
    let now = crate::util::now();
    let file = dir.join(format!("app-{}.log", now.format("%Y%m%d")));
    let _g = LOCK.lock();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "{} [{level}] {}", now.format("%Y-%m-%d %H:%M:%S"), msg.replace('\n', " "));
    }
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn warn(msg: &str) {
    write("WARN", msg);
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}
