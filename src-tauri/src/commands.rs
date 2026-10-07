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
use crate::records::{self, DraftCheck, DraftDto, ObservationDto, ObservationInput};
use crate::runtime::{self, RuntimeState};
use crate::schedule::{self, CalendarItem, DutyLogDto, DutyLogInput, EventDto, EventInput, TaskDto, TaskFilter, TaskInput, TodayOverview};
use crate::search::{self, SearchQuery, SearchResult};
use crate::settings::{self, AppSettings};
use crate::students::{self, ClassDto, ImportResult, StudentDto, StudentInput};
use crate::timetable;
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
    let (id, note, tt) = state.db.with(|c| {
        let id = notes::create_note(c, &raw, now)?;
        // 시간표 문장은 AI 를 기다리지 않고 규칙으로 바로 시간표에 반영한다
        let tt = crate::ttinput::apply_for_note(c, id, now).unwrap_or_else(|e| {
            crate::logging::warn(&format!("시간표 반영 실패: note={id} {e}"));
            Default::default()
        });
        if tt.covers_all {
            notes::mark_rule_classified(c, id, now)?;
        }
        Ok((id, notes::get_note(c, id)?, tt))
    })?;
    crate::logging::info(&format!("기록 저장: note={id} 시간표 {}/{}/{}", tt.applied, tt.conflicts, tt.changes));
    state.changed("notes");
    if tt.statements > 0 {
        state.changed("timetable");
    }
    // 기록 전체가 시간표 문장이면 AI 분류가 필요 없다
    if !tt.covers_all {
        state.enqueue(id, false);
    }
    Ok(note)
}

#[tauri::command]
pub async fn tt_conflicts(state: S<'_>) -> AppResult<Vec<crate::ttinput::TtEntry>> {
    state.db.with(|c| crate::ttinput::conflicts(c))
}

/// 다른 수업이 있던 시간표 칸: 바꾸기 / 그대로 두기
#[tauri::command]
pub async fn resolve_tt_conflict(state: S<'_>, id: i64, replace: bool) -> AppResult<crate::ttinput::TtEntry> {
    let e = state.db.with(|c| crate::ttinput::resolve_conflict(c, id, replace, util::now()))?;
    state.changed("timetable");
    state.changed("notes");
    Ok(e)
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
        let now = util::now();
        notes::update_raw(c, id, &raw, now)?;
        // 고친 원문의 시간표 문장을 다시 반영한다
        if let Err(e) = crate::ttinput::apply_for_note(c, id, now) {
            crate::logging::warn(&format!("시간표 반영 실패: note={id} {e}"));
        }
        notes::get_note(c, id)
    })?;
    state.changed("notes");
    state.changed("timetable");
    Ok(n)
}

#[tauri::command]
pub async fn delete_note(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| notes::delete_note(c, id))?;
    crate::logging::info(&format!("기록 삭제: note={id}"));
    state.changed("all");
    Ok(())
}

// ───────────────────────── 기록 속 개인정보 → 학생 명렬표 ─────────────────────────

/// "기존 → 새 값" 확인: accept=true 면 새 값으로 (보호자 연락처 4개가 찼으면 replace_id 를 바꾼다)
#[tauri::command]
pub async fn resolve_profile_change(state: S<'_>, id: i64, accept: bool, replace_id: Option<i64>) -> AppResult<()> {
    state.db.with(|c| crate::profile::resolve_change(c, id, accept, replace_id, util::now()))?;
    state.changed("students");
    Ok(())
}

/// "이 내용을 어디에 둘까요?": choice = record(학생부 자료) | roster(학생 명렬표 특이사항)
#[tauri::command]
pub async fn answer_note_question(state: S<'_>, id: i64, choice: String) -> AppResult<()> {
    state.db.with(|c| crate::profile::answer_question(c, id, &choice, util::now()))?;
    state.changed("all");
    Ok(())
}

/// 메뉴를 열었다 → 그 메뉴의 '새 항목' 숫자를 지우고, 이번 방문에 '새' 표시할 기준 시각(전에 본 시각)을 돌려준다
#[tauri::command]
pub async fn mark_seen(state: S<'_>, menu: String) -> AppResult<Option<String>> {
    state.db.with(|c| crate::seen::mark_seen(c, &menu, util::now()))
}

/// 기준 시각 뒤에 AI 가 새로 넣은 항목 (학생부 자료 점 표시, 학생 명렬표 '새' 표시)
#[tauri::command]
pub async fn new_since(state: S<'_>, since: String) -> AppResult<crate::seen::NewSince> {
    state.db.with(|c| crate::seen::new_since(c, &since))
}

/// 보호자 연락처 (최대 4개, 관계 + 번호)
#[tauri::command]
pub async fn set_student_contacts(state: S<'_>, student_id: i64, contacts: Vec<crate::profile::Contact>) -> AppResult<Vec<crate::profile::Contact>> {
    let v = state.db.with(|c| crate::profile::set_contacts(c, student_id, &contacts, util::now()))?;
    state.changed("students");
    Ok(v)
}

/// 기록을 일반 기록 / 정리된 업무기록으로 옮긴다 (track: "general" | "work" | null = AI 분류를 따름)
#[tauri::command]
pub async fn set_note_track(state: S<'_>, id: i64, track: Option<String>) -> AppResult<()> {
    state.db.with(|c| notes::set_track(c, id, track.as_deref(), util::now()))?;
    state.changed("notes");
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
    /// 기록에서 들어온 학생 명렬표 변경 이력
    pub profile_history: Vec<crate::profile::ProfileChange>,
}

#[tauri::command]
pub async fn get_student_detail(state: S<'_>, id: i64) -> AppResult<StudentDetail> {
    state.db.with(|c| {
        Ok(StudentDetail {
            student: students::get_student(c, id)?,
            observations: records::student_observations(c, id)?,
            notes: notes::list_notes(c, &NoteFilter { student_id: Some(id), limit: Some(500), ..Default::default() })?,
            drafts: records::student_drafts(c, id)?,
            profile_history: crate::profile::history(c, id)?,
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

/// 나이스 명렬표 엑셀 가져오기 (여러 파일)
#[tauri::command]
pub async fn import_neis_roster(state: S<'_>, paths: Vec<String>) -> AppResult<crate::neis_roster::NeisImportResult> {
    if paths.is_empty() {
        return Err(AppError::invalid("가져올 파일을 골라 주세요."));
    }
    let st = state.inner().clone();
    let r = blocking(move || {
        st.db.with(|c| {
            let store_rrn = settings::load(c)?.store_rrn;
            crate::neis_roster::import_files(c, &paths, store_rrn, util::now())
        })
    })
    .await?;
    state.changed("students");
    Ok(r)
}

// ───────────────────────── 학생부 자료 ─────────────────────────

// ───────────────────────── 오늘 업무 요약 ─────────────────────────

/// 오늘 업무 요약 (같은 사실로 만든 AI 요약이 있으면 그것, 없으면 규칙 요약)
#[tauri::command]
pub async fn today_briefing(state: S<'_>) -> AppResult<crate::briefing::Briefing> {
    state.db.with(|c| crate::briefing::load(c, util::now()))
}

/// [새로고침]: AI로 다시 요약한다 (AI 모델이 있을 때). 쓸 수 있는 줄이 없으면 규칙 요약을 그대로 둔다.
#[tauri::command]
pub async fn refresh_briefing(state: S<'_>) -> AppResult<crate::briefing::Briefing> {
    let st = state.inner().clone();
    blocking(move || {
        let now = util::now();
        let f = st.db.with(|c| crate::briefing::facts(c, now))?;
        if f.is_empty() {
            return st.db.with(|c| crate::briefing::load(c, now));
        }
        let cfg = app::require_model(&st)?;
        let out = st
            .runtime
            .chat(&cfg, crate::briefing::ai_body(&f), app::infer_timeout(&cfg))
            .map_err(|e| AppError::msg(format!("AI 요약을 만들지 못했습니다: {}", e.message)))?;
        let Some(lines) = crate::briefing::guard_ai(&out, &f) else {
            return Err(AppError::msg("AI 요약이 오늘의 사실과 맞지 않아 쓰지 않았습니다. 규칙으로 만든 요약을 그대로 보여 줍니다."));
        };
        st.db.with(|c| {
            crate::briefing::store_ai(c, now, &f, &lines)?;
            crate::briefing::load(c, now)
        })
    })
    .await
}

/// 학생 한 명의 학생부 자료 (영역 7개, 교과·자유학기는 과목·활동별 칸)
#[tauri::command]
pub async fn student_record(state: S<'_>, student_id: i64) -> AppResult<Vec<records::RecordTab>> {
    state.db.with(|c| records::student_record(c, student_id))
}

/// 학급별로 남은 학생: 칸 하나의 학생별 상태 (참고자료만 / 초안 / 최종안)
#[tauri::command]
pub async fn record_status(state: S<'_>, area: String, subject: String) -> AppResult<Vec<records::StatusRow>> {
    state.db.with(|c| records::record_status(c, &area, &subject))
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

/// 참고자료를 날짜순으로 모아 칸에 붙인다 (문장을 새로 만들지 않음)
#[tauri::command]
pub async fn record_gather(state: S<'_>, student_id: i64, area: String, subject: String) -> AppResult<records::RecordTab> {
    let tab = state.db.with(|c| {
        let refs = records::tab_refs(c, student_id, &area, &subject)?;
        if refs.is_empty() {
            return Err(AppError::invalid("이 칸에 쓸 참고자료가 없습니다."));
        }
        let body = records::template_body(&refs);
        let ids: Vec<i64> = refs.iter().map(|o| o.id).collect();
        records::create_draft(c, student_id, &area, &subject, &ids, &body, "template", util::now())?;
        records::record_tab(c, student_id, &area, &subject)
    })?;
    state.changed("records");
    Ok(tab)
}

/// AI 초안: 이 칸의 (빼지 않은) 참고자료만 근거로 쓴다
#[tauri::command]
pub async fn record_ai_draft(state: S<'_>, student_id: i64, area: String, subject: String) -> AppResult<records::RecordTab> {
    let st = state.inner().clone();
    let tab = blocking(move || {
        let refs = st.db.with(|c| records::tab_refs(c, student_id, &area, &subject))?;
        if refs.is_empty() {
            return Err(AppError::invalid("이 칸에 쓸 참고자료가 없습니다. 기록을 더 쓰거나 다른 영역의 참고자료를 이 영역으로 옮겨 주세요."));
        }
        let cfg = app::require_model(&st)?;
        let area_v = Area::parse(&area);
        // 참고자료에 개인정보(전화번호·주소)가 섞여 있어도 AI 초안 문맥에는 가린 모양으로만 넣는다
        let lines: Vec<String> = refs.iter().map(|o| crate::profile::scrub(&crate::text::tidy(&o.content))).collect();
        let out = st
            .runtime
            .chat(&cfg, llm::draft_body(area_v, &subject, &lines), app::infer_timeout(&cfg))
            .map_err(|e| AppError::msg(format!("AI 초안을 만들지 못했습니다: {}", e.message)))?;
        let body = llm::clean_draft(&out);
        if body.trim().is_empty() {
            return Err(AppError::msg("AI가 빈 초안을 돌려주었습니다. 다시 시도하거나 ‘참고자료 모아 붙이기’를 쓰세요."));
        }
        let ids: Vec<i64> = refs.iter().map(|o| o.id).collect();
        st.db.with(|c| {
            records::create_draft(c, student_id, &area, &subject, &ids, &body, "ai", util::now())?;
            records::record_tab(c, student_id, &area, &subject)
        })
    })
    .await?;
    state.changed("records");
    Ok(tab)
}

/// 칸 글 저장 (final = 최종안으로 표시). 빈 글이면 칸의 글을 지운다.
#[tauri::command]
pub async fn save_record_text(state: S<'_>, student_id: i64, area: String, subject: String, body: String, final_text: bool) -> AppResult<records::RecordTab> {
    let tab = state.db.with(|c| {
        records::save_record_text(c, student_id, &area, &subject, &body, final_text, util::now())?;
        records::record_tab(c, student_id, &area, &subject)
    })?;
    state.changed("records");
    Ok(tab)
}

#[tauri::command]
pub async fn check_record_text(state: S<'_>, student_id: i64, area: String, subject: String, body: String) -> AppResult<DraftCheck> {
    state.db.with(|c| records::check_record_text(c, student_id, &area, &subject, &body))
}

/// 학급(또는 전체) 최종안을 엑셀 파일로
#[tauri::command]
pub async fn export_record_finals(state: S<'_>, class_id: Option<i64>, path: String) -> AppResult<records::FinalsExport> {
    let st = state.inner().clone();
    blocking(move || st.db.with(|c| records::export_finals(c, class_id, std::path::Path::new(&path)))).await
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
    state.db.with(|c| {
        let birthdays = settings::load(c)?.birthdays_on_calendar;
        schedule::calendar(c, &view, &start, &end, birthdays)
    })
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
    state.db.with(|c| {
        let birthdays = settings::load(c)?.birthdays_on_calendar;
        schedule::today_overview(c, now.date(), now, birthdays)
    })
}

// ───────────────────────── 학급명렬표 ─────────────────────────

#[tauri::command]
pub async fn set_student_field(state: S<'_>, id: i64, field: String, value: String) -> AppResult<StudentDto> {
    let s = state.db.with(|c| {
        students::set_roster_field(c, id, &field, &value, util::now())?;
        students::get_student(c, id)
    })?;
    state.changed("students");
    Ok(s)
}

// ───────────────────────── 시간표 ─────────────────────────

#[derive(Serialize)]
pub struct SemesterInfo {
    pub school_year: i32,
    pub semester: u8,
}

/// 시간표 화면의 기본 학기. 2월에 새 학년도를 시작했으면 새 학년도 1학기를 연다.
#[tauri::command]
pub async fn current_semester(state: S<'_>) -> AppResult<SemesterInfo> {
    let today = util::now().date();
    let (school_year, semester) = state.db.with(|c| Ok(timetable::active_semester(c, today)))?;
    Ok(SemesterInfo { school_year, semester })
}

#[tauri::command]
pub async fn get_bell(state: S<'_>, year: i32, semester: u8) -> AppResult<timetable::BellSchedule> {
    state.db.with(|c| timetable::get_bell(c, year, semester))
}

/// 저장하지 않고 교시 시각만 계산해 미리 보여 준다
#[tauri::command]
pub async fn compute_bell(config: timetable::BellConfig) -> AppResult<Vec<timetable::PeriodSlot>> {
    timetable::compute_periods(&config)
}

#[tauri::command]
pub async fn save_bell(
    state: S<'_>,
    year: i32,
    semester: u8,
    config: timetable::BellConfig,
    periods: Option<Vec<timetable::PeriodSlot>>,
) -> AppResult<timetable::BellSchedule> {
    let b = state.db.with(|c| timetable::save_bell(c, year, semester, &config, periods, util::now()))?;
    state.changed("timetable");
    Ok(b)
}

#[tauri::command]
pub async fn get_timetable(state: S<'_>, year: i32, semester: u8, kind: String, class_label: Option<String>) -> AppResult<timetable::TimetableView> {
    state.db.with(|c| timetable::get_timetable(c, year, semester, &kind, class_label.as_deref().unwrap_or("")))
}

#[tauri::command]
pub async fn class_timetables(state: S<'_>, year: i32, semester: u8) -> AppResult<Vec<String>> {
    state.db.with(|c| timetable::class_timetable_labels(c, year, semester))
}

#[tauri::command]
pub async fn set_timetable_cell(
    state: S<'_>,
    year: i32,
    semester: u8,
    kind: String,
    class_label: Option<String>,
    cell: timetable::Cell,
) -> AppResult<()> {
    state.db.with(|c| timetable::set_cell(c, year, semester, &kind, class_label.as_deref().unwrap_or(""), &cell, util::now()))?;
    state.changed("timetable");
    Ok(())
}

#[tauri::command]
pub async fn add_school_row(state: S<'_>, year: i32, label: String) -> AppResult<String> {
    let l = state.db.with(|c| timetable::add_school_row(c, year, &label))?;
    state.changed("timetable");
    Ok(l)
}

#[tauri::command]
pub async fn delete_timetable(state: S<'_>, year: i32, semester: u8, kind: String, class_label: Option<String>) -> AppResult<()> {
    state.db.with(|c| timetable::delete_timetable(c, year, semester, &kind, class_label.as_deref().unwrap_or("")))?;
    state.changed("timetable");
    Ok(())
}

#[tauri::command]
pub async fn copy_semester(state: S<'_>, from_year: i32, from_semester: u8, to_year: i32, to_semester: u8) -> AppResult<timetable::CopyResult> {
    let r = state.db.with(|c| timetable::copy_semester(c, (from_year, from_semester), (to_year, to_semester), util::now()))?;
    state.changed("timetable");
    Ok(r)
}

#[tauri::command]
pub async fn today_timetable(state: S<'_>) -> AppResult<timetable::TodayTimetable> {
    state.db.with(|c| timetable::today(c, util::now()))
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
    /// 파일 하나 실행기로 켰으면 그 파일 (바탕화면 등)
    pub launcher_path: Option<String>,
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
        launcher_path: crate::paths::launcher_path().map(|p| p.display().to_string()),
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
        // 모델 선택은 select_model 로만, 항상 위는 set_always_on_top 으로만 바꾼다 (화면에 남은 예전 값으로 덮어쓰지 않게)
        next.selected_model = cur.selected_model.clone();
        next.always_on_top = cur.always_on_top;
        next.window_transparency = cur.window_transparency;
        settings::save(c, &next)?;
        let saved = settings::load(c)?;
        let now = util::now();
        // 학교를 바꾸면 예전 학교의 학급 목록을 버린다
        if saved.school.as_ref().map(|x| &x.code) != cur.school.as_ref().map(|x| &x.code) {
            c.execute("DELETE FROM settings WHERE key='neis_classes'", [])?;
        }
        // 내 과목·뺄 학급을 바꾸면 받아 둔 학교 시간표로 개인 시간표를 다시 만든다 (인터넷 필요 없음)
        if saved.my_subjects != cur.my_subjects || saved.my_classes_off != cur.my_classes_off {
            let (y, sem) = crate::timetable::active_semester(c, now.date());
            crate::neis::rebuild_personal(c, &saved, y, sem, now)?;
        }
        // 학사력 종류 규칙을 바꾸면 이미 있는 학사력에도 적용한다
        if saved.day_rules != cur.day_rules {
            crate::neis::reclassify_days(c, &saved.day_rules)?;
        }
        Ok(saved)
    })?;
    state.changed("all");
    (state.emit)("settings-changed", json!({}));
    Ok(s)
}

// ───────────────────────── 나이스 (학교 코드만 보낸다) ─────────────────────────

/// 학교 이름으로 찾기
#[tauri::command]
pub async fn neis_search_schools(state: S<'_>, name: String) -> AppResult<Vec<crate::neis::SchoolInfo>> {
    let key = state.db.with(|c| Ok(settings::load(c)?.neis_key))?;
    blocking(move || crate::neis::search_schools(&name, &key, &crate::neis::http_get)).await
}

#[tauri::command]
pub async fn neis_status(state: S<'_>) -> AppResult<crate::neis::NeisStatus> {
    state.db.with(|c| crate::neis::status(c, util::now()))
}

/// 지금 받기: timetable | schedule | classes | all
#[tauri::command]
pub async fn neis_sync(state: S<'_>, what: String) -> AppResult<Vec<crate::neis::SyncReport>> {
    let st = state.inner().clone();
    let r = blocking(move || {
        let now = util::now();
        let get = &crate::neis::http_get;
        let mut out = Vec::new();
        if what == "classes" || what == "all" {
            let s = st.db.with(|c| settings::load(c))?;
            if let Some(school) = s.school.filter(|x| !x.code.is_empty()) {
                let year = st.db.with(|c| Ok(students::active_school_year(c, now.date())))?;
                let classes = crate::neis::sync_classes(&st.db, &school, &s.neis_key, year, now, get).unwrap_or_default();
                out.push(crate::neis::SyncReport { status: "ok".into(), message: classes.join(", "), count: classes.len(), fetched_at: Some(util::fmt_dt(now)) });
            }
        }
        if what == "timetable" || what == "all" {
            out.push(crate::neis::sync_timetable(&st.db, now, get)?);
        }
        if what == "schedule" || what == "all" {
            out.push(crate::neis::sync_schedule(&st.db, now, get)?);
        }
        Ok(out)
    })
    .await?;
    state.changed("all");
    Ok(r)
}

/// 그날 급식 (없으면 다음 급식일)
#[tauri::command]
pub async fn get_meals(state: S<'_>, date: String) -> AppResult<crate::neis::MealsView> {
    let d = util::parse_d(&date).ok_or_else(|| AppError::invalid("날짜 형식이 올바르지 않습니다."))?;
    let st = state.inner().clone();
    blocking(move || crate::neis::meals_for(&st.db, d, util::now(), &crate::neis::http_get)).await
}

#[tauri::command]
pub async fn timetable_colors(state: S<'_>) -> AppResult<crate::timetable::TtColors> {
    state.db.with(|c| crate::timetable::colors(c))
}

/// 시간표 색 정하기 (scope: cell | class | room, color: 없으면 '없음')
#[tauri::command]
pub async fn set_timetable_color(state: S<'_>, scope: String, target: String, color: Option<String>) -> AppResult<crate::timetable::TtColors> {
    let v = state.db.with(|c| {
        crate::timetable::set_color(c, &scope, &target, color.as_deref())?;
        crate::timetable::colors(c)
    })?;
    state.changed("timetable");
    Ok(v)
}

#[tauri::command]
pub async fn set_autostart(enabled: bool) -> AppResult<bool> {
    let exe = crate::paths::launch_target()?;
    notify::set_autostart(enabled, &exe).map_err(AppError::msg)?;
    Ok(notify::autostart_target().is_some())
}

// ───────────────────────── 오늘 화면: 메모·칸 배치 ─────────────────────────
// 메모는 쓰는 대로 저장하므로 data-changed 를 보내지 않는다 (화면 전체를 다시 읽지 않게)

#[tauri::command]
pub async fn list_memos(state: S<'_>) -> AppResult<Vec<crate::memos::MemoDto>> {
    state.db.with(|c| crate::memos::list(c))
}

#[tauri::command]
pub async fn create_memo(state: S<'_>, color: Option<String>) -> AppResult<crate::memos::MemoDto> {
    state.db.with(|c| crate::memos::create(c, color.as_deref().unwrap_or("yellow"), util::now()))
}

#[tauri::command]
pub async fn update_memo(state: S<'_>, id: i64, body: Option<String>, color: Option<String>) -> AppResult<crate::memos::MemoDto> {
    state.db.with(|c| crate::memos::update(c, id, body.as_deref(), color.as_deref(), util::now()))
}

#[tauri::command]
pub async fn delete_memo(state: S<'_>, id: i64) -> AppResult<()> {
    state.db.with(|c| crate::memos::delete(c, id, util::now()))
}

#[tauri::command]
pub async fn restore_memo(state: S<'_>, id: i64) -> AppResult<crate::memos::MemoDto> {
    state.db.with(|c| crate::memos::restore(c, id))
}

#[tauri::command]
pub async fn shift_memo(state: S<'_>, id: i64, dir: i64) -> AppResult<Vec<crate::memos::MemoDto>> {
    state.db.with(|c| crate::memos::shift(c, id, dir))
}

#[tauri::command]
pub async fn get_today_layout(state: S<'_>) -> AppResult<crate::today_layout::TodayLayout> {
    state.db.with(|c| crate::today_layout::load(c))
}

#[tauri::command]
pub async fn set_today_layout(state: S<'_>, layout: crate::today_layout::TodayLayout) -> AppResult<crate::today_layout::TodayLayout> {
    state.db.with(|c| crate::today_layout::save(c, layout))
}

/// 창을 항상 위에 둘지: 설정에 저장하고 지금 창에도 바로 적용한다
#[tauri::command]
pub async fn set_always_on_top(app: tauri::AppHandle, state: S<'_>, enabled: bool) -> AppResult<bool> {
    state.db.with(|c| {
        let mut s = settings::load(c)?;
        s.always_on_top = enabled;
        settings::save(c, &s)
    })?;
    if let Some(w) = app.get_webview_window("main") {
        w.set_always_on_top(enabled).map_err(|e| AppError::msg(format!("창을 항상 위에 두지 못했습니다: {e}")))?;
        // 항상 위를 바꾸면 창 틀이 스타일을 다시 써 투명도가 풀린다 → 그 뒤에 다시 적용
        // (창 틀이 바로 바꾸지 않을 때를 위해 잠시 뒤에 한 번 더)
        #[cfg(windows)]
        if let Ok(h) = w.hwnd() {
            let raw = h.0 as isize;
            let _ = w.run_on_main_thread(move || crate::window_fx::ensure(raw));
            let w2 = w.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let _ = w2.run_on_main_thread(move || crate::window_fx::ensure(raw));
            });
        }
    }
    (state.emit)("settings-changed", json!({}));
    Ok(enabled)
}

/// 창 투명도(%): 설정에 저장하고 지금 창에 바로 적용한다. 끌면서 바꾸므로 화면 전체를 다시 읽게 하지 않는다.
#[tauri::command]
pub async fn set_window_transparency(app: AppHandle, state: S<'_>, value: u8) -> AppResult<u8> {
    let v = crate::window_fx::clamp(value);
    state.db.with(|c| {
        let mut s = settings::load(c)?;
        s.window_transparency = v;
        settings::save(c, &s)
    })?;
    #[cfg(windows)]
    if let Some(w) = app.get_webview_window("main") {
        let raw = w.hwnd().map_err(|e| AppError::msg(format!("창을 찾지 못했습니다: {e}")))?.0 as isize;
        let (tx, rx) = std::sync::mpsc::channel();
        w.run_on_main_thread(move || {
            let _ = tx.send(crate::window_fx::apply(raw, v));
        })
        .map_err(|e| AppError::msg(e.to_string()))?;
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| AppError::msg("창 투명도를 바꾸지 못했습니다 (응답 없음)."))?
            .map_err(AppError::msg)?;
    }
    #[cfg(not(windows))]
    let _ = app;
    Ok(v)
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
        "licenses" => {
            let p = crate::paths::exe_dir().join("licenses");
            if !p.is_dir() {
                return Err(AppError::invalid("라이선스 폴더가 없습니다 (개발용 실행에서는 만들어지지 않습니다)."));
            }
            p
        }
        _ => return Err(AppError::invalid("알 수 없는 폴더입니다.")),
    };
    std::fs::create_dir_all(&p)?;
    app.opener().open_path(p.display().to_string(), None::<&str>).map_err(|e| AppError::msg(format!("폴더를 열지 못했습니다: {e}")))
}

#[derive(Serialize)]
pub struct LicenseTexts {
    pub license: &'static str,
    pub notices: &'static str,
}

/// 라이선스 및 저작권 (프로젝트 맨 위 LICENSE.txt, THIRD_PARTY_NOTICES.txt 를 프로그램 안에 넣어 둔다)
#[tauri::command]
pub async fn license_texts() -> AppResult<LicenseTexts> {
    Ok(LicenseTexts { license: include_str!("../../LICENSE.txt"), notices: include_str!("../../THIRD_PARTY_NOTICES.txt") })
}

#[tauri::command]
pub async fn list_backups(state: S<'_>) -> AppResult<Vec<crate::backup::BackupFile>> {
    Ok(crate::backup::list(&state.paths.backups_dir))
}

#[tauri::command]
pub async fn backup_now(state: S<'_>) -> AppResult<String> {
    let st = state.inner().clone();
    let p = blocking(move || app::backup_now(&st, "manual")).await?;
    Ok(p.display().to_string())
}

/// 되돌리기 확인 창에 보여 줄 백업 내용 (사본으로 검사)
#[tauri::command]
pub async fn inspect_backup(state: S<'_>, name: String) -> AppResult<crate::backup::BackupCounts> {
    let st = state.inner().clone();
    blocking(move || crate::backup::inspect(&st.paths, &name)).await
}

/// 백업 되돌리기 예약. 화면은 이어서 restart_app 을 부른다.
#[tauri::command]
pub async fn request_restore(state: S<'_>, name: String) -> AppResult<crate::backup::BackupCounts> {
    let st = state.inner().clone();
    blocking(move || crate::backup::request(&st.paths, &name)).await
}

/// 이번 시작 때 되돌리기를 했으면 그 결과 (한 번만 돌려준다)
#[tauri::command]
pub async fn take_restore_outcome() -> AppResult<Option<crate::backup::RestoreOutcome>> {
    Ok(crate::backup::LAST_RESTORE.lock().unwrap_or_else(|e| e.into_inner()).take())
}

/// 이번 실행에서 내보낸 파일·폴더 (탐색기에서 보여 주기는 이 목록 안에서만)
static EXPORTED: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

#[tauri::command]
pub async fn export_data(state: S<'_>, options: crate::export::ExportOptions) -> AppResult<crate::export::ExportResult> {
    let st = state.inner().clone();
    let result = blocking(move || {
        let stamp = util::now().format("%Y-%m-%d %H%M").to_string();
        st.db.with(|c| crate::export::run(c, &options, &stamp))
    })
    .await?;
    crate::logging::info(&format!("내보내기: 표 {}개, 파일 {}개", result.sheets.len(), result.files.len()));
    EXPORTED.lock().unwrap_or_else(|e| e.into_inner()).push(std::path::PathBuf::from(&result.path));
    Ok(result)
}

// ───────────────────────── 학년도 마감·새 학년도 ─────────────────────────

#[tauri::command]
pub async fn school_years(state: S<'_>) -> AppResult<Vec<crate::archive::YearInfo>> {
    state.db.with(|c| crate::archive::years(c, util::now().date()))
}

/// 학년도 마감 기록물 (PDF 조판에 몇 초 걸릴 수 있어 별도 스레드)
#[tauri::command]
pub async fn make_year_archive(state: S<'_>, options: crate::archive::ArchiveOptions) -> AppResult<crate::archive::ArchiveResult> {
    let st = state.inner().clone();
    let result = blocking(move || st.db.with(|c| crate::archive::run(c, &options, util::now()))).await?;
    crate::logging::info(&format!("학년도 마감 기록물: 파일 {}개", result.files.len()));
    EXPORTED.lock().unwrap_or_else(|e| e.into_inner()).push(std::path::PathBuf::from(&result.folder));
    Ok(result)
}

#[tauri::command]
pub async fn new_year_preview(state: S<'_>, new_year: Option<i32>) -> AppResult<crate::archive::NewYearPreview> {
    let y = new_year.unwrap_or_else(|| crate::archive::default_new_year(util::now().date()));
    state.db.with(|c| crate::archive::preview(c, y))
}

#[tauri::command]
pub async fn start_new_year(state: S<'_>, options: crate::archive::NewYearOptions) -> AppResult<crate::archive::NewYearResult> {
    let st = state.inner().clone();
    let r = blocking(move || st.db.with(|c| crate::archive::start_new_year(c, &st.paths.backups_dir, &options, util::now()))).await?;
    state.changed("students");
    state.changed("timetable");
    state.changed("duties");
    Ok(r)
}

#[tauri::command]
pub async fn reveal_export(app: AppHandle, path: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let p = std::path::PathBuf::from(&path);
    if !EXPORTED.lock().unwrap_or_else(|e| e.into_inner()).contains(&p) {
        return Err(AppError::invalid("이번에 내보낸 파일만 열 수 있습니다."));
    }
    app.opener().reveal_item_in_dir(&p).map_err(|e| AppError::msg(format!("탐색기를 열지 못했습니다: {e}")))
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
