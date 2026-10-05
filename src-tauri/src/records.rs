//! 학생 관찰 기록과 학생부 자료(초안)
//!
//! 초안은 교사 검토용이다. 어떤 관찰 기록을 근거로 만들었는지 함께 저장하고,
//! 근거에 없는 낱말을 표시해 교사가 대조할 수 있게 한다. 학생부(NEIS)에 자동 입력하는 기능은 없다.

use crate::analysis::Area;
use crate::error::{AppError, AppResult};
use crate::text;
use crate::util;
use chrono::NaiveDateTime;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ObservationDto {
    pub id: i64,
    pub note_id: Option<i64>,
    pub student_id: Option<i64>,
    pub student_label: Option<String>,
    pub area: String,
    pub area_label: String,
    pub content: String,
    pub observed_on: String,
    pub source: String,
    pub edited: bool,
    pub created_at: String,
    pub note_raw: Option<String>,
    pub note_created_at: Option<String>,
    /// 교과는 과목, 자유학기는 활동 (다른 영역은 빈 값)
    pub subject: String,
    /// 학생부 참고자료에서 뺌
    pub excluded: bool,
}

const OBS_SELECT: &str = "SELECT o.id, o.note_id, o.student_id, s.name, c.label, o.area, o.content, o.observed_on, o.source, o.edited,
    o.created_at, n.raw_text, n.created_at, o.subject, o.excluded
    FROM observations o LEFT JOIN students s ON s.id=o.student_id LEFT JOIN classes c ON c.id=s.class_id
    LEFT JOIN notes n ON n.id=o.note_id";

fn map_obs(r: &rusqlite::Row) -> rusqlite::Result<ObservationDto> {
    let name: Option<String> = r.get(3)?;
    let label: Option<String> = r.get(4)?;
    let area: String = r.get(5)?;
    Ok(ObservationDto {
        id: r.get(0)?,
        note_id: r.get(1)?,
        student_id: r.get(2)?,
        student_label: name.map(|n| match &label {
            Some(l) => format!("{l} {n}"),
            None => n,
        }),
        area_label: Area::parse(&area).label().to_string(),
        area,
        content: r.get(6)?,
        observed_on: r.get(7)?,
        source: r.get(8)?,
        edited: r.get::<_, i64>(9)? != 0,
        created_at: r.get(10)?,
        note_raw: r.get(11)?,
        note_created_at: r.get(12)?,
        subject: r.get(13)?,
        excluded: r.get::<_, i64>(14)? != 0,
    })
}

pub fn query_observations(conn: &Connection, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> AppResult<Vec<ObservationDto>> {
    let mut st = conn.prepare(&format!("{OBS_SELECT} WHERE {where_sql}"))?;
    let rows = st.query_map(args, map_obs)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn student_observations(conn: &Connection, student_id: i64) -> AppResult<Vec<ObservationDto>> {
    query_observations(conn, "o.student_id=?1 ORDER BY o.observed_on DESC, o.id DESC", &[&student_id])
}

pub fn unassigned_observations(conn: &Connection) -> AppResult<Vec<ObservationDto>> {
    query_observations(conn, "o.student_id IS NULL ORDER BY o.observed_on DESC, o.id DESC LIMIT 200", &[])
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObservationInput {
    #[serde(default)]
    pub student_id: Option<i64>,
    pub area: String,
    pub content: String,
    pub observed_on: String,
    /// 교과 과목·자유학기 활동 (없으면 원문·시간표로 정한다)
    #[serde(default)]
    pub subject: Option<String>,
    /// 학생부 참고자료에서 빼기
    #[serde(default)]
    pub excluded: Option<bool>,
}

/// 자유학기활동의 세부 활동 (기재요령: 각 1,000자)
pub const FREE_SEMESTER_PARTS: [&str; 4] = ["진로탐색활동", "주제선택활동", "예술·체육활동", "동아리활동"];

/// 교과는 과목, 자유학기는 세부 활동을 정한다.
/// 교과: 원문에 과목 이름(내 과목 → 학교 시간표 과목)이 있으면 그 과목, 없으면 그 학생 학급에서 내가 가르치는 과목이 하나일 때 그 과목,
///       내 과목이 하나뿐이면 그 과목. 정할 수 없으면 빈 값(교사가 고른다).
pub fn infer_subject(conn: &Connection, area: Area, content: &str, student_id: Option<i64>) -> AppResult<String> {
    match area {
        Area::FreeSemester => {
            let part = if content.contains("주제선택") || content.contains("주제 선택") {
                "주제선택활동"
            } else if content.contains("진로탐색") || content.contains("진로 탐색") {
                "진로탐색활동"
            } else if content.contains("예술") || content.contains("체육") {
                "예술·체육활동"
            } else if content.contains("동아리") {
                "동아리활동"
            } else {
                ""
            };
            Ok(part.to_string())
        }
        Area::Subject => {
            let s = crate::settings::load(conn)?;
            let (year, sem) = crate::timetable::active_semester(conn, util::now().date());
            let mut names: Vec<String> = s.my_subjects.clone();
            for x in crate::neis::school_subjects(conn, year, sem).unwrap_or_default() {
                if !names.contains(&x) {
                    names.push(x);
                }
            }
            let mut by_len = names.clone();
            by_len.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));
            if let Some(n) = by_len.iter().find(|n| n.chars().count() >= 2 && content.contains(n.as_str())) {
                return Ok(n.clone());
            }
            if let Some(sid) = student_id {
                let label: Option<String> = conn
                    .query_row("SELECT c.label FROM students s JOIN classes c ON c.id=s.class_id WHERE s.id=?1", params![sid], |r| r.get(0))
                    .optional()?;
                if let Some(l) = label {
                    let personal = crate::timetable::get_timetable(conn, year, sem, "personal", "")?;
                    let mut mine: Vec<String> = personal.cells.iter().filter(|c| c.class_label == l && !c.subject.is_empty()).map(|c| c.subject.clone()).collect();
                    mine.sort();
                    mine.dedup();
                    if mine.len() == 1 {
                        return Ok(mine.remove(0));
                    }
                }
            }
            Ok(if s.my_subjects.len() == 1 { s.my_subjects[0].clone() } else { String::new() })
        }
        _ => Ok(String::new()),
    }
}

fn subject_for(area: &str, subject: &str) -> String {
    if matches!(area, "subject" | "free_semester") { subject.trim().chars().take(30).collect() } else { String::new() }
}

fn check_obs(input: &ObservationInput) -> AppResult<(String, String, String)> {
    let area = Area::parse(&input.area);
    let content = input.content.trim();
    if content.is_empty() {
        return Err(AppError::invalid("관찰 내용을 입력해 주세요."));
    }
    let observed = util::opt_date(Some(&input.observed_on))?.ok_or_else(|| AppError::invalid("관찰 날짜를 입력해 주세요."))?;
    Ok((area.as_str().to_string(), content.to_string(), observed))
}

pub fn create_observation(conn: &Connection, input: &ObservationInput, now: NaiveDateTime) -> AppResult<i64> {
    let (area, content, observed) = check_obs(input)?;
    let subject = match &input.subject {
        Some(s) => subject_for(&area, s),
        None => infer_subject(conn, Area::parse(&area), &content, input.student_id)?,
    };
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO observations(student_id, area, content, observed_on, source, created_at, updated_at, subject, excluded)
         VALUES (?1, ?2, ?3, ?4, 'manual', ?5, ?5, ?6, ?7)",
        params![input.student_id, area, content, observed, now_s, subject, input.excluded.unwrap_or(false) as i64],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_observation(conn: &Connection, id: i64, input: &ObservationInput, now: NaiveDateTime) -> AppResult<()> {
    let (area, content, observed) = check_obs(input)?;
    // 영역을 옮기면 과목도 새 영역에 맞게 (과목을 따로 주지 않았으면 원문으로 다시 정한다)
    let cur: Option<(String, String)> =
        conn.query_row("SELECT area, subject FROM observations WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let subject = match (&input.subject, &cur) {
        (Some(s), _) => subject_for(&area, s),
        (None, Some((a, s))) if *a == area => s.clone(),
        _ => infer_subject(conn, Area::parse(&area), &content, input.student_id)?,
    };
    let n = conn.execute(
        "UPDATE observations SET student_id=?2, area=?3, content=?4, observed_on=?5, subject=?7, excluded=COALESCE(?8, excluded),
            edited=CASE WHEN source='ai' THEN 1 ELSE edited END, updated_at=?6 WHERE id=?1",
        params![id, input.student_id, area, content, observed, util::fmt_dt(now), subject, input.excluded.map(|b| b as i64)],
    )?;
    if n == 0 {
        return Err(AppError::not_found("관찰 기록을 찾을 수 없습니다."));
    }
    // 학생이 정해졌으면 원문 기록과도 연결한다
    if let Some(sid) = input.student_id {
        conn.execute(
            "INSERT OR IGNORE INTO note_students(note_id, student_id, source)
             SELECT note_id, ?2, 'manual' FROM observations WHERE id=?1 AND note_id IS NOT NULL",
            params![id, sid],
        )?;
    }
    Ok(())
}

pub fn delete_observation(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM observations WHERE id=?1", params![id])?;
    Ok(())
}

// ───────────────────────── 초안 ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DraftDto {
    pub id: i64,
    pub student_id: i64,
    pub area: String,
    /// 교과 과목·자유학기 활동
    pub subject: String,
    pub body: String,
    pub generated_by: String,
    pub reviewed: bool,
    pub created_at: String,
    pub updated_at: String,
    pub source_ids: Vec<i64>,
    pub check: DraftCheck,
}

#[derive(Debug, Clone, Serialize)]
pub struct DraftCheck {
    /// 근거 관찰 기록에 없는 낱말 (교사가 대조해야 할 부분)
    pub novel_words: Vec<String>,
    pub chars: usize,
    /// 교육정보시스템(나이스) 바이트: 한글 1자 3, 영문·숫자·공백 1, 엔터 1 (2026학년도 기재요령)
    pub neis_bytes: usize,
    /// 영역 최대 글자 수 (한글 기준)와 바이트 한도 (글자 수 × 3)
    pub limit_chars: Option<usize>,
    pub limit_bytes: Option<usize>,
    /// 바이트 한도를 넘었는가
    pub over: bool,
}

pub fn neis_bytes(s: &str) -> usize {
    let mut n = 0;
    for ch in s.chars() {
        n += match ch {
            '\r' => 0,
            c if c.is_ascii() => 1,
            _ => 3,
        };
    }
    n
}

pub fn check_draft(body: &str, sources: &[String]) -> DraftCheck {
    check_limit(body, sources, None)
}

pub fn check_limit(body: &str, sources: &[String], limit_chars: Option<usize>) -> DraftCheck {
    let joined = sources.join("\n");
    let novel = if sources.is_empty() { vec![] } else { text::novel_words(body, &joined) };
    let bytes = neis_bytes(body);
    let limit_bytes = limit_chars.map(|c| c * 3);
    DraftCheck { novel_words: novel, chars: body.chars().count(), neis_bytes: bytes, limit_chars, limit_bytes, over: limit_bytes.is_some_and(|l| bytes > l) }
}

fn draft_sources(conn: &Connection, draft_id: i64) -> AppResult<(Vec<i64>, Vec<String>)> {
    let mut st = conn.prepare(
        "SELECT o.id, o.content FROM record_draft_sources ds JOIN observations o ON o.id=ds.observation_id WHERE ds.draft_id=?1 ORDER BY o.observed_on",
    )?;
    let rows: Vec<(i64, String)> = st.query_map(params![draft_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok((rows.iter().map(|r| r.0).collect(), rows.into_iter().map(|r| r.1).collect()))
}

type DraftRow = (i64, i64, String, String, String, i64, String, String, String);

fn map_draft(conn: &Connection, r: DraftRow) -> AppResult<DraftDto> {
    let (ids, contents) = draft_sources(conn, r.0)?;
    Ok(DraftDto {
        check: check_limit(&r.3, &contents, Area::parse(&r.2).char_limit()),
        id: r.0,
        student_id: r.1,
        area: r.2,
        subject: r.8,
        body: r.3,
        generated_by: r.4,
        reviewed: r.5 != 0,
        created_at: r.6,
        updated_at: r.7,
        source_ids: ids,
    })
}

pub fn get_draft(conn: &Connection, id: i64) -> AppResult<DraftDto> {
    let r = conn
        .query_row(
            "SELECT id, student_id, area, body, generated_by, reviewed, created_at, updated_at, subject FROM record_drafts WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("초안을 찾을 수 없습니다."))?;
    map_draft(conn, r)
}

pub fn student_drafts(conn: &Connection, student_id: i64) -> AppResult<Vec<DraftDto>> {
    let mut st = conn.prepare(
        "SELECT id, student_id, area, body, generated_by, reviewed, created_at, updated_at, subject FROM record_drafts WHERE student_id=?1
         ORDER BY updated_at DESC, id DESC",
    )?;
    let rows: Vec<DraftRow> = st
        .query_map(params![student_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))?
        .collect::<Result<_, _>>()?;
    rows.into_iter().map(|r| map_draft(conn, r)).collect()
}

/// 관찰 기록을 날짜순으로 모은 기본 초안 (문장을 새로 지어내지 않는다)
pub fn template_body(obs: &[ObservationDto]) -> String {
    let mut v: Vec<&ObservationDto> = obs.iter().collect();
    v.sort_by(|a, b| a.observed_on.cmp(&b.observed_on));
    v.iter()
        .map(|o| {
            let d = util::parse_d(&o.observed_on).map(|d| d.format("%-m.%-d.").to_string()).unwrap_or_default();
            format!("({d}) {}", text::tidy(&o.content))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_arguments)]
pub fn create_draft(
    conn: &mut Connection,
    student_id: i64,
    area: &str,
    subject: &str,
    observation_ids: &[i64],
    body: &str,
    generated_by: &str,
    now: NaiveDateTime,
) -> AppResult<i64> {
    let area_v = Area::parse(area);
    if area_v == Area::None {
        return Err(AppError::invalid("학생부 자료 영역을 골라 주세요."));
    }
    if !matches!(generated_by, "template" | "ai" | "manual") {
        return Err(AppError::invalid("초안 종류가 올바르지 않습니다."));
    }
    let tx = conn.transaction()?;
    let now_s = util::fmt_dt(now);
    tx.execute(
        "INSERT INTO record_drafts(student_id, area, body, generated_by, created_at, updated_at, subject) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
        params![student_id, area_v.as_str(), body.trim(), generated_by, now_s, subject_for(area_v.as_str(), subject)],
    )?;
    let id = tx.last_insert_rowid();
    for oid in observation_ids {
        // 다른 학생의 관찰을 근거로 연결하지 않는다
        tx.execute(
            "INSERT OR IGNORE INTO record_draft_sources(draft_id, observation_id)
             SELECT ?1, id FROM observations WHERE id=?2 AND student_id=?3",
            params![id, oid, student_id],
        )?;
    }
    tx.commit()?;
    Ok(id)
}

pub fn update_draft(conn: &Connection, id: i64, body: &str, reviewed: bool, now: NaiveDateTime) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE record_drafts SET body=?2, reviewed=?3, updated_at=?4 WHERE id=?1",
        params![id, body.trim(), reviewed as i64, util::fmt_dt(now)],
    )?;
    if n == 0 {
        return Err(AppError::not_found("초안을 찾을 수 없습니다."));
    }
    Ok(())
}

pub fn delete_draft(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM record_drafts WHERE id=?1", params![id])?;
    Ok(())
}

// ───────────────────────── 학생부 자료 화면 (영역 7개 × 과목) ─────────────────────────

/// 영역(교과는 과목, 자유학기는 활동) 하나의 칸: 참고자료 + 최종안 입력 칸
#[derive(Debug, Clone, Serialize)]
pub struct RecordTab {
    pub area: String,
    pub label: String,
    pub subject: String,
    /// 2026 기재요령 최대 글자 수 (한글 기준)
    pub limit_chars: Option<usize>,
    /// 참고자료 (뺀 것도 함께, excluded 로 표시)
    pub refs: Vec<ObservationDto>,
    /// 지금 칸의 글 (가장 최근 초안·최종안)
    pub text: Option<DraftDto>,
    /// none 자료 없음 | refs 참고자료만 | draft 초안 | final 최종안
    pub status: String,
}

fn status_of(refs: usize, text: Option<bool>) -> &'static str {
    match text {
        Some(true) => "final",
        Some(false) => "draft",
        None if refs > 0 => "refs",
        None => "none",
    }
}

/// 영역의 과목(활동) 목록: 교과는 내 과목 + 참고자료·글에 있는 과목, 자유학기는 활동 4가지 + 그 밖
fn subjects_for(area: Area, my_subjects: &[String], obs: &[ObservationDto], drafts: &[DraftDto]) -> Vec<String> {
    let mut v: Vec<String> = match area {
        Area::Subject => my_subjects.to_vec(),
        Area::FreeSemester => FREE_SEMESTER_PARTS.iter().map(|s| s.to_string()).collect(),
        _ => return vec![String::new()],
    };
    for s in obs.iter().filter(|o| o.area == area.as_str()).map(|o| &o.subject).chain(drafts.iter().filter(|d| d.area == area.as_str()).map(|d| &d.subject)) {
        if !v.contains(s) {
            v.push(s.clone());
        }
    }
    if v.is_empty() {
        v.push(String::new());
    }
    v
}

/// 학생 한 명의 학생부 자료: 영역 7개(교과·자유학기는 과목·활동별) + 참고자료가 있을 때만 출결·영역 미지정
pub fn student_record(conn: &Connection, student_id: i64) -> AppResult<Vec<RecordTab>> {
    let obs = student_observations(conn, student_id)?;
    let drafts = student_drafts(conn, student_id)?;
    let my = crate::settings::load(conn)?.my_subjects;
    let mut out = Vec::new();
    let mut push = |area: Area, subject: String| {
        let refs: Vec<ObservationDto> = obs.iter().filter(|o| o.area == area.as_str() && o.subject == subject).cloned().collect();
        let text = drafts.iter().find(|d| d.area == area.as_str() && d.subject == subject).cloned();
        let included = refs.iter().filter(|o| !o.excluded).count();
        let status = status_of(included, text.as_ref().map(|d| d.reviewed)).to_string();
        out.push(RecordTab { area: area.as_str().into(), label: area.label().into(), subject, limit_chars: area.char_limit(), refs, text, status });
    };
    for a in Area::RECORD_TABS {
        for s in subjects_for(a, &my, &obs, &drafts) {
            push(a, s);
        }
    }
    for a in [Area::Attendance, Area::Volunteer, Area::None] {
        if obs.iter().any(|o| o.area == a.as_str()) || drafts.iter().any(|d| d.area == a.as_str()) {
            push(a, String::new());
        }
    }
    Ok(out)
}

/// 학생부 칸 하나만 (AI 초안·저장 뒤 돌려줄 때)
pub fn record_tab(conn: &Connection, student_id: i64, area: &str, subject: &str) -> AppResult<RecordTab> {
    student_record(conn, student_id)?
        .into_iter()
        .find(|t| t.area == area && t.subject == subject)
        .ok_or_else(|| AppError::not_found("학생부 자료 칸을 찾을 수 없습니다."))
}

/// 칸의 참고자료 중 빼지 않은 것 (AI 초안의 근거)
pub fn tab_refs(conn: &Connection, student_id: i64, area: &str, subject: &str) -> AppResult<Vec<ObservationDto>> {
    let mut v = query_observations(
        conn,
        "o.student_id=?1 AND o.area=?2 AND o.subject=?3 AND o.excluded=0 ORDER BY o.observed_on, o.id",
        &[&student_id, &area, &subject],
    )?;
    v.retain(|o| !o.content.trim().is_empty());
    Ok(v)
}

/// 칸의 글을 저장한다. 그 칸의 가장 최근 글을 고치고(없으면 새로), final 이면 최종안으로 표시한다.
pub fn save_record_text(conn: &Connection, student_id: i64, area: &str, subject: &str, body: &str, final_: bool, now: NaiveDateTime) -> AppResult<()> {
    let a = Area::parse(area);
    if a == Area::None {
        return Err(AppError::invalid("학생부 자료 영역을 골라 주세요."));
    }
    let subject = subject_for(a.as_str(), subject);
    let latest: Option<i64> = conn
        .query_row(
            "SELECT id FROM record_drafts WHERE student_id=?1 AND area=?2 AND subject=?3 ORDER BY updated_at DESC, id DESC LIMIT 1",
            params![student_id, a.as_str(), subject],
            |r| r.get(0),
        )
        .optional()?;
    let body = body.trim();
    let now_s = util::fmt_dt(now);
    match latest {
        Some(id) if body.is_empty() => {
            // 칸을 비우면 그 칸의 글을 지운다 (참고자료만 남는다)
            conn.execute("DELETE FROM record_drafts WHERE student_id=?1 AND area=?2 AND subject=?3", params![student_id, a.as_str(), subject])?;
            let _ = id;
        }
        Some(id) => {
            conn.execute("UPDATE record_drafts SET body=?2, reviewed=?3, updated_at=?4 WHERE id=?1", params![id, body, final_ as i64, now_s])?;
        }
        None if body.is_empty() => {}
        None => {
            conn.execute(
                "INSERT INTO record_drafts(student_id, area, body, generated_by, reviewed, created_at, updated_at, subject) VALUES (?1, ?2, ?3, 'manual', ?4, ?5, ?5, ?6)",
                params![student_id, a.as_str(), body, final_ as i64, now_s, subject],
            )?;
        }
    }
    Ok(())
}

/// 칸 글 검사: 글자 수·바이트·한도, 빼지 않은 참고자료에 없는 표현
pub fn check_record_text(conn: &Connection, student_id: i64, area: &str, subject: &str, body: &str) -> AppResult<DraftCheck> {
    let refs: Vec<String> = tab_refs(conn, student_id, area, subject)?.into_iter().map(|o| o.content).collect();
    Ok(check_limit(body, &refs, Area::parse(area).char_limit()))
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusRow {
    pub student_id: i64,
    /// none | refs | draft | final
    pub status: String,
    pub refs: i64,
}

/// 학급별로 남은 학생을 보기 위한 칸 상태 (모든 학생)
pub fn record_status(conn: &Connection, area: &str, subject: &str) -> AppResult<Vec<StatusRow>> {
    let mut st = conn.prepare(
        "SELECT s.id,
           (SELECT COUNT(*) FROM observations o WHERE o.student_id=s.id AND o.area=?1 AND o.subject=?2 AND o.excluded=0),
           (SELECT d.reviewed FROM record_drafts d WHERE d.student_id=s.id AND d.area=?1 AND d.subject=?2 ORDER BY d.updated_at DESC, d.id DESC LIMIT 1)
         FROM students s WHERE s.archived=0",
    )?;
    let rows = st
        .query_map(params![area, subject], |r| {
            let refs: i64 = r.get(1)?;
            let reviewed: Option<i64> = r.get(2)?;
            Ok(StatusRow { student_id: r.get(0)?, status: status_of(refs as usize, reviewed.map(|x| x != 0)).to_string(), refs })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone, Serialize)]
pub struct FinalsExport {
    pub path: String,
    pub rows: usize,
    pub students: usize,
}

/// 학급 최종안을 엑셀로 (영역·과목 → 번호 순, 한 줄에 한 학생). 나이스에 자동으로 넣지 않는다.
pub fn export_finals(conn: &Connection, class_id: Option<i64>, path: &std::path::Path) -> AppResult<FinalsExport> {
    use rust_xlsxwriter::{Color, Format as XFormat, FormatAlign, FormatBorder, Workbook};
    let mut st = conn.prepare(
        "SELECT COALESCE(c.label,''), s.number, s.name, d.area, d.subject, d.body, s.id
         FROM record_drafts d JOIN students s ON s.id=d.student_id LEFT JOIN classes c ON c.id=s.class_id
         WHERE d.reviewed=1 AND s.archived=0 AND (?1 IS NULL OR s.class_id=?1)
           AND d.id = (SELECT d2.id FROM record_drafts d2 WHERE d2.student_id=d.student_id AND d2.area=d.area AND d2.subject=d.subject
                       ORDER BY d2.updated_at DESC, d2.id DESC LIMIT 1)",
    )?;
    let mut rows: Vec<(String, Option<i64>, String, String, String, String, i64)> = st
        .query_map(params![class_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?
        .collect::<Result<_, _>>()?;
    let order = |a: &str| Area::RECORD_TABS.iter().chain([Area::Attendance, Area::Volunteer].iter()).position(|x| x.as_str() == a).unwrap_or(99);
    rows.sort_by(|x, y| order(&x.3).cmp(&order(&y.3)).then(x.4.cmp(&y.4)).then(x.0.cmp(&y.0)).then(x.1.unwrap_or(999).cmp(&y.1.unwrap_or(999))));
    let xerr = |e: rust_xlsxwriter::XlsxError| AppError::msg(format!("엑셀 파일을 만들지 못했습니다: {e}"));
    let head = XFormat::new().set_bold().set_background_color(Color::RGB(0xE8F0EA)).set_border_bottom(FormatBorder::Thin).set_align(FormatAlign::Center);
    let top = XFormat::new().set_align(FormatAlign::Top);
    let wrap = XFormat::new().set_align(FormatAlign::Top).set_text_wrap();
    let warn = XFormat::new().set_align(FormatAlign::Top).set_font_color(Color::RGB(0xB8372C)).set_bold();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("학생부 최종안").map_err(xerr)?;
    let cols: [(&str, f64); 10] =
        [("학급", 7.0), ("번호", 6.0), ("이름", 10.0), ("영역", 20.0), ("과목·활동", 14.0), ("최종안", 80.0), ("글자 수", 8.0), ("바이트", 8.0), ("한도(바이트)", 11.0), ("확인", 10.0)];
    for (i, (h, w)) in cols.iter().enumerate() {
        ws.write_string_with_format(0, i as u16, *h, &head).map_err(xerr)?;
        ws.set_column_width(i as u16, *w).map_err(xerr)?;
    }
    let mut students = std::collections::BTreeSet::new();
    for (i, (label, number, name, area, subject, body, sid)) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        students.insert(*sid);
        let a = Area::parse(area);
        let bytes = neis_bytes(body);
        let limit = a.char_limit().map(|c| c * 3);
        ws.write_string_with_format(r, 0, label, &top).map_err(xerr)?;
        if let Some(n) = number {
            ws.write_number_with_format(r, 1, *n as f64, &top).map_err(xerr)?;
        }
        ws.write_string_with_format(r, 2, name, &top).map_err(xerr)?;
        ws.write_string_with_format(r, 3, a.label(), &top).map_err(xerr)?;
        ws.write_string_with_format(r, 4, subject, &top).map_err(xerr)?;
        ws.write_string_with_format(r, 5, body, &wrap).map_err(xerr)?;
        ws.write_number_with_format(r, 6, body.chars().count() as f64, &top).map_err(xerr)?;
        ws.write_number_with_format(r, 7, bytes as f64, &top).map_err(xerr)?;
        if let Some(l) = limit {
            ws.write_number_with_format(r, 8, l as f64, &top).map_err(xerr)?;
            if bytes > l {
                ws.write_string_with_format(r, 9, "한도 초과", &warn).map_err(xerr)?;
            }
        }
    }
    ws.set_freeze_panes(1, 0).map_err(xerr)?;
    ws.set_landscape();
    ws.set_print_fit_to_pages(1, 0);
    let tmp = path.with_extension("xlsx.part");
    wb.save(&tmp).map_err(xerr)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::msg(format!("파일을 저장하지 못했습니다. 엑셀에서 열려 있으면 닫고 다시 해 주세요. ({e})"))
    })?;
    Ok(FinalsExport { path: path.display().to_string(), rows: rows.len(), students: students.len() })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn subjects_status_finals_and_export() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let now = util::parse_dt("2026-10-02T09:00:00").unwrap();
        crate::settings::update(&c, |s| s.my_subjects = vec!["사회".into(), "역사".into()]).unwrap();
        let a = crate::students::create_student(&c, &crate::students::StudentInput { name: "홍길동".into(), class_label: Some("1-1".into()), number: Some(1), memo: None }, now).unwrap();
        let b = crate::students::create_student(&c, &crate::students::StudentInput { name: "성춘향".into(), class_label: Some("2-1".into()), number: Some(2), memo: None }, now).unwrap();
        // 1-1 은 사회, 2-1 은 역사를 가르친다 (개인 시간표)
        let cell = |wd, p, subj: &str, cls: &str| crate::timetable::Cell { weekday: wd, period: p, subject: subj.into(), class_label: cls.into(), ..Default::default() };
        crate::timetable::set_cell(&c, 2026, 2, "personal", "", &cell(1, 1, "사회", "1-1"), now).unwrap();
        crate::timetable::set_cell(&c, 2026, 2, "personal", "", &cell(2, 1, "역사", "2-1"), now).unwrap();
        let obs = |sid, area: &str, text: &str| ObservationInput { student_id: Some(sid), area: area.into(), content: text.into(), observed_on: "2026-10-01".into(), subject: None, excluded: None };
        let o1 = create_observation(&c, &obs(a, "subject", "모둠 토의에서 근거를 들어 발표함"), now).unwrap();
        let o2 = create_observation(&c, &obs(a, "subject", "역사 수업에서 연표를 정리함"), now).unwrap();
        let o3 = create_observation(&c, &obs(b, "subject", "질문을 자주 함"), now).unwrap();
        let o4 = create_observation(&c, &obs(a, "free_semester", "주제선택 시간에 지역 조사 발표"), now).unwrap();
        let subj = |id: i64| query_observations(&c, "o.id=?1", &[&id]).unwrap()[0].subject.clone();
        assert_eq!(subj(o1), "사회", "학급에서 가르치는 과목");
        assert_eq!(subj(o2), "역사", "원문에 과목 이름");
        assert_eq!(subj(o3), "역사");
        assert_eq!(subj(o4), "주제선택활동");
        // 교과 칸: 내 과목 2개
        let tabs = student_record(&c, a).unwrap();
        let s_tabs: Vec<(&str, &str)> = tabs.iter().filter(|t| t.area == "subject").map(|t| (t.subject.as_str(), t.status.as_str())).collect();
        assert_eq!(s_tabs, vec![("사회", "refs"), ("역사", "refs")]);
        // 참고자료 빼기 → 사회 칸은 참고자료 없음
        update_observation(&c, o1, &ObservationInput { excluded: Some(true), ..obs(a, "subject", "모둠 토의에서 근거를 들어 발표함") }, now).unwrap();
        assert_eq!(record_tab(&c, a, "subject", "사회").unwrap().status, "none");
        assert!(tab_refs(&c, a, "subject", "사회").unwrap().is_empty());
        // 다른 영역으로 옮기기 (과목은 비움)
        update_observation(&c, o2, &obs(a, "behavior", "역사 수업에서 연표를 정리함"), now).unwrap();
        assert_eq!((subj(o2).as_str(), record_tab(&c, a, "behavior", "").unwrap().refs.len()), ("", 1));
        // 최종안 저장 → 상태, 한도 검사
        save_record_text(&c, b, "subject", "역사", "질문을 자주 함.", false, now).unwrap();
        assert_eq!(record_tab(&c, b, "subject", "역사").unwrap().status, "draft");
        save_record_text(&c, b, "subject", "역사", "수업 중 질문을 자주 함.", true, now).unwrap();
        let tab = record_tab(&c, b, "subject", "역사").unwrap();
        assert_eq!((tab.status.as_str(), tab.text.as_ref().unwrap().body.as_str()), ("final", "수업 중 질문을 자주 함."));
        let st = record_status(&c, "subject", "역사").unwrap();
        assert_eq!(st.iter().find(|r| r.student_id == b).unwrap().status, "final");
        assert_eq!(st.iter().find(|r| r.student_id == a).unwrap().status, "none");
        let long = "가".repeat(301);
        let chk = check_record_text(&c, a, "behavior", "", &long).unwrap();
        assert_eq!((chk.limit_chars, chk.limit_bytes, chk.over), (Some(300), Some(900), true), "행특 300자");
        assert!(check_record_text(&c, b, "subject", "역사", "질문을 자주 하며 탁월한 리더십을 보임").unwrap().novel_words.iter().any(|w| w.contains("리더십")));
        // 엑셀
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("최종안.xlsx");
        let r = export_finals(&c, None, &out).unwrap();
        assert_eq!((r.rows, r.students), (1, 1));
        assert!(out.exists());
        // 칸을 비우면 글을 지운다
        save_record_text(&c, b, "subject", "역사", "  ", false, now).unwrap();
        assert_eq!(record_tab(&c, b, "subject", "역사").unwrap().status, "refs");
        let _ = &mut c;
    }
    #[test]
    fn neis_byte_count() {
        assert_eq!(neis_bytes("가나 a1\n"), 3 + 3 + 1 + 1 + 1 + 1, "2026 기재요령: 엔터 1바이트");
    }

    #[test]
    fn draft_keeps_sources_and_flags_novel_words() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let now = util::parse_dt("2026-09-29T09:00:00").unwrap();
        c.execute("INSERT INTO students(name, created_at, updated_at) VALUES ('홍길동', 'x', 'x')", []).unwrap();
        c.execute("INSERT INTO students(name, created_at, updated_at) VALUES ('성춘향', 'x', 'x')", []).unwrap();
        let o1 = create_observation(&c, &ObservationInput { student_id: Some(1), area: "subject".into(), content: "수업 중 실험 결과를 정확하게 발표함".into(), observed_on: "2026-09-20".into(), subject: None, excluded: None }, now).unwrap();
        let other = create_observation(&c, &ObservationInput { student_id: Some(2), area: "subject".into(), content: "다른 학생".into(), observed_on: "2026-09-21".into(), subject: None, excluded: None }, now).unwrap();
        let obs = student_observations(&c, 1).unwrap();
        let body = template_body(&obs);
        assert_eq!(body, "(9.20.) 수업 중 실험 결과를 정확하게 발표함");
        let id = create_draft(&mut c, 1, "subject", "", &[o1, other], "실험 결과를 정확하게 발표하며 탁월한 리더십을 보임", "ai", now).unwrap();
        let d = get_draft(&c, id).unwrap();
        assert_eq!(d.source_ids, vec![o1], "다른 학생의 관찰은 근거로 연결하지 않는다");
        assert!(d.check.novel_words.iter().any(|w| w.contains("리더십")));
        assert!(d.check.novel_words.iter().any(|w| w.contains("탁월")));
        let tabs = student_record(&c, 1).unwrap();
        assert_eq!(tabs.iter().filter(|t| t.area != "free_semester").count(), 6, "자유학기 밖 영역 6칸");
        assert_eq!(tabs.iter().filter(|t| t.area == "free_semester").count(), 4, "자유학기 활동 4칸");
        let subj = tabs.iter().find(|t| t.area == "subject").unwrap();
        assert_eq!((subj.status.as_str(), subj.text.as_ref().map(|d| d.id)), ("draft", Some(id)));
    }
}
