//! 오늘 화면 칸 배치 (자동 / 직접)
//!
//! - 자동: 화면 너비에 맞춰 1~4열로 정해진 순서대로 꽉 채운다 (화면이 정한다, 여기에는 모드만 저장)
//! - 직접: 열마다 칸 순서와 맨 아래 넓은 줄, 숨긴 칸을 저장한다
//!
//! 앱 설정(AppSettings)과 따로 저장해, 다른 설정을 저장할 때 화면에 남은 예전 배치로 덮어쓰지 않는다.

use crate::db;
use crate::error::AppResult;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

const KEY: &str = "today_layout";

/// 오늘 화면의 칸: 할 일, 오늘 업무 요약, 메모, 오늘 시간표, 급식, 오늘의 기록, 생일·최근 알림
pub const PANELS: [&str; 7] = ["tasks", "briefing", "memo", "timetable", "meals", "notes", "extras"];
pub const MAX_COLUMNS: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TodayLayout {
    /// auto | custom
    pub mode: String,
    /// 직접 배치: 열마다 위에서 아래로 칸 이름 (1~4열)
    pub columns: Vec<Vec<String>>,
    /// 직접 배치: 열 아래에 화면 너비 전체로 놓는 칸
    pub wide: Vec<String>,
    /// 숨긴 칸
    pub hidden: Vec<String>,
}

impl Default for TodayLayout {
    fn default() -> Self {
        TodayLayout { mode: "auto".into(), columns: vec![], wide: vec![], hidden: vec![] }
    }
}

/// 모르는 칸·겹친 칸을 빼고, 빠진 칸은 마지막 열 끝에 붙인다
pub fn sanitize(mut l: TodayLayout) -> TodayLayout {
    if l.mode != "custom" {
        l.mode = "auto".into();
    }
    let mut seen: Vec<String> = Vec::new();
    let mut keep = |v: &mut Vec<String>| {
        v.retain(|p| {
            if PANELS.contains(&p.as_str()) && !seen.contains(p) {
                seen.push(p.clone());
                true
            } else {
                false
            }
        });
    };
    l.columns.truncate(MAX_COLUMNS);
    for c in l.columns.iter_mut() {
        keep(c);
    }
    keep(&mut l.wide);
    keep(&mut l.hidden);
    if l.columns.is_empty() {
        l.columns.push(vec![]);
    }
    for p in PANELS {
        if !seen.iter().any(|s| s == p) {
            l.columns.last_mut().expect("열 하나 이상").push(p.to_string());
        }
    }
    l
}

pub fn load(conn: &Connection) -> AppResult<TodayLayout> {
    Ok(sanitize(db::get_setting(conn, KEY)?.and_then(|s| serde_json::from_str::<TodayLayout>(&s).ok()).unwrap_or_default()))
}

pub fn save(conn: &Connection, l: TodayLayout) -> AppResult<TodayLayout> {
    let l = sanitize(l);
    db::set_setting(conn, KEY, &serde_json::to_string(&l)?)?;
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn default_is_auto_with_every_panel() {
        let l = sanitize(TodayLayout::default());
        assert_eq!(l.mode, "auto");
        assert_eq!(l.columns.concat().len(), PANELS.len());
    }

    #[test]
    fn custom_layout_is_cleaned_and_saved() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let saved = save(
                c,
                TodayLayout {
                    mode: "custom".into(),
                    // 모르는 칸·겹친 칸은 빠지고, 5열째는 버린다
                    columns: vec![s(&["memo", "tasks", "없는칸"]), s(&["briefing", "memo"]), s(&[]), s(&["meals"]), s(&["timetable"])],
                    wide: s(&["notes"]),
                    hidden: s(&["extras"]),
                },
            )?;
            assert_eq!(saved.mode, "custom");
            assert_eq!(saved.columns, vec![s(&["memo", "tasks"]), s(&["briefing"]), s(&[]), s(&["meals", "timetable"])], "빠진 시간표는 마지막 열 끝에");
            assert_eq!(saved.wide, s(&["notes"]));
            assert_eq!(saved.hidden, s(&["extras"]));
            assert_eq!(load(c)?, saved);
            // 모르는 모드는 자동
            assert_eq!(save(c, TodayLayout { mode: "x".into(), ..saved })?.mode, "auto");
            Ok(())
        })
        .unwrap();
    }
}
