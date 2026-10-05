//! 한국어 날짜·시각 표현 해석기.
//!
//! 입력 시각(기록 작성 시각)을 기준으로 "오늘", "금요일까지", "다음 주 금요일", "10월 15일" 같은
//! 표현을 실제 날짜로 확정한다. AI에게 날짜 계산을 맡기지 않고 항상 이 모듈로 계산한다.
//! 원래 표현(`expr`)을 함께 돌려주어 교사가 해석 결과를 확인·수정할 수 있게 한다.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Weekday};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DateHit {
    /// 원문에서의 바이트 위치
    pub start: usize,
    pub end: usize,
    /// 원래 날짜 표현 (예: "금요일까지")
    pub expr: String,
    pub date: NaiveDate,
    /// "다음 주 중", "10월 말"처럼 정확한 날이 아닌 표현
    pub approximate: bool,
    /// "까지", "마감"이 붙은 기한 표현
    pub deadline: bool,
    /// 요일만·일(日)만 적은 표현처럼 과거/미래가 모호한 표현
    pub direction_ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimeHit {
    pub start: usize,
    pub end: usize,
    pub expr: String,
    pub time: NaiveTime,
}

const WEEKDAY_CHARS: [(char, Weekday); 7] = [
    ('월', Weekday::Mon),
    ('화', Weekday::Tue),
    ('수', Weekday::Wed),
    ('목', Weekday::Thu),
    ('금', Weekday::Fri),
    ('토', Weekday::Sat),
    ('일', Weekday::Sun),
];

fn weekday_from(s: &str) -> Option<Weekday> {
    let c = s.chars().next()?;
    WEEKDAY_CHARS.iter().find(|(k, _)| *k == c).map(|(_, w)| *w)
}

pub fn weekday_ko(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "월",
        Weekday::Tue => "화",
        Weekday::Wed => "수",
        Weekday::Thu => "목",
        Weekday::Fri => "금",
        Weekday::Sat => "토",
        Weekday::Sun => "일",
    }
}

/// 월요일 시작 주의 월요일
pub fn week_monday(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

fn last_day_of_month(year: i32, month: u32) -> Option<NaiveDate> {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1).map(|d| d - Duration::days(1))
}

fn add_months(year: i32, month: u32, add: i32) -> (i32, u32) {
    let total = year * 12 + (month as i32 - 1) + add;
    (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32)
}

/// 연도 없는 "M월 D일"의 연도를 추정한다.
/// 60일 넘게 지난 날짜는 내년으로, 300일 넘게 남은 날짜는 작년으로 본다.
pub fn infer_year(month: u32, day: u32, today: NaiveDate) -> Option<NaiveDate> {
    let cand = NaiveDate::from_ymd_opt(today.year(), month, day)?;
    if cand < today - Duration::days(60) {
        NaiveDate::from_ymd_opt(today.year() + 1, month, day).or(Some(cand))
    } else if cand > today + Duration::days(300) {
        NaiveDate::from_ymd_opt(today.year() - 1, month, day).or(Some(cand))
    } else {
        Some(cand)
    }
}

struct Candidate {
    start: usize,
    end: usize,
    date: NaiveDate,
    approximate: bool,
    ambiguous: bool,
}

static RE_FULL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\d{4})\s*(?:년\s*|[./-]\s*)(\d{1,2})\s*(?:월\s*|[./-]\s*)(\d{1,2})\s*일?").unwrap()
});
static RE_MD_KO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{1,2})\s*월\s*(\d{1,2})\s*일").unwrap());
static RE_MD_SLASH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{1,2})/(\d{1,2})").unwrap());
/// "10-2". 학급 표기("2-1")와 헷갈리지 않게 7~12월이거나, 바로 뒤에 시각이 올 때만 날짜로 본다.
static RE_MD_DASH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{1,2})-(\d{1,2})").unwrap());
static RE_TIME_AHEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(?:\d{1,2}:\d{2}|오전|오후|\d{1,2}\s*시)").unwrap());
static RE_MD_DOT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\d{1,2})\.\s?(\d{1,2})(?:\.|\s*\([월화수목금토일]\))").unwrap()
});
static RE_REL_DAY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(내일\s?모레|오늘|금일|내일|명일|모레|글피|어제|작일|그저께|그제)").unwrap()
});
static RE_WEEK_WD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(이번\s*주|금주|다음\s*주|담주|차주|다다음\s*주|지난\s*주|저번\s*주|지난|저번)\s*([월화수목금토일])\s*요일",
    )
    .unwrap()
});
static RE_WD: Lazy<Regex> = Lazy::new(|| Regex::new(r"([월화수목금토일])\s*요일").unwrap());
static RE_AFTER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d{1,3})\s*(일|주|개월|달)\s*(?:후|뒤)").unwrap());
static RE_MONTH_REL_DAY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(이번\s*달|이달|다음\s*달|담달)\s*(\d{1,2})\s*일").unwrap());
static RE_MONTH_REL_END: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(이번\s*달|이달|다음\s*달|담달)\s*(말|중|안|내|까지)|(월말)").unwrap()
});
static RE_MONTH_PART: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d{1,2})\s*월\s*(초순|중순|하순|초|중|말)").unwrap());
static RE_WEEK_ONLY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(이번\s*주|금주|다음\s*주|담주|차주)\s*(중|안|내|까지)").unwrap()
});
static RE_DAY_ONLY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d{1,2})\s*일\s*(?:까지|에|날|자로|자)").unwrap());
static RE_DEADLINE_TAIL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:\([월화수목금토일]\))?\s*(전까지|까지|마감)").unwrap());

fn prev_char(text: &str, idx: usize) -> Option<char> {
    text[..idx].chars().next_back()
}
fn next_char(text: &str, idx: usize) -> Option<char> {
    text[idx..].chars().next()
}

fn push(out: &mut Vec<Candidate>, start: usize, end: usize, date: Option<NaiveDate>, approx: bool, amb: bool) {
    if let Some(date) = date {
        out.push(Candidate { start, end, date, approximate: approx, ambiguous: amb });
    }
}

fn week_offset(word: &str) -> i64 {
    let w: String = word.chars().filter(|c| !c.is_whitespace()).collect();
    match w.as_str() {
        "이번주" | "금주" => 0,
        "다음주" | "담주" | "차주" => 1,
        "다다음주" => 2,
        "지난주" | "저번주" => -1,
        _ => 0,
    }
}

/// 텍스트 안의 모든 날짜 표현을 찾는다 (위치 순, 겹치지 않음).
pub fn find_dates(text: &str, today: NaiveDate) -> Vec<DateHit> {
    let mut c: Vec<Candidate> = Vec::new();

    for m in RE_FULL.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let y: i32 = m[1].parse().unwrap_or(0);
        let mo: u32 = m[2].parse().unwrap_or(0);
        let d: u32 = m[3].parse().unwrap_or(0);
        if (2000..=2100).contains(&y) {
            push(&mut c, whole.start(), whole.end(), NaiveDate::from_ymd_opt(y, mo, d), false, false);
        }
    }
    for m in RE_MD_KO.captures_iter(text) {
        let whole = m.get(0).unwrap();
        if prev_char(text, whole.start()).is_some_and(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let mo: u32 = m[1].parse().unwrap_or(0);
        let d: u32 = m[2].parse().unwrap_or(0);
        push(&mut c, whole.start(), whole.end(), infer_year(mo, d, today), false, false);
    }
    for m in RE_MD_SLASH.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let before_ok = prev_char(text, whole.start()).is_none_or(|ch| !ch.is_ascii_digit() && ch != '/');
        let after_ok = next_char(text, whole.end()).is_none_or(|ch| !ch.is_ascii_digit() && ch != '/');
        if !(before_ok && after_ok) {
            continue;
        }
        let mo: u32 = m[1].parse().unwrap_or(0);
        let d: u32 = m[2].parse().unwrap_or(0);
        push(&mut c, whole.start(), whole.end(), infer_year(mo, d, today), false, false);
    }
    for m in RE_MD_DASH.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let before_ok = prev_char(text, whole.start()).is_none_or(|ch| !ch.is_ascii_digit() && ch != '-' && ch != '.');
        let after_ok = next_char(text, whole.end()).is_none_or(|ch| !ch.is_ascii_digit() && ch != '-');
        if !(before_ok && after_ok) {
            continue;
        }
        let mo: u32 = m[1].parse().unwrap_or(0);
        let d: u32 = m[2].parse().unwrap_or(0);
        if !((7..=12).contains(&mo) || RE_TIME_AHEAD.is_match(&text[whole.end()..])) {
            continue;
        }
        push(&mut c, whole.start(), whole.end(), infer_year(mo, d, today), false, false);
    }
    for m in RE_MD_DOT.captures_iter(text) {
        let whole = m.get(0).unwrap();
        if prev_char(text, whole.start()).is_some_and(|ch| ch.is_ascii_digit() || ch == '.') {
            continue;
        }
        let mo: u32 = m[1].parse().unwrap_or(0);
        let d: u32 = m[2].parse().unwrap_or(0);
        push(&mut c, whole.start(), whole.end(), infer_year(mo, d, today), false, false);
    }
    for m in RE_REL_DAY.find_iter(text) {
        let w: String = m.as_str().chars().filter(|c| !c.is_whitespace()).collect();
        let off = match w.as_str() {
            "오늘" | "금일" => 0,
            "내일" | "명일" => 1,
            "모레" | "내일모레" => 2,
            "글피" => 3,
            "어제" | "작일" => -1,
            "그제" | "그저께" => -2,
            _ => continue,
        };
        push(&mut c, m.start(), m.end(), Some(today + Duration::days(off)), false, false);
    }
    for m in RE_WEEK_WD.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let Some(wd) = weekday_from(&m[2]) else { continue };
        let word: String = m[1].chars().filter(|c| !c.is_whitespace()).collect();
        let date = if word == "지난" || word == "저번" {
            // 가장 최근에 지난 그 요일 (오늘 제외)
            let mut d = today - Duration::days(1);
            while d.weekday() != wd {
                d -= Duration::days(1);
            }
            d
        } else {
            week_monday(today) + Duration::days(7 * week_offset(&word) + wd.num_days_from_monday() as i64)
        };
        push(&mut c, whole.start(), whole.end(), Some(date), false, false);
    }
    for m in RE_WD.captures_iter(text) {
        let whole = m.get(0).unwrap();
        // "매주 금요일"은 반복 표현이므로 특정 날짜로 확정하지 않는다.
        let before = &text[..whole.start()];
        if before.trim_end().ends_with("매주") || before.trim_end().ends_with("매") {
            continue;
        }
        let Some(wd) = weekday_from(&m[1]) else { continue };
        // 입력일을 포함하여 가장 가까운 그 요일
        let mut d = today;
        while d.weekday() != wd {
            d += Duration::days(1);
        }
        push(&mut c, whole.start(), whole.end(), Some(d), false, true);
    }
    for m in RE_AFTER.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let n: i64 = m[1].parse().unwrap_or(0);
        let date = match &m[2] {
            "일" => Some(today + Duration::days(n)),
            "주" => Some(today + Duration::days(7 * n)),
            _ => {
                let (y, mo) = add_months(today.year(), today.month(), n as i32);
                NaiveDate::from_ymd_opt(y, mo, today.day().min(last_day_of_month(y, mo).map(|d| d.day()).unwrap_or(28)))
            }
        };
        push(&mut c, whole.start(), whole.end(), date, false, false);
    }
    for m in RE_MONTH_REL_DAY.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let word: String = m[1].chars().filter(|c| !c.is_whitespace()).collect();
        let add = if word.starts_with("다음") || word == "담달" { 1 } else { 0 };
        let (y, mo) = add_months(today.year(), today.month(), add);
        let d: u32 = m[2].parse().unwrap_or(0);
        push(&mut c, whole.start(), whole.end(), NaiveDate::from_ymd_opt(y, mo, d), false, false);
    }
    for m in RE_MONTH_REL_END.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let add = match m.get(1) {
            Some(w) => {
                let word: String = w.as_str().chars().filter(|c| !c.is_whitespace()).collect();
                if word.starts_with("다음") || word == "담달" { 1 } else { 0 }
            }
            None => 0,
        };
        let (y, mo) = add_months(today.year(), today.month(), add);
        // "까지"는 뒤의 기한 표시로 처리하므로 날짜 표현에서 뺀다.
        let end = if m.get(2).is_some_and(|x| x.as_str() == "까지") { m.get(2).unwrap().start() } else { whole.end() };
        push(&mut c, whole.start(), end, last_day_of_month(y, mo), true, false);
    }
    for m in RE_MONTH_PART.captures_iter(text) {
        let whole = m.get(0).unwrap();
        if prev_char(text, whole.start()).is_some_and(|ch| ch.is_ascii_digit()) {
            continue;
        }
        // "10월 중간고사", "10월 초등부"처럼 다른 낱말의 일부인 경우 제외
        if next_char(text, whole.end()).is_some_and(|ch| ch == '간' || ch == '등') {
            continue;
        }
        let mo: u32 = m[1].parse().unwrap_or(0);
        let day = match &m[2] {
            "초" | "초순" => 5,
            "중" | "중순" => 15,
            _ => 0,
        };
        let date = if day == 0 {
            infer_year(mo, 1, today).and_then(|d| last_day_of_month(d.year(), d.month()))
        } else {
            infer_year(mo, day, today)
        };
        push(&mut c, whole.start(), whole.end(), date, true, false);
    }
    for m in RE_WEEK_ONLY.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let date = week_monday(today) + Duration::days(7 * week_offset(&m[1]) + 4);
        let end = if &m[2] == "까지" { m.get(2).unwrap().start() } else { whole.end() };
        push(&mut c, whole.start(), end, Some(date), true, false);
    }
    for m in RE_DAY_ONLY.captures_iter(text) {
        let whole = m.get(0).unwrap();
        if prev_char(text, whole.start()).is_some_and(|ch| ch.is_ascii_digit() || ch == '/' || ch == '.') {
            continue;
        }
        let d: u32 = m[1].parse().unwrap_or(0);
        let this_month = NaiveDate::from_ymd_opt(today.year(), today.month(), d);
        let date = match this_month {
            Some(x) if x >= today => Some(x),
            _ => {
                let (y, mo) = add_months(today.year(), today.month(), 1);
                NaiveDate::from_ymd_opt(y, mo, d)
            }
        };
        // "15일까지"의 "까지"는 기한 표시로 따로 붙인다.
        let num_end = m.get(1).unwrap().end();
        let day_char_end = text[num_end..].find('일').map(|i| num_end + i + '일'.len_utf8()).unwrap_or(whole.end());
        push(&mut c, whole.start(), day_char_end, date, false, true);
    }

    // 먼저 시작하고 더 긴 표현을 우선하여 겹치지 않게 고른다.
    c.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut hits: Vec<DateHit> = Vec::new();
    let mut last_end = 0usize;
    for cand in c {
        if cand.start < last_end {
            continue;
        }
        let mut end = cand.end;
        let mut deadline = false;
        if let Some(t) = RE_DEADLINE_TAIL.captures(&text[end..]) {
            deadline = true;
            end += t.get(0).unwrap().end();
        }
        last_end = end;
        hits.push(DateHit {
            start: cand.start,
            end,
            expr: text[cand.start..end].trim().to_string(),
            date: cand.date,
            approximate: cand.approximate,
            deadline,
            direction_ambiguous: cand.ambiguous,
        });
    }
    hits
}

/// 과거 사실을 적은 문장에서 요일·일(日)만 적었을 때, 가장 최근에 지난 날짜로 바꾼다.
pub fn to_past_occurrence(hit: &DateHit, today: NaiveDate) -> NaiveDate {
    if !hit.direction_ambiguous || hit.date <= today {
        return hit.date;
    }
    if hit.expr.contains("요일") {
        return hit.date - Duration::days(7);
    }
    // 일(日)만 적은 경우: 이번 달 그 날이 지났으면 이번 달, 아니면 지난 달
    let d = hit.date.day();
    if let Some(x) = NaiveDate::from_ymd_opt(today.year(), today.month(), d) {
        if x <= today {
            return x;
        }
    }
    let (y, mo) = add_months(today.year(), today.month(), -1);
    NaiveDate::from_ymd_opt(y, mo, d).unwrap_or(hit.date)
}

/// 교사가 고쳐 쓴 날짜 표현 하나를 다시 해석한다. (예: "다음 주 수요일")
pub fn parse_date_expr(expr: &str, today: NaiveDate) -> Option<DateHit> {
    find_dates(expr, today).into_iter().next()
}

static RE_TIME_KO: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(오전|오후|아침|저녁|밤|낮)?\s*(\d{1,2})\s*시(?:\s*(\d{1,2})\s*분|\s*(반))?").unwrap()
});
static RE_TIME_COLON: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{1,2}):(\d{2})").unwrap());

/// "오후 3시", "3시 30분", "15:30" 같은 시각 표현을 찾는다. "3교시", "2시간"은 시각이 아니다.
pub fn find_times(text: &str) -> Vec<TimeHit> {
    let mut out = Vec::new();
    for m in RE_TIME_KO.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let hour_end = m.get(2).unwrap().end();
        // "시간", "시각", "시험", "시작" 등은 제외
        let after_si = &text[hour_end..];
        let after_si = after_si.trim_start();
        let after = after_si.strip_prefix('시').unwrap_or("");
        if after.starts_with('간') || after.starts_with('험') || after.starts_with('작') || after.starts_with('각') {
            continue;
        }
        if prev_char(text, m.get(2).unwrap().start()).is_some_and(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let mut h: u32 = m[2].parse().unwrap_or(99);
        let min: u32 = if m.get(4).is_some() { 30 } else { m.get(3).map(|x| x.as_str().parse().unwrap_or(0)).unwrap_or(0) };
        match m.get(1).map(|x| x.as_str()) {
            Some("오후") | Some("저녁") | Some("밤") => {
                if h < 12 {
                    h += 12
                }
            }
            Some("오전") | Some("아침") => {
                if h == 12 {
                    h = 0
                }
            }
            Some("낮") => {
                if h < 6 {
                    h += 12
                }
            }
            _ => {
                // 학교 일과 기준: 1~6시는 오후로 본다.
                if (1..=6).contains(&h) {
                    h += 12
                }
            }
        }
        if let Some(t) = NaiveTime::from_hms_opt(h, min, 0) {
            out.push(TimeHit { start: whole.start(), end: whole.end(), expr: whole.as_str().trim().to_string(), time: t });
        }
    }
    for m in RE_TIME_COLON.captures_iter(text) {
        let whole = m.get(0).unwrap();
        if out.iter().any(|t| whole.start() < t.end && t.start < whole.end()) {
            continue;
        }
        let h: u32 = m[1].parse().unwrap_or(99);
        let min: u32 = m[2].parse().unwrap_or(99);
        if let Some(t) = NaiveTime::from_hms_opt(h, min, 0) {
            out.push(TimeHit { start: whole.start(), end: whole.end(), expr: whole.as_str().to_string(), time: t });
        }
    }
    out.sort_by_key(|t| t.start);
    out
}

/// 사람이 읽기 쉬운 날짜 (예: "10월 2일 (금)")
pub fn format_ko(d: NaiveDate) -> String {
    format!("{}월 {}일 ({})", d.month(), d.day(), weekday_ko(d.weekday()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    // 2026-09-29 는 화요일
    fn today() -> NaiveDate {
        d(2026, 9, 29)
    }
    fn one(text: &str) -> DateHit {
        let hits = find_dates(text, today());
        assert_eq!(hits.len(), 1, "{text}: {hits:?}");
        hits.into_iter().next().unwrap()
    }

    #[test]
    fn base_weekday_is_tuesday() {
        assert_eq!(today().weekday(), Weekday::Tue);
    }

    #[test]
    fn relative_days() {
        assert_eq!(one("오늘 3교시 지각").date, d(2026, 9, 29));
        assert_eq!(one("내일 회의").date, d(2026, 9, 30));
        assert_eq!(one("모레 제출").date, d(2026, 10, 1));
        assert_eq!(one("내일모레 제출").date, d(2026, 10, 1));
        assert_eq!(one("어제 상담함").date, d(2026, 9, 28));
    }

    #[test]
    fn friday_deadline_is_nearest_friday() {
        let h = one("금요일까지 보호자 연락");
        assert_eq!(h.date, d(2026, 10, 2));
        assert!(h.deadline);
        assert_eq!(h.expr, "금요일까지");
    }

    #[test]
    fn same_weekday_means_today() {
        // 입력일이 화요일이면 "화요일까지"는 오늘
        assert_eq!(one("화요일까지 제출").date, d(2026, 9, 29));
    }

    #[test]
    fn next_week_friday() {
        let h = one("다음 주 금요일에 회의");
        assert_eq!(h.date, d(2026, 10, 9));
        assert_eq!(h.expr, "다음 주 금요일");
        assert_eq!(one("다음주 월요일").date, d(2026, 10, 5));
        assert_eq!(one("이번 주 금요일").date, d(2026, 10, 2));
        assert_eq!(one("이번주 월요일").date, d(2026, 9, 28));
        assert_eq!(one("다다음 주 수요일").date, d(2026, 10, 14));
        assert_eq!(one("지난주 목요일").date, d(2026, 9, 24));
        assert_eq!(one("지난 금요일").date, d(2026, 9, 25));
    }

    #[test]
    fn explicit_month_day() {
        let h = one("10월 15일 교육장 공감소통 방문");
        assert_eq!(h.date, d(2026, 10, 15));
        assert_eq!(h.expr, "10월 15일");
        assert!(!h.deadline);
        assert_eq!(one("10/15 회의").date, d(2026, 10, 15));
        assert_eq!(one("10.15.(목) 회의").date, d(2026, 10, 15));
        assert_eq!(one("10.15(목) 회의").date, d(2026, 10, 15));
        assert_eq!(one("2026년 11월 3일").date, d(2026, 11, 3));
        assert_eq!(one("2026-11-03 제출").date, d(2026, 11, 3));
    }

    #[test]
    fn year_rollover() {
        // 9월에 적은 "3월 2일 개학"은 내년
        assert_eq!(one("3월 2일 개학").date, d(2027, 3, 2));
        // 조금 지난 날짜는 올해
        assert_eq!(one("9월 3일 상담").date, d(2026, 9, 3));
        let jan = d(2027, 1, 5);
        assert_eq!(find_dates("12월 20일 행사", jan)[0].date, d(2026, 12, 20));
    }

    #[test]
    fn deadline_suffix_variants() {
        let h = one("10월 15일까지 제출");
        assert!(h.deadline);
        assert_eq!(h.expr, "10월 15일까지");
        let h = one("15일까지 제출");
        assert_eq!(h.date, d(2026, 10, 15));
        assert!(h.deadline);
        let h = one("30일에 회의");
        assert_eq!(h.date, d(2026, 9, 30));
    }

    #[test]
    fn after_n_days() {
        assert_eq!(one("3일 후 재확인").date, d(2026, 10, 2));
        assert_eq!(one("2주 뒤 제출").date, d(2026, 10, 13));
    }

    #[test]
    fn approximate_expressions() {
        let h = one("다음 주 중 제출");
        assert!(h.approximate);
        assert_eq!(h.date, d(2026, 10, 9));
        let h = one("이번 달 말까지");
        assert!(h.approximate);
        assert_eq!(h.date, d(2026, 9, 30));
        assert!(h.deadline);
        let h = one("10월 말 행사");
        assert_eq!(h.date, d(2026, 10, 31));
        let h = one("다음 달 5일 회의");
        assert_eq!(h.date, d(2026, 10, 5));
    }

    #[test]
    fn repeated_weekday_is_not_a_date() {
        assert!(find_dates("매주 금요일 동아리", today()).is_empty());
    }

    #[test]
    fn no_false_positives() {
        assert!(find_dates("3교시 수업 중 발표", today()).is_empty());
        assert!(find_dates("2-1 홍길동 지각", today()).is_empty());
        assert!(find_dates("점수 3.5점", today()).is_empty());
        assert!(find_dates("1/2 정도 제출", today()).len() <= 1);
    }

    #[test]
    fn scenario_a_multiple_dates() {
        let hits = find_dates("오늘 3교시 2-1 홍길동 지각. 담임에게 알리고 금요일까지 보호자 연락.", today());
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].date, d(2026, 9, 29));
        assert_eq!(hits[1].date, d(2026, 10, 2));
        assert!(hits[1].deadline);
    }

    #[test]
    fn past_occurrence_for_past_sentences() {
        let h = one("금요일에 상담함");
        assert_eq!(to_past_occurrence(&h, today()), d(2026, 9, 25));
        let h = one("3일에 상담함");
        assert_eq!(to_past_occurrence(&h, today()), d(2026, 9, 3));
    }

    #[test]
    fn times() {
        let t = find_times("오후 3시 회의");
        assert_eq!(t[0].time, NaiveTime::from_hms_opt(15, 0, 0).unwrap());
        let t = find_times("3시 30분 상담");
        assert_eq!(t[0].time, NaiveTime::from_hms_opt(15, 30, 0).unwrap());
        let t = find_times("9시 반 회의");
        assert_eq!(t[0].time, NaiveTime::from_hms_opt(9, 30, 0).unwrap());
        let t = find_times("15:40 방과후");
        assert_eq!(t[0].time, NaiveTime::from_hms_opt(15, 40, 0).unwrap());
        assert!(find_times("3교시 수업").is_empty());
        assert!(find_times("2시간 동안").is_empty());
    }

    #[test]
    fn reparse_user_expr() {
        let h = parse_date_expr("다음 주 수요일", today()).unwrap();
        assert_eq!(h.date, d(2026, 10, 7));
        assert!(parse_date_expr("언젠가", today()).is_none());
    }
}
