//! 학년도 마감 기록물과 새 학년도 시작.
//!
//! 학년도 마감: 교사가 고른 폴더에 "2026학년도 교무수첩 기록물" 폴더를 만들고
//!   1. 학사·업무 기록 PDF (학사력 달력·목록, 시간표, 담당업무·업무 기록, 할 일)
//!   2. 학생 기록 PDF (학급 명렬표, 학생별 관찰·학생부 초안, 기록 일지, 기록에서 찾은 일정)
//!   3. 엑셀 원본 (그 학년도 자료만)
//!   4. 프로그램 백업(.db)
//!   읽어보기.txt, SHA256SUMS.txt
//! 를 넣는다. PDF 는 PDF/A-2b(장기 보존용)로 만든다.
//!
//! 새 학년도 시작: 먼저 백업하고, 지난 학년도 학생을 보관 처리(목록에서 숨김, 기록은 그대로)하고,
//!   일과 시간·시간표를 새 학년도 1학기로 복사하고, 필요하면 담당업무를 쉬게 한다.

use crate::analysis::Area;
use crate::backup;
use crate::db;
use crate::dates::weekday_ko;
use crate::error::{AppError, AppResult};
use crate::export::{self, ExportOptions, Sheet};
use crate::pdf;
use crate::settings;
use crate::timetable;
use crate::util;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Weekday};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const TEMPLATE: &str = include_str!("../templates/archive.typ");
const ARCHIVES_KEY: &str = "year_archives";
const STARTED_KEY: &str = "school_year_started";

// ───── 날짜 ─────

fn year_bounds(y: i32) -> (NaiveDate, NaiveDate) {
    (NaiveDate::from_ymd_opt(y, 3, 1).expect("3월 1일"), NaiveDate::from_ymd_opt(y + 1, 3, 1).expect("3월 1일"))
}

fn md(d: NaiveDate) -> String {
    format!("{}. {}.({})", d.month(), d.day(), weekday_ko(d.weekday()))
}

fn md_str(s: &str) -> String {
    s.get(..10).and_then(util::parse_d).map(md).unwrap_or_else(|| s.to_string())
}

fn ymd_dot(d: NaiveDate) -> String {
    format!("{}. {}. {}.", d.year(), d.month(), d.day())
}

fn check_year(y: i32) -> AppResult<()> {
    if (2000..=2100).contains(&y) { Ok(()) } else { Err(AppError::invalid("학년도가 올바르지 않습니다.")) }
}

/// "2-1" → (2, 1): 학급 이름 정렬용
fn class_key(label: &str) -> (i64, i64, String) {
    let mut it = label.split('-').map(|x| x.trim().parse::<i64>().unwrap_or(i64::MAX));
    (it.next().unwrap_or(i64::MAX), it.next().unwrap_or(i64::MAX), label.to_string())
}

fn area_order(a: &str) -> usize {
    Area::ALL.iter().position(|x| x.as_str() == a).unwrap_or(Area::ALL.len())
}

fn source_text(source: &str, edited: bool) -> &'static str {
    match (source, edited) {
        ("ai", true) => "AI·교사 수정",
        ("ai", false) => "AI",
        _ => "직접",
    }
}

// ───── 학년도 목록 ─────

#[derive(Debug, Clone, Serialize)]
pub struct YearInfo {
    pub school_year: i32,
    pub current: bool,
    pub notes: i64,
    pub students: i64,
    pub observations: i64,
    pub events: i64,
    pub timetable_cells: i64,
    /// 학년도 마감 기록물을 만든 시각
    pub archived_at: Option<String>,
}

fn archives(conn: &Connection) -> AppResult<BTreeMap<String, String>> {
    Ok(db::get_setting(conn, ARCHIVES_KEY)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
}

fn count_in_year(conn: &Connection, y: i32) -> AppResult<(i64, i64, i64, i64, i64)> {
    let (s, e) = export::year_range(Some(y));
    let q = |sql: &str| conn.query_row(sql, params![s, e, y], |r| r.get::<_, i64>(0));
    Ok((
        q("SELECT COUNT(*) FROM notes WHERE created_at >= ?1 AND created_at < ?2 AND ?3 IS NOT NULL")?,
        q("SELECT COUNT(*) FROM students s JOIN classes c ON c.id = s.class_id WHERE c.school_year = ?3 AND ?1 IS NOT NULL AND ?2 IS NOT NULL")?,
        q("SELECT COUNT(*) FROM observations WHERE observed_on >= ?1 AND observed_on < ?2 AND ?3 IS NOT NULL")?,
        q("SELECT COUNT(*) FROM events WHERE start_date < ?2 AND end_date >= ?1 AND ?3 IS NOT NULL")?,
        q("SELECT COUNT(*) FROM timetable_cells tc JOIN timetables t ON t.id = tc.timetable_id WHERE t.school_year = ?3 AND ?1 IS NOT NULL AND ?2 IS NOT NULL")?,
    ))
}

/// 자료가 있는 학년도 (지금 학년도는 늘 포함), 최근 것부터
pub fn years(conn: &Connection, today: NaiveDate) -> AppResult<Vec<YearInfo>> {
    let cur = util::school_year(today);
    let sy = |col: &str| {
        format!(
            "CAST(substr({col}, 1, 4) AS INTEGER) - (CASE WHEN CAST(substr({col}, 6, 2) AS INTEGER) < 3 THEN 1 ELSE 0 END)"
        )
    };
    let sql = format!(
        "SELECT school_year FROM classes UNION SELECT school_year FROM timetables
         UNION SELECT {} FROM notes UNION SELECT {} FROM events UNION SELECT {} FROM observations",
        sy("created_at"),
        sy("start_date"),
        sy("observed_on")
    );
    let mut ys: Vec<i32> = {
        let mut st = conn.prepare(&sql)?;
        st.query_map([], |r| r.get::<_, Option<i64>>(0))?
            .filter_map(|r| r.ok().flatten())
            .filter_map(|y| i32::try_from(y).ok())
            .filter(|y| (2000..=2100).contains(y))
            .collect()
    };
    ys.push(cur);
    ys.sort_unstable_by(|a, b| b.cmp(a));
    ys.dedup();
    let arch = archives(conn)?;
    ys.into_iter()
        .map(|y| {
            let (notes, students, observations, events, timetable_cells) = count_in_year(conn, y)?;
            Ok(YearInfo {
                school_year: y,
                current: y == cur,
                notes,
                students,
                observations,
                events,
                timetable_cells,
                archived_at: arch.get(&y.to_string()).cloned(),
            })
        })
        .collect()
}

// ───── 1. 학사·업무 기록 자료 ─────

struct Ev {
    layer: String,
    title: String,
    start: NaiveDate,
    end: NaiveDate,
    memo: String,
}

fn layer_text(l: &str) -> &'static str {
    match l {
        "annual" => "학사력",
        "monthly" => "월간 계획",
        "weekly" => "주간 계획",
        _ => "기록에서 찾은 일정",
    }
}

fn load_events(conn: &Connection, y: i32, note_layer: bool) -> AppResult<Vec<Ev>> {
    let (s, e) = export::year_range(Some(y));
    let mut st = conn.prepare(
        "SELECT layer, title, start_date, end_date, memo FROM events
         WHERE start_date < ?2 AND end_date >= ?1 AND ((?3 = 1 AND layer = 'note') OR (?3 = 0 AND layer <> 'note'))
         ORDER BY start_date, time IS NULL, time, id",
    )?;
    let rows = st
        .query_map(params![s, e, note_layer as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(layer, title, s, e, memo)| {
            let start = util::parse_d(&s)?;
            let end = util::parse_d(&e).unwrap_or(start);
            Some(Ev { layer, title, start, end, memo })
        })
        .collect())
}

fn ev_date(e: &Ev) -> String {
    if e.end > e.start { format!("{} ~ {}", md(e.start), md(e.end)) } else { md(e.start) }
}

/// 한 달 달력 (일요일 시작, 화면의 학사력과 같게). 여러 날 일정은 첫날에 "~끝날", 이어지는 주의 일요일에 "→"로 표시한다.
fn month_json(first: NaiveDate, evs: &[Ev]) -> J {
    let next = if first.month() == 12 {
        NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
    }
    .expect("다음 달");
    let blank = || json!({ "d": null, "sun": false, "sat": false, "items": [] });
    let mut cells: Vec<(J, usize)> = (0..first.weekday().num_days_from_sunday()).map(|_| (blank(), 0)).collect();
    let mut day = first;
    while day < next {
        let mut items = Vec::new();
        for e in evs.iter().filter(|e| e.start <= day && day <= e.end) {
            let mark = if e.layer == "annual" { "" } else { "· " };
            if day == e.start {
                let until = if e.end > e.start { format!(" (~{}. {}.)", e.end.month(), e.end.day()) } else { String::new() };
                items.push(format!("{mark}{}{until}", e.title));
            } else if day.weekday() == Weekday::Sun || day == first {
                items.push(format!("→ {}", e.title));
            }
        }
        // 칸 높이 계산: 좁은 칸에 한 줄 9자 정도 들어간다
        let lines: usize = items.iter().map(|s| s.chars().count().div_ceil(9).max(1)).sum();
        cells.push((
            json!({ "d": day.day(), "sun": day.weekday() == Weekday::Sun, "sat": day.weekday() == Weekday::Sat, "items": items }),
            lines,
        ));
        day = day.succ_opt().expect("다음 날");
    }
    while cells.len() % 7 != 0 {
        cells.push((blank(), 0));
    }
    let weeks: Vec<J> = cells
        .chunks(7)
        .map(|w| {
            let lines = w.iter().map(|c| c.1).max().unwrap_or(0);
            let h = (6.0 + lines as f64 * 3.3).max(15.0).round() as i64;
            json!({ "h": h, "days": w.iter().map(|c| c.0.clone()).collect::<Vec<_>>() })
        })
        .collect();
    json!({ "title": format!("{}년 {}월", first.year(), first.month()), "weeks": weeks })
}

struct TtCell {
    row_label: String,
    weekday: i64,
    period: i64,
    subject: String,
    teacher: String,
    class_label: String,
    room: String,
}

fn period_label(p: i64) -> String {
    match p {
        0..=10 => format!("{p}교시"),
        n if n > 100 => format!("방과후 {}", n - 100),
        n => n.to_string(),
    }
}

fn cell_lines(kind: &str, c: &TtCell) -> Vec<String> {
    let room = if c.room.is_empty() { String::new() } else { format!("({})", c.room) };
    let v = match kind {
        "personal" => vec![c.class_label.clone(), c.subject.clone(), room],
        "class" => vec![c.subject.clone(), c.teacher.clone(), room],
        _ => vec![c.subject.clone(), c.teacher.clone()],
    };
    v.into_iter().filter(|s| !s.is_empty()).collect()
}

fn semesters_json(conn: &Connection, y: i32) -> AppResult<Vec<J>> {
    let mut out = Vec::new();
    for sem in [1u8, 2] {
        let bell = timetable::get_bell(conn, y, sem)?;
        let tts: Vec<(i64, String, String)> = {
            let mut st = conn.prepare(
                "SELECT id, kind, class_label FROM timetables WHERE school_year = ?1 AND semester = ?2
                 AND EXISTS (SELECT 1 FROM timetable_cells c WHERE c.timetable_id = timetables.id)",
            )?;
            let mut v: Vec<(i64, String, String)> = st.query_map(params![y, sem], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
            v.sort_by_key(|(_, k, l)| (if k == "personal" { 0 } else if k == "class" { 1 } else { 2 }, class_key(l)));
            v
        };
        if !bell.saved && tts.is_empty() {
            continue;
        }
        let bell_rows: Vec<J> = if bell.saved {
            bell.periods
                .iter()
                .map(|p| {
                    let time = if p.kind == "arrival" || p.kind == "dismissal" { p.start.clone() } else { format!("{}~{}", p.start, p.end) };
                    json!({ "label": p.label, "time": time })
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut tables = Vec::new();
        let mut school = Vec::new();
        for (id, kind, label) in tts {
            let cells: Vec<TtCell> = {
                let mut st = conn.prepare(
                    "SELECT row_label, weekday, period, subject, teacher, class_label, room FROM timetable_cells WHERE timetable_id = ?1",
                )?;
                st.query_map(params![id], |r| {
                    Ok(TtCell {
                        row_label: r.get(0)?,
                        weekday: r.get(1)?,
                        period: r.get(2)?,
                        subject: r.get(3)?,
                        teacher: r.get(4)?,
                        class_label: r.get(5)?,
                        room: r.get(6)?,
                    })
                })?
                .collect::<Result<_, _>>()?
            };
            // 교시 줄: 저장한 일과 시간이 있으면 그대로, 없으면 입력한 칸의 교시로
            let mut periods: Vec<(i64, String, String)> = bell
                .periods
                .iter()
                .filter(|p| p.kind == "class" || p.kind == "after")
                .map(|p| {
                    // 방과후는 하는 요일도 함께 ("15:55~16:40 (월·화·목)")
                    let days = if p.kind == "after" && !p.days.is_empty() {
                        let names: Vec<&str> = p.days.iter().filter_map(|d| ["", "월", "화", "수", "목", "금", "토"].get(*d as usize).copied()).collect();
                        format!(" ({})", names.join("·"))
                    } else {
                        String::new()
                    };
                    (p.no as i64, p.label.clone(), format!("{}~{}{days}", p.start, p.end))
                })
                .collect();
            if !bell.saved {
                let mut nos: Vec<i64> = cells.iter().map(|c| c.period).collect();
                nos.sort_unstable();
                nos.dedup();
                periods = nos.into_iter().map(|n| (n, period_label(n), String::new())).collect();
            }
            for c in &cells {
                if !periods.iter().any(|p| p.0 == c.period) {
                    periods.push((c.period, period_label(c.period), String::new()));
                }
            }
            periods.sort_by_key(|p| p.0);
            let saturday = bell.config.saturday || cells.iter().any(|c| c.weekday == 6);
            let days: Vec<i64> = if saturday { (1..=6).collect() } else { (1..=5).collect() };
            const WD: [&str; 7] = ["", "월", "화", "수", "목", "금", "토"];
            if kind == "school" {
                let mut rows: Vec<String> = cells.iter().map(|c| c.row_label.clone()).collect();
                rows.sort_by_key(|l| class_key(l));
                rows.dedup();
                let class_periods: Vec<&(i64, String, String)> = periods.iter().filter(|p| p.0 <= 10).collect();
                for wd in &days {
                    if !cells.iter().any(|c| c.weekday == *wd) {
                        continue;
                    }
                    let mut head = vec!["학급".to_string()];
                    head.extend(class_periods.iter().map(|p| p.0.to_string()));
                    let rows_json: Vec<J> = rows
                        .iter()
                        .map(|rl| {
                            let cs: Vec<Vec<String>> = class_periods
                                .iter()
                                .map(|p| {
                                    cells
                                        .iter()
                                        .find(|c| &c.row_label == rl && c.weekday == *wd && c.period == p.0)
                                        .map(|c| cell_lines("school", c))
                                        .unwrap_or_default()
                                })
                                .collect();
                            json!({ "label": rl, "time": "", "cells": cs })
                        })
                        .collect();
                    school.push(json!({ "title": format!("전체 시간표 — {}요일", WD[*wd as usize]), "head": head, "rows": rows_json }));
                }
            } else {
                let mut head = vec!["교시".to_string()];
                head.extend(days.iter().map(|d| WD[*d as usize].to_string()));
                let rows_json: Vec<J> = periods
                    .iter()
                    .map(|p| {
                        let cs: Vec<Vec<String>> = days
                            .iter()
                            .map(|wd| {
                                cells.iter().find(|c| c.weekday == *wd && c.period == p.0).map(|c| cell_lines(&kind, c)).unwrap_or_default()
                            })
                            .collect();
                        json!({ "label": p.1, "time": p.2, "cells": cs })
                    })
                    .collect();
                let title = if kind == "personal" { "개인 시간표".to_string() } else { format!("{label} 학급 시간표") };
                tables.push(json!({ "title": title, "head": head, "rows": rows_json }));
            }
        }
        out.push(json!({ "title": format!("{sem}학기"), "bell": bell_rows, "tables": tables, "school": school }));
    }
    Ok(out)
}

fn cover(y: i32, doc_label: &str, opts: &ArchiveOptions, now: NaiveDateTime, summary: Vec<(String, String)>, notice: &str) -> J {
    let (s, e) = year_bounds(y);
    json!({
        "year_label": format!("{y}학년도"),
        "doc_label": doc_label,
        "period": format!("{} ~ {}", ymd_dot(s), ymd_dot(e.pred_opt().expect("전날"))),
        "school": opts.school_name.trim(),
        "teacher": opts.teacher_name.trim(),
        "made": format!("{} 만듦 · 로컬 AI 교무수첩 {}", ymd_dot(now.date()), env!("CARGO_PKG_VERSION")),
        "summary": summary.into_iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
        "notice": notice,
    })
}

pub fn business_data(conn: &Connection, y: i32, opts: &ArchiveOptions, now: NaiveDateTime) -> AppResult<J> {
    let (start, _) = year_bounds(y);
    let (rs, re) = export::year_range(Some(y));
    let evs = load_events(conn, y, false)?;
    let months: Vec<J> = (0..12)
        .map(|i| {
            let m0 = start.month0() + i;
            let first = NaiveDate::from_ymd_opt(start.year() + (m0 / 12) as i32, m0 % 12 + 1, 1).expect("달");
            month_json(first, &evs)
        })
        .collect();
    let events: Vec<J> = evs
        .iter()
        .map(|e| json!({ "date": ev_date(e), "layer": layer_text(&e.layer), "title": e.title, "memo": e.memo }))
        .collect();
    let semesters = semesters_json(conn, y)?;
    let tt_count: usize = semesters.iter().map(|s| s["tables"].as_array().map_or(0, Vec::len) + s["school"].as_array().map_or(0, Vec::len)).sum();

    // 담당업무와 업무 기록
    let duties: Vec<(i64, String, String, bool)> = {
        let mut st = conn.prepare("SELECT id, name, detail, active FROM duties ORDER BY sort_order, id")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0)))?.collect::<Result<_, _>>()?
    };
    let logs: Vec<(Option<i64>, String, String, bool)> = {
        let mut st = conn.prepare(
            "SELECT duty_id, COALESCE(date, substr(created_at, 1, 10)) AS k, content, done FROM duty_logs
             WHERE COALESCE(date, substr(created_at, 1, 10)) >= ?1 AND COALESCE(date, substr(created_at, 1, 10)) < ?2
             ORDER BY k, time IS NULL, time, id",
        )?;
        st.query_map(params![rs, re], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0)))?.collect::<Result<_, _>>()?
    };
    let log_json = |l: &(Option<i64>, String, String, bool)| json!({ "date": md_str(&l.1), "content": l.2, "done": if l.3 { "완료" } else { "" } });
    let mut duty_json: Vec<J> = duties
        .iter()
        .filter_map(|(id, name, detail, active)| {
            let ls: Vec<J> = logs.iter().filter(|l| l.0 == Some(*id)).map(log_json).collect();
            (*active || !ls.is_empty()).then(|| json!({ "name": name, "detail": detail, "logs": ls }))
        })
        .collect();
    let orphan: Vec<J> = logs.iter().filter(|l| l.0.is_none_or(|id| !duties.iter().any(|d| d.0 == id))).map(log_json).collect();
    if !orphan.is_empty() {
        duty_json.push(json!({ "name": "업무 미지정", "detail": "", "logs": orphan }));
    }

    // 할 일
    let tasks: Vec<(String, Option<String>, bool, Option<String>, String)> = {
        let mut st = conn.prepare(
            "SELECT t.title, t.due_date, t.done, t.done_at, COALESCE(du.name, '') FROM tasks t LEFT JOIN duties du ON du.id = t.duty_id
             WHERE COALESCE(t.due_date, substr(t.created_at, 1, 10)) >= ?1 AND COALESCE(t.due_date, substr(t.created_at, 1, 10)) < ?2
             ORDER BY t.due_date IS NULL, t.due_date, t.due_time, t.id",
        )?;
        st.query_map(params![rs, re], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?
    };
    let done = tasks.iter().filter(|t| t.2).count();
    let task_json: Vec<J> = tasks
        .iter()
        .map(|t| {
            json!({
                "due": t.1.as_deref().map(md_str).unwrap_or_else(|| "기한 없음".into()),
                "title": t.0,
                "duty": t.4,
                "done": if t.2 { t.3.as_deref().map(|d| format!("완료 {}", md_str(d))).unwrap_or_else(|| "완료".into()) } else { String::new() },
            })
        })
        .collect();

    let summary = vec![
        ("학사력·월간·주간 일정".to_string(), format!("{}개", evs.len())),
        ("시간표".to_string(), format!("{tt_count}개")),
        ("담당업무".to_string(), format!("{}개", duty_json.len())),
        ("업무 기록".to_string(), format!("{}건", logs.len())),
        ("할 일".to_string(), format!("{}개 (완료 {done})", tasks.len())),
    ];
    Ok(json!({
        "kind": "business",
        "title": format!("{y}학년도 학사·업무 기록"),
        "author": opts.teacher_name.trim(),
        "header": format!("{y}학년도 · 교사 업무 참고용"),
        "cover": cover(y, "학사·업무 기록", opts, now, summary,
            "학년도가 끝난 뒤 보관하려고, 교사가 로컬 AI 교무수첩에 적은 학사력·시간표·업무 기록·할 일을 모은 문서입니다. 장기 보존용 PDF(PDF/A)로 만들었습니다. 업무 기록이나 할 일에 학생 이름을 적었다면 그대로 들어 있습니다."),
        "months": months,
        "events": events,
        "semesters": semesters,
        "duties": duty_json,
        "tasks": task_json,
    }))
}

// ───── 2. 학생 기록 자료 ─────

pub fn student_data(conn: &Connection, y: i32, opts: &ArchiveOptions, now: NaiveDateTime) -> AppResult<J> {
    let (rs, re) = export::year_range(Some(y));
    let cols = settings::load(conn)?.roster_columns;
    // (켜짐, 머리글, 폭 mm — 0 이면 남는 폭)
    let optional: [(bool, &str, i64); 6] = [
        (cols.phone, "전화번호", 30),
        (cols.guardian_phone, "보호자 연락처", 30),
        (cols.address, "주소", 0),
        (cols.gender, "성별", 11),
        (cols.birthday, "생일", 22),
        (cols.memo, "특이사항", 0),
    ];
    let has_flex = optional.iter().any(|o| o.0 && o.2 == 0);
    let mut columns = vec!["번호", "이름"];
    let mut widths = vec![11, if has_flex { 24 } else { 0 }];
    for o in optional.iter().filter(|o| o.0) {
        columns.push(o.1);
        widths.push(o.2);
    }
    columns.push("관찰");
    widths.push(13);

    let classes: Vec<(i64, String)> = {
        let mut st = conn.prepare("SELECT id, label FROM classes WHERE school_year = ?1 ORDER BY grade, class_no")?;
        st.query_map(params![y], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let mut class_json = Vec::new();
    let mut student_total = 0;
    for (cid, label) in &classes {
        let mut st = conn.prepare(
            "SELECT number, name, phone,
                    COALESCE((SELECT GROUP_CONCAT(TRIM(relation || ' ' || phone), ', ') FROM (SELECT relation, phone FROM student_contacts sc WHERE sc.student_id = s.id ORDER BY pos, id)), guardian_phone),
                    address, gender, COALESCE(birthday, ''), memo,
                    (SELECT COUNT(*) FROM observations o WHERE o.student_id = s.id AND o.observed_on >= ?2 AND o.observed_on < ?3)
             FROM students s WHERE class_id = ?1 ORDER BY number IS NULL, number, name",
        )?;
        let rows: Vec<Vec<String>> = st
            .query_map(params![cid, rs, re], |r| {
                let vals: [String; 6] = [r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?];
                let mut row = vec![r.get::<_, Option<i64>>(0)?.map(|n| n.to_string()).unwrap_or_default(), r.get(1)?];
                row.extend(optional.iter().zip(vals).filter(|(o, _)| o.0).map(|(_, v)| v));
                row.push(r.get::<_, i64>(8)?.to_string());
                Ok(row)
            })?
            .collect::<Result<_, _>>()?;
        if rows.is_empty() {
            continue;
        }
        student_total += rows.len();
        class_json.push(json!({ "title": format!("{label} ({}명)", rows.len()), "columns": columns, "widths": widths, "rows": rows }));
    }

    // 학생별 관찰·초안
    let students: Vec<(i64, Option<String>, Option<i64>, String)> = {
        let mut st = conn.prepare(
            "SELECT s.id, c.label, s.number, s.name FROM students s LEFT JOIN classes c ON c.id = s.class_id
             WHERE EXISTS (SELECT 1 FROM observations o WHERE o.student_id = s.id AND o.observed_on >= ?1 AND o.observed_on < ?2)
                OR EXISTS (SELECT 1 FROM record_drafts d WHERE d.student_id = s.id
                           AND (c.school_year = ?3 OR (c.id IS NULL AND d.updated_at >= ?1 AND d.updated_at < ?2)))
             ORDER BY c.school_year IS NULL, c.school_year, c.grade, c.class_no, s.number IS NULL, s.number, s.name",
        )?;
        st.query_map(params![rs, re, y], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?
    };
    let mut obs_total = 0;
    let mut draft_total = 0;
    let mut student_json = Vec::new();
    for (sid, class, number, name) in &students {
        let obs: Vec<(String, String, String, String, bool)> = {
            let mut st = conn.prepare(
                "SELECT area || CASE WHEN subject <> '' THEN '|' || subject ELSE '' END, observed_on, content, source, edited FROM observations
                 WHERE student_id = ?1 AND observed_on >= ?2 AND observed_on < ?3 ORDER BY observed_on, id",
            )?;
            st.query_map(params![sid, rs, re], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? != 0)))?.collect::<Result<_, _>>()?
        };
        // "subject|사회" → "교과특기사항 (사회)"
        let area_title = |key: &str| -> String {
            let (a, s) = key.split_once('|').unwrap_or((key, ""));
            if s.is_empty() { Area::parse(a).label().to_string() } else { format!("{} ({s})", Area::parse(a).label()) }
        };
        let mut areas: Vec<String> = obs.iter().map(|o| o.0.clone()).collect();
        areas.sort_by_key(|a| (area_order(a.split('|').next().unwrap_or(a)), a.clone()));
        areas.dedup();
        let area_json: Vec<J> = areas
            .iter()
            .map(|a| {
                let items: Vec<J> = obs
                    .iter()
                    .filter(|o| &o.0 == a)
                    .map(|o| json!({ "date": md_str(&o.1), "content": o.2, "source": source_text(&o.3, o.4) }))
                    .collect();
                json!({ "label": area_title(a), "items": items })
            })
            .collect();
        let drafts: Vec<(String, String, String, bool, String)> = {
            let mut st = conn.prepare(
                "SELECT d.area || CASE WHEN d.subject <> '' THEN '|' || d.subject ELSE '' END, d.body, d.generated_by, d.reviewed, d.updated_at FROM record_drafts d
                 JOIN students s ON s.id = d.student_id LEFT JOIN classes c ON c.id = s.class_id
                 WHERE d.student_id = ?1 AND (c.school_year = ?4 OR (c.id IS NULL AND d.updated_at >= ?2 AND d.updated_at < ?3))
                 ORDER BY d.area, d.updated_at",
            )?;
            st.query_map(params![sid, rs, re, y], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0, r.get(4)?)))?.collect::<Result<_, _>>()?
        };
        let draft_json: Vec<J> = drafts
            .iter()
            .map(|d| {
                let by = match d.2.as_str() {
                    "ai" => "AI 초안",
                    "template" => "기본 초안",
                    _ => "직접 작성",
                };
                let meta = format!("{by} · {} · {}자 · {} 고침", if d.3 { "최종안" } else { "초안" }, d.1.chars().count(), md_str(&d.4));
                json!({ "area": area_title(&d.0), "body": d.1, "meta": meta })
            })
            .collect();
        obs_total += obs.len();
        draft_total += drafts.len();
        let title = match (class, number) {
            (Some(c), Some(n)) => format!("{c} {n}번 {name}"),
            (Some(c), None) => format!("{c} {name}"),
            _ => name.clone(),
        };
        student_json.push(json!({
            "title": title,
            "info": format!("관찰 {}건 · 학생부 초안 {}개", obs.len(), drafts.len()),
            "areas": area_json,
            "drafts": draft_json,
        }));
    }
    let unassigned: Vec<J> = {
        let mut st = conn.prepare(
            "SELECT observed_on, area, content FROM observations WHERE student_id IS NULL AND observed_on >= ?1 AND observed_on < ?2 ORDER BY observed_on, id",
        )?;
        st.query_map(params![rs, re], |r| {
            Ok(json!({ "date": md_str(&r.get::<_, String>(0)?), "area": Area::parse(&r.get::<_, String>(1)?).label(), "content": r.get::<_, String>(2)? }))
        })?
        .collect::<Result<_, _>>()?
    };

    // 기록 일지 (원문)
    let notes: Vec<(String, String, String)> = {
        let mut st = conn.prepare("SELECT created_at, raw_text, status FROM notes WHERE created_at >= ?1 AND created_at < ?2 ORDER BY created_at, id")?;
        st.query_map(params![rs, re], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    let mut journal: Vec<J> = Vec::new();
    let mut month_key = String::new();
    let mut day_key = String::new();
    for (at, text, status) in &notes {
        let (m, d) = (at.get(..7).unwrap_or(""), at.get(..10).unwrap_or(""));
        if m != month_key {
            month_key = m.to_string();
            day_key.clear();
            let title = util::parse_d(d).map(|x| format!("{}년 {}월", x.year(), x.month())).unwrap_or_else(|| m.to_string());
            journal.push(json!({ "title": title, "days": [] }));
        }
        let month = journal.last_mut().expect("달");
        let days = month["days"].as_array_mut().expect("날 목록");
        if d != day_key {
            day_key = d.to_string();
            days.push(json!({ "date": md_str(d), "notes": [] }));
        }
        let status = match status.as_str() {
            "classified" => "AI 분류",
            "manual" => "직접 분류",
            "pending" | "processing" => "분류 대기",
            _ => "미분류",
        };
        days.last_mut().expect("날")["notes"].as_array_mut().expect("기록 목록").push(json!({
            "time": at.get(11..16).unwrap_or(""),
            "text": text,
            "status": status,
        }));
    }
    let note_events: Vec<J> = load_events(conn, y, true)?
        .iter()
        .map(|e| json!({ "date": ev_date(e), "title": e.title, "memo": e.memo }))
        .collect();

    let summary = vec![
        ("학급".to_string(), format!("{}개", class_json.len())),
        ("학생".to_string(), format!("{student_total}명")),
        ("학생 관찰".to_string(), format!("{obs_total}건")),
        ("학생부 초안".to_string(), format!("{draft_total}개")),
        ("기록 원문".to_string(), format!("{}개", notes.len())),
    ];
    Ok(json!({
        "kind": "students",
        "title": format!("{y}학년도 학생 기록"),
        "author": opts.teacher_name.trim(),
        "header": format!("{y}학년도 · 학생 개인정보 포함"),
        "cover": cover(y, "학생 기록", opts, now, summary,
            "이 문서에는 학생 이름과 관찰 내용 같은 개인정보가 들어 있습니다. 학교 개인정보 보호 지침에 따라 안전한 곳에 보관하고, 보존 기간이 지나면 파기해 주세요. 학생부 초안은 교사 검토용 자료이며 공식 학교생활기록부가 아닙니다. 명렬표에는 명렬표 화면에서 켜 둔 항목만 들어갑니다."),
        "classes": class_json,
        "students": student_json,
        "unassigned": unassigned,
        "journal": journal,
        "note_events": note_events,
    }))
}

// ───── 마감 기록물 만들기 ─────

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArchiveOptions {
    pub school_year: i32,
    /// 기록물 폴더를 만들 상위 폴더
    #[serde(default)]
    pub path: String,
    #[serde(default = "yes")]
    pub business_pdf: bool,
    #[serde(default = "yes")]
    pub student_pdf: bool,
    #[serde(default = "yes")]
    pub excel: bool,
    #[serde(default = "yes")]
    pub db_copy: bool,
    #[serde(default)]
    pub school_name: String,
    #[serde(default)]
    pub teacher_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveFile {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub pages: Option<usize>,
    /// PDF 만: PDF/A 로 만들었는지
    pub pdfa: Option<bool>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveResult {
    pub folder: String,
    pub files: Vec<ArchiveFile>,
}

fn sha256_file(p: &Path) -> AppResult<String> {
    use std::io::Read;
    let mut h = Sha256::new();
    let mut f = std::fs::File::open(p)?;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

fn file_info(p: &Path, pages: Option<usize>, pdfa: Option<bool>, note: Option<String>) -> AppResult<ArchiveFile> {
    Ok(ArchiveFile {
        name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        size: std::fs::metadata(p)?.len(),
        sha256: sha256_file(p)?,
        pages,
        pdfa,
        note,
    })
}

fn readme(y: i32, now: NaiveDateTime, files: &[ArchiveFile]) -> String {
    let mut s = format!(
        "{y}학년도 교무수첩 기록물\r\n{} 에 로컬 AI 교무수첩 {} 으로 만들었습니다.\r\n\r\n",
        ymd_dot(now.date()),
        env!("CARGO_PKG_VERSION")
    );
    for f in files {
        let what = if f.name.starts_with("1.") {
            "학사력(달력·목록), 시간표, 담당업무와 업무 기록, 할 일"
        } else if f.name.starts_with("2.") {
            "학급 명렬표, 학생별 관찰(학생부 영역별)과 학생부 초안, 기록 일지(원문) — 학생 개인정보 포함"
        } else if f.name.starts_with("3.") {
            "위 내용의 표 자료 (항목마다 시트, 그 학년도 자료만) — 학생 개인정보 포함"
        } else {
            "이 시점의 프로그램 데이터 전체 (교무수첩 백업과 같은 형식) — 학생 개인정보 포함"
        };
        let pdf = match f.pdfa {
            Some(true) => " [PDF/A-2b 장기 보존용]",
            Some(false) => " [일반 PDF]",
            None => "",
        };
        s.push_str(&format!("- {}{pdf}\r\n  {what}\r\n", f.name));
    }
    s.push_str(
        "\r\n■ 개인정보\r\n  학생 정보가 들어 있는 파일은 학교 개인정보 보호 지침에 따라 안전한 곳에 보관하고,\r\n  보존 기간이 지나면 파기해 주세요. 파일에는 암호가 걸려 있지 않습니다.\r\n\
         \r\n■ 백업 파일(.db)을 프로그램에서 다시 열려면\r\n  %LOCALAPPDATA%\\teachers_note\\backups 폴더에 notebook-20270228-120000-manual.db 처럼\r\n  이름을 바꿔 복사한 뒤, 설정 → 데이터 → 백업 목록에서 [되돌리기]를 누릅니다.\r\n\
         \r\n■ 파일이 바뀌지 않았는지 확인\r\n  SHA256SUMS.txt 의 값과 비교합니다. (PowerShell: Get-FileHash \"파일 이름\")\r\n",
    );
    s
}

fn record_archive(conn: &Connection, y: i32, now: NaiveDateTime) -> AppResult<()> {
    let mut m = archives(conn)?;
    m.insert(y.to_string(), util::fmt_dt(now));
    db::set_setting(conn, ARCHIVES_KEY, &serde_json::to_string(&m)?)
}

pub fn run(conn: &Connection, opts: &ArchiveOptions, now: NaiveDateTime) -> AppResult<ArchiveResult> {
    let y = opts.school_year;
    check_year(y)?;
    if !(opts.business_pdf || opts.student_pdf || opts.excel || opts.db_copy) {
        return Err(AppError::invalid("만들 파일을 하나 이상 골라 주세요."));
    }
    let parent = PathBuf::from(opts.path.trim());
    if !parent.is_absolute() || !parent.is_dir() {
        return Err(AppError::invalid("기록물을 만들 폴더를 찾지 못했습니다."));
    }
    let folder = export::unique_dir(&parent, &format!("{y}학년도 교무수첩 기록물"));
    std::fs::create_dir_all(&folder)?;
    let res = build_files(conn, opts, &folder, now);
    if res.is_err() {
        // 이번에 만든 폴더만 지운다 (반쯤 만든 기록물이 남지 않게)
        let _ = std::fs::remove_dir_all(&folder);
    }
    let files = res?;
    record_archive(conn, y, now)?;
    Ok(ArchiveResult { folder: folder.display().to_string(), files })
}

fn build_files(conn: &Connection, opts: &ArchiveOptions, folder: &Path, now: NaiveDateTime) -> AppResult<Vec<ArchiveFile>> {
    let y = opts.school_year;
    let mut files = Vec::new();
    let mut write_pdf = |name: String, data: J| -> AppResult<()> {
        let r = pdf::render(TEMPLATE, data.to_string(), now.date())?;
        let p = folder.join(name);
        std::fs::write(&p, &r.bytes)?;
        files.push(file_info(&p, Some(r.pages), Some(r.pdfa), r.note)?);
        Ok(())
    };
    if opts.business_pdf {
        write_pdf(format!("1. 학사·업무 기록 ({y}학년도).pdf"), business_data(conn, y, opts, now)?)?;
    }
    if opts.student_pdf {
        write_pdf(format!("2. 학생 기록 ({y}학년도).pdf"), student_data(conn, y, opts, now)?)?;
    }
    if opts.excel {
        let eo = ExportOptions {
            format: export::Format::Xlsx,
            path: String::new(),
            sheets: Sheet::ALL.to_vec(),
            class_id: None,
            mask_contacts: false,
            roster_columns: None,
            school_year: Some(y),
            student_ids: None,
        };
        let tables = export::build(conn, &eo)?;
        let p = folder.join(format!("3. 엑셀 원본 ({y}학년도).xlsx"));
        export::write_xlsx(&tables, &p)?;
        files.push(file_info(&p, None, None, None)?);
    }
    if opts.db_copy {
        let p = folder.join(format!("4. 교무수첩 백업 ({y}학년도 마감).db"));
        conn.execute("VACUUM INTO ?1", params![p.to_string_lossy().to_string()])?;
        files.push(file_info(&p, None, None, None)?);
    }
    let readme_path = folder.join("읽어보기.txt");
    std::fs::write(&readme_path, format!("\u{feff}{}", readme(y, now, &files)))?;
    let mut all = files.clone();
    all.push(file_info(&readme_path, None, None, None)?);
    let sums: String = all.iter().map(|f| format!("{} *{}\r\n", f.sha256, f.name)).collect();
    std::fs::write(folder.join("SHA256SUMS.txt"), format!("\u{feff}{sums}"))?;
    Ok(files)
}

// ───── 새 학년도 시작 ─────

/// 12~2월에 누르면 다가오는 학년도, 그 밖에는 지금 학년도
pub fn default_new_year(today: NaiveDate) -> i32 {
    let cur = util::school_year(today);
    if matches!(today.month(), 12 | 1 | 2) { cur + 1 } else { cur }
}

/// 보관 처리할 학생: 지난 학년도 학급 학생, 또는 학급 없이 새 학년도 전에 등록되고 새 학년도 기록이 없는 학생
const ARCHIVE_COND: &str = "archived = 0 AND (
      class_id IN (SELECT id FROM classes WHERE school_year < ?1)
   OR (class_id IS NULL AND created_at < ?2
       AND NOT EXISTS (SELECT 1 FROM observations o WHERE o.student_id = students.id AND o.observed_on >= ?2)))";

#[derive(Debug, Clone, Serialize)]
pub struct NewYearPreview {
    pub new_year: i32,
    pub prev_year: i32,
    pub students_to_archive: i64,
    pub prev_classes: Vec<String>,
    /// 복사해 올 일과 시간 (예: "2026학년도 2학기")
    pub bell_source: Option<String>,
    pub new_has_bell: bool,
    pub timetables_source: i64,
    pub active_duties: i64,
    pub prev_archived_at: Option<String>,
    pub started_year: Option<i32>,
}

fn bell_source(conn: &Connection, prev: i32) -> AppResult<Option<(i32, u8)>> {
    for sem in [2u8, 1] {
        let saved: bool = conn
            .query_row("SELECT 1 FROM bell_schedules WHERE school_year = ?1 AND semester = ?2", params![prev, sem], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        let has_tt: bool = conn
            .query_row("SELECT 1 FROM timetables WHERE school_year = ?1 AND semester = ?2 LIMIT 1", params![prev, sem], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        if saved || has_tt {
            return Ok(Some((prev, sem)));
        }
    }
    Ok(None)
}

pub fn preview(conn: &Connection, new_year: i32) -> AppResult<NewYearPreview> {
    check_year(new_year)?;
    let prev = new_year - 1;
    let start = format!("{new_year}-03-01");
    let students: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM students WHERE {ARCHIVE_COND}"), params![new_year, start], |r| r.get(0))?;
    let prev_classes: Vec<String> = {
        let mut st = conn.prepare(
            "SELECT DISTINCT c.label FROM classes c JOIN students s ON s.class_id = c.id
             WHERE c.school_year < ?1 AND s.archived = 0 ORDER BY c.school_year, c.grade, c.class_no",
        )?;
        st.query_map(params![new_year], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    let src = bell_source(conn, prev)?;
    let new_has_bell: bool = conn
        .query_row("SELECT 1 FROM bell_schedules WHERE school_year = ?1 AND semester = 1", params![new_year], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    let timetables_source: i64 = match src {
        Some((y, s)) => conn.query_row("SELECT COUNT(*) FROM timetables WHERE school_year = ?1 AND semester = ?2", params![y, s], |r| r.get(0))?,
        None => 0,
    };
    let active_duties: i64 = conn.query_row("SELECT COUNT(*) FROM duties WHERE active = 1", [], |r| r.get(0))?;
    Ok(NewYearPreview {
        new_year,
        prev_year: prev,
        students_to_archive: students,
        prev_classes,
        bell_source: src.map(|(y, s)| format!("{y}학년도 {s}학기")),
        new_has_bell,
        timetables_source,
        active_duties,
        prev_archived_at: archives(conn)?.get(&prev.to_string()).cloned(),
        started_year: db::get_setting(conn, STARTED_KEY)?.and_then(|s| s.parse().ok()),
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewYearOptions {
    pub new_year: i32,
    #[serde(default = "yes")]
    pub archive_students: bool,
    #[serde(default = "yes")]
    pub copy_bell: bool,
    #[serde(default)]
    pub copy_timetables: bool,
    #[serde(default = "yes")]
    pub keep_duties: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NewYearResult {
    pub new_year: i32,
    pub archived_students: usize,
    pub bell_copied: bool,
    pub timetables_copied: usize,
    pub duties_paused: usize,
    /// 시작 직전에 만든 백업 파일 이름
    pub backup: String,
}

pub fn start_new_year(conn: &mut Connection, backups_dir: &Path, opts: &NewYearOptions, now: NaiveDateTime) -> AppResult<NewYearResult> {
    let y = opts.new_year;
    check_year(y)?;
    // 되돌릴 수 있게 먼저 백업한다
    let backup = backup::backup_with(conn, backups_dir, "new-year")?;
    let start = format!("{y}-03-01");
    let now_s = util::fmt_dt(now);
    let tx = conn.transaction()?;
    let archived_students = if opts.archive_students {
        tx.execute(&format!("UPDATE students SET archived = 1, updated_at = ?3 WHERE {ARCHIVE_COND}"), params![y, start, now_s])?
    } else {
        0
    };
    let duties_paused = if opts.keep_duties { 0 } else { tx.execute("UPDATE duties SET active = 0, updated_at = ?1 WHERE active = 1", params![now_s])? };
    db::set_setting(&tx, STARTED_KEY, &y.to_string())?;
    tx.commit()?;

    let mut bell_copied = false;
    let mut timetables_copied = 0;
    if let Some(src) = bell_source(conn, y - 1)? {
        if opts.copy_timetables {
            let r = timetable::copy_semester(conn, src, (y, 1), now)?;
            bell_copied = r.bell;
            timetables_copied = r.timetables;
        } else if opts.copy_bell {
            bell_copied = conn.execute(
                "INSERT INTO bell_schedules(school_year, semester, config, periods, updated_at)
                 SELECT ?3, 1, config, periods, ?4 FROM bell_schedules WHERE school_year = ?1 AND semester = ?2
                 AND NOT EXISTS (SELECT 1 FROM bell_schedules WHERE school_year = ?3 AND semester = 1)",
                params![src.0, src.1, y, now_s],
            )? > 0;
        }
    }
    crate::logging::info(&format!("새 학년도 시작: {y} (보관 학생 {archived_students}명)"));
    Ok(NewYearResult {
        new_year: y,
        archived_students,
        bell_copied,
        timetables_copied,
        duties_paused,
        backup: backup.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn dt(s: &str) -> NaiveDateTime {
        util::parse_dt(s).unwrap()
    }

    fn seed(db: &Db) {
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO classes(id, school_year, grade, class_no, label) VALUES (1, 2026, 2, 1, '2-1'), (2, 2026, 2, 10, '2-10'), (3, 2025, 1, 1, '1-1');
                 INSERT INTO students(id, class_id, number, name, phone, gender, birthday, memo, created_at, updated_at) VALUES
                   (1, 1, 3, '홍길동', '010-1234-5678', '남', '03-05', '땅콩 알레르기', '2026-03-02T09:00:00', 'x'),
                   (2, 2, 1, '성춘향', '', '여', NULL, '', '2026-03-02T09:00:00', 'x'),
                   (3, 3, 5, '작년학생', '', '', NULL, '', '2025-03-02T09:00:00', 'x'),
                   (4, NULL, NULL, '학급없음', '', '', NULL, '', '2026-05-01T09:00:00', 'x');
                 INSERT INTO notes(id, raw_text, created_at, updated_at, status) VALUES
                   (1, '홍길동 3교시 발표를 잘함, 칭찬함 #include \"x\" [괄호] *별표*
둘째 줄: 금요일까지 보호자 연락', '2026-09-29T10:15:00', 'x', 'classified'),
                   (2, '작년 기록', '2026-02-10T09:00:00', 'x', 'manual');
                 INSERT INTO observations(note_id, student_id, area, content, observed_on, source, edited, created_at, updated_at) VALUES
                   (1, 1, 'subject', '수업 중 발표를 잘함', '2026-09-29', 'ai', 0, 'x', 'x'),
                   (NULL, 1, 'behavior', '친구를 잘 도움', '2026-10-02', 'manual', 0, 'x', 'x'),
                   (NULL, NULL, 'none', '모둠 활동이 활발함', '2026-09-30', 'manual', 0, 'x', 'x'),
                   (2, 3, 'subject', '작년 관찰', '2026-02-10', 'manual', 0, 'x', 'x');
                 INSERT INTO record_drafts(student_id, area, body, generated_by, reviewed, created_at, updated_at) VALUES (1, 'subject', '발표에 적극적으로 참여함.', 'template', 1, 'x', '2026-10-05T10:00:00');
                 INSERT INTO duties(id, name, detail, active, created_at, updated_at) VALUES (1, '교무기획', '학사일정 관리, 수업일수', 1, 'x', 'x');
                 INSERT INTO duty_logs(duty_id, content, date, source, created_at, updated_at) VALUES (1, '학사일정 변경 보고', '2026-04-10', 'manual', 'x', 'x');
                 INSERT INTO tasks(title, due_date, done, done_at, source, created_at, updated_at) VALUES ('가정통신문 회수', '2026-10-02', 1, '2026-10-01T15:00:00', 'ai', 'x', 'x');
                 INSERT INTO events(layer, title, start_date, end_date, memo, source, created_at, updated_at) VALUES
                   ('annual', '2학기 기말고사', '2026-12-14', '2026-12-18', '', 'manual', 'x', 'x'),
                   ('annual', '겨울방학', '2027-01-05', '2027-03-01', '', 'manual', 'x', 'x'),
                   ('monthly', '학부모 공개수업', '2026-04-15', '2026-04-15', '5교시', 'manual', 'x', 'x'),
                   ('note', '홍길동 보호자 상담', '2026-10-06', '2026-10-06', '', 'ai', 'x', 'x');
                 INSERT INTO timetables(id, school_year, semester, kind, class_label, created_at, updated_at) VALUES
                   (1, 2026, 2, 'personal', '', 'x', 'x'), (2, 2026, 2, 'school', '', 'x', 'x');
                 INSERT INTO timetable_cells(timetable_id, row_label, weekday, period, subject, teacher, class_label, room) VALUES
                   (1, '', 1, 1, '국어', '', '2-1', '과학실'), (1, '', 3, 101, '방과후 독서', '', '', '도서관'),
                   (2, '2-1', 1, 1, '국어', '박선생', '', ''), (2, '2-10', 1, 2, '수학', '김선생', '', '');",
            )?;
            let s = settings::update(c, |s| {
                s.roster_columns.phone = true;
                s.roster_columns.memo = true;
            })?;
            assert!(s.roster_columns.phone);
            timetable::save_bell(c, 2026, 2, &timetable::BellConfig::default(), None, dt("2026-08-20T09:00:00"))?;
            Ok(())
        })
        .unwrap();
    }

    fn opts(dir: &Path) -> ArchiveOptions {
        ArchiveOptions {
            school_year: 2026,
            path: dir.display().to_string(),
            business_pdf: true,
            student_pdf: true,
            excel: true,
            db_copy: true,
            school_name: "OO중학교".into(),
            teacher_name: "홍길동".into(),
        }
    }

    #[test]
    fn data_is_limited_to_the_school_year() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let o = opts(Path::new("C:\\"));
        let now = dt("2027-02-20T15:00:00");
        let b = db.with(|c| business_data(c, 2026, &o, now)).unwrap();
        assert_eq!(b["months"].as_array().unwrap().len(), 12);
        assert_eq!(b["months"][0]["title"], "2026년 3월");
        assert_eq!(b["months"][11]["title"], "2027년 2월");
        // 학사력·월간 계획은 달력에, 기록에서 찾은 일정은 학생 기록 쪽으로
        let titles: Vec<&str> = b["events"].as_array().unwrap().iter().map(|e| e["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["학부모 공개수업", "2학기 기말고사", "겨울방학"]);
        assert_eq!(b["events"][1]["date"], "12. 14.(월) ~ 12. 18.(금)");
        // 12월 14일 칸에 '2학기 기말고사 (~12. 18.)'
        let dec = &b["months"][9];
        let found = dec["weeks"].as_array().unwrap().iter().flat_map(|w| w["days"].as_array().unwrap().iter()).find(|d| d["d"] == 14).unwrap();
        assert_eq!(found["items"][0], "2학기 기말고사 (~12. 18.)");
        let sem2 = b["semesters"].as_array().unwrap().iter().find(|s| s["title"] == "2학기").unwrap();
        assert_eq!(sem2["tables"][0]["title"], "개인 시간표");
        let first_row = &sem2["tables"][0]["rows"][0];
        assert_eq!(first_row["label"], "1교시");
        assert_eq!(first_row["cells"][0], json!(["2-1", "국어", "(과학실)"]));
        // 방과후 줄도 들어간다
        assert!(sem2["tables"][0]["rows"].as_array().unwrap().iter().any(|r| r["label"].as_str().unwrap().contains("방과후")));
        // 전체 시간표: 학급 이름 순서 (2-1 < 2-10)
        assert_eq!(sem2["school"][0]["rows"][1]["label"], "2-10");
        assert_eq!(b["duties"][0]["logs"][0]["content"], "학사일정 변경 보고");
        assert_eq!(b["tasks"][0]["done"], "완료 10. 1.(목)");

        let s = db.with(|c| student_data(c, 2026, &o, now)).unwrap();
        assert_eq!(s["classes"][0]["columns"], json!(["번호", "이름", "전화번호", "특이사항", "관찰"]));
        assert_eq!(s["classes"][0]["rows"][0], json!(["3", "홍길동", "010-1234-5678", "땅콩 알레르기", "2"]));
        let st: Vec<&str> = s["students"].as_array().unwrap().iter().map(|x| x["title"].as_str().unwrap()).collect();
        assert_eq!(st, ["2-1 3번 홍길동"]);
        // 영역 순서: 교과 → 행동특성
        assert_eq!(s["students"][0]["areas"][0]["label"], "교과특기사항");
        assert_eq!(s["students"][0]["areas"][1]["label"], "행동특성 및 종합의견");
        assert_eq!(s["students"][0]["drafts"][0]["meta"], "기본 초안 · 최종안 · 14자 · 10. 5.(월) 고침");
        assert_eq!(s["unassigned"].as_array().unwrap().len(), 1);
        assert_eq!(s["journal"].as_array().unwrap().len(), 1);
        assert_eq!(s["journal"][0]["days"][0]["notes"][0]["time"], "10:15");
        assert_eq!(s["note_events"][0]["title"], "홍길동 보호자 상담");
    }

    #[test]
    fn makes_archive_folder_with_pdfa_files_and_checksums() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let dir = tempfile::tempdir().unwrap();
        let r = db.with(|c| run(c, &opts(dir.path()), dt("2027-02-20T15:00:00"))).unwrap();
        assert!(r.folder.ends_with("2026학년도 교무수첩 기록물"));
        let names: Vec<&str> = r.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            ["1. 학사·업무 기록 (2026학년도).pdf", "2. 학생 기록 (2026학년도).pdf", "3. 엑셀 원본 (2026학년도).xlsx", "4. 교무수첩 백업 (2026학년도 마감).db"]
        );
        for f in r.files.iter().filter(|f| f.name.ends_with(".pdf")) {
            assert_eq!(f.pdfa, Some(true), "{}: {:?}", f.name, f.note);
            assert!(f.pages.unwrap() >= 3, "{} 쪽 수 {:?}", f.name, f.pages);
        }
        let folder = Path::new(&r.folder);
        let sums = std::fs::read_to_string(folder.join("SHA256SUMS.txt")).unwrap();
        for f in &r.files {
            assert!(sums.contains(&format!("{} *{}", f.sha256, f.name)));
            assert_eq!(sha256_file(&folder.join(&f.name)).unwrap(), f.sha256);
        }
        assert!(sums.contains("읽어보기.txt"));
        // 백업 사본은 이 프로그램이 다시 열 수 있다
        let counts = backup::inspect_file(&folder.join("4. 교무수첩 백업 (2026학년도 마감).db")).unwrap();
        assert_eq!(counts.notes, 2);
        // 마감 기록이 남고, 같은 학년도를 다시 만들면 (2) 폴더
        let ys = db.with(|c| years(c, NaiveDate::from_ymd_opt(2027, 2, 20).unwrap())).unwrap();
        let y26 = ys.iter().find(|y| y.school_year == 2026).unwrap();
        assert!(y26.archived_at.is_some());
        assert_eq!(y26.notes, 1);
        assert!(ys.iter().any(|y| y.school_year == 2025));
        let r2 = db.with(|c| run(c, &opts(dir.path()), dt("2027-02-20T16:00:00"))).unwrap();
        assert!(r2.folder.ends_with("(2)"));
        // 고른 것이 없으면 거절, 폴더도 만들지 않는다
        let mut none = opts(dir.path());
        (none.business_pdf, none.student_pdf, none.excel, none.db_copy) = (false, false, false, false);
        assert!(db.with(|c| run(c, &none, dt("2027-02-20T17:00:00"))).is_err());
        assert!(!dir.path().join("2026학년도 교무수첩 기록물 (3)").exists());
    }

    /// 눈으로 확인할 표본: TN_ARCHIVE_OUT=<폴더> cargo test --lib write_sample_archive -- --ignored
    #[test]
    #[ignore]
    fn write_sample_archive() {
        let Some(out) = std::env::var_os("TN_ARCHIVE_OUT") else { return };
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let r = db.with(|c| run(c, &opts(Path::new(&out)), dt("2027-02-20T15:00:00"))).unwrap();
        println!("{}", r.folder);
        let now = dt("2027-02-20T15:00:00");
        let o = opts(Path::new(&out));
        for (tag, data) in [
            ("business", db.with(|c| business_data(c, 2026, &o, now)).unwrap()),
            ("students", db.with(|c| student_data(c, 2026, &o, now)).unwrap()),
        ] {
            let pngs = pdf::render_pngs(TEMPLATE, data.to_string(), now.date()).unwrap();
            for (i, png) in pngs.iter().enumerate() {
                std::fs::write(Path::new(&out).join(format!("{tag}-{:02}.png", i + 1)), png).unwrap();
            }
        }
    }

    #[test]
    fn new_year_archives_old_students_and_copies_bell() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(default_new_year(NaiveDate::from_ymd_opt(2027, 2, 20).unwrap()), 2027);
        assert_eq!(default_new_year(NaiveDate::from_ymd_opt(2027, 3, 4).unwrap()), 2027);
        assert_eq!(default_new_year(NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()), 2026);
        let p = db.with(|c| preview(c, 2027)).unwrap();
        // 2026·2025 학급 학생 3명 + 학급 없는 학생 1명
        assert_eq!(p.students_to_archive, 4);
        assert_eq!(p.bell_source.as_deref(), Some("2026학년도 2학기"));
        assert!(!p.new_has_bell);
        assert!(p.prev_archived_at.is_none());
        let o = NewYearOptions { new_year: 2027, archive_students: true, copy_bell: true, copy_timetables: false, keep_duties: true };
        let r = db.with(|c| start_new_year(c, dir.path(), &o, dt("2027-02-25T09:00:00"))).unwrap();
        assert_eq!(r.archived_students, 4);
        assert!(r.bell_copied);
        assert_eq!(r.timetables_copied, 0);
        assert!(r.backup.contains("new-year"));
        assert!(dir.path().join(&r.backup).exists());
        let (active, bell_2027): (i64, i64) = db
            .with(|c| {
                Ok((
                    c.query_row("SELECT COUNT(*) FROM students WHERE archived = 0", [], |r| r.get(0))?,
                    c.query_row("SELECT COUNT(*) FROM bell_schedules WHERE school_year = 2027 AND semester = 1", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!((active, bell_2027), (0, 1));
        // 기록은 그대로 남는다
        let obs: i64 = db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM observations", [], |r| r.get(0))?)).unwrap();
        assert_eq!(obs, 4);
        // 보관한 학생도 그 학년도 기록물에는 들어간다
        let s = db.with(|c| student_data(c, 2026, &opts(dir.path()), dt("2027-02-26T09:00:00"))).unwrap();
        assert_eq!(s["students"][0]["title"], "2-1 3번 홍길동");
        // 시간표까지 복사 + 업무 쉬기
        let o2 = NewYearOptions { new_year: 2028, archive_students: true, copy_bell: true, copy_timetables: true, keep_duties: false };
        db.with(|c| {
            c.execute("INSERT INTO timetables(school_year, semester, kind, class_label, created_at, updated_at) VALUES (2027, 1, 'personal', '', 'x', 'x')", [])?;
            Ok(())
        })
        .unwrap();
        let r2 = db.with(|c| start_new_year(c, dir.path(), &o2, dt("2028-02-25T09:00:00"))).unwrap();
        assert_eq!(r2.timetables_copied, 1);
        assert_eq!(r2.duties_paused, 1);
        let p = db.with(|c| preview(c, 2028)).unwrap();
        assert_eq!(p.started_year, Some(2028));
    }

    #[test]
    fn classes_made_after_early_new_year_start_belong_to_new_year() {
        let db = Db::open_in_memory().unwrap();
        seed(&db);
        let dir = tempfile::tempdir().unwrap();
        let feb = NaiveDate::from_ymd_opt(2027, 2, 25).unwrap();
        assert_eq!(db.with(|c| Ok(crate::students::active_school_year(c, feb))).unwrap(), 2026);
        let o = NewYearOptions { new_year: 2027, archive_students: true, copy_bell: true, copy_timetables: false, keep_duties: true };
        db.with(|c| start_new_year(c, dir.path(), &o, dt("2027-02-25T09:00:00"))).unwrap();
        // 2월 말에 새 학년도를 시작하면 새로 넣는 학급은 2027학년도
        assert_eq!(db.with(|c| Ok(crate::students::active_school_year(c, feb))).unwrap(), 2027);
        let id = db
            .with(|c| crate::students::create_student(c, &crate::students::StudentInput { name: "새학생".into(), class_label: Some("3-1".into()), number: Some(1), memo: None }, dt("2027-02-26T09:00:00")))
            .unwrap();
        let y: i64 = db
            .with(|c| Ok(c.query_row("SELECT c.school_year FROM students s JOIN classes c ON c.id = s.class_id WHERE s.id = ?1", params![id], |r| r.get(0))?))
            .unwrap();
        assert_eq!(y, 2027);
        // 3월이 지나 달력 학년도가 따라오면 그대로 2027
        assert_eq!(db.with(|c| Ok(crate::students::active_school_year(c, NaiveDate::from_ymd_opt(2027, 3, 4).unwrap()))).unwrap(), 2027);
    }
}
