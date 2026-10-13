//! 기록 속 학생 개인정보 → 학생 명렬표 (2차 수정목록 8번)
//!
//! - 전화번호·보호자 연락처(관계 + 번호, 최대 4개)·주소·성별·생일·학생개인번호·이전학적·학급 번호는 AI보다 먼저 **형식 규칙**으로 찾는다.
//! - 찾은 문장은 학생부 참고자료·할 일·일정으로 보내지 않고 명렬표의 그 학생 칸에 넣는다.
//! - 이미 값이 있으면 덮어쓰지 않고 "기존 → 새 값" 확인을 기다린다(pending). 넣은 것·고른 것은 모두 이력으로 남는다.
//! - 특이사항으로 보이는 내용(건강·주의가 필요한 사정)은 기록 카드에서 "어디에 둘까요?"를 묻는다.
//! - 개인정보는 학생부 참고자료와 AI 초안 문맥에 넣지 않는다(`scrub`).

use crate::classify::PlanStudent;
use crate::error::{AppError, AppResult};
use crate::students;
use crate::util;
use chrono::NaiveDateTime;
use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// 명렬표 칸 (기록에서 찾아 넣는 것만. 비고는 직접 입력, 특이사항은 물어본 뒤)
pub const FIELDS: [&str; 9] = ["phone", "guardian", "address", "gender", "birthday", "student_no", "prev_record", "number", "remark"];
pub const MAX_CONTACTS: usize = 4;

pub fn field_label(f: &str) -> &'static str {
    match f {
        "phone" => "전화번호",
        "guardian" => "보호자 연락처",
        "address" => "주소",
        "gender" => "성별",
        "birthday" => "생일",
        "student_no" => "학생개인번호",
        "prev_record" => "이전학적",
        "number" => "번호",
        "remark" => "비고",
        "memo" => "특이사항",
        _ => "개인정보",
    }
}

/// 분류 계획의 개인정보 한 건 (규칙이 찾아 미리 채운다. 수동 분류 화면에서 뺄 수 있다)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfileItem {
    pub field: String,
    pub value: String,
    /// 보호자 연락처의 관계 (모·부·조모 …)
    #[serde(default)]
    pub relation: String,
    pub student: PlanStudent,
    /// 근거 문장
    #[serde(default)]
    pub text: String,
}

/// 특이사항으로 보이는 문장: 어디에 둘지 교사에게 묻는다
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecialItem {
    pub text: String,
    pub student: PlanStudent,
    /// 학생부 자료로 보낼 때 쓸 영역 (문장에서 짐작, 없으면 행특)
    #[serde(default)]
    pub area: String,
}

// ───────────────────────── 형식 규칙 ─────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub field: &'static str,
    pub value: String,
    pub relation: String,
}

static RE_PHONE: Lazy<Regex> = Lazy::new(|| Regex::new(r"01[016789][-.\s]?[0-9]{3,4}[-.\s]?[0-9]{4}|0(?:2|[3-6][0-9])-[0-9]{3,4}-[0-9]{4}").unwrap());
static RE_ADDRESS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?:(?:서울|부산|대구|인천|광주|대전|울산|세종|경기|강원|충북|충남|전북|전남|경북|경남|제주)[가-힣]*|[가-힣A-Za-z○]+도)\s+[가-힣A-Za-z○0-9]+(?:시|군|구)(?:\s+[가-힣A-Za-z○0-9]+(?:구|읍|면|동|리))*(?:\s+[가-힣A-Za-z○0-9]+(?:로|길))?(?:\s*[0-9]+(?:-[0-9]+)?(?:번지)?)?(?:\s*,?\s*[0-9]+동\s*[0-9]+호)?",
    )
    .unwrap()
});
static RE_ADDR_TAIL: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*(?:으로|로)?\s*(?:바뀜|바뀌|변경|이사|이전|옮김|옮겼|입니다|이다|임)[^0-9]*$").unwrap());
static RE_BIRTHDAY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:생일|생년월일)\s*(?:은|는|이|:)?\s*((?:[0-9]{4}\s*[.\-/년]\s*)?[0-9]{1,2}\s*[.\-/월]\s*[0-9]{1,2})").unwrap());
/// 성별: "성별 남/여" 또는 "남학생임"처럼 그 학생을 말할 때만 ("남학생들이 청소함" 같은 문장은 아님)
static RE_GENDER: Lazy<Regex> = Lazy::new(|| Regex::new(r"성별\s*(?:은|는|:)?\s*(남|여)|(남|여)학생\s*(?:임|이다|입니다|이며)").unwrap());
static RE_STUDENT_NO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:학생\s*개인\s*번호|개인\s*번호)\s*(?:은|는|:)?\s*([0-9]{6,12})").unwrap());
static RE_PREV: Lazy<Regex> = Lazy::new(|| Regex::new(r"이전\s*학적\s*(?:은|는|:)?\s*(.+)$").unwrap());
static RE_CLASS_NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:출석|학급)?\s*번호\s*(?:는|은|가|이|를|을|:)?\s*([0-9]{1,2})").unwrap());

/// 관계 낱말 (긴 것 먼저)
const RELATIONS: [(&str, &str); 24] = [
    ("외할머니", "외조모"),
    ("외할아버지", "외조부"),
    ("할머니", "조모"),
    ("할아버지", "조부"),
    ("어머니", "모"),
    ("어머님", "모"),
    ("엄마", "모"),
    ("모친", "모"),
    ("아버지", "부"),
    ("아버님", "부"),
    ("아빠", "부"),
    ("부친", "부"),
    ("외삼촌", "외삼촌"),
    ("삼촌", "삼촌"),
    ("이모", "이모"),
    ("고모", "고모"),
    ("누나", "누나"),
    ("언니", "언니"),
    ("오빠", "오빠"),
    ("학부모", "보호자"),
    ("보호자", "보호자"),
    ("부모님", "보호자"),
    ("집 전화", "집"),
    ("집전화", "집"),
];

/// 번호 모양 맞추기: 01012345678 → 010-1234-5678
pub fn normalize_phone(s: &str) -> String {
    let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    match d.len() {
        11 => format!("{}-{}-{}", &d[..3], &d[3..7], &d[7..]),
        10 if d.starts_with("02") => format!("{}-{}-{}", &d[..2], &d[2..6], &d[6..]),
        10 => format!("{}-{}-{}", &d[..3], &d[3..6], &d[6..]),
        9 if d.starts_with("02") => format!("{}-{}-{}", &d[..2], &d[2..5], &d[5..]),
        _ => s.trim().to_string(),
    }
}

fn relation_near(text: &str, start: usize, end: usize, after_prev: usize) -> String {
    // 번호 앞(지난 번호 뒤부터)에서 가장 가까운 관계 낱말
    let before = &text[after_prev..start];
    let mut best: Option<(usize, &str)> = None;
    for (w, rel) in RELATIONS {
        if let Some(i) = before.rfind(w) {
            if best.is_none_or(|(b, _)| i > b) {
                best = Some((i, rel));
            }
        }
    }
    if let Some((_, rel)) = best {
        return rel.to_string();
    }
    // "010-… (어머니)" 처럼 번호 바로 뒤
    let tail: String = text[end..].chars().take(8).collect();
    RELATIONS.iter().find(|(w, _)| tail.contains(w)).map(|(_, r)| r.to_string()).unwrap_or_default()
}

/// 한 문장에서 명렬표에 넣을 개인정보를 찾는다
pub fn find_hits(sentence: &str) -> Vec<Hit> {
    let t = sentence.trim();
    let mut out: Vec<Hit> = Vec::new();
    // 전화번호 (관계 낱말이 있으면 보호자 연락처)
    let mut prev_end = 0;
    for m in RE_PHONE.find_iter(t) {
        let before_ok = t[..m.start()].chars().last().is_none_or(|c| !c.is_ascii_digit());
        let after_ok = t[m.end()..].chars().next().is_none_or(|c| !c.is_ascii_digit());
        if !(before_ok && after_ok) {
            continue;
        }
        let rel = relation_near(t, m.start(), m.end(), prev_end);
        let value = normalize_phone(m.as_str());
        if rel.is_empty() {
            out.push(Hit { field: "phone", value, relation: String::new() });
        } else {
            out.push(Hit { field: "guardian", value, relation: rel });
        }
        prev_end = m.end();
    }
    // 주소: "주소" 뒤의 글, 없으면 시·도 → 시·군·구 → 읍·면·동·로·길이 이어지는 글
    let address = if let Some(i) = t.find("주소") {
        let mut v = t[i + "주소".len()..].trim_start();
        for p in ["는", "은", "가", "를", "지", ":", "：", "-", "—", "변경", "바뀜", "이"] {
            v = v.strip_prefix(p).unwrap_or(v).trim_start();
        }
        let v = RE_ADDR_TAIL.replace(v, "").trim().trim_end_matches(['.', ',', '·']).trim().to_string();
        let plausible = v.chars().count() >= 6 && (v.chars().any(|c| c.is_ascii_digit()) || ["동", "읍", "면", "로", "길", "리"].iter().any(|w| v.contains(w)));
        plausible.then_some(v)
    } else {
        RE_ADDRESS
            .find(t)
            .map(|m| m.as_str().trim().to_string())
            .filter(|v| ["읍", "면", "동", "로", "길", "리"].iter().any(|w| v.contains(w)) && v.chars().any(|c| c.is_ascii_digit()))
    };
    if let Some(v) = address {
        // 주소 안의 번호를 전화번호로 잘못 잡지 않게 (주소가 있으면 그 범위의 번호는 뺀다)
        out.retain(|h| !(h.field == "phone" && v.contains(&h.value)));
        out.push(Hit { field: "address", value: v, relation: String::new() });
    }
    if let Some(c) = RE_BIRTHDAY.captures(t) {
        if let Ok(Some(v)) = students::normalize_birthday(&c[1]) {
            out.push(Hit { field: "birthday", value: v, relation: String::new() });
        }
    }
    if let Some(c) = RE_GENDER.captures(t) {
        let g = c.get(1).or_else(|| c.get(2)).map(|m| m.as_str()).unwrap_or("");
        out.push(Hit { field: "gender", value: g.to_string(), relation: String::new() });
    }
    if let Some(c) = RE_STUDENT_NO.captures(t) {
        out.push(Hit { field: "student_no", value: c[1].to_string(), relation: String::new() });
    }
    if let Some(c) = RE_PREV.captures(t) {
        let v = c[1].trim().trim_end_matches(['.', ',']).trim().to_string();
        if !v.is_empty() {
            out.push(Hit { field: "prev_record", value: v, relation: String::new() });
        }
    }
    out
}

/// 학급 번호(출석번호): "3-1 홍길동 번호는 21번", "홍길동의 출석번호 7", "홍길동은 번호가 12번으로 바뀜".
/// '번호' 바로 앞이 그 학생 이름(+은·는·의·이·가)일 때만 — "문항 번호 3번", "사물함 번호 12", "전화번호 010-…"는 아니다.
pub fn find_number(sentence: &str, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    for c in RE_CLASS_NUMBER.captures_iter(sentence) {
        let (whole, num) = (c.get(0)?, c.get(1)?);
        // 숫자 뒤에 숫자·하이픈 등이 이어지면 전화번호 같은 다른 번호
        let rest = &sentence[num.end()..];
        let mut it = rest.chars();
        let next_ok = match it.next() {
            None => true,
            Some(ch) if ch.is_ascii_digit() || matches!(ch, '-' | ':' | '~' | '/') => false,
            Some('.') => !it.next().is_some_and(|d| d.is_ascii_digit()),
            Some(_) => true,
        };
        if !next_ok {
            continue;
        }
        let before = sentence[..whole.start()].trim_end().trim_end_matches([',', '·']).trim_end();
        let owner = before.ends_with(name) || ["의", "은", "는", "이", "가"].iter().any(|p| before.strip_suffix(p).is_some_and(|b| b.ends_with(name)));
        if !owner {
            continue;
        }
        let n: u32 = num.as_str().parse().ok()?;
        if (1..=99).contains(&n) {
            return Some(n.to_string());
        }
    }
    None
}

/// 학적 변동 (4차 3번): 전학·전출·전입. 학생 관찰이 아니라 교사가 처리할 일이다.
#[derive(Debug, Clone, PartialEq)]
pub struct Transfer {
    /// 전입(들어옴)이면 true, 전출(나감)이면 false
    pub incoming: bool,
    /// 가는 곳·온 곳 ("OO중", "광주"). 모르면 빈칸
    pub place: String,
    /// 명렬표에 없는 전입생 이름 ("이름 홍길동")
    pub name: Option<String>,
    /// "3-2"처럼 적은 학급
    pub class_label: Option<String>,
}

// "전학년"(모든 학년)·"전학생 대상"(모든 학생)은 학적 변동이 아니다
static RE_TR_IN: Lazy<Regex> = Lazy::new(|| Regex::new(r"전입|전학\s*(?:옴|왔|와서|온다|오기로|올\s)|전학생\s*(?:이\s*)?(?:옴|왔|온|들어)").unwrap());
static RE_TR_OUT: Lazy<Regex> = Lazy::new(|| Regex::new(r"전출|전학(?:[^년생]|$)").unwrap());
static RE_TR_FROM: Lazy<Regex> = Lazy::new(|| Regex::new(r"([가-힣A-Za-z0-9]+?)(?:에서|으로부터|로부터)\s*(?:전입|전학)").unwrap());
static RE_TR_TO: Lazy<Regex> = Lazy::new(|| Regex::new(r"([가-힣A-Za-z0-9]+?)(?:으로|로|에)?\s*(?:전출|전학)").unwrap());
static RE_TR_NAME: Lazy<Regex> = Lazy::new(|| Regex::new(r"이름\s*(?:은|는|:)?\s*([가-힣]{2,4})").unwrap());
static RE_TR_CLASS: Lazy<Regex> = Lazy::new(|| Regex::new(r"([1-6])\s*-\s*([0-9]{1,2})").unwrap());

pub fn find_transfer(sentence: &str) -> Option<Transfer> {
    let t = sentence.trim();
    let incoming = RE_TR_IN.is_match(t);
    if !incoming && !RE_TR_OUT.is_match(t) {
        return None;
    }
    let re = if incoming { &*RE_TR_FROM } else { &*RE_TR_TO };
    let place = re
        .captures(t)
        .map(|c| c[1].to_string())
        // 날짜·학년·학급이나 '전입생' 같은 말은 학교 이름이 아니다
        .filter(|w| {
            !w.chars().any(|c| c.is_ascii_digit()) && !w.ends_with("학년") && !w.ends_with('반') && !w.ends_with('생') && !["전학", "전입", "전출"].contains(&w.as_str())
        })
        .unwrap_or_default();
    let name = incoming.then(|| RE_TR_NAME.captures(t).map(|c| c[1].to_string())).flatten();
    let class_label = RE_TR_CLASS.captures(t).map(|c| format!("{}-{}", &c[1], &c[2]));
    Some(Transfer { incoming, place, name, class_label })
}

/// 명렬표 비고에 적을 말: "OO중 전학 예정 (10월 12일 기록)", "OO중에서 전입 (10월 12일 기록)"
pub fn transfer_remark(t: &Transfer, student_name: &str, recorded: chrono::NaiveDate, when: Option<chrono::NaiveDate>) -> String {
    use chrono::Datelike;
    let place = if t.place == student_name { "" } else { t.place.as_str() };
    let mut s = match (t.incoming, place.is_empty()) {
        (true, false) => format!("{place}에서 전입"),
        (true, true) => "전입".to_string(),
        (false, false) => format!("{place} 전학 예정"),
        (false, true) => "전학 예정".to_string(),
    };
    if let Some(d) = when.filter(|_| !t.incoming) {
        s.push_str(&format!(", {}월 {}일", d.month(), d.day()));
    }
    s.push_str(&format!(" ({}월 {}일 기록)", recorded.month(), recorded.day()));
    s
}

/// 특이사항으로 보이는 문장 (건강·주의가 필요한 사정). 출결 문장은 출결로 둔다.
const SPECIAL_WORDS: [&str; 26] = [
    "어지러", "어지럼", "알레르기", "알러지", "천식", "아토피", "당뇨", "뇌전증", "간질", "빈혈", "복용", "투약", "지병", "특이체질", "발작", "공황",
    "우울", "자해", "ADHD", "틱 증상", "틱장애", "장애", "건강상", "보건실에 자주", "자주 아프", "수술",
];
pub fn is_special(sentence: &str) -> bool {
    let t = sentence;
    if ["결석", "지각", "조퇴", "결과"].iter().any(|w| t.contains(w)) {
        return false;
    }
    SPECIAL_WORDS.iter().any(|w| t.contains(w))
}

/// 개인정보 가리기: 전화번호는 뒤 4자리만, 주소는 앞 두 낱말만 (학생부 AI 초안 문맥·화면 가리기용)
pub fn scrub(text: &str) -> String {
    let t = RE_PHONE.replace_all(text, |c: &regex::Captures| {
        let d: String = c[0].chars().filter(|c| c.is_ascii_digit()).collect();
        format!("●●●-●●●●-{}", &d[d.len().saturating_sub(4)..])
    });
    let t = RE_ADDRESS.replace_all(&t, |c: &regex::Captures| {
        let words: Vec<&str> = c[0].split_whitespace().take(2).collect();
        format!("{} ●●●", words.join(" "))
    });
    t.into_owned()
}

// ───────────────────────── 보호자 연락처 ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Contact {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub relation: String,
    pub phone: String,
}

pub fn contacts(conn: &Connection, student_id: i64) -> AppResult<Vec<Contact>> {
    let mut st = conn.prepare("SELECT id, relation, phone FROM student_contacts WHERE student_id=?1 ORDER BY pos, id")?;
    let v = st
        .query_map(params![student_id], |r| Ok(Contact { id: r.get(0)?, relation: r.get(1)?, phone: r.get(2)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// 학생 명렬표의 보호자 연락처 칸에 보일 모양: 모든 학생의 연락처 (학생 id → 목록)
pub fn all_contacts(conn: &Connection) -> AppResult<std::collections::HashMap<i64, Vec<Contact>>> {
    let mut st = conn.prepare("SELECT student_id, id, relation, phone FROM student_contacts ORDER BY student_id, pos, id")?;
    let mut m: std::collections::HashMap<i64, Vec<Contact>> = std::collections::HashMap::new();
    for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, Contact { id: r.get(1)?, relation: r.get(2)?, phone: r.get(3)? })))? {
        let (sid, c) = row?;
        m.entry(sid).or_default().push(c);
    }
    Ok(m)
}

/// students.guardian_phone 을 첫 연락처로 맞춘다 (예전 칸·내보내기 호환)
fn sync_guardian(conn: &Connection, student_id: i64, now: NaiveDateTime) -> AppResult<()> {
    let first: Option<String> = conn
        .query_row("SELECT phone FROM student_contacts WHERE student_id=?1 ORDER BY pos, id LIMIT 1", params![student_id], |r| r.get(0))
        .optional()?;
    conn.execute(
        "UPDATE students SET guardian_phone=?2, updated_at=?3 WHERE id=?1",
        params![student_id, first.unwrap_or_default(), util::fmt_dt(now)],
    )?;
    Ok(())
}

/// 보호자 연락처를 통째로 바꾼다 (명렬표에서 직접 고칠 때, 최대 4개)
pub fn set_contacts(conn: &Connection, student_id: i64, list: &[Contact], now: NaiveDateTime) -> AppResult<Vec<Contact>> {
    let cleaned: Vec<Contact> = list
        .iter()
        .filter(|c| !c.phone.trim().is_empty())
        .map(|c| Contact { id: c.id, relation: c.relation.trim().chars().take(10).collect(), phone: normalize_phone(c.phone.trim()) })
        .collect();
    if cleaned.len() > MAX_CONTACTS {
        return Err(AppError::invalid(format!("보호자 연락처는 {MAX_CONTACTS}개까지 둘 수 있습니다.")));
    }
    if cleaned.iter().any(|c| c.phone.chars().count() > 20) {
        return Err(AppError::invalid("번호가 너무 깁니다."));
    }
    let t = util::fmt_dt(now);
    // 있던 연락처는 id 를 그대로 두고 고친다 (확인을 기다리는 변경이 가리키는 연락처가 바뀌지 않게)
    let before: Vec<i64> = contacts(conn, student_id)?.iter().map(|c| c.id).collect();
    let mut kept: Vec<i64> = Vec::new();
    for (i, c) in cleaned.iter().enumerate() {
        if c.id != 0 && before.contains(&c.id) && !kept.contains(&c.id) {
            conn.execute(
                "UPDATE student_contacts SET relation=?2, phone=?3, pos=?4, updated_at=?5 WHERE id=?1",
                params![c.id, c.relation, c.phone, i as i64, t],
            )?;
            kept.push(c.id);
        } else {
            conn.execute(
                "INSERT INTO student_contacts(student_id, relation, phone, pos, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![student_id, c.relation, c.phone, i as i64, t],
            )?;
            kept.push(conn.last_insert_rowid());
        }
    }
    for id in before.iter().filter(|id| !kept.contains(id)) {
        conn.execute("DELETE FROM student_contacts WHERE id=?1", params![id])?;
    }
    sync_guardian(conn, student_id, now)?;
    contacts(conn, student_id)
}

/// 명렬표의 예전 '보호자 연락처' 칸을 직접 고쳤을 때: 첫 연락처의 번호를 바꾼다 (비우면 첫 연락처를 지운다)
pub fn set_first_contact(conn: &Connection, student_id: i64, phone: &str, now: NaiveDateTime) -> AppResult<()> {
    let mut list = contacts(conn, student_id)?;
    if phone.trim().is_empty() {
        if !list.is_empty() {
            list.remove(0);
        }
    } else if let Some(first) = list.first_mut() {
        first.phone = phone.to_string();
    } else {
        list.push(Contact { id: 0, relation: String::new(), phone: phone.to_string() });
    }
    set_contacts(conn, student_id, &list, now)?;
    Ok(())
}

// ───────────────────────── 기록 → 명렬표 ─────────────────────────

fn current_value(conn: &Connection, student_id: i64, field: &str) -> AppResult<String> {
    let col = match field {
        "phone" | "address" | "gender" | "student_no" | "prev_record" => field,
        "birthday" => "COALESCE(birthday,'')",
        "number" => "COALESCE(CAST(number AS TEXT),'')",
        "remark" => "remark",
        "memo" => "memo",
        _ => return Ok(String::new()),
    };
    Ok(conn.query_row(&format!("SELECT {col} FROM students WHERE id=?1"), params![student_id], |r| r.get::<_, String>(0)).optional()?.unwrap_or_default())
}

fn log_change(
    conn: &Connection,
    note_id: Option<i64>,
    student_id: i64,
    field: &str,
    relation: &str,
    old: &str,
    new: &str,
    status: &str,
    target_id: Option<i64>,
    source: &str,
    now: NaiveDateTime,
) -> AppResult<i64> {
    let t = util::fmt_dt(now);
    conn.execute(
        "INSERT INTO profile_changes(note_id, student_id, field, relation, old_value, new_value, status, target_id, source, created_at, decided_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, CASE WHEN ?7='pending' THEN NULL ELSE ?10 END)",
        params![note_id, student_id, field, relation, old, new, status, target_id, source, t],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 개인정보 한 건을 학생 칸에 넣는다. 비어 있으면 바로, 같으면 그대로, 다르면 확인 기다림.
fn apply_one(conn: &Connection, note_id: i64, sid: i64, item: &ProfileItem, source: &str, now: NaiveDateTime) -> AppResult<()> {
    let value = item.value.trim();
    if value.is_empty() || !FIELDS.contains(&item.field.as_str()) {
        return Ok(());
    }
    // 같은 기록에서 같은 내용을 이미 처리했으면(다시 분류) 건너뛴다
    let done: Option<i64> = conn
        .query_row(
            "SELECT id FROM profile_changes WHERE note_id=?1 AND student_id=?2 AND field=?3 AND new_value=?4 AND status IN ('applied','rejected')",
            params![note_id, sid, item.field, value],
            |r| r.get(0),
        )
        .optional()?;
    if done.is_some() {
        return Ok(());
    }
    if item.field == "guardian" {
        let list = contacts(conn, sid)?;
        if list.iter().any(|c| normalize_phone(&c.phone) == normalize_phone(value)) {
            return Ok(());
        }
        let rel = item.relation.trim();
        // 같은 관계의 번호가 이미 있으면 추가하지 않고 "기존 → 새 값" 확인
        if let Some(c) = list.iter().find(|c| !rel.is_empty() && c.relation == rel) {
            log_change(conn, Some(note_id), sid, "guardian", rel, &c.phone, value, "pending", Some(c.id), source, now)?;
            return Ok(());
        }
        if list.len() >= MAX_CONTACTS {
            // 4개가 찼으면 어느 것을 바꿀지 묻는다
            log_change(conn, Some(note_id), sid, "guardian", rel, "", value, "pending", None, source, now)?;
            return Ok(());
        }
        let mut next = list.clone();
        next.push(Contact { id: 0, relation: rel.to_string(), phone: value.to_string() });
        set_contacts(conn, sid, &next, now)?;
        log_change(conn, Some(note_id), sid, "guardian", rel, "", value, "applied", None, source, now)?;
        return Ok(());
    }
    let cur = current_value(conn, sid, &item.field)?;
    if cur.trim() == value {
        return Ok(());
    }
    // 비고(학적 변동): 덮어쓰지 않고 뒤에 이어 적는다 (4차 3번)
    if item.field == "remark" {
        let cur = cur.trim();
        if cur.contains(value) {
            return Ok(());
        }
        let next = if cur.is_empty() { value.to_string() } else { format!("{cur} / {value}") };
        if next.chars().count() > 200 {
            // 비고 칸이 가득 차면 바꿀지 기록 카드에서 묻는다
            log_change(conn, Some(note_id), sid, "remark", "", cur, value, "pending", None, source, now)?;
        } else {
            students::set_roster_field(conn, sid, "remark", &next, now)?;
            log_change(conn, Some(note_id), sid, "remark", "", "", value, "applied", None, source, now)?;
        }
        return Ok(());
    }
    // 같은 학급에 이미 그 번호인 학생이 있으면 바로 넣지 않고 확인을 기다린다
    let taken = item.field == "number"
        && conn
            .query_row(
                "SELECT 1 FROM students o JOIN students s ON s.id=?1 WHERE o.id<>s.id AND o.archived=0 AND o.class_id IS s.class_id AND o.number=?2",
                params![sid, value.parse::<i64>().unwrap_or(-1)],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
    if cur.trim().is_empty() && !taken {
        students::set_roster_field(conn, sid, &item.field, value, now)?;
        log_change(conn, Some(note_id), sid, &item.field, "", "", value, "applied", None, source, now)?;
    } else {
        log_change(conn, Some(note_id), sid, &item.field, "", cur.trim(), value, "pending", None, source, now)?;
    }
    Ok(())
}

/// 분류를 저장할 때: 개인정보를 명렬표로, 특이사항은 질문으로. 학생을 못 정한 이름은 `unresolved` 에 더한다.
const BACKFILL_KEY: &str = "profile_backfill_v1";

/// 한 번만: 2차 수정 전 빌드가 AI 학생 관찰로 학생부 자료에 넣은 개인정보를 학생 명렬표로 옮긴다.
/// 교사가 직접 쓴 관찰은 건드리지 않는다. 이미 값이 있으면 덮어쓰지 않고 기록 카드에서 확인을 기다린다.
pub fn backfill(conn: &Connection, now: NaiveDateTime) -> AppResult<usize> {
    if crate::db::get_setting(conn, BACKFILL_KEY)?.is_some() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let rows: Vec<(i64, i64, i64, String)> = {
        let mut st = tx.prepare("SELECT id, note_id, student_id, content FROM observations WHERE source='ai' AND note_id IS NOT NULL AND student_id IS NOT NULL ORDER BY id")?;
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<Vec<_>, _>>()?;
        v
    };
    let mut moved = 0;
    for (oid, note_id, sid, content) in rows {
        let hits = find_hits(&content);
        if hits.is_empty() {
            continue;
        }
        for h in hits {
            let student = PlanStudent { student_id: Some(sid), name: String::new(), class_label: None, number: None, candidates: vec![] };
            let item = ProfileItem { field: h.field.to_string(), value: h.value, relation: h.relation, student, text: content.clone() };
            apply_one(&tx, note_id, sid, &item, "ai", now)?;
        }
        tx.execute("DELETE FROM observations WHERE id=?1", params![oid])?;
        moved += 1;
    }
    crate::db::set_setting(&tx, BACKFILL_KEY, &util::fmt_dt(now))?;
    tx.commit()?;
    Ok(moved)
}

pub fn apply(
    conn: &Connection,
    note_id: i64,
    items: &[ProfileItem],
    specials: &[SpecialItem],
    source: &str,
    created: NaiveDateTime,
    now: NaiveDateTime,
    unresolved: &mut Vec<String>,
) -> AppResult<()> {
    // 다시 분류하면 아직 답하지 않은 확인·질문은 새로 만든다 (넣은 것·고른 것은 이력으로 남는다)
    conn.execute("DELETE FROM profile_changes WHERE note_id=?1 AND status='pending'", params![note_id])?;
    conn.execute("DELETE FROM note_questions WHERE note_id=?1 AND status='pending'", params![note_id])?;
    let mut resolve = |ps: &PlanStudent| -> AppResult<Option<i64>> {
        let id = students::resolve_plan_student(conn, ps, created)?;
        if id.is_none() {
            let label = match &ps.class_label {
                Some(l) => format!("{l} {}", ps.name),
                None => ps.name.clone(),
            };
            if !label.trim().is_empty() && !unresolved.contains(&label) {
                unresolved.push(label);
            }
        }
        Ok(id)
    };
    for item in items {
        if let Some(sid) = resolve(&item.student)? {
            apply_one(conn, note_id, sid, item, source, now)?;
            conn.execute("INSERT OR IGNORE INTO note_students(note_id, student_id, source) VALUES (?1, ?2, ?3)", params![note_id, sid, source])?;
        }
    }
    for sp in specials {
        let text = sp.text.trim();
        if text.is_empty() {
            continue;
        }
        let Some(sid) = resolve(&sp.student)? else { continue };
        let answered: Option<i64> = conn
            .query_row("SELECT id FROM note_questions WHERE note_id=?1 AND student_id=?2 AND text=?3", params![note_id, sid, text], |r| r.get(0))
            .optional()?;
        if answered.is_none() {
            let area = if sp.area.trim().is_empty() { "behavior" } else { sp.area.trim() };
            conn.execute(
                "INSERT INTO note_questions(note_id, student_id, text, area, status, created_at) VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
                params![note_id, sid, text, area, util::fmt_dt(now)],
            )?;
        }
        conn.execute("INSERT OR IGNORE INTO note_students(note_id, student_id, source) VALUES (?1, ?2, ?3)", params![note_id, sid, source])?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileChange {
    pub id: i64,
    pub note_id: Option<i64>,
    pub student_id: i64,
    pub student_label: String,
    pub field: String,
    pub field_label: String,
    pub relation: String,
    pub old_value: String,
    pub new_value: String,
    /// applied | pending | rejected
    pub status: String,
    /// 보호자 연락처: 바꿀 연락처 (없으면 4개가 차 교사가 고른다)
    pub target_id: Option<i64>,
    /// 보호자 연락처가 4개 찼을 때 고를 목록
    pub contacts: Vec<Contact>,
    pub source: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteQuestion {
    pub id: i64,
    pub note_id: i64,
    pub student_id: Option<i64>,
    pub student_label: String,
    pub text: String,
    pub area: String,
    /// pending | record | roster
    pub status: String,
}

fn label_of(conn: &Connection, sid: i64) -> String {
    students::student_label(conn, sid).unwrap_or_default()
}

pub fn changes(conn: &Connection, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> AppResult<Vec<ProfileChange>> {
    let mut st = conn.prepare(&format!(
        "SELECT id, note_id, student_id, field, relation, old_value, new_value, status, target_id, source, created_at FROM profile_changes WHERE {where_sql} ORDER BY id"
    ))?;
    let rows = st
        .query_map(args, |r| {
            Ok(ProfileChange {
                id: r.get(0)?,
                note_id: r.get(1)?,
                student_id: r.get(2)?,
                student_label: String::new(),
                field: r.get(3)?,
                field_label: String::new(),
                relation: r.get(4)?,
                old_value: r.get(5)?,
                new_value: r.get(6)?,
                status: r.get(7)?,
                target_id: r.get(8)?,
                contacts: vec![],
                source: r.get(9)?,
                created_at: r.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for mut c in rows {
        c.student_label = label_of(conn, c.student_id);
        c.field_label = field_label(&c.field).to_string();
        if c.field == "guardian" && c.status == "pending" && c.target_id.is_none() {
            c.contacts = contacts(conn, c.student_id)?;
        }
        out.push(c);
    }
    Ok(out)
}

/// 학생 명렬표 변경 이력 (최근 것부터): 넣음·확인 기다림·그대로 둠
pub fn history(conn: &Connection, student_id: i64) -> AppResult<Vec<ProfileChange>> {
    let mut v = changes(conn, "student_id=?1", &[&student_id])?;
    v.reverse();
    Ok(v)
}

pub fn for_note(conn: &Connection, note_id: i64) -> AppResult<(Vec<ProfileChange>, Vec<NoteQuestion>)> {
    let ch = changes(conn, "note_id=?1 AND status<>'rejected'", &[&note_id])?;
    let mut st = conn.prepare("SELECT id, note_id, student_id, text, area, status FROM note_questions WHERE note_id=?1 ORDER BY id")?;
    let qs = st
        .query_map(params![note_id], |r| {
            Ok(NoteQuestion {
                id: r.get(0)?,
                note_id: r.get(1)?,
                student_id: r.get(2)?,
                student_label: String::new(),
                text: r.get(3)?,
                area: r.get(4)?,
                status: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let qs = qs
        .into_iter()
        .map(|mut q| {
            q.student_label = q.student_id.map(|s| label_of(conn, s)).unwrap_or_default();
            q
        })
        .collect();
    Ok((ch, qs))
}

/// 교사가 답해야 할 것 (기록 메뉴 숫자): 특이사항 질문 + 개인정보 확인
pub fn pending_count(conn: &Connection) -> AppResult<i64> {
    let a: i64 = conn.query_row("SELECT COUNT(*) FROM note_questions WHERE status='pending'", [], |r| r.get(0))?;
    let b: i64 = conn.query_row("SELECT COUNT(*) FROM profile_changes WHERE status='pending'", [], |r| r.get(0))?;
    Ok(a + b)
}

/// "기존 → 새 값" 확인에 답한다. accept=true 면 새 값으로(보호자 4개가 찼으면 replace_id 를 바꾼다)
pub fn resolve_change(conn: &Connection, id: i64, accept: bool, replace_id: Option<i64>, now: NaiveDateTime) -> AppResult<()> {
    let c = changes(conn, "id=?1", &[&id])?.into_iter().next().ok_or_else(|| AppError::not_found("확인할 항목을 찾을 수 없습니다."))?;
    if c.status != "pending" {
        return Ok(());
    }
    let t = util::fmt_dt(now);
    if !accept {
        conn.execute("UPDATE profile_changes SET status='rejected', decided_at=?2 WHERE id=?1", params![id, t])?;
        return Ok(());
    }
    let mut old = c.old_value.clone();
    if c.field == "guardian" {
        let mut list = contacts(conn, c.student_id)?;
        let target = c.target_id.or(replace_id);
        let at = target
            .and_then(|tid| list.iter().position(|x| x.id == tid))
            .or_else(|| if c.target_id.is_some() && !c.relation.is_empty() { list.iter().position(|x| x.relation == c.relation) } else { None });
        match at {
            Some(i) => {
                old = format!("{} {}", list[i].relation, list[i].phone).trim().to_string();
                list[i] = Contact { id: list[i].id, relation: if c.relation.is_empty() { list[i].relation.clone() } else { c.relation.clone() }, phone: c.new_value.clone() };
            }
            None if list.len() < MAX_CONTACTS => list.push(Contact { id: 0, relation: c.relation.clone(), phone: c.new_value.clone() }),
            None => return Err(AppError::invalid("바꿀 보호자 연락처를 골라 주세요.")),
        }
        set_contacts(conn, c.student_id, &list, now)?;
    } else {
        students::set_roster_field(conn, c.student_id, &c.field, &c.new_value, now)?;
    }
    conn.execute("UPDATE profile_changes SET status='applied', old_value=?2, decided_at=?3 WHERE id=?1", params![id, old, t])?;
    Ok(())
}

/// "이 내용을 어디에 둘까요?"에 답한다: record = 학생부 자료 참고자료, roster = 학생 명렬표 특이사항
pub fn answer_question(conn: &Connection, id: i64, choice: &str, now: NaiveDateTime) -> AppResult<()> {
    let (note_id, sid, text, area, status): (i64, Option<i64>, String, String, String) = conn
        .query_row("SELECT note_id, student_id, text, area, status FROM note_questions WHERE id=?1", params![id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .optional()?
        .ok_or_else(|| AppError::not_found("질문을 찾을 수 없습니다."))?;
    let t = util::fmt_dt(now);
    // 이미 답했으면 먼저 되돌린다 (나중에 바꿀 수 있게)
    if status == "record" {
        conn.execute("DELETE FROM observations WHERE note_id=?1 AND student_id IS ?2 AND content=?3", params![note_id, sid, text])?;
    } else if status == "roster" {
        if let Some(s) = sid {
            let memo = current_value(conn, s, "memo")?;
            let rest: Vec<&str> = memo.lines().filter(|l| l.trim() != text.trim()).collect();
            conn.execute("UPDATE students SET memo=?2, updated_at=?3 WHERE id=?1", params![s, rest.join("\n"), t])?;
        }
    }
    match choice {
        "record" => {
            let (_, created) = crate::notes::raw_and_created(conn, note_id)?;
            conn.execute(
                "INSERT INTO observations(note_id, student_id, area, content, observed_on, source, edited, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'manual', 0, ?6, ?6)",
                params![note_id, sid, area, text, util::fmt_d(created.date()), t],
            )?;
        }
        "roster" => {
            if let Some(s) = sid {
                let memo = current_value(conn, s, "memo")?;
                let next = if memo.trim().is_empty() { text.clone() } else { format!("{}\n{}", memo.trim_end(), text) };
                students::set_roster_field(conn, s, "memo", &next, now)?;
            }
        }
        _ => return Err(AppError::invalid("둘 곳을 골라 주세요.")),
    }
    conn.execute("UPDATE note_questions SET status=?2, decided_at=?3 WHERE id=?1", params![id, choice, t])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(s: &str) -> Vec<(String, String, String)> {
        find_hits(s).into_iter().map(|h| (h.field.to_string(), h.value, h.relation)).collect()
    }

    #[test]
    fn finds_phone_address_birthday_and_more() {
        assert_eq!(fields("홍길동 전화번호 010-1234-5678"), vec![("phone".into(), "010-1234-5678".into(), "".into())]);
        assert_eq!(fields("홍길동 01012345678"), vec![("phone".into(), "010-1234-5678".into(), "".into())]);
        assert_eq!(fields("홍길동 어머니 연락처 010-9876-5432"), vec![("guardian".into(), "010-9876-5432".into(), "모".into())]);
        assert_eq!(
            fields("홍길동 어머니 010-1111-2222, 아버지 010-3333-4444"),
            vec![("guardian".into(), "010-1111-2222".into(), "모".into()), ("guardian".into(), "010-3333-4444".into(), "부".into())]
        );
        assert_eq!(fields("홍길동 할머니 010-5555-6666"), vec![("guardian".into(), "010-5555-6666".into(), "조모".into())]);
        assert_eq!(fields("홍길동 주소 OO시 OO구 OO로 12"), vec![("address".into(), "OO시 OO구 OO로 12".into(), "".into())]);
        assert_eq!(fields("홍길동 주소는 OO시 OO로 34로 이사함"), vec![("address".into(), "OO시 OO로 34".into(), "".into())]);
        assert_eq!(fields("홍길동 경기 OO시 OO동 123-4"), vec![("address".into(), "경기 OO시 OO동 123-4".into(), "".into())]);
        assert_eq!(fields("홍길동 생일 3월 5일"), vec![("birthday".into(), "03-05".into(), "".into())]);
        assert_eq!(fields("홍길동 생년월일 2012.3.5"), vec![("birthday".into(), "2012-03-05".into(), "".into())]);
        assert_eq!(fields("홍길동 성별 여"), vec![("gender".into(), "여".into(), "".into())]);
        assert_eq!(fields("홍길동은 남학생임"), vec![("gender".into(), "남".into(), "".into())]);
        assert!(fields("홍길동 등 남학생들이 청소함").is_empty());
        assert_eq!(fields("홍길동 학생개인번호 9000000001"), vec![("student_no".into(), "9000000001".into(), "".into())]);
        assert_eq!(fields("홍길동 이전학적 2학년1반1번"), vec![("prev_record".into(), "2학년1반1번".into(), "".into())]);
        // 개인정보가 없는 문장
        assert!(fields("홍길동 수업 중 발표를 잘함").is_empty());
        assert!(fields("10월 12일 3교시 공개수업").is_empty());
    }

    #[test]
    fn finds_class_number_only_right_after_the_name() {
        assert_eq!(find_number("3-1 홍길동 번호는 21번", "홍길동"), Some("21".into()));
        assert_eq!(find_number("홍길동의 출석번호 7", "홍길동"), Some("7".into()));
        assert_eq!(find_number("홍길동은 번호가 12번으로 바뀜.", "홍길동"), Some("12".into()));
        assert_eq!(find_number("홍길동 번호를 05번으로 변경", "홍길동"), Some("5".into()));
        assert_eq!(find_number("홍길동 전화번호 010-1234-5678", "홍길동"), None);
        assert_eq!(find_number("홍길동 번호 010-1234-5678", "홍길동"), None, "전화번호");
        assert_eq!(find_number("홍길동 사물함 번호 12", "홍길동"), None);
        assert_eq!(find_number("홍길동 수행평가 문항 번호 3번 틀림", "홍길동"), None);
        assert_eq!(find_number("홍길동 학생개인번호 9000000001", "홍길동"), None);
        assert_eq!(find_number("홍길동 번호 바뀜", "홍길동"), None);
        assert_eq!(find_number("홍길동 번호는 0번", "홍길동"), None);
    }

    #[test]
    fn finds_transfers() {
        let t = find_transfer("2학년 홍길동 OO중 전학예정").unwrap();
        assert_eq!((t.incoming, t.place.as_str()), (false, "OO중"));
        let t = find_transfer("1-1 홍길동 10월 20일 광주로 전학").unwrap();
        assert_eq!((t.incoming, t.place.as_str()), (false, "광주"));
        let t = find_transfer("3학년에 OO중에서 전입생 옴, 이름 홍길동").unwrap();
        assert_eq!((t.incoming, t.place.as_str(), t.name.as_deref()), (true, "OO중", Some("홍길동")));
        let t = find_transfer("홍길동 전학 옴").unwrap();
        assert_eq!((t.incoming, t.place.as_str()), (true, ""));
        assert_eq!(find_transfer("전주로 전학 감").map(|t| t.place), Some("전주".into()), "'전'으로 시작하는 지명");
        assert!(find_transfer("전학년 체육대회").is_none());
        assert!(find_transfer("전학생 대상 설문").is_none(), "모든 학생");
        let d = |m, dd| chrono::NaiveDate::from_ymd_opt(2026, m, dd).unwrap();
        let t = find_transfer("홍길동 전학 감").unwrap();
        assert_eq!(transfer_remark(&t, "홍길동", d(10, 12), None), "전학 예정 (10월 12일 기록)", "이름은 갈 곳이 아니다");
    }

    #[test]
    fn remark_is_appended_not_overwritten() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let now = util::parse_dt("2026-10-12T12:06:00").unwrap();
            let sid = students::create_student(c, &students::StudentInput { name: "홍길동".into(), class_label: Some("2-1".into()), number: Some(1), memo: None }, now)?;
            students::set_roster_field(c, sid, "remark", "특별반", now)?;
            let note = crate::notes::create_note(c, "2-1 홍길동 OO중 전학예정", now)?;
            let item = ProfileItem {
                field: "remark".into(),
                value: "OO중 전학 예정 (10월 12일 기록)".into(),
                relation: String::new(),
                student: PlanStudent { student_id: Some(sid), name: "홍길동".into(), class_label: Some("2-1".into()), number: None, candidates: vec![] },
                text: String::new(),
            };
            apply(c, note, &[item.clone()], &[], "ai", now, now, &mut vec![])?;
            assert_eq!(current_value(c, sid, "remark")?, "특별반 / OO중 전학 예정 (10월 12일 기록)");
            // 다시 분류해도 두 번 적지 않는다
            apply(c, note, &[item], &[], "ai", now, now, &mut vec![])?;
            assert_eq!(current_value(c, sid, "remark")?, "특별반 / OO중 전학 예정 (10월 12일 기록)");
            let h = history(c, sid)?;
            assert_eq!((h[0].field_label.as_str(), h[0].status.as_str()), ("비고", "applied"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn class_number_fills_empty_and_asks_when_taken() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let now = util::parse_dt("2026-10-09T09:00:00").unwrap();
            let mk = |name: &str, number: Option<i64>| students::create_student(c, &students::StudentInput { name: name.into(), class_label: Some("3-1".into()), number, memo: None }, now);
            let hong = mk("홍길동", None)?;
            let _other = mk("성춘향", Some(20))?;
            let note = crate::notes::create_note(c, "3-1 홍길동 번호는 21번", now)?;
            let item = |v: &str| ProfileItem {
                field: "number".into(),
                value: v.into(),
                relation: String::new(),
                student: PlanStudent { student_id: Some(hong), name: "홍길동".into(), class_label: Some("3-1".into()), number: None, candidates: vec![] },
                text: String::new(),
            };
            apply(c, note, &[item("21")], &[], "ai", now, now, &mut vec![])?;
            assert_eq!(students::get_student(c, hong)?.number, Some(21), "빈 번호는 바로 넣는다");
            let h = history(c, hong)?;
            assert_eq!((h[0].field_label.as_str(), h[0].status.as_str()), ("번호", "applied"));
            // 이미 번호가 있으면 확인을 기다린다
            let note2 = crate::notes::create_note(c, "3-1 홍길동 번호는 22번", now)?;
            apply(c, note2, &[item("22")], &[], "ai", now, now, &mut vec![])?;
            assert_eq!(students::get_student(c, hong)?.number, Some(21));
            let (pending, _) = for_note(c, note2)?;
            assert_eq!((pending[0].old_value.as_str(), pending[0].new_value.as_str(), pending[0].status.as_str()), ("21", "22", "pending"));
            resolve_change(c, pending[0].id, true, None, now)?;
            assert_eq!(students::get_student(c, hong)?.number, Some(22));
            // 같은 학급에 이미 그 번호인 학생이 있으면 비어 있어도 확인을 기다린다
            students::set_roster_field(c, hong, "number", "", now)?;
            let note3 = crate::notes::create_note(c, "3-1 홍길동 번호는 20번", now)?;
            apply(c, note3, &[item("20")], &[], "ai", now, now, &mut vec![])?;
            assert_eq!(students::get_student(c, hong)?.number, None);
            assert_eq!(for_note(c, note3)?.0[0].status, "pending");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn backfill_moves_old_ai_observations_once() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let now = util::parse_dt("2026-10-08T09:00:00").unwrap();
            let sid = students::create_student(c, &students::StudentInput { name: "홍길동".into(), class_label: Some("3-1".into()), number: None, memo: None }, now)?;
            let note = crate::notes::create_note(c, "3-1 홍길동 전화번호 010-1234-5678", now)?;
            let ins = |content: &str, source: &str| -> AppResult<()> {
                c.execute(
                    "INSERT INTO observations(note_id, student_id, area, content, observed_on, source, created_at, updated_at) VALUES (?1, ?2, 'none', ?3, '2026-10-06', ?4, 'x', 'x')",
                    params![note, sid, content, source],
                )?;
                Ok(())
            };
            ins("3-1 홍길동 전화번호 010-1234-5678", "ai")?;
            ins("3-1 홍길동 발표를 잘함", "ai")?;
            ins("홍길동 어머니 010-9876-5432 (교사가 직접 씀)", "manual")?;
            assert_eq!(backfill(c, now)?, 1);
            let s = students::get_student(c, sid)?;
            assert_eq!(s.phone, "010-1234-5678", "예전 AI 관찰의 번호가 명렬표로");
            let left: Vec<String> = c.prepare("SELECT content FROM observations ORDER BY id")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            assert_eq!(left, vec!["3-1 홍길동 발표를 잘함".to_string(), "홍길동 어머니 010-9876-5432 (교사가 직접 씀)".to_string()], "개인정보 관찰만 빠지고 직접 쓴 것은 그대로");
            // 두 번째부터는 하지 않는다
            ins("3-1 홍길동 주소 OO시 OO구 OO로 12", "ai")?;
            assert_eq!(backfill(c, now)?, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn special_and_scrub() {
        assert!(is_special("홍길동 수업 중 자주 어지러움 호소"));
        assert!(is_special("홍길동 땅콩 알레르기 있음"));
        assert!(!is_special("홍길동 감기로 결석"), "출결은 출결로");
        assert!(!is_special("홍길동 발표를 잘함"));
        let s = scrub("어머니 010-1234-5678, 주소 경기 OO시 OO동 12");
        assert!(s.contains("●●●-●●●●-5678") && !s.contains("1234-5678") && !s.contains("OO동 12"), "{s}");
    }
}
