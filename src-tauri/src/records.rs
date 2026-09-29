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
}

const OBS_SELECT: &str = "SELECT o.id, o.note_id, o.student_id, s.name, c.label, o.area, o.content, o.observed_on, o.source, o.edited,
    o.created_at, n.raw_text, n.created_at
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
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO observations(student_id, area, content, observed_on, source, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'manual', ?5, ?5)",
        params![input.student_id, area, content, observed, now_s],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_observation(conn: &Connection, id: i64, input: &ObservationInput, now: NaiveDateTime) -> AppResult<()> {
    let (area, content, observed) = check_obs(input)?;
    let n = conn.execute(
        "UPDATE observations SET student_id=?2, area=?3, content=?4, observed_on=?5,
            edited=CASE WHEN source='ai' THEN 1 ELSE edited END, updated_at=?6 WHERE id=?1",
        params![id, input.student_id, area, content, observed, util::fmt_dt(now)],
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
    /// NEIS 방식 바이트 (한글 3, 영문·숫자·공백 1, 줄바꿈 2) — 참고용
    pub neis_bytes: usize,
}

pub fn neis_bytes(s: &str) -> usize {
    let mut n = 0;
    for ch in s.chars() {
        n += match ch {
            '\n' => 2,
            '\r' => 0,
            c if c.is_ascii() => 1,
            _ => 3,
        };
    }
    n
}

pub fn check_draft(body: &str, sources: &[String]) -> DraftCheck {
    let joined = sources.join("\n");
    let novel = if sources.is_empty() { vec![] } else { text::novel_words(body, &joined) };
    DraftCheck { novel_words: novel, chars: body.chars().count(), neis_bytes: neis_bytes(body) }
}

fn draft_sources(conn: &Connection, draft_id: i64) -> AppResult<(Vec<i64>, Vec<String>)> {
    let mut st = conn.prepare(
        "SELECT o.id, o.content FROM record_draft_sources ds JOIN observations o ON o.id=ds.observation_id WHERE ds.draft_id=?1 ORDER BY o.observed_on",
    )?;
    let rows: Vec<(i64, String)> = st.query_map(params![draft_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok((rows.iter().map(|r| r.0).collect(), rows.into_iter().map(|r| r.1).collect()))
}

fn map_draft(conn: &Connection, r: (i64, i64, String, String, String, i64, String, String)) -> AppResult<DraftDto> {
    let (ids, contents) = draft_sources(conn, r.0)?;
    Ok(DraftDto {
        check: check_draft(&r.3, &contents),
        id: r.0,
        student_id: r.1,
        area: r.2,
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
            "SELECT id, student_id, area, body, generated_by, reviewed, created_at, updated_at FROM record_drafts WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("초안을 찾을 수 없습니다."))?;
    map_draft(conn, r)
}

pub fn student_drafts(conn: &Connection, student_id: i64) -> AppResult<Vec<DraftDto>> {
    let mut st = conn.prepare(
        "SELECT id, student_id, area, body, generated_by, reviewed, created_at, updated_at FROM record_drafts WHERE student_id=?1 ORDER BY updated_at DESC",
    )?;
    let rows: Vec<(i64, i64, String, String, String, i64, String, String)> = st
        .query_map(params![student_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?
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

pub fn create_draft(
    conn: &mut Connection,
    student_id: i64,
    area: &str,
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
        "INSERT INTO record_drafts(student_id, area, body, generated_by, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![student_id, area_v.as_str(), body.trim(), generated_by, now_s],
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

#[derive(Debug, Clone, Serialize)]
pub struct AreaBlock {
    pub area: String,
    pub label: String,
    pub observations: Vec<ObservationDto>,
    pub drafts: Vec<DraftDto>,
}

/// 학생부 자료 화면: 7개 영역별 관찰 기록과 초안
pub fn record_overview(conn: &Connection, student_id: i64) -> AppResult<Vec<AreaBlock>> {
    let obs = student_observations(conn, student_id)?;
    let drafts = student_drafts(conn, student_id)?;
    let mut out = Vec::new();
    for a in Area::ALL {
        out.push(AreaBlock {
            area: a.as_str().to_string(),
            label: a.label().to_string(),
            observations: obs.iter().filter(|o| o.area == a.as_str()).cloned().collect(),
            drafts: drafts.iter().filter(|d| d.area == a.as_str()).cloned().collect(),
        });
    }
    let unfiled: Vec<ObservationDto> = obs.iter().filter(|o| o.area == "none").cloned().collect();
    if !unfiled.is_empty() {
        out.push(AreaBlock { area: "none".into(), label: Area::None.label().into(), observations: unfiled, drafts: vec![] });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn neis_byte_count() {
        assert_eq!(neis_bytes("가나 a1\n"), 3 + 3 + 1 + 1 + 1 + 2);
    }

    #[test]
    fn draft_keeps_sources_and_flags_novel_words() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let now = util::parse_dt("2026-09-29T09:00:00").unwrap();
        c.execute("INSERT INTO students(name, created_at, updated_at) VALUES ('김민수', 'x', 'x')", []).unwrap();
        c.execute("INSERT INTO students(name, created_at, updated_at) VALUES ('이서연', 'x', 'x')", []).unwrap();
        let o1 = create_observation(&c, &ObservationInput { student_id: Some(1), area: "subject".into(), content: "수업 중 실험 결과를 정확하게 발표함".into(), observed_on: "2026-09-20".into() }, now).unwrap();
        let other = create_observation(&c, &ObservationInput { student_id: Some(2), area: "subject".into(), content: "다른 학생".into(), observed_on: "2026-09-21".into() }, now).unwrap();
        let obs = student_observations(&c, 1).unwrap();
        let body = template_body(&obs);
        assert_eq!(body, "(9.20.) 수업 중 실험 결과를 정확하게 발표함");
        let id = create_draft(&mut c, 1, "subject", &[o1, other], "실험 결과를 정확하게 발표하며 탁월한 리더십을 보임", "ai", now).unwrap();
        let d = get_draft(&c, id).unwrap();
        assert_eq!(d.source_ids, vec![o1], "다른 학생의 관찰은 근거로 연결하지 않는다");
        assert!(d.check.novel_words.iter().any(|w| w.contains("리더십")));
        assert!(d.check.novel_words.iter().any(|w| w.contains("탁월")));
        let blocks = record_overview(&c, 1).unwrap();
        assert_eq!(blocks.len(), 7);
        assert_eq!(blocks.iter().find(|b| b.area == "subject").unwrap().drafts.len(), 1);
    }
}
