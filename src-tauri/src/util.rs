use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

pub const DT_FMT: &str = "%Y-%m-%dT%H:%M:%S";
pub const D_FMT: &str = "%Y-%m-%d";
pub const T_FMT: &str = "%H:%M";

/// 이 PC의 현지 시각 (초 단위로 잘라 저장 형식과 맞춘다)
pub fn now() -> NaiveDateTime {
    let n = Local::now().naive_local();
    n.with_nanosecond(0).unwrap_or(n)
}

pub fn fmt_dt(d: NaiveDateTime) -> String {
    d.format(DT_FMT).to_string()
}
pub fn fmt_d(d: NaiveDate) -> String {
    d.format(D_FMT).to_string()
}
pub fn fmt_t(t: NaiveTime) -> String {
    t.format(T_FMT).to_string()
}
pub fn parse_dt(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, DT_FMT)
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").ok())
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok())
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").ok())
}
pub fn parse_d(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), D_FMT).ok()
}
pub fn parse_t(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), T_FMT)
        .ok()
        .or_else(|| NaiveTime::parse_from_str(s.trim(), "%H:%M:%S").ok())
}

/// 한국 학년도: 3월에 시작한다.
pub fn school_year(d: NaiveDate) -> i32 {
    if d.month() >= 3 { d.year() } else { d.year() - 1 }
}

/// 교사가 입력한 날짜 문자열을 검증한다 (빈 값은 None)
pub fn opt_date(s: Option<&str>) -> Result<Option<String>, crate::error::AppError> {
    match s.map(str::trim).filter(|x| !x.is_empty()) {
        None => Ok(None),
        Some(v) => parse_d(v)
            .map(|d| Some(fmt_d(d)))
            .ok_or_else(|| crate::error::AppError::invalid(format!("날짜 형식이 올바르지 않습니다: {v}"))),
    }
}

pub fn opt_time(s: Option<&str>) -> Result<Option<String>, crate::error::AppError> {
    match s.map(str::trim).filter(|x| !x.is_empty()) {
        None => Ok(None),
        Some(v) => parse_t(v)
            .map(|t| Some(fmt_t(t)))
            .ok_or_else(|| crate::error::AppError::invalid(format!("시각 형식이 올바르지 않습니다: {v}"))),
    }
}
