//! 알림 대기열.
//!
//! 업무 기록·할 일·일정의 날짜와 알림 설정(몇 일 전, 몇 시, 직접 정한 시각)으로부터 예약을 만든다.
//! 날짜나 설정이 바뀌면 남은 예약을 지우고 다시 만든다. 완료된 항목의 남은 알림은 지운다.

use crate::classify::ReminderSpec;
use crate::error::AppResult;
use crate::util;
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Task,
    Event,
    DutyLog,
}

impl TargetKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetKind::Task => "task",
            TargetKind::Event => "event",
            TargetKind::DutyLog => "duty_log",
        }
    }
    pub fn parse(s: &str) -> Option<TargetKind> {
        match s {
            "task" => Some(TargetKind::Task),
            "event" => Some(TargetKind::Event),
            "duty_log" => Some(TargetKind::DutyLog),
            _ => None,
        }
    }
    fn select_sql(&self) -> &'static str {
        match self {
            TargetKind::Task => {
                "SELECT title, due_date, due_time, done, remind_enabled, remind_offsets, remind_time, remind_custom_at FROM tasks WHERE id=?1"
            }
            TargetKind::Event => {
                "SELECT title, start_date, time, 0, remind_enabled, remind_offsets, remind_time, remind_custom_at FROM events WHERE id=?1"
            }
            TargetKind::DutyLog => {
                "SELECT content, date, time, done, remind_enabled, remind_offsets, remind_time, remind_custom_at FROM duty_logs WHERE id=?1"
            }
        }
    }
}

pub const OFFSET_CHOICES: [i64; 5] = [7, 3, 2, 1, 0];

pub fn offsets_to_json(v: &[i64]) -> String {
    let mut v: Vec<i64> = v.iter().copied().filter(|x| (0..=30).contains(x)).collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    serde_json::to_string(&v).unwrap_or_else(|_| "[]".into())
}

pub fn offsets_from_json(s: &str) -> Vec<i64> {
    serde_json::from_str::<Vec<i64>>(s).unwrap_or_default()
}

pub fn offset_label(off: i64) -> String {
    if off == 0 { "당일".to_string() } else { format!("{off}일 전") }
}

/// "3일 전·1일 전 08:30" 처럼 설정을 한 줄로
pub fn describe(spec: &ReminderSpec) -> String {
    if !spec.enabled {
        return "알림 없음".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    if !spec.offsets.is_empty() {
        let mut offs = spec.offsets.clone();
        offs.sort_unstable_by(|a, b| b.cmp(a));
        parts.push(format!("{} {}", offs.iter().map(|o| offset_label(*o)).collect::<Vec<_>>().join("·"), spec.time));
    }
    if let Some(c) = spec.custom_at.as_deref().and_then(util::parse_dt) {
        parts.push(format!("{} {}", c.format("%-m월 %-d일"), c.format("%H:%M")));
    }
    if parts.is_empty() { "알림 켜짐 (시점 없음)".to_string() } else { parts.join(", ") }
}

struct Target {
    date: Option<NaiveDate>,
    done: bool,
    spec: ReminderSpec,
}

fn load(conn: &Connection, kind: TargetKind, id: i64) -> AppResult<Option<Target>> {
    Ok(conn
        .query_row(kind.select_sql(), params![id], |r| {
            let date: Option<String> = r.get(1)?;
            let done: i64 = r.get(3)?;
            let enabled: i64 = r.get(4)?;
            let offsets: String = r.get(5)?;
            let time: String = r.get(6)?;
            let custom: Option<String> = r.get(7)?;
            Ok(Target {
                date: date.as_deref().and_then(util::parse_d),
                done: done != 0,
                spec: ReminderSpec { enabled: enabled != 0, offsets: offsets_from_json(&offsets), time, custom_at: custom },
            })
        })
        .optional()?)
}

/// 예약 시각 목록 계산 (지난 시각은 제외)
pub fn plan_fire_times(date: Option<NaiveDate>, spec: &ReminderSpec, now: NaiveDateTime) -> Vec<(NaiveDateTime, String)> {
    let mut out: Vec<(NaiveDateTime, String)> = Vec::new();
    if !spec.enabled {
        return out;
    }
    let t = util::parse_t(&spec.time).unwrap_or_else(|| NaiveTime::from_hms_opt(8, 30, 0).unwrap());
    if let Some(d) = date {
        for off in &spec.offsets {
            let at = (d - Duration::days(*off)).and_time(t);
            out.push((at, offset_label(*off)));
        }
    }
    if let Some(c) = spec.custom_at.as_deref().and_then(util::parse_dt) {
        out.push((c, "직접 정한 시각".to_string()));
    }
    out.retain(|(at, _)| *at >= now);
    out.sort_by_key(|(at, _)| *at);
    out.dedup_by_key(|(at, _)| *at);
    out
}

/// 대상의 남은 예약을 지우고 현재 설정으로 다시 만든다.
pub fn rebuild(conn: &Connection, kind: TargetKind, id: i64, now: NaiveDateTime) -> AppResult<usize> {
    conn.execute(
        "DELETE FROM reminders WHERE target_kind=?1 AND target_id=?2 AND status='scheduled'",
        params![kind.as_str(), id],
    )?;
    let Some(t) = load(conn, kind, id)? else { return Ok(0) };
    if t.done {
        return Ok(0);
    }
    let times = plan_fire_times(t.date, &t.spec, now);
    for (at, label) in &times {
        conn.execute(
            "INSERT INTO reminders(target_kind, target_id, fire_at, label) VALUES (?1, ?2, ?3, ?4)",
            params![kind.as_str(), id, util::fmt_dt(*at), label],
        )?;
    }
    Ok(times.len())
}

/// 대상이 지워질 때 예약도 지운다 (reminders 는 여러 표를 가리키므로 외래키 대신 직접 지운다)
pub fn delete_for(conn: &Connection, kind: TargetKind, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM reminders WHERE target_kind=?1 AND target_id=?2", params![kind.as_str(), id])?;
    Ok(())
}

pub fn delete_for_note(conn: &Connection, note_id: i64) -> AppResult<()> {
    conn.execute(
        "DELETE FROM reminders WHERE (target_kind='task' AND target_id IN (SELECT id FROM tasks WHERE note_id=?1))
            OR (target_kind='event' AND target_id IN (SELECT id FROM events WHERE note_id=?1))
            OR (target_kind='duty_log' AND target_id IN (SELECT id FROM duty_logs WHERE note_id=?1))",
        params![note_id],
    )?;
    Ok(())
}

pub fn next_for(conn: &Connection, kind: TargetKind, id: i64) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT fire_at FROM reminders WHERE target_kind=?1 AND target_id=?2 AND status='scheduled' ORDER BY fire_at LIMIT 1",
            params![kind.as_str(), id],
            |r| r.get(0),
        )
        .optional()?)
}

#[derive(Debug, Clone, Serialize)]
pub struct DueReminder {
    pub id: i64,
    pub target_kind: String,
    pub target_id: i64,
    pub fire_at: String,
    pub label: String,
    pub title: String,
    pub date: Option<String>,
}

fn target_title(conn: &Connection, kind: &str, id: i64) -> AppResult<(String, Option<String>)> {
    let sql = match kind {
        "task" => "SELECT title, due_date FROM tasks WHERE id=?1",
        "event" => "SELECT title, start_date FROM events WHERE id=?1",
        _ => "SELECT content, date FROM duty_logs WHERE id=?1",
    };
    Ok(conn
        .query_row(sql, params![id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))
        .optional()?
        .unwrap_or_else(|| ("(삭제된 항목)".to_string(), None)))
}

/// 지금 보여야 할 알림 (예약 시각이 지난 것)
pub fn due(conn: &Connection, now: NaiveDateTime) -> AppResult<Vec<DueReminder>> {
    let mut stmt = conn.prepare(
        "SELECT id, target_kind, target_id, fire_at, label FROM reminders WHERE status='scheduled' AND fire_at <= ?1 ORDER BY fire_at",
    )?;
    let rows: Vec<(i64, String, i64, String, String)> = stmt
        .query_map(params![util::fmt_dt(now)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, kind, target_id, fire_at, label) in rows {
        let (title, date) = target_title(conn, &kind, target_id)?;
        out.push(DueReminder { id, target_kind: kind, target_id, fire_at, label, title, date });
    }
    Ok(out)
}

pub fn mark(conn: &Connection, id: i64, status: &str, error: Option<&str>, now: NaiveDateTime) -> AppResult<()> {
    conn.execute(
        "UPDATE reminders SET status=?2, fired_at=?3, error=?4 WHERE id=?1",
        params![id, status, util::fmt_dt(now), error],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ReminderLog {
    pub id: i64,
    pub target_kind: String,
    pub target_id: i64,
    pub fire_at: String,
    pub label: String,
    pub status: String,
    pub fired_at: Option<String>,
    pub error: Option<String>,
    pub title: String,
    pub date: Option<String>,
}

/// 최근에 보냈거나 놓친 알림 (오늘 화면의 알림 기록)
pub fn recent(conn: &Connection, since: NaiveDateTime) -> AppResult<Vec<ReminderLog>> {
    let mut stmt = conn.prepare(
        "SELECT id, target_kind, target_id, fire_at, label, status, fired_at, error FROM reminders
         WHERE status IN ('fired','missed','failed') AND fired_at >= ?1 ORDER BY fired_at DESC LIMIT 30",
    )?;
    let rows: Vec<(i64, String, i64, String, String, String, Option<String>, Option<String>)> = stmt
        .query_map(params![util::fmt_dt(since)], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))
        })?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, target_kind, target_id, fire_at, label, status, fired_at, error) in rows {
        let (title, date) = target_title(conn, &target_kind, target_id)?;
        out.push(ReminderLog { id, target_kind, target_id, fire_at, label, status, fired_at, error, title, date });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn dt(s: &str) -> NaiveDateTime {
        util::parse_dt(s).unwrap()
    }

    fn insert_task(conn: &Connection, due: &str, offsets: &[i64], enabled: bool) -> i64 {
        conn.execute(
            "INSERT INTO tasks(title, due_date, source, remind_enabled, remind_offsets, remind_time, created_at, updated_at)
             VALUES ('보호자 연락', ?1, 'ai', ?2, ?3, '08:30', '2026-09-29T09:00:00', '2026-09-29T09:00:00')",
            params![due, enabled as i64, offsets_to_json(offsets)],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn scheduled(conn: &Connection, id: i64) -> Vec<String> {
        let mut st = conn
            .prepare("SELECT fire_at FROM reminders WHERE target_kind='task' AND target_id=?1 AND status='scheduled' ORDER BY fire_at")
            .unwrap();
        st.query_map(params![id], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
    }

    #[test]
    fn builds_multiple_offsets_and_skips_past() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let id = insert_task(&c, "2026-10-09", &[7, 3, 1], true);
        let n = rebuild(&c, TargetKind::Task, id, dt("2026-10-02T10:00:00")).unwrap();
        // 7일 전(10/2 08:30)은 이미 지났다
        assert_eq!(n, 2);
        assert_eq!(scheduled(&c, id), vec!["2026-10-06T08:30:00", "2026-10-08T08:30:00"]);
    }

    #[test]
    fn rebuild_after_date_change_and_done() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let id = insert_task(&c, "2026-10-09", &[1], true);
        rebuild(&c, TargetKind::Task, id, dt("2026-09-29T10:00:00")).unwrap();
        assert_eq!(scheduled(&c, id), vec!["2026-10-08T08:30:00"]);
        // 날짜를 바꾸면 기존 예약을 정리하고 다시 만든다
        c.execute("UPDATE tasks SET due_date='2026-10-16', remind_time='17:00' WHERE id=?1", params![id]).unwrap();
        rebuild(&c, TargetKind::Task, id, dt("2026-09-29T10:00:00")).unwrap();
        assert_eq!(scheduled(&c, id), vec!["2026-10-15T17:00:00"]);
        // 사용자가 정한 시각 추가
        c.execute("UPDATE tasks SET remind_custom_at='2026-10-01T12:10:00' WHERE id=?1", params![id]).unwrap();
        rebuild(&c, TargetKind::Task, id, dt("2026-09-29T10:00:00")).unwrap();
        assert_eq!(scheduled(&c, id), vec!["2026-10-01T12:10:00", "2026-10-15T17:00:00"]);
        // 완료하면 남은 알림이 사라진다
        c.execute("UPDATE tasks SET done=1 WHERE id=?1", params![id]).unwrap();
        rebuild(&c, TargetKind::Task, id, dt("2026-09-29T10:00:00")).unwrap();
        assert!(scheduled(&c, id).is_empty());
    }

    #[test]
    fn disabled_means_no_reminders_and_due_query() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let off = insert_task(&c, "2026-10-09", &[1], false);
        rebuild(&c, TargetKind::Task, off, dt("2026-09-29T10:00:00")).unwrap();
        assert!(scheduled(&c, off).is_empty());
        let on = insert_task(&c, "2026-09-30", &[1, 0], true);
        rebuild(&c, TargetKind::Task, on, dt("2026-09-29T08:00:00")).unwrap();
        let due_now = due(&c, dt("2026-09-29T09:00:00")).unwrap();
        assert_eq!(due_now.len(), 1);
        assert_eq!(due_now[0].label, "1일 전");
        assert_eq!(due_now[0].title, "보호자 연락");
        mark(&c, due_now[0].id, "fired", None, dt("2026-09-29T09:00:05")).unwrap();
        assert!(due(&c, dt("2026-09-29T09:00:10")).unwrap().is_empty());
        assert_eq!(recent(&c, dt("2026-09-29T00:00:00")).unwrap().len(), 1);
    }

    #[test]
    fn describe_spec() {
        let s = ReminderSpec { enabled: true, offsets: vec![1, 3], time: "08:30".into(), custom_at: None };
        assert_eq!(describe(&s), "3일 전·1일 전 08:30");
        let s = ReminderSpec { enabled: false, offsets: vec![1], time: "08:30".into(), custom_at: None };
        assert_eq!(describe(&s), "알림 없음");
    }
}
