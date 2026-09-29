//! 담당업무 프로필 (학교마다 다른 업무분장을 교사가 직접 등록)

use crate::analysis::DutyEntry;
use crate::error::{AppError, AppResult};
use crate::schedule::{self, DutyLogDto, TaskDto};
use crate::util;
use chrono::NaiveDateTime;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct DutyDto {
    pub id: i64,
    pub name: String,
    pub detail: String,
    pub guide: String,
    pub keywords: String,
    pub active: bool,
    pub sort_order: i64,
    pub log_count: i64,
    pub open_task_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DutyInput {
    pub name: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub guide: String,
    #[serde(default)]
    pub keywords: String,
    #[serde(default = "yes")]
    pub active: bool,
}

fn yes() -> bool {
    true
}

pub fn list_duties(conn: &Connection) -> AppResult<Vec<DutyDto>> {
    let mut st = conn.prepare(
        "SELECT d.id, d.name, d.detail, d.guide, d.keywords, d.active, d.sort_order,
            (SELECT COUNT(*) FROM duty_logs l WHERE l.duty_id=d.id),
            (SELECT COUNT(*) FROM tasks t WHERE t.duty_id=d.id AND t.done=0)
         FROM duties d ORDER BY d.active DESC, d.sort_order, d.id",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok(DutyDto {
                id: r.get(0)?,
                name: r.get(1)?,
                detail: r.get(2)?,
                guide: r.get(3)?,
                keywords: r.get(4)?,
                active: r.get::<_, i64>(5)? != 0,
                sort_order: r.get(6)?,
                log_count: r.get(7)?,
                open_task_count: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 분류에 쓰는 업무 목록. 순서가 AI 프롬프트의 번호(1부터)와 같다.
pub fn duty_entries(conn: &Connection) -> AppResult<Vec<DutyEntry>> {
    let mut st = conn.prepare("SELECT id, name, detail, guide, keywords FROM duties WHERE active=1 ORDER BY sort_order, id")?;
    let rows = st
        .query_map([], |r| {
            let kw: String = r.get(4)?;
            Ok(DutyEntry {
                id: r.get(0)?,
                name: r.get(1)?,
                detail: r.get(2)?,
                guide: r.get(3)?,
                keywords: kw.split([',', '，', '\n']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn check(input: &DutyInput) -> AppResult<()> {
    let n = input.name.trim();
    if n.is_empty() || n.chars().count() > 40 {
        return Err(AppError::invalid("업무명을 1~40자로 입력해 주세요."));
    }
    if input.guide.chars().count() > 1000 {
        return Err(AppError::invalid("분류 설명은 1000자 이내로 적어 주세요."));
    }
    Ok(())
}

pub fn create_duty(conn: &Connection, input: &DutyInput, now: NaiveDateTime) -> AppResult<i64> {
    check(input)?;
    let order: i64 = conn.query_row("SELECT IFNULL(MAX(sort_order), 0) + 1 FROM duties", [], |r| r.get(0))?;
    let now_s = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO duties(name, detail, guide, keywords, active, sort_order, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![input.name.trim(), input.detail.trim(), input.guide.trim(), input.keywords.trim(), input.active as i64, order, now_s],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_duty(conn: &Connection, id: i64, input: &DutyInput, now: NaiveDateTime) -> AppResult<()> {
    check(input)?;
    let n = conn.execute(
        "UPDATE duties SET name=?2, detail=?3, guide=?4, keywords=?5, active=?6, updated_at=?7 WHERE id=?1",
        params![id, input.name.trim(), input.detail.trim(), input.guide.trim(), input.keywords.trim(), input.active as i64, util::fmt_dt(now)],
    )?;
    if n == 0 {
        return Err(AppError::not_found("업무를 찾을 수 없습니다."));
    }
    Ok(())
}

/// 업무를 지워도 업무 기록과 할 일은 남는다 (업무 연결만 풀린다)
pub fn delete_duty(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM duties WHERE id=?1", params![id])?;
    Ok(())
}

pub fn get_duty_name(conn: &Connection, id: i64) -> AppResult<Option<String>> {
    Ok(conn.query_row("SELECT name FROM duties WHERE id=?1", params![id], |r| r.get(0)).optional()?)
}

#[derive(Debug, Clone, Serialize)]
pub struct DutyBoard {
    pub logs: Vec<DutyLogDto>,
    pub tasks: Vec<TaskDto>,
}

/// 업무 화면: 선택한 업무(없으면 전체)의 기록과 연결된 할 일
pub fn duty_board(conn: &Connection, duty_id: Option<i64>) -> AppResult<DutyBoard> {
    let (logs, tasks) = match duty_id {
        Some(id) => (
            schedule::query_duty_logs(conn, "duty_id=?1 ORDER BY created_at DESC LIMIT 300", &[&id])?,
            schedule::query_tasks(conn, "duty_id=?1 ORDER BY done, due_date IS NULL, due_date, id DESC LIMIT 300", &[&id])?,
        ),
        None => (
            schedule::query_duty_logs(conn, "1=1 ORDER BY created_at DESC LIMIT 300", &[])?,
            schedule::query_tasks(conn, "duty_id IS NOT NULL ORDER BY done, due_date IS NULL, due_date, id DESC LIMIT 300", &[])?,
        ),
    };
    Ok(DutyBoard { logs, tasks })
}
