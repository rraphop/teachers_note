//! 메뉴별 '새로 정리된 항목' 숫자 (2차 수정목록 5번)
//!
//! AI 가 정리해 학생부 자료·교무 업무·할 일·학생 명렬표에 넣은 항목 중, 그 메뉴를 마지막으로 연 뒤에 생긴 것을 센다.
//! 교사가 직접 만든 항목(source='manual')은 세지 않는다. 지워지면 숫자에서 빠지고, 다시 분류돼 다른 메뉴로 가면
//! 그 메뉴에서 새로 만든 항목(새 시각)으로 센다.

use crate::db;
use crate::error::{AppError, AppResult};
use crate::util;
use chrono::NaiveDateTime;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::BTreeMap;

const KEY: &str = "seen_marks";
pub const MENUS: [&str; 4] = ["records", "duties", "tasks", "students"];

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct NewCounts {
    pub records: i64,
    pub duties: i64,
    pub tasks: i64,
    pub students: i64,
}

fn marks(conn: &Connection) -> AppResult<Option<BTreeMap<String, String>>> {
    Ok(db::get_setting(conn, KEY)?.and_then(|s| serde_json::from_str(&s).ok()))
}

fn save(conn: &Connection, m: &BTreeMap<String, String>) -> AppResult<()> {
    db::set_setting(conn, KEY, &serde_json::to_string(m)?)
}

/// 처음 쓰는 날은 지금을 기준으로 삼는다 (업데이트 직전까지 쌓인 항목을 모두 '새 항목'으로 세지 않게)
fn marks_or_init(conn: &Connection, now: NaiveDateTime) -> AppResult<BTreeMap<String, String>> {
    if let Some(m) = marks(conn)? {
        return Ok(m);
    }
    let t = util::fmt_dt(now);
    let m: BTreeMap<String, String> = MENUS.iter().map(|k| (k.to_string(), t.clone())).collect();
    save(conn, &m)?;
    Ok(m)
}

pub fn counts(conn: &Connection, now: NaiveDateTime) -> AppResult<NewCounts> {
    let m = marks_or_init(conn, now)?;
    let since = |k: &str| m.get(k).cloned().unwrap_or_else(|| util::fmt_dt(now));
    let n = |sql: &str, s: String| -> AppResult<i64> { Ok(conn.query_row(sql, params![s], |r| r.get(0))?) };
    Ok(NewCounts {
        records: n("SELECT COUNT(*) FROM observations WHERE source='ai' AND created_at > ?1", since("records"))?,
        duties: n("SELECT COUNT(*) FROM duty_logs WHERE source='ai' AND created_at > ?1", since("duties"))?,
        tasks: n("SELECT COUNT(*) FROM tasks WHERE source='ai' AND done=0 AND created_at > ?1", since("tasks"))?,
        students: n("SELECT COUNT(*) FROM profile_changes WHERE source='ai' AND status='applied' AND created_at > ?1", since("students"))?,
    })
}

/// 메뉴를 열었다: 지금을 '본 시각'으로 적고, 그 전 시각을 돌려준다 (이번 방문에 '새' 표시할 기준)
pub fn mark_seen(conn: &Connection, menu: &str, now: NaiveDateTime) -> AppResult<Option<String>> {
    if !MENUS.contains(&menu) {
        return Err(AppError::invalid("알 수 없는 메뉴입니다."));
    }
    let mut m = marks_or_init(conn, now)?;
    let prev = m.insert(menu.to_string(), util::fmt_dt(now));
    save(conn, &m)?;
    Ok(prev)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NewObservation {
    pub student_id: Option<i64>,
    pub class_label: Option<String>,
    pub area: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct NewSince {
    /// 학생부 자료: 새 참고자료 (학생·학급·영역 점 표시용)
    pub observations: Vec<NewObservation>,
    /// 학생 명렬표: 개인정보가 새로 들어간 학생
    pub profile_students: Vec<i64>,
}

pub fn new_since(conn: &Connection, since: &str) -> AppResult<NewSince> {
    let mut st = conn.prepare(
        "SELECT o.student_id, c.label, o.area, o.subject FROM observations o
         LEFT JOIN students s ON s.id=o.student_id LEFT JOIN classes c ON c.id=s.class_id
         WHERE o.source='ai' AND o.created_at > ?1",
    )?;
    let observations = st
        .query_map(params![since], |r| Ok(NewObservation { student_id: r.get(0)?, class_label: r.get(1)?, area: r.get(2)?, subject: r.get(3)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut st = conn.prepare("SELECT DISTINCT student_id FROM profile_changes WHERE source='ai' AND status='applied' AND created_at > ?1")?;
    let profile_students = st.query_map(params![since], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
    Ok(NewSince { observations, profile_students })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn counts_ai_items_after_last_visit() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let t0 = util::parse_dt("2026-10-07T08:00:00").unwrap();
            // 처음엔 지금을 기준으로 (이전 항목은 세지 않음)
            c.execute("INSERT INTO tasks(title, source, created_at, updated_at) VALUES ('옛 할 일', 'ai', '2026-10-06T08:00:00', 'x')", [])?;
            assert_eq!(counts(c, t0)?, NewCounts::default());
            // AI 가 만든 것만, 마지막으로 본 뒤의 것만
            c.execute("INSERT INTO tasks(title, source, created_at, updated_at) VALUES ('새 할 일', 'ai', '2026-10-07T09:00:00', 'x')", [])?;
            c.execute("INSERT INTO tasks(title, source, created_at, updated_at) VALUES ('직접 쓴 할 일', 'manual', '2026-10-07T09:00:00', 'x')", [])?;
            c.execute("INSERT INTO duty_logs(content, source, created_at, updated_at) VALUES ('업무', 'ai', '2026-10-07T09:00:00', 'x')", [])?;
            c.execute("INSERT INTO observations(area, content, observed_on, source, created_at, updated_at) VALUES ('subject', '발표', '2026-10-07', 'ai', '2026-10-07T09:00:00', 'x')", [])?;
            let t1 = util::parse_dt("2026-10-07T10:00:00").unwrap();
            assert_eq!(counts(c, t1)?, NewCounts { records: 1, duties: 1, tasks: 1, students: 0 });
            // 할 일 메뉴를 열면 그 숫자만 사라지고, 이번 방문의 기준(전 시각)을 돌려준다
            assert_eq!(mark_seen(c, "tasks", t1)?.as_deref(), Some("2026-10-07T08:00:00"));
            assert_eq!(counts(c, t1)?, NewCounts { records: 1, duties: 1, tasks: 0, students: 0 });
            assert_eq!(new_since(c, "2026-10-07T08:00:00")?.observations.len(), 1);
            assert!(mark_seen(c, "없는메뉴", t1).is_err());
            Ok(())
        })
        .unwrap();
    }
}
