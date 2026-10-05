//! 오늘 화면 메모 (윈도우 스티커 메모처럼 날이 바뀌어도 그대로 남는다)
//!
//! 글은 쓰는 대로 저장한다. 지운 메모는 표에 남겨 두어 바로 되돌릴 수 있고, 30일이 지나면 정리한다.

use crate::error::{AppError, AppResult};
use crate::util;
use chrono::{Duration, NaiveDateTime};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

/// 메모 색 (윈도우 스티커 메모와 비슷한 6가지)
pub const MEMO_COLORS: [&str; 6] = ["yellow", "green", "pink", "purple", "blue", "gray"];
/// 메모 한 장에 넣을 수 있는 글자 수
pub const MEMO_MAX_CHARS: usize = 20_000;
/// 지운 메모를 남겨 두는 기간
const KEEP_DELETED_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MemoDto {
    pub id: i64,
    pub body: String,
    pub color: String,
    pub pos: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<MemoDto> {
    Ok(MemoDto { id: r.get(0)?, body: r.get(1)?, color: r.get(2)?, pos: r.get(3)?, created_at: r.get(4)?, updated_at: r.get(5)? })
}

const COLS: &str = "id, body, color, pos, created_at, updated_at";

fn clean_color(c: &str) -> &'static str {
    MEMO_COLORS.iter().find(|x| **x == c).copied().unwrap_or("yellow")
}

pub fn list(conn: &Connection) -> AppResult<Vec<MemoDto>> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM memos WHERE deleted_at IS NULL ORDER BY pos, id"))?;
    let v = st.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<MemoDto> {
    conn.query_row(&format!("SELECT {COLS} FROM memos WHERE id=?1"), params![id], row)
        .optional()?
        .ok_or_else(|| AppError::not_found("메모를 찾을 수 없습니다."))
}

/// 새 메모는 맨 앞에 붙는다
pub fn create(conn: &Connection, color: &str, now: NaiveDateTime) -> AppResult<MemoDto> {
    let first: Option<i64> = conn.query_row("SELECT MIN(pos) FROM memos WHERE deleted_at IS NULL", [], |r| r.get(0))?;
    let t = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO memos(body, color, pos, created_at, updated_at) VALUES ('', ?1, ?2, ?3, ?3)",
        params![clean_color(color), first.map(|p| p - 1).unwrap_or(0), t],
    )?;
    get(conn, conn.last_insert_rowid())
}

/// 글이나 색을 바꾼다 (None 이면 그대로)
pub fn update(conn: &Connection, id: i64, body: Option<&str>, color: Option<&str>, now: NaiveDateTime) -> AppResult<MemoDto> {
    let cur = get(conn, id)?;
    let body = body.map(|b| b.chars().take(MEMO_MAX_CHARS).collect::<String>()).unwrap_or(cur.body);
    let color = color.map(clean_color).unwrap_or(clean_color(&cur.color));
    conn.execute("UPDATE memos SET body=?1, color=?2, updated_at=?3 WHERE id=?4", params![body, color, util::fmt_dt(now), id])?;
    get(conn, id)
}

/// 지우기: 표에는 남겨 두어 [되돌리기] 할 수 있다. 오래 지난 지운 메모는 이때 정리한다.
pub fn delete(conn: &Connection, id: i64, now: NaiveDateTime) -> AppResult<()> {
    get(conn, id)?;
    conn.execute("UPDATE memos SET deleted_at=?1 WHERE id=?2", params![util::fmt_dt(now), id])?;
    conn.execute("DELETE FROM memos WHERE deleted_at IS NOT NULL AND deleted_at < ?1", params![util::fmt_dt(now - Duration::days(KEEP_DELETED_DAYS))])?;
    Ok(())
}

pub fn restore(conn: &Connection, id: i64) -> AppResult<MemoDto> {
    conn.execute("UPDATE memos SET deleted_at=NULL WHERE id=?1", params![id])?;
    get(conn, id)
}

/// 앞뒤로 한 칸 옮기기 (-1 = 앞으로, 1 = 뒤로)
pub fn shift(conn: &Connection, id: i64, dir: i64) -> AppResult<Vec<MemoDto>> {
    let mut v = list(conn)?;
    if let Some(i) = v.iter().position(|m| m.id == id) {
        let j = i as i64 + dir.signum();
        if j >= 0 && (j as usize) < v.len() {
            v.swap(i, j as usize);
        }
    }
    for (k, m) in v.iter().enumerate() {
        conn.execute("UPDATE memos SET pos=?1 WHERE id=?2", params![k as i64, m.id])?;
    }
    list(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn at(s: &str) -> NaiveDateTime {
        util::parse_dt(s).unwrap()
    }

    #[test]
    fn memos_stay_across_days_and_can_be_restored() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let a = create(c, "yellow", at("2026-10-02T08:00:00"))?;
            let b = create(c, "없는색", at("2026-10-02T08:01:00"))?;
            assert_eq!(b.color, "yellow", "모르는 색은 노랑");
            // 새 메모가 맨 앞
            assert_eq!(list(c)?.iter().map(|m| m.id).collect::<Vec<_>>(), vec![b.id, a.id]);
            update(c, a.id, Some("1학년 협의회 자료 챙기기"), Some("pink"), at("2026-10-03T09:00:00"))?;
            // 날이 바뀌어도 그대로
            let m = get(c, a.id)?;
            assert_eq!((m.body.as_str(), m.color.as_str(), m.updated_at.as_str()), ("1학년 협의회 자료 챙기기", "pink", "2026-10-03T09:00:00"));
            // 색만 바꾸면 글은 그대로
            update(c, a.id, None, Some("blue"), at("2026-10-04T09:00:00"))?;
            assert_eq!(get(c, a.id)?.body, "1학년 협의회 자료 챙기기");
            // 순서 바꾸기
            assert_eq!(shift(c, a.id, -1)?.iter().map(|m| m.id).collect::<Vec<_>>(), vec![a.id, b.id]);
            // 지우면 목록에서 빠지고 되돌리면 돌아온다
            delete(c, a.id, at("2026-10-05T09:00:00"))?;
            assert_eq!(list(c)?.len(), 1);
            restore(c, a.id)?;
            assert_eq!(list(c)?.len(), 2);
            // 30일 지난 지운 메모는 다음에 지울 때 정리
            delete(c, b.id, at("2026-10-05T09:00:00"))?;
            delete(c, a.id, at("2026-11-10T09:00:00"))?;
            let left: i64 = c.query_row("SELECT COUNT(*) FROM memos", [], |r| r.get(0))?;
            assert_eq!(left, 1, "b(30일 넘게 지난 지운 메모)는 정리, a 는 남김");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn long_memo_is_cut() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let m = create(c, "green", at("2026-10-02T08:00:00"))?;
            let long = "가".repeat(MEMO_MAX_CHARS + 10);
            assert_eq!(update(c, m.id, Some(&long), None, at("2026-10-02T08:01:00"))?.body.chars().count(), MEMO_MAX_CHARS);
            Ok(())
        })
        .unwrap();
    }
}
