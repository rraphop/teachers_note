//! 나이스 명렬표 엑셀 가져오기
//!
//! 나이스 → 학적 → 기본학적관리 → 명렬표출력 → 학급별 조회 → 엑셀파일 다운로드로 받은 파일을 읽는다.
//! 나이스 공개 API로는 받을 수 없는 자료라서 교사가 받은 파일을 그대로 넣는다.
//!
//! - 열 순서는 출력 설정에 따라 다르므로(기본 5열, 전체 12열) 머리글 이름으로 열을 찾는다.
//! - 모든 칸이 글자로 들어 있어 번호는 숫자로 바꾼다.
//! - 같은 학생은 학생개인번호(학년이 올라가도 그대로) → 학급·번호·이름 → 학급·이름 순으로 찾는다.
//!   다시 가져와도 인원이 늘지 않고, 빈칸이 기존 값을 지우지 않으며, 기존 기록은 그대로 둔다.
//! - 주민등록번호는 설정에서 켠 경우에만, 뒷자리를 가린 모양으로만 저장한다.
//! - 주소·우편번호·주민등록번호는 AI 분류 문맥에 넣지 않는다 (분류에는 이름·학급·번호만 쓴다).

use crate::error::{AppError, AppResult};
use crate::students::{active_school_year, find_or_create_class, normalize_birthday};
use crate::util;
use calamine::{Data, Range, Reader, open_workbook_auto};
use chrono::NaiveDateTime;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

/// 머리글로 찾는 열
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Grade,
    Class,
    Number,
    Name,
    Remark,
    Rrn,
    Gender,
    Birth,
    Postal,
    Address,
    PrevRecord,
    StudentNo,
}

impl Col {
    fn label(self) -> &'static str {
        match self {
            Col::Grade => "학년",
            Col::Class => "반",
            Col::Number => "번호",
            Col::Name => "성명",
            Col::Remark => "비고",
            Col::Rrn => "주민등록번호",
            Col::Gender => "성별",
            Col::Birth => "생년월일",
            Col::Postal => "우편번호",
            Col::Address => "주소",
            Col::PrevRecord => "이전학적정보",
            Col::StudentNo => "학생개인번호",
        }
    }
}

/// 머리글 글자 → 열. 공백·줄바꿈을 지우고 비교한다 ("주민등록번호\r\n(외국인등록번호)").
/// "번호"는 정확히 같을 때만 번호 열로 본다 (학생개인번호·우편번호·주민등록번호와 구분).
fn header_col(raw: &str) -> Option<Col> {
    let h: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    Some(match h.as_str() {
        "학년" => Col::Grade,
        "반" | "학급" => Col::Class,
        "번호" | "번" | "출석번호" => Col::Number,
        "성명" | "이름" | "학생명" | "학생성명" => Col::Name,
        "비고" => Col::Remark,
        "성별" => Col::Gender,
        "생년월일" => Col::Birth,
        "우편번호" => Col::Postal,
        "주소" => Col::Address,
        "학생개인번호" | "개인번호" => Col::StudentNo,
        _ if h.starts_with("주민등록번호") || h.starts_with("외국인등록번호") => Col::Rrn,
        _ if h.starts_with("이전학적") => Col::PrevRecord,
        _ if h.starts_with("주소") => Col::Address,
        _ => return None,
    })
}

/// 명렬표 한 줄
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RosterRow {
    pub grade: i64,
    pub class_no: i64,
    pub number: Option<i64>,
    pub name: String,
    pub remark: String,
    pub rrn: String,
    pub gender: String,
    pub birthday: Option<String>,
    pub postal_code: String,
    pub address: String,
    pub prev_record: String,
    pub student_no: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedRoster {
    pub rows: Vec<RosterRow>,
    /// 파일에 있던 선택 열 (학년·반·번호·성명 밖의 열)
    pub optional_columns: Vec<String>,
    pub has_rrn: bool,
    pub problems: Vec<String>,
}

/// 칸 값을 글자로. 숫자 칸(9.0)은 "9", 엑셀 날짜 칸은 "YYYY-MM-DD".
fn cell_text(d: &Data) -> String {
    match d {
        Data::String(s) => s.trim().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Data::Float(f) => f.to_string(),
        Data::DateTime(dt) => {
            let base = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap_or_default();
            base.checked_add_signed(chrono::Duration::days(dt.as_f64().floor() as i64)).map(util::fmt_d).unwrap_or_default()
        }
        Data::DateTimeIso(s) => s.get(..10).unwrap_or(s).to_string(),
        Data::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn small_number(s: &str) -> Option<i64> {
    let d = digits(s);
    if d.is_empty() || d.len() > 3 { None } else { d.parse().ok() }
}

/// 나이스가 뒷자리를 가려 내보낸 모양("YYMMDD-G******")만 남긴다. 혹시 전체 번호가 들어 있어도 뒷자리는 저장하지 않는다.
fn mask_rrn(s: &str) -> String {
    let d = digits(s);
    if d.len() < 7 {
        return String::new();
    }
    format!("{}-{}******", &d[..6], &d[6..7])
}

fn gender(s: &str) -> String {
    match s.trim() {
        "남" | "남성" | "남자" | "M" | "m" => "남".into(),
        "여" | "여성" | "여자" | "F" | "f" => "여".into(),
        _ => String::new(),
    }
}

/// 시트 범위를 읽는다. 머리글은 앞쪽 10줄 안에서 학년·반·번호·성명이 모두 있는 줄로 찾는다.
pub fn parse_range(range: &Range<Data>) -> AppResult<ParsedRoster> {
    let rows: Vec<&[Data]> = range.rows().collect();
    let mut header: Option<(usize, Vec<Option<Col>>)> = None;
    for (ri, row) in rows.iter().enumerate().take(10) {
        let cols: Vec<Option<Col>> = row.iter().map(|c| header_col(&cell_text(c))).collect();
        let has = |k: Col| cols.contains(&Some(k));
        if has(Col::Grade) && has(Col::Class) && has(Col::Number) && has(Col::Name) {
            header = Some((ri, cols));
            break;
        }
    }
    let Some((head_row, cols)) = header else {
        return Err(AppError::invalid(
            "나이스 명렬표 형식이 아닙니다. 첫 줄에 학년·반·번호·성명 머리글이 있는 파일을 골라 주세요.",
        ));
    };
    let find = |k: Col| cols.iter().position(|c| *c == Some(k));
    let mut out = ParsedRoster::default();
    for k in [Col::Gender, Col::Birth, Col::Postal, Col::Address, Col::PrevRecord, Col::StudentNo, Col::Rrn, Col::Remark] {
        if find(k).is_some() {
            out.optional_columns.push(k.label().to_string());
        }
    }
    out.has_rrn = find(Col::Rrn).is_some();
    let get = |row: &[Data], k: Col| -> String { find(k).and_then(|i| row.get(i)).map(cell_text).unwrap_or_default() };
    for (ri, row) in rows.iter().enumerate().skip(head_row + 1) {
        let line = ri + 1 + range.start().map(|s| s.0 as usize).unwrap_or(0);
        if row.iter().all(|c| cell_text(c).is_empty()) {
            continue;
        }
        let name: String = get(row, Col::Name).split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            continue;
        }
        let (Some(grade), Some(class_no)) = (small_number(&get(row, Col::Grade)), small_number(&get(row, Col::Class))) else {
            out.problems.push(format!("{line}번째 줄: 학년·반을 읽지 못해 건너뛰었습니다."));
            continue;
        };
        if !(1..=6).contains(&grade) || !(1..=30).contains(&class_no) {
            out.problems.push(format!("{line}번째 줄: 학년·반 값이 범위를 벗어나 건너뛰었습니다."));
            continue;
        }
        if name.chars().count() > 20 {
            out.problems.push(format!("{line}번째 줄: 이름이 너무 길어 건너뛰었습니다."));
            continue;
        }
        let birth_raw = get(row, Col::Birth);
        let birthday = match normalize_birthday(&birth_raw) {
            Ok(b) => b,
            Err(_) => {
                out.problems.push(format!("{line}번째 줄: 생년월일을 읽지 못했습니다."));
                None
            }
        };
        let student_no = digits(&get(row, Col::StudentNo));
        out.rows.push(RosterRow {
            grade,
            class_no,
            number: small_number(&get(row, Col::Number)),
            name,
            remark: util_limit(&get(row, Col::Remark), 200),
            rrn: mask_rrn(&get(row, Col::Rrn)),
            gender: gender(&get(row, Col::Gender)),
            birthday,
            postal_code: util_limit(&digits(&get(row, Col::Postal)), 10),
            address: util_limit(&get(row, Col::Address), 200),
            prev_record: util_limit(&get(row, Col::PrevRecord).split_whitespace().collect::<String>(), 40),
            student_no: if student_no.len() >= 6 { student_no } else { String::new() },
        });
    }
    // 번호 순으로 (값이 글자라 "10"이 "2"보다 앞에 오는 일을 막는다)
    out.rows.sort_by_key(|r| (r.grade, r.class_no, r.number.unwrap_or(i64::MAX)));
    Ok(out)
}

fn util_limit(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

/// 파일 하나를 읽는다 (xlsx·xls). 첫 시트만 본다.
pub fn parse_file(path: &Path) -> AppResult<ParsedRoster> {
    let mut wb = open_workbook_auto(path).map_err(|_| AppError::invalid("엑셀 파일을 열지 못했습니다. 나이스에서 받은 .xlsx 또는 .xls 파일인지 확인해 주세요."))?;
    let range = wb
        .worksheet_range_at(0)
        .ok_or_else(|| AppError::invalid("엑셀 파일에 시트가 없습니다."))?
        .map_err(|_| AppError::invalid("엑셀 시트를 읽지 못했습니다."))?;
    parse_range(&range)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FileResult {
    pub file: String,
    /// 이 파일에 든 학급 (보통 하나)
    pub classes: Vec<String>,
    pub total: usize,
    /// 새로 등록한 학생
    pub added: usize,
    /// 이미 있던 학생에게 정보를 보탠 수
    pub updated: usize,
    /// 기록에서 자동 등록된 '확인 필요' 학생과 합친 수
    pub merged: usize,
    /// 바뀐 것이 없는 학생
    pub unchanged: usize,
    pub optional_columns: Vec<String>,
    /// 주민등록번호 열이 있었지만 설정이 꺼져 있어 저장하지 않음
    pub rrn_skipped: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NeisImportResult {
    pub files: Vec<FileResult>,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct Existing {
    class_id: Option<i64>,
    number: Option<i64>,
    name: String,
    remark: String,
    rrn: String,
    gender: String,
    birthday: Option<String>,
    postal_code: String,
    address: String,
    prev_record: String,
    student_no: String,
    needs_review: bool,
    archived: bool,
}

fn load_existing(conn: &Connection, id: i64) -> AppResult<Existing> {
    Ok(conn.query_row(
        "SELECT class_id, number, name, remark, rrn, gender, birthday, postal_code, address, prev_record, student_no, needs_review, archived
         FROM students WHERE id=?1",
        params![id],
        |r| {
            Ok(Existing {
                class_id: r.get(0)?,
                number: r.get(1)?,
                name: r.get(2)?,
                remark: r.get(3)?,
                rrn: r.get(4)?,
                gender: r.get(5)?,
                birthday: r.get(6)?,
                postal_code: r.get(7)?,
                address: r.get(8)?,
                prev_record: r.get(9)?,
                student_no: r.get(10)?,
                needs_review: r.get::<_, i64>(11)? != 0,
                archived: r.get::<_, i64>(12)? != 0,
            })
        },
    )?)
}

/// 같은 학생 찾기. (학생 id, 기록에서 자동 등록된 학생과 합친 것인지)
fn match_student(conn: &Connection, row: &RosterRow, class_id: i64) -> AppResult<Option<(i64, bool)>> {
    // 1. 학생개인번호 (보관한 학생도 찾는다 — 학년이 올라가 학급이 바뀐 경우)
    if !row.student_no.is_empty() {
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM students WHERE student_no=?1 ORDER BY archived, id DESC LIMIT 1",
                params![row.student_no],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            return Ok(Some((id, false)));
        }
    }
    // 학생개인번호가 서로 다르게 적혀 있으면 다른 학생이다
    let no_ok = "(student_no='' OR ?4='' OR student_no=?4)";
    // 2. 학급 + 번호 + 이름
    if let Some(n) = row.number {
        if let Some(id) = conn
            .query_row(
                &format!("SELECT id FROM students WHERE class_id=?1 AND number=?2 AND name=?3 AND archived=0 AND {no_ok} ORDER BY id LIMIT 1"),
                params![class_id, n, row.name, row.student_no],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            return Ok(Some((id, false)));
        }
    }
    // 3. 학급 + 이름 (번호가 비어 있거나 같은 학생). 여럿이면 정하지 않는다.
    let same_class: Vec<(i64, bool)> = {
        let mut st = conn.prepare(&format!(
            "SELECT id, needs_review FROM students WHERE class_id=?1 AND name=?2 AND archived=0 AND (number IS NULL OR ?3 IS NULL OR number=?3) AND {no_ok}"
        ))?;
        st.query_map(params![class_id, row.name, row.number, row.student_no], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)))?
            .collect::<Result<_, _>>()?
    };
    if same_class.len() == 1 {
        return Ok(Some(same_class[0]));
    }
    if same_class.len() > 1 {
        return Ok(None);
    }
    // 4. 기록에서 학급 없이 자동 등록된 같은 이름의 학생 (하나일 때만)
    let loose: Vec<i64> = {
        let mut st = conn.prepare("SELECT id FROM students WHERE class_id IS NULL AND name=?1 AND archived=0 AND needs_review=1 AND student_no=''")?;
        st.query_map(params![row.name], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    if loose.len() == 1 {
        return Ok(Some((loose[0], true)));
    }
    Ok(None)
}

fn keep_or<'a>(new: &'a str, old: &'a str) -> &'a str {
    if new.trim().is_empty() { old } else { new }
}

/// 한 학생 반영. 반환: 0 새로, 1 보탬, 2 합침, 3 그대로
fn upsert(conn: &Connection, row: &RosterRow, class_id: i64, store_rrn: bool, now: NaiveDateTime) -> AppResult<u8> {
    let now_s = util::fmt_dt(now);
    let rrn = if store_rrn { row.rrn.as_str() } else { "" };
    let Some((id, loose)) = match_student(conn, row, class_id)? else {
        conn.execute(
            "INSERT INTO students(class_id, number, name, remark, rrn, gender, birthday, postal_code, address, prev_record, student_no, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            params![
                class_id,
                row.number,
                row.name,
                row.remark,
                rrn,
                row.gender,
                row.birthday,
                row.postal_code,
                row.address,
                row.prev_record,
                row.student_no,
                now_s
            ],
        )?;
        return Ok(0);
    };
    let old = load_existing(conn, id)?;
    let new = Existing {
        class_id: Some(class_id),
        number: row.number.or(old.number),
        name: row.name.clone(),
        remark: keep_or(&row.remark, &old.remark).to_string(),
        rrn: keep_or(rrn, &old.rrn).to_string(),
        gender: keep_or(&row.gender, &old.gender).to_string(),
        birthday: row.birthday.clone().or(old.birthday.clone()),
        postal_code: keep_or(&row.postal_code, &old.postal_code).to_string(),
        address: keep_or(&row.address, &old.address).to_string(),
        prev_record: keep_or(&row.prev_record, &old.prev_record).to_string(),
        student_no: keep_or(&row.student_no, &old.student_no).to_string(),
        needs_review: false,
        archived: false,
    };
    if new == old {
        return Ok(3);
    }
    conn.execute(
        "UPDATE students SET class_id=?2, number=?3, name=?4, remark=?5, rrn=?6, gender=?7, birthday=?8, postal_code=?9, address=?10,
                prev_record=?11, student_no=?12, needs_review=0, archived=0, updated_at=?13 WHERE id=?1",
        params![
            id,
            new.class_id,
            new.number,
            new.name,
            new.remark,
            new.rrn,
            new.gender,
            new.birthday,
            new.postal_code,
            new.address,
            new.prev_record,
            new.student_no,
            now_s
        ],
    )?;
    Ok(if loose || old.needs_review { 2 } else { 1 })
}

/// 읽은 명렬표를 DB에 반영한다 (파일 하나 = 트랜잭션 하나)
pub fn import_parsed(conn: &mut Connection, file: &str, parsed: &ParsedRoster, store_rrn: bool, now: NaiveDateTime) -> AppResult<FileResult> {
    let tx = conn.transaction()?;
    let year = active_school_year(&tx, now.date());
    let mut res = FileResult {
        file: file.to_string(),
        total: parsed.rows.len(),
        optional_columns: parsed.optional_columns.clone(),
        rrn_skipped: parsed.has_rrn && !store_rrn,
        ..Default::default()
    };
    let mut classes = BTreeSet::new();
    for row in &parsed.rows {
        let label = format!("{}-{}", row.grade, row.class_no);
        let class_id = find_or_create_class(&tx, &label, year)?;
        classes.insert((row.grade, row.class_no));
        match upsert(&tx, row, class_id, store_rrn, now)? {
            0 => res.added += 1,
            1 => res.updated += 1,
            2 => res.merged += 1,
            _ => res.unchanged += 1,
        }
    }
    tx.commit()?;
    res.classes = classes.into_iter().map(|(g, c)| format!("{g}-{c}")).collect();
    Ok(res)
}

/// 여러 파일을 차례로 가져온다. 한 파일이 잘못되어도 나머지는 가져온다.
pub fn import_files(conn: &mut Connection, paths: &[String], store_rrn: bool, now: NaiveDateTime) -> AppResult<NeisImportResult> {
    let mut out = NeisImportResult::default();
    for p in paths {
        let path = Path::new(p);
        let file = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.clone());
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "xlsx" | "xls" | "xlsm") {
            out.files.push(FileResult { file, error: Some("엑셀 파일(.xlsx, .xls)이 아닙니다.".into()), ..Default::default() });
            continue;
        }
        match parse_file(path) {
            Ok(parsed) => {
                for pr in &parsed.problems {
                    out.problems.push(format!("{file} {pr}"));
                }
                if parsed.rows.is_empty() {
                    out.files.push(FileResult { file, error: Some("학생이 한 명도 없습니다.".into()), ..Default::default() });
                    continue;
                }
                out.files.push(import_parsed(conn, &file, &parsed, store_rrn, now)?);
            }
            Err(e) => out.files.push(FileResult { file, error: Some(e.to_string()), ..Default::default() }),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::students;
    use rust_xlsxwriter::Workbook;

    fn now() -> NaiveDateTime {
        util::parse_dt("2026-10-01T09:00:00").unwrap()
    }

    /// 나이스 명렬표 모양의 가짜 파일 (실제 학생 정보 아님)
    fn write_xlsx(path: &Path, header: &[&str], rows: &[Vec<&str>]) {
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("empty0").unwrap();
        for (c, h) in header.iter().enumerate() {
            ws.write_string(0, c as u16, *h).unwrap();
        }
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                if !v.is_empty() {
                    ws.write_string(r as u32 + 1, c as u16, *v).unwrap();
                }
            }
        }
        wb.save(path).unwrap();
    }

    const BASIC: [&str; 5] = ["학년", "반", "번호", "성명", "비고"];
    const FULL: [&str; 12] = [
        "학년",
        "반",
        "번호",
        "성명",
        "주민등록번호\r\n(외국인등록번호)",
        "성별",
        "생년월일",
        "우편번호",
        "주소",
        "이전학적정보",
        "학생개인번호",
        "비고",
    ];

    #[test]
    fn reads_basic_and_full_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("기본.xlsx");
        // 번호가 글자라서 "10"이 "2"보다 앞에 와도 숫자 순으로 정리된다
        write_xlsx(&p, &BASIC, &[vec!["2", "1", "10", "홍길동", ""], vec!["2", "1", "2", "심청", ""], vec!["", "", "", "", ""]]);
        let r = parse_file(&p).unwrap();
        assert_eq!(r.rows.iter().map(|x| (x.number, x.name.as_str())).collect::<Vec<_>>(), vec![(Some(2), "심청"), (Some(10), "홍길동")]);
        assert!(r.optional_columns.iter().all(|c| c == "비고"));
        assert!(!r.has_rrn);

        let p = dir.path().join("전체.xlsx");
        write_xlsx(
            &p,
            &FULL,
            &[vec!["3", "1", "1", "홍길동", "110217-3123456", "남성", "2011.02.17.", "", "OO도 OO군 OO읍 OO로 9, 99동 (OO리)", "2학년1반1번", "1234567890", ""]],
        );
        let r = parse_file(&p).unwrap();
        let s = &r.rows[0];
        assert_eq!((s.grade, s.class_no, s.number), (3, 1, Some(1)));
        assert_eq!(s.gender, "남");
        assert_eq!(s.birthday.as_deref(), Some("2011-02-17"));
        assert_eq!(s.rrn, "110217-3******", "뒷자리는 저장하지 않는다");
        assert_eq!(s.prev_record, "2학년1반1번");
        assert_eq!(s.student_no, "1234567890");
        assert_eq!(s.postal_code, "");
        assert!(r.has_rrn);
        assert_eq!(r.optional_columns.len(), 8);
    }

    /// E2E 시험용 가짜 명렬표 파일을 만든다 (TN_FAKE_ROSTER_DIR). 이름·번호는 모두 지어낸 값이다.
    #[test]
    #[ignore]
    fn write_fake_roster_files() {
        let Ok(dir) = std::env::var("TN_FAKE_ROSTER_DIR") else { return };
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        write_xlsx(
            &dir.join("학생명렬표.xlsx"),
            &BASIC,
            &[vec!["1", "1", "1", "심청", ""], vec!["1", "1", "2", "전우치", ""], vec!["1", "1", "10", "박흥부", ""]],
        );
        write_xlsx(&dir.join("학생명렬표 1.xlsx"), &BASIC, &[vec!["2", "1", "1", "장화", ""], vec!["2", "1", "2", "홍련", ""]]);
        write_xlsx(
            &dir.join("학생명렬표 3.xlsx"),
            &FULL,
            &[
                vec!["3", "1", "1", "남궁철수", "100101-3123456", "남성", "2010.01.01.", "58000", "OO시 OO로 1, 101동 (OO동)", "2학년1반1번", "9000000001", ""],
                vec!["3", "1", "2", "김철수", "100202-4123456", "여성", "2010.02.02.", "", "OO시 OO로 2", "2학년1반2번", "9000000002", "전입"],
            ],
        );
        // 같은 3-1 의 기본 5열 파일 (전체 열 파일과 합쳐져야 한다)
        write_xlsx(&dir.join("학생명렬표 2.xlsx"), &BASIC, &[vec!["3", "1", "1", "남궁철수", ""], vec!["3", "1", "2", "김철수", ""]]);
    }

    /// 실제 나이스 파일 확인용 (TN_ROSTER_FILES="경로|경로"). 학생 정보는 출력하지 않고 개수만 본다.
    #[test]
    #[ignore]
    fn real_files_counts_only() {
        let Ok(list) = std::env::var("TN_ROSTER_FILES") else { return };
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let paths: Vec<String> = list.split('|').map(String::from).collect();
        for round in 1..=2 {
            let r = import_files(&mut c, &paths, false, now()).unwrap();
            for f in &r.files {
                println!(
                    "[{round}회차] {} 학급 {:?} 인원 {} / 새로 {} 보탬 {} 합침 {} 그대로 {} / 선택 열 {} / 주민번호 건너뜀 {} / 오류 {:?}",
                    f.file,
                    f.classes,
                    f.total,
                    f.added,
                    f.updated,
                    f.merged,
                    f.unchanged,
                    f.optional_columns.join("·"),
                    f.rrn_skipped,
                    f.error
                );
            }
            println!("[{round}회차] 문제 {}건: {:?}", r.problems.len(), r.problems);
        }
        let q = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
        println!(
            "DB: 학생 {} / 학급 {} / 학생개인번호 {} / 성별 {} / 생년월일 {} / 주소 {} / 우편번호 {} / 이전학적 {} / 주민번호 {} / 번호 빈칸 {}",
            q("SELECT COUNT(*) FROM students"),
            q("SELECT COUNT(*) FROM classes"),
            q("SELECT COUNT(*) FROM students WHERE student_no<>''"),
            q("SELECT COUNT(*) FROM students WHERE gender<>''"),
            q("SELECT COUNT(*) FROM students WHERE birthday IS NOT NULL"),
            q("SELECT COUNT(*) FROM students WHERE address<>''"),
            q("SELECT COUNT(*) FROM students WHERE postal_code<>''"),
            q("SELECT COUNT(*) FROM students WHERE prev_record<>''"),
            q("SELECT COUNT(*) FROM students WHERE rrn<>''"),
            q("SELECT COUNT(*) FROM students WHERE number IS NULL"),
        );
    }

    #[test]
    fn rejects_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("다른표.xlsx");
        write_xlsx(&p, &["날짜", "내용"], &[vec!["10/2", "회의"]]);
        assert!(parse_file(&p).is_err());
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        let txt = dir.path().join("메모.txt");
        std::fs::write(&txt, "x").unwrap();
        let r = import_files(&mut c, &[p.to_string_lossy().into(), txt.to_string_lossy().into()], false, now()).unwrap();
        assert!(r.files.iter().all(|f| f.error.is_some()));
    }

    #[test]
    fn reimport_merges_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        // 기록에서 학급 없이 자동 등록된 학생 + 관찰 기록
        c.execute("INSERT INTO students(name, needs_review, created_at, updated_at) VALUES ('이영희', 1, 'x', 'x')", []).unwrap();
        let loose_id = c.last_insert_rowid();
        c.execute(
            "INSERT INTO observations(student_id, area, content, observed_on, source, created_at, updated_at) VALUES (?1, 'subject', '발표', '2026-09-29', 'ai', 'x', 'x')",
            params![loose_id],
        )
        .unwrap();

        // 1차: 기본 5열
        let basic = dir.path().join("학생명렬표.xlsx");
        write_xlsx(&basic, &BASIC, &[vec!["1", "1", "1", "홍길동", ""], vec!["1", "1", "2", "이영희", ""], vec!["1", "1", "3", "장화", ""]]);
        let r = import_files(&mut c, &[basic.to_string_lossy().into()], false, now()).unwrap();
        let f = &r.files[0];
        assert_eq!((f.total, f.added, f.merged), (3, 2, 1), "{f:?}");
        assert_eq!(f.classes, vec!["1-1"]);
        let s = students::get_student(&c, loose_id).unwrap();
        assert_eq!((s.class_label.as_deref(), s.number, s.needs_review, s.observation_count), (Some("1-1"), Some(2), false, 1));

        // 2차: 같은 학급 전체 12열 — 인원은 그대로, 정보만 보탠다. 빈칸은 기존 값을 지우지 않는다.
        students::set_roster_field(&c, loose_id, "address", "직접 적은 주소", now()).unwrap();
        let full = dir.path().join("학생명렬표 3.xlsx");
        write_xlsx(
            &full,
            &FULL,
            &[
                vec!["1", "1", "1", "홍길동", "120101-4123456", "여성", "2012.01.01.", "58000", "OO시 OO로 1", "", "1000000001", ""],
                vec!["1", "1", "2", "이영희", "120202-3123456", "남성", "2012.02.02.", "", "", "", "1000000002", "전입"],
                vec!["1", "1", "3", "장화", "", "", "", "", "", "", "1000000003", ""],
            ],
        );
        let r = import_files(&mut c, &[full.to_string_lossy().into()], false, now()).unwrap();
        let f = &r.files[0];
        assert_eq!((f.total, f.added, f.updated), (3, 0, 3), "{f:?}");
        assert!(f.rrn_skipped);
        let count: i64 = c.query_row("SELECT COUNT(*) FROM students", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 3);
        let s = students::get_student(&c, loose_id).unwrap();
        assert_eq!(s.address, "직접 적은 주소", "빈칸이 기존 값을 지우지 않는다");
        assert_eq!((s.gender.as_str(), s.student_no.as_str(), s.remark.as_str(), s.rrn.as_str()), ("남", "1000000002", "전입", ""));

        // 3차: 같은 파일 다시 — 바뀐 것 없음
        let r = import_files(&mut c, &[full.to_string_lossy().into()], true, now()).unwrap();
        assert_eq!(r.files[0].added, 0);
        let s = students::get_student(&c, loose_id).unwrap();
        assert_eq!(s.rrn, "120202-3******", "설정을 켜면 가린 모양으로만 저장");

        // 다음 학년도: 학생개인번호로 같은 학생을 찾아 학급을 옮긴다 (기록은 그대로)
        let next = dir.path().join("다음해.xlsx");
        write_xlsx(&next, &FULL, &[vec!["2", "1", "5", "이영희", "", "", "", "", "", "1학년1반2번", "1000000002", ""]]);
        let r = import_files(&mut c, &[next.to_string_lossy().into()], false, now()).unwrap();
        assert_eq!(r.files[0].updated, 1);
        let s = students::get_student(&c, loose_id).unwrap();
        assert_eq!((s.class_label.as_deref(), s.number, s.observation_count, s.prev_record.as_str()), (Some("2-1"), Some(5), 1, "1학년1반2번"));
    }
}
