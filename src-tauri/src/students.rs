//! 학생·학급

use crate::analysis::{self, RosterEntry};
use crate::classify::PlanStudent;
use crate::error::{AppError, AppResult};
use crate::util;
use chrono::{NaiveDate, NaiveDateTime};
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
    pub phone: String,
    pub guardian_phone: String,
    pub address: String,
    pub gender: String,
    pub birthday: Option<String>,
    /// 나이스 명렬표에서 가져온 항목 (주소·우편번호·주민등록번호는 학생 상세에서만 보인다)
    pub student_no: String,
    pub postal_code: String,
    pub prev_record: String,
    pub remark: String,
    pub rrn: String,
    /// 보호자 연락처 (관계 + 번호, 최대 4개)
    pub contacts: Vec<crate::profile::Contact>,
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

/// 생일 입력을 정리한다: "2012-03-05", "2012.3.5", "20120305" → "2012-03-05" / "3/5", "03-05", "3월 5일", "0305" → "03-05"
pub fn normalize_birthday(s: &str) -> AppResult<Option<String>> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let digits: Vec<u32> = t
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u32>().unwrap_or(0))
        .collect();
    let bad = || AppError::invalid(format!("생일 형식을 이해하지 못했습니다: {t} (예: 2012-03-05 또는 3월 5일)"));
    let (y, m, d) = match digits.as_slice() {
        [ymd] if t.len() == 8 => (Some(ymd / 10000), (ymd / 100) % 100, ymd % 100),
        [md] if t.len() == 4 => (None, md / 100, md % 100),
        [y, m, d] if *y >= 1900 => (Some(*y), *m, *d),
        [m, d] => (None, *m, *d),
        _ => return Err(bad()),
    };
    // 연도 없는 생일은 윤년(2000)으로 확인해 2월 29일도 받는다
    let check_year = y.unwrap_or(2000) as i32;
    if chrono::NaiveDate::from_ymd_opt(check_year, m, d).is_none() {
        return Err(bad());
    }
    Ok(Some(match y {
        Some(y) => format!("{y:04}-{m:02}-{d:02}"),
        None => format!("{m:02}-{d:02}"),
    }))
}

/// 명렬표 칸 하나를 고친다 (허용된 항목만)
pub fn set_roster_field(conn: &Connection, id: i64, field: &str, value: &str, now: NaiveDateTime) -> AppResult<()> {
    let v = value.trim();
    let now_s = util::fmt_dt(now);
    let n = match field {
        "name" => conn.execute("UPDATE students SET name=?2, updated_at=?3 WHERE id=?1", params![id, validate_name(v)?, now_s])?,
        "class_label" => {
            let class_id = if v.is_empty() { None } else { Some(find_or_create_class(conn, v, active_school_year(conn, now.date()))?) };
            conn.execute("UPDATE students SET class_id=?2, updated_at=?3 WHERE id=?1", params![id, class_id, now_s])?
        }
        "number" => {
            let num: Option<i64> = if v.is_empty() {
                None
            } else {
                Some(v.parse().map_err(|_| AppError::invalid("번호는 숫자로 적어 주세요."))?)
            };
            conn.execute("UPDATE students SET number=?2, updated_at=?3 WHERE id=?1", params![id, num, now_s])?
        }
        "gender" => {
            let g = match v {
                "" => "",
                "남" | "남자" | "M" | "m" => "남",
                "여" | "여자" | "F" | "f" => "여",
                _ => return Err(AppError::invalid("성별은 '남' 또는 '여'로 적어 주세요.")),
            };
            conn.execute("UPDATE students SET gender=?2, updated_at=?3 WHERE id=?1", params![id, g, now_s])?
        }
        "birthday" => conn.execute("UPDATE students SET birthday=?2, updated_at=?3 WHERE id=?1", params![id, normalize_birthday(v)?, now_s])?,
        // 주민등록번호는 지우기만 할 수 있다 (가져오기 때 설정을 켠 경우에만 채워진다)
        "rrn" if v.is_empty() => conn.execute("UPDATE students SET rrn='', updated_at=?2 WHERE id=?1", params![id, now_s])?,
        "student_no" | "postal_code" if !v.chars().all(|c| c.is_ascii_digit()) => {
            return Err(AppError::invalid("숫자만 적어 주세요."));
        }
        "phone" | "guardian_phone" | "address" | "memo" | "student_no" | "postal_code" | "prev_record" | "remark" => {
            let limit = match field {
                "memo" => 2000,
                "address" | "remark" => 200,
                "student_no" | "postal_code" => 12,
                _ => 40,
            };
            if v.chars().count() > limit {
                return Err(AppError::invalid(format!("{limit}자 이내로 적어 주세요.")));
            }
            conn.execute(&format!("UPDATE students SET {field}=?2, updated_at=?3 WHERE id=?1"), params![id, v, now_s])?
        }
        _ => return Err(AppError::invalid("고칠 수 없는 항목입니다.")),
    };
    if n == 0 {
        return Err(AppError::not_found("학생을 찾을 수 없습니다."));
    }
    // 예전 '보호자 연락처' 한 칸을 고치면 첫 연락처로 (나이스 명렬표 가져오기 등)
    if field == "guardian_phone" {
        let first: Option<String> = conn
            .query_row("SELECT phone FROM student_contacts WHERE student_id=?1 ORDER BY pos, id LIMIT 1", params![id], |r| r.get(0))
            .optional()?;
        if first.as_deref().map(str::trim) != Some(v) {
            crate::profile::set_first_contact(conn, id, v, now)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Birthday {
    pub student_id: i64,
    pub label: String,
    pub date: String,
}

/// 기간 안에 드는 학생 생일 (해마다 반복, 2월 29일생은 평년에 2월 28일)
pub fn birthdays_in_range(conn: &Connection, start: chrono::NaiveDate, end: chrono::NaiveDate) -> AppResult<Vec<Birthday>> {
    use chrono::Datelike;
    let mut st = conn.prepare(
        "SELECT s.id, s.name, c.label, s.birthday FROM students s LEFT JOIN classes c ON c.id=s.class_id
         WHERE s.archived=0 AND s.birthday IS NOT NULL AND s.birthday <> ''",
    )?;
    let rows: Vec<(i64, String, Option<String>, String)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, name, label, bday) in rows {
        let md = if bday.len() == 10 { &bday[5..] } else { bday.as_str() };
        let (Some(m), Some(d)) = (md.get(0..2).and_then(|x| x.parse::<u32>().ok()), md.get(3..5).and_then(|x| x.parse::<u32>().ok())) else {
            continue;
        };
        for y in start.year()..=end.year() {
            let date = chrono::NaiveDate::from_ymd_opt(y, m, d).or_else(|| if m == 2 && d == 29 { chrono::NaiveDate::from_ymd_opt(y, 2, 28) } else { None });
            if let Some(date) = date.filter(|x| *x >= start && *x <= end) {
                out.push(Birthday {
                    student_id: id,
                    label: match &label {
                        Some(l) => format!("{l} {name}"),
                        None => name.clone(),
                    },
                    date: util::fmt_d(date),
                });
            }
        }
    }
    out.sort_by(|a, b| a.date.cmp(&b.date).then(a.label.cmp(&b.label)));
    Ok(out)
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

/// 지금 쓰는 학년도. 달력 기준 학년도와, 2월에 '새 학년도 시작'으로 한 해 앞당긴 학년도 중 큰 것.
/// (2월 말에 새 학년도를 시작하고 명렬표를 넣으면 새 학년도 학급으로 들어가게 한다)
pub fn active_school_year(conn: &Connection, today: NaiveDate) -> i32 {
    let cur = util::school_year(today);
    let started = crate::db::get_setting(conn, "school_year_started").ok().flatten().and_then(|s| s.trim().parse::<i32>().ok());
    match started {
        Some(s) if s == cur + 1 => s,
        _ => cur,
    }
}

/// 학급 목록. 지난 학년도 학급은 보관 처리하지 않은 학생이 있을 때만 보인다 (새 학년도 시작 뒤 목록 정리).
pub fn list_classes(conn: &Connection) -> AppResult<Vec<ClassDto>> {
    let current = active_school_year(conn, util::now().date());
    let mut st = conn.prepare(
        "SELECT c.id, c.school_year, c.label, (SELECT COUNT(*) FROM students s WHERE s.class_id=c.id AND s.archived=0)
         FROM classes c
         WHERE c.school_year >= ?1 OR EXISTS (SELECT 1 FROM students s WHERE s.class_id=c.id AND s.archived=0)
         ORDER BY c.school_year DESC, c.grade, c.class_no",
    )?;
    let rows = st
        .query_map(params![current], |r| Ok(ClassDto { id: r.get(0)?, school_year: r.get(1)?, label: r.get(2)?, student_count: r.get(3)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 분류에 쓰는 학생 명단 (이름·학급·번호만. 주소·생년월일 같은 정보는 넣지 않는다)
///
/// 끝에 이름이 빈 '학급만' 항목을 붙여 이 학교의 학급 구성을 알린다. 학년마다 반이 하나면
/// "1학년 홍길동"를 1-1 로 본다. 학급은 확인된 학생(명렬표·교사가 등록)이 있는 학급, 시간표의 학급, 나이스 학급 목록에서 찾는다.
pub fn roster(conn: &Connection) -> AppResult<Vec<RosterEntry>> {
    let mut st = conn.prepare(
        "SELECT s.id, s.name, c.label, s.number FROM students s LEFT JOIN classes c ON c.id=s.class_id WHERE s.archived=0",
    )?;
    let mut rows = st
        .query_map([], |r| Ok(RosterEntry { id: r.get(0)?, name: r.get(1)?, class_label: r.get(2)?, number: r.get(3)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    let year = active_school_year(conn, util::now().date());
    let mut labels: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut st = conn.prepare(
        "SELECT DISTINCT c.label FROM classes c JOIN students s ON s.class_id=c.id
         WHERE c.school_year=?1 AND s.archived=0 AND s.needs_review=0",
    )?;
    for l in st.query_map(params![year], |r| r.get::<_, String>(0))? {
        labels.insert(l?);
    }
    let mut st = conn.prepare(
        "SELECT DISTINCT tc.class_label FROM timetable_cells tc JOIN timetables t ON t.id=tc.timetable_id WHERE t.school_year=?1 AND tc.class_label<>''
         UNION SELECT DISTINCT class_label FROM timetables WHERE school_year=?1 AND kind='class' AND class_label<>''",
    )?;
    for l in st.query_map(params![year], |r| r.get::<_, String>(0))? {
        if let Some((g, n)) = parse_class_label(&l?) {
            labels.insert(format!("{g}-{n}"));
        }
    }
    // 나이스에서 받은 학교의 학급 목록 (학년마다 반이 몇 개인지)
    for l in crate::neis::saved_classes(conn) {
        if let Some((g, n)) = parse_class_label(&l) {
            labels.insert(format!("{g}-{n}"));
        }
    }
    rows.extend(labels.into_iter().map(|l| RosterEntry { id: 0, name: String::new(), class_label: Some(l), number: None }));
    Ok(rows)
}

const STUDENT_SELECT: &str = "SELECT s.id, s.name, s.class_id, c.label, s.number, s.memo, s.needs_review,
    (SELECT COUNT(*) FROM note_students ns WHERE ns.student_id=s.id),
    (SELECT COUNT(*) FROM observations o WHERE o.student_id=s.id),
    (SELECT MAX(n.created_at) FROM note_students ns JOIN notes n ON n.id=ns.note_id WHERE ns.student_id=s.id),
    s.phone, s.guardian_phone, s.address, s.gender, s.birthday,
    s.student_no, s.postal_code, s.prev_record, s.remark, s.rrn
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
        phone: r.get(10)?,
        guardian_phone: r.get(11)?,
        address: r.get(12)?,
        gender: r.get(13)?,
        birthday: r.get(14)?,
        student_no: r.get(15)?,
        postal_code: r.get(16)?,
        prev_record: r.get(17)?,
        remark: r.get(18)?,
        rrn: r.get(19)?,
        contacts: vec![],
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
    let mut contacts = crate::profile::all_contacts(conn)?;
    for s in rows.iter_mut() {
        fill_area_counts(conn, s)?;
        s.contacts = contacts.remove(&s.id).unwrap_or_default();
    }
    Ok(rows)
}

pub fn get_student(conn: &Connection, id: i64) -> AppResult<StudentDto> {
    let mut s = conn
        .query_row(&format!("{STUDENT_SELECT} WHERE s.id=?1"), params![id], map_student)
        .optional()?
        .ok_or_else(|| AppError::not_found("학생을 찾을 수 없습니다."))?;
    fill_area_counts(conn, &mut s)?;
    s.contacts = crate::profile::contacts(conn, s.id)?;
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
        Some(l) => Some(find_or_create_class(conn, l, active_school_year(conn, now.date()))?),
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
        Some(l) => Some(find_or_create_class(conn, l, active_school_year(conn, now.date()))?),
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

/// 명단 붙여넣기: 한 줄에 한 명 ("2-1 3 홍길동", "2학년 1반 3번 홍길동", 표에서 복사한 "2<TAB>1<TAB>3<TAB>홍길동")
pub fn import_roster(conn: &mut Connection, text: &str, now: NaiveDateTime) -> AppResult<ImportResult> {
    let tx = conn.transaction()?;
    let mut res = ImportResult { added: 0, skipped: 0, problems: vec![] };
    let year = active_school_year(&tx, now.date());
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
        Some(l) if parse_class_label(l).is_some() => Some(find_or_create_class(conn, l, active_school_year(conn, now.date()))?),
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
        let r = import_roster(&mut c, "2-1 3 홍길동\n2학년 3반 5번 홍길동\n2\t1\t7\t성춘향\n이상한 줄\n2-1 3 홍길동", now()).unwrap();
        assert_eq!(r.added, 3);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.problems.len(), 1);
        let roster = roster(&c).unwrap();
        assert_eq!(roster.iter().filter(|r| !r.name.is_empty()).count(), 3);
        // 학급 구성 항목 (이름 없음): 2-1, 2-3
        assert_eq!(roster.iter().filter(|r| r.name.is_empty()).count(), 2);
        // 학급이 있으면 동명이인 중 한 명으로 정해진다
        let ps = PlanStudent { student_id: None, name: "홍길동".into(), class_label: Some("2-3".into()), number: None, candidates: vec![] };
        let id = resolve_plan_student(&c, &ps, now()).unwrap().unwrap();
        assert_eq!(student_label(&c, id).unwrap(), "2-3 홍길동");
        // 학급이 없으면 정하지 않는다
        let ps = PlanStudent { student_id: None, name: "홍길동".into(), class_label: None, number: None, candidates: vec![] };
        assert!(resolve_plan_student(&c, &ps, now()).unwrap().is_none());
        // 새 학생은 확인 필요로 등록
        let ps = PlanStudent { student_id: None, name: "이몽룡".into(), class_label: Some("1-2".into()), number: None, candidates: vec![] };
        let nid = resolve_plan_student(&c, &ps, now()).unwrap().unwrap();
        assert!(get_student(&c, nid).unwrap().needs_review);
        // 사람 이름 같지 않은 단어는 학생으로 만들지 않는다
        let ps = PlanStudent { student_id: None, name: "체육대회".into(), class_label: None, number: None, candidates: vec![] };
        assert!(resolve_plan_student(&c, &ps, now()).unwrap().is_none());
    }

    #[test]
    fn birthday_formats() {
        assert_eq!(normalize_birthday("2012-03-05").unwrap().as_deref(), Some("2012-03-05"));
        assert_eq!(normalize_birthday("2012.3.5").unwrap().as_deref(), Some("2012-03-05"));
        assert_eq!(normalize_birthday("20120305").unwrap().as_deref(), Some("2012-03-05"));
        assert_eq!(normalize_birthday("3월 5일").unwrap().as_deref(), Some("03-05"));
        assert_eq!(normalize_birthday("3/5").unwrap().as_deref(), Some("03-05"));
        assert_eq!(normalize_birthday("0229").unwrap().as_deref(), Some("02-29"));
        assert_eq!(normalize_birthday("").unwrap(), None);
        assert!(normalize_birthday("2월 30일").is_err());
        assert!(normalize_birthday("생일").is_err());
    }

    #[test]
    fn roster_fields_and_birthdays() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let a = create_student(&c, &StudentInput { name: "홍길동".into(), class_label: Some("2-1".into()), number: Some(3), memo: None }, now()).unwrap();
        let b = create_student(&c, &StudentInput { name: "성춘향".into(), class_label: Some("2-1".into()), number: Some(7), memo: None }, now()).unwrap();
        set_roster_field(&c, a, "phone", "010-1234-5678", now()).unwrap();
        set_roster_field(&c, a, "gender", "남자", now()).unwrap();
        set_roster_field(&c, a, "birthday", "2012.10.3", now()).unwrap();
        set_roster_field(&c, b, "birthday", "2월 29일", now()).unwrap();
        set_roster_field(&c, b, "class_label", "2-3", now()).unwrap();
        assert!(set_roster_field(&c, a, "gender", "모름", now()).is_err());
        assert!(set_roster_field(&c, a, "needs_review", "1", now()).is_err(), "허용되지 않은 항목");
        let s = get_student(&c, a).unwrap();
        assert_eq!((s.phone.as_str(), s.gender.as_str(), s.birthday.as_deref()), ("010-1234-5678", "남", Some("2012-10-03")));
        assert_eq!(get_student(&c, b).unwrap().class_label.as_deref(), Some("2-3"));
        let d = |s: &str| util::parse_d(s).unwrap();
        let bd = birthdays_in_range(&c, d("2026-09-01"), d("2027-03-31")).unwrap();
        let got: Vec<(String, String)> = bd.into_iter().map(|b| (b.label, b.date)).collect();
        assert_eq!(got, vec![("2-1 홍길동".to_string(), "2026-10-03".to_string()), ("2-3 성춘향".to_string(), "2027-02-28".to_string())]);
    }

    #[test]
    fn merge_moves_records() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let a = create_student(&c, &StudentInput { name: "홍길동".into(), class_label: Some("2-1".into()), number: Some(3), memo: None }, now()).unwrap();
        let b = create_student(&c, &StudentInput { name: "홍길동".into(), class_label: None, number: None, memo: None }, now()).unwrap();
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
