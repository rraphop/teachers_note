//! 사용자 데이터 경로와 프로그램(Portable) 경로.
//!
//! 사용자 데이터는 %LOCALAPPDATA%\<DATA_DIR_NAME> 에 두어, Portable 폴더를 옮기거나 새 버전으로
//! 바꿔도 기록과 모델이 유지된다. 프로그램 폴더에는 사용자 데이터를 쓰지 않는다.

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};

/// 비교 기간 동안 기존 구현(%LOCALAPPDATA%\TeacherNotebook)의 데이터를 건드리지 않도록 별도 폴더를 쓴다.
/// 명세의 경로로 바꾸려면 이 값만 "TeacherNotebook" 으로 바꾸면 된다.
pub const DATA_DIR_NAME: &str = "TeacherNotebookV2";

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

    /// 사용자 데이터 위치. 자동 시험에서만 TN_DATA_ROOT 로 바꿀 수 있다.
    pub fn resolve() -> AppResult<AppPaths> {
        if let Some(r) = std::env::var_os("TN_DATA_ROOT").filter(|v| !v.is_empty()) {
            return Ok(AppPaths::from_root(PathBuf::from(r)));
        }
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local")))
            .ok_or_else(|| AppError::msg("사용자 데이터 폴더(LocalAppData)를 찾지 못했습니다."))?;
        Ok(AppPaths::from_root(base.join(DATA_DIR_NAME)))
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
    fn wipe_only_touches_allowed_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("TeacherNotebookV2");
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
