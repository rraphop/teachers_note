//! 기록 입력창에 적은 시간표 문장을 알아보고 시간표에 반영한다 (0.5).
//!
//! - 정규 시간표: "월 3교시 2-1 국어 과학실", "화·목 1교시 3-2 수학", "매주 수요일 5·6교시 동아리"
//!   → 지금 쓰는 학기의 개인 시간표 칸. 빈 칸은 바로 채우고, 다른 수업이 있는 칸은 덮어쓰지 않고 교사에게 묻는다.
//! - 하루짜리 변경: "내일 3교시 2-1 보강", "10월 5일 2교시 3-1과 교체", "금요일 4교시 자습"
//!   → 원래 시간표는 그대로 두고 그 날짜의 '시간표 변경'으로 저장한다 (오늘 화면·주간/월간 일정표에 표시).
//! - 학생 이름·출결 사실·지난 일(~했음)·할 일 표현이 있는 문장은 시간표로 보지 않는다
//!   ("3교시 홍길동 지각"은 지금처럼 출결 기록).
//!
//! AI 없이 규칙으로만 알아본다. 같은 문장은 학생 관찰·일정·할 일로 따로 만들지 않는다 (classify 에서 걸러냄).

use crate::analysis::{self, Analysis};
use crate::error::{AppError, AppResult};
use crate::text::{self, Span};
use crate::timetable::{self, Cell};
use crate::util;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq)]
pub enum TtKind {
    Regular,
    Change,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TtSlot {
    pub period: i64,
    pub class_label: String,
    pub subject: String,
    pub room: String,
}

#[derive(Debug, Clone)]
pub struct TtStatement {
    pub span: Span,
    pub kind: TtKind,
    /// 정규 시간표: 요일 (1=월 … 6=토) — 교시마다 다를 수 있어 slot 과 짝을 이룬다
    pub slots: Vec<(Vec<u8>, TtSlot)>,
    /// 하루짜리 변경의 날짜
    pub date: Option<NaiveDate>,
    /// 보강 | 교체 | 자습 | 휴강 | 대강 | 변경
    pub change_type: String,
    pub text: String,
}

static RE_PERIOD: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{1,2}(?:\s*[·,~∼\-]\s*\d{1,2})*)\s*교시").unwrap());
static RE_AFTER: Lazy<Regex> = Lazy::new(|| Regex::new(r"방과\s*후").unwrap());
static RE_CLASS: Lazy<Regex> = Lazy::new(|| Regex::new(r"([1-6])\s*(?:-|학년\s*)\s*(\d{1,2})\s*반?").unwrap());
static RE_PAREN: Lazy<Regex> = Lazy::new(|| Regex::new(r"[(（]([^)）]{1,20})[)）]").unwrap());
static RE_CHANGE: Lazy<Regex> = Lazy::new(|| Regex::new(r"보강|교체|자습|결강|휴강|대강|대체|변경|바뀜|바뀌|바꿈|취소|수업\s*없음").unwrap());
static RE_BARE_WEEKDAY: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[월화수목금토일]요일$").unwrap());
static RE_ROOM_WORD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[가-힣A-Za-z0-9\-]{0,10}(?:실|강당|운동장|도서관|교실|체육관|관|터)$").unwrap()
});

const SUBJECTS: &[&str] = &[
    "기술·가정", "기술가정", "통합과학", "통합사회", "생명과학", "지구과학", "확률과 통계", "언어와 매체", "화법과 작문", "영어회화",
    "한국사", "물리학", "미적분", "일본어", "중국어", "스포츠", "동아리", "방과후", "국어", "영어", "수학", "과학", "사회", "역사",
    "도덕", "음악", "미술", "체육", "기술", "가정", "정보", "한문", "진로", "창체", "자율", "봉사", "자습", "보충", "특강", "화학",
    "물리", "기하", "문학", "독서", "통합",
];

/// 알아본 뒤 남은 낱말 중 과목이 아닌 것
const FILLER: &[&str] = &[
    "매주", "수업", "시간", "시간표", "그리고", "학년", "반", "및", "또", "이번", "다음", "주", "오늘", "내일", "모레", "요일", "교실에서",
    "입니다", "있음", "있다", "합니다", "함", "임", "들어감", "들어가요", "해요", "하기", "하고", "은", "는", "이", "가", "을", "를", "에",
    "도", "의", "와", "과", "때", "부터", "까지",
];

fn change_type(word: &str) -> &'static str {
    match word {
        w if w.starts_with("보강") => "보강",
        w if w.starts_with("교체") || w.starts_with("바꿈") => "교체",
        w if w.starts_with("자습") => "자습",
        w if w.starts_with("결강") || w.starts_with("휴강") || w.starts_with("취소") || w.contains("없음") => "휴강",
        w if w.starts_with("대강") || w.starts_with("대체") => "대강",
        _ => "변경",
    }
}

fn is_hangul(c: char) -> bool {
    ('가'..='힣').contains(&c)
}

fn wd_of(c: char) -> Option<u8> {
    match c {
        '월' => Some(1),
        '화' => Some(2),
        '수' => Some(3),
        '목' => Some(4),
        '금' => Some(5),
        '토' => Some(6),
        _ => None,
    }
}

/// 요일 묶음: "월", "월요일", "화·목", "화목", "월, 수, 금" (앞이 숫자·한글이면 "10월"·"수학" 같은 낱말이라 제외)
fn weekday_runs(s: &str) -> Vec<(usize, usize, Vec<u8>)> {
    let cs: Vec<(usize, char)> = s.char_indices().collect();
    let n = cs.len();
    let ch = |k: usize| cs.get(k).map(|c| c.1);
    let byte = |k: usize| cs.get(k).map(|c| c.0).unwrap_or(s.len());
    // k 위치 글자가 요일 낱말이면 (요일 번호, 낱말 다음 위치). "월요일", 또는 뒤가 한글이 아닌 "월",
    // 또는 "화목"처럼 요일 글자가 붙은 경우만 인정한다 ("수학"·"토론"·"월간"은 아님)
    let token = |k: usize| -> Option<(u8, usize)> {
        let d = wd_of(ch(k)?)?;
        if ch(k + 1) == Some('요') && ch(k + 2) == Some('일') {
            return Some((d, k + 3));
        }
        match ch(k + 1) {
            None => Some((d, k + 1)),
            Some(c) if !is_hangul(c) || wd_of(c).is_some() || matches!(c, '와' | '과' | '및') => Some((d, k + 1)),
            _ => None,
        }
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        // 앞이 숫자·한글이면 "10월", "화요"처럼 다른 낱말의 일부
        let prev_ok = i == 0 || !(is_hangul(cs[i - 1].1) || cs[i - 1].1.is_ascii_digit());
        let Some((d0, mut end)) = (if prev_ok { token(i) } else { None }) else {
            i += 1;
            continue;
        };
        let mut days = vec![d0];
        loop {
            let mut m = end;
            while m < n && matches!(cs[m].1, '·' | ',' | '/' | ' ' | '와' | '과' | '및') {
                m += 1;
            }
            match token(m) {
                Some((d, e)) => {
                    days.push(d);
                    end = e;
                }
                None => break,
            }
        }
        days.sort_unstable();
        days.dedup();
        out.push((cs[i].0, byte(end), days));
        i = end;
    }
    out
}

fn periods_of(expr: &str) -> Vec<i64> {
    let nums: Vec<i64> = Regex::new(r"\d{1,2}").expect("숫자").find_iter(expr).filter_map(|m| m.as_str().parse().ok()).collect();
    let is_range = expr.contains('~') || expr.contains('∼') || expr.contains('-');
    let mut out = if is_range && nums.len() == 2 && nums[0] < nums[1] && nums[1] - nums[0] <= 10 { (nums[0]..=nums[1]).collect() } else { nums };
    out.retain(|p| (0..=10).contains(p));
    out.dedup();
    out
}

/// 같은 길이의 공백으로 가려 위치를 유지한다
fn mask(s: &mut String, start: usize, end: usize) {
    let blank = " ".repeat(end - start);
    s.replace_range(start..end, &blank);
}

fn strip_particle(w: &str) -> &str {
    for p in ["에서는", "에서", "으로", "이고", "에는", "은", "는", "이", "가", "을", "를", "에", "로", "과", "와", "도"] {
        if let Some(x) = w.strip_suffix(p) {
            if x.chars().count() >= 1 {
                return x;
            }
        }
    }
    w
}

/// 한 구간(교시 하나 몫)에서 학급·과목·장소를 읽는다
fn read_zone(zone: &str) -> TtSlot {
    let mut z = zone.to_string();
    let mut slot = TtSlot::default();
    if let Some(m) = RE_PAREN.captures(&z.clone()) {
        let inner = m[1].trim().to_string();
        if !inner.is_empty() {
            slot.room = inner;
        }
        let whole = m.get(0).expect("괄호");
        mask(&mut z, whole.start(), whole.end());
    }
    if let Some(m) = RE_CLASS.captures(&z.clone()) {
        slot.class_label = format!("{}-{}", &m[1], &m[2]);
        let whole = m.get(0).expect("학급");
        mask(&mut z, whole.start(), whole.end());
    }
    // 띄어 쓴 과목 이름 ("확률과 통계")
    for subj in SUBJECTS.iter().filter(|s| s.contains(' ')) {
        if let Some(p) = z.find(subj) {
            slot.subject = subj.to_string();
            mask(&mut z, p, p + subj.len());
            break;
        }
    }
    // 낱말 단위로 읽는다 ("과학실"을 과목 '과학'으로 읽지 않게)
    let mut fallback: Option<String> = None;
    for raw_word in z.split(|c: char| c.is_whitespace() || matches!(c, ',' | '·' | '/' | '.' | ':' | ';')) {
        let w = strip_particle(raw_word.trim());
        if w.is_empty() || FILLER.contains(&w) || RE_CHANGE.is_match(w) {
            continue;
        }
        if slot.room.is_empty() && w.chars().count() >= 2 && RE_ROOM_WORD.is_match(w) && !SUBJECTS.contains(&w) {
            slot.room = w.to_string();
            continue;
        }
        if slot.subject.is_empty() && SUBJECTS.contains(&w) {
            slot.subject = w.to_string();
            continue;
        }
        if fallback.is_none() && (2..=12).contains(&w.chars().count()) && w.chars().any(is_hangul) {
            fallback = Some(w.to_string());
        }
    }
    if slot.subject.is_empty() {
        if let Some(f) = fallback {
            slot.subject = f;
        }
    }
    slot
}

fn is_subject_or_room(name: &str) -> bool {
    SUBJECTS.iter().any(|s| name.contains(s) || s.contains(name)) || RE_ROOM_WORD.is_match(name)
}

/// 원문에서 시간표 문장을 찾는다
pub fn find(a: &Analysis) -> Vec<TtStatement> {
    let raw = a.text.as_str();
    let mut out = Vec::new();
    for sent in &a.sentences {
        let s = sent.slice(raw);
        let has_period = RE_PERIOD.is_match(s) || RE_AFTER.is_match(s);
        if !has_period {
            continue;
        }
        // 학생 이름(과목·장소 이름을 이름으로 잘못 본 경우 제외)·출결 사실·지난 일이 있으면 기록으로 둔다
        let students = a.students.iter().any(|m| sent.contains(m.span.start) && !is_subject_or_room(&m.name));
        if students || analysis::has_attendance_fact(s) || analysis::is_past(s) {
            continue;
        }
        let change_kw = RE_CHANGE.find(s).map(|m| change_type(m.as_str()).to_string());
        let hits: Vec<&crate::dates::DateHit> = a.dates.iter().filter(|d| sent.contains(d.start)).collect();
        // "금요일까지 …" 같은 기한·해야 할 일은 할 일로 둔다
        if hits.iter().any(|d| d.deadline) || analysis::has_obligation(s) {
            continue;
        }
        if change_kw.is_none() && analysis::has_action(s) {
            continue;
        }
        let every = s.contains("매주");
        let explicit = hits.iter().find(|d| !RE_BARE_WEEKDAY.is_match(d.expr.trim()));
        let bare = hits.iter().find(|d| RE_BARE_WEEKDAY.is_match(d.expr.trim()));
        let runs = weekday_runs(s);

        // 교시 위치와 각 교시의 구간
        let mut marks: Vec<(usize, usize, Vec<i64>)> = RE_PERIOD
            .captures_iter(s)
            .map(|c| {
                let m = c.get(0).expect("교시");
                (m.start(), m.end(), periods_of(&c[1]))
            })
            .collect();
        for m in RE_AFTER.find_iter(s) {
            if !marks.iter().any(|x| x.0 <= m.start() && m.start() < x.1) {
                marks.push((m.start(), m.end(), vec![101]));
            }
        }
        marks.sort_by_key(|m| m.0);
        marks.retain(|m| !m.2.is_empty());
        if marks.is_empty() {
            continue;
        }
        // 요일·날짜 표현을 가린 문장 (학급·과목 읽기용)
        let mut masked = s.to_string();
        for (st, en, _) in &runs {
            mask(&mut masked, *st, *en);
        }
        for d in &hits {
            let (st, en) = (d.start.saturating_sub(sent.start), (d.end.saturating_sub(sent.start)).min(s.len()));
            if st < en && masked.is_char_boundary(st) && masked.is_char_boundary(en) {
                mask(&mut masked, st, en);
            }
        }
        for m in ["매주"] {
            if let Some(p) = masked.find(m) {
                mask(&mut masked, p, p + m.len());
            }
        }
        let mut slots: Vec<(Vec<u8>, TtSlot)> = Vec::new();
        for (i, (st, en, periods)) in marks.iter().enumerate() {
            let zone_end = marks.get(i + 1).map(|m| m.0).unwrap_or(s.len());
            let mut z = masked[*en..zone_end].to_string();
            let mut slot = read_zone(&z);
            if slot.class_label.is_empty() && slot.subject.is_empty() {
                // "2-1 국어 월 3교시"처럼 앞에 적은 경우
                let prev_end = if i == 0 { 0 } else { marks[i - 1].1 };
                z = masked[prev_end..*st].to_string();
                let before = read_zone(&z);
                if !before.class_label.is_empty() || !before.subject.is_empty() {
                    slot = TtSlot { room: if slot.room.is_empty() { before.room.clone() } else { slot.room.clone() }, ..before };
                }
            }
            // 이 교시에 붙는 요일: 바로 앞의 요일 묶음, 없으면 뒤의 첫 요일 묶음
            let days = runs
                .iter()
                .rev()
                .find(|r| r.1 <= *st)
                .or_else(|| runs.iter().find(|r| r.0 >= *en))
                .map(|r| r.2.clone())
                .unwrap_or_default();
            for p in periods {
                let mut sl = slot.clone();
                sl.period = if *p == 101 { 101 } else { *p };
                if *p == 101 && sl.subject.is_empty() {
                    sl.subject = "방과후".into();
                }
                slots.push((days.clone(), sl));
            }
        }
        let text = text::tidy(s);
        let statement = if every {
            TtKind::Regular
        } else if let Some(d) = explicit {
            if change_kw.is_none() {
                continue;
            }
            let _ = d;
            TtKind::Change
        } else if change_kw.is_some() {
            TtKind::Change
        } else if !runs.is_empty() {
            TtKind::Regular
        } else {
            continue;
        };
        match statement {
            TtKind::Regular => {
                let slots: Vec<(Vec<u8>, TtSlot)> = slots
                    .into_iter()
                    .filter(|(d, s)| !d.is_empty() && (!s.class_label.is_empty() || !s.subject.is_empty()))
                    .collect();
                if slots.is_empty() {
                    continue;
                }
                out.push(TtStatement { span: *sent, kind: TtKind::Regular, slots, date: None, change_type: String::new(), text });
            }
            TtKind::Change => {
                // 날짜: 분명한 날짜 → 요일만 적은 날짜 → 요일 묶음의 가장 가까운 그 요일
                let date = explicit.map(|d| d.date).or(bare.map(|d| d.date)).or_else(|| {
                    let wd = runs.first()?.2.first().copied()?;
                    (0..7).map(|k| a.today + Duration::days(k)).find(|d| d.weekday().number_from_monday() as u8 == wd)
                });
                let Some(date) = date else { continue };
                let slots: Vec<(Vec<u8>, TtSlot)> =
                    slots.into_iter().map(|(_, s)| (vec![date.weekday().number_from_monday() as u8], s)).collect();
                out.push(TtStatement {
                    span: *sent,
                    kind: TtKind::Change,
                    slots,
                    date: Some(date),
                    change_type: change_kw.unwrap_or_else(|| "변경".into()),
                    text,
                });
            }
        }
    }
    out
}

/// 시간표 문장과 같은 내용의 분류 항목인지 (학생 관찰·일정·할 일로 중복해 만들지 않게)
pub fn covered_by(statements: &[TtStatement], raw: &str, item_text: &str) -> bool {
    statements.iter().any(|s| text::coverage(item_text, s.span.slice(raw)) >= 0.6)
}

/// 원문의 모든 문장이 시간표 문장인지 (그러면 AI 분류 없이 끝낸다)
pub fn covers_all(a: &Analysis, statements: &[TtStatement]) -> bool {
    !statements.is_empty()
        && a.sentences.iter().all(|s| {
            let t = s.slice(&a.text);
            t.chars().filter(|c| c.is_alphanumeric()).count() < 2 || statements.iter().any(|st| st.span == *s)
        })
}

// ───── 저장 ─────

#[derive(Debug, Clone, Serialize)]
pub struct TtEntry {
    pub id: i64,
    pub note_id: i64,
    /// regular | change
    pub kind: String,
    pub school_year: Option<i32>,
    pub semester: Option<u8>,
    pub weekday: Option<i64>,
    pub date: Option<String>,
    pub period: i64,
    pub class_label: String,
    pub subject: String,
    pub room: String,
    pub change_type: String,
    pub text: String,
    /// applied | conflict | replaced | kept | change
    pub status: String,
    /// 충돌 때 원래 칸 내용
    pub previous: String,
    /// 교시 시작 시각 (일과 시간 기준)
    pub time: Option<String>,
    /// 사람이 읽는 한 줄 ("월 3교시 2-1 국어 (과학실)")
    pub label: String,
}

const WD: [&str; 8] = ["", "월", "화", "수", "목", "금", "토", "일"];

pub fn period_label(p: i64) -> String {
    if p > 100 { format!("방과후{}", if p > 101 { (p - 100).to_string() } else { String::new() }) } else { format!("{p}교시") }
}

fn cell_text(class_label: &str, subject: &str, room: &str) -> String {
    let mut parts: Vec<String> = [class_label, subject].iter().filter(|s| !s.trim().is_empty()).map(|s| s.to_string()).collect();
    if !room.trim().is_empty() {
        parts.push(format!("({room})"));
    }
    parts.join(" ")
}

fn md(d: NaiveDate) -> String {
    format!("{}. {}.({})", d.month(), d.day(), WD[d.weekday().number_from_monday() as usize])
}

/// 그 날짜의 일과 시간으로 교시 시작 시각을 찾는다
pub fn period_time(conn: &Connection, date: NaiveDate, period: i64) -> Option<String> {
    let (y, s) = timetable::semester_of(date);
    let bell = timetable::get_bell(conn, y, s).ok()?;
    bell.periods.iter().find(|p| p.no as i64 == period && (p.kind == "class" || p.kind == "after")).map(|p| p.start.clone())
}

static RE_ONE_PERIOD: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:^|[^0-9])(\d{1,2})\s*교시").unwrap());

/// 일정·할 일 문구의 "5교시"를 시각으로 ("5교시 소방 대피 훈련" → 13:20)
pub fn time_from_period_text(conn: &Connection, date: NaiveDate, text: &str) -> Option<String> {
    let p: i64 = RE_ONE_PERIOD.captures(text)?[1].parse().ok()?;
    if !(0..=10).contains(&p) {
        return None;
    }
    period_time(conn, date, p)
}

fn map_entry(conn: &Connection, r: &rusqlite::Row) -> rusqlite::Result<TtEntry> {
    let kind: String = r.get(2)?;
    let date: Option<String> = r.get(6)?;
    let weekday: Option<i64> = r.get(5)?;
    let period: i64 = r.get(7)?;
    let (class_label, subject, room, change_type): (String, String, String, String) = (r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?);
    let d = date.as_deref().and_then(util::parse_d);
    let when = match (&d, weekday) {
        (Some(d), _) => format!("{} {}", md(*d), period_label(period)),
        (None, Some(w)) => format!("{} {}", WD[w.clamp(0, 7) as usize], period_label(period)),
        _ => period_label(period),
    };
    let what = cell_text(&class_label, &subject, &room);
    let label = if kind == "change" {
        [when, what, change_type.clone()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ")
    } else {
        format!("{when} {what}")
    };
    let time = d.and_then(|d| period_time(conn, d, period));
    Ok(TtEntry {
        id: r.get(0)?,
        note_id: r.get(1)?,
        kind,
        school_year: r.get(3)?,
        semester: r.get(4)?,
        weekday,
        date,
        period,
        class_label,
        subject,
        room,
        change_type,
        text: r.get(12)?,
        status: r.get(13)?,
        previous: r.get(14)?,
        time,
        label,
    })
}

const ENTRY_COLS: &str =
    "id, note_id, kind, school_year, semester, weekday, date, period, class_label, subject, room, change_type, text, status, previous";

pub fn query(conn: &Connection, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> AppResult<Vec<TtEntry>> {
    let mut st = conn.prepare(&format!("SELECT {ENTRY_COLS} FROM tt_entries WHERE {where_sql}"))?;
    let rows = st.query_map(args, |r| map_entry(conn, r))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn for_note(conn: &Connection, note_id: i64) -> AppResult<Vec<TtEntry>> {
    query(conn, "note_id = ?1 ORDER BY id", &[&note_id])
}

pub fn conflicts(conn: &Connection) -> AppResult<Vec<TtEntry>> {
    query(conn, "status = 'conflict' ORDER BY id", &[])
}

pub fn changes_between(conn: &Connection, start: &str, end: &str) -> AppResult<Vec<TtEntry>> {
    query(conn, "kind = 'change' AND date >= ?1 AND date <= ?2 ORDER BY date, period, id", &[&start, &end])
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplySummary {
    pub statements: usize,
    pub applied: usize,
    pub conflicts: usize,
    pub changes: usize,
    /// 기록 전체가 시간표 문장이었는지
    pub covers_all: bool,
}

/// 기록 하나의 시간표 문장을 반영한다. 이 기록에서 나온 이전 반영 기록은 새로 만든다.
/// (이미 채운 시간표 칸은 지우지 않는다 — 시간표는 교사의 것이므로)
pub fn apply_for_note(conn: &Connection, note_id: i64, now: NaiveDateTime) -> AppResult<ApplySummary> {
    let (raw, created) = crate::notes::raw_and_created(conn, note_id)?;
    let roster = crate::students::roster(conn)?;
    let duties = crate::duties::duty_entries(conn)?;
    let a = analysis::analyze(&raw, created.date(), &roster, &duties);
    let found = find(&a);
    conn.execute("DELETE FROM tt_entries WHERE note_id = ?1", params![note_id])?;
    let mut sum = ApplySummary { statements: found.len(), covers_all: covers_all(&a, &found), ..Default::default() };
    if found.is_empty() {
        return Ok(sum);
    }
    let (year, sem) = timetable::active_semester(conn, created.date());
    let now_s = util::fmt_dt(now);
    for st in &found {
        for (days, slot) in &st.slots {
            match st.kind {
                TtKind::Regular => {
                    for wd in days {
                        let wd = *wd as i64;
                        let view = timetable::get_timetable(conn, year, sem, "personal", "")?;
                        let existing = view.cells.iter().find(|c| c.weekday == wd && c.period == slot.period).cloned();
                        let new_cell = Cell {
                            row_label: String::new(),
                            weekday: wd,
                            period: slot.period,
                            subject: slot.subject.clone(),
                            teacher: String::new(),
                            class_label: slot.class_label.clone(),
                            room: slot.room.clone(),
                            memo: String::new(),
                            source: String::new(),
                        };
                        let (status, previous) = match &existing {
                            None => {
                                timetable::set_cell(conn, year, sem, "personal", "", &new_cell, now)?;
                                ("applied", String::new())
                            }
                            Some(c) => {
                                let same_class = slot.class_label.is_empty() || c.class_label == slot.class_label;
                                let same_subject = slot.subject.is_empty() || c.subject == slot.subject;
                                if same_class && same_subject {
                                    // 같은 수업이면 비어 있는 장소만 채운다
                                    if c.room.is_empty() && !slot.room.is_empty() {
                                        let mut merged = c.clone();
                                        merged.room = slot.room.clone();
                                        timetable::set_cell(conn, year, sem, "personal", "", &merged, now)?;
                                    }
                                    ("applied", String::new())
                                } else {
                                    ("conflict", cell_text(&c.class_label, &c.subject, &c.room))
                                }
                            }
                        };
                        if status == "conflict" {
                            sum.conflicts += 1;
                        } else {
                            sum.applied += 1;
                        }
                        conn.execute(
                            "INSERT INTO tt_entries(note_id, kind, school_year, semester, weekday, date, period, class_label, subject, room,
                                change_type, text, status, previous, created_at, updated_at)
                             VALUES (?1, 'regular', ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, '', ?9, ?10, ?11, ?12, ?12)",
                            params![note_id, year, sem, wd, slot.period, slot.class_label, slot.subject, slot.room, st.text, status, previous, now_s],
                        )?;
                    }
                }
                TtKind::Change => {
                    let Some(date) = st.date else { continue };
                    conn.execute(
                        "INSERT INTO tt_entries(note_id, kind, school_year, semester, weekday, date, period, class_label, subject, room,
                            change_type, text, status, previous, created_at, updated_at)
                         VALUES (?1, 'change', NULL, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'change', '', ?10, ?10)",
                        params![
                            note_id,
                            date.weekday().number_from_monday() as i64,
                            util::fmt_d(date),
                            slot.period,
                            slot.class_label,
                            slot.subject,
                            slot.room,
                            st.change_type,
                            st.text,
                            now_s
                        ],
                    )?;
                    sum.changes += 1;
                }
            }
        }
    }
    Ok(sum)
}

/// 다른 수업이 있던 칸: 바꾸거나 그대로 둔다
pub fn resolve_conflict(conn: &Connection, id: i64, replace: bool, now: NaiveDateTime) -> AppResult<TtEntry> {
    let e = query(conn, "id = ?1", &[&id])?.into_iter().next().ok_or_else(|| AppError::not_found("시간표 반영 기록을 찾지 못했습니다."))?;
    if e.status != "conflict" {
        return Err(AppError::invalid("이미 정리한 시간표 제안입니다."));
    }
    let (Some(year), Some(sem), Some(wd)) = (e.school_year, e.semester, e.weekday) else {
        return Err(AppError::invalid("시간표 제안 정보가 올바르지 않습니다."));
    };
    if replace {
        let view = timetable::get_timetable(conn, year, sem, "personal", "")?;
        let memo = view.cells.iter().find(|c| c.weekday == wd && c.period == e.period).map(|c| c.memo.clone()).unwrap_or_default();
        let cell = Cell {
            row_label: String::new(),
            weekday: wd,
            period: e.period,
            subject: e.subject.clone(),
            teacher: String::new(),
            class_label: e.class_label.clone(),
            room: e.room.clone(),
            memo,
            source: String::new(),
        };
        timetable::set_cell(conn, year, sem, "personal", "", &cell, now)?;
    }
    conn.execute(
        "UPDATE tt_entries SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, if replace { "replaced" } else { "kept" }, util::fmt_dt(now)],
    )?;
    query(conn, "id = ?1", &[&id])?.into_iter().next().ok_or_else(|| AppError::not_found("시간표 반영 기록을 찾지 못했습니다."))
}

/// 오늘 화면용: 그 날짜의 변경 (교시별)
pub fn changes_on(conn: &Connection, date: NaiveDate) -> AppResult<Vec<TtEntry>> {
    let d = util::fmt_d(date);
    changes_between(conn, &d, &d)
}

pub fn has_conflict_for(conn: &Connection, note_id: i64) -> AppResult<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM tt_entries WHERE note_id = ?1 AND status = 'conflict' LIMIT 1", params![note_id], |_| Ok(true))
        .optional()?
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 29).unwrap() // 화요일
    }
    fn found(s: &str) -> Vec<TtStatement> {
        let a = analysis::analyze(s, today(), &[], &[]);
        find(&a)
    }
    fn regular(s: &str) -> Vec<(Vec<u8>, i64, String, String, String)> {
        let f = found(s);
        assert!(f.iter().all(|x| x.kind == TtKind::Regular), "정규가 아님: {s}");
        f.iter().flat_map(|x| x.slots.iter().map(|(d, sl)| (d.clone(), sl.period, sl.class_label.clone(), sl.subject.clone(), sl.room.clone()))).collect()
    }

    #[test]
    fn weekday_runs_skip_ordinary_words() {
        let r = |s: &str| weekday_runs(s).into_iter().map(|x| x.2).collect::<Vec<_>>();
        assert_eq!(r("월 3교시"), vec![vec![1]]);
        assert_eq!(r("화·목 1교시"), vec![vec![2, 4]]);
        assert_eq!(r("화목 1교시"), vec![vec![2, 4]]);
        assert_eq!(r("월, 수, 금 5교시"), vec![vec![1, 3, 5]]);
        assert_eq!(r("매주 수요일 5교시"), vec![vec![3]]);
        assert_eq!(r("월요일에 3교시"), vec![vec![1]]);
        assert!(r("10월 5일 수학 토론 월간 계획").is_empty());
        assert!(r("수업 준비").is_empty());
    }

    #[test]
    fn regular_timetable_sentences() {
        assert_eq!(regular("월 3교시 2-1 국어 과학실"), vec![(vec![1], 3, "2-1".into(), "국어".into(), "과학실".into())]);
        assert_eq!(regular("월요일 3교시 2-1 국어 (과학실)"), vec![(vec![1], 3, "2-1".into(), "국어".into(), "과학실".into())]);
        assert_eq!(regular("화·목 1교시 3-2 수학"), vec![(vec![2, 4], 1, "3-2".into(), "수학".into(), String::new())]);
        let r = regular("매주 수요일 5·6교시 동아리");
        assert_eq!(r.iter().map(|x| x.1).collect::<Vec<_>>(), vec![5, 6]);
        assert!(r.iter().all(|x| x.0 == vec![3] && x.3 == "동아리"));
        // 한 문장에 여러 교시: 요일은 앞에서 이어진다
        let r = regular("월요일 1교시는 2-1 국어, 2교시는 2-3 국어");
        assert_eq!(r, vec![(vec![1], 1, "2-1".into(), "국어".into(), String::new()), (vec![1], 2, "2-3".into(), "국어".into(), String::new())]);
        // 줄마다 한 칸
        let r = regular("월 1교시 1-2 한문\n화 2교시 2학년 3반 정보 컴퓨터실");
        assert_eq!(r[0], (vec![1], 1, "1-2".into(), "한문".into(), String::new()));
        assert_eq!(r[1], (vec![2], 2, "2-3".into(), "정보".into(), "컴퓨터실".into()));
        // 범위와 방과후
        assert_eq!(regular("금 5~7교시 1-1 창체").iter().map(|x| x.1).collect::<Vec<_>>(), vec![5, 6, 7]);
        assert_eq!(regular("수요일 방과후 독서 도서관")[0], (vec![3], 101, String::new(), "독서".into(), "도서관".into()));
    }

    #[test]
    fn one_day_changes() {
        let f = found("내일 3교시 2-1 보강");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, TtKind::Change);
        assert_eq!(f[0].date, NaiveDate::from_ymd_opt(2026, 9, 30));
        assert_eq!(f[0].change_type, "보강");
        assert_eq!(f[0].slots[0].1.class_label, "2-1");
        let f = found("10월 5일 2교시 3-1과 교체");
        assert_eq!((f[0].date, f[0].change_type.as_str(), f[0].slots[0].1.class_label.as_str()), (NaiveDate::from_ymd_opt(2026, 10, 5), "교체", "3-1"));
        // 요일만 적었어도 변경 낱말이 있으면 가까운 그 요일 하루
        let f = found("금요일 4교시 자습");
        assert_eq!((f[0].kind.clone(), f[0].date, f[0].change_type.as_str()), (TtKind::Change, NaiveDate::from_ymd_opt(2026, 10, 2), "자습"));
        let f = found("다음 주 수요일 5교시 휴강");
        assert_eq!((f[0].date, f[0].change_type.as_str()), (NaiveDate::from_ymd_opt(2026, 10, 7), "휴강"));
    }

    #[test]
    fn records_and_tasks_are_not_timetable() {
        for s in [
            "오늘 3교시 2-1 홍길동 지각",
            "3교시 수업 중 발표를 잘함",
            "월요일 3교시 2-1 국어 수업을 했음",
            "금요일까지 3교시 보강 계획 제출하기",
            "오늘 5교시 전교생 소방 대피 훈련",
            "내일 3교시 2-1 국어",
            "3교시 2-1 국어",
            "10월 수학 교과협의회",
        ] {
            assert!(found(s).is_empty(), "시간표로 보면 안 됨: {s}");
        }
    }

    /// 평가용 기록 35개(학생 관찰·할 일·일정)는 어느 것도 시간표로 보면 안 된다
    #[test]
    fn eval_notes_are_never_timetable() {
        for file in ["eval-set.json", "eval-holdout.json"] {
            let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("scripts").join("e2e").join(file);
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            for it in v["items"].as_array().unwrap() {
                let raw = it["raw"].as_str().unwrap();
                let f = found(raw);
                assert!(f.is_empty(), "{file} {}: {:?}", it["id"], f.iter().map(|x| x.text.clone()).collect::<Vec<_>>());
            }
        }
    }

    fn setup() -> Db {
        let db = Db::open_in_memory().unwrap();
        db
    }

    fn note(db: &Db, raw: &str, at: &str) -> i64 {
        db.with(|c| crate::notes::create_note(c, raw, util::parse_dt(at).unwrap())).unwrap()
    }

    #[test]
    fn applies_to_empty_cells_and_asks_on_conflict() {
        let db = setup();
        let now = util::parse_dt("2026-09-29T10:00:00").unwrap();
        let id = note(&db, "월 3교시 2-1 국어 과학실\n화·목 1교시 3-2 수학", "2026-09-29T10:00:00");
        let s = db.with(|c| apply_for_note(c, id, now)).unwrap();
        assert_eq!((s.statements, s.applied, s.conflicts, s.covers_all), (2, 3, 0, true));
        let v = db.with(|c| timetable::get_timetable(c, 2026, 2, "personal", "")).unwrap();
        let mon3 = v.cells.iter().find(|c| c.weekday == 1 && c.period == 3).unwrap();
        assert_eq!((mon3.class_label.as_str(), mon3.subject.as_str(), mon3.room.as_str()), ("2-1", "국어", "과학실"));
        assert_eq!(v.cells.len(), 3);
        // 다시 반영해도 같은 결과 (중복 없음)
        let s = db.with(|c| apply_for_note(c, id, now)).unwrap();
        assert_eq!((s.applied, s.conflicts), (3, 0));
        // 다른 수업이 있는 칸은 덮어쓰지 않고 묻는다
        let id2 = note(&db, "월 3교시 2-3 국어", "2026-09-29T11:00:00");
        let s = db.with(|c| apply_for_note(c, id2, now)).unwrap();
        assert_eq!((s.applied, s.conflicts), (0, 1));
        let conf = db.with(|c| conflicts(c)).unwrap();
        assert_eq!(conf.len(), 1);
        assert_eq!(conf[0].previous, "2-1 국어 (과학실)");
        assert_eq!(conf[0].label, "월 3교시 2-3 국어");
        let v = db.with(|c| timetable::get_timetable(c, 2026, 2, "personal", "")).unwrap();
        assert_eq!(v.cells.iter().find(|c| c.weekday == 1 && c.period == 3).unwrap().class_label, "2-1");
        // 바꾸기
        let e = db.with(|c| resolve_conflict(c, conf[0].id, true, now)).unwrap();
        assert_eq!(e.status, "replaced");
        let v = db.with(|c| timetable::get_timetable(c, 2026, 2, "personal", "")).unwrap();
        let mon3 = v.cells.iter().find(|c| c.weekday == 1 && c.period == 3).unwrap();
        assert_eq!((mon3.class_label.as_str(), mon3.room.as_str()), ("2-3", ""));
        assert!(db.with(|c| resolve_conflict(c, conf[0].id, true, now)).is_err());
    }

    #[test]
    fn one_day_change_is_stored_with_period_time() {
        let db = setup();
        let now = util::parse_dt("2026-09-29T10:00:00").unwrap();
        let id = note(&db, "내일 3교시 2-1 보강. 홍길동 보호자에게 금요일까지 연락", "2026-09-29T10:00:00");
        let s = db.with(|c| apply_for_note(c, id, now)).unwrap();
        assert_eq!((s.changes, s.covers_all), (1, false));
        let ch = db.with(|c| changes_on(c, NaiveDate::from_ymd_opt(2026, 9, 30).unwrap())).unwrap();
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0].label, "9. 30.(수) 3교시 2-1 보강");
        // 기본 일과 시간: 3교시 10:50
        assert_eq!(ch[0].time.as_deref(), Some("10:50"));
        // 시간표 칸은 그대로
        let v = db.with(|c| timetable::get_timetable(c, 2026, 2, "personal", "")).unwrap();
        assert!(v.cells.is_empty());
        // 일정 문구의 교시 → 시각
        let t = db.with(|c| Ok(time_from_period_text(c, NaiveDate::from_ymd_opt(2026, 9, 29).unwrap(), "5교시 전교생 소방 대피 훈련"))).unwrap();
        assert_eq!(t.as_deref(), Some("13:20"));
    }
}
