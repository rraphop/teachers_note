//! 로컬 AI 교무수첩 — 파일 하나 실행기 ("교무수첩_1.0.0.exe").
//!
//! 바탕화면에 이 파일 하나만 두고 실행한다. 새 버전은 "교무수첩_1.0.1.exe" 처럼 새 이름으로 받는다.
//! (1.0 까지 쓰던 예전 이름 "교무수첩(0.9).exe" 도 알아보고 정리 대상에 넣는다)
//!
//! 1. 안에 든 프로그램 묶음(화면·AI 엔진 llama.cpp·라이선스)의 파일 목록과 SHA-256 을 읽는다.
//! 2. 더 새로운 버전을 이미 풀어 쓴 적이 있으면(지난 버전 파일을 잘못 켠 경우) 새 버전을 쓰라고 알린다.
//! 3. %LOCALAPPDATA%\teachers_note\program\<버전> 의 파일이 목록과 모두 같으면 바로 실행한다.
//!    (1.0 첫 배포의 예전 폴더 TeacherNotebookV2 가 있으면 먼저 이름을 바꿔 옮긴다)
//!    다르거나 없으면(처음 실행·업데이트 직후·파일 손상) "준비 중" 창을 띄우고 새로 풀어 검사한 뒤 실행한다.
//! 4. 새 버전을 처음 풀었을 때, 같은 폴더의 지난 버전 파일("교무수첩_0.9.0.exe", 예전 이름 "교무수첩(0.9).exe")을
//!    휴지통으로 옮길지 묻는다. 같은 버전이라도 예전 이름이면 함께 묻는다.
//! 5. 더 오래된 버전 폴더만 정리한다. 기록·AI 모델 등 사용자 데이터는 건드리지 않는다.
//!
//! AI 엔진은 따로 도는 프로그램(llama-server.exe)이고, 화면 부품(WebView2Loader.dll)도 파일로 있어야 해서
//! EXE 하나 안에서만 돌 수는 없다. 그래서 풀어서 쓰되, 풀 위치는 사용자 데이터 폴더로 고정하고 매번 검사한다.

#![cfg_attr(windows, windows_subsystem = "windows")]

use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

static PAYLOAD: &[u8] = include_bytes!(env!("TN_PAYLOAD"));
static MANIFEST: &str = include_str!(env!("TN_MANIFEST"));

#[derive(serde::Deserialize)]
struct Manifest {
    version: String,
    /// 풀린 폴더 안의 본체 실행 파일
    main: String,
    files: Vec<Entry>,
}

#[derive(serde::Deserialize)]
struct Entry {
    path: String,
    size: u64,
    sha256: String,
}

const TITLE: &str = "로컬 AI 교무수첩";

fn data_root() -> PathBuf {
    // 자동 시험은 따로 만든 데이터 폴더를 쓴다 (본체와 같은 규칙)
    if let Some(r) = std::env::var_os("TN_DATA_ROOT").filter(|v| !v.is_empty()) {
        return PathBuf::from(r);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local")))
        .unwrap_or_else(std::env::temp_dir);
    pick_root(&base)
}

/// 데이터 폴더 이름 (본체 paths::DATA_DIR_NAME 과 같다)
const DATA_DIR_NAME: &str = "teachers_note";
/// 1.0 첫 배포까지 쓰던 이름
const LEGACY_DIR_NAME: &str = "TeacherNotebookV2";

/// 본체와 같은 규칙: 새 이름 폴더가 있으면 그것, 없고 예전 이름 폴더가 있으면 이름을 바꿔 옮긴다.
/// 못 바꾸면(예전 버전이 켜져 있는 등) 이번에는 예전 폴더를 그대로 쓴다.
fn pick_root(base: &Path) -> PathBuf {
    let new = base.join(DATA_DIR_NAME);
    let old = base.join(LEGACY_DIR_NAME);
    if new.exists() || !old.is_dir() {
        return new;
    }
    match std::fs::rename(&old, &new) {
        Ok(()) => new,
        Err(_) => old,
    }
}

fn rel_path(dir: &Path, rel: &str) -> PathBuf {
    rel.split(['/', '\\']).filter(|s| !s.is_empty()).fold(dir.to_path_buf(), |p, s| p.join(s))
}

fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// 풀린 폴더가 목록과 똑같은지 (크기 먼저, 그다음 SHA-256)
fn verify(dir: &Path, m: &Manifest) -> bool {
    m.files.iter().all(|e| {
        let p = rel_path(dir, &e.path);
        match std::fs::metadata(&p) {
            Ok(md) if md.is_file() && md.len() == e.size => sha256_file(&p).is_ok_and(|h| h.eq_ignore_ascii_case(&e.sha256)),
            _ => false,
        }
    })
}

fn extract(dir: &Path, m: &Manifest) -> Result<(), String> {
    let parent = dir.parent().ok_or("풀 위치가 올바르지 않습니다.")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("폴더를 만들지 못했습니다: {e}"))?;
    let tmp = parent.join(format!(".{}-{}", m.version, std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let res = (|| -> Result<(), String> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(PAYLOAD)).map_err(|e| format!("묶음을 읽지 못했습니다: {e}"))?;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| format!("묶음을 읽지 못했습니다: {e}"))?;
            // 압축 안의 경로가 폴더 밖을 가리키면 거절한다
            let rel = f.enclosed_name().ok_or("묶음 안의 경로가 올바르지 않습니다.")?;
            let out = tmp.join(rel);
            if f.is_dir() {
                std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                continue;
            }
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut w = std::fs::File::create(&out).map_err(|e| format!("파일을 쓰지 못했습니다 ({}): {e}", out.display()))?;
            std::io::copy(&mut f, &mut w).map_err(|e| format!("파일을 쓰지 못했습니다: {e}"))?;
        }
        if !verify(&tmp, m) {
            return Err("풀어 놓은 파일의 검사(SHA-256)가 맞지 않습니다. 파일이 손상되었거나 백신이 일부를 막았을 수 있습니다.".into());
        }
        if dir.exists() {
            // 두 번 눌러 다른 실행기가 먼저 풀어 놓았으면 그것을 쓴다
            if verify(dir, m) {
                let _ = std::fs::remove_dir_all(&tmp);
                return Ok(());
            }
            std::fs::remove_dir_all(dir).map_err(|e| format!("이전 파일을 바꾸지 못했습니다 (교무수첩이 켜져 있으면 닫고 다시 실행해 주세요): {e}"))?;
        }
        std::fs::rename(&tmp, dir).map_err(|e| format!("파일을 제자리에 두지 못했습니다: {e}"))
    })();
    if res.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    res
}

type Ver = (u32, u32, u32);

/// 프로그램 버전 "0.6.1" (뒤에 붙은 "-test" 등은 무시)
fn parse_ver(s: &str) -> Option<Ver> {
    let core = s.split('-').next()?;
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().map(|x| x.parse()).unwrap_or(Ok(0)).ok()?;
    let c = it.next().map(|x| x.parse()).unwrap_or(Ok(0)).ok()?;
    Some((a, b, c))
}

/// 화면에 쓰는 버전: "0.6.1" → "0.61", "0.6.0" → "0.6"
fn display_ver(v: Ver) -> String {
    if v.2 == 0 { format!("{}.{}", v.0, v.1) } else { format!("{}.{}{}", v.0, v.1, v.2) }
}

/// 파일 이름 "교무수첩_1.0.0.exe" (안내 문구에도 이 이름을 쓴다)
fn file_name(v: Ver) -> String {
    format!("교무수첩_{}.{}.{}.exe", v.0, v.1, v.2)
}

/// 파일 이름의 버전과 예전 이름 여부: "교무수첩_1.0.0.exe" → ((1, 0, 0), false), 예전 "교무수첩(0.61).exe" → ((0, 6, 1), true)
fn file_ver(name: &str) -> Option<(Ver, bool)> {
    let stem = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE"))?;
    if let Some(v) = stem.strip_prefix("교무수첩_") {
        let mut it = v.split('.');
        let ver = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
        return if it.next().is_none() { Some((ver, false)) } else { None };
    }
    legacy_file_ver(stem).map(|v| (v, true))
}

/// 1.0 까지의 예전 이름 "교무수첩(0.61)" (".exe" 뗀 것) → (0, 6, 1)
fn legacy_file_ver(stem: &str) -> Option<Ver> {
    let inner = stem.strip_prefix("교무수첩(")?.strip_suffix(')')?;
    let (a, rest) = inner.split_once('.')?;
    let mut cs = rest.chars();
    let b = cs.next()?.to_digit(10)?;
    let tail: String = cs.collect();
    let c = if tail.is_empty() { 0 } else { tail.parse().ok()? };
    Some((a.parse().ok()?, b, c))
}

/// 풀어 둔 버전 폴더 중 가장 새로운 것
fn newest_extracted(programs: &Path) -> Option<Ver> {
    std::fs::read_dir(programs)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with('.') { None } else { parse_ver(&n) }
        })
        .max()
}

/// 지금보다 오래된 버전 폴더만 정리한다 (새 버전은 남긴다, 쓰는 중이면 다음에 다시)
fn cleanup(programs: &Path, mine: Ver) {
    let Ok(rd) = std::fs::read_dir(programs) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        // 다른 실행기가 지금 풀고 있는 임시 폴더(".버전-번호")는 건드리지 않는다
        if name.starts_with('.') || !e.path().is_dir() {
            continue;
        }
        if parse_ver(&name).is_some_and(|v| v < mine) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// 이 파일과 같은 폴더에 있는 지난 버전 파일들 (같은 버전이라도 예전 이름이면 넣는다)
fn older_files_beside(me: &Path, mine: Ver) -> Vec<PathBuf> {
    let Some(dir) = me.parent() else { return vec![] };
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p != me)
        .filter(|p| p.file_name().and_then(|n| n.to_str()).and_then(file_ver).is_some_and(|(v, legacy)| v < mine || (legacy && v == mine)))
        .collect();
    out.sort();
    out
}

/// 자동 시험용: TN_TEST_ANSWER=yes|no 이면 묻지 않고 그 답을 쓴다 (실제 사용에는 설정되지 않음)
fn test_answer() -> Option<bool> {
    std::env::var("TN_TEST_ANSWER").ok().map(|v| v.eq_ignore_ascii_case("yes"))
}

#[cfg(windows)]
mod ui {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DeleteObject, FF_DONTCARE, FW_NORMAL, HGDIOBJ, OUT_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
    };
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, GetSystemMetrics, IDYES, MB_ICONERROR, MB_ICONQUESTION, MB_OK, MB_YESNO, MSG,
        MessageBoxW, PM_REMOVE, PeekMessageW,
        SM_CXSCREEN, SM_CYSCREEN, SendMessageW, TranslateMessage, WINDOW_STYLE, WM_SETFONT, WS_BORDER, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };
    use windows::core::{HSTRING, w};

    /// STATIC 창의 글자 가운데 맞춤 (SS_CENTER)
    const SS_CENTER: u32 = 0x1;

    pub fn error(msg: &str) {
        unsafe {
            MessageBoxW(None, &HSTRING::from(msg), &HSTRING::from(super::TITLE), MB_OK | MB_ICONERROR);
        }
    }

    /// 예/아니요 질문 (기본 선택은 '예')
    pub fn ask(msg: &str) -> bool {
        unsafe { MessageBoxW(None, &HSTRING::from(msg), &HSTRING::from(super::TITLE), MB_YESNO | MB_ICONQUESTION) == IDYES }
    }

    /// 파일을 휴지통으로 옮긴다 (되살릴 수 있게). 휴지통에 못 넣는 경우 Windows 가 영구 삭제 전에 경고한다.
    pub fn recycle(files: &[std::path::PathBuf]) -> Result<(), String> {
        use windows::Win32::UI::Shell::{SHFILEOPSTRUCTW, SHFileOperationW};
        // 이중 NUL 로 끝나는 파일 목록
        let mut from: Vec<u16> = Vec::new();
        for f in files {
            from.extend(f.as_os_str().encode_wide());
            from.push(0);
        }
        from.push(0);
        const FO_DELETE: u32 = 3;
        // FOF_ALLOWUNDO(휴지통) | FOF_NOCONFIRMATION(이미 물었음) | FOF_SILENT | FOF_WANTNUKEWARNING(영구 삭제면 경고)
        const FLAGS: u16 = 0x0040 | 0x0010 | 0x0004 | 0x4000;
        let mut op = SHFILEOPSTRUCTW { wFunc: FO_DELETE, pFrom: windows::core::PCWSTR(from.as_ptr()), fFlags: FLAGS, ..Default::default() };
        let r = unsafe { SHFileOperationW(&mut op) };
        if r == 0 && !op.fAnyOperationsAborted.as_bool() { Ok(()) } else { Err(format!("휴지통으로 옮기지 못했습니다 (코드 {r})")) }
    }

    /// 오래 걸리는 일을 하는 동안 작은 "준비 중" 창을 띄운다
    pub fn with_splash<T: Send + 'static>(text: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
        let job = std::thread::spawn(work);
        unsafe {
            let (w, h) = (440, 110);
            let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
            let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
            let hwnd: Option<HWND> = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                &HSTRING::from(text),
                WS_POPUP | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(SS_CENTER),
                x,
                y,
                w,
                h,
                None,
                None,
                None,
                None,
            )
            .ok();
            let font = CreateFontW(
                -17,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
                w!("Malgun Gothic"),
            );
            if let Some(h) = hwnd {
                SendMessageW(h, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            }
            let mut msg = MSG::default();
            while !job.is_finished() {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            if let Some(h) = hwnd {
                let _ = DestroyWindow(h);
            }
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
        job.join().expect("준비 작업")
    }
}

#[cfg(not(windows))]
mod ui {
    pub fn error(msg: &str) {
        eprintln!("{msg}");
    }
    pub fn ask(_msg: &str) -> bool {
        false
    }
    pub fn recycle(_files: &[std::path::PathBuf]) -> Result<(), String> {
        Err("지원하지 않는 운영체제입니다.".into())
    }
    pub fn with_splash<T: Send + 'static>(_text: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
        work()
    }
}

fn main() {
    let m: Manifest = match serde_json::from_str(MANIFEST) {
        Ok(m) => m,
        Err(e) => {
            ui::error(&format!("실행 파일이 손상되었습니다. 새로 받아 주세요. ({e})"));
            std::process::exit(1);
        }
    };
    let mine = parse_ver(&m.version).unwrap_or((0, 0, 0));
    let me_name = file_name(mine);
    let programs = data_root().join("program");
    let dir = programs.join(&m.version);

    // 지난 버전 파일을 잘못 켰으면: 새 버전을 쓰라고 알린다 (그래도 원하면 실행, 새 버전 폴더는 지우지 않음)
    if let Some(newest) = newest_extracted(&programs).filter(|v| *v > mine) {
        let go = test_answer().unwrap_or_else(|| {
            ui::ask(&format!(
                "이 파일은 지난 버전({me_name})입니다.\n이 PC에서는 이미 더 새로운 {}을 쓰고 있습니다.\n\n새 버전 파일로 실행하는 것을 권합니다. 이 파일은 지워도 됩니다.\n\n그래도 이 지난 버전으로 실행할까요?",
                file_name(newest)
            ))
        });
        if !go {
            std::process::exit(3);
        }
    }

    let mut fresh = false;
    if !verify(&dir, &m) {
        fresh = true;
        let (dir2, version) = (dir.clone(), m.version.clone());
        let res = ui::with_splash(
            &format!(
                "\n로컬 AI 교무수첩 {}을(를) 준비하고 있습니다…\n(새 버전을 처음 켤 때 한 번, 몇 초 걸립니다)",
                parse_ver(&version).map(display_ver).unwrap_or(version.clone())
            ),
            move || {
                let m: Manifest = serde_json::from_str(MANIFEST).expect("목록");
                extract(&dir2, &m)
            },
        );
        if let Err(e) = res {
            ui::error(&format!(
                "교무수첩을 준비하지 못했습니다.\n\n{e}\n\n풀 위치: {}\n\n백신이 막았다면 압축(ZIP) 버전을 써 주세요.",
                dir.display()
            ));
            std::process::exit(1);
        }
    }
    let exe = rel_path(&dir, &m.main);
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // 패키징 검사(--self-test)는 결과를 기다려 종료 코드를 넘긴다. 평소에는 본체를 띄우고 바로 끝낸다.
    let wait = args.iter().any(|a| a == "--self-test" || a == "--export-catalog");

    // 새 버전을 처음 풀었으면: 같은 폴더의 지난 버전 파일을 휴지통으로 옮길지 한 번 묻는다
    if fresh && !wait {
        if let Ok(me) = std::env::current_exe() {
            let old = older_files_beside(&me, mine);
            if !old.is_empty() {
                let names: Vec<String> = old.iter().filter_map(|p| p.file_name().map(|n| format!("  {}", n.to_string_lossy()))).collect();
                let yes = test_answer().unwrap_or_else(|| {
                    ui::ask(&format!(
                        "새 버전 {me_name}을 처음 켰습니다.\n같은 폴더에 지난 버전(또는 예전 이름) 파일이 있습니다:\n\n{}\n\n휴지통으로 옮길까요? (기록과 AI 모델은 그대로입니다. '아니요'를 누르면 그대로 둡니다)",
                        names.join("\n")
                    ))
                });
                if yes {
                    let res = if test_answer().is_some() {
                        // 자동 시험은 임시 폴더의 시험 파일만 다루므로 바로 지운다
                        old.iter().try_for_each(std::fs::remove_file).map_err(|e| e.to_string())
                    } else {
                        ui::recycle(&old)
                    };
                    if let Err(e) = res {
                        ui::error(&format!("지난 버전 파일을 옮기지 못했습니다: {e}\n직접 지워 주세요."));
                    }
                }
            }
        }
    }
    let mut cmd = Command::new(&exe);
    cmd.args(&args);
    if let Ok(me) = std::env::current_exe() {
        // 본체가 자동 시작 등록 등에 이 파일 위치를 쓰게 한다 (업데이트로 덮어써도 끊기지 않게)
        cmd.env("TN_LAUNCHER", me);
    }
    match cmd.spawn() {
        Ok(mut child) => {
            cleanup(&programs, mine);
            if wait {
                let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
                std::process::exit(code);
            }
        }
        Err(e) => {
            ui::error(&format!("교무수첩을 시작하지 못했습니다: {e}\n\n{}\n\n백신이 막았다면 압축(ZIP) 버전을 써 주세요.", exe.display()));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_ver("0.6.1"), Some((0, 6, 1)));
        assert_eq!(parse_ver("0.6.0-update-test"), Some((0, 6, 0)));
        assert_eq!(parse_ver("abc"), None);
        assert_eq!(display_ver((0, 6, 1)), "0.61");
        assert_eq!(display_ver((0, 6, 0)), "0.6");
        assert_eq!(display_ver((1, 0, 0)), "1.0");
        // 지금 이름
        assert_eq!(file_name((1, 0, 0)), "교무수첩_1.0.0.exe");
        assert_eq!(file_ver("교무수첩_1.0.0.exe"), Some(((1, 0, 0), false)));
        assert_eq!(file_ver("교무수첩_1.0.12.EXE"), Some(((1, 0, 12), false)));
        assert_eq!(file_ver("교무수첩_1.0.exe"), None);
        assert_eq!(file_ver("교무수첩_1.0.0.1.exe"), None);
        assert_eq!(file_ver("교무수첩_1.0.0 - 복사본.exe"), None);
        // 1.0 까지의 예전 이름
        assert_eq!(file_ver("교무수첩(0.61).exe"), Some(((0, 6, 1), true)));
        assert_eq!(file_ver("교무수첩(0.6).exe"), Some(((0, 6, 0), true)));
        assert_eq!(file_ver("교무수첩(1.12).exe"), Some(((1, 1, 2), true)));
        assert_eq!(file_ver("teachers_note.exe"), None);
        assert_eq!(file_ver("교무수첩(0.6) - 복사본.exe"), None);
        assert!(file_ver("교무수첩(0.6).exe").unwrap().0 < file_ver("교무수첩(0.61).exe").unwrap().0);
        assert!(file_ver("교무수첩(0.9).exe").unwrap().0 < file_ver("교무수첩_1.0.0.exe").unwrap().0);
    }

    #[test]
    fn renames_legacy_data_folder() {
        let base = std::env::temp_dir().join(format!("tn-launcher-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("TeacherNotebookV2").join("program")).unwrap();
        assert_eq!(pick_root(&base), base.join("teachers_note"));
        assert!(base.join("teachers_note").join("program").is_dir() && !base.join("TeacherNotebookV2").exists());
        assert_eq!(pick_root(&base), base.join("teachers_note"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn finds_only_older_version_files_beside() {
        let dir = std::env::temp_dir().join(format!("tn-launcher-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["교무수첩(0.9).exe", "교무수첩(1.0).exe", "교무수첩_0.9.0.exe", "교무수첩_1.0.0.exe", "교무수첩_1.1.0.exe", "메모.txt", "teachers_note.exe"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let me = dir.join("교무수첩_1.0.0.exe");
        let old = older_files_beside(&me, (1, 0, 0));
        // 지난 버전(예전 이름·새 이름)과 같은 버전의 예전 이름 파일, 새 버전은 그대로
        assert_eq!(old, vec![dir.join("교무수첩(0.9).exe"), dir.join("교무수첩(1.0).exe"), dir.join("교무수첩_0.9.0.exe")]);
        // 버전 폴더 정리는 더 오래된 것만
        let programs = dir.join("program");
        for v in ["0.6.0", "0.6.1", "0.7.0", ".0.6.1-123"] {
            std::fs::create_dir_all(programs.join(v)).unwrap();
        }
        assert_eq!(newest_extracted(&programs), Some((0, 7, 0)));
        cleanup(&programs, (0, 6, 1));
        assert!(!programs.join("0.6.0").exists());
        assert!(programs.join("0.6.1").exists() && programs.join("0.7.0").exists() && programs.join(".0.6.1-123").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}