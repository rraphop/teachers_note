//! 데이터 내보내기: 엑셀(.xlsx) 한 파일에 표마다 시트 하나, 또는 표마다 CSV 파일 하나.
//!
//! 보관·열람·인쇄용이다. 되돌리기(복원)는 백업 파일(.db)이 맡는다.
//! 내보낸 파일에는 암호가 없으므로 저장 위치는 교사가 직접 고르고, 화면에서 개인정보 경고를 보여 준다.
//! 기술 로그에는 파일 형식과 표 개수만 남기고 내용·경로는 남기지 않는다.

use crate::analysis::Area;
use crate::error::{AppError, AppResult};
use crate::settings::RosterColumns;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sheet {
    Roster,
    Notes,
    Observations,
    Drafts,
    Tasks,
    Events,
    Duties,
    DutyLogs,
    Timetable,
}

impl Sheet {
    pub const ALL: [Sheet; 9] = [
        Sheet::Roster,
        Sheet::Notes,
        Sheet::Observations,
        Sheet::Drafts,
        Sheet::Tasks,
        Sheet::Events,
        Sheet::Duties,
        Sheet::DutyLogs,
        Sheet::Timetable,
    ];

    /// 시트 이름이자 CSV 파일 이름
    pub fn title(&self) -> &'static str {
        match self {
            Sheet::Roster => "학급명렬표",
            Sheet::Notes => "기록 원문",
            Sheet::Observations => "학생 관찰",
            Sheet::Drafts => "학생부 초안",
            Sheet::Tasks => "할 일",
            Sheet::Events => "일정·학사력",
            Sheet::Duties => "담당업무",
            Sheet::DutyLogs => "업무 기록",
            Sheet::Timetable => "시간표",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Xlsx,
    Csv,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportOptions {
    pub format: Format,
    /// xlsx: 저장할 파일 경로 / csv: 내보내기 폴더를 만들 상위 폴더
    pub path: String,
    pub sheets: Vec<Sheet>,
    /// 명렬표·학생 관찰·학생부 초안을 이 학급으로 좁힌다
    #[serde(default)]
    pub class_id: Option<i64>,
    /// 전화번호·보호자 연락처·주소를 가린다
    #[serde(default)]
    pub mask_contacts: bool,
    /// 명렬표 선택 항목 (없으면 모든 항목)
    #[serde(default)]
    pub roster_columns: Option<RosterColumns>,
    /// 이 학년도(3월 1일 ~ 다음 해 2월 말) 자료만 — 학년도 마감 기록물용. 보관 처리한 학생도 포함한다.
    #[serde(default)]
    pub school_year: Option<i32>,
    /// 명렬표를 이 학생들로 좁힌다 (학생 명렬표에서 찾은 결과)
    #[serde(default)]
    pub student_ids: Option<Vec<i64>>,
}

/// 학년도 범위 [시작, 끝) — 'YYYY-MM-DD…' 문자열 비교용
pub fn year_range(y: Option<i32>) -> (Option<String>, Option<String>) {
    match y {
        Some(y) => (Some(format!("{y}-03-01")), Some(format!("{}-03-01", y + 1))),
        None => (None, None),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SheetCount {
    pub title: String,
    pub rows: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportResult {
    /// 만든 파일(xlsx) 또는 폴더(csv)
    pub path: String,
    pub files: Vec<String>,
    pub sheets: Vec<SheetCount>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Int(i64),
}

impl Value {
    fn text(s: impl Into<String>) -> Value {
        Value::Text(s.into())
    }
    fn opt_int(n: Option<i64>) -> Value {
        n.map(Value::Int).unwrap_or_else(|| Value::text(""))
    }
    fn as_csv(&self) -> String {
        match self {
            Value::Text(s) => csv_field(s),
            Value::Int(n) => n.to_string(),
        }
    }
}

pub struct Table {
    pub sheet: Sheet,
    /// (머리글, 엑셀 열 너비, 줄바꿈 여부)
    pub columns: Vec<(&'static str, f64, bool)>,
    pub rows: Vec<Vec<Value>>,
}

// ───── 값 다듬기 ─────

fn dt_text(s: &str) -> String {
    // 'YYYY-MM-DDTHH:MM:SS' → 'YYYY-MM-DD HH:MM'
    let s = s.replace('T', " ");
    if s.len() >= 16 { s[..16].to_string() } else { s }
}

fn source_text(source: &str, edited: bool) -> &'static str {
    match (source, edited) {
        ("ai", true) => "AI(교사 수정)",
        ("ai", false) => "AI",
        _ => "직접",
    }
}

fn yes(b: bool) -> &'static str {
    if b { "예" } else { "" }
}

fn status_text(s: &str) -> &'static str {
    match s {
        "pending" => "분류 대기",
        "processing" => "분류 중",
        "classified" => "AI 분류",
        "manual" => "직접 분류",
        _ => "미분류",
    }
}

fn layer_text(s: &str) -> &'static str {
    match s {
        "annual" => "학사력",
        "monthly" => "월간 계획",
        "weekly" => "주간 계획",
        _ => "기록에서 찾은 일정",
    }
}

fn weekday_text(w: i64) -> &'static str {
    match w {
        1 => "월",
        2 => "화",
        3 => "수",
        4 => "목",
        5 => "금",
        6 => "토",
        _ => "",
    }
}

fn period_text(p: i64) -> String {
    match p {
        0..=10 => format!("{p}교시"),
        n if n > 100 => format!("방과후 {}", n - 100),
        n => n.to_string(),
    }
}

fn birthday_text(b: &str) -> String {
    // 'YYYY-MM-DD' 그대로, 'MM-DD' 는 '3월 5일'
    if b.len() == 5 {
        let mut it = b.split('-').filter_map(|x| x.parse::<u32>().ok());
        if let (Some(m), Some(d)) = (it.next(), it.next()) {
            return format!("{m}월 {d}일");
        }
    }
    b.to_string()
}

/// 명렬표 화면의 '연락처·주소 가리기'와 같은 방식
pub fn mask_contact(v: &str, address: bool) -> String {
    let v = v.trim();
    if v.is_empty() {
        return String::new();
    }
    if address {
        let head: Vec<&str> = v.split_whitespace().take(2).collect();
        return format!("{} ●●●", head.join(" "));
    }
    let digits: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 4 { format!("●●●-●●●●-{}", &digits[digits.len() - 4..]) } else { "●●●●".into() }
}

// ───── 표 만들기 ─────

pub fn build(conn: &Connection, opts: &ExportOptions) -> AppResult<Vec<Table>> {
    let mut sheets: Vec<Sheet> = Vec::new();
    for s in Sheet::ALL {
        if opts.sheets.contains(&s) {
            sheets.push(s);
        }
    }
    if sheets.is_empty() {
        return Err(AppError::invalid("내보낼 항목을 하나 이상 골라 주세요."));
    }
    sheets.into_iter().map(|s| build_sheet(conn, s, opts)).collect()
}

fn build_sheet(conn: &Connection, sheet: Sheet, o: &ExportOptions) -> AppResult<Table> {
    let r = year_range(o.school_year);
    match sheet {
        Sheet::Roster => roster(conn, o),
        Sheet::Notes => notes(conn, &r),
        Sheet::Observations => observations(conn, o.class_id, &r),
        Sheet::Drafts => drafts(conn, o.class_id, o.school_year, &r),
        Sheet::Tasks => tasks(conn, &r),
        Sheet::Events => events(conn, &r),
        Sheet::Duties => duties(conn),
        Sheet::DutyLogs => duty_logs(conn, &r),
        Sheet::Timetable => timetable(conn, o.school_year),
    }
}

type Range = (Option<String>, Option<String>);

const CLASS_ORDER: &str = "c.school_year IS NULL, c.school_year, c.grade, c.class_no, s.number IS NULL, s.number, s.name";

fn roster(conn: &Connection, o: &ExportOptions) -> AppResult<Table> {
    let all = RosterColumns {
        phone: true,
        guardian_phone: true,
        address: true,
        gender: true,
        birthday: true,
        memo: true,
        student_no: true,
        prev_record: true,
        remark: true,
    };
    let cols = o.roster_columns.clone().unwrap_or(all);
    // 화면의 '보일 항목' 순서와 같게
    let optional: [(bool, &'static str, f64, bool); 9] = [
        (cols.phone, "전화번호", 16.0, false),
        (cols.guardian_phone, "보호자 연락처", 16.0, false),
        (cols.address, "주소", 40.0, true),
        (cols.gender, "성별", 6.0, false),
        (cols.birthday, "생일", 14.0, false),
        (cols.memo, "특이사항", 40.0, true),
        (cols.student_no, "학생개인번호", 14.0, false),
        (cols.prev_record, "이전학적", 14.0, false),
        (cols.remark, "비고", 20.0, true),
    ];
    // 보호자 연락처는 1~4 를 따로 열로 (관계 + 번호)
    const GUARDIAN_COLS: [&str; 4] = ["보호자 연락처1", "보호자 연락처2", "보호자 연락처3", "보호자 연락처4"];
    let mut columns = vec![("학급", 8.0, false), ("번호", 6.0, false), ("이름", 12.0, false)];
    for c in optional.iter().filter(|c| c.0) {
        if c.1 == "보호자 연락처" {
            columns.extend(GUARDIAN_COLS.iter().map(|g| (*g, 18.0, false)));
        } else {
            columns.push((c.1, c.2, c.3));
        }
    }
    columns.push(("관찰 기록 수", 10.0, false));
    let mut contacts = crate::profile::all_contacts(conn)?;

    let sql = format!(
        "SELECT COALESCE(c.label,''), s.number, s.name, s.phone, s.guardian_phone, s.address, s.gender, COALESCE(s.birthday,''), s.memo,
                (SELECT COUNT(*) FROM observations ob WHERE ob.student_id = s.id), s.student_no, s.prev_record, s.remark, s.id
         FROM students s LEFT JOIN classes c ON c.id = s.class_id
         WHERE ((?2 IS NULL AND s.archived = 0) OR c.school_year = ?2) AND (?1 IS NULL OR s.class_id = ?1)
           AND (?3 IS NULL OR instr(?3, ',' || s.id || ',') > 0)
         ORDER BY {CLASS_ORDER}"
    );
    let only: Option<String> = o.student_ids.as_ref().map(|ids| format!(",{},", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")));
    let mut st = conn.prepare(&sql)?;
    let raw = st
        .query_map(params![o.class_id, o.school_year, only], |r| {
            let phone: String = r.get(3)?;
            let guardian: String = r.get(4)?;
            let address: String = r.get(5)?;
            let (phone, address) = if o.mask_contacts { (mask_contact(&phone, false), mask_contact(&address, true)) } else { (phone, address) };
            let birthday: String = r.get(7)?;
            let values = [
                Value::text(phone),
                Value::text(guardian),
                Value::text(address),
                Value::text(r.get::<_, String>(6)?),
                Value::text(birthday_text(&birthday)),
                Value::text(r.get::<_, String>(8)?),
                Value::text(r.get::<_, String>(10)?),
                Value::text(r.get::<_, String>(11)?),
                Value::text(r.get::<_, String>(12)?),
            ];
            let head = vec![Value::text(r.get::<_, String>(0)?), Value::opt_int(r.get(1)?), Value::text(r.get::<_, String>(2)?)];
            Ok((r.get::<_, i64>(13)?, head, values, r.get::<_, i64>(9)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut rows = Vec::new();
    for (sid, head, values, obs) in raw {
        let mut row = head;
        for (c, v) in optional.iter().zip(values) {
            if !c.0 {
                continue;
            }
            if c.1 == "보호자 연락처" {
                let list = contacts.remove(&sid).unwrap_or_default();
                for i in 0..4 {
                    let cell = list
                        .get(i)
                        .map(|x| {
                            let phone = if o.mask_contacts { mask_contact(&x.phone, false) } else { x.phone.clone() };
                            format!("{} {}", x.relation, phone).trim().to_string()
                        })
                        .unwrap_or_default();
                    row.push(Value::text(cell));
                }
            } else {
                row.push(v);
            }
        }
        row.push(Value::Int(obs));
        rows.push(row);
    }
    Ok(Table { sheet: Sheet::Roster, columns, rows })
}

fn notes(conn: &Connection, range: &Range) -> AppResult<Table> {
    let mut st = conn.prepare(
        "SELECT n.id, n.created_at, n.raw_text, COALESCE(n.summary,''), n.status,
                COALESCE((SELECT GROUP_CONCAT(x.name, ', ') FROM note_students ns JOIN students x ON x.id = ns.student_id WHERE ns.note_id = n.id), '')
         FROM notes n WHERE (?1 IS NULL OR (n.created_at >= ?1 AND n.created_at < ?2)) ORDER BY n.created_at, n.id",
    )?;
    let rows = st
        .query_map(params![range.0, range.1], |r| {
            Ok(vec![
                Value::Int(r.get(0)?),
                Value::text(dt_text(&r.get::<_, String>(1)?)),
                Value::text(r.get::<_, String>(2)?),
                Value::text(r.get::<_, String>(3)?),
                Value::text(status_text(&r.get::<_, String>(4)?)),
                Value::text(r.get::<_, String>(5)?),
            ])
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Table {
        sheet: Sheet::Notes,
        columns: vec![
            ("기록 번호", 9.0, false),
            ("작성 시각", 17.0, false),
            ("원문", 60.0, true),
            ("요약", 40.0, true),
            ("분류 상태", 10.0, false),
            ("관련 학생", 16.0, true),
        ],
        rows,
    })
}

fn observations(conn: &Connection, class_id: Option<i64>, range: &Range) -> AppResult<Table> {
    let sql = format!(
        "SELECT COALESCE(c.label,''), s.number, COALESCE(s.name,''), o.area, o.observed_on, o.content, o.source, o.edited, o.note_id
         FROM observations o LEFT JOIN students s ON s.id = o.student_id LEFT JOIN classes c ON c.id = s.class_id
         WHERE (?1 IS NULL OR s.class_id = ?1) AND (?2 IS NULL OR (o.observed_on >= ?2 AND o.observed_on < ?3))
         ORDER BY s.id IS NULL, {CLASS_ORDER}, o.area, o.observed_on, o.id"
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st
        .query_map(params![class_id, range.0, range.1], |r| {
            let name: String = r.get(2)?;
            Ok(vec![
                Value::text(r.get::<_, String>(0)?),
                Value::opt_int(r.get(1)?),
                Value::text(if name.is_empty() { "학생 미지정".to_string() } else { name }),
                Value::text(Area::parse(&r.get::<_, String>(3)?).label()),
                Value::text(r.get::<_, String>(4)?),
                Value::text(r.get::<_, String>(5)?),
                Value::text(source_text(&r.get::<_, String>(6)?, r.get::<_, i64>(7)? != 0)),
                Value::opt_int(r.get(8)?),
            ])
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Table {
        sheet: Sheet::Observations,
        columns: vec![
            ("학급", 8.0, false),
            ("번호", 6.0, false),
            ("이름", 12.0, false),
            ("영역", 18.0, false),
            ("날짜", 11.0, false),
            ("내용", 60.0, true),
            ("출처", 12.0, false),
            ("기록 번호", 9.0, false),
        ],
        rows,
    })
}

fn drafts(conn: &Connection, class_id: Option<i64>, year: Option<i32>, range: &Range) -> AppResult<Table> {
    let sql = format!(
        "SELECT COALESCE(c.label,''), s.number, s.name, d.area, d.body, d.generated_by, d.reviewed, d.updated_at, d.subject
         FROM record_drafts d JOIN students s ON s.id = d.student_id LEFT JOIN classes c ON c.id = s.class_id
         WHERE (?1 IS NULL OR s.class_id = ?1)
           AND (?2 IS NULL OR c.school_year = ?2 OR (c.id IS NULL AND d.updated_at >= ?3 AND d.updated_at < ?4))
         ORDER BY {CLASS_ORDER}, d.area, d.updated_at"
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st
        .query_map(params![class_id, year, range.0, range.1], |r| {
            let body: String = r.get(4)?;
            let by: String = r.get(5)?;
            Ok(vec![
                Value::text(r.get::<_, String>(0)?),
                Value::opt_int(r.get(1)?),
                Value::text(r.get::<_, String>(2)?),
                Value::text({
                    let label = Area::parse(&r.get::<_, String>(3)?).label();
                    let subject: String = r.get(8)?;
                    if subject.is_empty() { label.to_string() } else { format!("{label} ({subject})") }
                }),
                Value::Int(body.chars().count() as i64),
                Value::text(body),
                Value::text(match by.as_str() {
                    "ai" => "AI 초안",
                    "template" => "기본 초안",
                    _ => "직접 작성",
                }),
                Value::text(yes(r.get::<_, i64>(6)? != 0)),
                Value::text(dt_text(&r.get::<_, String>(7)?)),
            ])
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Table {
        sheet: Sheet::Drafts,
        columns: vec![
            ("학급", 8.0, false),
            ("번호", 6.0, false),
            ("이름", 12.0, false),
            ("영역", 18.0, false),
            ("글자 수", 8.0, false),
            ("초안", 70.0, true),
            ("만든 방법", 10.0, false),
            ("최종안", 9.0, false),
            ("고친 시각", 17.0, false),
        ],
        rows,
    })
}

fn tasks(conn: &Connection, range: &Range) -> AppResult<Table> {
    let mut st = conn.prepare(
        "SELECT t.title, COALESCE(t.due_date,''), COALESCE(t.due_time,''), COALESCE(t.date_expr,''), t.done, COALESCE(t.done_at,''),
                COALESCE(du.name,''), t.source, t.edited, t.note_id
         FROM tasks t LEFT JOIN duties du ON du.id = t.duty_id
         WHERE (?1 IS NULL OR (COALESCE(t.due_date, substr(t.created_at, 1, 10)) >= ?1 AND COALESCE(t.due_date, substr(t.created_at, 1, 10)) < ?2))
         ORDER BY t.done, t.due_date IS NULL, t.due_date, t.due_time, t.id",
    )?;
    let rows = st
        .query_map(params![range.0, range.1], |r| {
            Ok(vec![
                Value::text(r.get::<_, String>(0)?),
                Value::text(r.get::<_, String>(1)?),
                Value::text(r.get::<_, String>(2)?),
                Value::text(r.get::<_, String>(3)?),
                Value::text(if r.get::<_, i64>(4)? != 0 { "완료" } else { "" }),
                Value::text(dt_text(&r.get::<_, String>(5)?)),
                Value::text(r.get::<_, String>(6)?),
                Value::text(source_text(&r.get::<_, String>(7)?, r.get::<_, i64>(8)? != 0)),
                Value::opt_int(r.get(9)?),
            ])
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Table {
        sheet: Sheet::Tasks,
        columns: vec![
            ("할 일", 50.0, true),
            ("기한", 11.0, false),
            ("시각", 7.0, false),
            ("원래 표현", 14.0, false),
            ("완료", 6.0, false),
            ("완료 시각", 17.0, false),
            ("담당업무", 14.0, false),
            ("출처", 12.0, false),
            ("기록 번호", 9.0, false),
        ],
        rows,
    })
}

fn events(conn: &Connection, range: &Range) -> AppResult<Table> {
    let mut st = conn.prepare(
        "SELECT layer, title, start_date, end_date, COALESCE(time,''), COALESCE(date_expr,''), memo, source, edited, note_id
         FROM events WHERE (?1 IS NULL OR (start_date < ?2 AND end_date >= ?1))
         ORDER BY start_date, time IS NULL, time, id",
    )?;
    let rows = st
        .query_map(params![range.0, range.1], |r| {
            let start: String = r.get(2)?;
            let end: String = r.get(3)?;
            Ok(vec![
                Value::text(layer_text(&r.get::<_, String>(0)?)),
                Value::text(r.get::<_, String>(1)?),
                Value::text(start.clone()),
                Value::text(if end == start { String::new() } else { end }),
                Value::text(r.get::<_, String>(4)?),
                Value::text(r.get::<_, String>(5)?),
                Value::text(r.get::<_, String>(6)?),
                Value::text(source_text(&r.get::<_, String>(7)?, r.get::<_, i64>(8)? != 0)),
                Value::opt_int(r.get(9)?),
            ])
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Table {
        sheet: Sheet::Events,
        columns: vec![
            ("구분", 16.0, false),
            ("제목", 36.0, true),
            ("날짜", 11.0, false),
            ("끝나는 날", 11.0, false),
            ("시각", 7.0, false),
            ("원래 표현", 14.0, false),
            ("메모", 30.0, true),
            ("출처", 12.0, false),
            ("기록 번호", 9.0, false),
        ],
        rows,
    })
}

fn duties(conn: &Connection) -> AppResult<Table> {
    let mut st = conn.prepare("SELECT name, detail, active FROM duties ORDER BY sort_order, id")?;
    let rows = st
        .query_map([], |r| {
            Ok(vec![
                Value::text(r.get::<_, String>(0)?),
                Value::text(r.get::<_, String>(1)?),
                Value::text(if r.get::<_, i64>(2)? != 0 { "사용" } else { "사용 안 함" }),
            ])
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Table {
        sheet: Sheet::Duties,
        columns: vec![("업무명", 20.0, false), ("세부업무", 70.0, true), ("상태", 10.0, false)],
        rows,
    })
}

fn duty_logs(conn: &Connection, range: &Range) -> AppResult<Table> {
    let mut st = conn.prepare(
        "SELECT COALESCE(du.name,''), l.content, COALESCE(l.date,''), COALESCE(l.time,''), l.done, l.source, l.edited, l.note_id
         FROM duty_logs l LEFT JOIN duties du ON du.id = l.duty_id
         WHERE (?1 IS NULL OR (COALESCE(l.date, substr(l.created_at, 1, 10)) >= ?1 AND COALESCE(l.date, substr(l.created_at, 1, 10)) < ?2))
         ORDER BY l.date IS NULL, l.date, l.time, l.id",
    )?;
    let rows = st
        .query_map(params![range.0, range.1], |r| {
            let duty: String = r.get(0)?;
            Ok(vec![
                Value::text(if duty.is_empty() { "업무 미지정".to_string() } else { duty }),
                Value::text(r.get::<_, String>(1)?),
                Value::text(r.get::<_, String>(2)?),
                Value::text(r.get::<_, String>(3)?),
                Value::text(if r.get::<_, i64>(4)? != 0 { "완료" } else { "" }),
                Value::text(source_text(&r.get::<_, String>(5)?, r.get::<_, i64>(6)? != 0)),
                Value::opt_int(r.get(7)?),
            ])
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Table {
        sheet: Sheet::DutyLogs,
        columns: vec![
            ("담당업무", 16.0, false),
            ("내용", 60.0, true),
            ("날짜", 11.0, false),
            ("시각", 7.0, false),
            ("완료", 6.0, false),
            ("출처", 12.0, false),
            ("기록 번호", 9.0, false),
        ],
        rows,
    })
}

fn timetable(conn: &Connection, year: Option<i32>) -> AppResult<Table> {
    let mut st = conn.prepare(
        "SELECT t.school_year, t.semester, t.kind, t.class_label, c.row_label, c.weekday, c.period,
                c.subject, c.teacher, c.class_label, c.room, c.memo
         FROM timetable_cells c JOIN timetables t ON t.id = c.timetable_id
         WHERE (?1 IS NULL OR t.school_year = ?1)
         ORDER BY t.school_year, t.semester,
                  CASE t.kind WHEN 'personal' THEN 0 WHEN 'class' THEN 1 ELSE 2 END,
                  t.class_label, c.row_label, c.weekday, c.period",
    )?;
    let rows = st
        .query_map(params![year], |r| {
            let kind: String = r.get(2)?;
            let tt_class: String = r.get(3)?;
            let row_label: String = r.get(4)?;
            let (kind_text, target) = match kind.as_str() {
                "personal" => ("개인", String::new()),
                "class" => ("학급", tt_class),
                _ => ("전체", row_label),
            };
            Ok(vec![
                Value::Int(r.get(0)?),
                Value::text(format!("{}학기", r.get::<_, i64>(1)?)),
                Value::text(kind_text),
                Value::text(target),
                Value::text(weekday_text(r.get(5)?)),
                Value::text(period_text(r.get(6)?)),
                Value::text(r.get::<_, String>(7)?),
                Value::text(r.get::<_, String>(8)?),
                Value::text(r.get::<_, String>(9)?),
                Value::text(r.get::<_, String>(10)?),
                Value::text(r.get::<_, String>(11)?),
            ])
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Table {
        sheet: Sheet::Timetable,
        columns: vec![
            ("학년도", 8.0, false),
            ("학기", 7.0, false),
            ("시간표", 8.0, false),
            ("대상", 8.0, false),
            ("요일", 6.0, false),
            ("교시", 9.0, false),
            ("과목", 14.0, false),
            ("교사", 10.0, false),
            ("수업 학급", 9.0, false),
            ("장소", 10.0, false),
            ("메모", 24.0, true),
        ],
        rows,
    })
}

// ───── 파일 쓰기 ─────

/// CSV 한 칸. 쉼표·따옴표·줄바꿈이 있으면 따옴표로 감싸고,
/// =, +, -, @ 로 시작하는 글은 엑셀이 수식으로 실행하지 않도록 앞에 ' 를 붙인다.
pub fn csv_field(s: &str) -> String {
    let risky = s.starts_with(['=', '+', '-', '@', '\t', '\r']) && s.trim().parse::<f64>().is_err();
    let s = if risky { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// 엑셀에서 한글이 깨지지 않도록 UTF-8 BOM 과 CRLF 줄바꿈을 쓴다
pub fn csv_text(t: &Table) -> String {
    let mut out = String::from("\u{feff}");
    out.push_str(&t.columns.iter().map(|c| csv_field(c.0)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
    for row in &t.rows {
        out.push_str(&row.iter().map(Value::as_csv).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

fn xlsx_err(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::msg(format!("엑셀 파일을 만들지 못했습니다: {e}"))
}

fn write_error(e: std::io::Error, path: &Path) -> AppError {
    // 32: 다른 프로그램이 파일을 쓰는 중 (엑셀에서 열려 있음)
    if e.raw_os_error() == Some(32) || e.kind() == std::io::ErrorKind::PermissionDenied {
        return AppError::msg(format!(
            "'{}' 파일이 엑셀 등 다른 프로그램에서 열려 있거나 쓸 수 없는 위치입니다. 파일을 닫거나 다른 위치를 골라 주세요.",
            path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        ));
    }
    AppError::Io(e)
}

pub fn write_xlsx(tables: &[Table], path: &Path) -> AppResult<()> {
    use rust_xlsxwriter::{Color, Format as XFormat, FormatAlign, FormatBorder, Workbook};
    let head = XFormat::new()
        .set_bold()
        .set_background_color(Color::RGB(0xE8F0EA))
        .set_border_bottom(FormatBorder::Thin)
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter);
    let cell = XFormat::new().set_align(FormatAlign::Top);
    let wrap = XFormat::new().set_align(FormatAlign::Top).set_text_wrap();
    let mut wb = Workbook::new();
    for t in tables {
        let ws = wb.add_worksheet();
        ws.set_name(t.sheet.title()).map_err(xlsx_err)?;
        for (c, (title, width, _)) in t.columns.iter().enumerate() {
            let c = c as u16;
            ws.write_string_with_format(0, c, *title, &head).map_err(xlsx_err)?;
            ws.set_column_width(c, *width).map_err(xlsx_err)?;
        }
        for (r, row) in t.rows.iter().enumerate() {
            let r = (r + 1) as u32;
            for (c, v) in row.iter().enumerate() {
                let fmt = if t.columns.get(c).is_some_and(|x| x.2) { &wrap } else { &cell };
                match v {
                    Value::Text(s) if s.is_empty() => {}
                    Value::Text(s) => {
                        ws.write_string_with_format(r, c as u16, s, fmt).map_err(xlsx_err)?;
                    }
                    Value::Int(n) => {
                        ws.write_number_with_format(r, c as u16, *n as f64, fmt).map_err(xlsx_err)?;
                    }
                }
            }
        }
        ws.set_freeze_panes(1, 0).map_err(xlsx_err)?;
        if !t.rows.is_empty() {
            ws.autofilter(0, 0, t.rows.len() as u32, (t.columns.len() - 1) as u16).map_err(xlsx_err)?;
        }
        // 인쇄: 가로 방향, 폭은 한 쪽에 맞추고 머리글 줄 반복
        ws.set_landscape();
        ws.set_print_fit_to_pages(1, 0);
        ws.set_repeat_rows(0, 0).map_err(xlsx_err)?;
    }
    // 반쯤 쓴 파일이 남지 않도록 임시 파일에 쓰고 이름을 바꾼다
    let tmp = path.with_extension("xlsx.part");
    wb.save(&tmp).map_err(|e| match e {
        rust_xlsxwriter::XlsxError::IoError(io) => write_error(io, &tmp),
        other => xlsx_err(other),
    })?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(write_error(e, path));
    }
    Ok(())
}

pub fn write_csv(tables: &[Table], dir: &Path) -> AppResult<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut files = Vec::new();
    for t in tables {
        let p = dir.join(format!("{}.csv", t.sheet.title()));
        std::fs::write(&p, csv_text(t)).map_err(|e| write_error(e, &p))?;
        files.push(p);
    }
    Ok(files)
}

/// 같은 이름이 있으면 ' (2)', ' (3)' 을 붙인다
pub fn unique_dir(parent: &Path, base: &str) -> PathBuf {
    let first = parent.join(base);
    if !first.exists() {
        return first;
    }
    (2..1000).map(|i| parent.join(format!("{base} ({i})"))).find(|p| !p.exists()).unwrap_or(first)
}

pub fn run(conn: &Connection, opts: &ExportOptions, stamp: &str) -> AppResult<ExportResult> {
    let raw = opts.path.trim();
    if raw.is_empty() {
        return Err(AppError::invalid("저장할 위치를 골라 주세요."));
    }
    let target = PathBuf::from(raw);
    if !target.is_absolute() {
        return Err(AppError::invalid("저장 위치가 올바르지 않습니다."));
    }
    let tables = build(conn, opts)?;
    let sheets = tables.iter().map(|t| SheetCount { title: t.sheet.title().into(), rows: t.rows.len() }).collect();
    match opts.format {
        Format::Xlsx => {
            let path = if target.extension().is_some_and(|e| e.eq_ignore_ascii_case("xlsx")) {
                target
            } else {
                target.with_extension("xlsx")
            };
            if !path.parent().is_some_and(Path::is_dir) {
                return Err(AppError::invalid("저장할 폴더를 찾지 못했습니다."));
            }
            write_xlsx(&tables, &path)?;
            let p = path.display().to_string();
            Ok(ExportResult { path: p.clone(), files: vec![p], sheets })
        }
        Format::Csv => {
            if !target.is_dir() {
                return Err(AppError::invalid("CSV 파일을 넣을 폴더를 찾지 못했습니다."));
            }
            let dir = unique_dir(&target, &format!("교무수첩 내보내기 {stamp}"));
            let files = write_csv(&tables, &dir)?;
            Ok(ExportResult {
                path: dir.display().to_string(),
                files: files.iter().map(|f| f.display().to_string()).collect(),
                sheets,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn seed(db: &Db) {
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO classes(id, school_year, grade, class_no, label) VALUES (1, 2026, 2, 1, '2-1'), (2, 2026, 2, 2, '2-2');
                 INSERT INTO students(id, class_id, number, name, phone, guardian_phone, address, gender, birthday, memo, created_at, updated_at) VALUES
                   (1, 1, 1, '홍길동', '010-1234-5678', '010-9999-0001', 'OO도 OO시 OO구 OO로 1', '남', '03-05', '알레르기: 땅콩', 'x', 'x'),
                   (2, 1, 2, '성춘향', '', '', '', '여', '2012-11-20', '', 'x', 'x'),
                   (3, 2, 1, '이몽룡', '', '', '', '', NULL, '', 'x', 'x');
                 INSERT INTO notes(id, raw_text, created_at, updated_at, status, summary) VALUES
                   (1, '홍길동 3교시 수업 중 발표를 잘함, 칭찬함', '2026-09-29T10:15:00', 'x', 'classified', '홍길동 발표 칭찬'),
                   (2, '=SUM(A1) 같은 글, \"따옴표\"', '2026-09-29T11:00:00', 'x', 'pending', NULL);
                 INSERT INTO note_students(note_id, student_id, source) VALUES (1, 1, 'ai');
                 INSERT INTO observations(note_id, student_id, area, content, observed_on, source, edited, created_at, updated_at) VALUES
                   (1, 1, 'subject', '수업 중 발표를 잘함', '2026-09-29', 'ai', 0, 'x', 'x'),
                   (NULL, 3, 'behavior', '친구를 도움', '2026-09-28', 'manual', 0, 'x', 'x');
                 INSERT INTO tasks(title, due_date, source, created_at, updated_at) VALUES ('가정통신문 회수', '2026-10-02', 'ai', 'x', 'x');
                 INSERT INTO events(layer, title, start_date, end_date, source, created_at, updated_at) VALUES ('annual', '2학기 기말고사', '2026-12-08', '2026-12-10', 'manual', 'x', 'x');
                 INSERT INTO timetables(id, school_year, semester, kind, class_label, created_at, updated_at) VALUES (1, 2026, 2, 'personal', '', 'x', 'x');
                 INSERT INTO timetable_cells(timetable_id, weekday, period, subject, class_label, room) VALUES (1, 1, 1, '국어', '2-1', ''), (1, 2, 101, '방과후 독서', '', '도서관');",
            )?;
            Ok(())
        })
        .unwrap();
    }

    fn opts(sheets: Vec<Sheet>) -> ExportOptions {
        ExportOptions { format: Format::Csv, path: String::new(), sheets, class_id: None, mask_contacts: false, roster_columns: None, school_year: None, student_ids: None }
    }

    #[test]
    fn school_year_filter_keeps_only_that_year() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO classes(id, school_year, grade, class_no, label) VALUES (9, 2025, 1, 1, '1-1');
                 INSERT INTO students(id, class_id, number, name, archived, created_at, updated_at) VALUES (9, 9, 1, '작년학생', 1, 'x', 'x'), (10, 1, 9, '보관학생', 1, 'x', 'x');
                 INSERT INTO notes(raw_text, created_at, updated_at) VALUES ('작년 기록', '2026-02-10T09:00:00', 'x');
                 INSERT INTO events(layer, title, start_date, end_date, source, created_at, updated_at) VALUES ('annual', '작년 졸업식', '2026-02-12', '2026-02-12', 'manual', 'x', 'x'),
                   ('annual', '겨울방학', '2027-01-05', '2027-03-02', 'manual', 'x', 'x');",
            )?;
            Ok(())
        })
        .unwrap();
        let mut o = opts(Sheet::ALL.to_vec());
        o.school_year = Some(2026);
        let tables = db.with(|c| build(c, &o)).unwrap();
        let get = |s: Sheet| tables.iter().find(|t| t.sheet == s).unwrap();
        // 그 학년도 학급 학생은 보관 처리됐어도 들어가고, 다른 학년도 학생은 빠진다
        let names: Vec<&Value> = get(Sheet::Roster).rows.iter().map(|r| &r[2]).collect();
        assert!(names.contains(&&Value::text("보관학생")) && !names.contains(&&Value::text("작년학생")));
        assert_eq!(get(Sheet::Notes).rows.len(), 2);
        let events: Vec<&Value> = get(Sheet::Events).rows.iter().map(|r| &r[1]).collect();
        // 학년도에 걸친 일정(겨울방학)은 들어가고, 지난 학년도 일정은 빠진다
        assert!(events.contains(&&Value::text("겨울방학")) && !events.contains(&&Value::text("작년 졸업식")));
        // 거르지 않으면 보관 학생은 명렬표에서 빠진다
        let all = db.with(|c| build(c, &opts(vec![Sheet::Roster]))).unwrap();
        assert!(all[0].rows.iter().all(|r| r[2] != Value::text("보관학생")));
    }

    #[test]
    fn csv_quoting_and_formula_guard() {
        assert_eq!(csv_field("홍길동"), "홍길동");
        assert_eq!(csv_field("가, 나"), "\"가, 나\"");
        assert_eq!(csv_field("그는 \"네\"라고"), "\"그는 \"\"네\"\"라고\"");
        assert_eq!(csv_field("첫 줄\n둘째 줄"), "\"첫 줄\n둘째 줄\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("- 준비물 챙기기"), "'- 준비물 챙기기");
        assert_eq!(csv_field("-3"), "-3");
        assert_eq!(csv_field("010-1234-5678"), "010-1234-5678");
    }

    #[test]
    fn mask_like_roster_screen() {
        assert_eq!(mask_contact("010-1234-5678", false), "●●●-●●●●-5678");
        assert_eq!(mask_contact("12", false), "●●●●");
        assert_eq!(mask_contact("OO도 OO시 OO구 OO로 1", true), "OO도 OO시 ●●●");
        assert_eq!(mask_contact("", true), "");
    }

    #[test]
    fn roster_all_columns_order_and_mask() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        // 보호자 연락처는 관계 + 번호로 1~4 열
        db.with(|c| {
            let sid: i64 = c.query_row("SELECT id FROM students WHERE name='홍길동'", [], |r| r.get(0))?;
            crate::profile::set_contacts(
                c,
                sid,
                &[
                    crate::profile::Contact { id: 0, relation: "모".into(), phone: "010-9876-5432".into() },
                    crate::profile::Contact { id: 0, relation: "부".into(), phone: "010-1111-2222".into() },
                ],
                crate::util::parse_dt("2026-09-29T10:00:00").unwrap(),
            )?;
            Ok(())
        })
        .unwrap();
        let t = db.with(|c| roster(c, &opts(vec![Sheet::Roster]))).unwrap();
        let heads: Vec<&str> = t.columns.iter().map(|c| c.0).collect();
        assert_eq!(
            heads,
            [
                "학급", "번호", "이름", "전화번호", "보호자 연락처1", "보호자 연락처2", "보호자 연락처3", "보호자 연락처4", "주소", "성별", "생일", "특이사항", "학생개인번호",
                "이전학적", "비고", "관찰 기록 수"
            ]
        );
        assert_eq!(t.rows.len(), 3);
        assert_eq!(t.rows[0][2], Value::text("홍길동"));
        assert_eq!(t.rows[0][3], Value::text("010-1234-5678"));
        assert_eq!((&t.rows[0][4], &t.rows[0][5], &t.rows[0][6]), (&Value::text("모 010-9876-5432"), &Value::text("부 010-1111-2222"), &Value::text("")));
        assert_eq!(t.rows[0][10], Value::text("3월 5일"));
        assert_eq!(t.rows[0][15], Value::Int(1));
        // 학급 순서: 2-1 두 명 뒤에 2-2
        assert_eq!(t.rows[2][0], Value::text("2-2"));

        let mut o = opts(vec![Sheet::Roster]);
        o.mask_contacts = true;
        o.class_id = Some(1);
        o.roster_columns = Some(RosterColumns { phone: true, birthday: true, ..Default::default() });
        let t = db.with(|c| roster(c, &o)).unwrap();
        let heads: Vec<&str> = t.columns.iter().map(|c| c.0).collect();
        assert_eq!(heads, ["학급", "번호", "이름", "전화번호", "생일", "관찰 기록 수"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][3], Value::text("●●●-●●●●-5678"));
        assert_eq!(t.rows[1][4], Value::text("2012-11-20"));
        // 가리기: 보호자 연락처도 뒤 4자리만
        o.roster_columns = Some(RosterColumns { guardian_phone: true, ..Default::default() });
        let t = db.with(|c| roster(c, &o)).unwrap();
        assert_eq!(t.rows[0][3], Value::text("모 ●●●-●●●●-5432"));
    }

    #[test]
    fn every_sheet_builds_and_csv_is_excel_friendly() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let tables = db.with(|c| build(c, &opts(Sheet::ALL.to_vec()))).unwrap();
        assert_eq!(tables.len(), Sheet::ALL.len());
        let notes = tables.iter().find(|t| t.sheet == Sheet::Notes).unwrap();
        let text = csv_text(notes);
        assert!(text.starts_with('\u{feff}'));
        assert!(text.contains("\r\n1,2026-09-29 10:15,\"홍길동 3교시 수업 중 발표를 잘함, 칭찬함\",홍길동 발표 칭찬,AI 분류,홍길동\r\n"));
        assert!(text.contains("'=SUM(A1) 같은 글"));
        let obs = tables.iter().find(|t| t.sheet == Sheet::Observations).unwrap();
        assert_eq!(obs.rows[0][3], Value::text("교과특기사항"));
        let tt = tables.iter().find(|t| t.sheet == Sheet::Timetable).unwrap();
        assert_eq!(tt.rows[1][5], Value::text("방과후 1"));
        let ev = tables.iter().find(|t| t.sheet == Sheet::Events).unwrap();
        assert_eq!(ev.rows[0][0], Value::text("학사력"));
        assert_eq!(ev.rows[0][3], Value::text("2026-12-10"));
        // 아무것도 고르지 않으면 거절
        assert!(db.with(|c| build(c, &opts(vec![]))).is_err());
    }

    #[test]
    fn writes_xlsx_and_csv_files() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let dir = tempfile::tempdir().unwrap();
        let mut o = opts(Sheet::ALL.to_vec());
        o.format = Format::Xlsx;
        o.path = dir.path().join("교무수첩").display().to_string();
        let r = db.with(|c| run(c, &o, "2026-09-30 1530")).unwrap();
        assert!(r.path.ends_with("교무수첩.xlsx"));
        let bytes = std::fs::read(&r.path).unwrap();
        assert_eq!(&bytes[..2], b"PK");
        assert!(!dir.path().join("교무수첩.xlsx.part").exists());

        o.format = Format::Csv;
        o.path = dir.path().display().to_string();
        let r1 = db.with(|c| run(c, &o, "2026-09-30 1530")).unwrap();
        let r2 = db.with(|c| run(c, &o, "2026-09-30 1530")).unwrap();
        assert_eq!(r1.files.len(), Sheet::ALL.len());
        assert!(r2.path.ends_with("(2)"));
        let roster = std::fs::read_to_string(Path::new(&r1.path).join("학급명렬표.csv")).unwrap();
        assert!(roster.starts_with("\u{feff}학급,번호,이름,"));
        // 상대 경로는 받지 않는다
        o.path = "상대경로".into();
        assert!(db.with(|c| run(c, &o, "x")).is_err());
    }
}
