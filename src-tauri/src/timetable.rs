//! 시간표: 학기별 일과 시간(등교·0교시·수업·쉬는 시간·점심·하교·방과후)과
//! 전체(학교) / 개인(교사) / 학급 시간표.
//! 교시 번호: 0 = 0교시, 1~10 = 정규 교시, 101~ = 방과후. (점심 100, 등교·하교는 표시용)

use crate::error::{AppError, AppResult};
use crate::util;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const LUNCH_NO: i32 = 100;
pub const AFTER_BASE: i32 = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AfterSchool {
    pub name: String,
    pub start: String,
    pub end: String,
    /// 하는 요일 (1=월 … 6=토). 비어 있으면 매일 (예전 설정과 같게)
    pub days: Vec<u8>,
}

impl Default for AfterSchool {
    fn default() -> Self {
        AfterSchool { name: String::new(), start: "16:40".into(), end: "17:30".into(), days: vec![] }
    }
}

/// 요일 목록 정리: 1~6만, 겹침 없이 차례대로
fn clean_days(days: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = days.iter().copied().filter(|d| (1..=6).contains(d)).collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BellConfig {
    /// 등교 시각
    pub arrival: String,
    /// 0교시 사용 (고등학교 등)
    pub zero_period: bool,
    pub zero_start: String,
    /// 1교시 시작
    pub first_start: String,
    /// 수업 시간(분): 40·45·50 등
    pub period_minutes: u32,
    /// 쉬는 시간(분): 5·10·15 등
    pub break_minutes: u32,
    /// 몇 교시 뒤에 점심인지 (0 = 점심 없음)
    pub lunch_after: u32,
    /// 점심 시간(분): 30~90
    pub lunch_minutes: u32,
    /// 정규 교시 수 (1~10)
    pub period_count: u32,
    /// 하교 시각 (빈 값이면 마지막 교시 끝 + 10분)
    pub dismissal: String,
    /// 방과후 (초·중·고 모두 여러 개 가능)
    pub after_school: Vec<AfterSchool>,
    /// 토요일 수업
    pub saturday: bool,
}

impl Default for BellConfig {
    fn default() -> Self {
        BellConfig {
            arrival: "08:30".into(),
            zero_period: false,
            zero_start: "07:50".into(),
            first_start: "09:00".into(),
            period_minutes: 45,
            break_minutes: 10,
            lunch_after: 4,
            lunch_minutes: 50,
            period_count: 7,
            dismissal: String::new(),
            after_school: vec![],
            saturday: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeriodSlot {
    /// 0~10 교시, 100 점심, 101~ 방과후, -1 등교, -2 하교
    pub no: i32,
    pub label: String,
    pub start: String,
    pub end: String,
    /// class | lunch | after | arrival | dismissal
    pub kind: String,
    /// 방과후를 하는 요일 (1=월 … 6=토). 비어 있으면 매일
    #[serde(default)]
    pub days: Vec<u8>,
}

impl PeriodSlot {
    /// 이 요일(1=월 …)에 있는 교시인가 (방과후는 정한 요일에만)
    pub fn on_weekday(&self, wd: u32) -> bool {
        self.days.is_empty() || self.days.contains(&(wd as u8))
    }
}

fn t(s: &str, what: &str) -> AppResult<NaiveTime> {
    util::parse_t(s).ok_or_else(|| AppError::invalid(format!("{what} 시각 형식이 올바르지 않습니다: {s} (예: 08:50)")))
}

fn add(t0: NaiveTime, minutes: u32) -> AppResult<NaiveTime> {
    let (t1, wrap) = t0.overflowing_add_signed(Duration::minutes(minutes as i64));
    if wrap != 0 {
        return Err(AppError::invalid("교시가 자정을 넘어갑니다. 시작 시각과 시간을 확인해 주세요."));
    }
    Ok(t1)
}

fn slot(no: i32, label: impl Into<String>, start: NaiveTime, end: NaiveTime, kind: &str) -> PeriodSlot {
    PeriodSlot { no, label: label.into(), start: util::fmt_t(start), end: util::fmt_t(end), kind: kind.into(), days: vec![] }
}

/// 설정값으로 교시별 시각을 계산한다.
pub fn compute_periods(c: &BellConfig) -> AppResult<Vec<PeriodSlot>> {
    if !(20..=120).contains(&c.period_minutes) {
        return Err(AppError::invalid("수업 시간은 20~120분 사이로 정해 주세요."));
    }
    if c.break_minutes > 60 {
        return Err(AppError::invalid("쉬는 시간은 60분 이하로 정해 주세요."));
    }
    if !(1..=10).contains(&c.period_count) {
        return Err(AppError::invalid("교시 수는 1~10교시 사이로 정해 주세요."));
    }
    if c.lunch_after > c.period_count {
        return Err(AppError::invalid("점심 위치가 교시 수보다 뒤에 있습니다."));
    }
    if c.lunch_after > 0 && !(10..=180).contains(&c.lunch_minutes) {
        return Err(AppError::invalid("점심 시간은 10~180분 사이로 정해 주세요."));
    }
    let mut out = Vec::new();
    if !c.arrival.trim().is_empty() {
        let a = t(&c.arrival, "등교")?;
        out.push(slot(-1, "등교", a, a, "arrival"));
    }
    let first = t(&c.first_start, "1교시 시작")?;
    if c.zero_period {
        let z = t(&c.zero_start, "0교시 시작")?;
        let ze = add(z, c.period_minutes)?;
        if ze > first {
            return Err(AppError::invalid("0교시가 1교시 시작 전에 끝나지 않습니다."));
        }
        out.push(slot(0, "0교시", z, ze, "class"));
    }
    let mut cur = first;
    let mut last_end = first;
    for i in 1..=c.period_count {
        let end = add(cur, c.period_minutes)?;
        out.push(slot(i as i32, format!("{i}교시"), cur, end, "class"));
        last_end = end;
        if i == c.lunch_after && c.lunch_minutes > 0 {
            let le = add(end, c.lunch_minutes)?;
            out.push(slot(LUNCH_NO, "점심", end, le, "lunch"));
            cur = le;
        } else {
            cur = add(end, c.break_minutes)?;
        }
    }
    let dismissal = if c.dismissal.trim().is_empty() { add(last_end, 10)? } else { t(&c.dismissal, "하교")? };
    out.push(slot(-2, "하교", dismissal, dismissal, "dismissal"));
    for (k, a) in c.after_school.iter().enumerate() {
        let s = t(&a.start, "방과후 시작")?;
        let e = t(&a.end, "방과후 끝")?;
        if e <= s {
            return Err(AppError::invalid("방과후 끝 시각이 시작보다 빠릅니다."));
        }
        let name = if a.name.trim().is_empty() { format!("방과후 {}", k + 1) } else { a.name.trim().to_string() };
        let mut sl = slot(AFTER_BASE + 1 + k as i32, name, s, e, "after");
        sl.days = clean_days(&a.days);
        out.push(sl);
    }
    Ok(out)
}

fn check_periods(p: &[PeriodSlot]) -> AppResult<()> {
    for s in p {
        let a = t(&s.start, &s.label)?;
        let b = t(&s.end, &s.label)?;
        if b < a {
            return Err(AppError::invalid(format!("{}의 끝 시각이 시작보다 빠릅니다.", s.label)));
        }
    }
    Ok(())
}

/// 학년도·학기: 3~8월 1학기, 9~2월 2학기
pub fn semester_of(d: NaiveDate) -> (i32, u8) {
    (util::school_year(d), if (3..=8).contains(&d.month()) { 1 } else { 2 })
}

#[derive(Debug, Clone, Serialize)]
pub struct BellSchedule {
    pub school_year: i32,
    pub semester: u8,
    pub config: BellConfig,
    pub periods: Vec<PeriodSlot>,
    pub saved: bool,
    pub updated_at: Option<String>,
}

pub fn get_bell(conn: &Connection, year: i32, sem: u8) -> AppResult<BellSchedule> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT config, periods, updated_at FROM bell_schedules WHERE school_year=?1 AND semester=?2",
            params![year, sem],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(match row {
        Some((c, p, u)) => BellSchedule {
            school_year: year,
            semester: sem,
            config: serde_json::from_str(&c).unwrap_or_default(),
            periods: serde_json::from_str(&p).unwrap_or_default(),
            saved: true,
            updated_at: Some(u),
        },
        None => {
            let config = BellConfig::default();
            let periods = compute_periods(&config)?;
            BellSchedule { school_year: year, semester: sem, config, periods, saved: false, updated_at: None }
        }
    })
}

/// 일과 시간 저장. periods 를 주지 않으면 설정값으로 계산한다.
pub fn save_bell(conn: &Connection, year: i32, sem: u8, config: &BellConfig, periods: Option<Vec<PeriodSlot>>, now: NaiveDateTime) -> AppResult<BellSchedule> {
    check_sem(sem)?;
    let mut periods = match periods {
        Some(p) if !p.is_empty() => p,
        _ => compute_periods(config)?,
    };
    // 교시 시각을 직접 고쳐 저장해도 방과후 요일은 설정값을 따른다
    for (k, a) in config.after_school.iter().enumerate() {
        if let Some(sl) = periods.iter_mut().find(|p| p.kind == "after" && p.no == AFTER_BASE + 1 + k as i32) {
            sl.days = clean_days(&a.days);
        }
    }
    check_periods(&periods)?;
    conn.execute(
        "INSERT INTO bell_schedules(school_year, semester, config, periods, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(school_year, semester) DO UPDATE SET config=excluded.config, periods=excluded.periods, updated_at=excluded.updated_at",
        params![year, sem, serde_json::to_string(config)?, serde_json::to_string(&periods)?, util::fmt_dt(now)],
    )?;
    get_bell(conn, year, sem)
}

fn check_sem(sem: u8) -> AppResult<()> {
    if sem == 1 || sem == 2 { Ok(()) } else { Err(AppError::invalid("학기는 1 또는 2입니다.")) }
}

fn check_kind(kind: &str) -> AppResult<()> {
    if matches!(kind, "school" | "personal" | "class") { Ok(()) } else { Err(AppError::invalid("시간표 종류가 올바르지 않습니다.")) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Cell {
    /// 전체 시간표의 행(학급). 개인·학급 시간표는 빈 값
    pub row_label: String,
    /// 1=월 … 6=토
    pub weekday: i64,
    pub period: i64,
    pub subject: String,
    pub teacher: String,
    /// 개인 시간표에서 들어가는 학급
    pub class_label: String,
    pub room: String,
    pub memo: String,
    /// '' = 교사가 입력, 'neis' = 나이스에서 받음
    pub source: String,
}

impl Cell {
    fn is_empty(&self) -> bool {
        [&self.subject, &self.teacher, &self.class_label, &self.room, &self.memo].iter().all(|s| s.trim().is_empty())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TimetableView {
    pub school_year: i32,
    pub semester: u8,
    pub kind: String,
    pub class_label: String,
    pub cells: Vec<Cell>,
    /// 전체 시간표의 행(학급) 목록
    pub rows: Vec<String>,
    pub updated_at: Option<String>,
}

fn timetable_id(conn: &Connection, year: i32, sem: u8, kind: &str, class_label: &str) -> AppResult<Option<(i64, String)>> {
    Ok(conn
        .query_row(
            "SELECT id, updated_at FROM timetables WHERE school_year=?1 AND semester=?2 AND kind=?3 AND class_label=?4",
            params![year, sem, kind, class_label],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

fn ensure_timetable(conn: &Connection, year: i32, sem: u8, kind: &str, class_label: &str, now: NaiveDateTime) -> AppResult<i64> {
    if let Some((id, _)) = timetable_id(conn, year, sem, kind, class_label)? {
        conn.execute("UPDATE timetables SET updated_at=?2 WHERE id=?1", params![id, util::fmt_dt(now)])?;
        return Ok(id);
    }
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO timetables(school_year, semester, kind, class_label, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![year, sem, kind, class_label, now_s],
    )?;
    Ok(conn.last_insert_rowid())
}

fn norm_class(kind: &str, class_label: &str) -> AppResult<String> {
    if kind != "class" {
        return Ok(String::new());
    }
    let l = class_label.trim();
    let (g, n) = crate::students::parse_class_label(l).ok_or_else(|| AppError::invalid(format!("학급 표기를 이해하지 못했습니다: {l} (예: 2-1)")))?;
    Ok(format!("{g}-{n}"))
}

fn load_cells(conn: &Connection, id: i64) -> AppResult<Vec<Cell>> {
    let mut st = conn.prepare(
        "SELECT row_label, weekday, period, subject, teacher, class_label, room, memo, source FROM timetable_cells WHERE timetable_id=?1
         ORDER BY row_label, weekday, period",
    )?;
    let rows = st
        .query_map(params![id], |r| {
            Ok(Cell {
                row_label: r.get(0)?,
                weekday: r.get(1)?,
                period: r.get(2)?,
                subject: r.get(3)?,
                teacher: r.get(4)?,
                class_label: r.get(5)?,
                room: r.get(6)?,
                memo: r.get(7)?,
                source: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_timetable(conn: &Connection, year: i32, sem: u8, kind: &str, class_label: &str) -> AppResult<TimetableView> {
    check_sem(sem)?;
    check_kind(kind)?;
    let label = if kind == "class" && class_label.trim().is_empty() { String::new() } else { norm_class(kind, class_label)? };
    let found = timetable_id(conn, year, sem, kind, &label)?;
    let cells = match &found {
        Some((id, _)) => load_cells(conn, *id)?,
        None => vec![],
    };
    let mut rows: Vec<String> = Vec::new();
    if kind == "school" {
        // 그 학년도 학급 + 학급 시간표가 있는 학급 + 이미 칸이 있는 행
        let mut st = conn.prepare("SELECT label FROM classes WHERE school_year=?1 ORDER BY grade, class_no")?;
        rows.extend(st.query_map(params![year], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?);
        for l in class_timetable_labels(conn, year, sem)? {
            if !rows.contains(&l) {
                rows.push(l);
            }
        }
        for c in &cells {
            if !c.row_label.is_empty() && !rows.contains(&c.row_label) {
                rows.push(c.row_label.clone());
            }
        }
        rows.sort_by_key(|l| crate::students::parse_class_label(l).unwrap_or((99, 99)));
    }
    Ok(TimetableView { school_year: year, semester: sem, kind: kind.into(), class_label: label, cells, rows, updated_at: found.map(|f| f.1) })
}

pub fn class_timetable_labels(conn: &Connection, year: i32, sem: u8) -> AppResult<Vec<String>> {
    let mut st = conn.prepare("SELECT class_label FROM timetables WHERE school_year=?1 AND semester=?2 AND kind='class' ORDER BY class_label")?;
    let mut v = st.query_map(params![year, sem], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    v.sort_by_key(|l| crate::students::parse_class_label(l).unwrap_or((99, 99)));
    Ok(v)
}

/// 칸 하나를 저장한다. 내용이 모두 비어 있으면 그 칸을 지운다.
pub fn set_cell(conn: &Connection, year: i32, sem: u8, kind: &str, class_label: &str, cell: &Cell, now: NaiveDateTime) -> AppResult<()> {
    check_sem(sem)?;
    check_kind(kind)?;
    if !(1..=6).contains(&cell.weekday) {
        return Err(AppError::invalid("요일이 올바르지 않습니다."));
    }
    if !(0..=10).contains(&cell.period) && !(AFTER_BASE + 1..=AFTER_BASE + 20).contains(&(cell.period as i32)) {
        return Err(AppError::invalid("교시가 올바르지 않습니다."));
    }
    let label = norm_class(kind, class_label)?;
    let row = if kind == "school" {
        let r = cell.row_label.trim();
        let (g, n) = crate::students::parse_class_label(r).ok_or_else(|| AppError::invalid("전체 시간표의 학급을 확인해 주세요 (예: 2-1)."))?;
        format!("{g}-{n}")
    } else {
        String::new()
    };
    let id = ensure_timetable(conn, year, sem, kind, &label, now)?;
    if cell.is_empty() {
        conn.execute(
            "DELETE FROM timetable_cells WHERE timetable_id=?1 AND row_label=?2 AND weekday=?3 AND period=?4",
            params![id, row, cell.weekday, cell.period],
        )?;
        return Ok(());
    }
    let clip = |s: &str, n: usize| s.trim().chars().take(n).collect::<String>();
    conn.execute(
        "INSERT INTO timetable_cells(timetable_id, row_label, weekday, period, subject, teacher, class_label, room, memo)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(timetable_id, row_label, weekday, period) DO UPDATE SET subject=excluded.subject, teacher=excluded.teacher,
           class_label=excluded.class_label, room=excluded.room, memo=excluded.memo, source=''",
        params![
            id,
            row,
            cell.weekday,
            cell.period,
            clip(&cell.subject, 30),
            clip(&cell.teacher, 30),
            clip(&cell.class_label, 20),
            clip(&cell.room, 30),
            clip(&cell.memo, 200)
        ],
    )?;
    Ok(())
}

/// 나이스에서 받은 칸으로 바꾼다: 예전에 받은 칸('neis')만 지우고 새로 넣는다. 교사가 입력한 칸이 있는 자리는 그대로 둔다.
pub fn replace_neis_cells(conn: &Connection, year: i32, sem: u8, kind: &str, class_label: &str, cells: &[Cell], now: NaiveDateTime) -> AppResult<usize> {
    check_sem(sem)?;
    check_kind(kind)?;
    let label = norm_class(kind, class_label)?;
    let id = ensure_timetable(conn, year, sem, kind, &label, now)?;
    conn.execute("DELETE FROM timetable_cells WHERE timetable_id=?1 AND source='neis'", params![id])?;
    let mut n = 0;
    for c in cells {
        n += conn.execute(
            "INSERT OR IGNORE INTO timetable_cells(timetable_id, row_label, weekday, period, subject, teacher, class_label, room, memo, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', 'neis')",
            params![id, c.row_label, c.weekday, c.period, c.subject, c.teacher, c.class_label, c.room],
        )?;
    }
    Ok(n)
}

/// 시간표 칸 색 (15가지). 이름만 저장하고 실제 색은 화면이 정한다.
pub const TT_COLORS: [&str; 15] = [
    "red", "orange", "amber", "yellow", "lime", "green", "teal", "cyan", "sky", "blue", "indigo", "purple", "pink", "brown", "gray",
];

/// 색은 칸 → 교과 → 교사 → 학급 → 교실 순으로 정한다 (화면이 이 순서로 찾는다)
#[derive(Debug, Clone, Serialize, Default)]
pub struct TtColors {
    /// "학년도-학기|종류|시간표학급|행|요일|교시" → 색
    pub cells: BTreeMap<String, String>,
    /// "국어" → 색
    pub subjects: BTreeMap<String, String>,
    /// "김선생" → 색
    pub teachers: BTreeMap<String, String>,
    /// "2-1" → 색
    pub classes: BTreeMap<String, String>,
    /// "과학실" → 색
    pub rooms: BTreeMap<String, String>,
}

pub fn colors(conn: &Connection) -> AppResult<TtColors> {
    let mut out = TtColors::default();
    let mut st = conn.prepare("SELECT scope, target, color FROM timetable_colors")?;
    for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
        let (scope, target, color) = row?;
        match scope.as_str() {
            "cell" => out.cells.insert(target, color),
            "subject" => out.subjects.insert(target, color),
            "teacher" => out.teachers.insert(target, color),
            "class" => out.classes.insert(target, color),
            _ => out.rooms.insert(target, color),
        };
    }
    Ok(out)
}

/// 색 정하기 (color 가 None 이면 '없음'으로 되돌린다)
pub fn set_color(conn: &Connection, scope: &str, target: &str, color: Option<&str>) -> AppResult<()> {
    if !matches!(scope, "cell" | "subject" | "teacher" | "class" | "room") {
        return Err(AppError::invalid("색을 정할 대상이 올바르지 않습니다."));
    }
    let target = target.trim();
    if target.is_empty() || target.chars().count() > 80 {
        return Err(AppError::invalid("색을 정할 칸·교과·교사·학급·교실을 확인해 주세요."));
    }
    let target = if scope == "class" {
        let (g, n) = crate::students::parse_class_label(target).ok_or_else(|| AppError::invalid("학급 표기를 확인해 주세요 (예: 2-1)."))?;
        format!("{g}-{n}")
    } else {
        target.to_string()
    };
    match color {
        None | Some("") => {
            conn.execute("DELETE FROM timetable_colors WHERE scope=?1 AND target=?2", params![scope, target])?;
        }
        Some(c) if TT_COLORS.contains(&c) => {
            conn.execute(
                "INSERT INTO timetable_colors(scope, target, color) VALUES (?1, ?2, ?3) ON CONFLICT(scope, target) DO UPDATE SET color=excluded.color",
                params![scope, target, c],
            )?;
        }
        Some(_) => return Err(AppError::invalid("고를 수 없는 색입니다.")),
    }
    Ok(())
}

/// 전체 시간표에 학급(행)을 더한다 (학급 목록에 등록)
pub fn add_school_row(conn: &Connection, year: i32, label: &str) -> AppResult<String> {
    let id = crate::students::find_or_create_class(conn, label, year)?;
    Ok(conn.query_row("SELECT label FROM classes WHERE id=?1", params![id], |r| r.get(0))?)
}

pub fn delete_timetable(conn: &Connection, year: i32, sem: u8, kind: &str, class_label: &str) -> AppResult<()> {
    let label = norm_class(kind, class_label)?;
    conn.execute(
        "DELETE FROM timetables WHERE school_year=?1 AND semester=?2 AND kind=?3 AND class_label=?4",
        params![year, sem, kind, label],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct CopyResult {
    pub timetables: usize,
    pub cells: usize,
    pub bell: bool,
}

/// 다른 학기의 일과 시간과 시간표를 복사한다. 대상 학기에 이미 있는 시간표는 건드리지 않는다.
pub fn copy_semester(conn: &mut Connection, from: (i32, u8), to: (i32, u8), now: NaiveDateTime) -> AppResult<CopyResult> {
    check_sem(from.1)?;
    check_sem(to.1)?;
    if from == to {
        return Err(AppError::invalid("같은 학기로는 복사할 수 없습니다."));
    }
    let tx = conn.transaction()?;
    let mut res = CopyResult { timetables: 0, cells: 0, bell: false };
    let has_bell: bool = tx
        .query_row("SELECT 1 FROM bell_schedules WHERE school_year=?1 AND semester=?2", params![to.0, to.1], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if !has_bell {
        let n = tx.execute(
            "INSERT INTO bell_schedules(school_year, semester, config, periods, updated_at)
             SELECT ?3, ?4, config, periods, ?5 FROM bell_schedules WHERE school_year=?1 AND semester=?2",
            params![from.0, from.1, to.0, to.1, util::fmt_dt(now)],
        )?;
        res.bell = n > 0;
    }
    let src: Vec<(i64, String, String)> = {
        let mut st = tx.prepare("SELECT id, kind, class_label FROM timetables WHERE school_year=?1 AND semester=?2")?;
        st.query_map(params![from.0, from.1], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    for (sid, kind, label) in src {
        if timetable_id(&tx, to.0, to.1, &kind, &label)?.is_some() {
            continue;
        }
        let tid = ensure_timetable(&tx, to.0, to.1, &kind, &label, now)?;
        res.cells += tx.execute(
            "INSERT INTO timetable_cells(timetable_id, row_label, weekday, period, subject, teacher, class_label, room, memo, source)
             SELECT ?2, row_label, weekday, period, subject, teacher, class_label, room, memo, source FROM timetable_cells WHERE timetable_id=?1",
            params![sid, tid],
        )?;
        res.timetables += 1;
    }
    tx.commit()?;
    Ok(res)
}

#[derive(Debug, Clone, Serialize)]
pub struct TodaySlot {
    pub period: PeriodSlot,
    pub cell: Option<Cell>,
    /// before | now | after — 지금 시각 기준
    pub status: String,
    /// 기록에서 찾은 오늘 하루의 시간표 변경 (보강·교체·자습·휴강 …)
    pub change: Option<crate::ttinput::TtEntry>,
}

/// 지금 쓰는 학기: 달력 기준 학기. 단 2월에 새 학년도를 시작했으면 새 학년도 1학기.
pub fn active_semester(conn: &Connection, today: NaiveDate) -> (i32, u8) {
    let (y, s) = semester_of(today);
    let active = crate::students::active_school_year(conn, today);
    if active > y { (active, 1) } else { (y, s) }
}

#[derive(Debug, Clone, Serialize)]
pub struct TodayTimetable {
    pub school_year: i32,
    pub semester: u8,
    pub weekday: u32,
    pub has_timetable: bool,
    pub slots: Vec<TodaySlot>,
}

/// 오늘의 개인 시간표 (주말이면 비어 있다)
pub fn today(conn: &Connection, now: NaiveDateTime) -> AppResult<TodayTimetable> {
    let d = now.date();
    let (year, sem) = semester_of(d);
    let wd = d.weekday().number_from_monday();
    let bell = get_bell(conn, year, sem)?;
    let personal = get_timetable(conn, year, sem, "personal", "")?;
    let changes = crate::ttinput::changes_on(conn, d)?;
    let has = !personal.cells.is_empty() || !changes.is_empty();
    let school_day = wd <= 5 || (wd == 6 && bell.config.saturday) || !changes.is_empty();
    let mut slots = Vec::new();
    if school_day {
        let nt = now.time();
        for p in bell.periods.iter().filter(|p| p.kind == "class" || p.kind == "after" || p.kind == "lunch") {
            let cell = personal.cells.iter().find(|c| c.weekday == wd as i64 && c.period == p.no as i64).cloned();
            // 같은 교시에 변경이 여럿이면 나중에 적은 것
            let change = changes.iter().rev().find(|c| c.period == p.no as i64).cloned();
            // 방과후는 정한 요일에만, 그리고 그 칸에 수업이 있을 때만 보인다 (그날 변경이 있으면 보인다)
            if p.kind == "after" && ((cell.is_none() && change.is_none()) || (!p.on_weekday(wd) && change.is_none())) {
                continue;
            }
            let (s, e) = (util::parse_t(&p.start), util::parse_t(&p.end));
            let status = match (s, e) {
                (Some(s), Some(e)) if nt >= s && nt < e => "now",
                (Some(_), Some(e)) if nt >= e => "after",
                _ => "before",
            };
            slots.push(TodaySlot { period: p.clone(), cell, status: status.into(), change });
        }
    }
    Ok(TodayTimetable { school_year: year, semester: sem, weekday: wd, has_timetable: has, slots })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn now() -> NaiveDateTime {
        util::parse_dt("2026-09-29T10:15:00").unwrap()
    }

    #[test]
    fn default_middle_school_bell() {
        let p = compute_periods(&BellConfig::default()).unwrap();
        let get = |no: i32| p.iter().find(|s| s.no == no).unwrap();
        assert_eq!((get(1).start.as_str(), get(1).end.as_str()), ("09:00", "09:45"));
        assert_eq!(get(2).start, "09:55");
        assert_eq!(get(4).end, "12:30");
        assert_eq!((get(LUNCH_NO).start.as_str(), get(LUNCH_NO).end.as_str()), ("12:30", "13:20"));
        assert_eq!(get(5).start, "13:20");
        assert_eq!(get(7).end, "15:55");
        assert_eq!(get(-2).start, "16:05", "하교 = 마지막 교시 + 10분");
        assert!(p.iter().all(|s| s.no != 0));
    }

    #[test]
    fn high_school_with_zero_period_and_after_school() {
        let c = BellConfig {
            zero_period: true,
            zero_start: "07:40".into(),
            first_start: "08:30".into(),
            period_minutes: 50,
            break_minutes: 10,
            lunch_after: 4,
            lunch_minutes: 60,
            period_count: 10,
            dismissal: "19:20".into(),
            after_school: vec![AfterSchool { name: "야간 자율학습".into(), start: "19:00".into(), end: "21:00".into(), days: vec![] }],
            ..Default::default()
        };
        let p = compute_periods(&c).unwrap();
        assert_eq!(p.iter().find(|s| s.no == 0).unwrap().end, "08:30");
        assert_eq!(p.iter().filter(|s| s.kind == "class").count(), 11);
        assert_eq!(p.iter().find(|s| s.no == 10).unwrap().end, "19:10");
        let a = p.iter().find(|s| s.kind == "after").unwrap();
        assert!(a.days.is_empty() && a.on_weekday(3), "요일을 정하지 않은 방과후는 매일");
        assert_eq!((a.no, a.label.as_str()), (101, "야간 자율학습"));
        // 0교시가 1교시와 겹치면 오류
        assert!(compute_periods(&BellConfig { zero_period: true, zero_start: "08:30".into(), ..BellConfig::default() }).is_err());
        assert!(compute_periods(&BellConfig { period_count: 11, ..BellConfig::default() }).is_err());
        assert!(compute_periods(&BellConfig { lunch_minutes: 90, lunch_after: 3, period_minutes: 40, break_minutes: 5, ..BellConfig::default() }).is_ok());
    }

    #[test]
    fn after_school_on_chosen_weekdays() {
        // 월: 7·8교시(방과후 1·2), 화·목: 8교시(방과후 2)만
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let cfg = BellConfig {
            period_count: 6,
            after_school: vec![
                AfterSchool { name: "7교시".into(), start: "15:00".into(), end: "15:45".into(), days: vec![1] },
                AfterSchool { name: "8교시".into(), start: "15:55".into(), end: "16:40".into(), days: vec![4, 2, 1, 9] },
            ],
            ..BellConfig::default()
        };
        let b = save_bell(&c, 2026, 2, &cfg, None, now()).unwrap();
        let after: Vec<(&str, Vec<u8>)> = b.periods.iter().filter(|p| p.kind == "after").map(|p| (p.label.as_str(), p.days.clone())).collect();
        assert_eq!(after, vec![("7교시", vec![1]), ("8교시", vec![1, 2, 4])], "요일은 1~6만, 차례대로");
        let cell = |wd, p, subj: &str| Cell { weekday: wd, period: p, subject: subj.into(), ..Default::default() };
        set_cell(&c, 2026, 2, "personal", "", &cell(1, 101, "방과후 수학"), now()).unwrap();
        set_cell(&c, 2026, 2, "personal", "", &cell(1, 102, "방과후 영어"), now()).unwrap();
        set_cell(&c, 2026, 2, "personal", "", &cell(2, 102, "방과후 영어"), now()).unwrap();
        set_cell(&c, 2026, 2, "personal", "", &cell(3, 101, "예전 칸"), now()).unwrap();
        let after_today = |d: &str| -> Vec<String> {
            today(&c, util::parse_dt(&format!("{d}T08:00:00")).unwrap()).unwrap().slots.iter().filter(|s| s.period.kind == "after").map(|s| s.period.label.clone()).collect()
        };
        assert_eq!(after_today("2026-09-28"), vec!["7교시", "8교시"], "월요일");
        assert_eq!(after_today("2026-09-29"), vec!["8교시"], "화요일");
        assert!(after_today("2026-09-30").is_empty(), "수요일에는 방과후가 없다 (예전에 적은 칸이 있어도)");
        // 교시 시각을 직접 고쳐 저장해도 요일은 설정을 따른다
        let mut edited = b.periods.clone();
        for p in edited.iter_mut() {
            p.days.clear();
        }
        let b2 = save_bell(&c, 2026, 2, &cfg, Some(edited), now()).unwrap();
        assert_eq!(b2.periods.iter().find(|p| p.label == "7교시").unwrap().days, vec![1]);
    }
    #[test]
    fn semesters() {
        let d = |s: &str| util::parse_d(s).unwrap();
        assert_eq!(semester_of(d("2026-03-02")), (2026, 1));
        assert_eq!(semester_of(d("2026-09-29")), (2026, 2));
        assert_eq!(semester_of(d("2027-02-10")), (2026, 2));
    }

    #[test]
    fn colors_set_and_reset() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        set_color(&c, "class", "2학년 1반", Some("blue")).unwrap();
        set_color(&c, "room", "과학실", Some("green")).unwrap();
        set_color(&c, "subject", "국어", Some("pink")).unwrap();
        set_color(&c, "teacher", "김선생", Some("teal")).unwrap();
        let v = colors(&c).unwrap();
        assert_eq!((v.subjects.get("국어").map(String::as_str), v.teachers.get("김선생").map(String::as_str)), (Some("pink"), Some("teal")));
        set_color(&c, "cell", "2026-2|personal||-|1|3", Some("red")).unwrap();
        assert!(set_color(&c, "class", "2-1", Some("rainbow")).is_err());
        assert!(set_color(&c, "desk", "x", Some("red")).is_err());
        let v = colors(&c).unwrap();
        assert_eq!(v.classes.get("2-1").map(String::as_str), Some("blue"));
        assert_eq!(v.rooms.len(), 1);
        set_color(&c, "class", "2-1", None).unwrap();
        assert!(colors(&c).unwrap().classes.is_empty());
    }
    #[test]
    fn cells_views_today_and_copy() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.lock();
        // 화요일 2교시 = 10:15 진행 중 (09:55~10:40)
        let cell = |wd, p, subj: &str, cls: &str| Cell { weekday: wd, period: p, subject: subj.into(), class_label: cls.into(), ..Default::default() };
        set_cell(&c, 2026, 2, "personal", "", &cell(2, 2, "국어", "2-1"), now()).unwrap();
        set_cell(&c, 2026, 2, "personal", "", &cell(2, 5, "국어", "2-3"), now()).unwrap();
        set_cell(&c, 2026, 2, "class", "2학년 1반", &cell(1, 1, "수학", ""), now()).unwrap();
        let sch = Cell { row_label: "2-1".into(), weekday: 1, period: 1, subject: "수학".into(), teacher: "박".into(), ..Default::default() };
        set_cell(&c, 2026, 2, "school", "", &sch, now()).unwrap();
        assert!(set_cell(&c, 2026, 2, "personal", "", &cell(7, 1, "x", ""), now()).is_err());
        let v = get_timetable(&c, 2026, 2, "personal", "").unwrap();
        assert_eq!(v.cells.len(), 2);
        let cl = get_timetable(&c, 2026, 2, "class", "2-1").unwrap();
        assert_eq!(cl.class_label, "2-1");
        assert_eq!(cl.cells[0].subject, "수학");
        assert_eq!(class_timetable_labels(&c, 2026, 2).unwrap(), vec!["2-1".to_string()]);
        let school = get_timetable(&c, 2026, 2, "school", "").unwrap();
        assert!(school.rows.contains(&"2-1".to_string()));
        // 빈 칸으로 저장하면 지워진다
        set_cell(&c, 2026, 2, "personal", "", &cell(2, 5, "", ""), now()).unwrap();
        assert_eq!(get_timetable(&c, 2026, 2, "personal", "").unwrap().cells.len(), 1);
        // 오늘 시간표
        let t = today(&c, now()).unwrap();
        assert_eq!(t.weekday, 2);
        assert!(t.has_timetable);
        let now_slot = t.slots.iter().find(|s| s.status == "now").unwrap();
        assert_eq!(now_slot.period.no, 2);
        assert_eq!(now_slot.cell.as_ref().unwrap().class_label, "2-1");
        // 다음 학기로 복사
        save_bell(&c, 2026, 2, &BellConfig { period_minutes: 50, ..BellConfig::default() }, None, now()).unwrap();
        let r = copy_semester(&mut c, (2026, 2), (2027, 1), now()).unwrap();
        assert!(r.bell);
        assert_eq!(r.timetables, 3);
        assert_eq!(get_bell(&c, 2027, 1).unwrap().config.period_minutes, 50);
        assert_eq!(get_timetable(&c, 2027, 1, "personal", "").unwrap().cells.len(), 1);
        // 이미 있는 대상은 덮어쓰지 않는다
        let r2 = copy_semester(&mut c, (2026, 2), (2027, 1), now()).unwrap();
        assert_eq!(r2.timetables, 0);
    }
}
