//! AI 모델 관리: 카탈로그(HTTPS 주소·크기·SHA-256·라이선스 고정), 다운로드(이어받기·일시정지·재시도),
//! 검증, 파일로 가져오기, 삭제, 기기 사양에 따른 추천.

use crate::error::{AppError, AppResult};
use crate::util;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct CatalogModel {
    pub id: &'static str,
    /// 1~5 (초경량형 ~ 울트라)
    pub tier: u8,
    pub tier_label: &'static str,
    pub name: &'static str,
    pub params: &'static str,
    pub quant: &'static str,
    pub file_name: &'static str,
    pub url: &'static str,
    pub size_bytes: u64,
    pub sha256: &'static str,
    pub license: &'static str,
    pub license_url: &'static str,
    pub publisher: &'static str,
    pub min_ram_gb: u32,
    pub recommended_ram_gb: u32,
    pub description: &'static str,
    /// 같은 등급의 대안 모델
    pub alternative: bool,
}

/// 모든 주소는 Hugging Face 의 특정 커밋에 고정한다 (파일이 바뀌어도 SHA-256 검증에서 걸러진다).
pub const CATALOG: &[CatalogModel] = &[
    CatalogModel {
        id: "qwen2.5-0.5b",
        tier: 1,
        tier_label: "초경량형",
        name: "Qwen2.5 0.5B Instruct",
        params: "0.5B",
        quant: "Q4_K_M",
        file_name: "qwen2.5-0.5b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/9217f5db79a29953eb74d5343926648285ec7e67/qwen2.5-0.5b-instruct-q4_k_m.gguf",
        size_bytes: 491_400_032,
        sha256: "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db",
        license: "Apache-2.0",
        license_url: "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct/blob/main/LICENSE",
        publisher: "Qwen (공식)",
        min_ram_gb: 4,
        recommended_ram_gb: 4,
        description: "AI 엔진 동작 확인과 아주 짧은 메모용. 분류 정확도는 낮으므로 결과를 꼭 확인하세요.",
        alternative: false,
    },
    CatalogModel {
        id: "qwen3.5-2b",
        tier: 2,
        tier_label: "기본형",
        name: "Qwen3.5 2B",
        params: "2B",
        quant: "Q4_K_M",
        file_name: "Qwen3.5-2B-Q4_K_M.gguf",
        url: "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/f6d5376be1edb4d416d56da11e5397a961aca8ae/Qwen3.5-2B-Q4_K_M.gguf",
        size_bytes: 1_280_835_840,
        sha256: "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",
        license: "Apache-2.0",
        license_url: "https://huggingface.co/Qwen/Qwen3.5-2B/blob/main/LICENSE",
        publisher: "Qwen 모델 · unsloth 변환",
        min_ram_gb: 8,
        recommended_ram_gb: 8,
        description: "RAM 8GB 학교 PC 권장. 일상 메모 분류에 알맞습니다.",
        alternative: false,
    },
    CatalogModel {
        id: "qwen3.5-4b",
        tier: 3,
        tier_label: "고급형",
        name: "Qwen3.5 4B",
        params: "4B",
        quant: "Q4_K_M",
        file_name: "Qwen3.5-4B-Q4_K_M.gguf",
        url: "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/e87f176479d0855a907a41277aca2f8ee7a09523/Qwen3.5-4B-Q4_K_M.gguf",
        size_bytes: 2_740_937_888,
        sha256: "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
        license: "Apache-2.0",
        license_url: "https://huggingface.co/Qwen/Qwen3.5-4B/blob/main/LICENSE",
        publisher: "Qwen 모델 · unsloth 변환",
        min_ram_gb: 12,
        recommended_ram_gb: 16,
        description: "RAM 16GB PC 권장. 긴 메모와 학생 관찰 영역 구분이 더 정확합니다.",
        alternative: false,
    },
    CatalogModel {
        id: "gemma4-e4b",
        tier: 4,
        tier_label: "최고급형",
        name: "Gemma 4 E4B",
        params: "4B(유효)",
        quant: "Q4_0",
        file_name: "gemma-4-E4B-it-Q4_0.gguf",
        url: "https://huggingface.co/ggml-org/gemma-4-E4B-it-GGUF/resolve/b8093469224f83f5c38f691eb906c380e9e63114/gemma-4-E4B-it-Q4_0.gguf",
        size_bytes: 4_590_807_392,
        sha256: "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2",
        license: "Apache-2.0",
        license_url: "https://huggingface.co/google/gemma-4-E4B-it",
        publisher: "Google 모델 · ggml-org 변환",
        min_ram_gb: 16,
        recommended_ram_gb: 24,
        description: "RAM 24GB PC 권장. 한국어 문장 이해가 좋은 다른 계열 모델입니다.",
        alternative: false,
    },
    CatalogModel {
        id: "qwen3.5-9b",
        tier: 5,
        tier_label: "울트라",
        name: "Qwen3.5 9B",
        params: "9B",
        quant: "Q4_K_M",
        file_name: "Qwen3.5-9B-Q4_K_M.gguf",
        url: "https://huggingface.co/unsloth/Qwen3.5-9B-GGUF/resolve/3885219b6810b007914f3a7950a8d1b469d598a5/Qwen3.5-9B-Q4_K_M.gguf",
        size_bytes: 5_680_522_464,
        sha256: "03b74727a860a56338e042c4420bb3f04b2fec5734175f4cb9fa853daf52b7e8",
        license: "Apache-2.0",
        license_url: "https://huggingface.co/Qwen/Qwen3.5-9B/blob/main/LICENSE",
        publisher: "Qwen 모델 · unsloth 변환",
        min_ram_gb: 16,
        recommended_ram_gb: 32,
        description: "RAM 32GB PC 권장. 가장 정확하지만 처리 시간이 깁니다.",
        alternative: false,
    },
    CatalogModel {
        id: "qwen3-8b",
        tier: 5,
        tier_label: "울트라 (대안)",
        name: "Qwen3 8B",
        params: "8B",
        quant: "Q4_K_M",
        file_name: "Qwen3-8B-Q4_K_M.gguf",
        url: "https://huggingface.co/Qwen/Qwen3-8B-GGUF/resolve/7c41481f57cb95916b40956ab2f0b139b296d974/Qwen3-8B-Q4_K_M.gguf",
        size_bytes: 5_027_783_488,
        sha256: "d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785",
        license: "Apache-2.0",
        license_url: "https://huggingface.co/Qwen/Qwen3-8B/blob/main/LICENSE",
        publisher: "Qwen (공식)",
        min_ram_gb: 16,
        recommended_ram_gb: 32,
        description: "이미 받아 둔 Qwen3 8B 파일이 있으면 '파일로 설치'로 다시 받지 않고 쓸 수 있습니다.",
        alternative: true,
    },
];

pub fn find(id: &str) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|m| m.id == id)
}

/// 설치된 RAM 으로 권장 모델을 고른다 (CPU 실행 기준)
pub fn recommend(total_ram_bytes: u64) -> &'static str {
    let gb = (total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0)).round() as u32;
    match gb {
        0..=5 => "qwen2.5-0.5b",
        6..=11 => "qwen3.5-2b",
        12..=19 => "qwen3.5-4b",
        20..=27 => "gemma4-e4b",
        _ => "qwen3.5-9b",
    }
}

// ───────────────────────── 설치 기록 ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct InstalledModel {
    pub model_id: String,
    pub file_name: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub installed_at: String,
    pub verified_at: Option<String>,
}

pub fn installed(conn: &Connection) -> AppResult<Vec<InstalledModel>> {
    let mut st = conn.prepare("SELECT model_id, file_name, size_bytes, sha256, installed_at, verified_at FROM installed_models ORDER BY installed_at")?;
    let rows = st
        .query_map([], |r| {
            Ok(InstalledModel {
                model_id: r.get(0)?,
                file_name: r.get(1)?,
                size_bytes: r.get(2)?,
                sha256: r.get(3)?,
                installed_at: r.get(4)?,
                verified_at: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_installed(conn: &Connection, id: &str) -> AppResult<Option<InstalledModel>> {
    Ok(installed(conn)?.into_iter().find(|m| m.model_id == id))
}

pub fn mark_installed(conn: &Connection, m: &CatalogModel, now: chrono::NaiveDateTime) -> AppResult<()> {
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO installed_models(model_id, file_name, size_bytes, sha256, installed_at, verified_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(model_id) DO UPDATE SET file_name=excluded.file_name, size_bytes=excluded.size_bytes, sha256=excluded.sha256, verified_at=excluded.verified_at",
        params![m.id, m.file_name, m.size_bytes as i64, m.sha256, now_s],
    )?;
    Ok(())
}

pub fn unmark_installed(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM installed_models WHERE model_id=?1", params![id])?;
    Ok(())
}

pub fn set_verified(conn: &Connection, id: &str, now: chrono::NaiveDateTime) -> AppResult<()> {
    conn.execute("UPDATE installed_models SET verified_at=?2 WHERE model_id=?1", params![id, util::fmt_dt(now)])?;
    Ok(())
}

pub fn model_path(models_dir: &Path, m: &CatalogModel) -> PathBuf {
    models_dir.join(m.file_name)
}

pub fn part_path(models_dir: &Path, m: &CatalogModel) -> PathBuf {
    models_dir.join(format!("{}.part", m.file_name))
}

/// 시작할 때 설치 기록과 실제 파일을 맞춘다 (파일이 없거나 크기가 다르면 설치 기록을 지운다)
pub fn reconcile(conn: &Connection, models_dir: &Path) -> AppResult<Vec<String>> {
    let mut removed = Vec::new();
    for im in installed(conn)? {
        let ok = match find(&im.model_id) {
            Some(m) => std::fs::metadata(model_path(models_dir, m)).map(|md| md.len() == m.size_bytes).unwrap_or(false),
            None => false,
        };
        if !ok {
            unmark_installed(conn, &im.model_id)?;
            removed.push(im.model_id);
        }
    }
    Ok(removed)
}

// ───────────────────────── 검증 ─────────────────────────

/// 파일 SHA-256 (진행률 콜백: 읽은 바이트)
pub fn sha256_file(path: &Path, cancel: Option<&AtomicBool>, mut progress: impl FnMut(u64)) -> std::io::Result<Option<String>> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done: u64 = 0;
    let mut last = Instant::now();
    loop {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Ok(None);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        if last.elapsed() > Duration::from_millis(250) {
            progress(done);
            last = Instant::now();
        }
    }
    progress(done);
    Ok(Some(hex::encode(h.finalize())))
}

// ───────────────────────── 다운로드 ─────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DlProgress {
    pub model_id: String,
    /// downloading | paused | verifying | failed | done | copying
    pub status: String,
    pub received: u64,
    pub total: u64,
    pub bytes_per_sec: u64,
    pub message: Option<String>,
}

struct Ctl {
    cancel: Arc<AtomicBool>,
    discard: Arc<AtomicBool>,
}

pub struct Downloader {
    ctl: Mutex<HashMap<String, Ctl>>,
    progress: Mutex<HashMap<String, DlProgress>>,
    on_progress: Arc<dyn Fn(&DlProgress) + Send + Sync>,
}

pub enum Finish {
    Installed,
    Paused,
    Cancelled,
}

/// 네트워크 요청은 학생 정보와 무관한 모델 파일 다운로드뿐이다.
fn download_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("teachers_note/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())
}

pub fn free_space(dir: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        use windows::core::HSTRING;
        let mut avail: u64 = 0;
        let h = HSTRING::from(dir.as_os_str());
        unsafe {
            if GetDiskFreeSpaceExW(&h, Some(&mut avail), None, None).is_ok() {
                return Some(avail);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        None
    }
}

fn friendly_net_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "인터넷 응답이 너무 느립니다".into()
    } else if e.is_connect() {
        "다운로드 서버에 연결하지 못했습니다 (인터넷 연결이나 학교 방화벽을 확인해 주세요)".into()
    } else {
        "다운로드 중 연결이 끊겼습니다".into()
    }
}

impl Downloader {
    pub fn new(on_progress: impl Fn(&DlProgress) + Send + Sync + 'static) -> Downloader {
        Downloader { ctl: Mutex::new(HashMap::new()), progress: Mutex::new(HashMap::new()), on_progress: Arc::new(on_progress) }
    }

    pub fn snapshot(&self) -> HashMap<String, DlProgress> {
        self.progress.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.ctl.lock().unwrap_or_else(|e| e.into_inner()).contains_key(id)
    }

    fn emit(&self, p: DlProgress) {
        self.progress.lock().unwrap_or_else(|e| e.into_inner()).insert(p.model_id.clone(), p.clone());
        (self.on_progress)(&p);
    }

    pub fn clear(&self, id: &str) {
        self.progress.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
    }

    pub fn pause_all(&self) {
        for c in self.ctl.lock().unwrap_or_else(|e| e.into_inner()).values() {
            c.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn pause(&self, id: &str) {
        if let Some(c) = self.ctl.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            c.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// 받던 것을 멈추고 .part 파일도 지운다
    pub fn cancel(&self, id: &str, models_dir: &Path) {
        let active = {
            let g = self.ctl.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(c) = g.get(id) {
                c.discard.store(true, Ordering::Relaxed);
                c.cancel.store(true, Ordering::Relaxed);
                true
            } else {
                false
            }
        };
        if !active {
            if let Some(m) = find(id) {
                let _ = std::fs::remove_file(part_path(models_dir, m));
            }
            self.clear(id);
        }
    }

    /// 다운로드를 시작하거나 이어서 받는다. 완료되면 on_done 을 부른다 (같은 스레드).
    pub fn start(
        self: &Arc<Self>,
        m: &'static CatalogModel,
        url: String,
        models_dir: PathBuf,
        on_done: impl FnOnce(Result<Finish, String>) + Send + 'static,
    ) -> AppResult<()> {
        {
            let mut g = self.ctl.lock().unwrap_or_else(|e| e.into_inner());
            if g.contains_key(m.id) {
                return Err(AppError::invalid("이미 받고 있습니다."));
            }
            g.insert(m.id.to_string(), Ctl { cancel: Arc::new(AtomicBool::new(false)), discard: Arc::new(AtomicBool::new(false)) });
        }
        std::fs::create_dir_all(&models_dir)?;
        let me = self.clone();
        std::thread::spawn(move || {
            let (cancel, discard) = {
                let g = me.ctl.lock().unwrap_or_else(|e| e.into_inner());
                let c = g.get(m.id).expect("ctl");
                (c.cancel.clone(), c.discard.clone())
            };
            let res = me.run(m, &url, &models_dir, &cancel);
            let res = match res {
                Ok(Finish::Paused) if discard.load(Ordering::Relaxed) => {
                    let _ = std::fs::remove_file(part_path(&models_dir, m));
                    me.clear(m.id);
                    Ok(Finish::Cancelled)
                }
                other => other,
            };
            me.ctl.lock().unwrap_or_else(|e| e.into_inner()).remove(m.id);
            match &res {
                Ok(Finish::Installed) => me.emit(DlProgress {
                    model_id: m.id.into(),
                    status: "done".into(),
                    received: m.size_bytes,
                    total: m.size_bytes,
                    bytes_per_sec: 0,
                    message: None,
                }),
                Ok(Finish::Cancelled) => {
                    (me.on_progress)(&DlProgress { model_id: m.id.into(), status: "idle".into(), received: 0, total: m.size_bytes, bytes_per_sec: 0, message: None });
                }
                Ok(Finish::Paused) => {}
                Err(msg) => {
                    let received = std::fs::metadata(part_path(&models_dir, m)).map(|x| x.len()).unwrap_or(0);
                    me.emit(DlProgress {
                        model_id: m.id.into(),
                        status: "failed".into(),
                        received,
                        total: m.size_bytes,
                        bytes_per_sec: 0,
                        message: Some(msg.clone()),
                    });
                }
            }
            on_done(res);
        });
        Ok(())
    }

    fn run(&self, m: &CatalogModel, url: &str, dir: &Path, cancel: &AtomicBool) -> Result<Finish, String> {
        let part = part_path(dir, m);
        let final_path = model_path(dir, m);
        let total = m.size_bytes;
        let mut have = std::fs::metadata(&part).map(|x| x.len()).unwrap_or(0);
        if have > total {
            std::fs::remove_file(&part).map_err(|e| e.to_string())?;
            have = 0;
        }
        if let Some(free) = free_space(dir) {
            let need = total - have + 64 * 1024 * 1024;
            if free < need {
                return Err(format!(
                    "저장 공간이 부족합니다. {:.1}GB가 더 필요합니다 (남은 공간 {:.1}GB).",
                    need as f64 / 1e9,
                    free as f64 / 1e9
                ));
            }
        }
        let client = download_client()?;
        let mut attempts = 0u32;
        while have < total {
            if cancel.load(Ordering::Relaxed) {
                self.emit(DlProgress { model_id: m.id.into(), status: "paused".into(), received: have, total, bytes_per_sec: 0, message: None });
                return Ok(Finish::Paused);
            }
            let mut req = client.get(url);
            if have > 0 {
                req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
            }
            let resp = match req.send() {
                Ok(r) => r,
                Err(e) => {
                    attempts += 1;
                    if attempts > 5 {
                        return Err(friendly_net_error(&e));
                    }
                    self.emit(DlProgress {
                        model_id: m.id.into(),
                        status: "downloading".into(),
                        received: have,
                        total,
                        bytes_per_sec: 0,
                        message: Some(format!("{} — 다시 시도합니다 ({attempts}/5)", friendly_net_error(&e))),
                    });
                    if wait_or_cancel(Duration::from_secs(2u64.pow(attempts.min(4))), cancel) {
                        continue;
                    }
                    continue;
                }
            };
            let status = resp.status().as_u16();
            let mut file = match status {
                206 => OpenOptions::new().create(true).append(true).open(&part).map_err(|e| e.to_string())?,
                200 => {
                    // 서버가 이어받기를 지원하지 않으면 처음부터
                    have = 0;
                    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&part).map_err(|e| e.to_string())?;
                    f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                    f
                }
                416 => break,
                404 => return Err("다운로드 주소에서 파일을 찾지 못했습니다 (404).".into()),
                403 | 401 => return Err("다운로드가 거부되었습니다. 학교 네트워크에서 차단되었을 수 있습니다.".into()),
                s => {
                    attempts += 1;
                    if attempts > 5 {
                        return Err(format!("다운로드 서버 오류 ({s})"));
                    }
                    wait_or_cancel(Duration::from_secs(2u64.pow(attempts.min(4))), cancel);
                    continue;
                }
            };
            let mut body = resp;
            let mut buf = vec![0u8; 256 * 1024];
            let mut window_start = Instant::now();
            let mut window_bytes: u64 = 0;
            let mut last_emit = Instant::now() - Duration::from_secs(1);
            let mut bps: u64 = 0;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    let _ = file.flush();
                    self.emit(DlProgress { model_id: m.id.into(), status: "paused".into(), received: have, total, bytes_per_sec: 0, message: None });
                    return Ok(Finish::Paused);
                }
                let n = match body.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break, // 끊기면 바깥 반복에서 이어받는다
                };
                if have + n as u64 > total {
                    return Err("받은 파일이 예상보다 큽니다. 다운로드 주소를 확인해 주세요.".into());
                }
                file.write_all(&buf[..n]).map_err(|e| format!("파일을 쓰지 못했습니다: {e}"))?;
                have += n as u64;
                window_bytes += n as u64;
                attempts = 0;
                if window_start.elapsed() >= Duration::from_secs(1) {
                    bps = (window_bytes as f64 / window_start.elapsed().as_secs_f64()) as u64;
                    window_start = Instant::now();
                    window_bytes = 0;
                }
                if last_emit.elapsed() > Duration::from_millis(300) {
                    self.emit(DlProgress { model_id: m.id.into(), status: "downloading".into(), received: have, total, bytes_per_sec: bps, message: None });
                    last_emit = Instant::now();
                }
            }
            file.flush().map_err(|e| e.to_string())?;
            drop(file);
            have = std::fs::metadata(&part).map(|x| x.len()).unwrap_or(have);
            if have < total {
                attempts += 1;
                if attempts > 5 {
                    return Err("다운로드가 여러 번 끊겼습니다. '이어받기'를 눌러 다시 시도해 주세요.".into());
                }
                wait_or_cancel(Duration::from_secs(2), cancel);
            }
        }
        // 크기와 SHA-256 을 확인한 뒤에만 설치 완료로 처리한다
        let len = std::fs::metadata(&part).map(|x| x.len()).map_err(|e| e.to_string())?;
        if len != total {
            let _ = std::fs::remove_file(&part);
            return Err(format!("받은 파일 크기({len})가 예상({total})과 다릅니다. 다시 받아 주세요."));
        }
        self.emit(DlProgress { model_id: m.id.into(), status: "verifying".into(), received: 0, total, bytes_per_sec: 0, message: None });
        let hash = sha256_file(&part, Some(cancel), |done| {
            self.emit(DlProgress { model_id: m.id.into(), status: "verifying".into(), received: done, total, bytes_per_sec: 0, message: None })
        })
        .map_err(|e| e.to_string())?;
        let Some(hash) = hash else {
            self.emit(DlProgress { model_id: m.id.into(), status: "paused".into(), received: total, total, bytes_per_sec: 0, message: None });
            return Ok(Finish::Paused);
        };
        if !hash.eq_ignore_ascii_case(m.sha256) {
            let _ = std::fs::remove_file(&part);
            return Err("파일 검증(SHA-256)에 실패했습니다. 받은 파일이 손상되어 지웠습니다. 다시 받아 주세요.".into());
        }
        if final_path.exists() {
            std::fs::remove_file(&final_path).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&part, &final_path).map_err(|e| e.to_string())?;
        Ok(Finish::Installed)
    }

    /// 다른 PC에서 받은 모델 파일(USB 등)을 검증한 뒤 복사해 설치한다. 원본 파일은 바꾸지 않는다.
    pub fn import_file(&self, src: &Path, models_dir: &Path, cancel: &AtomicBool) -> Result<&'static CatalogModel, String> {
        let len = std::fs::metadata(src).map_err(|e| format!("파일을 열 수 없습니다: {e}"))?.len();
        let Some(m) = CATALOG.iter().find(|m| m.size_bytes == len) else {
            return Err("목록에 있는 모델 파일이 아닙니다 (파일 크기가 맞지 않음).".into());
        };
        let hash = sha256_file(src, Some(cancel), |done| {
            self.emit(DlProgress { model_id: m.id.into(), status: "verifying".into(), received: done, total: len, bytes_per_sec: 0, message: None })
        })
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "취소했습니다.".to_string())?;
        if !hash.eq_ignore_ascii_case(m.sha256) {
            self.clear(m.id);
            return Err(format!("{} 파일과 크기는 같지만 내용(SHA-256)이 다릅니다. 손상된 파일일 수 있습니다.", m.name));
        }
        std::fs::create_dir_all(models_dir).map_err(|e| e.to_string())?;
        let dest = model_path(models_dir, m);
        let tmp = part_path(models_dir, m);
        let mut r = File::open(src).map_err(|e| e.to_string())?;
        let mut w = File::create(&tmp).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 20];
        let mut done = 0u64;
        let mut last = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(w);
                let _ = std::fs::remove_file(&tmp);
                return Err("취소했습니다.".into());
            }
            let n = r.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            w.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            if last.elapsed() > Duration::from_millis(300) {
                self.emit(DlProgress { model_id: m.id.into(), status: "copying".into(), received: done, total: len, bytes_per_sec: 0, message: None });
                last = Instant::now();
            }
        }
        w.flush().map_err(|e| e.to_string())?;
        drop(w);
        if std::fs::metadata(&tmp).map(|x| x.len()).unwrap_or(0) != len {
            let _ = std::fs::remove_file(&tmp);
            return Err("복사한 파일 크기가 맞지 않습니다.".into());
        }
        if dest.exists() {
            std::fs::remove_file(&dest).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
        self.emit(DlProgress { model_id: m.id.into(), status: "done".into(), received: len, total: len, bytes_per_sec: 0, message: None });
        Ok(m)
    }
}

fn wait_or_cancel(d: Duration, cancel: &AtomicBool) -> bool {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

/// 모델 파일과 받다 만 파일을 지운다
pub fn delete_files(models_dir: &Path, m: &CatalogModel) -> AppResult<()> {
    for p in [model_path(models_dir, m), part_path(models_dir, m)] {
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| AppError::msg(format!("모델 파일을 지우지 못했습니다: {e}")))?;
        }
    }
    Ok(())
}

pub fn selected_model(conn: &Connection) -> AppResult<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key='app_settings'", [], |r| r.get::<_, String>(0))
        .optional()?
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("selected_model").and_then(|x| x.as_str()).map(String::from)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;

    /// Range 요청을 지원하는 작은 HTTP 서버. drop_after 바이트를 보낸 뒤 첫 연결을 끊는다.
    fn serve(data: Arc<Vec<u8>>, drop_after: Option<usize>) -> (String, Arc<AtomicUsize>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let h2 = hits.clone();
        std::thread::spawn(move || {
            for stream in l.incoming() {
                let Ok(mut s) = stream else { continue };
                let n = h2.fetch_add(1, Ordering::SeqCst);
                let mut req = Vec::new();
                let mut b = [0u8; 1];
                while !req.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).unwrap_or(0) == 0 {
                        break;
                    }
                    req.push(b[0]);
                }
                let text = String::from_utf8_lossy(&req).to_lowercase();
                let start = text
                    .lines()
                    .find(|l| l.starts_with("range: bytes="))
                    .and_then(|l| l.trim_start_matches("range: bytes=").trim_end_matches('-').trim().trim_end_matches('-').parse::<usize>().ok())
                    .unwrap_or(0);
                let body = &data[start.min(data.len())..];
                let head = if start > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        body.len(),
                        start,
                        data.len() - 1,
                        data.len()
                    )
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                };
                let _ = s.write_all(head.as_bytes());
                let limit = if n == 0 { drop_after.unwrap_or(usize::MAX) } else { usize::MAX };
                let _ = s.write_all(&body[..limit.min(body.len())]);
            }
        });
        (format!("http://127.0.0.1:{port}/model.gguf"), hits)
    }

    fn fake_model(data: &[u8]) -> &'static CatalogModel {
        let sha: &'static str = Box::leak(hex::encode(Sha256::digest(data)).into_boxed_str());
        Box::leak(Box::new(CatalogModel {
            id: "test-model",
            tier: 1,
            tier_label: "시험",
            name: "시험 모델",
            params: "0",
            quant: "-",
            file_name: "test-model.gguf",
            url: "",
            size_bytes: data.len() as u64,
            sha256: sha,
            license: "-",
            license_url: "-",
            publisher: "-",
            min_ram_gb: 1,
            recommended_ram_gb: 1,
            description: "-",
            alternative: false,
        }))
    }

    fn data(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 % 251) as u8).collect()
    }

    fn run_to_end(dl: &Arc<Downloader>, m: &'static CatalogModel, url: &str, dir: &Path) -> Result<Finish, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        dl.start(m, url.to_string(), dir.to_path_buf(), move |r| {
            let _ = tx.send(r);
        })
        .unwrap();
        rx.recv_timeout(Duration::from_secs(60)).unwrap()
    }

    #[test]
    fn resumes_after_connection_drop_and_verifies() {
        let d = Arc::new(data(3_000_000));
        let (url, hits) = serve(d.clone(), Some(1_000_000));
        let m = fake_model(&d);
        let dir = tempfile::tempdir().unwrap();
        let dl = Arc::new(Downloader::new(|_| {}));
        let r = run_to_end(&dl, m, &url, dir.path());
        assert!(matches!(r, Ok(Finish::Installed)), "{:?}", r.err());
        assert!(hits.load(Ordering::SeqCst) >= 2, "끊긴 뒤 Range 요청으로 이어받아야 한다");
        let got = std::fs::read(model_path(dir.path(), m)).unwrap();
        assert_eq!(got.len(), d.len());
        assert!(!part_path(dir.path(), m).exists());
    }

    #[test]
    fn resumes_existing_part_file() {
        let d = Arc::new(data(2_000_000));
        let (url, _) = serve(d.clone(), None);
        let m = fake_model(&d);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(part_path(dir.path(), m), &d[..700_000]).unwrap();
        let dl = Arc::new(Downloader::new(|_| {}));
        assert!(matches!(run_to_end(&dl, m, &url, dir.path()), Ok(Finish::Installed)));
        assert_eq!(std::fs::read(model_path(dir.path(), m)).unwrap(), *d);
    }

    #[test]
    fn rejects_wrong_hash() {
        let d = Arc::new(data(500_000));
        let (url, _) = serve(d.clone(), None);
        let mut bad = data(500_000);
        bad[10] ^= 0xff;
        let m = fake_model(&bad);
        let dir = tempfile::tempdir().unwrap();
        let dl = Arc::new(Downloader::new(|_| {}));
        let r = run_to_end(&dl, m, &url, dir.path());
        assert!(r.is_err());
        assert!(r.err().unwrap().contains("SHA-256"));
        assert!(!model_path(dir.path(), m).exists());
        assert!(!part_path(dir.path(), m).exists());
    }

    #[test]
    fn recommend_by_ram() {
        let gib = 1024u64 * 1024 * 1024;
        assert_eq!(recommend(8 * gib - 300 * 1024 * 1024), "qwen3.5-2b");
        assert_eq!(recommend(4 * gib), "qwen2.5-0.5b");
        assert_eq!(recommend(16 * gib), "qwen3.5-4b");
        assert_eq!(recommend(24 * gib), "gemma4-e4b");
        assert_eq!(recommend(32 * gib), "qwen3.5-9b");
    }

    #[test]
    fn catalog_is_well_formed() {
        let mut tiers: Vec<u8> = CATALOG.iter().filter(|m| !m.alternative).map(|m| m.tier).collect();
        tiers.sort();
        assert_eq!(tiers, vec![1, 2, 3, 4, 5]);
        for m in CATALOG {
            assert!(m.url.starts_with("https://huggingface.co/"), "{}", m.id);
            assert!(m.url.ends_with(m.file_name));
            assert_eq!(m.sha256.len(), 64);
            assert!(m.size_bytes > 100_000_000);
        }
    }

    #[test]
    fn reconcile_drops_missing_files() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let c = db.lock();
        let dir = tempfile::tempdir().unwrap();
        mark_installed(&c, find("qwen2.5-0.5b").unwrap(), util::now()).unwrap();
        assert_eq!(reconcile(&c, dir.path()).unwrap(), vec!["qwen2.5-0.5b".to_string()]);
        assert!(installed(&c).unwrap().is_empty());
    }
}
