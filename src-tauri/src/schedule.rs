//! 할 일·일정·업무 기록과 오늘 화면, 일정표 조회

use crate::classify::ReminderSpec;
use crate::error::{AppError, AppResult};
use crate::reminders::{self, TargetKind};
use crate::util;
use chrono::{Duration, NaiveDate, NaiveDateTime};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

// ───────────────────────── 공통 ─────────────────────────

fn spec_from_row(enabled: i64, offsets: String, time: String, custom: Option<String>) -> ReminderSpec {
    ReminderSpec { enabled: enabled != 0, offsets: reminders::offsets_from_json(&offsets), time, custom_at: custom }
}

fn norm_spec(spec: &ReminderSpec) -> AppResult<(i64, String, String, Option<String>)> {
    let time = util::opt_time(Some(&spec.time))?.unwrap_or_else(|| "08:30".into());
    let custom = match spec.custom_at.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(c) => Some(util::fmt_dt(
            util::parse_dt(c).ok_or_else(|| AppError::invalid(format!("알림 시각 형식이 올바르지 않습니다: {c}")))?,
        )),
        None => None,
    };
    Ok((spec.enabled as i64, reminders::offsets_to_json(&spec.offsets), time, custom))
}

fn check_title(s: &str, what: &str) -> AppResult<String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(AppError::invalid(format!("{what} 내용을 입력해 주세요.")));
    }
    if t.chars().count() > 500 {
        return Err(AppError::invalid(format!("{what} 내용은 500자 이내로 적어 주세요.")));
    }
    Ok(t.to_string())
}

fn duty_label(conn: &Connection, duty_id: Option<i64>) -> AppResult<Option<String>> {
    let Some(id) = duty_id else { return Ok(None) };
    Ok(conn
        .query_row("SELECT name, detail FROM duties WHERE id=?1", params![id], |r| {
            let n: String = r.get(0)?;
            let d: String = r.get(1)?;
            Ok(if d.trim().is_empty() { n } else { format!("{n} / {d}") })
        })
        .optional()?)
}

// ───────────────────────── 할 일 ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TaskDto {
    pub id: i64,
    pub note_id: Option<i64>,
    pub duty_id: Option<i64>,
    pub duty_label: Option<String>,
    pub title: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub date_expr: Option<String>,
    pub approximate: bool,
    pub done: bool,
    pub done_at: Option<String>,
    pub source: String,
    pub edited: bool,
    pub remind: ReminderSpec,
    pub remind_text: String,
    pub next_reminder: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskInput {
    pub title: String,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub due_time: Option<String>,
    #[serde(default)]
    pub date_expr: Option<String>,
    #[serde(default)]
    pub duty_id: Option<i64>,
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub remind: Option<ReminderSpec>,
}

const TASK_COLS: &str = "id, note_id, duty_id, title, due_date, due_time, date_expr, approximate, done, done_at, source, edited,
    remind_enabled, remind_offsets, remind_time, remind_custom_at, created_at";

fn map_task(r: &rusqlite::Row) -> rusqlite::Result<TaskDto> {
    let remind = spec_from_row(r.get(12)?, r.get(13)?, r.get(14)?, r.get(15)?);
    Ok(TaskDto {
        id: r.get(0)?,
        note_id: r.get(1)?,
        duty_id: r.get(2)?,
        duty_label: None,
        title: r.get(3)?,
        due_date: r.get(4)?,
        due_time: r.get(5)?,
        date_expr: r.get(6)?,
        approximate: r.get::<_, i64>(7)? != 0,
        done: r.get::<_, i64>(8)? != 0,
        done_at: r.get(9)?,
        source: r.get(10)?,
        edited: r.get::<_, i64>(11)? != 0,
        remind_text: reminders::describe(&remind),
        remind,
        next_reminder: None,
        created_at: r.get(16)?,
    })
}

fn finish_task(conn: &Connection, mut t: TaskDto) -> AppResult<TaskDto> {
    t.duty_label = duty_label(conn, t.duty_id)?;
    t.next_reminder = reminders::next_for(conn, TargetKind::Task, t.id)?;
    Ok(t)
}

pub fn get_task(conn: &Connection, id: i64) -> AppResult<TaskDto> {
    let t = conn
        .query_row(&format!("SELECT {TASK_COLS} FROM tasks WHERE id=?1"), params![id], map_task)
        .optional()?
        .ok_or_else(|| AppError::not_found("할 일을 찾을 수 없습니다."))?;
    finish_task(conn, t)
}

pub fn query_tasks(conn: &Connection, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> AppResult<Vec<TaskDto>> {
    let mut st = conn.prepare(&format!("SELECT {TASK_COLS} FROM tasks WHERE {where_sql}"))?;
    let rows = st.query_map(args, map_task)?.collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(|t| finish_task(conn, t)).collect()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskFilter {
    /// open | done | all
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub duty_id: Option<i64>,
}

pub fn list_tasks(conn: &Connection, f: &TaskFilter) -> AppResult<Vec<TaskDto>> {
    let state = f.state.as_deref().unwrap_or("open");
    let mut w = String::from(match state {
        "done" => "done=1",
        "all" => "1=1",
        _ => "done=0",
    });
    if let Some(d) = f.duty_id {
        w.push_str(&format!(" AND duty_id={d}"));
    }
    w.push_str(if state == "done" {
        " ORDER BY done_at DESC LIMIT 300"
    } else {
        " ORDER BY due_date IS NULL, due_date, due_time IS NULL, due_time, id"
    });
    query_tasks(conn, &w, &[])
}

pub fn create_task(conn: &Connection, input: &TaskInput, now: NaiveDateTime, default_remind: &ReminderSpec) -> AppResult<i64> {
    let title = check_title(&input.title, "할 일")?;
    let due = util::opt_date(input.due_date.as_deref())?;
    let time = util::opt_time(input.due_time.as_deref())?;
    let spec = input.remind.clone().unwrap_or_else(|| ReminderSpec { enabled: due.is_some() && default_remind.enabled, ..default_remind.clone() });
    let (en, offs, rt, custom) = norm_spec(&spec)?;
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO tasks(duty_id, title, due_date, due_time, date_expr, source, remind_enabled, remind_offsets, remind_time, remind_custom_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'manual', ?6, ?7, ?8, ?9, ?10, ?10)",
        params![input.duty_id, title, due, time, input.date_expr.as_deref().map(str::trim).filter(|s| !s.is_empty()), en, offs, rt, custom, now_s],
    )?;
    let id = conn.last_insert_rowid();
    reminders::rebuild(conn, TargetKind::Task, id, now)?;
    Ok(id)
}

pub fn update_task(conn: &Connection, id: i64, input: &TaskInput, now: NaiveDateTime) -> AppResult<()> {
    let cur = get_task(conn, id)?;
    let title = check_title(&input.title, "할 일")?;
    let due = util::opt_date(input.due_date.as_deref())?;
    let time = util::opt_time(input.due_time.as_deref())?;
    let spec = input.remind.clone().unwrap_or(cur.remind.clone());
    let (en, offs, rt, custom) = norm_spec(&spec)?;
    let done = input.done.unwrap_or(cur.done);
    let done_at = if done { cur.done_at.clone().or_else(|| Some(util::fmt_dt(now))) } else { None };
    let date_expr = input.date_expr.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    // 날짜를 직접 고치면 '대략' 표시는 없앤다
    let approximate = cur.approximate && cur.due_date == due;
    conn.execute(
        "UPDATE tasks SET title=?2, due_date=?3, due_time=?4, date_expr=?5, duty_id=?6, done=?7, done_at=?8, approximate=?9,
            remind_enabled=?10, remind_offsets=?11, remind_time=?12, remind_custom_at=?13,
            edited=CASE WHEN source='ai' THEN 1 ELSE edited END, updated_at=?14 WHERE id=?1",
        params![id, title, due, time, date_expr, input.duty_id, done as i64, done_at, approximate as i64, en, offs, rt, custom, util::fmt_dt(now)],
    )?;
    reminders::rebuild(conn, TargetKind::Task, id, now)?;
    sync_done_record(conn, id, now)?;
    Ok(())
}

pub fn set_task_done(conn: &Connection, id: i64, done: bool, now: NaiveDateTime) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE tasks SET done=?2, done_at=CASE WHEN ?2=1 THEN ?3 ELSE NULL END, updated_at=?3 WHERE id=?1",
        params![id, done as i64, util::fmt_dt(now)],
    )?;
    if n == 0 {
        return Err(AppError::not_found("할 일을 찾을 수 없습니다."));
    }
    reminders::rebuild(conn, TargetKind::Task, id, now)?;
    sync_done_record(conn, id, now)?;
    Ok(())
}

/// "10월 8일 13:30 완료 · 군청에 참석 인원 회신"
pub fn done_record_text(done_at: NaiveDateTime, title: &str) -> String {
    use chrono::{Datelike, Timelike};
    format!("{}월 {}일 {:02}:{:02} 완료 · {}", done_at.month(), done_at.day(), done_at.hour(), done_at.minute(), title.trim())
}

/// 할 일의 완료 기록을 맞춘다 (3차 6번): 끝낸 할 일이면 업무와 연결된 것은 그 업무의 업무 기록, 아니면 일반 기록을 하나 남기고,
/// 끝내지 않은 할 일이면 남겨 둔 완료 기록을 지운다. 교사가 체크한 것이라 출처는 'manual'(새 항목 숫자에 넣지 않음).
pub fn sync_done_record(conn: &Connection, task_id: i64, now: NaiveDateTime) -> AppResult<()> {
    let row: Option<(String, Option<i64>, Option<i64>, i64, Option<String>)> = conn
        .query_row("SELECT title, duty_id, note_id, done, done_at FROM tasks WHERE id=?1", params![task_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .optional()?;
    conn.execute("DELETE FROM duty_logs WHERE from_task_id=?1", params![task_id])?;
    conn.execute("DELETE FROM notes WHERE from_task_id=?1", params![task_id])?;
    let Some((title, duty_id, note_id, done, done_at)) = row else { return Ok(()) };
    if done == 0 {
        return Ok(());
    }
    let at = done_at.as_deref().and_then(util::parse_dt).unwrap_or(now);
    let text = done_record_text(at, &title);
    let at_s = util::fmt_dt(at);
    let now_s = util::fmt_dt(now);
    match duty_id {
        Some(d) => {
            conn.execute(
                "INSERT INTO duty_logs(note_id, duty_id, content, date, time, done, source, from_task_id, ref_note_id, created_at, updated_at)
                 VALUES (NULL, ?1, ?2, ?3, ?4, 1, 'manual', ?5, ?6, ?7, ?7)",
                params![d, text, util::fmt_d(at.date()), util::fmt_t(at.time()), task_id, note_id, now_s],
            )?;
        }
        None => {
            // 일반 기록 (AI 가 다시 할 일로 정리하지 않도록 정리된 상태로 둔다)
            conn.execute(
                "INSERT INTO notes(raw_text, created_at, updated_at, status, summary, summary_source, classified_at, classified_by, track, from_task_id, ref_note_id)
                 VALUES (?1, ?2, ?3, 'classified', ?1, 'extract', ?3, 'manual', 'general', ?4, ?5)",
                params![text, at_s, now_s, task_id, note_id],
            )?;
        }
    }
    Ok(())
}

pub fn delete_task(conn: &Connection, id: i64) -> AppResult<()> {
    reminders::delete_for(conn, TargetKind::Task, id)?;
    conn.execute("DELETE FROM tasks WHERE id=?1", params![id])?;
    Ok(())
}

// ───────────────────────── 일정 ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct EventDto {
    pub id: i64,
    pub layer: String,
    pub note_id: Option<i64>,
    pub title: String,
    pub start_date: String,
    pub end_date: String,
    pub time: Option<String>,
    pub date_expr: Option<String>,
    pub memo: String,
    pub source: String,
    pub edited: bool,
    pub remind: ReminderSpec,
    pub remind_text: String,
    pub next_reminder: Option<String>,
    pub created_at: String,
    /// '' = 교사가 추가, 'neis' = 나이스에서 받은 학사력
    pub ext_source: String,
    /// holiday | closure | vacation | work ('' = 학사력이 아님)
    pub day_kind: String,
    /// 학년 태그 ("1,3", 빈 값 = 전체)
    pub grades: String,
    /// 목록에 보이지 않고 날짜 색만 칠함 (토요휴업일)
    pub hidden: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventInput {
    pub layer: String,
    pub title: String,
    pub start_date: String,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub date_expr: Option<String>,
    #[serde(default)]
    pub memo: Option<String>,
    #[serde(default)]
    pub remind: Option<ReminderSpec>,
}

pub const LAYERS: [&str; 4] = ["annual", "monthly", "weekly", "note"];

const EVENT_COLS: &str = "id, layer, note_id, title, start_date, end_date, time, date_expr, memo, source, edited,
    remind_enabled, remind_offsets, remind_time, remind_custom_at, created_at, ext_source, day_kind, grades, hidden";

fn map_event(r: &rusqlite::Row) -> rusqlite::Result<EventDto> {
    let remind = spec_from_row(r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?);
    Ok(EventDto {
        id: r.get(0)?,
        layer: r.get(1)?,
        note_id: r.get(2)?,
        title: r.get(3)?,
        start_date: r.get(4)?,
        end_date: r.get(5)?,
        time: r.get(6)?,
        date_expr: r.get(7)?,
        memo: r.get(8)?,
        source: r.get(9)?,
        edited: r.get::<_, i64>(10)? != 0,
        remind_text: reminders::describe(&remind),
        remind,
        next_reminder: None,
        created_at: r.get(15)?,
        ext_source: r.get(16)?,
        day_kind: r.get(17)?,
        grades: r.get(18)?,
        hidden: r.get::<_, i64>(19)? != 0,
    })
}

pub fn get_event(conn: &Connection, id: i64) -> AppResult<EventDto> {
    let mut e = conn
        .query_row(&format!("SELECT {EVENT_COLS} FROM events WHERE id=?1"), params![id], map_event)
        .optional()?
        .ok_or_else(|| AppError::not_found("일정을 찾을 수 없습니다."))?;
    e.next_reminder = reminders::next_for(conn, TargetKind::Event, id)?;
    Ok(e)
}

pub fn query_events(conn: &Connection, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> AppResult<Vec<EventDto>> {
    let mut st = conn.prepare(&format!("SELECT {EVENT_COLS} FROM events WHERE {where_sql}"))?;
    let mut rows = st.query_map(args, map_event)?.collect::<Result<Vec<_>, _>>()?;
    for e in rows.iter_mut() {
        e.next_reminder = reminders::next_for(conn, TargetKind::Event, e.id)?;
    }
    Ok(rows)
}

fn check_event(input: &EventInput) -> AppResult<(String, String, String, Option<String>)> {
    if !LAYERS.contains(&input.layer.as_str()) {
        return Err(AppError::invalid("일정 종류가 올바르지 않습니다."));
    }
    let title = check_title(&input.title, "일정")?;
    let start = util::opt_date(Some(&input.start_date))?.ok_or_else(|| AppError::invalid("일정 날짜를 입력해 주세요."))?;
    let end = util::opt_date(input.end_date.as_deref())?.unwrap_or_else(|| start.clone());
    if end < start {
        return Err(AppError::invalid("끝나는 날짜가 시작 날짜보다 빠릅니다."));
    }
    let time = util::opt_time(input.time.as_deref())?;
    Ok((title, start, end, time))
}

/// 학사력이면 이름 규칙으로 종류(휴일·휴업일·방학·업무)를 정한다
fn day_kind_of(conn: &Connection, layer: &str, title: &str) -> AppResult<&'static str> {
    if layer != "annual" {
        return Ok("");
    }
    Ok(crate::neis::day_kind_for(title, "", &crate::settings::load(conn)?.day_rules))
}

pub fn create_event(conn: &Connection, input: &EventInput, now: NaiveDateTime, default_remind: &ReminderSpec) -> AppResult<i64> {
    let (title, start, end, time) = check_event(input)?;
    // 학사력·계획표는 기본 알림을 끈다 (필요한 항목만 교사가 켠다)
    let spec = input.remind.clone().unwrap_or_else(|| ReminderSpec { enabled: false, ..default_remind.clone() });
    let (en, offs, rt, custom) = norm_spec(&spec)?;
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO events(layer, title, start_date, end_date, time, date_expr, memo, source, remind_enabled, remind_offsets, remind_time, remind_custom_at, created_at, updated_at, day_kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'manual', ?8, ?9, ?10, ?11, ?12, ?12, ?13)",
        params![
            input.layer,
            title,
            start,
            end,
            time,
            input.date_expr,
            input.memo.clone().unwrap_or_default(),
            en,
            offs,
            rt,
            custom,
            now_s,
            day_kind_of(conn, &input.layer, &title)?
        ],
    )?;
    let id = conn.last_insert_rowid();
    reminders::rebuild(conn, TargetKind::Event, id, now)?;
    Ok(id)
}

pub fn update_event(conn: &Connection, id: i64, input: &EventInput, now: NaiveDateTime) -> AppResult<()> {
    let cur = get_event(conn, id)?;
    let (title, start, end, time) = check_event(input)?;
    let spec = input.remind.clone().unwrap_or(cur.remind);
    let (en, offs, rt, custom) = norm_spec(&spec)?;
    // 나이스 학사력을 고치면 교사의 학사력이 된다 (같은 일정 표시 ext_key 는 남겨 다시 받아도 겹치지 않게)
    let kind = if input.layer == "annual" && cur.ext_source == "neis" && cur.title == title {
        cur.day_kind.clone()
    } else {
        day_kind_of(conn, &input.layer, &title)?.to_string()
    };
    conn.execute(
        "UPDATE events SET layer=?2, title=?3, start_date=?4, end_date=?5, time=?6, date_expr=?7, memo=?8,
            remind_enabled=?9, remind_offsets=?10, remind_time=?11, remind_custom_at=?12,
            edited=CASE WHEN source='ai' THEN 1 ELSE edited END, updated_at=?13, day_kind=?14,
            ext_source=CASE WHEN ext_source='neis' THEN '' ELSE ext_source END WHERE id=?1",
        params![id, input.layer, title, start, end, time, input.date_expr, input.memo.clone().unwrap_or_default(), en, offs, rt, custom, util::fmt_dt(now), kind],
    )?;
    reminders::rebuild(conn, TargetKind::Event, id, now)?;
    Ok(())
}

pub fn delete_event(conn: &Connection, id: i64) -> AppResult<()> {
    reminders::delete_for(conn, TargetKind::Event, id)?;
    // 나이스에서 받은 학사력은 지운 것으로 표시만 한다 (다시 받아도 되살아나지 않게)
    let n = conn.execute(
        "UPDATE events SET ext_source='removed', hidden=1, day_kind='', remind_enabled=0 WHERE id=?1 AND ext_source='neis'",
        params![id],
    )?;
    if n == 0 {
        conn.execute("DELETE FROM events WHERE id=?1", params![id])?;
    }
    Ok(())
}

// ───────────────────────── 업무 기록 ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DutyLogDto {
    pub id: i64,
    pub note_id: Option<i64>,
    pub duty_id: Option<i64>,
    pub duty_label: Option<String>,
    pub content: String,
    pub date: Option<String>,
    pub time: Option<String>,
    pub date_expr: Option<String>,
    pub done: bool,
    pub source: String,
    pub edited: bool,
    pub remind: ReminderSpec,
    pub remind_text: String,
    pub next_reminder: Option<String>,
    pub created_at: String,
    /// 할 일을 끝내 남긴 완료 기록이면 그 할 일
    pub from_task_id: Option<i64>,
    /// 그 할 일이 나온 원래 기록
    pub ref_note_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DutyLogInput {
    #[serde(default)]
    pub duty_id: Option<i64>,
    pub content: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub date_expr: Option<String>,
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub remind: Option<ReminderSpec>,
}

const DUTY_LOG_COLS: &str = "id, note_id, duty_id, content, date, time, date_expr, done, source, edited,
    remind_enabled, remind_offsets, remind_time, remind_custom_at, created_at, from_task_id, ref_note_id";

fn map_duty_log(r: &rusqlite::Row) -> rusqlite::Result<DutyLogDto> {
    let remind = spec_from_row(r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?);
    Ok(DutyLogDto {
        id: r.get(0)?,
        note_id: r.get(1)?,
        duty_id: r.get(2)?,
        duty_label: None,
        content: r.get(3)?,
        date: r.get(4)?,
        time: r.get(5)?,
        date_expr: r.get(6)?,
        done: r.get::<_, i64>(7)? != 0,
        source: r.get(8)?,
        edited: r.get::<_, i64>(9)? != 0,
        remind_text: reminders::describe(&remind),
        remind,
        next_reminder: None,
        created_at: r.get(14)?,
        from_task_id: r.get(15)?,
        ref_note_id: r.get(16)?,
    })
}

pub fn query_duty_logs(conn: &Connection, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> AppResult<Vec<DutyLogDto>> {
    let mut st = conn.prepare(&format!("SELECT {DUTY_LOG_COLS} FROM duty_logs WHERE {where_sql}"))?;
    let mut rows = st.query_map(args, map_duty_log)?.collect::<Result<Vec<_>, _>>()?;
    for d in rows.iter_mut() {
        d.duty_label = duty_label(conn, d.duty_id)?;
        d.next_reminder = reminders::next_for(conn, TargetKind::DutyLog, d.id)?;
    }
    Ok(rows)
}

pub fn get_duty_log(conn: &Connection, id: i64) -> AppResult<DutyLogDto> {
    query_duty_logs(conn, "id=?1", &[&id])?.into_iter().next().ok_or_else(|| AppError::not_found("업무 기록을 찾을 수 없습니다."))
}

pub fn create_duty_log(conn: &Connection, input: &DutyLogInput, now: NaiveDateTime, default_remind: &ReminderSpec) -> AppResult<i64> {
    let content = check_title(&input.content, "업무 기록")?;
    let date = util::opt_date(input.date.as_deref())?;
    let time = util::opt_time(input.time.as_deref())?;
    let spec = input.remind.clone().unwrap_or_else(|| ReminderSpec { enabled: date.is_some() && default_remind.enabled, ..default_remind.clone() });
    let (en, offs, rt, custom) = norm_spec(&spec)?;
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO duty_logs(duty_id, content, date, time, date_expr, done, source, remind_enabled, remind_offsets, remind_time, remind_custom_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'manual', ?7, ?8, ?9, ?10, ?11, ?11)",
        params![input.duty_id, content, date, time, input.date_expr, input.done.unwrap_or(false) as i64, en, offs, rt, custom, now_s],
    )?;
    let id = conn.last_insert_rowid();
    reminders::rebuild(conn, TargetKind::DutyLog, id, now)?;
    Ok(id)
}

pub fn update_duty_log(conn: &Connection, id: i64, input: &DutyLogInput, now: NaiveDateTime) -> AppResult<()> {
    let cur = get_duty_log(conn, id)?;
    let content = check_title(&input.content, "업무 기록")?;
    let date = util::opt_date(input.date.as_deref())?;
    let time = util::opt_time(input.time.as_deref())?;
    let spec = input.remind.clone().unwrap_or(cur.remind);
    let (en, offs, rt, custom) = norm_spec(&spec)?;
    conn.execute(
        "UPDATE duty_logs SET duty_id=?2, content=?3, date=?4, time=?5, date_expr=?6, done=?7,
            remind_enabled=?8, remind_offsets=?9, remind_time=?10, remind_custom_at=?11,
            edited=CASE WHEN source='ai' THEN 1 ELSE edited END, updated_at=?12 WHERE id=?1",
        params![id, input.duty_id, content, date, time, input.date_expr, input.done.unwrap_or(cur.done) as i64, en, offs, rt, custom, util::fmt_dt(now)],
    )?;
    reminders::rebuild(conn, TargetKind::DutyLog, id, now)?;
    Ok(())
}

pub fn set_duty_log_done(conn: &Connection, id: i64, done: bool, now: NaiveDateTime) -> AppResult<()> {
    conn.execute("UPDATE duty_logs SET done=?2, updated_at=?3 WHERE id=?1", params![id, done as i64, util::fmt_dt(now)])?;
    reminders::rebuild(conn, TargetKind::DutyLog, id, now)?;
    Ok(())
}

pub fn delete_duty_log(conn: &Connection, id: i64) -> AppResult<()> {
    reminders::delete_for(conn, TargetKind::DutyLog, id)?;
    conn.execute("DELETE FROM duty_logs WHERE id=?1", params![id])?;
    Ok(())
}

// ───────────────────────── 오늘 화면 ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AgendaItem {
    /// task | duty_log | event
    pub kind: String,
    pub id: i64,
    pub title: String,
    pub date: Option<String>,
    pub end_date: Option<String>,
    pub time: Option<String>,
    pub date_expr: Option<String>,
    pub approximate: bool,
    pub done: bool,
    pub remind_text: String,
    pub remind_enabled: bool,
    pub next_reminder: Option<String>,
    pub source: String,
    pub note_id: Option<i64>,
    pub duty_label: Option<String>,
    pub layer: Option<String>,
    /// 일정 메모 (장소·방문자 같은 설명)
    pub memo: String,
    /// 학사력 종류 (holiday | closure | vacation | work | 빈 값)
    pub day_kind: String,
    /// 휴일·휴업일·방학이면 설정에서 고른 색 이름 (그 밖의 일정은 층 색으로 보인다)
    pub day_color: Option<String>,
}

fn agenda_from_task(t: TaskDto) -> AgendaItem {
    AgendaItem {
        kind: "task".into(),
        id: t.id,
        title: t.title,
        date: t.due_date,
        end_date: None,
        time: t.due_time,
        date_expr: t.date_expr,
        approximate: t.approximate,
        done: t.done,
        remind_enabled: t.remind.enabled,
        remind_text: t.remind_text,
        next_reminder: t.next_reminder,
        source: t.source,
        note_id: t.note_id,
        duty_label: t.duty_label,
        layer: None,
        memo: String::new(),
        day_kind: String::new(),
        day_color: None,
    }
}

fn agenda_from_duty(d: DutyLogDto) -> AgendaItem {
    AgendaItem {
        kind: "duty_log".into(),
        id: d.id,
        title: d.content,
        date: d.date,
        end_date: None,
        time: d.time,
        date_expr: d.date_expr,
        approximate: false,
        done: d.done,
        remind_enabled: d.remind.enabled,
        remind_text: d.remind_text,
        next_reminder: d.next_reminder,
        source: d.source,
        note_id: d.note_id,
        duty_label: d.duty_label,
        layer: None,
        memo: String::new(),
        day_kind: String::new(),
        day_color: None,
    }
}

fn agenda_from_event(e: EventDto, colors: &crate::settings::DayColors) -> AgendaItem {
    let day_color = match e.day_kind.as_str() {
        "holiday" => Some(colors.holiday.clone()),
        "closure" => Some(colors.closure.clone()),
        "vacation" => Some(colors.vacation.clone()),
        _ => None,
    };
    AgendaItem {
        kind: "event".into(),
        id: e.id,
        title: e.title,
        date: Some(e.start_date),
        end_date: Some(e.end_date),
        time: e.time,
        date_expr: e.date_expr,
        approximate: false,
        done: false,
        remind_enabled: e.remind.enabled,
        remind_text: e.remind_text,
        next_reminder: e.next_reminder,
        source: e.source,
        note_id: e.note_id,
        duty_label: None,
        layer: Some(e.layer),
        memo: e.memo,
        day_kind: e.day_kind,
        day_color,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TodayOverview {
    pub today: String,
    pub overdue: Vec<AgendaItem>,
    pub due_today: Vec<AgendaItem>,
    pub upcoming: Vec<AgendaItem>,
    pub undated: Vec<AgendaItem>,
    pub events_today: Vec<AgendaItem>,
    pub events_upcoming: Vec<AgendaItem>,
    pub unclassified_count: i64,
    /// 교사가 답해야 할 것: 특이사항 "어디에 둘까요?" + 개인정보 "기존 → 새 값" 확인
    pub needs_answer_count: i64,
    /// 메뉴별 새로 정리된 항목 (학생부 자료·교무 업무·할 일·학생 명렬표)
    pub new_counts: crate::seen::NewCounts,
    pub notes_today: i64,
    pub reminder_log: Vec<reminders::ReminderLog>,
    /// 오늘 생일인 학생 (설정에서 켰을 때)
    pub birthdays_today: Vec<String>,
}

pub const UPCOMING_DAYS: i64 = 14;

fn sort_agenda(v: &mut [AgendaItem]) {
    v.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then(a.time.is_none().cmp(&b.time.is_none()))
            .then(a.time.cmp(&b.time))
            .then(a.id.cmp(&b.id))
    });
}

/// 오늘 화면: 기한 지난 할 일 → 오늘 할 일 → 앞으로 2주
pub fn today_overview(conn: &Connection, today: NaiveDate, now: NaiveDateTime, include_birthdays: bool) -> AppResult<TodayOverview> {
    let t = util::fmt_d(today);
    let until = util::fmt_d(today + Duration::days(UPCOMING_DAYS));
    let mut open: Vec<AgendaItem> = query_tasks(conn, "done=0 AND due_date IS NOT NULL AND due_date <= ?1", &[&until])?
        .into_iter()
        .map(agenda_from_task)
        .collect();
    open.extend(
        query_duty_logs(conn, "done=0 AND date IS NOT NULL AND date >= ?1 AND date <= ?2", &[&t, &until])?
            .into_iter()
            .map(agenda_from_duty),
    );
    sort_agenda(&mut open);
    let mut overdue = Vec::new();
    let mut due_today = Vec::new();
    let mut upcoming = Vec::new();
    for it in open {
        match it.date.as_deref() {
            Some(d) if d < t.as_str() => overdue.push(it),
            Some(d) if d == t.as_str() => due_today.push(it),
            _ => upcoming.push(it),
        }
    }
    let undated: Vec<AgendaItem> = query_tasks(conn, "done=0 AND due_date IS NULL ORDER BY created_at DESC LIMIT 20", &[])?
        .into_iter()
        .map(agenda_from_task)
        .collect();
    let colors = crate::settings::load(conn)?.day_colors;
    let mut events_today: Vec<AgendaItem> = query_events(conn, "start_date <= ?1 AND end_date >= ?1 AND hidden=0", &[&t])?
        .into_iter()
        .map(|e| agenda_from_event(e, &colors))
        .collect();
    sort_agenda(&mut events_today);
    let tomorrow = util::fmt_d(today + Duration::days(1));
    let mut events_upcoming: Vec<AgendaItem> = query_events(conn, "start_date >= ?1 AND start_date <= ?2 AND hidden=0", &[&tomorrow, &until])?
        .into_iter()
        .map(|e| agenda_from_event(e, &colors))
        .collect();
    sort_agenda(&mut events_upcoming);
    let unclassified_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM notes WHERE status IN ('pending','processing','failed')", [], |r| r.get(0))?;
    let notes_today: i64 = conn.query_row("SELECT COUNT(*) FROM notes WHERE substr(created_at,1,10)=?1", params![t], |r| r.get(0))?;
    let reminder_log = reminders::recent(conn, now - Duration::days(2))?;
    let birthdays_today = if include_birthdays {
        crate::students::birthdays_in_range(conn, today, today)?.into_iter().map(|b| b.label).collect()
    } else {
        vec![]
    };
    Ok(TodayOverview {
        today: t,
        overdue,
        due_today,
        upcoming,
        undated,
        events_today,
        events_upcoming,
        unclassified_count,
        needs_answer_count: crate::profile::pending_count(conn)?,
        new_counts: crate::seen::counts(conn, now)?,
        notes_today,
        reminder_log,
        birthdays_today,
    })
}

// ───────────────────────── 일정표 ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct CalendarItem {
    /// event | task | duty_log
    pub kind: String,
    pub id: i64,
    /// annual | monthly | weekly | note | task | duty
    pub layer: String,
    pub title: String,
    pub start_date: String,
    pub end_date: String,
    pub time: Option<String>,
    pub done: bool,
    pub source: String,
    pub note_id: Option<i64>,
    pub memo: String,
    /// 학사력 종류 (holiday | closure | vacation | work)
    pub day_kind: String,
    /// 학년 태그
    pub grades: String,
    /// 목록에 보이지 않고 날짜 색만 칠함 (토요휴업일)
    pub hidden: bool,
}

/// 반영 방향 규칙: 연간 학사력 → 연·월·주 / 월간 계획 → 월·주 / 주간 계획 → 주 / 기록·할 일·업무 → 월·주
pub fn layer_visible(layer: &str, view: &str) -> bool {
    match layer {
        // 학생 생일은 학사력처럼 모든 보기에 나온다 (설정에서 켰을 때만)
        "annual" | "birthday" => matches!(view, "year" | "month" | "week"),
        "monthly" | "note" | "task" | "duty" | "tt_change" => matches!(view, "month" | "week"),
        "weekly" => view == "week",
        _ => false,
    }
}

pub fn calendar(conn: &Connection, view: &str, start: &str, end: &str, include_birthdays: bool) -> AppResult<Vec<CalendarItem>> {
    if !matches!(view, "year" | "month" | "week") {
        return Err(AppError::invalid("일정표 보기 종류가 올바르지 않습니다."));
    }
    let start = util::opt_date(Some(start))?.ok_or_else(|| AppError::invalid("시작 날짜가 필요합니다."))?;
    let end = util::opt_date(Some(end))?.ok_or_else(|| AppError::invalid("끝 날짜가 필요합니다."))?;
    let mut out: Vec<CalendarItem> = Vec::new();
    if include_birthdays && layer_visible("birthday", view) {
        if let (Some(s), Some(e)) = (util::parse_d(&start), util::parse_d(&end)) {
            for b in crate::students::birthdays_in_range(conn, s, e)? {
                out.push(CalendarItem {
                    kind: "birthday".into(),
                    id: b.student_id,
                    layer: "birthday".into(),
                    title: format!("{} 생일", b.label),
                    start_date: b.date.clone(),
                    end_date: b.date,
                    time: None,
                    done: false,
                    source: "manual".into(),
                    note_id: None,
                    memo: String::new(),
                    day_kind: String::new(),
                    grades: String::new(),
                    hidden: false,
                });
            }
        }
    }
    for e in query_events(conn, "start_date <= ?2 AND end_date >= ?1 AND ext_source<>'removed' ORDER BY start_date, time IS NULL, time, id", &[&start, &end])? {
        if !layer_visible(&e.layer, view) {
            continue;
        }
        out.push(CalendarItem {
            kind: "event".into(),
            id: e.id,
            layer: e.layer,
            title: e.title,
            start_date: e.start_date,
            end_date: e.end_date,
            time: e.time,
            done: false,
            source: e.source,
            note_id: e.note_id,
            memo: e.memo,
            day_kind: e.day_kind,
            grades: e.grades,
            hidden: e.hidden,
        });
    }
    if layer_visible("task", view) {
        for t in query_tasks(conn, "due_date IS NOT NULL AND due_date >= ?1 AND due_date <= ?2 ORDER BY due_date, due_time", &[&start, &end])? {
            let d = t.due_date.clone().unwrap_or_default();
            out.push(CalendarItem {
                kind: "task".into(),
                id: t.id,
                layer: "task".into(),
                title: t.title,
                start_date: d.clone(),
                end_date: d,
                time: t.due_time,
                done: t.done,
                source: t.source,
                note_id: t.note_id,
                memo: String::new(),
                day_kind: String::new(),
                grades: String::new(),
                hidden: false,
            });
        }
    }
    if layer_visible("duty", view) {
        for l in query_duty_logs(conn, "date IS NOT NULL AND date >= ?1 AND date <= ?2 ORDER BY date, time", &[&start, &end])? {
            let d = l.date.clone().unwrap_or_default();
            out.push(CalendarItem {
                kind: "duty_log".into(),
                id: l.id,
                layer: "duty".into(),
                title: l.content,
                start_date: d.clone(),
                end_date: d,
                time: l.time,
                done: l.done,
                source: l.source,
                note_id: l.note_id,
                memo: String::new(),
                day_kind: String::new(),
                grades: String::new(),
                hidden: false,
            });
        }
    }
    // 기록에서 찾은 하루짜리 시간표 변경 (보강·교체·자습·휴강 …)
    if layer_visible("tt_change", view) {
        for e in crate::ttinput::changes_between(conn, &start, &end)? {
            let d = e.date.clone().unwrap_or_default();
            out.push(CalendarItem {
                kind: "tt_change".into(),
                id: e.id,
                layer: "tt_change".into(),
                title: [crate::ttinput::period_label(e.period), e.class_label, e.subject, e.change_type]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
                start_date: d.clone(),
                end_date: d,
                time: e.time,
                done: false,
                source: "ai".into(),
                note_id: Some(e.note_id),
                memo: e.text,
                day_kind: String::new(),
                grades: String::new(),
                hidden: false,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn now() -> NaiveDateTime {
        util::parse_dt("2026-09-29T09:00:00").unwrap()
    }
    fn today() -> NaiveDate {
        now().date()
    }
    #[test]
    fn done_task_leaves_record_and_undo_removes_it() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let at = util::parse_dt("2026-10-08T13:30:00").unwrap();
        // 업무와 연결 없는 할 일 → 일반 기록
        let t1 = create_task(&c, &TaskInput { title: "군청에 참석 인원 회신".into(), due_date: None, due_time: None, date_expr: None, duty_id: None, done: None, remind: None }, now(), &spec()).unwrap();
        set_task_done(&c, t1, true, at).unwrap();
        let (raw, track, status): (String, String, String) =
            c.query_row("SELECT raw_text, track, status FROM notes WHERE from_task_id=?1", params![t1], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(raw, "10월 8일 13:30 완료 · 군청에 참석 인원 회신");
        assert_eq!((track.as_str(), status.as_str()), ("general", "classified"));
        // 체크를 풀면 완료 기록도 지운다
        set_task_done(&c, t1, false, at).unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM notes WHERE from_task_id=?1", params![t1], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        // 업무와 연결된 할 일 → 그 업무의 업무 기록 (교사가 체크한 것이라 manual)
        let duty = crate::duties::create_duty(&c, &crate::duties::DutyInput { name: "교무부장".into(), detail: "국제교류".into(), guide: String::new(), keywords: String::new(), active: true }, now()).unwrap();
        let t2 = create_task(&c, &TaskInput { title: "교장 보고".into(), due_date: None, due_time: None, date_expr: None, duty_id: Some(duty), done: None, remind: None }, now(), &spec()).unwrap();
        set_task_done(&c, t2, true, at).unwrap();
        let logs = query_duty_logs(&c, "from_task_id=?1", &[&t2]).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!((logs[0].content.as_str(), logs[0].duty_id, logs[0].source.as_str(), logs[0].date.as_deref()), ("10월 8일 13:30 완료 · 교장 보고", Some(duty), "manual", Some("2026-10-08")));
        let general: i64 = c.query_row("SELECT COUNT(*) FROM notes WHERE from_task_id=?1", params![t2], |r| r.get(0)).unwrap();
        assert_eq!(general, 0, "업무와 연결된 할 일은 일반 기록을 만들지 않는다");
        // 할 일을 지워도 완료 기록은 남는다 (이미 끝낸 일의 기록)
        delete_task(&c, t2).unwrap();
        assert_eq!(query_duty_logs(&c, "from_task_id=?1", &[&t2]).unwrap().len(), 1);
    }

    fn spec() -> ReminderSpec {
        ReminderSpec { enabled: true, offsets: vec![1], time: "08:30".into(), custom_at: None }
    }
    fn ev(layer: &str, title: &str, date: &str) -> EventInput {
        EventInput { layer: layer.into(), title: title.into(), start_date: date.into(), end_date: None, time: None, date_expr: None, memo: None, remind: None }
    }

    #[test]
    fn one_way_layer_visibility() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        create_event(&c, &ev("annual", "개학", "2026-10-05"), now(), &spec()).unwrap();
        create_event(&c, &ev("monthly", "교과협의회", "2026-10-06"), now(), &spec()).unwrap();
        create_event(&c, &ev("weekly", "3교시 공개수업", "2026-10-07"), now(), &spec()).unwrap();
        let titles = |view: &str| -> Vec<String> {
            calendar(&c, view, "2026-10-01", "2026-10-31", false).unwrap().into_iter().map(|i| i.title).collect()
        };
        assert_eq!(titles("year"), vec!["개학"]);
        assert_eq!(titles("month"), vec!["개학", "교과협의회"]);
        assert_eq!(titles("week"), vec!["개학", "교과협의회", "3교시 공개수업"]);
    }

    #[test]
    fn birthdays_follow_setting_and_show_in_all_views() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let id = crate::students::create_student(
            &c,
            &crate::students::StudentInput { name: "홍길동".into(), class_label: Some("2-1".into()), number: Some(3), memo: None },
            now(),
        )
        .unwrap();
        crate::students::set_roster_field(&c, id, "birthday", "10월 3일", now()).unwrap();
        let has = |view: &str, s: &str, e: &str, on: bool| calendar(&c, view, s, e, on).unwrap().iter().any(|i| i.kind == "birthday" && i.title == "2-1 홍길동 생일");
        assert!(!has("month", "2026-09-28", "2026-11-08", false), "끄면 보이지 않는다");
        assert!(has("year", "2026-01-01", "2026-12-31", true));
        assert!(has("month", "2026-09-28", "2026-11-08", true));
        assert!(has("week", "2026-09-28", "2026-10-04", true));
        let o = today_overview(&c, util::parse_d("2026-10-03").unwrap(), now(), true).unwrap();
        assert_eq!(o.birthdays_today, vec!["2-1 홍길동".to_string()]);
    }

    #[test]
    fn dated_tasks_show_on_week_and_month_only() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let input = TaskInput { title: "보호자 연락".into(), due_date: Some("2026-10-02".into()), due_time: None, date_expr: Some("금요일까지".into()), duty_id: None, done: None, remind: None };
        create_task(&c, &input, now(), &spec()).unwrap();
        let week = calendar(&c, "week", "2026-09-28", "2026-10-04", false).unwrap();
        assert!(week.iter().any(|i| i.kind == "task" && i.title == "보호자 연락" && i.start_date == "2026-10-02"));
        let month = calendar(&c, "month", "2026-09-28", "2026-11-08", false).unwrap();
        assert!(month.iter().any(|i| i.kind == "task"));
        let year = calendar(&c, "year", "2026-01-01", "2026-12-31", false).unwrap();
        assert!(year.iter().all(|i| i.kind != "task"));
    }

    #[test]
    fn today_order_and_reminders() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let mk = |t: &str, d: &str| TaskInput { title: t.into(), due_date: Some(d.into()), due_time: None, date_expr: None, duty_id: None, done: None, remind: None };
        create_task(&c, &mk("지난 일", "2026-09-25"), now(), &spec()).unwrap();
        create_task(&c, &mk("오늘 일", "2026-09-29"), now(), &spec()).unwrap();
        let later = create_task(&c, &mk("다음 주 일", "2026-10-09"), now(), &spec()).unwrap();
        create_task(&c, &mk("먼 일", "2026-11-30"), now(), &spec()).unwrap();
        let o = today_overview(&c, today(), now(), false).unwrap();
        assert_eq!(o.overdue.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["지난 일"]);
        assert_eq!(o.due_today.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["오늘 일"]);
        assert_eq!(o.upcoming.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["다음 주 일"]);
        let t = get_task(&c, later).unwrap();
        assert!(t.remind.enabled);
        assert_eq!(t.next_reminder.as_deref(), Some("2026-10-08T08:30:00"));
        set_task_done(&c, later, true, now()).unwrap();
        assert!(get_task(&c, later).unwrap().next_reminder.is_none());
        let o = today_overview(&c, today(), now(), false).unwrap();
        assert!(o.upcoming.is_empty());
    }

    #[test]
    fn edit_task_date_rebuilds_reminders() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let id = create_task(
            &c,
            &TaskInput { title: "보호자 연락".into(), due_date: Some("2026-10-02".into()), due_time: None, date_expr: None, duty_id: None, done: None, remind: None },
            now(),
            &spec(),
        )
        .unwrap();
        let mut input = TaskInput { title: "보호자 전화 연락".into(), due_date: Some("2026-10-09".into()), due_time: Some("15:00".into()), date_expr: Some("다음 주 금요일".into()), duty_id: None, done: None, remind: None };
        input.remind = Some(ReminderSpec { enabled: true, offsets: vec![3, 1], time: "07:50".into(), custom_at: None });
        update_task(&c, id, &input, now()).unwrap();
        let t = get_task(&c, id).unwrap();
        assert_eq!(t.title, "보호자 전화 연락");
        assert_eq!(t.due_time.as_deref(), Some("15:00"));
        assert_eq!(t.next_reminder.as_deref(), Some("2026-10-06T07:50:00"));
        assert!(update_task(&c, id, &TaskInput { title: " ".into(), due_date: None, due_time: None, date_expr: None, duty_id: None, done: None, remind: None }, now()).is_err());
    }
}
