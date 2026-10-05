//! 오늘 업무 요약 (오늘 화면): 학사력(휴일·휴업일·방학·시험), 오늘 일정, 오늘 시간표(수업·공강·변경),
//! 오늘 마감·기한 지난 할 일, 오늘 업무 기록에서 '사실'을 모아 3~5줄로 보여 준다. 기록 원문은 다시 보여 주지 않는다.
//!
//! - 규칙 요약: 언제나 바로 만든다 (시간순, 휴일·시험은 맨 위).
//! - AI 요약: [새로고침]을 누를 때만 만든다. 줄마다 근거 사실 번호가 있어야 하고, 사실에 없는 표현이 많은 줄은 버린다.
//!   만든 요약은 그날·그 사실 그대로일 때만 다시 보여 준다 (사실이 바뀌면 규칙 요약으로 돌아간다).

use crate::db;
use crate::error::AppResult;
use crate::schedule;
use crate::text;
use crate::timetable;
use crate::util;
use chrono::NaiveDateTime;
use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// 요약의 근거가 되는 사실 하나
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Fact {
    pub id: usize,
    /// "09:00" (없으면 시간 없음)
    pub time: Option<String>,
    pub text: String,
    /// day 학사력 | event 일정 | class 수업 | change 시간표 변경 | task 할 일 | duty 업무 | overdue 기한 지남
    pub kind: String,
    /// 누르면 갈 곳: event | task | duty_log | timetable | tasks
    pub target_kind: String,
    pub target_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryLine {
    pub text: String,
    pub time: Option<String>,
    pub target_kind: String,
    pub target_id: Option<i64>,
    /// 휴일·시험처럼 맨 위에 강조할 줄
    pub top: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Briefing {
    pub date: String,
    pub lines: Vec<SummaryLine>,
    /// rule | ai
    pub by: String,
    pub generated_at: Option<String>,
    /// 사실이 하나도 없음 (오늘 할 것이 없음)
    pub empty: bool,
}

static RE_EXAM: Lazy<Regex> = Lazy::new(|| Regex::new(r"고사|시험|평가").unwrap());

fn kind_label(k: &str) -> &'static str {
    match k {
        "holiday" => "휴일",
        "closure" => "휴업일",
        "vacation" => "방학",
        _ => "",
    }
}

/// 오늘의 사실 목록 (시간순: 학사력 → 시간 있는 것 → 시간 없는 것)
pub fn facts(conn: &Connection, now: NaiveDateTime) -> AppResult<Vec<Fact>> {
    let today = util::fmt_d(now.date());
    let mut out: Vec<Fact> = Vec::new();
    let mut push = |time: Option<String>, text: String, kind: &str, target_kind: &str, target_id: Option<i64>| {
        out.push(Fact { id: 0, time, text, kind: kind.into(), target_kind: target_kind.into(), target_id });
    };
    // 1) 학사력: 휴일·휴업일·방학·시험은 맨 위
    let events = schedule::query_events(conn, "start_date <= ?1 AND end_date >= ?1 AND ext_source<>'removed' ORDER BY time IS NULL, time, id", &[&today])?;
    let mut day_off = false;
    let mut banner_ids: Vec<i64> = Vec::new();
    for e in events.iter().filter(|e| e.layer == "annual") {
        let k = kind_label(&e.day_kind);
        if !k.is_empty() && !(e.hidden && now.date().format("%u").to_string() != "6") {
            day_off |= e.day_kind != "work";
            push(None, format!("오늘은 {}({k})", e.title), "day", "event", Some(e.id));
            banner_ids.push(e.id);
        } else if RE_EXAM.is_match(&e.title) && !e.hidden {
            push(None, format!("오늘은 {}", e.title), "day", "event", Some(e.id));
            banner_ids.push(e.id);
        }
    }
    // 2) 오늘 일정 (시간·메모 포함)
    for e in events.iter().filter(|e| !e.hidden && !banner_ids.contains(&e.id)) {
        let mut t = e.title.clone();
        if !e.memo.trim().is_empty() {
            t.push_str(&format!(" — {}", text::truncate_chars(e.memo.trim(), 40)));
        }
        let shown = match &e.time {
            Some(tm) => format!("{tm} {t}"),
            None => t,
        };
        push(e.time.clone(), shown, "event", "event", Some(e.id));
    }
    // 3) 오늘 시간표 (휴일·방학이 아닐 때): 수업·공강 한 줄, 시간표 변경은 따로
    if !day_off {
        let tt = timetable::today(conn, now)?;
        let classes: Vec<&timetable::TodaySlot> = tt.slots.iter().filter(|s| s.period.kind == "class").collect();
        let taught: Vec<String> = classes
            .iter()
            .filter_map(|s| s.cell.as_ref().map(|c| format!("{} {}", s.period.label, [c.class_label.as_str(), c.subject.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" "))))
            .collect();
        if !taught.is_empty() {
            let free: Vec<String> = classes.iter().filter(|s| s.cell.is_none()).map(|s| s.period.no.to_string()).collect();
            let first = classes.iter().find(|s| s.cell.is_some()).map(|s| s.period.start.clone());
            let mut t = format!("수업 {}시간: {}", taught.len(), taught.join(", "));
            if !free.is_empty() {
                t.push_str(&format!(" (공강 {}교시)", free.join("·")));
            }
            push(first, t, "class", "timetable", None);
        }
        for s in tt.slots.iter().filter(|s| s.change.is_some()) {
            let c = s.change.as_ref().unwrap();
            let what = [c.change_type.as_str(), c.class_label.as_str(), c.subject.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ");
            push(Some(s.period.start.clone()), format!("시간표 변경: {} {what}", s.period.label), "change", "timetable", None);
        }
    }
    // 4) 오늘 마감 할 일·업무
    let due = schedule::query_tasks(conn, "done=0 AND due_date=?1 ORDER BY due_time IS NULL, due_time, id", &[&today])?;
    for t in &due {
        let text = match &t.due_time {
            Some(tm) => format!("{tm}까지 {}", t.title),
            None => format!("오늘까지 {}", t.title),
        };
        push(t.due_time.clone(), text, "task", "task", Some(t.id));
    }
    let duties = schedule::query_duty_logs(conn, "done=0 AND date=?1 ORDER BY time IS NULL, time, id", &[&today])?;
    for d in &duties {
        let label = d.duty_label.clone().unwrap_or_else(|| "업무".into());
        let text = match &d.time {
            Some(tm) => format!("{tm} [{label}] {}", text::truncate_chars(&d.content, 40)),
            None => format!("[{label}] {}", text::truncate_chars(&d.content, 40)),
        };
        push(d.time.clone(), text, "duty", "duty_log", Some(d.id));
    }
    // 5) 기한 지난 할 일 (한 줄로)
    let overdue = schedule::query_tasks(conn, "done=0 AND due_date IS NOT NULL AND due_date<?1 ORDER BY due_date, id", &[&today])?;
    if !overdue.is_empty() {
        let names: Vec<String> = overdue.iter().take(2).map(|t| t.title.clone()).collect();
        let more = if overdue.len() > 2 { format!(" 외 {}건", overdue.len() - 2) } else { String::new() };
        push(None, format!("기한 지난 할 일 {}건: {}{more}", overdue.len(), names.join(", ")), "overdue", "tasks", None);
    }
    // 학사력 → 시간 있는 것(시간순) → 시간 없는 것 (일정 → 할 일 → 업무 → 기한 지남)
    // 휴일·휴업일·방학 → 시험 → 시간 있는 것 → 일정 → 할 일 → 업무 → 기한 지남
    let rank = |f: &Fact| match f.kind.as_str() {
        "day" if f.text.ends_with(')') => 0,
        "day" => 1,
        _ if f.time.is_some() => 2,
        "event" => 3,
        "task" => 4,
        "duty" => 5,
        _ => 6,
    };
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.time.cmp(&b.time)));
    for (i, f) in out.iter_mut().enumerate() {
        f.id = i + 1;
    }
    Ok(out)
}

fn line_of(f: &Fact) -> SummaryLine {
    SummaryLine { text: f.text.clone(), time: f.time.clone(), target_kind: f.target_kind.clone(), target_id: f.target_id, top: f.kind == "day" }
}

/// 규칙 요약: 사실을 그대로 시간순으로, 5줄이 넘으면 할 일을 한 줄로 모으고 나머지는 '그 밖에 n건'
pub fn rule_lines(facts: &[Fact]) -> Vec<SummaryLine> {
    let mut lines: Vec<SummaryLine> = Vec::new();
    let tasks: Vec<&Fact> = facts.iter().filter(|f| f.kind == "task" && f.time.is_none()).collect();
    let merge_tasks = facts.len() > 5 && tasks.len() > 1;
    let mut task_line_done = false;
    for f in facts {
        if merge_tasks && f.kind == "task" && f.time.is_none() {
            if !task_line_done {
                let names: Vec<String> = tasks.iter().take(2).map(|t| t.text.trim_start_matches("오늘까지 ").to_string()).collect();
                let more = if tasks.len() > 2 { format!(" 외 {}건", tasks.len() - 2) } else { String::new() };
                lines.push(SummaryLine {
                    text: format!("오늘 마감 할 일 {}건: {}{more}", tasks.len(), names.join(", ")),
                    time: None,
                    target_kind: "tasks".into(),
                    target_id: None,
                    top: false,
                });
                task_line_done = true;
            }
            continue;
        }
        lines.push(line_of(f));
    }
    if lines.len() > 5 {
        let rest = lines.len() - 4;
        lines.truncate(4);
        lines.push(SummaryLine { text: format!("그 밖에 {rest}건 — 할 일·일정 보기"), time: None, target_kind: "tasks".into(), target_id: None, top: false });
    }
    lines
}

pub fn facts_hash(facts: &[Fact]) -> String {
    let mut h = Sha256::new();
    for f in facts {
        h.update(format!("{}|{:?}|{}|{}|{:?}\n", f.kind, f.time, f.text, f.target_kind, f.target_id).as_bytes());
    }
    hex::encode(h.finalize())[..16].to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cached {
    date: String,
    hash: String,
    generated_at: String,
    lines: Vec<SummaryLine>,
}

const CACHE_KEY: &str = "today_briefing";

/// 오늘 요약: 같은 날·같은 사실로 만든 AI 요약이 있으면 그것, 없으면 규칙 요약
pub fn load(conn: &Connection, now: NaiveDateTime) -> AppResult<Briefing> {
    let f = facts(conn, now)?;
    let date = util::fmt_d(now.date());
    let hash = facts_hash(&f);
    if let Some(c) = db::get_setting(conn, CACHE_KEY)?.and_then(|s| serde_json::from_str::<Cached>(&s).ok()) {
        if c.date == date && c.hash == hash && !c.lines.is_empty() {
            return Ok(Briefing { date, lines: c.lines, by: "ai".into(), generated_at: Some(c.generated_at), empty: f.is_empty() });
        }
    }
    Ok(Briefing { date, lines: rule_lines(&f), by: "rule".into(), generated_at: None, empty: f.is_empty() })
}

pub fn store_ai(conn: &Connection, now: NaiveDateTime, facts: &[Fact], lines: &[SummaryLine]) -> AppResult<()> {
    let c = Cached { date: util::fmt_d(now.date()), hash: facts_hash(facts), generated_at: util::fmt_dt(now), lines: lines.to_vec() };
    db::set_setting(conn, CACHE_KEY, &serde_json::to_string(&c)?)
}

// ───────────────────────── AI ─────────────────────────

pub fn ai_body(facts: &[Fact]) -> Value {
    let list = facts.iter().map(|f| format!("[{}] {}", f.id, f.text)).collect::<Vec<_>>().join("\n");
    let system = "너는 중학교 교사의 하루 업무를 아침에 한눈에 보도록 요약하는 도우미다.
규칙
1. 아래 '오늘의 사실' 목록에 있는 내용만 쓴다. 목록에 없는 일, 시간, 장소, 평가, 조언을 만들지 않는다.
2. 3~5줄로 쓴다. 비슷한 사실은 한 줄로 묶는다. 시간 순서를 지킨다.
3. '오늘은 ~(휴일)', '오늘은 ~고사'처럼 오늘이 어떤 날인지 알려 주는 사실이 있으면 첫 줄에 쓴다.
4. 줄마다 근거 사실 번호를 refs에 적는다.
5. 한 줄은 60자를 넘지 않는다.";
    json!({
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": format!("오늘의 사실:\n{list}") }
        ],
        "temperature": 0.2,
        "top_p": 0.9,
        "seed": 7,
        "max_tokens": 500,
        "chat_template_kwargs": { "enable_thinking": false },
        "response_format": { "type": "json_schema", "json_schema": { "name": "briefing", "strict": true, "schema": {
            "type": "object",
            "properties": { "lines": { "type": "array", "minItems": 1, "maxItems": 5, "items": {
                "type": "object",
                "properties": { "text": { "type": "string", "maxLength": 120 }, "refs": { "type": "array", "minItems": 1, "maxItems": 8, "items": { "type": "integer" } } },
                "required": ["text", "refs"],
                "additionalProperties": false
            } } },
            "required": ["lines"],
            "additionalProperties": false
        } } }
    })
}

#[derive(Debug, Deserialize)]
struct AiOut {
    lines: Vec<AiLine>,
}

#[derive(Debug, Deserialize)]
struct AiLine {
    text: String,
    refs: Vec<usize>,
}

/// AI 요약 검증: 근거 번호가 맞고, 근거 사실에 없는 표현이 많지 않은 줄만. 학사력 줄이 빠졌으면 맨 위에 붙인다.
/// 쓸 수 있는 줄이 없으면 None (규칙 요약을 쓴다)
pub fn guard_ai(raw: &str, facts: &[Fact]) -> Option<Vec<SummaryLine>> {
    let s = raw.trim();
    let out: AiOut = serde_json::from_str(s).ok().or_else(|| {
        let (a, b) = (s.find('{')?, s.rfind('}')?);
        serde_json::from_str(&s[a..=b]).ok()
    })?;
    let mut lines: Vec<SummaryLine> = Vec::new();
    let mut used: Vec<usize> = Vec::new();
    for l in out.lines.iter().take(5) {
        let refs: Vec<&Fact> = l.refs.iter().filter_map(|r| facts.iter().find(|f| f.id == *r)).collect();
        let t = text::tidy(&l.text);
        if refs.is_empty() || t.is_empty() {
            continue;
        }
        let source = refs.iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join(" ");
        if text::coverage(&t, &source) < 0.7 {
            continue;
        }
        used.extend(refs.iter().map(|f| f.id));
        let first = refs[0];
        lines.push(SummaryLine {
            text: text::truncate_chars(&t, 80),
            time: refs.iter().filter_map(|f| f.time.clone()).min(),
            target_kind: first.target_kind.clone(),
            target_id: first.target_id,
            top: refs.iter().any(|f| f.kind == "day"),
        });
    }
    if lines.is_empty() {
        return None;
    }
    // 휴일·시험 사실을 빠뜨렸으면 맨 위에
    for f in facts.iter().filter(|f| f.kind == "day" && !used.contains(&f.id)).rev() {
        lines.insert(0, line_of(f));
    }
    lines.sort_by_key(|l| !l.top);
    lines.truncate(6);
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn now() -> NaiveDateTime {
        // 2026-10-02 (금) 08:10
        util::parse_dt("2026-10-02T08:10:00").unwrap()
    }

    fn seed(c: &Connection) {
        c.execute_batch(
            "INSERT INTO events(layer, title, start_date, end_date, time, memo, source, day_kind, created_at, updated_at) VALUES
               ('note', '한일국제교류 컨설팅', '2026-10-02', '2026-10-02', '11:00', '본관 2층 학생회의실', 'ai', '', 'x', 'x'),
               ('annual', '중간고사', '2026-10-02', '2026-10-02', NULL, '', 'manual', 'work', 'x', 'x');
             INSERT INTO tasks(title, due_date, done, source, remind_enabled, created_at, updated_at) VALUES
               ('보호자 연락', '2026-10-02', 0, 'ai', 0, 'x', 'x'),
               ('결석계 받기', '2026-09-30', 0, 'ai', 0, 'x', 'x'),
               ('끝난 일', '2026-10-02', 1, 'ai', 0, 'x', 'x');",
        )
        .unwrap();
        let cell = |wd, p, subj: &str, cls: &str| timetable::Cell { weekday: wd, period: p, subject: subj.into(), class_label: cls.into(), ..Default::default() };
        timetable::set_cell(c, 2026, 2, "personal", "", &cell(5, 1, "사회", "1-1"), now()).unwrap();
        timetable::set_cell(c, 2026, 2, "personal", "", &cell(5, 3, "역사", "2-1"), now()).unwrap();
    }

    #[test]
    fn facts_in_time_order_with_exam_on_top() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        seed(&c);
        let f = facts(&c, now()).unwrap();
        let texts: Vec<&str> = f.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts[0], "오늘은 중간고사", "{texts:?}");
        assert!(texts[1].starts_with("수업 2시간: 1교시 1-1 사회, 3교시 2-1 역사 (공강"), "{texts:?}");
        assert_eq!(texts[2], "11:00 한일국제교류 컨설팅 — 본관 2층 학생회의실");
        assert_eq!(texts[3], "오늘까지 보호자 연락");
        assert_eq!(texts[4], "기한 지난 할 일 1건: 결석계 받기");
        assert!(!texts.iter().any(|t| t.contains("끝난 일")));
        let lines = rule_lines(&f);
        assert!(lines.len() <= 5 && lines[0].top);
        assert_eq!((lines[2].target_kind.as_str(), lines[2].time.as_deref()), ("event", Some("11:00")));
    }

    #[test]
    fn holiday_hides_timetable() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        seed(&c);
        c.execute(
            "INSERT INTO events(layer, title, start_date, end_date, source, day_kind, created_at, updated_at) VALUES ('annual', '개교기념일', '2026-10-02', '2026-10-02', 'manual', 'closure', 'x', 'x')",
            [],
        )
        .unwrap();
        let f = facts(&c, now()).unwrap();
        assert_eq!(f[0].text, "오늘은 개교기념일(휴업일)");
        assert!(!f.iter().any(|x| x.kind == "class"), "휴업일에는 수업 줄을 넣지 않는다");
    }

    #[test]
    fn ai_guard_and_cache() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        seed(&c);
        let f = facts(&c, now()).unwrap();
        // 근거 없는 줄·지어낸 줄은 버리고, 빠진 시험 줄은 맨 위에 붙인다
        let raw = r#"{"lines":[
            {"text":"1교시 1-1 사회, 3교시 2-1 역사 수업","refs":[2]},
            {"text":"11시 한일국제교류 컨설팅 (본관 2층 학생회의실)","refs":[3]},
            {"text":"교장선생님께 결재 올리기","refs":[4]},
            {"text":"오늘까지 보호자 연락","refs":[]}]}"#;
        let lines = guard_ai(raw, &f).unwrap();
        assert_eq!(lines[0].text, "오늘은 중간고사");
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(guard_ai("엉망", &f).is_none());
        // 같은 사실이면 AI 요약을 다시 보여 주고, 사실이 바뀌면 규칙 요약
        store_ai(&c, now(), &f, &lines).unwrap();
        assert_eq!(load(&c, now()).unwrap().by, "ai");
        c.execute("UPDATE tasks SET done=1 WHERE title='보호자 연락'", []).unwrap();
        assert_eq!(load(&c, now()).unwrap().by, "rule");
    }
}
