//! 사용자 데이터 경로와 프로그램(Portable) 경로.
//!
//! 사용자 데이터는 %LOCALAPPDATA%\<DATA_DIR_NAME> 에 두어, Portable 폴더를 옮기거나 새 버전으로
//! 바꿔도 기록과 모델이 유지된다. 프로그램 폴더에는 사용자 데이터를 쓰지 않는다.

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};

/// 데이터 폴더 이름 (저장소 이름과 같게). 기존 구현(%LOCALAPPDATA%\TeacherNotebook)과는 따로 있다.
pub const DATA_DIR_NAME: &str = "teachers_note";
/// 1.0 첫 배포까지 쓰던 데이터 폴더 이름. 켤 때 새 이름으로 옮긴다 (실행기도 같은 규칙).
pub const LEGACY_DIR_NAMES: [&str; 1] = ["TeacherNotebookV2"];

/// base(%LOCALAPPDATA%) 아래 데이터 폴더를 고른다.
/// 새 이름이 있으면 그것을 쓰고, 없고 예전 이름이 있으면 폴더 이름만 바꿔(같은 드라이브라 한 번에) 옮긴다.
/// 이름을 못 바꾸면(예전 버전이 켜져 있는 등) 이번에는 예전 폴더를 그대로 쓰고 다음에 다시 해 본다.
/// 돌려주는 글은 기록(로그)에 남길 내용.
pub fn pick_root(base: &Path) -> (PathBuf, Option<String>) {
    let new = base.join(DATA_DIR_NAME);
    if new.exists() {
        return (new, None);
    }
    for name in LEGACY_DIR_NAMES {
        let old = base.join(name);
        if !old.is_dir() {
            continue;
        }
        return match std::fs::rename(&old, &new) {
            Ok(()) => (new, Some(format!("데이터 폴더 이름을 {name} → {DATA_DIR_NAME} 로 바꿈"))),
            Err(e) => (old, Some(format!("데이터 폴더 이름을 바꾸지 못해 이번에는 {name} 을 그대로 씀: {e}"))),
        };
    }
    (new, None)
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub models_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub rules_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub webview_dir: PathBuf,
    pub settings_json: PathBuf,
    pub wipe_marker: PathBuf,
    pub restore_marker: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: PathBuf) -> AppPaths {
        AppPaths {
            data_dir: root.join("data"),
            db_path: root.join("data").join("notebook.db"),
            models_dir: root.join("models"),
            backups_dir: root.join("backups"),
            rules_dir: root.join("rules"),
            logs_dir: root.join("logs"),
            webview_dir: root.join("webview"),
            settings_json: root.join("settings.json"),
            wipe_marker: root.join("WIPE_REQUESTED"),
            restore_marker: root.join("RESTORE_REQUESTED"),
            root,
        }
    }

    /// 사용자 데이터 위치 (+ 예전 폴더를 옮겼으면 그 기록). 자동 시험에서만 TN_DATA_ROOT 로 바꿀 수 있다.
    pub fn resolve() -> AppResult<(AppPaths, Option<String>)> {
        if let Some(r) = std::env::var_os("TN_DATA_ROOT").filter(|v| !v.is_empty()) {
            return Ok((AppPaths::from_root(PathBuf::from(r)), None));
        }
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local")))
            .ok_or_else(|| AppError::msg("사용자 데이터 폴더(LocalAppData)를 찾지 못했습니다."))?;
        let (root, note) = pick_root(&base);
        Ok((AppPaths::from_root(root), note))
    }

    pub fn ensure(&self) -> AppResult<()> {
        for d in [&self.root, &self.data_dir, &self.models_dir, &self.backups_dir, &self.rules_dir, &self.logs_dir, &self.webview_dir] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }

    /// 전체 삭제 때 지워도 되는 하위 경로 (이 목록 밖은 절대 지우지 않는다)
    pub fn wipe_targets(&self) -> Vec<PathBuf> {
        vec![
            self.data_dir.clone(),
            self.models_dir.clone(),
            self.backups_dir.clone(),
            self.rules_dir.clone(),
            self.logs_dir.clone(),
            self.webview_dir.clone(),
            self.settings_json.clone(),
        ]
    }
}

/// 자동 시험인가: 시험용 데이터 폴더(TN_DATA_ROOT)를 쓰거나, 가짜 LOCALAPPDATA 로 실제 폴더 규칙을 시험할 때(TN_TEST_SANDBOX).
/// 시험 중에는 실제 시작 프로그램 목록과 이미 켜진 교무수첩 창을 건드리지 않는다.
pub fn is_test_run() -> bool {
    ["TN_DATA_ROOT", "TN_TEST_SANDBOX"].iter().any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
}

/// 파일 하나 실행기(바탕화면의 TeacherNotebook.exe)로 켰으면 그 파일 위치
pub fn launcher_path() -> Option<PathBuf> {
    std::env::var_os("TN_LAUNCHER").filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_file())
}

/// 자동 시작 등에 등록할 실행 파일: 실행기로 켰으면 실행기(업데이트로 덮어써도 끊기지 않게), 아니면 이 프로그램
pub fn launch_target() -> std::io::Result<PathBuf> {
    match launcher_path() {
        Some(p) => Ok(p),
        None => std::env::current_exe(),
    }
}

/// 실행 파일이 있는 폴더 (Portable 폴더)
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// llama.cpp 런타임 폴더를 실행 파일 위치 기준으로 찾는다.
pub fn runtime_dir() -> Option<PathBuf> {
    let mut candidates = vec![exe_dir().join("runtime").join("llama.cpp")];
    if let Some(v) = std::env::var_os("TN_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        candidates.insert(0, PathBuf::from(v));
    }
    if cfg!(debug_assertions) {
        // 개발 중(cargo run)에는 src-tauri/runtime 을 쓴다
        candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime").join("llama.cpp"));
    }
    candidates.into_iter().find(|d| d.join(server_exe_name()).is_file())
}

pub fn server_exe_name() -> &'static str {
    if cfg!(windows) { "llama-server.exe" } else { "llama-server" }
}

/// 경로가 root 아래에 있는지 (정규화 후 비교)
pub fn is_within(root: &Path, p: &Path) -> bool {
    match (root.canonicalize(), p.canonicalize()) {
        (Ok(r), Ok(c)) => c.starts_with(&r) && c != r,
        _ => false,
    }
}

/// 사용자 데이터 전체 삭제 (프로그램 시작 직후, DB 를 열기 전에만 호출한다)
pub fn perform_wipe(paths: &AppPaths) -> AppResult<Vec<String>> {
    let mut removed = Vec::new();
    if !paths.wipe_marker.exists() {
        return Ok(removed);
    }
    for t in paths.wipe_targets() {
        if !t.exists() {
            continue;
        }
        if !is_within(&paths.root, &t) {
            continue;
        }
        let res = if t.is_dir() { std::fs::remove_dir_all(&t) } else { std::fs::remove_file(&t) };
        match res {
            Ok(()) => removed.push(t.display().to_string()),
            Err(e) => return Err(AppError::msg(format!("{} 을(를) 지우지 못했습니다: {e}", t.display()))),
        }
    }
    // 지운 백업으로 되돌리려는 예약도 함께 없앤다
    let _ = std::fs::remove_file(&paths.restore_marker);
    std::fs::remove_file(&paths.wipe_marker)?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_data_folder_is_renamed_once() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        // 처음 쓰는 PC: 새 이름
        assert_eq!(pick_root(base), (base.join("teachers_note"), None));
        // 예전 폴더만 있으면 이름을 바꿔 옮긴다 (안의 파일 그대로)
        let old = base.join("TeacherNotebookV2");
        std::fs::create_dir_all(old.join("data")).unwrap();
        std::fs::write(old.join("data").join("notebook.db"), b"db").unwrap();
        let (root, note) = pick_root(base);
        assert_eq!(root, base.join("teachers_note"));
        assert!(note.is_some_and(|n| n.contains("바꿈")));
        assert_eq!(std::fs::read(root.join("data").join("notebook.db")).unwrap(), b"db");
        assert!(!old.exists());
        // 다음부터는 새 폴더를 그대로 (예전 이름 폴더가 또 생겨도 건드리지 않음)
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(pick_root(base), (base.join("teachers_note"), None));
        assert!(old.exists());
    }

    #[test]
    fn wipe_only_touches_allowed_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("teachers_note");
        let paths = AppPaths::from_root(root.clone());
        paths.ensure().unwrap();
        std::fs::write(paths.db_path.clone(), b"db").unwrap();
        std::fs::write(paths.models_dir.join("m.gguf"), b"m").unwrap();
        std::fs::write(&paths.settings_json, b"{}").unwrap();
        // 루트 밖의 형제 폴더는 절대 지우면 안 된다
        let sibling = dir.path().join("TeacherNotebook");
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sibling.join("notebook.db"), b"old").unwrap();
        // 표식이 없으면 아무것도 지우지 않는다
        assert!(perform_wipe(&paths).unwrap().is_empty());
        assert!(paths.db_path.exists());
        std::fs::write(&paths.wipe_marker, b"1").unwrap();
        let removed = perform_wipe(&paths).unwrap();
        assert!(!removed.is_empty());
        assert!(!paths.db_path.exists());
        assert!(!paths.models_dir.exists());
        assert!(!paths.wipe_marker.exists());
        assert!(sibling.join("notebook.db").exists());
    }
}
