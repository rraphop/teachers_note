//! 앱 상태와 백그라운드 작업 (AI 분류 대기열, 알림 확인, AI 엔진 자동 내리기, 자동 백업)

use crate::classify::Ctx;
use crate::db::Db;
use crate::device;
use crate::error::{AppError, AppResult};
use crate::llm::{self, ChatBackend};
use crate::models::{self, Downloader};
use crate::notes::{self, ApplyMode};
use crate::paths::AppPaths;
use crate::reminders;
use crate::runtime::{LaunchConfig, Runtime, RuntimeError};
use crate::settings;
use crate::util;
use crate::{duties, students};
use chrono::Duration as ChronoDuration;
use rusqlite::params;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type Emitter = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub enum Job {
    Classify { note_id: i64, forced: bool },
    Shutdown,
}

pub struct AppState {
    pub paths: AppPaths,
    pub db: Arc<Db>,
    pub runtime: Arc<Runtime>,
    pub downloader: Arc<Downloader>,
    pub queue: Mutex<Sender<Job>>,
    pub emit: Emitter,
    /// 대기열에 있거나 처리 중인 기록 (중복 등록 방지)
    pub queued: Arc<Mutex<HashSet<i64>>>,
    pub shutting_down: Arc<std::sync::atomic::AtomicBool>,
}

impl AppState {
    pub fn enqueue(&self, note_id: i64, forced: bool) {
        {
            let mut q = self.queued.lock().unwrap_or_else(|e| e.into_inner());
            if !q.insert(note_id) {
                return;
            }
        }
        let _ = self.queue.lock().unwrap_or_else(|e| e.into_inner()).send(Job::Classify { note_id, forced });
    }

    pub fn changed(&self, what: &str) {
        (self.emit)("data-changed", json!({ "what": what }));
    }
}

/// 선택된 모델로 AI 엔진 설정을 만든다
pub fn launch_config(db: &Db, paths: &AppPaths) -> AppResult<Option<LaunchConfig>> {
    db.with(|c| {
        let s = settings::load(c)?;
        let Some(id) = s.selected_model else { return Ok(None) };
        let Some(m) = models::find(&id) else { return Ok(None) };
        if models::get_installed(c, &id)?.is_none() {
            return Ok(None);
        }
        let dev = device::cached(c)?;
        let threads = s.threads.filter(|t| *t > 0).unwrap_or_else(|| device::default_threads(dev.as_ref()));
        Ok(Some(LaunchConfig {
            model_id: id,
            model_path: models::model_path(&paths.models_dir, m),
            size_bytes: m.size_bytes,
            ctx: 4096,
            threads,
            gpu_layers: 0,
        }))
    })
}

pub fn infer_timeout(cfg: &LaunchConfig) -> Duration {
    let gb = (cfg.size_bytes as f64 / 1e9).ceil() as u64;
    Duration::from_secs(180 + 40 * gb)
}

pub struct RuntimeBackend<'a> {
    pub rt: &'a Runtime,
    pub cfg: &'a LaunchConfig,
}

impl ChatBackend for RuntimeBackend<'_> {
    fn chat(&self, body: Value, timeout: Duration) -> Result<String, RuntimeError> {
        self.rt.chat(self.cfg, body, timeout)
    }
}

pub const NO_MODEL_REASON: &str =
    "AI 모델이 아직 설치되지 않았습니다. 설정 > AI 모델에서 설치하면 다시 분류할 수 있고, 지금은 '직접 분류'로 정리할 수 있습니다.";

/// 기록 하나를 AI 로 분류한다 (DB 잠금은 짧게만 잡는다)
pub fn classify_one(state: &AppState, note_id: i64, forced: bool) -> AppResult<()> {
    let (raw, created, roster, duty_list, auto, remind_default) = state.db.with(|c| {
        let (raw, created) = notes::raw_and_created(c, note_id)?;
        let s = settings::load(c)?;
        Ok((raw, created, students::roster(c)?, duties::duty_entries(c)?, s.ai_auto_classify, s.remind_default))
    })?;
    if !auto && !forced {
        state.db.with(|c| notes::set_status(c, note_id, "pending", Some("자동 분류가 꺼져 있습니다. 'AI로 분류'를 누르면 분류합니다."), util::now()))?;
        return Ok(());
    }
    let Some(cfg) = launch_config(&state.db, &state.paths)? else {
        state.db.with(|c| notes::set_status(c, note_id, "failed", Some(NO_MODEL_REASON), util::now()))?;
        (state.emit)("note-updated", json!({ "id": note_id }));
        return Ok(());
    };
    state.db.with(|c| notes::set_status(c, note_id, "processing", Some("AI가 정리하고 있습니다."), util::now()))?;
    (state.emit)("note-updated", json!({ "id": note_id }));

    let started = Instant::now();
    let started_at = util::fmt_dt(util::now());
    let ctx = Ctx { today: created.date(), roster: &roster, duties: &duty_list };
    let backend = RuntimeBackend { rt: &state.runtime, cfg: &cfg };
    let result = llm::classify_note(&backend, &raw, &ctx, infer_timeout(&cfg));
    let ms = started.elapsed().as_millis() as i64;
    let now = util::now();
    match result {
        Ok(out) => {
            state.db.with(|c| {
                // 처리하는 동안 기록이 지워졌으면 결과를 버린다
                if notes::raw_and_created(c, note_id).is_err() {
                    return Ok(());
                }
                notes::apply_plan(c, note_id, &out.plan, ApplyMode::Ai { model_id: &cfg.model_id }, now, &remind_default)?;
                c.execute(
                    "INSERT INTO ai_runs(note_id, model_id, started_at, finished_at, ok, duration_ms, raw_output) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)",
                    params![note_id, cfg.model_id, started_at, util::fmt_dt(now), ms, out.raw_output],
                )?;
                Ok(())
            })?;
            crate::logging::info(&format!("AI 분류 완료: note={note_id} model={} {}ms", cfg.model_id, ms));
        }
        Err(e) => {
            state.db.with(|c| {
                if notes::raw_and_created(c, note_id).is_err() {
                    return Ok(());
                }
                notes::set_status(c, note_id, "failed", Some(&format!("AI 분류 실패: {}", e.message)), now)?;
                c.execute(
                    "INSERT INTO ai_runs(note_id, model_id, started_at, finished_at, ok, error_kind, duration_ms) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
                    params![note_id, cfg.model_id, started_at, util::fmt_dt(now), format!("{:?}", e.kind), ms],
                )?;
                Ok(())
            })?;
            crate::logging::warn(&format!("AI 분류 실패: note={note_id} kind={:?}", e.kind));
        }
    }
    (state.emit)("note-updated", json!({ "id": note_id }));
    state.changed("notes");
    Ok(())
}

pub fn spawn_worker(state: Arc<AppState>, rx: Receiver<Job>) {
    std::thread::Builder::new()
        .name("ai-worker".into())
        .spawn(move || {
            while let Ok(job) = rx.recv() {
                match job {
                    Job::Shutdown => break,
                    Job::Classify { note_id, forced } => {
                        if let Err(e) = classify_one(&state, note_id, forced) {
                            crate::logging::warn(&format!("분류 작업 오류: note={note_id} {e}"));
                            let _ = state.db.with(|c| notes::set_status(c, note_id, "failed", Some(&e.to_string()), util::now()));
                            (state.emit)("note-updated", json!({ "id": note_id }));
                        }
                        state.queued.lock().unwrap_or_else(|e| e.into_inner()).remove(&note_id);
                    }
                }
            }
        })
        .expect("worker thread");
}

/// 알림 확인·AI 엔진 자동 내리기·자동 백업을 주기적으로 한다
pub fn spawn_ticker(state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("ticker".into())
        .spawn(move || {
            let mut last_backup_day = None;
            loop {
                if state.shutting_down.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let now = util::now();
                if let Err(e) = fire_due_reminders(&state, now) {
                    crate::logging::warn(&format!("알림 확인 오류: {e}"));
                }
                let idle = state
                    .db
                    .with(|c| Ok(settings::load(c)?.idle_unload_minutes))
                    .unwrap_or(10);
                if state.runtime.unload_if_idle(Duration::from_secs(idle as u64 * 60)) {
                    crate::logging::info("AI 엔진을 쓰지 않아 메모리에서 내렸습니다");
                }
                let today = now.date();
                if last_backup_day != Some(today) {
                    last_backup_day = Some(today);
                    if let Err(e) = daily_backup(&state) {
                        crate::logging::warn(&format!("자동 백업 실패: {e}"));
                    }
                }
                for _ in 0..20 {
                    if state.shutting_down.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        })
        .expect("ticker thread");
}

/// 예약 시각이 지난 알림을 보낸다. 12시간 넘게 지난 알림(프로그램이 꺼져 있던 동안)은 '놓침'으로 모아 한 번 알린다.
pub fn fire_due_reminders(state: &AppState, now: chrono::NaiveDateTime) -> AppResult<usize> {
    let due = state.db.with(|c| reminders::due(c, now))?;
    if due.is_empty() {
        return Ok(0);
    }
    let stale_before = now - ChronoDuration::hours(12);
    let mut missed = 0;
    let mut sent = 0;
    for r in &due {
        let fire_at = util::parse_dt(&r.fire_at).unwrap_or(now);
        if fire_at < stale_before {
            state.db.with(|c| reminders::mark(c, r.id, "missed", None, now))?;
            missed += 1;
            continue;
        }
        let when = r
            .date
            .as_deref()
            .and_then(util::parse_d)
            .map(crate::dates::format_ko)
            .unwrap_or_default();
        let title = format!("[{}] {}", r.label, r.title);
        let body = if when.is_empty() { "로컬 AI 교무수첩".to_string() } else { format!("{when} · 로컬 AI 교무수첩") };
        match crate::notify::show(&title, &body) {
            Ok(()) => state.db.with(|c| reminders::mark(c, r.id, "fired", None, now))?,
            Err(e) => state.db.with(|c| reminders::mark(c, r.id, "failed", Some(&e), now))?,
        }
        sent += 1;
        (state.emit)("reminder-fired", json!({ "id": r.id, "title": r.title, "label": r.label, "date": r.date }));
    }
    if missed > 0 {
        let _ = crate::notify::show(
            &format!("놓친 알림 {missed}개"),
            "프로그램이 꺼져 있는 동안 지난 알림이 있습니다. 오늘 화면에서 확인해 주세요.",
        );
        (state.emit)("reminder-fired", json!({ "missed": missed }));
    }
    state.changed("reminders");
    Ok(sent + missed)
}

pub fn backup_now(state: &AppState, label: &str) -> AppResult<std::path::PathBuf> {
    state.db.with(|c| crate::backup::backup_with(c, &state.paths.backups_dir, label))
}

/// 하루 한 번 자동 백업하고 최근 14개만 남긴다
pub fn daily_backup(state: &AppState) -> AppResult<()> {
    let today = util::now().format("%Y%m%d").to_string();
    std::fs::create_dir_all(&state.paths.backups_dir)?;
    let mut autos: Vec<std::path::PathBuf> = std::fs::read_dir(&state.paths.backups_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("notebook-") && n.ends_with("-auto.db")))
        .collect();
    if autos.iter().any(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&format!("notebook-{today}")))) {
        return Ok(());
    }
    // 기록이 없어도 명렬표·시간표·일정만 있으면 백업한다
    if !state.db.with(|c| crate::backup::has_user_data(c))? {
        return Ok(());
    }
    let p = backup_now(state, "auto")?;
    autos.push(p);
    autos.sort();
    while autos.len() > 14 {
        let old = autos.remove(0);
        let _ = std::fs::remove_file(old);
    }
    Ok(())
}

/// 시작할 때: 처리 중에 꺼진 기록을 되돌리고, 모델이 있으면 미분류 기록을 다시 대기열에 넣는다
pub fn recover_on_start(state: &AppState) -> AppResult<()> {
    let ids: Vec<i64> = state.db.with(|c| {
        c.execute(
            "UPDATE notes SET status='pending', status_reason='프로그램이 종료되어 다시 분류합니다.' WHERE status='processing'",
            [],
        )?;
        let mut st = c.prepare("SELECT id FROM notes WHERE status='pending' ORDER BY created_at")?;
        let ids = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
        Ok(ids)
    })?;
    let has_model = launch_config(&state.db, &state.paths)?.is_some();
    for id in ids {
        if has_model {
            state.enqueue(id, false);
        } else {
            state.db.with(|c| notes::set_status(c, id, "failed", Some(NO_MODEL_REASON), util::now()))?;
        }
    }
    Ok(())
}

pub fn new_queue() -> (Sender<Job>, Receiver<Job>) {
    channel()
}

pub fn require_model(state: &AppState) -> AppResult<LaunchConfig> {
    launch_config(&state.db, &state.paths)?.ok_or_else(|| AppError::msg(NO_MODEL_REASON))
}
