//! 화면(React)에서 부르는 명령

use crate::analysis::Area;
use crate::app::{self, AppState};
use crate::classify::{self, Ctx, Plan};
use crate::dates;
use crate::device::{self, DeviceInfo};
use crate::duties::{self, DutyBoard, DutyDto, DutyInput};
use crate::error::{AppError, AppResult};
use crate::llm;
use crate::models::{self, CatalogModel, DlProgress, Finish};
use crate::notes::{self, ApplyMode, NoteDto, NoteFilter};
use crate::notify;
use crate::records::{self, AreaBlock, DraftCheck, DraftDto, ObservationDto, ObservationInput};
use crate::runtime::{self, RuntimeState};
use crate::schedule::{self, CalendarItem, DutyLogDto, DutyLogInput, EventDto, EventInput, TaskDto, TaskFilter, TaskInput, TodayOverview};
use crate::search::{self, SearchQuery, SearchResult};
use crate::settings::{self, AppSettings};
use crate::students::{self, ClassDto, ImportResult, StudentDto, StudentInput};
use crate::util;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, State};

type S<'a> = State<'a, Arc<AppState>>;

/// 오래 걸리는 작업은 별도 스레드에서 한다 (화면이 멈추지 않게)
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(move || std::thread::spawn(f).join().map_err(|_| AppError::msg("작업 중 오류가 발생했습니다."))?)
        .await
        .map_err(|e| AppError::msg(e.to_string()))?
}

// ───────────────────────── 기록 ─────────────────────────

#[tauri::command]
pub async fn create_note(state: S<'_>, raw: String) -> AppResult<NoteDto> {
    let now = util::now();
    let (id, note) = state.db.with(|c| {
        let id = notes::create_note(c, &raw, now)?;
        Ok((id, notes::get_note(c, id)?))
    })?;
    crate::logging::info(&format!("기록 저장: note={id}"));
    state.changed("notes");
    state.enqueue(id, false);
    Ok(note)
}

#[tauri::command]
pub async fn list_notes(state: S<'_>, filter: Option<NoteFilter>) -> AppResult<Vec<NoteDto>> {
    state.db.with(|c| notes::list_notes(c, &filter.unwrap_or_default()))
}

#[derive(Serialize)]
pub struct AiRunDto {
    pub model_id: String,
    pub started_at: String,
    pub ok: bool,
    pub error_kind: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Serialize)]
pub struct NoteDetail {
    pub note: NoteDto,
    pub plan: Plan,
    pub has_user_changes: bool,
    pub runs: Vec<AiRunDto>,
}

#[tauri::command]
pub async fn get_note_detail(state: S<'_>, id: i64) -> AppResult<NoteDetail> {
    state.db.with(|c| {
        let note = notes::get_note(c, id)?;
        let plan = notes::current_plan(c, id)?;
        let has_user_changes = notes::has_user_changes(c, id)?;
        let mut st = c.prepare(
            "SELECT model_id, started_at, ok, error_kind, duration_ms FROM ai_runs WHERE note_id=?1 ORDER BY id DESC LIMIT 5",
        )?;
        let runs = st
            .query_map(params![id], |r| {
                Ok(AiRunDto {
                    model_id: r.get(0)?,
                    started_at: r.get(1)?,
                    ok: r.get::<_, i64>(2)? != 0,
                    error_kind: r.get(3)?,
                    duration_ms: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(NoteDetail { note, plan, has_user_changes, runs })
    })
}

#[tauri::command]
pub async fn update_note_raw(state: S<'_>, id: i64, raw: String) -> AppResult<NoteDto> {
    let n = state.db.with(|c| {
        notes::update_raw(c, id, &raw, util::now())?;
        notes::get_note(c, id)
    })?;
    state.changed("notes");
    Ok(n)
}

#[tauri::command]
pub async fn delete_note(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| notes::delete_note(c, id))?;
    crate::logging::info(&format!("기록 삭제: note={id}"));
    state.changed("all");
    Ok(())
}

#[tauri::command]
pub async fn reclassify_note(state: S<'_>, id: i64) -> AppResult<()> {
    app::require_model(&state)?;
    state.db.with(|c| notes::set_status(c, id, "pending", Some("AI 분류를 기다리고 있습니다."), util::now()))?;
    (state.emit)("note-updated", json!({ "id": id }));
    state.enqueue(id, true);
    Ok(())
}

#[tauri::command]
pub async fn reclassify_unclassified(state: S<'_>) -> AppResult<usize> {
    app::require_model(&state)?;
    let ids: Vec<i64> = state.db.with(|c| {
        let mut st = c.prepare("SELECT id FROM notes WHERE status IN ('pending','failed') ORDER BY created_at")?;
        let ids = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
        for id in &ids {
            notes::set_status(c, *id, "pending", Some("AI 분류를 기다리고 있습니다."), util::now())?;
        }
        Ok(ids)
    })?;
    for id in &ids {
        state.enqueue(*id, true);
    }
    state.changed("notes");
    Ok(ids.len())
}

/// 직접 분류 화면에 미리 채울 규칙 기반 제안 (AI 없이도 동작)
#[tauri::command]
pub async fn suggest_plan(state: S<'_>, id: i64) -> AppResult<Plan> {
    state.db.with(|c| {
        let (raw, created) = notes::raw_and_created(c, id)?;
        let roster = students::roster(c)?;
        let duty_list = duties::duty_entries(c)?;
        Ok(classify::rule_plan(&raw, &Ctx { today: created.date(), roster: &roster, duties: &duty_list }))
    })
}

#[tauri::command]
pub async fn save_plan(state: S<'_>, id: i64, plan: Plan) -> AppResult<NoteDto> {
    let n = state.db.with(|c| {
        let remind = settings::load(c)?.remind_default;
        notes::apply_plan(c, id, &plan, ApplyMode::Manual, util::now(), &remind)?;
        notes::get_note(c, id)
    })?;
    crate::logging::info(&format!("직접 분류 저장: note={id}"));
    state.changed("all");
    Ok(n)
}

#[derive(Serialize)]
pub struct ResolvedDate {
    pub date: String,
    pub label: String,
    pub expr: String,
    pub approximate: bool,
    pub deadline: bool,
}

/// 교사가 적은 날짜 표현("다음 주 수요일")을 기준 날짜로 해석해 미리 보여 준다
#[tauri::command]
pub async fn resolve_date_expr(expr: String, base: Option<String>) -> AppResult<Option<ResolvedDate>> {
    let base = util::opt_date(base.as_deref())?.and_then(|d| util::parse_d(&d)).unwrap_or_else(|| util::now().date());
    Ok(dates::parse_date_expr(&expr, base).map(|h| ResolvedDate {
        date: util::fmt_d(h.date),
        label: dates::format_ko(h.date),
        expr: h.expr,
        approximate: h.approximate,
        deadline: h.deadline,
    }))
}

// ───────────────────────── 학생 ─────────────────────────

#[tauri::command]
pub async fn list_students(state: S<'_>, query: Option<String>, class_id: Option<i64>) -> AppResult<Vec<StudentDto>> {
    state.db.with(|c| students::list_students(c, query.as_deref(), class_id))
}

#[tauri::command]
pub async fn list_classes(state: S<'_>) -> AppResult<Vec<ClassDto>> {
    state.db.with(|c| students::list_classes(c))
}

#[derive(Serialize)]
pub struct StudentDetail {
    pub student: StudentDto,
    pub observations: Vec<ObservationDto>,
    pub notes: Vec<NoteDto>,
    pub drafts: Vec<DraftDto>,
}

#[tauri::command]
pub async fn get_student_detail(state: S<'_>, id: i64) -> AppResult<StudentDetail> {
    state.db.with(|c| {
        Ok(StudentDetail {
            student: students::get_student(c, id)?,
            observations: records::student_observations(c, id)?,
            notes: notes::list_notes(c, &NoteFilter { student_id: Some(id), limit: Some(500), ..Default::default() })?,
            drafts: records::student_drafts(c, id)?,
        })
    })
}

#[tauri::command]
pub async fn create_student(state: S<'_>, input: StudentInput) -> AppResult<i64> {
    let id = state.db.with(|c| students::create_student(c, &input, util::now()))?;
    state.changed("students");
    Ok(id)
}

#[tauri::command]
pub async fn update_student(state: S<'_>, id: i64, input: StudentInput) -> AppResult<()> {
    state.db.with(|c| students::update_student(c, id, &input, util::now()))?;
    state.changed("students");
    Ok(())
}

#[tauri::command]
pub async fn delete_student(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| students::delete_student(c, id))?;
    state.changed("students");
    Ok(())
}

#[tauri::command]
pub async fn merge_students(state: S<'_>, from_id: i64, into_id: i64) -> AppResult<()> {
    state.db.with(|c| students::merge_students(c, from_id, into_id))?;
    state.changed("students");
    Ok(())
}

#[tauri::command]
pub async fn import_roster(state: S<'_>, text: String) -> AppResult<ImportResult> {
    let r = state.db.with(|c| students::import_roster(c, &text, util::now()))?;
    state.changed("students");
    Ok(r)
}

// ───────────────────────── 학생부 자료 ─────────────────────────

#[tauri::command]
pub async fn record_overview(state: S<'_>, student_id: i64) -> AppResult<Vec<AreaBlock>> {
    state.db.with(|c| records::record_overview(c, student_id))
}

#[tauri::command]
pub async fn unassigned_observations(state: S<'_>) -> AppResult<Vec<ObservationDto>> {
    state.db.with(|c| records::unassigned_observations(c))
}

#[tauri::command]
pub async fn create_observation(state: S<'_>, input: ObservationInput) -> AppResult<i64> {
    let id = state.db.with(|c| records::create_observation(c, &input, util::now()))?;
    state.changed("records");
    Ok(id)
}

#[tauri::command]
pub async fn update_observation(state: S<'_>, id: i64, input: ObservationInput) -> AppResult<()> {
    state.db.with(|c| records::update_observation(c, id, &input, util::now()))?;
    state.changed("records");
    Ok(())
}

#[tauri::command]
pub async fn delete_observation(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| records::delete_observation(c, id))?;
    state.changed("records");
    Ok(())
}

fn source_texts(c: &rusqlite::Connection, student_id: i64, ids: &[i64]) -> AppResult<Vec<ObservationDto>> {
    let all = records::student_observations(c, student_id)?;
    Ok(all.into_iter().filter(|o| ids.contains(&o.id)).collect())
}

#[tauri::command]
pub async fn create_draft_template(state: S<'_>, student_id: i64, area: String, observation_ids: Vec<i64>) -> AppResult<DraftDto> {
    let d = state.db.with(|c| {
        let obs = source_texts(c, student_id, &observation_ids)?;
        if obs.is_empty() {
            return Err(AppError::invalid("초안에 쓸 관찰 기록을 하나 이상 골라 주세요."));
        }
        let body = records::template_body(&obs);
        let ids: Vec<i64> = obs.iter().map(|o| o.id).collect();
        let id = records::create_draft(c, student_id, &area, &ids, &body, "template", util::now())?;
        records::get_draft(c, id)
    })?;
    state.changed("records");
    Ok(d)
}

#[tauri::command]
pub async fn generate_draft_ai(state: S<'_>, student_id: i64, area: String, observation_ids: Vec<i64>) -> AppResult<DraftDto> {
    let st = state.inner().clone();
    let d = blocking(move || {
        let obs = st.db.with(|c| source_texts(c, student_id, &observation_ids))?;
        if obs.is_empty() {
            return Err(AppError::invalid("초안에 쓸 관찰 기록을 하나 이상 골라 주세요."));
        }
        let cfg = app::require_model(&st)?;
        let area_v = Area::parse(&area);
        let lines: Vec<String> = obs.iter().map(|o| crate::text::tidy(&o.content)).collect();
        let out = st
            .runtime
            .chat(&cfg, llm::draft_body(area_v, &lines), app::infer_timeout(&cfg))
            .map_err(|e| AppError::msg(format!("AI 초안을 만들지 못했습니다: {}", e.message)))?;
        let body = llm::clean_draft(&out);
        if body.trim().is_empty() {
            return Err(AppError::msg("AI가 빈 초안을 돌려주었습니다. 다시 시도하거나 기본 초안을 쓰세요."));
        }
        let ids: Vec<i64> = obs.iter().map(|o| o.id).collect();
        st.db.with(|c| {
            let id = records::create_draft(c, student_id, &area, &ids, &body, "ai", util::now())?;
            records::get_draft(c, id)
        })
    })
    .await?;
    state.changed("records");
    Ok(d)
}

#[tauri::command]
pub async fn update_draft(state: S<'_>, id: i64, body: String, reviewed: bool) -> AppResult<DraftDto> {
    let d = state.db.with(|c| {
        records::update_draft(c, id, &body, reviewed, util::now())?;
        records::get_draft(c, id)
    })?;
    state.changed("records");
    Ok(d)
}

#[tauri::command]
pub async fn delete_draft(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| records::delete_draft(c, id))?;
    state.changed("records");
    Ok(())
}

#[tauri::command]
pub async fn check_draft_text(state: S<'_>, draft_id: i64, body: String) -> AppResult<DraftCheck> {
    state.db.with(|c| {
        let d = records::get_draft(c, draft_id)?;
        let obs = source_texts(c, d.student_id, &d.source_ids)?;
        let texts: Vec<String> = obs.into_iter().map(|o| o.content).collect();
        Ok(records::check_draft(&body, &texts))
    })
}

// ───────────────────────── 업무 ─────────────────────────

#[tauri::command]
pub async fn list_duties(state: S<'_>) -> AppResult<Vec<DutyDto>> {
    state.db.with(|c| duties::list_duties(c))
}

#[tauri::command]
pub async fn create_duty(state: S<'_>, input: DutyInput) -> AppResult<i64> {
    let id = state.db.with(|c| duties::create_duty(c, &input, util::now()))?;
    state.changed("duties");
    Ok(id)
}

#[tauri::command]
pub async fn update_duty(state: S<'_>, id: i64, input: DutyInput) -> AppResult<()> {
    state.db.with(|c| duties::update_duty(c, id, &input, util::now()))?;
    state.changed("duties");
    Ok(())
}

#[tauri::command]
pub async fn delete_duty(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| duties::delete_duty(c, id))?;
    state.changed("duties");
    Ok(())
}

#[tauri::command]
pub async fn duty_board(state: S<'_>, duty_id: Option<i64>) -> AppResult<DutyBoard> {
    state.db.with(|c| duties::duty_board(c, duty_id))
}

#[tauri::command]
pub async fn create_duty_log(state: S<'_>, input: DutyLogInput) -> AppResult<i64> {
    let id = state.db.with(|c| {
        let remind = settings::load(c)?.remind_default;
        schedule::create_duty_log(c, &input, util::now(), &remind)
    })?;
    state.changed("duties");
    Ok(id)
}

#[tauri::command]
pub async fn update_duty_log(state: S<'_>, id: i64, input: DutyLogInput) -> AppResult<DutyLogDto> {
    let d = state.db.with(|c| {
        schedule::update_duty_log(c, id, &input, util::now())?;
        schedule::get_duty_log(c, id)
    })?;
    state.changed("duties");
    Ok(d)
}

#[tauri::command]
pub async fn set_duty_log_done(state: S<'_>, id: i64, done: bool) -> AppResult<()> {
    state.db.with(|c| schedule::set_duty_log_done(c, id, done, util::now()))?;
    state.changed("duties");
    Ok(())
}

#[tauri::command]
pub async fn delete_duty_log(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| schedule::delete_duty_log(c, id))?;
    state.changed("duties");
    Ok(())
}

// ───────────────────────── 할 일 ─────────────────────────

#[tauri::command]
pub async fn list_tasks(state: S<'_>, filter: Option<TaskFilter>) -> AppResult<Vec<TaskDto>> {
    state.db.with(|c| schedule::list_tasks(c, &filter.unwrap_or_default()))
}

#[tauri::command]
pub async fn create_task(state: S<'_>, input: TaskInput) -> AppResult<TaskDto> {
    let t = state.db.with(|c| {
        let remind = settings::load(c)?.remind_default;
        let id = schedule::create_task(c, &input, util::now(), &remind)?;
        schedule::get_task(c, id)
    })?;
    state.changed("tasks");
    Ok(t)
}

#[tauri::command]
pub async fn update_task(state: S<'_>, id: i64, input: TaskInput) -> AppResult<TaskDto> {
    let t = state.db.with(|c| {
        schedule::update_task(c, id, &input, util::now())?;
        schedule::get_task(c, id)
    })?;
    state.changed("tasks");
    Ok(t)
}

#[tauri::command]
pub async fn set_task_done(state: S<'_>, id: i64, done: bool) -> AppResult<()> {
    state.db.with(|c| schedule::set_task_done(c, id, done, util::now()))?;
    state.changed("tasks");
    Ok(())
}

#[tauri::command]
pub async fn delete_task(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| schedule::delete_task(c, id))?;
    state.changed("tasks");
    Ok(())
}

#[tauri::command]
pub async fn get_task(state: S<'_>, id: i64) -> AppResult<TaskDto> {
    state.db.with(|c| schedule::get_task(c, id))
}

#[tauri::command]
pub async fn get_duty_log(state: S<'_>, id: i64) -> AppResult<DutyLogDto> {
    state.db.with(|c| schedule::get_duty_log(c, id))
}

// ───────────────────────── 일정 ─────────────────────────

#[tauri::command]
pub async fn calendar(state: S<'_>, view: String, start: String, end: String) -> AppResult<Vec<CalendarItem>> {
    state.db.with(|c| schedule::calendar(c, &view, &start, &end))
}

#[tauri::command]
pub async fn get_event(state: S<'_>, id: i64) -> AppResult<EventDto> {
    state.db.with(|c| schedule::get_event(c, id))
}

#[tauri::command]
pub async fn create_event(state: S<'_>, input: EventInput) -> AppResult<i64> {
    let id = state.db.with(|c| {
        let remind = settings::load(c)?.remind_default;
        schedule::create_event(c, &input, util::now(), &remind)
    })?;
    state.changed("events");
    Ok(id)
}

#[tauri::command]
pub async fn update_event(state: S<'_>, id: i64, input: EventInput) -> AppResult<EventDto> {
    let e = state.db.with(|c| {
        schedule::update_event(c, id, &input, util::now())?;
        schedule::get_event(c, id)
    })?;
    state.changed("events");
    Ok(e)
}

#[tauri::command]
pub async fn delete_event(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| schedule::delete_event(c, id))?;
    state.changed("events");
    Ok(())
}

// ───────────────────────── 오늘·검색 ─────────────────────────

#[tauri::command]
pub async fn today_overview(state: S<'_>) -> AppResult<TodayOverview> {
    let now = util::now();
    state.db.with(|c| schedule::today_overview(c, now.date(), now))
}

#[tauri::command]
pub async fn search(state: S<'_>, query: SearchQuery) -> AppResult<SearchResult> {
    state.db.with(|c| search::search(c, &query))
}

// ───────────────────────── AI 모델 ─────────────────────────

#[derive(Serialize)]
pub struct CatalogEntry {
    pub model: CatalogModel,
    pub installed: bool,
    pub verified_at: Option<String>,
    pub selected: bool,
    pub loaded: bool,
    pub recommended: bool,
    pub part_bytes: u64,
    pub progress: Option<DlProgress>,
    /// ok | tight | too_big | unknown — 이 PC 메모리에 맞는지
    pub fit: String,
}

#[derive(Serialize)]
pub struct ModelsView {
    pub entries: Vec<CatalogEntry>,
    pub selected: Option<String>,
    pub device: Option<DeviceInfo>,
    pub models_dir: String,
}

#[tauri::command]
pub async fn model_catalog(state: S<'_>) -> AppResult<ModelsView> {
    let snapshot = state.downloader.snapshot();
    let loaded = state.runtime.loaded_model();
    let dir = state.paths.models_dir.clone();
    state.db.with(|c| {
        let s = settings::load(c)?;
        let inst = models::installed(c)?;
        let dev = device::cached(c)?;
        let rec = dev.as_ref().map(|d| d.recommended_model.clone());
        let ram_gb = dev.as_ref().map(|d| d.ram_total_bytes as f64 / 1073741824.0);
        let entries = models::CATALOG
            .iter()
            .map(|m| {
                let im = inst.iter().find(|i| i.model_id == m.id);
                let fit = match ram_gb {
                    None => "unknown",
                    Some(g) if g + 0.5 < m.min_ram_gb as f64 => "too_big",
                    Some(g) if g + 0.5 < m.recommended_ram_gb as f64 => "tight",
                    _ => "ok",
                };
                CatalogEntry {
                    model: m.clone(),
                    installed: im.is_some(),
                    verified_at: im.and_then(|i| i.verified_at.clone()),
                    selected: s.selected_model.as_deref() == Some(m.id),
                    loaded: loaded.as_deref() == Some(m.id),
                    recommended: rec.as_deref() == Some(m.id),
                    part_bytes: std::fs::metadata(models::part_path(&dir, m)).map(|x| x.len()).unwrap_or(0),
                    progress: snapshot.get(m.id).cloned(),
                    fit: fit.into(),
                }
            })
            .collect();
        Ok(ModelsView { entries, selected: s.selected_model, device: dev, models_dir: dir.display().to_string() })
    })
}

fn on_installed(st: &Arc<AppState>, m: &'static CatalogModel) {
    let now = util::now();
    let res = st.db.with(|c| {
        models::mark_installed(c, m, now)?;
        // 선택한 모델이 없으면 방금 설치한 모델을 쓴다
        settings::update(c, |s| {
            if s.selected_model.is_none() {
                s.selected_model = Some(m.id.to_string());
            }
        })?;
        Ok(())
    });
    if let Err(e) = res {
        crate::logging::error(&format!("모델 설치 기록 실패: {e}"));
    }
    crate::logging::info(&format!("모델 설치 완료: {}", m.id));
    (st.emit)("models-changed", json!({ "id": m.id }));
}

#[tauri::command]
pub async fn start_download(state: S<'_>, id: String) -> AppResult<()> {
    let m = models::find(&id).ok_or_else(|| AppError::not_found("목록에 없는 모델입니다."))?;
    let st = state.inner().clone();
    let url = std::env::var("TN_MODEL_URL_OVERRIDE").ok().filter(|_| cfg!(debug_assertions)).unwrap_or_else(|| m.url.to_string());
    state.downloader.start(m, url, state.paths.models_dir.clone(), move |res| match res {
        Ok(Finish::Installed) => on_installed(&st, m),
        Ok(_) => (st.emit)("models-changed", json!({ "id": m.id })),
        Err(e) => {
            crate::logging::warn(&format!("모델 다운로드 실패: {} ({e})", m.id));
            (st.emit)("models-changed", json!({ "id": m.id }));
        }
    })?;
    crate::logging::info(&format!("모델 다운로드 시작: {}", m.id));
    Ok(())
}

#[tauri::command]
pub async fn pause_download(state: S<'_>, id: String) -> AppResult<()> {
    state.downloader.pause(&id);
    Ok(())
}

#[tauri::command]
pub async fn cancel_download(state: S<'_>, id: String) -> AppResult<()> {
    state.downloader.cancel(&id, &state.paths.models_dir);
    (state.emit)("models-changed", json!({ "id": id }));
    Ok(())
}

#[tauri::command]
pub async fn select_model(state: S<'_>, id: String) -> AppResult<()> {
    let st = state.inner().clone();
    state.db.with(|c| {
        if models::get_installed(c, &id)?.is_none() {
            return Err(AppError::invalid("설치된 모델만 고를 수 있습니다."));
        }
        settings::update(c, |s| s.selected_model = Some(id.clone()))?;
        Ok(())
    })?;
    // 다른 모델이 올라와 있으면 내린다 (다음 분류 때 새 모델로 켜진다)
    if st.runtime.loaded_model().is_some_and(|l| l != id) {
        blocking(move || {
            st.runtime.stop();
            Ok(())
        })
        .await?;
    }
    (state.emit)("models-changed", json!({}));
    Ok(())
}

/// 모델 삭제: 실행 중이면 AI 엔진을 멈추고, 파일·설치 기록·선택 상태·화면 상태를 함께 정리한다
#[tauri::command]
pub async fn delete_model(state: S<'_>, id: String) -> AppResult<()> {
    let m = models::find(&id).ok_or_else(|| AppError::not_found("목록에 없는 모델입니다."))?;
    let st = state.inner().clone();
    blocking(move || {
        st.downloader.pause(m.id);
        if st.runtime.loaded_model().as_deref() == Some(m.id) {
            st.runtime.stop();
        }
        // 파일 잠금이 풀릴 때까지 잠깐 기다리며 지운다
        let mut last = None;
        for _ in 0..10 {
            match models::delete_files(&st.paths.models_dir, m) {
                Ok(()) => {
                    last = None;
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
            }
        }
        if let Some(e) = last {
            return Err(e);
        }
        st.db.with(|c| {
            models::unmark_installed(c, m.id)?;
            settings::update(c, |s| {
                if s.selected_model.as_deref() == Some(m.id) {
                    s.selected_model = None;
                }
            })?;
            Ok(())
        })?;
        st.downloader.clear(m.id);
        crate::logging::info(&format!("모델 삭제: {}", m.id));
        Ok(())
    })
    .await?;
    (state.emit)("models-changed", json!({ "id": id, "deleted": true }));
    Ok(())
}

#[derive(Serialize)]
pub struct VerifyResult {
    pub ok: bool,
    pub message: String,
}

#[tauri::command]
pub async fn verify_model(state: S<'_>, id: String) -> AppResult<VerifyResult> {
    let m = models::find(&id).ok_or_else(|| AppError::not_found("목록에 없는 모델입니다."))?;
    let st = state.inner().clone();
    let r = blocking(move || {
        let path = models::model_path(&st.paths.models_dir, m);
        let len = std::fs::metadata(&path).map(|x| x.len()).map_err(|_| AppError::msg("모델 파일이 없습니다. 다시 설치해 주세요."))?;
        if len != m.size_bytes {
            return Ok(VerifyResult { ok: false, message: "파일 크기가 맞지 않습니다. 손상되었을 수 있으니 삭제 후 다시 받아 주세요.".into() });
        }
        let emit = st.emit.clone();
        let hash = models::sha256_file(&path, None, |done| {
            emit(
                "model-download",
                json!(DlProgress { model_id: m.id.into(), status: "verifying".into(), received: done, total: len, bytes_per_sec: 0, message: None }),
            )
        })
        .map_err(|e| AppError::msg(e.to_string()))?
        .unwrap_or_default();
        st.downloader.clear(m.id);
        if hash.eq_ignore_ascii_case(m.sha256) {
            st.db.with(|c| models::set_verified(c, m.id, util::now()))?;
            Ok(VerifyResult { ok: true, message: "파일이 온전합니다 (SHA-256 일치).".into() })
        } else {
            Ok(VerifyResult { ok: false, message: "파일 내용이 원본과 다릅니다 (SHA-256 불일치). 삭제 후 다시 받아 주세요.".into() })
        }
    })
    .await?;
    (state.emit)("models-changed", json!({ "id": id }));
    Ok(r)
}

#[tauri::command]
pub async fn import_model_file(state: S<'_>, path: String) -> AppResult<String> {
    let st = state.inner().clone();
    let id = blocking(move || {
        let cancel = AtomicBool::new(false);
        let m = st
            .downloader
            .import_file(std::path::Path::new(&path), &st.paths.models_dir, &cancel)
            .map_err(AppError::msg)?;
        on_installed(&st, m);
        Ok(m.id.to_string())
    })
    .await?;
    Ok(id)
}

#[derive(Serialize)]
pub struct RuntimeStatus {
    pub state: RuntimeState,
    pub runtime_found: bool,
    pub runtime_dir: Option<String>,
    pub loaded_model: Option<String>,
    /// AI 엔진이 연결을 기다리는 주소 (127.0.0.1 전용인지 확인)
    pub listening: Vec<String>,
    pub local_only: Option<bool>,
    pub diagnostics: Vec<String>,
}

/// 지금 기록하면 AI가 정리할 수 있는지 (선택·설치된 모델이 있고 자동 분류가 켜져 있는지)
#[tauri::command]
pub async fn ai_available(state: S<'_>) -> AppResult<bool> {
    let auto = state.db.with(|c| Ok(settings::load(c)?.ai_auto_classify))?;
    Ok(auto && app::launch_config(&state.db, &state.paths)?.is_some())
}

#[tauri::command]
pub async fn runtime_status(state: S<'_>) -> AppResult<RuntimeStatus> {
    let rt = &state.runtime;
    let listening = rt.pid().map(runtime::listening_addresses).unwrap_or_default();
    let local_only = if listening.is_empty() { None } else { Some(listening.iter().all(|a| a.starts_with("127.0.0.1:"))) };
    Ok(RuntimeStatus {
        state: rt.state(),
        runtime_found: rt.runtime_dir().is_some(),
        runtime_dir: rt.runtime_dir().map(|p| p.display().to_string()),
        loaded_model: rt.loaded_model(),
        listening,
        local_only,
        diagnostics: rt.diagnostics(),
    })
}

#[tauri::command]
pub async fn runtime_start(state: S<'_>) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let cfg = app::require_model(&st)?;
        st.runtime.ensure_ready(&cfg).map_err(|e| AppError::msg(e.message))?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn runtime_stop(state: S<'_>) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        st.runtime.stop();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn device_info(state: S<'_>, refresh: bool) -> AppResult<DeviceInfo> {
    if !refresh {
        if let Some(d) = state.db.with(|c| device::cached(c))? {
            return Ok(d);
        }
    }
    let st = state.inner().clone();
    blocking(move || {
        let info = device::probe(&st.paths.root);
        st.db.with(|c| device::store(c, &info))?;
        Ok(info)
    })
    .await
}

// ───────────────────────── 설정·데이터 ─────────────────────────

#[derive(Serialize)]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub models_dir: String,
    pub exe_dir: String,
    pub db_path: String,
    pub autostart: bool,
    pub wipe_pending: bool,
    pub data_dir_name: String,
}

#[tauri::command]
pub async fn app_info(state: S<'_>) -> AppResult<AppInfo> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: state.paths.root.display().to_string(),
        models_dir: state.paths.models_dir.display().to_string(),
        exe_dir: crate::paths::exe_dir().display().to_string(),
        db_path: state.paths.db_path.display().to_string(),
        autostart: notify::autostart_target().is_some(),
        wipe_pending: state.paths.wipe_marker.exists(),
        data_dir_name: crate::paths::DATA_DIR_NAME.into(),
    })
}

#[tauri::command]
pub async fn get_settings(state: S<'_>) -> AppResult<AppSettings> {
    state.db.with(|c| settings::load(c))
}

#[tauri::command]
pub async fn update_settings(state: S<'_>, value: AppSettings) -> AppResult<AppSettings> {
    let s = state.db.with(|c| {
        let cur = settings::load(c)?;
        let mut next = value.clone();
        // 모델 선택은 select_model 로만 바꾼다
        next.selected_model = cur.selected_model;
        settings::save(c, &next)?;
        settings::load(c)
    })?;
    (state.emit)("settings-changed", json!({}));
    Ok(s)
}

#[tauri::command]
pub async fn set_autostart(enabled: bool) -> AppResult<bool> {
    let exe = std::env::current_exe()?;
    notify::set_autostart(enabled, &exe).map_err(AppError::msg)?;
    Ok(notify::autostart_target().is_some())
}

#[tauri::command]
pub async fn test_notification() -> AppResult<String> {
    blocking(|| {
        notify::show("테스트 알림", "알림이 이렇게 화면 오른쪽 아래에 표시됩니다.").map_err(AppError::msg)?;
        Ok("테스트 알림을 보냈습니다. 화면 오른쪽 아래를 확인해 주세요. 보이지 않으면 Windows '방해 금지(집중 지원)'가 켜져 있는지 확인해 주세요.".to_string())
    })
    .await
}

#[tauri::command]
pub async fn open_folder(app: AppHandle, state: S<'_>, which: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let p = match which.as_str() {
        "data" => state.paths.root.clone(),
        "models" => state.paths.models_dir.clone(),
        "backups" => state.paths.backups_dir.clone(),
        "logs" => state.paths.logs_dir.clone(),
        _ => return Err(AppError::invalid("알 수 없는 폴더입니다.")),
    };
    std::fs::create_dir_all(&p)?;
    app.opener().open_path(p.display().to_string(), None::<&str>).map_err(|e| AppError::msg(format!("폴더를 열지 못했습니다: {e}")))
}

#[derive(Serialize)]
pub struct BackupFile {
    pub name: String,
    pub size: u64,
    pub modified: String,
}

#[tauri::command]
pub async fn list_backups(state: S<'_>) -> AppResult<Vec<BackupFile>> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&state.paths.backups_dir) {
        for e in rd.flatten() {
            let md = e.metadata()?;
            let modified = md
                .modified()
                .ok()
                .map(|t| chrono::DateTime::<chrono::Local>::from(t).naive_local())
                .map(util::fmt_dt)
                .unwrap_or_default();
            out.push(BackupFile { name: e.file_name().to_string_lossy().to_string(), size: md.len(), modified });
        }
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

#[tauri::command]
pub async fn backup_now(state: S<'_>) -> AppResult<String> {
    let st = state.inner().clone();
    let p = blocking(move || app::backup_now(&st, "manual")).await?;
    Ok(p.display().to_string())
}

#[derive(Deserialize)]
pub struct WipeRequest {
    pub confirm_text: String,
}

pub const WIPE_PHRASE: &str = "모두 삭제";

/// 모든 로컬 데이터 삭제 요청: 확인 문구 + 운영체제 확인 창을 모두 거친 뒤, 다음 시작 때 지운다
#[tauri::command]
pub async fn request_wipe(app: AppHandle, state: S<'_>, request: WipeRequest) -> AppResult<bool> {
    if request.confirm_text.trim() != WIPE_PHRASE {
        return Err(AppError::invalid(format!("확인 문구 '{WIPE_PHRASE}'를 정확히 입력해 주세요.")));
    }
    let st = state.inner().clone();
    let confirmed = blocking(move || {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        Ok(app
            .dialog()
            .message(format!(
                "이 PC에 저장된 기록, 학생 정보, 일정, AI 모델, 백업을 모두 지웁니다.\n되돌릴 수 없습니다.\n\n삭제 위치: {}\n\n프로그램을 다시 시작할 때 지워집니다. 계속할까요?",
                st.paths.root.display()
            ))
            .title("모든 로컬 데이터 삭제")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom("삭제 예약".into(), "취소".into()))
            .blocking_show())
    })
    .await?;
    if !confirmed {
        return Ok(false);
    }
    std::fs::write(&state.paths.wipe_marker, util::fmt_dt(util::now()))?;
    crate::logging::warn("모든 로컬 데이터 삭제가 예약되었습니다 (다음 시작 때 실행)");
    Ok(true)
}

#[tauri::command]
pub async fn cancel_wipe(state: S<'_>) -> AppResult<()> {
    if state.paths.wipe_marker.exists() {
        std::fs::remove_file(&state.paths.wipe_marker)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn restart_app(app: AppHandle) -> AppResult<()> {
    app.restart();
}

pub static FRONTEND_READY: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn frontend_ready() -> AppResult<()> {
    FRONTEND_READY.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn show_main_window(app: AppHandle) -> AppResult<()> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    Ok(())
}
