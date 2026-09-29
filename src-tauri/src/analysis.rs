//! 규칙 기반 분석: 출결 사실, 학생 언급, 해야 할 행동, 과거 사실, 학생부 영역 단서, 담당업무 핵심어.
//!
//! 이 결과는 (1) AI 결과를 검증·보정하는 안전장치, (2) AI가 없을 때 수동 분류 화면의 제안으로 쓰인다.
//! 여기서 찾는 것은 모두 원문에 글자로 적힌 것뿐이다.

use crate::dates::{self, DateHit, TimeHit};
use crate::text::{self, Span};
use chrono::NaiveDate;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Attendance,
    Subject,
    Autonomous,
    Career,
    Volunteer,
    Club,
    Behavior,
    None,
}

impl Area {
    pub const ALL: [Area; 7] = [
        Area::Attendance,
        Area::Subject,
        Area::Autonomous,
        Area::Career,
        Area::Volunteer,
        Area::Club,
        Area::Behavior,
    ];
    pub fn as_str(&self) -> &'static str {
        match self {
            Area::Attendance => "attendance",
            Area::Subject => "subject",
            Area::Autonomous => "autonomous",
            Area::Career => "career",
            Area::Volunteer => "volunteer",
            Area::Club => "club",
            Area::Behavior => "behavior",
            Area::None => "none",
        }
    }
    pub fn parse(s: &str) -> Area {
        match s.trim() {
            "attendance" => Area::Attendance,
            "subject" => Area::Subject,
            "autonomous" => Area::Autonomous,
            "career" => Area::Career,
            "volunteer" => Area::Volunteer,
            "club" => Area::Club,
            "behavior" => Area::Behavior,
            _ => Area::None,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Area::Attendance => "출결특기사항",
            Area::Subject => "교과특기사항",
            Area::Autonomous => "창체 — 자율",
            Area::Career => "창체 — 진로",
            Area::Volunteer => "창체 — 봉사",
            Area::Club => "창체 — 동아리",
            Area::Behavior => "행동특성 및 종합의견",
            Area::None => "영역 미지정",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// 학생 관찰 기록
    Student,
    /// 할 일
    Task,
    /// 일정
    Event,
    /// 담당업무 기록
    Duty,
    /// 기타 메모
    Memo,
}

impl ItemKind {
    pub fn parse(s: &str) -> ItemKind {
        match s.trim() {
            "student" => ItemKind::Student,
            "task" => ItemKind::Task,
            "event" => ItemKind::Event,
            "duty" => ItemKind::Duty,
            _ => ItemKind::Memo,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RosterEntry {
    pub id: i64,
    pub name: String,
    pub class_label: Option<String>,
    pub number: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct DutyEntry {
    pub id: i64,
    pub name: String,
    pub detail: String,
    pub guide: String,
    pub keywords: Vec<String>,
}

impl DutyEntry {
    pub fn label(&self) -> String {
        if self.detail.trim().is_empty() {
            self.name.clone()
        } else {
            format!("{} / {}", self.name, self.detail)
        }
    }
    /// 분류에 쓰는 핵심어: 업무명, 세부업무(와 그 안의 3글자 이상 낱말), 교사가 적은 핵심어
    pub fn match_terms(&self) -> Vec<String> {
        const GENERIC: [&str; 16] = [
            "관리", "업무", "담당", "운영", "계획", "지원", "협조", "보고서", "계획서", "추진", "관련", "전반", "기타", "총괄", "업무지원", "행정",
        ];
        let mut v: Vec<String> = Vec::new();
        for t in [self.name.as_str(), self.detail.as_str()] {
            for part in t.split(['/', ',', '·', '(', ')']) {
                let p = part.trim();
                if p.chars().count() >= 2 && !GENERIC.contains(&p) {
                    v.push(p.to_string());
                }
                for w in p.split_whitespace() {
                    if w.chars().count() >= 3 && !GENERIC.contains(&w) {
                        v.push(w.to_string());
                    }
                }
            }
        }
        for k in &self.keywords {
            let k = k.trim();
            if k.chars().count() >= 2 {
                v.push(k.to_string());
            }
        }
        v.sort();
        v.dedup();
        v
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StudentMention {
    pub span: Span,
    pub name: String,
    pub class_label: Option<String>,
    pub number: Option<i64>,
    /// 명단에서 확정된 학생
    pub roster_id: Option<i64>,
    /// 동명이인 후보 (확정 못 함)
    pub candidates: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AttendanceKind {
    Absent,
    Late,
    EarlyLeave,
    ClassAbsence,
    Recognized,
}

#[derive(Debug, Clone)]
pub struct ClauseInfo {
    pub span: Span,
    pub sentence: usize,
    pub sentence_span: Span,
    pub dates: Vec<DateHit>,
    pub times: Vec<TimeHit>,
    pub attendance: Option<AttendanceKind>,
    pub in_class: bool,
    pub group_subject: bool,
    pub action: bool,
    pub obligation: bool,
    pub past: bool,
    pub area_hint: Option<Area>,
    pub students: Vec<usize>,
    pub duties: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct Analysis {
    pub text: String,
    pub today: NaiveDate,
    pub clauses: Vec<ClauseInfo>,
    pub sentences: Vec<Span>,
    pub students: Vec<StudentMention>,
    pub dates: Vec<DateHit>,
}

static RE_ATTENDANCE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(질병|미인정|무단|인정|기타|병)?\s*(결석|지각|조퇴|결과)|(출석\s*인정|교외\s*체험\s*학습|현장\s*체험\s*학습)").unwrap()
});
static RE_ATT_NEGATION: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:하지\s*않|하지\s*말|안\s*하|없|예방|방지|줄이|0\s*회|제로)").unwrap());
/// "결석계", "지각 확인서"처럼 출결 낱말이 서류 이름의 일부인 경우 (사실이 아니라 서류)
static RE_ATT_DOCUMENT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:계|신고서|확인서|증명서|서류|사유서|통계|현황|처리\s*기준|규정)").unwrap());
static RE_PERIOD_BEFORE: Lazy<Regex> = Lazy::new(|| Regex::new(r"교시\s*$").unwrap());

static RE_IN_CLASS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"수업|교시|과제|숙제|발표|모둠|수행\s*평가|쪽지\s*시험|필기|교과서|실험|실습|토론|학습지|활동지|국어|영어|수학|과학|사회\s*시간|역사|도덕|음악|미술|체육|기술|가정|정보\s*시간|한문|교과").unwrap()
});
static RE_GROUP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"학생들|아이들|반\s*전체|우리\s*반|몇몇|일부\s*학생|모둠|학급\s*전체|남학생들|여학생들").unwrap());

static RE_ACTION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"제출|연락|확인|보내|발송|전달|작성|준비|처리|신청|회신|알리|알려|안내|공지|점검|검토|결재|입력|등록|수합|취합|출력|인쇄|예약|챙기|챙겨|협의|요청|문의|구입|주문|배부|게시|업로드|올리|올려|채점|상담|면담|통화|전화|보고하|반납|수정|정리|받기|받아|받을|받으|수령|회수|걷기|걷어|수거|모으|모아|나눠|나누어|배포|복사|만들|만드|바꾸|바꿔|고치|고쳐|옮기|붙이|치우|닫기|끄기").unwrap()
});
static RE_OBLIGATION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"해야|하기\b|하기$|할\s*것|할것|하자|할\s*예정|예정|필요|하도록|부탁|요망|바람|하기로|할게|해\s*줘|해\s*주세요|드리기|받기|보내기|잊지\s*말").unwrap()
});
static RE_PAST: Lazy<Regex> = Lazy::new(|| {
    // '완료', '마감'은 "금요일까지 완료"처럼 앞으로 할 일에도 쓰이므로 과거 표지로 보지 않는다.
    // 개조식 끝맺음 "~옴(왔음)·~봄·~줌·~냄·~씀"도 이미 있었던 일이다 ("보호자 전화 옴").
    // 단, "봄 소풍", "줌 회의"와 헷갈리지 않도록 새 끝맺음은 절 끝에 올 때만 인정한다.
    Regex::new(r"(었|았|였|했|됐|렸|쳤|냈|줬|웠|혔|켰|셨|왔|봤|갔)(다|음|습니다|어요|고|으며|는데|지만|을|던)|(함|됨|했음|하였음|끝냄|끝남)(\s|[.,!?)]|$)|[가-힣]\s?(옴|봄|줌|냄|씀)\s*([.,!?)]|$)|받았|드렸|주었|하였").unwrap()
});

static AREA_RULES: Lazy<Vec<(Area, Regex)>> = Lazy::new(|| {
    vec![
        (Area::Club, Regex::new(r"동아리").unwrap()),
        (Area::Volunteer, Regex::new(r"봉사").unwrap()),
        (Area::Career, Regex::new(r"진로|장래|꿈|희망\s*직업|직업\s*체험|진학|적성|진로\s*탐색|진로\s*체험|진로\s*상담").unwrap()),
        (
            Area::Autonomous,
            Regex::new(r"자율|학급\s*회의|학생회|자치|임원|반장|부반장|회장|부회장|체육\s*대회|축제|캠페인|학급\s*활동|수련회|수학여행|현장\s*체험|학급\s*행사|1인\s*1역").unwrap(),
        ),
        (
            Area::Behavior,
            Regex::new(r"친구|교우|배려|태도|생활|갈등|다툼|싸움|규칙|성실|책임감|예의|칭찬|도움|리더십|욕설|장난|괴롭|사과|양보|협동|청소").unwrap(),
        ),
    ]
});

static RE_CLASS_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\d)\s*(?:-|–|학년\s*)\s*(\d{1,2})\s*반?\s*(?:(\d{1,2})\s*번\s*)?([가-힣]{2,6})").unwrap()
});

const NAME_STOPWORDS: [&str; 30] = [
    "학생", "전체", "모두", "담임", "교실", "친구", "아이", "학급", "교과", "수업", "남학생", "여학생", "학부모", "보호자",
    "부모", "어머니", "아버지", "선생", "교사", "반장", "부반장", "회장", "교시", "모둠", "일부", "몇몇", "우리", "대상", "명단", "전원",
];
const DOUBLE_SURNAMES: [&str; 8] = ["남궁", "황보", "제갈", "선우", "독고", "사공", "서문", "동방"];
const NAME_TAIL_WORDS: [&str; 12] = ["지각", "결석", "조퇴", "결과", "학생", "상담", "발표", "외출", "등교", "하교", "면담", "보건"];

fn char_len(s: &str) -> usize {
    s.chars().count()
}
fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
fn skip_chars(s: &str, n: usize) -> String {
    s.chars().skip(n).collect()
}

/// "김민수가", "김민수지각" 같은 토큰에서 이름만 남긴다.
pub fn clean_name(token: &str) -> Option<String> {
    let mut t = token.trim().to_string();
    if t.is_empty() {
        return None;
    }
    for multi in ["에게서", "에게", "한테", "께서", "이랑", "하고", "으로", "부터", "까지", "처럼", "보다", "이는", "이가", "이를", "이의"] {
        if t.ends_with(multi) && char_len(&t) - char_len(multi) >= 2 {
            t = take_chars(&t, char_len(&t) - char_len(multi));
            break;
        }
    }
    let n = char_len(&t);
    let is_double = DOUBLE_SURNAMES.iter().any(|d| t.starts_with(d));
    let full_len = if is_double { 4 } else { 3 };
    if n > full_len {
        let rest = skip_chars(&t, full_len);
        let singles = ['가', '이', '은', '는', '을', '를', '의', '와', '과', '도', '만', '랑', '께', '로'];
        if char_len(&rest) == 1 && rest.chars().next().is_some_and(|c| singles.contains(&c)) {
            t = take_chars(&t, full_len);
        } else if NAME_TAIL_WORDS.iter().any(|w| rest.starts_with(w)) || n >= full_len + 2 {
            t = take_chars(&t, full_len);
        }
    } else if n == 3 && !is_double {
        // 3글자 이름의 끝 글자가 흔히 쓰는 조사면 떼되, 이름에 흔한 '은', '이'는 그대로 둔다
        let last = t.chars().last().unwrap_or(' ');
        if ['가', '는', '를', '의', '도', '랑', '와'].contains(&last) && NAME_TAIL_WORDS.iter().all(|w| !t.contains(w)) {
            // "민수가"(이름 2글자 + 조사) 가능성: 성으로 시작하지 않으면 뗀다
            if !is_common_surname(t.chars().next().unwrap_or(' ')) {
                t = take_chars(&t, 2);
            }
        }
    }
    if NAME_STOPWORDS.iter().any(|w| t.starts_with(w)) {
        return None;
    }
    if char_len(&t) < 2 {
        return None;
    }
    Some(t)
}

/// 명단에 없는 이름을 새 학생으로 만들 만큼 사람 이름다운가 (성 + 이름 형태)
pub fn plausible_full_name(name: &str) -> bool {
    let n = char_len(name);
    if !name.chars().all(|c| ('가'..='힣').contains(&c)) {
        return false;
    }
    if DOUBLE_SURNAMES.iter().any(|d| name.starts_with(d)) {
        return (3..=4).contains(&n);
    }
    n == 3 && name.chars().next().is_some_and(is_common_surname)
}

pub fn is_common_surname(c: char) -> bool {
    "김이박최정강조윤장임한오서신권황안송류유전홍고문양손배백허남심노하곽성차주우구민진나지엄채원천방공현함변염여추도소석선설마길연위표명기반라왕금옥육인맹제모탁국어은편용예경봉사부".contains(c)
}

/// 원문 속 학생 언급을 찾는다.
pub fn find_students(text: &str, roster: &[RosterEntry]) -> Vec<StudentMention> {
    let mut out: Vec<StudentMention> = Vec::new();

    // 1) "2-1 김민수", "2학년 1반 3번 김민수" 형태
    for m in RE_CLASS_NAME.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let Some(name) = clean_name(&m[4]) else { continue };
        let grade: i64 = m[1].parse().unwrap_or(0);
        let class_no: i64 = m[2].parse().unwrap_or(0);
        if !(1..=6).contains(&grade) || !(1..=30).contains(&class_no) {
            continue;
        }
        let label = format!("{grade}-{class_no}");
        let number = m.get(3).and_then(|x| x.as_str().parse::<i64>().ok());
        let name_start = m.get(4).unwrap().start();
        let span = Span { start: whole.start(), end: name_start + name.len() };
        let matches: Vec<&RosterEntry> = roster
            .iter()
            .filter(|r| r.name == name && r.class_label.as_deref() == Some(label.as_str()))
            .filter(|r| number.is_none() || r.number.is_none() || r.number == number)
            .collect();
        let (roster_id, candidates) = match matches.len() {
            1 => (Some(matches[0].id), vec![]),
            0 => (None, vec![]),
            _ => (None, matches.iter().map(|r| r.id).collect()),
        };
        // 명단에 없으면 "성 + 이름" 형태일 때만 학생으로 본다 ("2-1 체육대회" 같은 오인 방지)
        if roster_id.is_none() && candidates.is_empty() && !plausible_full_name(&name) {
            continue;
        }
        out.push(StudentMention { span, name, class_label: Some(label), number, roster_id, candidates });
    }

    // 2) 명단에 있는 이름 (전체 이름)
    let mut by_name: std::collections::BTreeMap<&str, Vec<&RosterEntry>> = Default::default();
    for r in roster {
        by_name.entry(r.name.as_str()).or_default().push(r);
    }
    for (name, entries) in &by_name {
        if char_len(name) < 2 {
            continue;
        }
        for (pos, _) in text.match_indices(name) {
            let span = Span { start: pos, end: pos + name.len() };
            if out.iter().any(|o| o.span.overlaps(&span)) {
                continue;
            }
            // 앞 글자가 한글이면 다른 낱말의 일부 ("박김민수" 같은 경우)로 보고 건너뛴다
            if text[..pos].chars().next_back().is_some_and(|c| ('가'..='힣').contains(&c)) {
                continue;
            }
            let (roster_id, candidates) = if entries.len() == 1 {
                (Some(entries[0].id), vec![])
            } else {
                (None, entries.iter().map(|r| r.id).collect())
            };
            out.push(StudentMention {
                span,
                name: name.to_string(),
                class_label: if entries.len() == 1 { entries[0].class_label.clone() } else { None },
                number: None,
                roster_id,
                candidates,
            });
        }
    }

    // 3) 성을 뺀 이름 ("민수가 발표함") — 명단에서 하나로 정해질 때만
    let mut given: std::collections::BTreeMap<String, Vec<&RosterEntry>> = Default::default();
    for r in roster {
        let n = char_len(&r.name);
        let skip = if DOUBLE_SURNAMES.iter().any(|d| r.name.starts_with(d)) { 2 } else { 1 };
        if n >= 3 && n - skip >= 2 {
            given.entry(skip_chars(&r.name, skip)).or_default().push(r);
        }
    }
    for (g, entries) in &given {
        for (pos, _) in text.match_indices(g.as_str()) {
            let span = Span { start: pos, end: pos + g.len() };
            if out.iter().any(|o| o.span.overlaps(&span)) {
                continue;
            }
            if text[..pos].chars().next_back().is_some_and(|c| ('가'..='힣').contains(&c)) {
                continue;
            }
            let (roster_id, candidates) = if entries.len() == 1 {
                (Some(entries[0].id), vec![])
            } else {
                (None, entries.iter().map(|r| r.id).collect())
            };
            out.push(StudentMention {
                span,
                name: if entries.len() == 1 { entries[0].name.clone() } else { g.clone() },
                class_label: if entries.len() == 1 { entries[0].class_label.clone() } else { None },
                number: None,
                roster_id,
                candidates,
            });
        }
    }
    out.sort_by_key(|m| m.span.start);
    out
}

/// 절 안에 실제 출결 사실이 적혀 있는가
pub fn attendance_fact(clause: &str) -> Option<AttendanceKind> {
    for m in RE_ATTENDANCE.captures_iter(clause) {
        let whole = m.get(0).unwrap();
        let after = &clause[whole.end()..];
        if RE_ATT_NEGATION.is_match(after) || RE_ATT_DOCUMENT.is_match(after) {
            continue;
        }
        if m.get(3).is_some() {
            return Some(AttendanceKind::Recognized);
        }
        let qualifier = m.get(1).map(|x| x.as_str());
        let kind = match &m[2] {
            "결석" => AttendanceKind::Absent,
            "지각" => AttendanceKind::Late,
            "조퇴" => AttendanceKind::EarlyLeave,
            _ => {
                // '결과'는 '결과 보고'처럼 일반 낱말일 때가 많다. 교시·출결 수식어가 있을 때만 출결로 본다.
                let before = &clause[..whole.start()];
                let is_att = qualifier.is_some()
                    || RE_PERIOD_BEFORE.is_match(before)
                    || after.trim_start().starts_with("처리")
                    || after.trim_start().starts_with('(');
                if !is_att {
                    continue;
                }
                AttendanceKind::ClassAbsence
            }
        };
        return Some(kind);
    }
    None
}

pub fn has_attendance_fact(s: &str) -> bool {
    attendance_fact(s).is_some()
}

pub fn is_in_class(s: &str) -> bool {
    RE_IN_CLASS.is_match(s)
}

/// 행동어를 품은 행사 이름 ("교과협의회"의 '협의', "학부모 상담 주간"의 '상담')은 할 일이 아니라 일정이다
static RE_EVENT_NOUN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"협의회|설명회|연수회|발표회|간담회|토론회|보고회|평가회|위원회|총회|상담\s*주간|확인\s*주간|시상식|입학식|졸업식|개학식|방학식|체육\s*대회|축제").unwrap()
});

pub fn has_action(s: &str) -> bool {
    let stripped = RE_EVENT_NOUN.replace_all(s, " ");
    RE_ACTION.is_match(&stripped)
}

/// "~기"로 끝나는 할 일 메모 ("당번표 다시 만들기", "결석계 받기"). "학기·일기·분기" 같은 명사는 제외.
fn ends_with_todo_ki(s: &str) -> bool {
    const NOUNS: [&str; 18] = ["학기", "일기", "분기", "전기", "인기", "용기", "공기", "시기", "무기", "경기", "동기", "초기", "말기", "후기", "중기", "주기", "상기", "여기"];
    let t = s.trim().trim_end_matches(['.', '!', '。', ')']).trim();
    let Some(last) = t.split_whitespace().last() else { return false };
    last.ends_with('기') && last.chars().count() >= 2 && !NOUNS.iter().any(|n| last.ends_with(n))
}

pub fn has_obligation(s: &str) -> bool {
    RE_OBLIGATION.is_match(s) || ends_with_todo_ki(s)
}

pub fn is_past(s: &str) -> bool {
    RE_PAST.is_match(s)
}

/// 원문에 그 영역을 뒷받침하는 낱말이 있는가
pub fn area_evidence(area: Area, s: &str) -> bool {
    match area {
        Area::Attendance => has_attendance_fact(s),
        Area::Subject => is_in_class(s),
        Area::None => false,
        other => AREA_RULES.iter().any(|(a, re)| *a == other && re.is_match(s)),
    }
}

/// 원문 단서로 본 학생부 영역 (출결은 사실이 있을 때만)
pub fn area_hint(s: &str) -> Option<Area> {
    if has_attendance_fact(s) {
        return Some(Area::Attendance);
    }
    for (area, re) in AREA_RULES.iter() {
        if matches!(area, Area::Behavior) && is_in_class(s) {
            return Some(Area::Subject);
        }
        if re.is_match(s) {
            return Some(*area);
        }
    }
    if is_in_class(s) {
        return Some(Area::Subject);
    }
    None
}

pub fn match_duties(s: &str, duties: &[DutyEntry]) -> Vec<i64> {
    let mut out = Vec::new();
    for d in duties {
        if d.match_terms().iter().any(|t| s.contains(t.as_str())) {
            out.push(d.id);
        }
    }
    out
}

pub fn analyze(text: &str, today: NaiveDate, roster: &[RosterEntry], duties: &[DutyEntry]) -> Analysis {
    let sentences = text::sentences(text);
    let students = find_students(text, roster);
    let dates = dates::find_dates(text, today);
    let times = dates::find_times(text);
    let mut clauses = Vec::new();
    for c in text::clauses(text) {
        let s = c.span.slice(text);
        let sentence_span = sentences.get(c.sentence).copied().unwrap_or(c.span);
        let cd: Vec<DateHit> = dates.iter().filter(|d| c.span.contains(d.start)).cloned().collect();
        let ct: Vec<TimeHit> = times.iter().filter(|t| c.span.contains(t.start)).cloned().collect();
        let action = has_action(s);
        let obligation = has_obligation(s) || cd.iter().any(|d| d.deadline);
        let past = is_past(s);
        clauses.push(ClauseInfo {
            span: c.span,
            sentence: c.sentence,
            sentence_span,
            dates: cd,
            times: ct,
            attendance: attendance_fact(s),
            in_class: is_in_class(s) || is_in_class(sentence_span.slice(text)),
            group_subject: RE_GROUP.is_match(s),
            action,
            obligation,
            past,
            area_hint: area_hint(s).or_else(|| area_hint(sentence_span.slice(text))),
            students: students
                .iter()
                .enumerate()
                .filter(|(_, m)| c.span.contains(m.span.start))
                .map(|(i, _)| i)
                .collect(),
            duties: match_duties(s, duties),
        });
    }
    Analysis { text: text.to_string(), today, clauses, sentences, students, dates }
}

impl Analysis {
    /// 절에 붙는 날짜: 절 안의 날짜, 없으면 같은 문장에서 앞에 나온 날짜 (한국어 부사어는 뒤의 서술어를 꾸민다)
    pub fn clause_date(&self, idx: usize) -> Option<DateHit> {
        let c = self.clauses.get(idx)?;
        if let Some(d) = c.dates.first() {
            return Some(d.clone());
        }
        self.clauses[..idx]
            .iter()
            .rev()
            .take_while(|p| p.sentence == c.sentence)
            .find_map(|p| p.dates.last().cloned())
    }

    pub fn clause_text(&self, idx: usize) -> &str {
        self.clauses[idx].span.slice(&self.text)
    }

    /// 할 일로 볼 수 있는 절인가: 행동어 또는 기한이 있고, 이미 한 일이 아님
    pub fn clause_is_task(&self, idx: usize) -> bool {
        let c = &self.clauses[idx];
        if c.past || c.attendance.is_some() {
            return false;
        }
        let has_deadline = c.dates.iter().any(|d| d.deadline);
        // "~까지"가 붙은 기한은 그 자체로 해야 할 일이다 ("금요일까지 결석계")
        if has_deadline || (c.action && c.obligation) {
            return true;
        }
        // 행동어 없이 의무 표현만 있어도 앞으로의 날짜가 있으면 할 일 ("다음 주 월요일 결석계 받기")
        if c.obligation && self.clause_date(idx).is_some_and(|d| d.date >= self.today) {
            return true;
        }
        if c.action {
            // 기한 있는 할 일과 "~하고"로 이어진 행동 ("담임에게 알리고 금요일까지 보호자 연락")
            let text = self.clause_text(idx);
            let chained = text.ends_with('고') || text.ends_with("하며");
            let next_is_task = self
                .clauses
                .get(idx + 1)
                .is_some_and(|n| n.sentence == c.sentence && n.action && !n.past && (n.obligation || !n.dates.is_empty()));
            if chained && next_is_task {
                return true;
            }
            // 앞으로의 날짜가 붙은 행동
            if self.clause_date(idx).is_some_and(|d| d.date >= self.today) {
                return true;
            }
        }
        false
    }

    /// 일정으로 볼 수 있는 절인가: 오늘 이후 날짜가 있고, 행동·출결·과거 사실이 아님
    pub fn clause_is_event(&self, idx: usize) -> bool {
        let c = &self.clauses[idx];
        if c.past || c.attendance.is_some() || c.action || !c.students.is_empty() {
            return false;
        }
        c.dates.iter().any(|d| d.date >= self.today && !d.deadline)
    }
}

static RE_TRAILING_GO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(하|알리|보내|받|드리|전하|챙기|올리)고$").unwrap());

/// 할 일·일정 제목: 날짜·시각 표현과 끝 문장부호를 떼고 "~하고"는 "~하기"로 바꾼다.
/// 원문 낱말만 사용한다.
pub fn title_from(text: &str, remove: &[(usize, usize)]) -> String {
    let mut ranges: Vec<(usize, usize)> = remove.to_vec();
    ranges.sort();
    let mut out = String::new();
    let mut pos = 0usize;
    for (a, b) in ranges {
        if a < pos || b > text.len() || a > b {
            continue;
        }
        out.push_str(&text[pos..a]);
        out.push(' ');
        pos = b;
    }
    out.push_str(&text[pos..]);
    let mut t = text::tidy(&out);
    // 날짜를 떼고 남은 조사 ("금요일에 회의" → "회의")
    for p in ["에 ", "에,", "까지 ", "부터 "] {
        if let Some(rest) = t.strip_prefix(p) {
            t = rest.trim().to_string();
        }
    }
    t = t.trim_start_matches([',', ' ']).trim().to_string();
    if let Some(m) = RE_TRAILING_GO.find(&t) {
        let stem = &t[m.start()..m.end() - '고'.len_utf8()];
        t = format!("{}{}기", &t[..m.start()], stem);
    }
    if t.chars().count() < 2 {
        return text::tidy(text);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()
    }

    #[test]
    fn attendance_requires_real_fact() {
        assert_eq!(attendance_fact("3교시 김민수 지각"), Some(AttendanceKind::Late));
        assert_eq!(attendance_fact("질병 결석 처리"), Some(AttendanceKind::Absent));
        assert_eq!(attendance_fact("5교시 결과"), Some(AttendanceKind::ClassAbsence));
        assert_eq!(attendance_fact("교외체험학습 신청서 받음"), Some(AttendanceKind::Recognized));
        assert_eq!(attendance_fact("다음 주 월요일까지 결석계 받기"), None);
        assert_eq!(attendance_fact("지각 확인서 제출"), None);
        assert_eq!(attendance_fact("이번 달 결석 현황 정리"), None);
        assert_eq!(attendance_fact("학생들이 수업 중 손장난을 했고 지적을 받았습니다."), None);
        assert_eq!(attendance_fact("평가 결과 보고 준비"), None);
        assert_eq!(attendance_fact("지각하지 않도록 지도함"), None);
        assert_eq!(attendance_fact("이번 달 지각 없음"), None);
        assert_eq!(attendance_fact("지각 예방 캠페인"), None);
    }

    #[test]
    fn names_with_class() {
        let m = find_students("오늘 3교시 2-1 김민수 지각.", &[]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "김민수");
        assert_eq!(m[0].class_label.as_deref(), Some("2-1"));
        let m = find_students("2학년 3반 12번 이서연은 발표를 잘함", &[]);
        assert_eq!(m[0].name, "이서연");
        assert_eq!(m[0].class_label.as_deref(), Some("2-3"));
        assert_eq!(m[0].number, Some(12));
        let m = find_students("2-1 김민수지각", &[]);
        assert_eq!(m[0].name, "김민수");
        let m = find_students("2-1 학생들 전체 지도", &[]);
        assert!(m.is_empty());
    }

    #[test]
    fn roster_and_homonyms() {
        let roster = vec![
            RosterEntry { id: 1, name: "김민수".into(), class_label: Some("2-1".into()), number: Some(3) },
            RosterEntry { id: 2, name: "김민수".into(), class_label: Some("2-3".into()), number: Some(5) },
            RosterEntry { id: 3, name: "이서연".into(), class_label: Some("2-1".into()), number: Some(7) },
        ];
        let m = find_students("2-1 김민수 지각", &roster);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].roster_id, Some(1));
        let m = find_students("김민수 발표 우수", &roster);
        assert_eq!(m[0].roster_id, None);
        assert_eq!(m[0].candidates, vec![1, 2]);
        let m = find_students("서연이가 친구를 도와줌", &roster);
        assert_eq!(m[0].roster_id, Some(3));
    }

    #[test]
    fn tasks_and_events() {
        let t = "오늘 3교시 2-1 김민수 지각. 담임에게 알리고 금요일까지 보호자 연락.";
        let a = analyze(t, today(), &[], &[]);
        assert_eq!(a.clauses.len(), 3);
        assert!(!a.clause_is_task(0));
        assert!(a.clause_is_task(1), "담임에게 알리고");
        assert!(a.clause_is_task(2), "금요일까지 보호자 연락");
        assert_eq!(a.clause_date(2).unwrap().date, NaiveDate::from_ymd_opt(2026, 10, 2).unwrap());
        // 날짜가 뒤에 나오므로 "담임에게 알리고"에는 기한을 붙이지 않는다
        assert!(a.clause_date(1).is_none());

        let a = analyze("10월 15일 교육장 공감소통 방문", today(), &[], &[]);
        assert!(a.clause_is_event(0));
        assert!(!a.clause_is_task(0));

        // 행사 이름 속 행동어("협의회"의 협의)는 할 일로 보지 않는다
        let a = analyze("다음 주 수요일 오후 3시 교과협의회", today(), &[], &[]);
        assert!(a.clause_is_event(0));
        assert!(!a.clause_is_task(0));
        let a = analyze("교과협의회 자료 금요일까지 제출", today(), &[], &[]);
        assert!(a.clause_is_task(0));

        let a = analyze("학생들이 수업 중 손장난을 했고 지적을 받았습니다.", today(), &[], &[]);
        assert!(a.clauses.iter().all(|c| c.attendance.is_none()));
        assert!((0..a.clauses.len()).all(|i| !a.clause_is_task(i)));
        assert_eq!(a.clauses[0].area_hint, Some(Area::Subject));
    }

    #[test]
    fn nominal_past_endings() {
        assert!(is_past("보호자 전화 옴."));
        assert!(is_past("결과를 알려 줌"));
        assert!(is_past("학부모가 다녀왔음"));
        assert!(!is_past("내년 봄 소풍 계획서 제출하기"));
        assert!(!is_past("줌 회의 링크 보내기"));
        let a = analyze("청소 당번표 다시 만들기.", today(), &[], &[]);
        assert!(a.clause_is_task(0));
    }

    #[test]
    fn past_facts_are_not_tasks() {
        let a = analyze("학부모와 통화함. 상담 결과 확인 완료", today(), &[], &[]);
        assert!((0..a.clauses.len()).all(|i| !a.clause_is_task(i)));
        let a = analyze("내일 회의 자료 준비", today(), &[], &[]);
        assert!(a.clause_is_task(0));
    }

    #[test]
    fn duty_keywords() {
        let duties = vec![DutyEntry {
            id: 7,
            name: "교무기획".into(),
            detail: "학사일정 관리".into(),
            guide: String::new(),
            keywords: vec!["학사력".into()],
        }];
        assert_eq!(match_duties("2학기 학사일정 관리 회의", &duties), vec![7]);
        assert_eq!(match_duties("학사력 수정", &duties), vec![7]);
        assert!(match_duties("김민수 지각", &duties).is_empty());
    }

    #[test]
    fn titles() {
        let t = "금요일까지 보호자 연락.";
        let hits = dates::find_dates(t, today());
        assert_eq!(title_from(t, &[(hits[0].start, hits[0].end)]), "보호자 연락");
        assert_eq!(title_from("담임에게 알리고", &[]), "담임에게 알리기");
        let t = "10월 15일 교육장 공감소통 방문";
        let hits = dates::find_dates(t, today());
        assert_eq!(title_from(t, &[(hits[0].start, hits[0].end)]), "교육장 공감소통 방문");
    }

    #[test]
    fn area_hints() {
        assert_eq!(area_hint("동아리 시간에 로봇 제작"), Some(Area::Club));
        assert_eq!(area_hint("진로 체험 보고서를 성실히 씀"), Some(Area::Career));
        assert_eq!(area_hint("친구를 배려함"), Some(Area::Behavior));
        assert_eq!(area_hint("수업 중 장난"), Some(Area::Subject));
        assert_eq!(area_hint("오늘 날씨 맑음"), None);
    }
}
