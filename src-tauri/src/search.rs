//! 검색: SQLite FTS5(trigram)로 원문·요약을 찾는다.
//! trigram 은 3글자 이상 낱말에 쓰고, 1~2글자 낱말("지각")은 LIKE 로 함께 거른다.

use crate::error::AppResult;
use crate::notes::{self, NoteDto};
use crate::schedule::{self, EventDto, TaskDto};
use crate::students::{self, StudentDto};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SearchQuery {
    pub text: String,
    #[serde(default)]
    pub student_id: Option<i64>,
    #[serde(default)]
    pub duty_id: Option<i64>,
    /// all | unclassified | classified
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteHit {
    pub note: NoteDto,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub terms: Vec<String>,
    pub notes: Vec<NoteHit>,
    pub students: Vec<StudentDto>,
    pub tasks: Vec<TaskDto>,
    pub events: Vec<EventDto>,
}

fn fts_phrase(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

fn like_pattern(term: &str) -> String {
    let esc = term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{esc}%")
}

/// 검색어가 처음 나오는 곳 앞뒤를 잘라 보여 준다
pub fn snippet(text: &str, terms: &[String], width: usize) -> String {
    let lower = text.to_lowercase();
    let pos = terms.iter().filter_map(|t| lower.find(&t.to_lowercase())).min();
    let chars: Vec<char> = text.chars().collect();
    let Some(byte_pos) = pos else {
        return chars.iter().take(width * 2).collect::<String>();
    };
    let char_pos = text[..byte_pos.min(text.len())].chars().count();
    let start = char_pos.saturating_sub(width);
    let end = (char_pos + width).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    if start > 0 {
        s = format!("…{s}");
    }
    if end < chars.len() {
        s.push('…');
    }
    s.replace('\n', " ")
}

pub fn search(conn: &Connection, q: &SearchQuery) -> AppResult<SearchResult> {
    let terms: Vec<String> = q.text.split_whitespace().map(|s| s.to_string()).filter(|s| !s.is_empty()).take(8).collect();
    let mut result = SearchResult { terms: terms.clone(), notes: vec![], students: vec![], tasks: vec![], events: vec![] };
    let has_filter = q.student_id.is_some() || q.duty_id.is_some() || q.from.is_some() || q.to.is_some();
    if terms.is_empty() && !has_filter {
        return Ok(result);
    }
    let mut w: Vec<String> = vec!["1=1".into()];
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let long: Vec<&String> = terms.iter().filter(|t| t.chars().count() >= 3).collect();
    if !long.is_empty() {
        let expr = long.iter().map(|t| fts_phrase(t)).collect::<Vec<_>>().join(" AND ");
        args.push(Box::new(expr));
        w.push(format!("id IN (SELECT rowid FROM notes_fts WHERE notes_fts MATCH ?{})", args.len()));
    }
    for t in terms.iter().filter(|t| t.chars().count() < 3) {
        args.push(Box::new(like_pattern(t)));
        let i = args.len();
        w.push(format!("(raw_text LIKE ?{i} ESCAPE '\\' OR IFNULL(summary,'') LIKE ?{i} ESCAPE '\\')"));
    }
    if let Some(s) = q.student_id {
        args.push(Box::new(s));
        w.push(format!("id IN (SELECT note_id FROM note_students WHERE student_id=?{})", args.len()));
    }
    if let Some(d) = q.duty_id {
        args.push(Box::new(d));
        let i = args.len();
        w.push(format!(
            "(id IN (SELECT note_id FROM duty_logs WHERE duty_id=?{i}) OR id IN (SELECT note_id FROM tasks WHERE duty_id=?{i}))"
        ));
    }
    match q.status.as_deref() {
        Some("unclassified") => w.push("status IN ('pending','processing','failed')".into()),
        Some("classified") => w.push("status IN ('classified','manual')".into()),
        _ => {}
    }
    if let Some(f) = crate::util::opt_date(q.from.as_deref())? {
        args.push(Box::new(f));
        w.push(format!("substr(created_at,1,10) >= ?{}", args.len()));
    }
    if let Some(t) = crate::util::opt_date(q.to.as_deref())? {
        args.push(Box::new(t));
        w.push(format!("substr(created_at,1,10) <= ?{}", args.len()));
    }
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let sql = format!("{} ORDER BY created_at DESC LIMIT {limit}", w.join(" AND "));
    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    for n in notes::query_notes(conn, &sql, &refs)? {
        let snip = snippet(&n.raw_text, &terms, 40);
        result.notes.push(NoteHit { note: n, snippet: snip });
    }

    if !terms.is_empty() {
        let first = terms[0].as_str();
        result.students = students::list_students(conn, Some(first), None)?.into_iter().take(10).collect();
        let pat = like_pattern(first);
        result.tasks = schedule::query_tasks(conn, "title LIKE ?1 ESCAPE '\\' ORDER BY done, due_date IS NULL, due_date LIMIT 20", &[&pat])?;
        result.events = schedule::query_events(conn, "title LIKE ?1 ESCAPE '\\' ORDER BY start_date DESC LIMIT 20", &[&pat])?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::util;

    #[test]
    fn finds_by_long_and_short_terms() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let now = util::parse_dt("2026-09-29T09:00:00").unwrap();
        notes::create_note(&c, "오늘 3교시 2-1 김민수 지각. 금요일까지 보호자 연락.", now).unwrap();
        notes::create_note(&c, "10월 15일 교육장 공감소통 방문", now).unwrap();
        let r = search(&c, &SearchQuery { text: "김민수".into(), ..Default::default() }).unwrap();
        assert_eq!(r.notes.len(), 1);
        assert!(r.notes[0].snippet.contains("김민수"));
        let r = search(&c, &SearchQuery { text: "지각".into(), ..Default::default() }).unwrap();
        assert_eq!(r.notes.len(), 1);
        let r = search(&c, &SearchQuery { text: "공감소통 방문".into(), ..Default::default() }).unwrap();
        assert_eq!(r.notes.len(), 1);
        // 부분 일치 (낱말 중간)
        let r = search(&c, &SearchQuery { text: "감소통".into(), ..Default::default() }).unwrap();
        assert_eq!(r.notes.len(), 1);
        let r = search(&c, &SearchQuery { text: "없는말".into(), ..Default::default() }).unwrap();
        assert!(r.notes.is_empty());
        // 특수문자가 있어도 오류가 나지 않는다
        let r = search(&c, &SearchQuery { text: "\"김민수 OR *".into(), ..Default::default() }).unwrap();
        assert!(r.notes.is_empty());
    }

    #[test]
    fn snippet_window() {
        let s = snippet("가나다라마바사아자차카타파하 김민수 지각", &["김민수".into()], 3);
        assert!(s.contains("김민수"));
        assert!(s.starts_with('…'));
    }
}
