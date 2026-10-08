//! 분류 계획(Plan): 기록 하나에서 만들 학생 관찰·할 일·일정·업무 기록의 목록.
//!
//! - `rule_plan`: 규칙만으로 만든 제안 (AI가 없을 때 수동 분류 화면에 미리 채움)
//! - `guard_llm`: AI가 낸 결과를 원문 근거·출결·할 일 규칙으로 검증하고 보정한 최종 계획
//!
//! 모든 항목의 문구는 원문에서 발췌한 것이다. 원문에 없는 표현은 원문 문장으로 바꾸거나 버린다.

use crate::analysis::{self, Analysis, Area, DutyEntry, ItemKind, RosterEntry, StudentMention};
use crate::dates::{self, DateHit};
use crate::text::{self, Span};
use crate::util;
use chrono::NaiveDate;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanStudent {
    #[serde(default)]
    pub student_id: Option<i64>,
    pub name: String,
    #[serde(default)]
    pub class_label: Option<String>,
    #[serde(default)]
    pub number: Option<i64>,
    /// 동명이인 후보 — 교사가 골라야 한다
    #[serde(default)]
    pub candidates: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReminderSpec {
    pub enabled: bool,
    #[serde(default)]
    pub offsets: Vec<i64>,
    #[serde(default = "default_remind_time")]
    pub time: String,
    #[serde(default)]
    pub custom_at: Option<String>,
}

fn default_remind_time() -> String {
    "08:30".to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ItemOrigin {
    /// AI가 찾은 항목
    Ai,
    /// 규칙으로 찾거나 보완한 항목
    Rule,
    /// 교사가 직접 만든 항목
    #[default]
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanItem {
    pub kind: ItemKind,
    pub text: String,
    #[serde(default)]
    pub students: Vec<PlanStudent>,
    #[serde(default = "area_none")]
    pub area: Area,
    #[serde(default)]
    pub duty_id: Option<i64>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub date_expr: Option<String>,
    #[serde(default)]
    pub approximate: bool,
    #[serde(default)]
    pub remind: Option<ReminderSpec>,
    #[serde(default)]
    pub origin: ItemOrigin,
    /// 완료 여부 (할 일·업무) — 분류를 고쳐 다시 저장할 때 유지한다
    #[serde(default)]
    pub done: bool,
    /// 교사가 AI 결과를 고쳤는지
    #[serde(default)]
    pub edited: bool,
    /// 일정의 메모 (같은 문장의 장소·방문자 같은 설명)
    #[serde(default)]
    pub memo: Option<String>,
}

fn area_none() -> Area {
    Area::None
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Plan {
    pub summary: String,
    pub items: Vec<PlanItem>,
    #[serde(default)]
    pub guard_notes: Vec<String>,
    #[serde(default)]
    pub unknown_names: Vec<String>,
    /// 기록 속 학생 개인정보 → 학생 명렬표 (형식 규칙으로 찾는다)
    #[serde(default)]
    pub profile: Vec<crate::profile::ProfileItem>,
    /// 특이사항으로 보이는 문장 → 어디에 둘지 교사에게 묻는다
    #[serde(default)]
    pub specials: Vec<crate::profile::SpecialItem>,
}

pub struct Ctx<'a> {
    pub today: NaiveDate,
    pub roster: &'a [RosterEntry],
    pub duties: &'a [DutyEntry],
}

/// AI가 내는 JSON (llama-server 의 JSON 스키마 제약으로 형식은 보장된다)
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LlmOutput {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub items: Vec<LlmItem>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LlmItem {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub students: Vec<String>,
    #[serde(default)]
    pub area: String,
    #[serde(default)]
    pub duty: i64,
    #[serde(default)]
    pub date: String,
}

pub fn parse_llm_json(s: &str) -> Option<LlmOutput> {
    if let Ok(v) = serde_json::from_str::<LlmOutput>(s.trim()) {
        return Some(v);
    }
    let a = s.find('{')?;
    let b = s.rfind('}')?;
    if b <= a {
        return None;
    }
    serde_json::from_str::<LlmOutput>(&s[a..=b]).ok()
}

const MAX_ITEMS: usize = 16;
const SUMMARY_MAX: usize = 60;

fn plan_student(m: &StudentMention) -> PlanStudent {
    PlanStudent {
        student_id: m.roster_id,
        name: m.name.clone(),
        class_label: m.class_label.clone(),
        number: m.number,
        candidates: m.candidates.clone(),
    }
}

fn same_student(a: &PlanStudent, b: &PlanStudent) -> bool {
    match (a.student_id, b.student_id) {
        (Some(x), Some(y)) => x == y,
        _ => a.name == b.name && (a.class_label.is_none() || b.class_label.is_none() || a.class_label == b.class_label),
    }
}

fn push_student(v: &mut Vec<PlanStudent>, s: PlanStudent) {
    if !v.iter().any(|x| same_student(x, &s)) {
        v.push(s);
    }
}

/// 원문에 적은 학급이 명렬표와 달라 명렬표를 따른 학생 안내
fn class_mismatch_notes(a: &Analysis) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for m in &a.students {
        if let (Some(w), Some(l)) = (&m.written_class, &m.class_label) {
            let s = format!("{} 학생은 원문에 '{w}'(으)로 적혀 있지만 명렬표의 {l}(으)로 연결했습니다. 맞는지 확인해 주세요.", m.name);
            if !v.contains(&s) {
                v.push(s);
            }
        }
    }
    v
}

fn extractive_summary(raw: &str, today: NaiveDate) -> String {
    let first = text::sentences(raw).first().map(|s| text::tidy(s.slice(raw))).unwrap_or_default();
    text::truncate_chars(&strip_leading_when(&first, today), SUMMARY_MAX)
}

/// 항목 문구에서 날짜·시각 표현을 떼어 제목을 만든다
fn item_title(raw: &str, span: Span, date: Option<&DateHit>, time: Option<&dates::TimeHit>) -> String {
    let seg = span.slice(raw);
    let mut remove = Vec::new();
    for (s, e) in [date.map(|d| (d.start, d.end)), time.map(|t| (t.start, t.end))].into_iter().flatten() {
        if s >= span.start && e <= span.end {
            remove.push((s - span.start, e - span.start));
        }
    }
    strip_condition(&analysis::title_from(seg, &remove))
}

/// 할 일 제목 앞의 조건("답 오면", "회신 오는 대로")은 떼어 낸다 — "답 오면 교장 보고" → "교장 보고"
fn strip_condition(title: &str) -> String {
    static RE_COND: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^\s*(?:[^\s]+\s+){0,2}?[^\s]*(?:오면|되면|나오면|받으면|끝나면|들어오면|있으면|없으면|생기면|정해지면|확정되면|마치면|오는\s*대로|되는\s*대로)\s+").unwrap()
    });
    let t = RE_COND.replace(title, "").trim().to_string();
    if t.is_empty() { title.to_string() } else { t }
}

/// 할 일의 날짜: 그 일을 한(지난) 시각은 마감이 아니다. "~까지" 같은 마감 표현이 있거나 오늘 이후 날짜일 때만 쓴다.
fn task_when(mut r: Resolved, today: NaiveDate) -> Resolved {
    if r.date.as_ref().is_some_and(|d| !d.deadline && d.date < today) {
        r.date = None;
        r.time = None;
    }
    r
}

struct Resolved {
    date: Option<DateHit>,
    time: Option<dates::TimeHit>,
}

/// 날짜·시각을 떼고 남는 글자가 있는가 ("10. 2. 11:00," 처럼 날짜뿐인 절인지)
fn has_words_besides_when(a: &Analysis, sp: Span) -> bool {
    let mut cut: Vec<(usize, usize)> = a.dates.iter().map(|d| (d.start, d.end)).collect();
    cut.extend(a.clauses.iter().flat_map(|c| c.times.iter()).map(|t| (t.start, t.end)));
    let raw = a.text.as_str();
    raw[sp.start..sp.end]
        .char_indices()
        .filter(|(i, ch)| {
            let p = sp.start + i;
            ch.is_alphanumeric() && !cut.iter().any(|(s, e)| *s <= p && p < *e)
        })
        .count()
        >= 2
}

/// 일정·할 일의 제목과 메모.
/// "10. 2. 11:00 한일국제교류 컨설팅, OO고 홍길동 선생님 방문, 본관 2층 학생회의실"
///   → 제목 "한일국제교류 컨설팅", 메모 "OO고 홍길동 선생님 방문, 본관 2층 학생회의실"
/// 날짜가 든 절이 제목이고, 같은 문장의 나머지 절 중 할 일·일정·학생·출결이 아닌 설명 절이 메모가 된다.
fn title_and_memo(a: &Analysis, span: Span, r: &Resolved) -> (String, Option<String>) {
    let raw = a.text.as_str();
    let date_in_span = r.date.as_ref().filter(|d| span.contains(d.start));
    let head = date_in_span
        .and_then(|d| a.clauses.iter().position(|c| c.span.contains(d.start)))
        .or_else(|| a.clauses.iter().position(|c| c.span.contains(span.start)));
    let Some(mut hi) = head else {
        return (item_title(raw, span, date_in_span, r.time.as_ref()), None);
    };
    let sentence = a.clauses[hi].sentence;
    let plain = |i: usize| {
        let c = &a.clauses[i];
        c.sentence == sentence && c.students.is_empty() && c.attendance.is_none() && !a.clause_is_task(i) && !a.clause_is_event(i)
    };
    // 날짜·시각뿐인 절("10월 5일 15시,")이면 바로 뒤(없으면 앞)의 설명 절을 제목으로 쓴다
    let mut when_only: Option<usize> = None;
    if !has_words_besides_when(a, a.clauses[hi].span) {
        let next = (hi + 1 < a.clauses.len() && plain(hi + 1)).then_some(hi + 1);
        let prev = (hi > 0 && plain(hi - 1)).then(|| hi - 1);
        if let Some(n) = next.or(prev) {
            when_only = Some(hi);
            hi = n;
        }
    }
    let hc = a.clauses[hi].span;
    // AI가 절 일부만 짚었으면 그 부분을, 여러 절을 묶었으면 제목 절만 쓴다
    let title_span = if hc.contains(span.start) && span.end <= hc.end { span } else { hc };
    let title = item_title(raw, title_span, r.date.as_ref().filter(|d| title_span.contains(d.start)), r.time.as_ref().filter(|t| title_span.contains(t.start)));
    let memo: Vec<String> = (0..a.clauses.len())
        .filter(|&i| i != hi && Some(i) != when_only && plain(i))
        .map(|i| text::tidy(a.clauses[i].span.slice(raw)))
        .filter(|s| s.chars().filter(|c| c.is_alphanumeric()).count() >= 2)
        .collect();
    let memo = (!memo.is_empty()).then(|| text::truncate_chars(&memo.join(", "), 200));
    (title, memo)
}

/// 맨 앞의 날짜·시각 표현을 뗀다 (카드 제목·요약용: "10. 2. 11:00 한일국제교류 컨설팅" → "한일국제교류 컨설팅")
pub fn strip_leading_when(s: &str, today: NaiveDate) -> String {
    let dates = dates::find_dates(s, today);
    let times = dates::find_times(s);
    let mut pos = 0usize;
    loop {
        let rest = &s[pos..];
        let lead = rest.len() - rest.trim_start_matches([' ', ',', '(', ')']).len();
        let at = pos + lead;
        let next = dates
            .iter()
            .map(|d| (d.start, d.end))
            .chain(times.iter().map(|t| (t.start, t.end)))
            .filter(|(st, _)| *st == at)
            .map(|(_, e)| e)
            .max();
        match next {
            Some(e) if e > at => pos = e,
            _ => break,
        }
    }
    if pos == 0 {
        return s.to_string();
    }
    let mut rest = s[pos..].trim_start_matches([' ', ',', '.', '(', ')']);
    // 날짜 뒤에 남은 조사 ("10월 2일에 회의" → "회의")
    for p in ["에 ", "에,", "까지 ", "부터 "] {
        if let Some(r) = rest.strip_prefix(p) {
            rest = r;
        }
    }
    let out = text::tidy(rest);
    if out.chars().filter(|c| c.is_alphanumeric()).count() < 2 { s.to_string() } else { out }
}

/// 원문 구간에 붙는 날짜·시각
fn resolve_when(a: &Analysis, span: Span, prefer_deadline: bool) -> Resolved {
    let in_span: Vec<&DateHit> = a.dates.iter().filter(|d| span.contains(d.start)).collect();
    let date = if prefer_deadline {
        in_span.iter().find(|d| d.deadline).or(in_span.first()).map(|d| (*d).clone())
    } else {
        in_span.first().map(|d| (*d).clone())
    };
    let date = date.or_else(|| {
        let idx = a.clauses.iter().position(|c| c.span.contains(span.start) || span.contains(c.span.start))?;
        a.clause_date(idx)
    });
    // 시각: 구간 안, 없으면 구간이 속한 절 안 ("다음 주 수요일 오후 3시 교과협의회"에서 "교과협의회"만 짚은 경우)
    let time = a
        .clauses
        .iter()
        .flat_map(|c| c.times.iter())
        .find(|t| span.contains(t.start))
        .or_else(|| {
            a.clauses
                .iter()
                .find(|c| c.span.contains(span.start))
                .and_then(|c| c.times.first())
        })
        .cloned();
    Resolved { date, time }
}

fn observed_on(a: &Analysis, span: Span, date: Option<&DateHit>) -> String {
    let seg = span.slice(&a.text);
    let d = match date {
        Some(h) => {
            if h.date > a.today && analysis::is_past(seg) {
                dates::to_past_occurrence(h, a.today)
            } else {
                h.date
            }
        }
        None => a.today,
    };
    util::fmt_d(d)
}

fn with_when(mut item: PlanItem, r: &Resolved) -> PlanItem {
    if let Some(d) = &r.date {
        item.date = Some(util::fmt_d(d.date));
        item.date_expr = Some(d.expr.clone());
        item.approximate = d.approximate;
    }
    if let Some(t) = &r.time {
        item.time = Some(util::fmt_t(t.time));
    }
    item
}

fn blank(kind: ItemKind, text: String, origin: ItemOrigin) -> PlanItem {
    PlanItem {
        kind,
        text,
        students: vec![],
        area: Area::None,
        duty_id: None,
        date: None,
        time: None,
        date_expr: None,
        approximate: false,
        remind: None,
        origin,
        done: false,
        edited: false,
        memo: None,
    }
}

/// 규칙만으로 만든 분류 계획
pub fn rule_plan(raw: &str, ctx: &Ctx) -> Plan {
    let a = analysis::analyze(raw, ctx.today, ctx.roster, ctx.duties);
    rule_plan_from(&a, ctx)
}

fn rule_plan_from(a: &Analysis, ctx: &Ctx) -> Plan {
    let raw = a.text.as_str();
    let mut items: Vec<PlanItem> = Vec::new();

    // 1) 학생 관찰: 문장 단위
    for (si, sent) in a.sentences.iter().enumerate() {
        let idxs: Vec<usize> = (0..a.clauses.len()).filter(|&i| a.clauses[i].sentence == si).collect();
        if idxs.is_empty() {
            continue;
        }
        let mut students: Vec<PlanStudent> = Vec::new();
        for &i in &idxs {
            for &m in &a.clauses[i].students {
                push_student(&mut students, plan_student(&a.students[m]));
            }
        }
        let attendance = idxs.iter().any(|&i| a.clauses[i].attendance.is_some());
        let hint = idxs.iter().find_map(|&i| a.clauses[i].area_hint);
        let all_tasks_or_events = idxs.iter().all(|&i| a.clause_is_task(i) || a.clause_is_event(i));
        let group = idxs.iter().any(|&i| a.clauses[i].group_subject || a.clauses[i].in_class);
        let r = resolve_when(a, *sent, false);
        if !students.is_empty() {
            if all_tasks_or_events && !attendance {
                // "홍길동 보호자에게 금요일까지 연락" 같은 문장은 할 일로만 둔다
                continue;
            }
            let mut it = blank(ItemKind::Student, text::tidy(sent.slice(raw)), ItemOrigin::Rule);
            it.students = students;
            it.area = if attendance { Area::Attendance } else { hint.unwrap_or(Area::None) };
            it.date = Some(observed_on(a, *sent, r.date.as_ref()));
            items.push(it);
        } else if group && !all_tasks_or_events && hint.is_some_and(|h| h != Area::Attendance) {
            let mut it = blank(ItemKind::Student, text::tidy(sent.slice(raw)), ItemOrigin::Rule);
            it.area = hint.unwrap_or(Area::None);
            it.date = Some(observed_on(a, *sent, r.date.as_ref()));
            items.push(it);
        }
    }

    // 2) 담당업무 (문서 전체에서 찾은 업무)
    let mut duty_ids: Vec<i64> = a.clauses.iter().flat_map(|c| c.duties.iter().copied()).collect();
    duty_ids.sort_unstable();
    duty_ids.dedup();
    let single_duty = if duty_ids.len() == 1 { Some(duty_ids[0]) } else { None };

    // 3) 할 일
    for i in 0..a.clauses.len() {
        if !a.clause_is_task(i) {
            continue;
        }
        let c = &a.clauses[i];
        let r = resolve_when(a, c.span, true);
        let title = item_title(raw, c.span, r.date.as_ref().filter(|d| c.span.contains(d.start)), r.time.as_ref());
        let r = task_when(r, a.today);
        let mut it = with_when(blank(ItemKind::Task, title, ItemOrigin::Rule), &r);
        it.duty_id = c.duties.first().copied().or(single_duty);
        for &m in &c.students {
            push_student(&mut it.students, plan_student(&a.students[m]));
        }
        items.push(it);
    }

    // 4) 일정
    for i in 0..a.clauses.len() {
        if !a.clause_is_event(i) {
            continue;
        }
        let c = &a.clauses[i];
        let r = resolve_when(a, c.span, false);
        let (title, memo) = title_and_memo(a, c.span, &r);
        let mut it = with_when(blank(ItemKind::Event, title, ItemOrigin::Rule), &r);
        it.memo = memo;
        items.push(it);
    }

    // 5) 업무 기록
    for did in &duty_ids {
        let sents: Vec<String> = a
            .sentences
            .iter()
            .enumerate()
            .filter(|(si, _)| single_duty.is_some() || a.clauses.iter().any(|c| c.sentence == *si && c.duties.contains(did)))
            .map(|(_, s)| text::tidy(s.slice(raw)))
            .collect();
        let mut it = blank(ItemKind::Duty, text::truncate_chars(&sents.join(". "), 500), ItemOrigin::Rule);
        it.duty_id = Some(*did);
        // 업무 기록 날짜: 그 업무 문장에 적힌 날짜 ("10. 2. 11:00 한일국제교류 컨설팅" → 10월 2일)
        let first_date = a
            .sentences
            .iter()
            .enumerate()
            .filter(|(si, _)| single_duty.is_some() || a.clauses.iter().any(|c| c.sentence == *si && c.duties.contains(did)))
            .find(|(_, s)| a.dates.iter().any(|d| s.contains(d.start)))
            .map(|(_, s)| resolve_when(a, *s, false));
        if let Some(r) = first_date {
            it = with_when(it, &r);
        }
        items.push(it);
    }

    let _ = ctx;
    // 시간표 문장("월 3교시 2-1 국어", "내일 3교시 보강")은 시간표에 반영하므로 관찰·일정·할 일로 만들지 않는다
    let tt = crate::ttinput::find(a);
    items.retain(|i| !crate::ttinput::covered_by(&tt, raw, &i.text));
    let mut plan = Plan { summary: extractive_summary(raw, a.today), items, guard_notes: class_mismatch_notes(a), unknown_names: vec![], profile: vec![], specials: vec![] };
    attach_profile(a, &mut plan);
    plan
}

/// 개인정보 문장 → `Plan.profile`, 특이사항 문장 → `Plan.specials`.
/// 그 문장에서 나온 학생 관찰(개인정보면 할 일·일정까지)은 뺀다 — 개인정보는 학생부 자료·할 일로 보내지 않는다.
fn attach_profile(a: &Analysis, plan: &mut Plan) {
    let raw = a.text.as_str();
    let mut everyone: Vec<PlanStudent> = Vec::new();
    for c in &a.clauses {
        for &m in &c.students {
            push_student(&mut everyone, plan_student(&a.students[m]));
        }
    }
    for (si, sent) in a.sentences.iter().enumerate() {
        let text = sent.slice(raw);
        let mut studs: Vec<PlanStudent> = Vec::new();
        for c in a.clauses.iter().filter(|c| c.sentence == si) {
            for &m in &c.students {
                push_student(&mut studs, plan_student(&a.students[m]));
            }
        }
        // 문장에 이름이 없으면, 기록 전체에 학생이 한 명일 때 그 학생 ("홍길동 전학 옴. 주소 …")
        if studs.is_empty() && everyone.len() == 1 {
            studs = everyone.clone();
        }
        let span = *sent;
        let drop = |items: &mut Vec<PlanItem>, kinds: &[ItemKind]| {
            items.retain(|i| !(kinds.contains(&i.kind) && locate(raw, &i.text).is_some_and(|sp| sp.overlaps(&span))));
        };
        let hits = crate::profile::find_hits(text);
        if studs.len() != 1 {
            // 누구의 정보인지 하나로 정할 수 없으면 명렬표에 넣지 않는다. 그래도 학생부 자료로는 보내지 않는다.
            if !hits.is_empty() {
                drop(&mut plan.items, &[ItemKind::Student]);
                if studs.len() > 1 {
                    let note = "개인정보(연락처·주소 등)가 있지만 여러 학생이 함께 적혀 누구의 것인지 정할 수 없어 학생 명렬표에 넣지 않았습니다. 학생별로 나눠 적어 주세요.".to_string();
                    if !plan.guard_notes.contains(&note) {
                        plan.guard_notes.push(note);
                    }
                }
            }
            continue;
        }
        let student = studs.remove(0);
        if !hits.is_empty() {
            for h in hits {
                let it = crate::profile::ProfileItem { field: h.field.to_string(), value: h.value, relation: h.relation, student: student.clone(), text: text::tidy(text) };
                if !plan.profile.contains(&it) {
                    plan.profile.push(it);
                }
            }
            drop(&mut plan.items, &[ItemKind::Student, ItemKind::Task, ItemKind::Event]);
        } else if crate::profile::is_special(text) {
            let area = a
                .clauses
                .iter()
                .filter(|c| c.sentence == si)
                .find_map(|c| c.area_hint)
                .filter(|h| *h != Area::Attendance && *h != Area::None)
                .map(|h| h.as_str().to_string())
                .unwrap_or_default();
            plan.specials.push(crate::profile::SpecialItem { text: text::tidy(text), student, area });
            drop(&mut plan.items, &[ItemKind::Student]);
        }
    }
}

/// 원문에서 항목 문구의 위치를 찾는다 (정확히 있으면 그 위치, 아니면 가장 비슷한 절)
fn locate(raw: &str, t: &str) -> Option<Span> {
    let t = t.trim().trim_end_matches(['.', '。']);
    if t.is_empty() {
        return None;
    }
    if let Some(p) = raw.find(t) {
        return Some(Span { start: p, end: p + t.len() });
    }
    text::best_source_span(t, raw).filter(|(_, s)| *s >= 0.3).map(|(sp, _)| sp)
}

fn covers(i: &PlanItem, t: &str) -> bool {
    text::coverage(t, &i.text) >= 0.6 || text::coverage(&i.text, t) >= 0.8
}

fn covered(items: &[PlanItem], kinds: &[ItemKind], t: &str) -> bool {
    items.iter().filter(|i| kinds.contains(&i.kind)).any(|i| covers(i, t))
}

/// AI 항목이 규칙 항목과 같은 내용인데 날짜·시각이 빠졌으면 규칙이 찾은 날짜·시각으로 채운다
fn fill_when(items: &mut [PlanItem], kind: ItemKind, rule: &PlanItem) {
    if let Some(it) = items.iter_mut().find(|i| i.kind == kind && covers(i, &rule.text)) {
        if it.date.is_none() && rule.date.is_some() {
            it.date = rule.date.clone();
            it.date_expr = rule.date_expr.clone();
            it.approximate = rule.approximate;
        }
        if it.time.is_none() && it.date == rule.date {
            it.time = rule.time.clone();
        }
    }
}

/// AI 결과를 검증·보정한다.
pub fn guard_llm(raw: &str, out: &LlmOutput, ctx: &Ctx) -> Plan {
    let a = analysis::analyze(raw, ctx.today, ctx.roster, ctx.duties);
    let rules = rule_plan_from(&a, ctx);
    let mut notes: Vec<String> = class_mismatch_notes(&a);
    let mut unknown: Vec<String> = Vec::new();
    let mut items: Vec<PlanItem> = Vec::new();

    let mut queue: std::collections::VecDeque<LlmItem> = out.items.iter().take(MAX_ITEMS).cloned().collect();
    let mut splits = 0usize;
    while let Some(li) = queue.pop_front() {
        let li = &li;
        let kind = ItemKind::parse(&li.kind);
        if kind == ItemKind::Memo {
            continue;
        }
        let mut t = text::tidy(&li.text);
        if t.is_empty() {
            continue;
        }
        // 1) 원문 근거 확인
        if text::coverage(&t, raw) < 0.85 {
            match text::best_source_span(&t, raw) {
                Some((sp, score)) if score >= 0.3 => {
                    notes.push(format!("원문에 없는 표현(\"{}\")을 원문 문장으로 바꾸었습니다.", text::truncate_chars(&t, 30)));
                    t = text::tidy(sp.slice(raw));
                }
                _ => {
                    notes.push(format!("원문에 근거가 없는 항목(\"{}\")을 제외했습니다.", text::truncate_chars(&t, 30)));
                    continue;
                }
            }
        }
        let Some(span) = locate(raw, &t) else { continue };
        // 여러 문장을 한 항목으로 묶은 경우 문장별로 나누어 따로 검증한다
        // ("10월 20일 체육대회. 반별 응원 준비 필요." → 일정 + 할 일)
        let sents: Vec<Span> = a.sentences.iter().filter(|s| s.overlaps(&span)).copied().collect();
        if sents.len() > 1 && splits < 24 {
            for s in sents {
                let (st, en) = (s.start.max(span.start), s.end.min(span.end));
                if st >= en {
                    continue;
                }
                let piece = text::tidy(&raw[st..en]);
                if piece.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
                    continue;
                }
                let mut p = li.clone();
                p.text = piece;
                p.date = String::new();
                queue.push_back(p);
                splits += 1;
            }
            continue;
        }
        let seg = span.slice(raw);
        let sentence = a.sentences.iter().find(|s| s.contains(span.start)).map(|s| s.slice(raw)).unwrap_or(seg);

        // 2) 학생: 원문에 실제로 있는 이름만
        let mut students: Vec<PlanStudent> = Vec::new();
        for name in &li.students {
            let n = name.trim();
            if n.is_empty() {
                continue;
            }
            let bare = n.split_whitespace().last().unwrap_or(n);
            let bare = analysis::clean_name(bare).unwrap_or_else(|| bare.to_string());
            if !raw.contains(bare.as_str()) {
                notes.push(format!("원문에 없는 이름(\"{n}\")을 제외했습니다."));
                continue;
            }
            if let Some(m) = a.students.iter().find(|m| m.name == bare || m.name.ends_with(bare.as_str())) {
                push_student(&mut students, plan_student(m));
            } else if analysis::plausible_full_name(&bare) {
                push_student(
                    &mut students,
                    PlanStudent { student_id: None, name: bare.clone(), class_label: None, number: None, candidates: vec![] },
                );
            } else if !unknown.contains(&bare) {
                unknown.push(bare.clone());
            }
        }
        for m in a.students.iter().filter(|m| span.contains(m.span.start)) {
            push_student(&mut students, plan_student(m));
        }

        // 3) 날짜: 계산은 항상 규칙으로 한다. AI가 적은 날짜 표현은 원문에 있을 때만 참고한다.
        let mut r = resolve_when(&a, span, kind == ItemKind::Task);
        if r.date.is_none() {
            let de = li.date.trim();
            if !de.is_empty() && raw.contains(de) {
                if let Some(h) = a.dates.iter().find(|h| h.expr.contains(de) || de.contains(h.expr.as_str())) {
                    r.date = Some(h.clone());
                }
            }
        }

        let attendance_fact = analysis::has_attendance_fact(seg);
        let in_class = analysis::is_in_class(seg) || a.clauses.iter().any(|c| c.span.overlaps(&span) && c.in_class);
        let past = analysis::is_past(seg);
        let action = analysis::has_action(seg);
        let deadline = r.date.as_ref().is_some_and(|d| d.deadline);

        // 4) 종류 검증
        let mut kind = kind;
        if kind == ItemKind::Task && (past || !(action || deadline) || (attendance_fact && !action)) {
            let to = if !students.is_empty() {
                ItemKind::Student
            } else if r.date.as_ref().is_some_and(|d| d.date >= ctx.today) && !past {
                ItemKind::Event
            } else {
                ItemKind::Memo
            };
            notes.push(format!(
                "\"{}\"에는 해야 할 행동이나 기한이 없어 할 일로 만들지 않았습니다.",
                text::truncate_chars(&t, 30)
            ));
            kind = to;
        }
        // 학생 이름과 함께 적힌 "진로 상담", "보호자 통화"는 기한·의무 표현이나 앞날짜가 없으면 한 일의 기록으로 본다
        let obligation = analysis::has_obligation(seg);
        let future = r.date.as_ref().is_some_and(|d| d.date > ctx.today);
        if kind == ItemKind::Task && !students.is_empty() && !deadline && !obligation && !future {
            notes.push(format!(
                "\"{}\"에 기한이나 해야 한다는 표현이 없어 학생 관찰 기록으로 보았습니다.",
                text::truncate_chars(&t, 30)
            ));
            kind = ItemKind::Student;
        }
        if kind == ItemKind::Event && r.date.is_none() {
            notes.push(format!("\"{}\"에 날짜가 없어 일정으로 만들지 않았습니다.", text::truncate_chars(&t, 30)));
            kind = if !students.is_empty() {
                ItemKind::Student
            } else if action && !past {
                ItemKind::Task
            } else {
                ItemKind::Memo
            };
        }
        // "~까지"는 일정이 아니라 기한이다
        if kind == ItemKind::Event && deadline {
            kind = ItemKind::Task;
        }
        // "수업중 잠 → 지도함"처럼 이미 있었던 일(오늘·지난 날짜)은 일정이 아니라 관찰·메모다
        if kind == ItemKind::Event && (past || analysis::is_past(sentence)) && r.date.as_ref().is_some_and(|d| d.date <= ctx.today) {
            notes.push(format!("\"{}\"은(는) 이미 있었던 일이라 일정으로 만들지 않았습니다.", text::truncate_chars(&t, 30)));
            kind = if students.is_empty() { ItemKind::Memo } else { ItemKind::Student };
        }
        // 결석·지각 같은 출결 사실은 달력 일정이 아니라 학생 기록이다
        if kind == ItemKind::Event && attendance_fact {
            kind = if students.is_empty() { ItemKind::Memo } else { ItemKind::Student };
        }
        let mut duty_id = None;
        if kind == ItemKind::Duty || li.duty > 0 {
            let idx = li.duty;
            duty_id = if idx >= 1 && (idx as usize) <= ctx.duties.len() {
                Some(ctx.duties[idx as usize - 1].id)
            } else {
                analysis::match_duties(seg, ctx.duties).first().copied()
            };
            // "OO중 교무부장 연락옴": 업무 이름이 다른 학교 선생님의 직함으로만 나오면 내 업무 기록이 아니다
            if let Some(d) = duty_id.and_then(|id| ctx.duties.iter().find(|d| d.id == id)) {
                let terms = d.match_terms();
                if terms.iter().any(|w| sentence.contains(w.as_str())) && !terms.iter().any(|w| analysis::mentions_as_own(sentence, w)) {
                    notes.push(format!("\"{}\"의 '{}'은(는) 다른 학교 선생님의 직함이라 내 업무로 보지 않았습니다.", text::truncate_chars(&t, 30), d.name));
                    duty_id = None;
                }
            }
            if kind == ItemKind::Duty && duty_id.is_none() {
                notes.push(format!("\"{}\"와 맞는 담당업무가 없어 업무 기록으로 만들지 않았습니다.", text::truncate_chars(&t, 30)));
                kind = ItemKind::Memo;
            }
        }
        if kind == ItemKind::Student && students.is_empty() {
            let group = a.clauses.iter().any(|c| c.span.overlaps(&span) && (c.group_subject || c.in_class));
            if !group {
                kind = ItemKind::Memo;
            }
        }
        if kind == ItemKind::Memo {
            continue;
        }

        // 5) 영역 검증: 출결은 실제 출결 사실이 있을 때만
        let mut area = Area::parse(&li.area);
        if kind == ItemKind::Student {
            if area == Area::Attendance && !attendance_fact {
                let fixed = analysis::area_hint(seg)
                    .filter(|x| *x != Area::Attendance)
                    .or(if in_class { Some(Area::Subject) } else { None })
                    .unwrap_or(Area::None);
                notes.push(format!(
                    "출결(결석·지각·조퇴·결과) 사실이 없어 출결특기사항 대신 '{}'(으)로 바꾸었습니다.",
                    fixed.label()
                ));
                area = fixed;
            }
            // 봉사·동아리·자유학기·일상생활은 학교에서 정해진 활동이라 원문에 그 낱말이 있을 때만 인정한다
            let needs = match area {
                Area::Volunteer => Some("봉사"),
                Area::Club => Some("동아리"),
                Area::FreeSemester => Some("자유학기"),
                Area::Daily => Some("일상생활"),
                _ => None,
            };
            if let Some(word) = needs {
                if !analysis::area_evidence(area, sentence) && !sentence.contains(word) {
                    let fixed = analysis::area_hint(sentence).filter(|x| *x != Area::Attendance || attendance_fact).unwrap_or(Area::None);
                    notes.push(format!("원문에 '{word}' 활동이 없어 '{}' 대신 '{}'(으)로 바꾸었습니다.", area.label(), fixed.label()));
                    area = fixed;
                }
            }
            // AI가 고른 영역에 원문 근거가 없고, 원문에 다른 영역의 뚜렷한 낱말이 있으면 그쪽을 따른다
            // ("동아리 시간에 … 발표" → 동아리, "학급회의에서 의견" → 자율, "영어 발표" → 교과)
            if let Some(hint) = analysis::area_hint(sentence) {
                if hint != area && !analysis::area_evidence(area, sentence) && analysis::area_evidence(hint, sentence) {
                    if area != Area::None {
                        notes.push(format!("원문 낱말에 맞춰 '{}'을(를) '{}'(으)로 바꾸었습니다.", area.label(), hint.label()));
                    }
                    area = hint;
                }
            }
            if area == Area::None {
                area = analysis::area_hint(seg).unwrap_or(Area::None);
            }
        } else {
            area = Area::None;
        }

        let mut it = blank(kind, t.clone(), ItemOrigin::Ai);
        it.area = area;
        it.duty_id = duty_id;
        it.students = students;
        match kind {
            ItemKind::Student => {
                it.date = Some(observed_on(&a, span, r.date.as_ref()));
            }
            ItemKind::Event => {
                let (title, memo) = title_and_memo(&a, span, &r);
                it.text = title;
                it.memo = memo;
                it = with_when(it, &r);
            }
            ItemKind::Task => {
                let date_in_span = r.date.as_ref().filter(|d| span.contains(d.start));
                it.text = item_title(raw, span, date_in_span, r.time.as_ref());
                let r = task_when(r, ctx.today);
                it = with_when(it, &r);
            }
            ItemKind::Duty => {
                it.text = text::tidy(seg);
                // 업무 기록 날짜: 업무 문장에 적힌 날짜 (기록한 날이 아니라 그 일이 있는 날)
                if r.date.is_some() {
                    it = with_when(it, &r);
                }
            }
            ItemKind::Memo => {}
        }
        items.push(it);
    }

    // 6) 규칙으로 보완: AI가 놓친 출결 사실, 기한 있는 할 일, 일정, 업무
    for ri in &rules.items {
        match ri.kind {
            ItemKind::Student if ri.area == Area::Attendance => {
                let missing: Vec<PlanStudent> = ri
                    .students
                    .iter()
                    .filter(|s| {
                        !items
                            .iter()
                            .any(|i| i.kind == ItemKind::Student && i.area == Area::Attendance && i.students.iter().any(|x| same_student(x, s)))
                    })
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    let mut it = ri.clone();
                    it.students = missing;
                    notes.push("원문의 출결 사실을 출결특기사항 자료로 연결했습니다.".to_string());
                    items.push(it);
                }
            }
            ItemKind::Student => {
                let missing: Vec<PlanStudent> = ri
                    .students
                    .iter()
                    .filter(|s| !items.iter().any(|i| i.kind == ItemKind::Student && i.students.iter().any(|x| same_student(x, s))))
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    let mut it = ri.clone();
                    it.students = missing;
                    items.push(it);
                } else if ri.students.is_empty() && !items.iter().any(|i| i.kind == ItemKind::Student) {
                    items.push(ri.clone());
                }
            }
            ItemKind::Task => {
                if !covered(&items, &[ItemKind::Task, ItemKind::Event], &ri.text) {
                    notes.push(format!("해야 할 일(\"{}\")을 할 일로 추가했습니다.", text::truncate_chars(&ri.text, 30)));
                    items.push(ri.clone());
                } else {
                    fill_when(&mut items, ItemKind::Task, ri);
                }
            }
            ItemKind::Event => {
                if !covered(&items, &[ItemKind::Event], &ri.text) {
                    items.push(ri.clone());
                } else {
                    fill_when(&mut items, ItemKind::Event, ri);
                }
            }
            ItemKind::Duty => {
                if !items.iter().any(|i| i.kind == ItemKind::Duty) {
                    items.push(ri.clone());
                }
            }
            ItemKind::Memo => {}
        }
    }

    // 7) 같은 내용 합치기: 종류가 같고 한쪽 문구가 다른 쪽에 거의 다 들어 있으면 하나로 (더 긴 문구를 남긴다)
    let mut merged: Vec<PlanItem> = Vec::new();
    for it in items {
        if let Some(prev) = merged.iter_mut().find(|p| {
            let contained = text::coverage(&p.text, &it.text) >= 0.9 || text::coverage(&it.text, &p.text) >= 0.9;
            let same_when = p.date.is_none() || it.date.is_none() || p.date == it.date;
            p.kind == it.kind
                && p.area == it.area
                && contained
                && same_when
                && (p.kind != ItemKind::Duty || p.duty_id == it.duty_id)
        }) {
            if it.text.chars().count() > prev.text.chars().count() {
                prev.text = it.text.clone();
            }
            for s in it.students {
                push_student(&mut prev.students, s);
            }
            if prev.date.is_none() {
                prev.date = it.date;
                prev.date_expr = it.date_expr;
                prev.time = it.time;
                prev.approximate = it.approximate;
            }
            if prev.duty_id.is_none() {
                prev.duty_id = it.duty_id;
            }
            continue;
        }
        merged.push(it);
    }

    // 7-1) "OO고 홍길동 선생님 방문"처럼 다른 일정의 메모에 들어간 설명 절을 따로 일정으로 만들었으면 뺀다
    let memos: Vec<(usize, Option<String>, String)> = merged
        .iter()
        .enumerate()
        .filter(|(_, i)| i.kind == ItemKind::Event)
        .filter_map(|(n, i)| i.memo.clone().map(|m| (n, i.date.clone(), m)))
        .collect();
    let drop_memo: Vec<usize> = merged
        .iter()
        .enumerate()
        .filter(|(n, i)| {
            i.kind == ItemKind::Event && memos.iter().any(|(k, d, m)| k != n && *d == i.date && text::coverage(&i.text, m) >= 0.9)
        })
        .map(|(n, _)| n)
        .collect();
    let mut merged: Vec<PlanItem> = merged.into_iter().enumerate().filter(|(n, _)| !drop_memo.contains(n)).map(|(_, i)| i).collect();

    // 7-2) 영역 없는 학생 관찰이 할 일·일정과 같은 내용이면 관찰로 남기지 않는다 (학생은 할 일에 연결)
    let actions: Vec<(usize, String)> = merged
        .iter()
        .enumerate()
        .filter(|(_, i)| matches!(i.kind, ItemKind::Task | ItemKind::Event))
        .map(|(n, i)| (n, i.text.clone()))
        .collect();
    let mut drop_idx: Vec<usize> = Vec::new();
    let mut moves: Vec<(usize, Vec<PlanStudent>)> = Vec::new();
    for (n, it) in merged.iter().enumerate() {
        // 영역 없는 관찰, 또는 학생 없는 관찰("전교생 대피 훈련")이 일정·할 일과 같은 내용이면 정리한다
        if it.kind != ItemKind::Student || (it.area != Area::None && !it.students.is_empty()) {
            continue;
        }
        if let Some((target, _)) = actions
            .iter()
            .find(|(_, t)| text::coverage(t, &it.text) >= 0.6 || text::coverage(&it.text, t) >= 0.6)
        {
            drop_idx.push(n);
            moves.push((*target, it.students.clone()));
        }
    }
    for (target, studs) in moves {
        if merged[target].kind == ItemKind::Task {
            for s in studs {
                push_student(&mut merged[target].students, s);
            }
        }
    }
    let merged: Vec<PlanItem> = merged.into_iter().enumerate().filter(|(n, _)| !drop_idx.contains(n)).map(|(_, i)| i).collect();

    // 8) 할 일에 업무 연결: 기록이 한 업무에 관한 것이면 할 일도 그 업무에 연결
    let mut merged = merged;
    let duty_ids: Vec<i64> = merged.iter().filter(|i| i.kind == ItemKind::Duty).filter_map(|i| i.duty_id).collect();
    if duty_ids.len() == 1 {
        for it in merged.iter_mut().filter(|i| i.kind == ItemKind::Task && i.duty_id.is_none()) {
            it.duty_id = Some(duty_ids[0]);
        }
    }

    // 8-2) 시간표 문장은 시간표에 반영하므로 AI 가 관찰·일정·할 일로 본 것을 뺀다
    let tt = crate::ttinput::find(&a);
    let before = merged.len();
    merged.retain(|i| !crate::ttinput::covered_by(&tt, raw, &i.text));
    if merged.len() < before {
        notes.push("시간표 문장은 학생 관찰·일정 대신 시간표에 반영했습니다.".to_string());
    }

    // 9) 요약: 원문과 많이 다르면 원문 첫 문장
    let mut summary = text::tidy(&out.summary);
    if summary.is_empty() || text::coverage(&summary, raw) < 0.6 {
        if !summary.is_empty() {
            notes.push("AI 요약이 원문과 달라 원문 첫 문장을 요약으로 썼습니다.".to_string());
        }
        summary = extractive_summary(raw, ctx.today);
    }
    summary = text::truncate_chars(&strip_leading_when(&summary, ctx.today), SUMMARY_MAX);

    notes.dedup();
    let mut plan = Plan { summary, items: merged.into_iter().take(MAX_ITEMS).collect(), guard_notes: notes, unknown_names: unknown, profile: vec![], specials: vec![] };
    attach_profile(&a, &mut plan);
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()
    }
    fn ctx<'a>(roster: &'a [RosterEntry], duties: &'a [DutyEntry]) -> Ctx<'a> {
        Ctx { today: today(), roster, duties }
    }
    fn llm(json: &str) -> LlmOutput {
        parse_llm_json(json).expect("json")
    }
    fn kinds(p: &Plan, k: ItemKind) -> Vec<&PlanItem> {
        p.items.iter().filter(|i| i.kind == k).collect()
    }

    const A: &str = "오늘 3교시 2-1 홍길동 지각. 담임에게 알리고 금요일까지 보호자 연락.";

    /// 수정목록 1번: 숫자로 적은 날짜 + 시각 + 설명 절
    fn exchange_duty() -> Vec<DutyEntry> {
        vec![DutyEntry { id: 7, name: "교무부장".into(), detail: "국제교류, 학사 운영".into(), guide: String::new(), keywords: vec![] }]
    }

    #[test]
    fn numeric_dates_title_time_memo() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let duties = exchange_duty();
        let c = Ctx { today, roster: &[], duties: &duties };
        for d in ["10.2.", "10. 2.", "10월 2일", "10-2", "10/2"] {
            for t in ["11:00", "오전 11시"] {
                let raw = format!("{d} {t} 한일국제교류 컨설팅, OO고 홍길동 선생님 방문, 본관 2층 학생회의실");
                let p = rule_plan(&raw, &c);
                let ev = kinds(&p, ItemKind::Event);
                assert_eq!(ev.len(), 1, "{raw}: {:?}", p.items);
                assert_eq!(ev[0].text, "한일국제교류 컨설팅", "{raw}");
                assert_eq!(ev[0].date.as_deref(), Some("2026-10-02"), "{raw}");
                assert_eq!(ev[0].time.as_deref(), Some("11:00"), "{raw}");
                assert_eq!(ev[0].memo.as_deref(), Some("OO고 홍길동 선생님 방문, 본관 2층 학생회의실"), "{raw}");
                assert_eq!(p.summary, "한일국제교류 컨설팅, OO고 홍길동 선생님 방문, 본관 2층 학생회의실", "{raw}");
                let duty = kinds(&p, ItemKind::Duty);
                assert_eq!(duty.len(), 1, "{raw}");
                assert_eq!(duty[0].date.as_deref(), Some("2026-10-02"), "업무기록 날짜 {raw}");
            }
        }
    }

    #[test]
    fn numeric_date_with_ai_output() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let duties = exchange_duty();
        let c = Ctx { today, roster: &[], duties: &duties };
        let raw = "10. 2. 11:00 한일국제교류 컨설팅, OO고 홍길동 선생님 방문, 본관 2층 학생회의실";
        // AI가 문장 전체를 일정으로, 설명 절을 따로 일정으로 낸 경우
        let out = llm(
            r#"{"summary":"10. 2. 한일국제교류 컨설팅","items":[
            {"kind":"event","text":"10. 2. 11:00 한일국제교류 컨설팅, OO고 홍길동 선생님 방문, 본관 2층 학생회의실","students":[],"area":"none","duty":1,"date":"10. 2."},
            {"kind":"event","text":"OO고 홍길동 선생님 방문","students":[],"area":"none","duty":0,"date":""},
            {"kind":"duty","text":"한일국제교류 컨설팅","students":[],"area":"none","duty":1,"date":""}]}"#,
        );
        let p = guard_llm(raw, &out, &c);
        let ev = kinds(&p, ItemKind::Event);
        assert_eq!(ev.len(), 1, "{:?}", p.items);
        assert_eq!((ev[0].text.as_str(), ev[0].date.as_deref(), ev[0].time.as_deref()), ("한일국제교류 컨설팅", Some("2026-10-02"), Some("11:00")));
        assert_eq!(ev[0].memo.as_deref(), Some("OO고 홍길동 선생님 방문, 본관 2층 학생회의실"));
        assert_eq!(p.summary, "한일국제교류 컨설팅");
        let duty = kinds(&p, ItemKind::Duty);
        assert_eq!(duty.len(), 1);
        assert_eq!((duty[0].duty_id, duty[0].date.as_deref()), (Some(7), Some("2026-10-02")));
    }

    fn one_class_school() -> Vec<RosterEntry> {
        let s = |id, name: &str, l: &str, n| RosterEntry { id, name: name.into(), class_label: Some(l.into()), number: Some(n) };
        let c = |l: &str| RosterEntry { id: 0, name: String::new(), class_label: Some(l.into()), number: None };
        vec![s(1, "심청", "1-1", 1), s(2, "전우치", "1-1", 2), c("1-1"), c("2-1"), c("3-1")]
    }

    #[test]
    fn done_actions_are_not_tasks() {
        let c = ctx(&[], &[]);
        // 이미 한 일: 할 일이 아니다 (통화한 시각을 마감으로 쓰지도 않는다)
        let p = rule_plan("10.6. 오전 9:40 OO군청에 참석 인원 물어봄", &c);
        assert!(kinds(&p, ItemKind::Task).is_empty(), "{:?}", p.items);
        let p = rule_plan("10. 6. 오전 9:40 경 OO군청 문화체육관광과에 연락 방법과 참석 인원 물어봄 참석인원은 교장1 교원3 학생 14, 총 22명 예정", &c);
        assert!(kinds(&p, ItemKind::Task).is_empty(), "{:?}", p.items);
        // 앞으로 할 일: 마감 표현이 없으면 마감 없이
        let p = rule_plan("OO군청에 참석 인원 물어볼 것", &c);
        let t = kinds(&p, ItemKind::Task);
        assert_eq!(t.len(), 1, "{:?}", p.items);
        assert_eq!(t[0].date, None);
        // "~까지"는 마감
        let p = rule_plan("금요일까지 OO군청에 참석 인원 회신", &c);
        let t = kinds(&p, ItemKind::Task);
        assert_eq!((t.len(), t[0].date.as_deref()), (1, Some("2026-10-02")), "{:?}", p.items);
        // 섞여 있으면 앞으로 할 부분만
        let p = rule_plan("OO군청에 물어봄. 답 오면 금요일까지 교장 보고", &c);
        let t = kinds(&p, ItemKind::Task);
        assert_eq!(t.len(), 1, "{:?}", p.items);
        assert_eq!((t[0].text.as_str(), t[0].date.as_deref()), ("교장 보고", Some("2026-10-02")));
    }

    #[test]
    fn ai_task_for_done_action_is_dropped_and_past_time_is_not_due() {
        let c = ctx(&[], &[]);
        let raw = "9월 28일 오전 9:40 OO군청에 연락 방법과 참석 인원 물어봄 참석인원은 총 22명 예정";
        let out = llm(
            r#"{"summary":"군청 문의","items":[{"kind":"task","text":"OO군청에 연락 방법과 참석 인원 물어봄 참석인원은 총 22명 예정","students":[],"area":"none","duty":0,"date":"9월 28일"}]}"#,
        );
        let p = guard_llm(raw, &out, &c);
        assert!(kinds(&p, ItemKind::Task).is_empty(), "AI 가 할 일로 내도 이미 한 일이면 빼야 한다: {:?}", p.items);
        // 할 일이지만 적힌 날짜가 지난 시각(마감 표현 없음)이면 마감으로 쓰지 않는다
        let raw2 = "9월 28일 오전 9:40 보호자 상담 다시 연락하기";
        let out2 = llm(r#"{"summary":"보호자 연락","items":[{"kind":"task","text":"보호자 상담 다시 연락하기","students":[],"area":"none","duty":0,"date":"9월 28일"}]}"#);
        let p2 = guard_llm(raw2, &out2, &c);
        let t = kinds(&p2, ItemKind::Task);
        assert_eq!(t.len(), 1, "{:?}", p2.items);
        assert_eq!((t[0].date.as_deref(), t[0].time.as_deref()), (None, None));
    }

    #[test]
    fn personal_info_with_two_students_goes_nowhere() {
        let r = one_class_school();
        let c = Ctx { today: today(), roster: &r, duties: &[] };
        let raw = "1-1 심청, 전우치 보호자 연락처 010-1234-5678";
        // AI 가 번호가 든 문장을 학생 관찰로 낸 경우에도 학생부 자료로 보내지 않는다
        let out = llm(
            r#"{"summary":"보호자 연락처","items":[
            {"kind":"student","text":"1-1 심청, 전우치 보호자 연락처 010-1234-5678","students":["1-1 심청","1-1 전우치"],"area":"behavior","duty":0,"date":""}]}"#,
        );
        for p in [rule_plan(raw, &c), guard_llm(raw, &out, &c)] {
            assert!(p.profile.is_empty(), "누구의 번호인지 모르면 명렬표에 넣지 않음: {:?}", p.profile);
            assert!(!p.items.iter().any(|i| i.text.contains("010-")), "학생부 자료로 보내지 않음: {:?}", p.items);
            assert!(p.guard_notes.iter().any(|n| n.contains("여러 학생")), "{:?}", p.guard_notes);
        }
    }

    #[test]
    fn grade_list_observation_not_event() {
        let r = one_class_school();
        let c = Ctx { today: today(), roster: &r, duties: &[] };
        let raw = "오늘 1학년 심청, 전우치 수업중 잠 → 지도함";
        let p = rule_plan(raw, &c);
        assert!(kinds(&p, ItemKind::Event).is_empty(), "{:?}", p.items);
        let obs = kinds(&p, ItemKind::Student);
        assert_eq!(obs.len(), 1, "{:?}", p.items);
        assert_eq!(obs[0].students.iter().map(|s| (s.student_id, s.class_label.as_deref())).collect::<Vec<_>>(), vec![(Some(1), Some("1-1")), (Some(2), Some("1-1"))]);
        // AI가 일정으로 낸 경우에도 일정으로 만들지 않는다
        let out = llm(r#"{"summary":"수업중 잠","items":[{"kind":"event","text":"오늘 1학년 심청, 전우치 수업중 잠 → 지도함","students":["심청","전우치"],"area":"subject","duty":0,"date":"오늘"}]}"#);
        let p = guard_llm(raw, &out, &c);
        assert!(kinds(&p, ItemKind::Event).is_empty(), "{:?}", p.items);
        assert_eq!(kinds(&p, ItemKind::Student).len(), 1, "{:?}", p.items);
    }

    #[test]
    fn written_class_mismatch_is_noted() {
        let r = one_class_school();
        let c = Ctx { today: today(), roster: &r, duties: &[] };
        let p = rule_plan("2-1 심청 지각", &c);
        let obs = kinds(&p, ItemKind::Student);
        assert_eq!(obs[0].students[0].student_id, Some(1));
        assert!(p.guard_notes.iter().any(|n| n.contains("2-1") && n.contains("1-1")), "{:?}", p.guard_notes);
    }
    #[test]
    fn other_school_title_note_is_general() {
        let duties = exchange_duty();
        let c = Ctx { today: today(), roster: &[], duties: &duties };
        let raw = "OO중 교무부장 연락옴";
        assert!(rule_plan(raw, &c).items.is_empty());
        let out = llm(r#"{"summary":"OO중 교무부장 연락","items":[{"kind":"duty","text":"OO중 교무부장 연락옴","students":[],"area":"none","duty":1,"date":""}]}"#);
        let p = guard_llm(raw, &out, &c);
        assert!(p.items.is_empty(), "{:?}", p.items);
    }
    #[test]
    fn when_only_clause_takes_next_clause_as_title() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let c = Ctx { today, roster: &[], duties: &[] };
        let p = rule_plan("10월 5일 15시, 학년 협의회, 3층 회의실", &c);
        let ev = kinds(&p, ItemKind::Event);
        assert_eq!(ev.len(), 1, "{:?}", p.items);
        assert_eq!((ev[0].text.as_str(), ev[0].time.as_deref(), ev[0].memo.as_deref()), ("학년 협의회", Some("15:00"), Some("3층 회의실")));
    }

    #[test]
    fn scenario_a_rules_only() {
        let p = rule_plan(A, &ctx(&[], &[]));
        let obs = kinds(&p, ItemKind::Student);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].area, Area::Attendance);
        assert_eq!(obs[0].students[0].name, "홍길동");
        assert_eq!(obs[0].students[0].class_label.as_deref(), Some("2-1"));
        let tasks = kinds(&p, ItemKind::Task);
        let contact = tasks.iter().find(|t| t.text == "보호자 연락").expect("보호자 연락");
        assert_eq!(contact.date.as_deref(), Some("2026-10-02"));
        assert_eq!(contact.date_expr.as_deref(), Some("금요일까지"));
        assert!(tasks.iter().any(|t| t.text == "담임에게 알리기"));
    }

    #[test]
    fn scenario_a_llm_misclassification_is_repaired() {
        // AI가 지각을 행동특성으로, 보호자 연락을 놓친 경우
        let out = llm(r#"{"summary":"홍길동 지각","items":[
            {"kind":"student","text":"2-1 홍길동 지각","students":["홍길동"],"area":"behavior","duty":0,"date":"오늘"}
        ]}"#);
        let p = guard_llm(A, &out, &ctx(&[], &[]));
        assert!(kinds(&p, ItemKind::Student).iter().any(|i| i.area == Area::Attendance && i.students.iter().any(|s| s.name == "홍길동")));
        assert!(kinds(&p, ItemKind::Task).iter().any(|t| t.text == "보호자 연락" && t.date.as_deref() == Some("2026-10-02")));
    }

    #[test]
    fn scenario_b_not_attendance() {
        let raw = "학생들이 수업 중 손장난을 했고 지적을 받았습니다.";
        let out = llm(r#"{"summary":"수업 중 손장난 지적","items":[
            {"kind":"student","text":"학생들이 수업 중 손장난을 했고 지적을 받았습니다","students":[],"area":"attendance","duty":0,"date":""},
            {"kind":"task","text":"지적을 받았습니다","students":[],"area":"none","duty":0,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        assert!(p.items.iter().all(|i| i.area != Area::Attendance), "{:?}", p.items);
        assert!(kinds(&p, ItemKind::Task).is_empty());
        let obs = kinds(&p, ItemKind::Student);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].area, Area::Subject);
        assert!(p.guard_notes.iter().any(|n| n.contains("출결")));
        // 규칙만으로도 출결이 아니다
        let rp = rule_plan(raw, &ctx(&[], &[]));
        assert!(rp.items.iter().all(|i| i.area != Area::Attendance));
    }

    #[test]
    fn scenario_c_no_invention() {
        let raw = "10월 15일 교육장 공감소통 방문";
        let out = llm(r#"{"summary":"교육장에게 교육 방안을 제안한다","items":[
            {"kind":"event","text":"교육장에게 교육 방안을 제안한다","students":[],"area":"none","duty":0,"date":"10월 15일"}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        let ev = kinds(&p, ItemKind::Event);
        assert_eq!(ev.len(), 1, "{:?}", p.items);
        assert_eq!(ev[0].text, "교육장 공감소통 방문");
        assert_eq!(ev[0].date.as_deref(), Some("2026-10-15"));
        assert!(!p.summary.contains("제안"));
        assert!(kinds(&p, ItemKind::Task).is_empty());
    }

    #[test]
    fn scenario_d_duty_without_duplicate_task() {
        let duties = vec![DutyEntry {
            id: 3,
            name: "교무기획".into(),
            detail: "학사일정 관리".into(),
            guide: "학사일정, 학사력, 수업일수".into(),
            keywords: vec![],
        }];
        let raw = "2학기 학사일정 변경안이 교무회의에서 확정됨.";
        let out = llm(r#"{"summary":"학사일정 변경안 확정","items":[
            {"kind":"duty","text":"2학기 학사일정 변경안이 교무회의에서 확정됨","students":[],"area":"none","duty":1,"date":""},
            {"kind":"task","text":"학사일정 변경안 확정","students":[],"area":"none","duty":1,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &duties));
        assert_eq!(kinds(&p, ItemKind::Duty).len(), 1);
        assert_eq!(kinds(&p, ItemKind::Duty)[0].duty_id, Some(3));
        assert!(kinds(&p, ItemKind::Task).is_empty(), "{:?}", p.items);

        let raw2 = "2학기 학사일정 변경안이 확정됨. 금요일까지 교육청에 제출.";
        let out2 = llm(r#"{"summary":"학사일정 변경안 확정","items":[
            {"kind":"duty","text":"2학기 학사일정 변경안이 확정됨","students":[],"area":"none","duty":1,"date":""}
        ]}"#);
        let p2 = guard_llm(raw2, &out2, &ctx(&[], &duties));
        let tasks = kinds(&p2, ItemKind::Task);
        assert_eq!(tasks.len(), 1, "{:?}", p2.items);
        assert_eq!(tasks[0].text, "교육청에 제출");
        assert_eq!(tasks[0].date.as_deref(), Some("2026-10-02"));
        assert_eq!(tasks[0].duty_id, Some(3));
    }

    #[test]
    fn hallucinated_names_are_dropped() {
        let raw = "2-1 홍길동 발표를 적극적으로 함";
        let out = llm(r#"{"summary":"홍길동 발표","items":[
            {"kind":"student","text":"2-1 홍길동 발표를 적극적으로 함","students":["홍길동","이몽룡"],"area":"subject","duty":0,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        let obs = kinds(&p, ItemKind::Student);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].students.len(), 1);
        assert_eq!(obs[0].students[0].name, "홍길동");
        assert!(p.guard_notes.iter().any(|n| n.contains("이몽룡")));
    }

    #[test]
    fn invented_items_are_removed() {
        let raw = "내일 학부모 총회";
        let out = llm(r#"{"summary":"학부모 총회","items":[
            {"kind":"task","text":"교장선생님께 결재 올리기","students":[],"area":"none","duty":0,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        assert!(kinds(&p, ItemKind::Task).is_empty(), "{:?}", p.items);
        let ev = kinds(&p, ItemKind::Event);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].text, "학부모 총회");
        assert_eq!(ev[0].date.as_deref(), Some("2026-09-30"));
    }

    #[test]
    fn career_area_is_kept() {
        let raw = "2-1 성춘향 진로 상담에서 간호사가 꿈이라고 말함";
        let out = llm(r#"{"summary":"성춘향 진로 상담","items":[
            {"kind":"student","text":"2-1 성춘향 진로 상담에서 간호사가 꿈이라고 말함","students":["성춘향"],"area":"career","duty":0,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        let obs = kinds(&p, ItemKind::Student);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].area, Area::Career);
        assert!(kinds(&p, ItemKind::Task).is_empty());
    }

    // 아래 세 시험은 0.5B 모델이 실제로 낸 결과 형태를 재현한 회귀 시험이다 (E2E에서 발견)
    #[test]
    fn deadline_event_becomes_task() {
        let raw = "어제 1-2 임꺽정 결석. 다음 주 월요일까지 결석계 받기.";
        let out = llm(r#"{"summary":"임꺽정 결석","items":[
            {"kind":"student","text":"어제 1-2 임꺽정 결석","students":["임꺽정"],"area":"attendance","duty":0,"date":"어제"},
            {"kind":"event","text":"다음 주 월요일까지 결석계 받기","students":[],"area":"none","duty":0,"date":"다음 주 월요일까지"}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        assert!(kinds(&p, ItemKind::Event).is_empty(), "{:?}", p.items);
        let t = kinds(&p, ItemKind::Task);
        assert_eq!(t.len(), 1, "{:?}", p.items);
        assert_eq!(t[0].date.as_deref(), Some("2026-10-05"));
        assert_eq!(t[0].text, "결석계 받기");
        let obs = kinds(&p, ItemKind::Student);
        assert_eq!(obs[0].date.as_deref(), Some("2026-09-28"));
        // 규칙만으로도 할 일
        let rp = rule_plan(raw, &ctx(&[], &[]));
        assert!(kinds(&rp, ItemKind::Task).iter().any(|t| t.date.as_deref() == Some("2026-10-05")));
    }

    #[test]
    fn unfiled_student_items_fold_into_tasks() {
        let out = llm(r#"{"summary":"홍길동 지각","items":[
            {"kind":"student","text":"오늘 3교시 2-1 홍길동 지각","students":["홍길동"],"area":"attendance","duty":0,"date":"오늘"},
            {"kind":"student","text":"담임에게 알리고","students":["홍길동"],"area":"none","duty":0,"date":""},
            {"kind":"student","text":"금요일까지 보호자 연락","students":["홍길동"],"area":"none","duty":0,"date":"금요일까지"}
        ]}"#);
        let p = guard_llm(A, &out, &ctx(&[], &[]));
        let obs = kinds(&p, ItemKind::Student);
        assert_eq!(obs.len(), 1, "{:?}", p.items);
        assert_eq!(obs[0].area, Area::Attendance);
        let contact = kinds(&p, ItemKind::Task).into_iter().find(|t| t.text == "보호자 연락").expect("보호자 연락");
        assert!(contact.students.iter().any(|s| s.name == "홍길동"));
    }

    #[test]
    fn contained_tasks_merge() {
        let duties = vec![DutyEntry { id: 3, name: "교무기획".into(), detail: "학사일정 관리".into(), guide: String::new(), keywords: vec!["학사력".into()] }];
        let raw = "학사력 수정본 금요일까지 교육청에 제출.";
        let out = llm(r#"{"summary":"교육청에 제출","items":[
            {"kind":"task","text":"학사력 수정본 금요일까지 교육청에 제출","students":[],"area":"none","duty":1,"date":"금요일까지"},
            {"kind":"task","text":"금요일까지 교육청에 제출","students":[],"area":"none","duty":0,"date":"금요일까지"},
            {"kind":"duty","text":"학사력 수정본 금요일까지 교육청에 제출","students":[],"area":"none","duty":1,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &duties));
        let t = kinds(&p, ItemKind::Task);
        assert_eq!(t.len(), 1, "{:?}", p.items);
        assert_eq!(t[0].text, "학사력 수정본 교육청에 제출");
        assert_eq!(t[0].duty_id, Some(3));
        assert_eq!(kinds(&p, ItemKind::Duty).len(), 1);
    }

    #[test]
    fn student_counseling_is_an_observation_not_a_task() {
        // 0.5B 실제 출력: 같은 문구를 진로·봉사 관찰과 날짜 없는 일정으로 중복 제출
        let raw = "2-3 성춘향 진로 상담. 간호사가 되고 싶다고 함.";
        let out = llm(r#"{"summary":"성춘향 진로 상담","items":[
            {"kind":"student","text":"2-3 성춘향 진로 상담","students":["성춘향"],"area":"career","duty":0,"date":""},
            {"kind":"student","text":"2-3 성춘향 진로 상담","students":["성춘향"],"area":"volunteer","duty":0,"date":""},
            {"kind":"event","text":"2-3 성춘향 진로 상담","students":[],"area":"none","duty":0,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        assert!(kinds(&p, ItemKind::Task).is_empty(), "{:?}", p.items);
        assert!(kinds(&p, ItemKind::Event).is_empty(), "{:?}", p.items);
        let obs = kinds(&p, ItemKind::Student);
        assert!(obs.iter().all(|o| o.area == Area::Career), "{:?}", obs);
        assert_eq!(obs.len(), 1, "{:?}", obs);
    }

    #[test]
    fn event_keeps_time_from_its_clause() {
        let raw = "다음 주 수요일 오후 3시 교과협의회";
        let out = llm(r#"{"summary":"교과협의회","items":[
            {"kind":"event","text":"교과협의회","students":[],"area":"none","duty":0,"date":"다음 주 수요일"}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        let ev = kinds(&p, ItemKind::Event);
        assert_eq!(ev.len(), 1, "{:?}", p.items);
        assert_eq!(ev[0].date.as_deref(), Some("2026-10-07"));
        assert_eq!(ev[0].time.as_deref(), Some("15:00"));
    }

    #[test]
    fn multi_sentence_item_is_split() {
        let raw = "10월 20일 체육대회. 반별 응원 준비 필요.";
        let out = llm(r#"{"summary":"체육대회","items":[
            {"kind":"task","text":"체육대회. 반별 응원 준비 필요","students":[],"area":"none","duty":0,"date":"10월 20일"}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        let ev = kinds(&p, ItemKind::Event);
        assert!(ev.iter().any(|e| e.text.contains("체육대회") && e.date.as_deref() == Some("2026-10-20")), "{:?}", p.items);
        let t = kinds(&p, ItemKind::Task);
        assert!(t.iter().any(|t| t.text.contains("응원 준비")), "{:?}", p.items);
        assert!(t.iter().all(|t| !t.text.contains("체육대회")), "{:?}", p.items);
    }

    #[test]
    fn volunteer_needs_evidence() {
        let raw = "1-1 최유진 동아리 시간에 로봇 코딩 발표를 맡아 잘 해냄.";
        let out = llm(r#"{"summary":"최유진 발표","items":[
            {"kind":"student","text":"1-1 최유진 동아리 시간에 로봇 코딩 발표를 맡아 잘 해냄","students":["최유진"],"area":"volunteer","duty":0,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        let obs = kinds(&p, ItemKind::Student);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].area, Area::Club);
    }

    // 2B 모델 실제 출력 재현: 원문 낱말과 다른 영역
    #[test]
    fn area_follows_explicit_words() {
        let raw = "2학년 1반 3번 홍길동 학급회의에서 의견을 적극 제시함.";
        let out = llm(r#"{"summary":"홍길동 의견 제시","items":[
            {"kind":"student","text":"2학년 1반 3번 홍길동 학급회의에서 의견을 적극 제시함","students":["홍길동"],"area":"behavior","duty":0,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        assert_eq!(kinds(&p, ItemKind::Student)[0].area, Area::Autonomous);
        let raw = "1-1 최유진 동아리 시간에 로봇 코딩 발표를 맡아 잘 해냄.";
        let out = llm(r#"{"summary":"최유진 발표","items":[
            {"kind":"student","text":"1-1 최유진 동아리 시간에 로봇 코딩 발표를 맡아 잘 해냄","students":["최유진"],"area":"autonomous","duty":0,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        assert_eq!(kinds(&p, ItemKind::Student)[0].area, Area::Club);
        // 근거가 있는 AI 판단은 존중한다 ("친구" → 행동특성, 수업 중이어도)
        let raw = "2-1 홍길동 수업 중 친구를 도와 모둠 활동을 이끔.";
        let out = llm(r#"{"summary":"홍길동","items":[
            {"kind":"student","text":"2-1 홍길동 수업 중 친구를 도와 모둠 활동을 이끔","students":["홍길동"],"area":"behavior","duty":0,"date":""}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        assert_eq!(kinds(&p, ItemKind::Student)[0].area, Area::Behavior);
    }

    // 아래 두 시험은 검증 세트(H1, H13)에서 드러난 일반적인 빈틈을 막는다 (공개 후 수정)
    #[test]
    fn attendance_fact_is_not_a_calendar_event() {
        let raw = "3-1 윤서아 오늘 결석(질병). 보호자 전화 옴.";
        let out = llm(r#"{"summary":"윤서아 결석","items":[
            {"kind":"student","text":"3-1 윤서아 오늘 결석(질병)","students":["윤서아"],"area":"attendance","duty":0,"date":"오늘"},
            {"kind":"event","text":"보호자 전화 옴","students":[],"area":"none","duty":0,"date":""},
            {"kind":"event","text":"3-1 윤서아 결석(질병)","students":[],"area":"none","duty":0,"date":"오늘"}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        assert!(kinds(&p, ItemKind::Event).is_empty(), "{:?}", p.items);
        assert!(kinds(&p, ItemKind::Task).is_empty(), "{:?}", p.items);
        assert!(kinds(&p, ItemKind::Student).iter().any(|o| o.area == Area::Attendance));
    }

    #[test]
    fn studentless_observation_duplicating_event_is_dropped() {
        let raw = "오늘 5교시 전교생 소방 대피 훈련.";
        let out = llm(r#"{"summary":"대피 훈련","items":[
            {"kind":"student","text":"5교시 전교생 소방 대피 훈련","students":[],"area":"subject","duty":0,"date":"오늘"},
            {"kind":"event","text":"5교시 전교생 소방 대피 훈련","students":[],"area":"none","duty":0,"date":"오늘"}
        ]}"#);
        let p = guard_llm(raw, &out, &ctx(&[], &[]));
        assert!(kinds(&p, ItemKind::Student).is_empty(), "{:?}", p.items);
        assert_eq!(kinds(&p, ItemKind::Event).len(), 1);
    }

    #[test]
    fn broken_json_is_tolerated() {
        assert!(parse_llm_json("```json\n{\"summary\":\"a\",\"items\":[]}\n```").is_some());
        assert!(parse_llm_json("not json").is_none());
    }
}
