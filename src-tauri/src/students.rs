//! 학생·학급

use crate::analysis::{self, RosterEntry};
use crate::classify::PlanStudent;
use crate::error::{AppError, AppResult};
use crate::util;
use chrono::NaiveDateTime;
use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct ClassDto {
    pub id: i64,
    pub school_year: i64,
    pub label: String,
    pub student_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StudentDto {
    pub id: i64,
    pub name: String,
    pub class_id: Option<i64>,
    pub class_label: Option<String>,
    pub number: Option<i64>,
    pub memo: String,
    pub needs_review: bool,
    pub note_count: i64,
    pub observation_count: i64,
    pub last_noted_at: Option<String>,
    pub area_counts: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StudentInput {
    pub name: String,
    #[serde(default)]
    pub class_label: Option<String>,
    #[serde(default)]
    pub number: Option<i64>,
    #[serde(default)]
    pub memo: Option<String>,
}

static RE_LABEL: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(\d)\s*(?:-|학년\s*)\s*(\d{1,2})\s*반?\s*$").unwrap());

/// "2-1", "2학년 1반" → (2, 1)
pub fn parse_class_label(s: &str) -> Option<(i64, i64)> {
    let c = RE_LABEL.captures(s)?;
    let g: i64 = c[1].parse().ok()?;
    let n: i64 = c[2].parse().ok()?;
    if (1..=6).contains(&g) && (1..=30).contains(&n) { Some((g, n)) } else { None }
}

pub fn find_or_create_class(conn: &Connection, label: &str, school_year: i32) -> AppResult<i64> {
    let (g, n) = parse_class_label(label).ok_or_else(|| AppError::invalid(format!("학급 표기를 이해하지 못했습니다: {label} (예: 2-1)")))?;
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM classes WHERE school_year=?1 AND grade=?2 AND class_no=?3",
            params![school_year, g, n],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO classes(school_year, grade, class_no, label) VALUES (?1, ?2, ?3, ?4)",
        params![school_year, g, n, format!("{g}-{n}")],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_classes(conn: &Connection) -> AppResult<Vec<ClassDto>> {
    let mut st = conn.prepare(
        "SELECT c.id, c.school_year, c.label, (SELECT COUNT(*) FROM students s WHERE s.class_id=c.id AND s.archived=0)
         FROM classes c ORDER BY c.school_year DESC, c.grade, c.class_no",
    )?;
    let rows = st
        .query_map([], |r| Ok(ClassDto { id: r.get(0)?, school_year: r.get(1)?, label: r.get(2)?, student_count: r.get(3)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 분류에 쓰는 학생 명단
pub fn roster(conn: &Connection) -> AppResult<Vec<RosterEntry>> {
    let mut st = conn.prepare(
        "SELECT s.id, s.name, c.label, s.number FROM students s LEFT JOIN classes c ON c.id=s.class_id WHERE s.archived=0",
    )?;
    let rows = st
        .query_map([], |r| Ok(RosterEntry { id: r.get(0)?, name: r.get(1)?, class_label: r.get(2)?, number: r.get(3)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

const STUDENT_SELECT: &str = "SELECT s.id, s.name, s.class_id, c.label, s.number, s.memo, s.needs_review,
    (SELECT COUNT(*) FROM note_students ns WHERE ns.student_id=s.id),
    (SELECT COUNT(*) FROM observations o WHERE o.student_id=s.id),
    (SELECT MAX(n.created_at) FROM note_students ns JOIN notes n ON n.id=ns.note_id WHERE ns.student_id=s.id)
    FROM students s LEFT JOIN classes c ON c.id=s.class_id";

fn map_student(r: &rusqlite::Row) -> rusqlite::Result<StudentDto> {
    Ok(StudentDto {
        id: r.get(0)?,
        name: r.get(1)?,
        class_id: r.get(2)?,
        class_label: r.get(3)?,
        number: r.get(4)?,
        memo: r.get(5)?,
        needs_review: r.get::<_, i64>(6)? != 0,
        note_count: r.get(7)?,
        observation_count: r.get(8)?,
        last_noted_at: r.get(9)?,
        area_counts: BTreeMap::new(),
    })
}

fn fill_area_counts(conn: &Connection, s: &mut StudentDto) -> AppResult<()> {
    let mut st = conn.prepare("SELECT area, COUNT(*) FROM observations WHERE student_id=?1 GROUP BY area")?;
    let rows = st.query_map(params![s.id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (a, n) = row?;
        s.area_counts.insert(a, n);
    }
    Ok(())
}

pub fn list_students(conn: &Connection, query: Option<&str>, class_id: Option<i64>) -> AppResult<Vec<StudentDto>> {
    let mut sql = format!("{STUDENT_SELECT} WHERE s.archived=0");
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
        args.push(Box::new(format!("%{q}%")));
        sql.push_str(&format!(" AND (s.name LIKE ?{0} OR c.label LIKE ?{0})", args.len()));
    }
    if let Some(cid) = class_id {
        args.push(Box::new(cid));
        sql.push_str(&format!(" AND s.class_id = ?{}", args.len()));
    }
    sql.push_str(" ORDER BY c.school_year DESC, c.grade IS NULL, c.grade, c.class_no, s.number IS NULL, s.number, s.name");
    let mut st = conn.prepare(&sql)?;
    let mut rows = st
        .query_map(rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())), map_student)?
        .collect::<Result<Vec<_>, _>>()?;
    for s in rows.iter_mut() {
        fill_area_counts(conn, s)?;
    }
    Ok(rows)
}

pub fn get_student(conn: &Connection, id: i64) -> AppResult<StudentDto> {
    let mut s = conn
        .query_row(&format!("{STUDENT_SELECT} WHERE s.id=?1"), params![id], map_student)
        .optional()?
        .ok_or_else(|| AppError::not_found("학생을 찾을 수 없습니다."))?;
    fill_area_counts(conn, &mut s)?;
    Ok(s)
}

fn validate_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 20 {
        return Err(AppError::invalid("학생 이름을 1~20자로 입력해 주세요."));
    }
    Ok(n.to_string())
}

pub fn create_student(conn: &Connection, input: &StudentInput, now: NaiveDateTime) -> AppResult<i64> {
    let name = validate_name(&input.name)?;
    let class_id = match input.class_label.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(l) => Some(find_or_create_class(conn, l, util::school_year(now.date()))?),
        None => None,
    };
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO students(class_id, number, name, memo, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![class_id, input.number, name, input.memo.clone().unwrap_or_default(), now_s],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_student(conn: &Connection, id: i64, input: &StudentInput, now: NaiveDateTime) -> AppResult<()> {
    let name = validate_name(&input.name)?;
    let class_id = match input.class_label.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(l) => Some(find_or_create_class(conn, l, util::school_year(now.date()))?),
        None => None,
    };
    let n = conn.execute(
        "UPDATE students SET name=?2, class_id=?3, number=?4, memo=?5, needs_review=0, updated_at=?6 WHERE id=?1",
        params![id, name, class_id, input.number, input.memo.clone().unwrap_or_default(), util::fmt_dt(now)],
    )?;
    if n == 0 {
        return Err(AppError::not_found("학생을 찾을 수 없습니다."));
    }
    Ok(())
}

pub fn delete_student(conn: &mut Connection, id: i64) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM students WHERE id=?1", params![id])?;
    tx.commit()?;
    Ok(())
}

/// 같은 학생으로 합치기 (자동 등록된 학생을 기존 학생에 합칠 때)
pub fn merge_students(conn: &mut Connection, from_id: i64, into_id: i64) -> AppResult<()> {
    if from_id == into_id {
        return Ok(());
    }
    let tx = conn.transaction()?;
    tx.execute("UPDATE observations SET student_id=?2 WHERE student_id=?1", params![from_id, into_id])?;
    tx.execute("UPDATE OR IGNORE note_students SET student_id=?2 WHERE student_id=?1", params![from_id, into_id])?;
    tx.execute("DELETE FROM note_students WHERE student_id=?1", params![from_id])?;
    tx.execute("UPDATE record_drafts SET student_id=?2 WHERE student_id=?1", params![from_id, into_id])?;
    tx.execute("DELETE FROM students WHERE id=?1", params![from_id])?;
    tx.commit()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub added: usize,
    pub skipped: usize,
    pub problems: Vec<String>,
}

static RE_ROSTER_LINE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(\d)\s*(?:-|학년|\t|\s)\s*(\d{1,2})\s*반?\s*(?:[\t\s,]+(\d{1,2})\s*번?)?\s*[\t\s,]+([가-힣A-Za-z]{2,10})\s*$").unwrap()
});

/// 명단 붙여넣기: 한 줄에 한 명 ("2-1 3 김민수", "2학년 1반 3번 김민수", 표에서 복사한 "2<TAB>1<TAB>3<TAB>김민수")
pub fn import_roster(conn: &mut Connection, text: &str, now: NaiveDateTime) -> AppResult<ImportResult> {
    let tx = conn.transaction()?;
    let mut res = ImportResult { added: 0, skipped: 0, problems: vec![] };
    let year = util::school_year(now.date());
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Some(c) = RE_ROSTER_LINE.captures(line) else {
            res.problems.push(format!("{}번째 줄을 이해하지 못했습니다: {}", i + 1, line.trim()));
            continue;
        };
        let label = format!("{}-{}", &c[1], &c[2]);
        let number: Option<i64> = c.get(3).and_then(|m| m.as_str().parse().ok());
        let name = c[4].to_string();
        let class_id = find_or_create_class(&tx, &label, year)?;
        let exists: Option<i64> = tx
            .query_row(
                "SELECT id FROM students WHERE class_id=?1 AND name=?2 AND IFNULL(number,-1)=IFNULL(?3,-1)",
                params![class_id, name, number],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_some() {
            res.skipped += 1;
            continue;
        }
        let now_s = util::fmt_dt(now);
        tx.execute(
            "INSERT INTO students(class_id, number, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![class_id, number, name, now_s],
        )?;
        res.added += 1;
    }
    tx.commit()?;
    Ok(res)
}

/// 분류 계획의 학생을 실제 학생 id 로 바꾼다. 동명이인처럼 정할 수 없으면 None.
pub fn resolve_plan_student(conn: &Connection, ps: &PlanStudent, now: NaiveDateTime) -> AppResult<Option<i64>> {
    if let Some(id) = ps.student_id {
        let ok: Option<i64> = conn.query_row("SELECT id FROM students WHERE id=?1", params![id], |r| r.get(0)).optional()?;
        if ok.is_some() {
            return Ok(Some(id));
        }
    }
    if !ps.candidates.is_empty() {
        return Ok(None);
    }
    let name = ps.name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    let class_id = match ps.class_label.as_deref() {
        Some(l) if parse_class_label(l).is_some() => Some(find_or_create_class(conn, l, util::school_year(now.date()))?),
        _ => None,
    };
    // 같은 학급·이름(·번호)의 학생이 이미 있으면 그 학생
    let existing: Vec<i64> = {
        let mut st = conn.prepare(
            "SELECT id FROM students WHERE name=?1 AND archived=0 AND (?2 IS NULL OR class_id=?2) AND (?3 IS NULL OR number IS NULL OR number=?3)",
        )?;
        st.query_map(params![name, class_id, ps.number], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    match existing.len() {
        1 => return Ok(existing.first().copied()),
        n if n > 1 => return Ok(None),
        _ => {}
    }
    if class_id.is_none() && !analysis::plausible_full_name(name) {
        return Ok(None);
    }
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO students(class_id, number, name, needs_review, created_at, updated_at) VALUES (?1, ?2, ?3, 1, ?4, ?4)",
        params![class_id, ps.number, name, now_s],
    )?;
    Ok(Some(conn.last_insert_rowid()))
}

pub fn student_label(conn: &Connection, id: i64) -> AppResult<String> {
    let (name, label): (String, Option<String>) = conn.query_row(
        "SELECT s.name, c.label FROM students s LEFT JOIN classes c ON c.id=s.class_id WHERE s.id=?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(match label {
        Some(l) => format!("{l} {name}"),
        None => name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn now() -> NaiveDateTime {
        util::parse_dt("2026-09-29T09:00:00").unwrap()
    }

    #[test]
    fn roster_import_and_homonyms() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let r = import_roster(&mut c, "2-1 3 김민수\n2학년 3반 5번 김민수\n2\t1\t7\t이서연\n이상한 줄\n2-1 3 김민수", now()).unwrap();
        assert_eq!(r.added, 3);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.problems.len(), 1);
        let roster = roster(&c).unwrap();
        assert_eq!(roster.len(), 3);
        // 학급이 있으면 동명이인 중 한 명으로 정해진다
        let ps = PlanStudent { student_id: None, name: "김민수".into(), class_label: Some("2-3".into()), number: None, candidates: vec![] };
        let id = resolve_plan_student(&c, &ps, now()).unwrap().unwrap();
        assert_eq!(student_label(&c, id).unwrap(), "2-3 김민수");
        // 학급이 없으면 정하지 않는다
        let ps = PlanStudent { student_id: None, name: "김민수".into(), class_label: None, number: None, candidates: vec![] };
        assert!(resolve_plan_student(&c, &ps, now()).unwrap().is_none());
        // 새 학생은 확인 필요로 등록
        let ps = PlanStudent { student_id: None, name: "박지훈".into(), class_label: Some("1-2".into()), number: None, candidates: vec![] };
        let nid = resolve_plan_student(&c, &ps, now()).unwrap().unwrap();
        assert!(get_student(&c, nid).unwrap().needs_review);
        // 사람 이름 같지 않은 단어는 학생으로 만들지 않는다
        let ps = PlanStudent { student_id: None, name: "체육대회".into(), class_label: None, number: None, candidates: vec![] };
        assert!(resolve_plan_student(&c, &ps, now()).unwrap().is_none());
    }

    #[test]
    fn merge_moves_records() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let a = create_student(&c, &StudentInput { name: "김민수".into(), class_label: Some("2-1".into()), number: Some(3), memo: None }, now()).unwrap();
        let b = create_student(&c, &StudentInput { name: "김민수".into(), class_label: None, number: None, memo: None }, now()).unwrap();
        c.execute(
            "INSERT INTO observations(student_id, area, content, observed_on, source, created_at, updated_at) VALUES (?1, 'subject', '발표', '2026-09-29', 'ai', 'x', 'x')",
            params![b],
        )
        .unwrap();
        merge_students(&mut c, b, a).unwrap();
        assert_eq!(get_student(&c, a).unwrap().observation_count, 1);
        assert!(get_student(&c, b).is_err());
    }
}
