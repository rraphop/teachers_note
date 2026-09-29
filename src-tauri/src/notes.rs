//! 기록(원문)과 분류 결과 반영
//!
//! 원문은 AI를 기다리지 않고 먼저 저장한다 (status = pending).
//! 분류 결과(Plan)는 한 트랜잭션으로 파생 데이터(관찰·할 일·일정·업무 기록)에 반영한다.

use crate::analysis::{Area, ItemKind};
use crate::classify::{ItemOrigin, Plan, PlanItem, PlanStudent, ReminderSpec};
use crate::error::{AppError, AppResult};
use crate::reminders::{self, TargetKind};
use crate::students;
use crate::util;
use chrono::NaiveDateTime;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_NOTE_CHARS: usize = 20_000;

#[derive(Debug, Clone, Serialize)]
pub struct Chip {
    /// student | task | event | duty
    pub kind: String,
    pub label: String,
    pub area: Option<String>,
    pub date: Option<String>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteDto {
    pub id: i64,
    pub raw_text: String,
    pub created_at: String,
    pub updated_at: String,
    pub raw_edited_at: Option<String>,
    pub status: String,
    pub status_reason: Option<String>,
    pub summary: Option<String>,
    pub summary_source: Option<String>,
    pub classified_at: Option<String>,
    pub classified_by: Option<String>,
    pub guard_notes: Vec<String>,
    pub unknown_names: Vec<String>,
    pub chips: Vec<Chip>,
}

const NOTE_COLS: &str = "id, raw_text, created_at, updated_at, raw_edited_at, status, status_reason, summary, summary_source,
    classified_at, classified_by, guard_notes, unknown_names";

fn map_note(r: &rusqlite::Row) -> rusqlite::Result<NoteDto> {
    let guard: String = r.get(11)?;
    let unknown: String = r.get(12)?;
    Ok(NoteDto {
        id: r.get(0)?,
        raw_text: r.get(1)?,
        created_at: r.get(2)?,
        updated_at: r.get(3)?,
        raw_edited_at: r.get(4)?,
        status: r.get(5)?,
        status_reason: r.get(6)?,
        summary: r.get(7)?,
        summary_source: r.get(8)?,
        classified_at: r.get(9)?,
        classified_by: r.get(10)?,
        guard_notes: serde_json::from_str(&guard).unwrap_or_default(),
        unknown_names: serde_json::from_str(&unknown).unwrap_or_default(),
        chips: vec![],
    })
}

fn chips(conn: &Connection, note_id: i64) -> AppResult<Vec<Chip>> {
    let mut out = Vec::new();
    let mut st = conn.prepare(
        "SELECT o.area, s.name, c.label FROM observations o LEFT JOIN students s ON s.id=o.student_id
         LEFT JOIN classes c ON c.id=s.class_id WHERE o.note_id=?1 ORDER BY o.id",
    )?;
    let rows = st.query_map(params![note_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?)))?;
    for row in rows {
        let (area, name, label) = row?;
        let who = match (label, name) {
            (Some(l), Some(n)) => format!("{l} {n}"),
            (None, Some(n)) => n,
            _ => "학생 미지정".to_string(),
        };
        out.push(Chip { kind: "student".into(), label: who, area: Some(area), date: None, done: false });
    }
    let mut st = conn.prepare("SELECT title, due_date, done FROM tasks WHERE note_id=?1 ORDER BY id")?;
    for row in st.query_map(params![note_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?)))? {
        let (t, d, done) = row?;
        out.push(Chip { kind: "task".into(), label: t, area: None, date: d, done: done != 0 });
    }
    let mut st = conn.prepare("SELECT title, start_date FROM events WHERE note_id=?1 ORDER BY id")?;
    for row in st.query_map(params![note_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (t, d) = row?;
        out.push(Chip { kind: "event".into(), label: t, area: None, date: Some(d), done: false });
    }
    let mut st = conn.prepare(
        "SELECT d.name, l.date, l.done FROM duty_logs l LEFT JOIN duties d ON d.id=l.duty_id WHERE l.note_id=?1 ORDER BY l.id",
    )?;
    for row in st.query_map(params![note_id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?)))? {
        let (n, d, done) = row?;
        out.push(Chip { kind: "duty".into(), label: n.unwrap_or_else(|| "업무".into()), area: None, date: d, done: done != 0 });
    }
    Ok(out)
}

pub fn get_note(conn: &Connection, id: i64) -> AppResult<NoteDto> {
    let mut n = conn
        .query_row(&format!("SELECT {NOTE_COLS} FROM notes WHERE id=?1"), params![id], map_note)
        .optional()?
        .ok_or_else(|| AppError::not_found("기록을 찾을 수 없습니다."))?;
    n.chips = chips(conn, id)?;
    Ok(n)
}

pub fn query_notes(conn: &Connection, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> AppResult<Vec<NoteDto>> {
    let mut st = conn.prepare(&format!("SELECT {NOTE_COLS} FROM notes WHERE {where_sql}"))?;
    let mut rows = st.query_map(args, map_note)?.collect::<Result<Vec<_>, _>>()?;
    for n in rows.iter_mut() {
        n.chips = chips(conn, n.id)?;
    }
    Ok(rows)
}

/// 원문을 즉시 저장한다. AI 처리 성공 여부와 관계없이 원문은 남는다.
pub fn create_note(conn: &Connection, raw: &str, now: NaiveDateTime) -> AppResult<i64> {
    let text = raw.trim_end();
    if text.trim().is_empty() {
        return Err(AppError::invalid("기록할 내용을 입력해 주세요."));
    }
    if text.chars().count() > MAX_NOTE_CHARS {
        return Err(AppError::invalid(format!("한 번에 {MAX_NOTE_CHARS}자까지 기록할 수 있습니다. 나누어 기록해 주세요.")));
    }
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO notes(raw_text, created_at, updated_at, status) VALUES (?1, ?2, ?2, 'pending')",
        params![text, now_s],
    )?;
    Ok(conn.last_insert_rowid())
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NoteFilter {
    /// all | unclassified | classified
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub student_id: Option<i64>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

pub fn list_notes(conn: &Connection, f: &NoteFilter) -> AppResult<Vec<NoteDto>> {
    let mut w: Vec<String> = vec!["1=1".into()];
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    match f.status.as_deref() {
        Some("unclassified") => w.push("status IN ('pending','processing','failed')".into()),
        Some("classified") => w.push("status IN ('classified','manual')".into()),
        _ => {}
    }
    if let Some(d) = util::opt_date(f.date.as_deref())? {
        args.push(Box::new(d));
        w.push(format!("substr(created_at,1,10)=?{}", args.len()));
    }
    if let Some(s) = f.student_id {
        args.push(Box::new(s));
        w.push(format!("id IN (SELECT note_id FROM note_students WHERE student_id=?{})", args.len()));
    }
    let limit = f.limit.unwrap_or(100).clamp(1, 500);
    let offset = f.offset.unwrap_or(0).max(0);
    let sql = format!("{} ORDER BY created_at DESC, id DESC LIMIT {limit} OFFSET {offset}", w.join(" AND "));
    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    query_notes(conn, &sql, &refs)
}

pub fn set_status(conn: &Connection, id: i64, status: &str, reason: Option<&str>, now: NaiveDateTime) -> AppResult<()> {
    conn.execute(
        "UPDATE notes SET status=?2, status_reason=?3, updated_at=?4 WHERE id=?1",
        params![id, status, reason, util::fmt_dt(now)],
    )?;
    Ok(())
}

/// 교사가 원문을 고친다. 분류 결과는 그대로 두고, 다시 분류할지는 화면에서 묻는다.
pub fn update_raw(conn: &Connection, id: i64, raw: &str, now: NaiveDateTime) -> AppResult<()> {
    let text = raw.trim_end();
    if text.trim().is_empty() {
        return Err(AppError::invalid("원문을 비울 수 없습니다. 기록을 지우려면 삭제를 눌러 주세요."));
    }
    if text.chars().count() > MAX_NOTE_CHARS {
        return Err(AppError::invalid(format!("한 번에 {MAX_NOTE_CHARS}자까지 기록할 수 있습니다.")));
    }
    let now_s = util::fmt_dt(now);
    let n = conn.execute(
        "UPDATE notes SET raw_text=?2, raw_edited_at=?3, updated_at=?3 WHERE id=?1",
        params![id, text, now_s],
    )?;
    if n == 0 {
        return Err(AppError::not_found("기록을 찾을 수 없습니다."));
    }
    Ok(())
}

pub fn delete_note(conn: &mut Connection, id: i64) -> AppResult<()> {
    let tx = conn.transaction()?;
    reminders::delete_for_note(&tx, id)?;
    tx.execute("DELETE FROM notes WHERE id=?1", params![id])?;
    tx.commit()?;
    Ok(())
}

pub fn raw_and_created(conn: &Connection, id: i64) -> AppResult<(String, NaiveDateTime)> {
    let (raw, created): (String, String) = conn
        .query_row("SELECT raw_text, created_at FROM notes WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| AppError::not_found("기록을 찾을 수 없습니다."))?;
    let created = util::parse_dt(&created).ok_or_else(|| AppError::msg("기록 시각 형식이 올바르지 않습니다."))?;
    Ok((raw, created))
}

/// 파생 데이터가 교사 손을 탔는지 (다시 분류하면 덮어쓰므로 경고용)
pub fn has_user_changes(conn: &Connection, id: i64) -> AppResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM tasks WHERE note_id=?1 AND (edited=1 OR done=1 OR source='manual'))
              + (SELECT COUNT(*) FROM events WHERE note_id=?1 AND (edited=1 OR source='manual'))
              + (SELECT COUNT(*) FROM duty_logs WHERE note_id=?1 AND (edited=1 OR done=1 OR source='manual'))
              + (SELECT COUNT(*) FROM observations WHERE note_id=?1 AND (edited=1 OR source='manual'))
              + (SELECT COUNT(*) FROM record_draft_sources ds JOIN observations o ON o.id=ds.observation_id WHERE o.note_id=?1)",
        params![id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub enum ApplyMode<'a> {
    Ai { model_id: &'a str },
    Manual,
}

/// 출처 표시: AI가 찾은 항목은 'ai', 교사가 만든 항목과 교사가 확인해 저장한 규칙 제안은 'manual'
fn source_of(origin: ItemOrigin, mode: &ApplyMode) -> &'static str {
    match (origin, mode) {
        (ItemOrigin::Ai, _) => "ai",
        (ItemOrigin::Rule, ApplyMode::Ai { .. }) => "ai",
        _ => "manual",
    }
}

/// 분류 계획을 반영한다. 이 기록에서 나온 기존 파생 데이터는 모두 새 계획으로 바뀐다.
pub fn apply_plan(conn: &mut Connection, note_id: i64, plan: &Plan, mode: ApplyMode, now: NaiveDateTime, default_remind: &ReminderSpec) -> AppResult<()> {
    let tx = conn.transaction()?;
    let (_, created) = raw_and_created(&tx, note_id)?;
    let note_date = util::fmt_d(created.date());
    reminders::delete_for_note(&tx, note_id)?;
    for t in ["tasks", "events", "duty_logs", "observations", "note_students"] {
        tx.execute(&format!("DELETE FROM {t} WHERE note_id=?1"), params![note_id])?;
    }
    let now_s = util::fmt_dt(now);
    let mut unresolved: Vec<String> = plan.unknown_names.clone();
    let mut new_targets: Vec<(TargetKind, i64)> = Vec::new();

    for item in &plan.items {
        let source = source_of(item.origin, &mode);
        let edited = item.edited as i64;
        let text = item.text.trim();
        if text.is_empty() {
            continue;
        }
        let date = util::opt_date(item.date.as_deref())?;
        let time = util::opt_time(item.time.as_deref())?;
        let spec = item.remind.clone().unwrap_or_else(|| ReminderSpec {
            enabled: date.is_some() && default_remind.enabled,
            ..default_remind.clone()
        });
        let offsets = reminders::offsets_to_json(&spec.offsets);
        let custom = spec.custom_at.as_deref().and_then(util::parse_dt).map(util::fmt_dt);
        match item.kind {
            ItemKind::Student => {
                let observed = date.clone().unwrap_or_else(|| note_date.clone());
                let area = if item.area == Area::None { "none" } else { item.area.as_str() };
                let mut ids: Vec<i64> = Vec::new();
                for ps in &item.students {
                    match students::resolve_plan_student(&tx, ps, created)? {
                        Some(id) => ids.push(id),
                        None => {
                            let label = describe_unresolved(&tx, ps)?;
                            if !unresolved.contains(&label) {
                                unresolved.push(label);
                            }
                        }
                    }
                }
                ids.sort_unstable();
                ids.dedup();
                if ids.is_empty() {
                    // 학생이 없거나 정하지 못했으면 관찰 내용이 사라지지 않도록 '학생 미지정'으로 남긴다
                    ids.push(-1);
                }
                for sid in ids {
                    let sid_opt = if sid > 0 { Some(sid) } else { None };
                    tx.execute(
                        "INSERT INTO observations(note_id, student_id, area, content, observed_on, source, edited, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                        params![note_id, sid_opt, area, text, observed, source, edited, now_s],
                    )?;
                    if let Some(s) = sid_opt {
                        tx.execute(
                            "INSERT OR IGNORE INTO note_students(note_id, student_id, source) VALUES (?1, ?2, ?3)",
                            params![note_id, s, source],
                        )?;
                    }
                }
            }
            ItemKind::Task => {
                tx.execute(
                    "INSERT INTO tasks(note_id, duty_id, title, due_date, due_time, date_expr, approximate, done, done_at, source, edited,
                        remind_enabled, remind_offsets, remind_time, remind_custom_at, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
                    params![
                        note_id,
                        valid_duty(&tx, item.duty_id)?,
                        text,
                        date,
                        time,
                        item.date_expr,
                        item.approximate as i64,
                        item.done as i64,
                        if item.done { Some(now_s.clone()) } else { None },
                        source,
                        edited,
                        spec.enabled as i64,
                        offsets,
                        spec.time,
                        custom,
                        now_s
                    ],
                )?;
                new_targets.push((TargetKind::Task, tx.last_insert_rowid()));
                link_students(&tx, note_id, &item.students, created, source, &mut unresolved)?;
            }
            ItemKind::Event => {
                let Some(d) = date.clone() else { continue };
                tx.execute(
                    "INSERT INTO events(layer, note_id, title, start_date, end_date, time, date_expr, source, edited,
                        remind_enabled, remind_offsets, remind_time, remind_custom_at, created_at, updated_at)
                     VALUES ('note', ?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
                    params![note_id, text, d, time, item.date_expr, source, edited, spec.enabled as i64, offsets, spec.time, custom, now_s],
                )?;
                new_targets.push((TargetKind::Event, tx.last_insert_rowid()));
            }
            ItemKind::Duty => {
                let duty = valid_duty(&tx, item.duty_id)?;
                let spec_enabled = spec.enabled && date.is_some();
                tx.execute(
                    "INSERT INTO duty_logs(note_id, duty_id, content, date, time, date_expr, done, source, edited,
                        remind_enabled, remind_offsets, remind_time, remind_custom_at, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
                    params![note_id, duty, text, date, time, item.date_expr, item.done as i64, source, edited, spec_enabled as i64, offsets, spec.time, custom, now_s],
                )?;
                new_targets.push((TargetKind::DutyLog, tx.last_insert_rowid()));
            }
            ItemKind::Memo => {}
        }
    }

    for (k, id) in new_targets {
        reminders::rebuild(&tx, k, id, now)?;
    }

    let (status, by, summary_source) = match mode {
        ApplyMode::Ai { model_id } => ("classified", model_id.to_string(), "ai"),
        ApplyMode::Manual => ("manual", "manual".to_string(), "manual"),
    };
    let summary = plan.summary.trim();
    tx.execute(
        "UPDATE notes SET status=?2, status_reason=NULL, summary=?3, summary_source=?4, classified_at=?5, classified_by=?6,
            guard_notes=?7, unknown_names=?8, updated_at=?5 WHERE id=?1",
        params![
            note_id,
            status,
            if summary.is_empty() { None } else { Some(summary) },
            summary_source,
            now_s,
            by,
            serde_json::to_string(&plan.guard_notes)?,
            serde_json::to_string(&unresolved)?
        ],
    )?;
    tx.commit()?;
    Ok(())
}

fn valid_duty(conn: &Connection, duty_id: Option<i64>) -> AppResult<Option<i64>> {
    let Some(id) = duty_id else { return Ok(None) };
    Ok(conn.query_row("SELECT id FROM duties WHERE id=?1", params![id], |r| r.get(0)).optional()?)
}

fn link_students(
    conn: &Connection,
    note_id: i64,
    list: &[PlanStudent],
    created: NaiveDateTime,
    source: &str,
    unresolved: &mut Vec<String>,
) -> AppResult<()> {
    for ps in list {
        match students::resolve_plan_student(conn, ps, created)? {
            Some(id) => {
                conn.execute(
                    "INSERT OR IGNORE INTO note_students(note_id, student_id, source) VALUES (?1, ?2, ?3)",
                    params![note_id, id, source],
                )?;
            }
            None => {
                let label = describe_unresolved(conn, ps)?;
                if !unresolved.contains(&label) {
                    unresolved.push(label);
                }
            }
        }
    }
    Ok(())
}

fn describe_unresolved(conn: &Connection, ps: &PlanStudent) -> AppResult<String> {
    if ps.candidates.len() > 1 {
        let mut labels = Vec::new();
        for c in &ps.candidates {
            if let Ok(l) = students::student_label(conn, *c) {
                labels.push(l);
            }
        }
        return Ok(format!("{} (동명이인: {})", ps.name, labels.join(", ")));
    }
    Ok(match &ps.class_label {
        Some(l) => format!("{l} {}", ps.name),
        None => ps.name.clone(),
    })
}

/// 현재 파생 데이터를 수정 화면용 계획으로 되돌린다.
pub fn current_plan(conn: &Connection, note_id: i64) -> AppResult<Plan> {
    let note = get_note(conn, note_id)?;
    let mut items: Vec<PlanItem> = Vec::new();
    // 관찰: 같은 내용·영역·날짜는 한 항목으로 묶는다
    let mut groups: BTreeMap<(String, String, String, String, i64), Vec<PlanStudent>> = BTreeMap::new();
    let mut order: Vec<(String, String, String, String, i64)> = Vec::new();
    {
        let mut st = conn.prepare(
            "SELECT o.content, o.area, o.observed_on, o.source, o.edited, o.student_id, s.name, c.label, s.number
             FROM observations o LEFT JOIN students s ON s.id=o.student_id LEFT JOIN classes c ON c.id=s.class_id
             WHERE o.note_id=?1 ORDER BY o.id",
        )?;
        let rows = st.query_map(params![note_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<i64>>(8)?,
            ))
        })?;
        for row in rows {
            let (content, area, observed, source, edited, sid, name, label, number) = row?;
            let key = (content, area, observed, source, edited);
            if !groups.contains_key(&key) {
                order.push(key.clone());
            }
            let e = groups.entry(key).or_default();
            if let (Some(id), Some(n)) = (sid, name) {
                e.push(PlanStudent { student_id: Some(id), name: n, class_label: label, number, candidates: vec![] });
            }
        }
    }
    for key in order {
        let students = groups.remove(&key).unwrap_or_default();
        let (content, area, observed, source, edited) = key;
        items.push(PlanItem {
            kind: ItemKind::Student,
            text: content,
            students,
            area: Area::parse(&area),
            duty_id: None,
            date: Some(observed),
            time: None,
            date_expr: None,
            approximate: false,
            remind: None,
            origin: if source == "manual" { ItemOrigin::Manual } else { ItemOrigin::Ai },
            done: false,
            edited: edited != 0,
        });
    }
    for t in crate::schedule::query_tasks(conn, "note_id=?1 ORDER BY id", &[&note_id])? {
        items.push(PlanItem {
            kind: ItemKind::Task,
            text: t.title,
            students: vec![],
            area: Area::None,
            duty_id: t.duty_id,
            date: t.due_date,
            time: t.due_time,
            date_expr: t.date_expr,
            approximate: t.approximate,
            remind: Some(t.remind),
            origin: if t.source == "manual" { ItemOrigin::Manual } else { ItemOrigin::Ai },
            done: t.done,
            edited: t.edited,
        });
    }
    for e in crate::schedule::query_events(conn, "note_id=?1 ORDER BY id", &[&note_id])? {
        items.push(PlanItem {
            kind: ItemKind::Event,
            text: e.title,
            students: vec![],
            area: Area::None,
            duty_id: None,
            date: Some(e.start_date),
            time: e.time,
            date_expr: e.date_expr,
            approximate: false,
            remind: Some(e.remind),
            origin: if e.source == "manual" { ItemOrigin::Manual } else { ItemOrigin::Ai },
            done: false,
            edited: e.edited,
        });
    }
    for d in crate::schedule::query_duty_logs(conn, "note_id=?1 ORDER BY id", &[&note_id])? {
        items.push(PlanItem {
            kind: ItemKind::Duty,
            text: d.content,
            students: vec![],
            area: Area::None,
            duty_id: d.duty_id,
            date: d.date,
            time: d.time,
            date_expr: d.date_expr,
            approximate: false,
            remind: Some(d.remind),
            origin: if d.source == "manual" { ItemOrigin::Manual } else { ItemOrigin::Ai },
            done: d.done,
            edited: d.edited,
        });
    }
    Ok(Plan {
        summary: note.summary.unwrap_or_default(),
        items,
        guard_notes: note.guard_notes,
        unknown_names: note.unknown_names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::{self, Ctx};
    use crate::db::Db;

    fn now() -> NaiveDateTime {
        util::parse_dt("2026-09-29T10:15:00").unwrap()
    }
    fn spec() -> ReminderSpec {
        ReminderSpec { enabled: true, offsets: vec![1], time: "08:30".into(), custom_at: None }
    }

    #[test]
    fn raw_saved_first_and_survives_failure() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let id = create_note(&c, "오늘 3교시 2-1 김민수 지각.", now()).unwrap();
        set_status(&c, id, "failed", Some("AI 모델이 아직 설치되지 않았습니다."), now()).unwrap();
        let n = get_note(&c, id).unwrap();
        assert_eq!(n.raw_text, "오늘 3교시 2-1 김민수 지각.");
        assert_eq!(n.status, "failed");
        assert!(n.status_reason.unwrap().contains("설치"));
        assert!(create_note(&c, "   ", now()).is_err());
        let un = list_notes(&c, &NoteFilter { status: Some("unclassified".into()), ..Default::default() }).unwrap();
        assert_eq!(un.len(), 1);
    }

    #[test]
    fn apply_rule_plan_scenario_a_and_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let raw = "오늘 3교시 2-1 김민수 지각. 담임에게 알리고 금요일까지 보호자 연락.";
        let id = create_note(&c, raw, now()).unwrap();
        let plan = classify::rule_plan(raw, &Ctx { today: now().date(), roster: &[], duties: &[] });
        apply_plan(&mut c, id, &plan, ApplyMode::Manual, now(), &spec()).unwrap();
        let n = get_note(&c, id).unwrap();
        assert_eq!(n.status, "manual");
        assert!(n.chips.iter().any(|ch| ch.kind == "student" && ch.label == "2-1 김민수" && ch.area.as_deref() == Some("attendance")));
        assert!(n.chips.iter().any(|ch| ch.kind == "task" && ch.label == "보호자 연락" && ch.date.as_deref() == Some("2026-10-02")));
        // 학생이 자동 등록되고 관찰이 출결 영역에 연결된다
        let sid: i64 = c.query_row("SELECT id FROM students WHERE name='김민수'", [], |r| r.get(0)).unwrap();
        let area: String = c.query_row("SELECT area FROM observations WHERE student_id=?1", params![sid], |r| r.get(0)).unwrap();
        assert_eq!(area, "attendance");
        // 알림 예약: 10/1 08:30
        let fire: String = c
            .query_row("SELECT r.fire_at FROM reminders r JOIN tasks t ON t.id=r.target_id WHERE r.target_kind='task' AND t.title='보호자 연락'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fire, "2026-10-01T08:30:00");
        // 현재 분류를 계획으로 되돌려 다시 저장해도 같은 결과 (학생 중복 생성 없음)
        let mut p2 = current_plan(&c, id).unwrap();
        for it in p2.items.iter_mut() {
            if it.kind == ItemKind::Task && it.text == "보호자 연락" {
                it.date = Some("2026-10-09".into());
                it.date_expr = Some("다음 주 금요일".into());
                it.edited = true;
            }
        }
        apply_plan(&mut c, id, &p2, ApplyMode::Manual, now(), &spec()).unwrap();
        let students: i64 = c.query_row("SELECT COUNT(*) FROM students", [], |r| r.get(0)).unwrap();
        assert_eq!(students, 1);
        let due: String = c.query_row("SELECT due_date FROM tasks WHERE title='보호자 연락'", [], |r| r.get(0)).unwrap();
        assert_eq!(due, "2026-10-09");
        let fire: String = c
            .query_row("SELECT r.fire_at FROM reminders r JOIN tasks t ON t.id=r.target_id WHERE r.target_kind='task' AND t.title='보호자 연락'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fire, "2026-10-08T08:30:00");
        let reminders_total: i64 = c.query_row("SELECT COUNT(*) FROM reminders", [], |r| r.get(0)).unwrap();
        assert_eq!(reminders_total, 1, "이전 예약이 정리되어야 한다");
        // 삭제하면 파생 데이터와 예약이 함께 사라진다
        delete_note(&mut c, id).unwrap();
        let left: i64 = c.query_row("SELECT (SELECT COUNT(*) FROM tasks)+(SELECT COUNT(*) FROM observations)+(SELECT COUNT(*) FROM reminders)", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn group_observation_without_student_is_kept() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let raw = "학생들이 수업 중 손장난을 했고 지적을 받았습니다.";
        let id = create_note(&c, raw, now()).unwrap();
        let plan = classify::rule_plan(raw, &Ctx { today: now().date(), roster: &[], duties: &[] });
        apply_plan(&mut c, id, &plan, ApplyMode::Manual, now(), &spec()).unwrap();
        let (sid, area): (Option<i64>, String) = c.query_row("SELECT student_id, area FROM observations", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert!(sid.is_none());
        assert_eq!(area, "subject");
        let tasks: i64 = c.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0)).unwrap();
        assert_eq!(tasks, 0);
    }
}
