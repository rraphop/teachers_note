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
    /// 자유학기활동 (진로탐색·주제선택·예술체육·동아리)
    FreeSemester,
    /// 일상생활 활동상황 (특수교육 기본 교육과정)
    Daily,
    None,
}

impl Area {
    /// 학생부 자료 화면의 영역 7개 (수정목록 순서: 창체 자율·자치, 교과, 행특, 동아리, 진로, 자유학기, 일상생활)
    pub const RECORD_TABS: [Area; 7] = [
        Area::Autonomous,
        Area::Subject,
        Area::Behavior,
        Area::Club,
        Area::Career,
        Area::FreeSemester,
        Area::Daily,
    ];
    /// 관찰 기록이 가질 수 있는 모든 영역 (학생부 영역 7개 + 출결·봉사)
    pub const ALL: [Area; 9] = [
        Area::Attendance,
        Area::Subject,
        Area::Autonomous,
        Area::Career,
        Area::Volunteer,
        Area::Club,
        Area::Behavior,
        Area::FreeSemester,
        Area::Daily,
    ];
    /// 2026학년도 중학교 학교생활기록부 기재요령 '영역별 입력 가능 최대 글자수' (한글 기준, 1자 = 3바이트)
    pub fn char_limit(&self) -> Option<usize> {
        match self {
            Area::Autonomous | Area::Club | Area::Career | Area::Subject | Area::Attendance => Some(500),
            Area::Behavior => Some(300),
            Area::FreeSemester | Area::Daily => Some(1000),
            Area::Volunteer | Area::None => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Area::Attendance => "attendance",
            Area::Subject => "subject",
            Area::Autonomous => "autonomous",
            Area::Career => "career",
            Area::Volunteer => "volunteer",
            Area::Club => "club",
            Area::Behavior => "behavior",
            Area::FreeSemester => "free_semester",
            Area::Daily => "daily",
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
            "free_semester" => Area::FreeSemester,
            "daily" => Area::Daily,
            _ => Area::None,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Area::Attendance => "출결특기사항",
            Area::Subject => "교과특기사항",
            Area::Autonomous => "창체 — 자율·자치",
            Area::Career => "창체 — 진로",
            Area::Volunteer => "창체 — 봉사",
            Area::Club => "창체 — 동아리",
            Area::Behavior => "행동특성 및 종합의견",
            Area::FreeSemester => "자유학기활동",
            Area::Daily => "일상생활 활동상황",
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
        match self.detail.split([',', '\n', ';']).map(str::trim).find(|s| !s.is_empty()) {
            Some(first) if first.chars().count() <= 20 => format!("{} / {}", self.name, first),
            _ => self.name.clone(),
        }
    }
    /// AI 없이도 쓰는 분류 낱말: 업무명과, 세부업무를 쉼표로 나눈 짧은 낱말·구(12글자 이하)와 그 안의 3글자 이상 낱말.
    /// 긴 설명 문장은 AI가 읽고 판단한다 (문장 속 흔한 낱말로 잘못 분류하지 않도록 규칙에는 쓰지 않는다).
    pub fn match_terms(&self) -> Vec<String> {
        const GENERIC: [&str; 18] = [
            "관리", "업무", "담당", "운영", "계획", "지원", "협조", "보고서", "계획서", "추진", "관련", "전반", "기타", "총괄", "업무지원", "행정", "교육청", "학교",
        ];
        let mut v: Vec<String> = Vec::new();
        for t in [self.name.as_str(), self.detail.as_str()] {
            for part in t.split(['/', ',', '·', '(', ')', '\n', ';', '，']) {
                let p = part.trim().trim_end_matches(['.', '。']);
                if p.chars().count() > 12 {
                    continue;
                }
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
    /// 원문에 적은 학급이 명렬표와 달라 명렬표를 따랐을 때, 원문에 적은 학급 (교사 확인용)
    pub written_class: Option<String>,
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
    // 개조식 끝맺음 "~옴(왔음)·~봄·~줌·~냄·~씀"도 이미 있었던 일이다 ("보호자 전화 옴"). "수업중 잠 → 지도함"의 '잠'도 그렇다.
    // 단, "봄 소풍", "줌 회의"와 헷갈리지 않도록 새 끝맺음은 절 끝에 올 때만 인정한다.
    Regex::new(r"(었|았|였|했|됐|렸|쳤|냈|줬|웠|혔|켰|셨|왔|봤|갔)(다|음|습니다|어요|고|으며|는데|지만|을|던)|(함|됨|했음|하였음|끝냄|끝남)(\s|[.,!?)]|$)|[가-힣]\s?(옴|봄|줌|냄|씀)\s*([.,!?)→]|$)|(?:^|\s)잠\s*(?:[.,!?)→]|$)|받았|드렸|주었|하였").unwrap()
});

static AREA_RULES: Lazy<Vec<(Area, Regex)>> = Lazy::new(|| {
    vec![
        (Area::FreeSemester, Regex::new(r"자유학기|주제\s*선택|진로\s*탐색\s*활동|예술\s*[·ㆍ]?\s*체육\s*활동").unwrap()),
        (Area::Daily, Regex::new(r"일상\s*생활").unwrap()),
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

/// 학년·학급 표기: "2-1", "2학년 1반", "2학년1반", "2학년", "2년" (+ "3번")
static RE_CLASS_CTX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\d)\s*(?:[-–]\s*(\d{1,2})\s*반?|학년\s*(?:(\d{1,2})\s*반)?|년\s*(?:(\d{1,2})\s*반)?)\s*(?:(\d{1,2})\s*번)?").unwrap()
});
static RE_NAME_TOKEN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[가-힣]{2,8}").unwrap());

/// 이름 뒤에 붙는 조사 (긴 것부터)
const NAME_PARTICLES: [&str; 27] = [
    "에게서", "에게", "한테", "께서", "이랑", "이가", "이는", "이를", "이의", "이도", "이와", "하고", "랑", "가", "이", "은", "는", "을", "를", "의",
    "와", "과", "도", "만", "께", "에", "로",
];

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

/// "홍길동가", "홍길동지각" 같은 토큰에서 이름만 남긴다.
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
            // "길동이가"(이름 2글자 + 조사) 가능성: 성으로 시작하지 않으면 뗀다
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

fn is_hangul(c: char) -> bool {
    ('가'..='힣').contains(&c)
}

/// 이름이 여기서 끝나는가. 두 글자 이름은 뒤가 공백·문장부호·조사(+공백)로 끊길 때만 이름으로 본다
/// ("가나다라" 속의 "가나"를 이름으로 보지 않도록). 세 글자 이상은 그대로 인정한다 ("홍길동지각").
fn name_ends_here(text: &str, end: usize, name_len: usize) -> bool {
    let rest = &text[end..];
    let Some(next) = rest.chars().next() else { return true };
    if !is_hangul(next) || name_len >= 3 {
        return true;
    }
    NAME_PARTICLES.iter().any(|p| rest.strip_prefix(p).is_some_and(|after| after.chars().next().is_none_or(|c| !is_hangul(c))))
}

/// 학년 → 학급: 그 학년의 학급이 하나뿐이면 그 학급 ("1학년" = 1-1)
fn grade_to_class(grade: i64, roster: &[RosterEntry]) -> Option<String> {
    let labels: std::collections::BTreeSet<&str> = roster
        .iter()
        .filter_map(|r| r.class_label.as_deref())
        .filter(|l| l.split('-').next().and_then(|g| g.parse::<i64>().ok()) == Some(grade))
        .collect();
    if labels.len() == 1 { labels.into_iter().next().map(String::from) } else { None }
}

fn label_grade(label: &str) -> Option<i64> {
    label.split('-').next()?.parse().ok()
}

/// 명단에서 이 자리에 시작하는 이름 (긴 이름 먼저)
fn roster_name_at<'a>(text: &str, pos: usize, names: &[&'a str]) -> Option<&'a str> {
    names.iter().copied().find(|n| text[pos..].starts_with(n) && name_ends_here(text, pos + n.len(), char_len(n)))
}

/// 학급 표기 뒤의 이름을 명단과 맞춘다. 적은 학급에 없고 다른 학급에 한 명뿐이면 명렬표를 따르고 적은 학급을 남긴다.
fn mention_in_class(
    name: &str,
    span: Span,
    label: Option<&str>,
    grade: i64,
    number: Option<i64>,
    roster: &[RosterEntry],
) -> Option<StudentMention> {
    let same_name: Vec<&RosterEntry> = roster.iter().filter(|r| r.name == name).collect();
    // 번호가 맞는 학생이 있으면 그 학생들만
    fn by_number(v: Vec<&RosterEntry>, number: Option<i64>) -> Vec<&RosterEntry> {
        let f: Vec<&RosterEntry> = v.iter().copied().filter(|r| number.is_none() || r.number.is_none() || r.number == number).collect();
        if f.is_empty() { v } else { f }
    }
    let in_ctx: Vec<&RosterEntry> = same_name
        .iter()
        .copied()
        .filter(|r| match label {
            Some(l) => r.class_label.as_deref() == Some(l),
            None => r.class_label.as_deref().and_then(label_grade) == Some(grade),
        })
        .collect();
    let in_ctx = by_number(in_ctx, number);
    let mk = |roster_id, candidates, class_label: Option<String>, written: Option<String>| StudentMention {
        span,
        name: name.to_string(),
        class_label,
        number,
        roster_id,
        candidates,
        written_class: written,
    };
    match in_ctx.len() {
        1 => return Some(mk(Some(in_ctx[0].id), vec![], in_ctx[0].class_label.clone(), None)),
        n if n > 1 => return Some(mk(None, in_ctx.iter().map(|r| r.id).collect(), label.map(String::from), None)),
        _ => {}
    }
    // 적은 학급에는 없지만 명렬표의 다른 학급에 있는 학생
    match same_name.len() {
        1 => {
            let r = same_name[0];
            let written = label.map(String::from).unwrap_or_else(|| format!("{grade}학년"));
            Some(mk(Some(r.id), vec![], r.class_label.clone(), Some(written)))
        }
        n if n > 1 => Some(mk(None, same_name.iter().map(|r| r.id).collect(), None, None)),
        // 명단에 없으면 "성 + 이름" 형태일 때만 학생으로 본다 ("2-1 체육대회" 같은 오인 방지)
        _ if plausible_full_name(name) => Some(mk(None, vec![], label.map(String::from), None)),
        _ => None,
    }
}

/// 원문 속 학생 언급을 찾는다.
/// 1) 학급 표기 뒤의 이름들 ("2-1 홍길동", "1학년 홍길동, 김철수", "1년 가나") — 쉼표로 나열한 이름 모두에 학급을 붙인다
/// 2) 명단에 있는 전체 이름 (학년이 없어도 명렬표에서 학급을 찾는다)
/// 3) 성을 뺀 이름 ("길동이가 발표함") — 명단에서 하나로 정해질 때만
pub fn find_students(text: &str, roster: &[RosterEntry]) -> Vec<StudentMention> {
    let mut out: Vec<StudentMention> = Vec::new();
    let mut names: Vec<&str> = roster.iter().map(|r| r.name.as_str()).filter(|n| char_len(n) >= 2).collect();
    names.sort_by(|a, b| char_len(b).cmp(&char_len(a)).then(a.cmp(b)));
    names.dedup();

    // 1) 학급 표기 뒤의 이름
    for m in RE_CLASS_CTX.captures_iter(text) {
        let whole = m.get(0).unwrap();
        // "2026학년도", "10-2"(날짜) 같은 경우는 학급 표기가 아니다
        if text[..whole.start()].chars().next_back().is_some_and(|c| c.is_ascii_digit() || c == '.' || c == '/')
            || text[whole.end()..].starts_with('도')
        {
            continue;
        }
        let grade: i64 = m[1].parse().unwrap_or(0);
        let class_no: Option<i64> = m.get(2).or(m.get(3)).or(m.get(4)).and_then(|x| x.as_str().parse().ok());
        if !(1..=6).contains(&grade) || class_no.is_some_and(|c| !(1..=30).contains(&c)) {
            continue;
        }
        let number = m.get(5).and_then(|x| x.as_str().parse::<i64>().ok());
        let label = match class_no {
            Some(c) => Some(format!("{grade}-{c}")),
            None => grade_to_class(grade, roster),
        };
        let mut pos = whole.end();
        let mut first = true;
        let mut after_comma = false;
        loop {
            let at = pos + (text[pos..].len() - text[pos..].trim_start_matches([' ', '\t']).len());
            // 명단의 이름 → 명단에 없는 사람 이름 순으로
            let (name, name_end, token_end) = if let Some(n) = roster_name_at(text, at, &names) {
                let mut te = at + n.len();
                if let Some(p) = NAME_PARTICLES.iter().find(|p| text[te..].starts_with(*p)) {
                    te += p.len();
                }
                (n.to_string(), at + n.len(), te)
            } else if let Some(tok) = RE_NAME_TOKEN.find(&text[at..]).filter(|_| first || after_comma) {
                // 명단에 없는 이름은 학급 바로 뒤나 쉼표 뒤에서만 받는다 ("1학년 홍길동 지도함"의 '지도함'을 이름으로 보지 않도록)
                let Some(n) = clean_name(tok.as_str()) else { break };
                if !tok.as_str().starts_with(n.as_str()) || !plausible_full_name(&n) {
                    break;
                }
                (n.clone(), at + n.len(), at + tok.end())
            } else {
                break;
            };
            let span = Span { start: if first { whole.start() } else { at }, end: name_end };
            if out.iter().any(|o| o.span.overlaps(&span)) {
                break;
            }
            let Some(mention) = mention_in_class(&name, span, label.as_deref(), grade, if first { number } else { None }, roster) else {
                break;
            };
            out.push(mention);
            first = false;
            // 다음 이름: 쉼표·가운뎃점 뒤, 또는 공백 뒤 (붙어 있으면 끝)
            let r = &text[token_end..];
            let r2 = r.trim_start_matches([' ', '\t']);
            if let Some(after) = r2.strip_prefix([',', '，', '·', '、']) {
                pos = text.len() - after.len();
                after_comma = true;
            } else if r2.len() < r.len() {
                pos = token_end;
                after_comma = false;
            } else {
                break;
            }
        }
    }
    // 1-2) 학급과 함께 처음 적은 명단 밖 학생을 같은 기록에서 이름만으로 다시 부른 경우 ("1-1 전우치 …. 전우치 …")
    let introduced: Vec<(String, Option<String>)> = {
        let mut v: Vec<(String, Option<String>)> = Vec::new();
        for m in out.iter().filter(|m| m.roster_id.is_none() && m.candidates.is_empty() && m.class_label.is_some()) {
            if !v.iter().any(|(n, _)| *n == m.name) && !names.contains(&m.name.as_str()) {
                v.push((m.name.clone(), m.class_label.clone()));
            }
        }
        v
    };
    for (name, label) in &introduced {
        for (pos, _) in text.match_indices(name.as_str()) {
            let span = Span { start: pos, end: pos + name.len() };
            if out.iter().any(|o| o.span.overlaps(&span))
                || text[..pos].chars().next_back().is_some_and(is_hangul)
                || !name_ends_here(text, span.end, char_len(name))
            {
                continue;
            }
            out.push(StudentMention {
                span,
                name: name.clone(),
                class_label: label.clone(),
                number: None,
                roster_id: None,
                candidates: vec![],
                written_class: None,
            });
        }
    }

    // 2) 명단에 있는 이름 (긴 이름 먼저, 두 글자 이름은 앞뒤가 끊길 때만)
    let mut by_name: std::collections::BTreeMap<&str, Vec<&RosterEntry>> = Default::default();
    for r in roster.iter().filter(|r| char_len(&r.name) >= 2) {
        by_name.entry(r.name.as_str()).or_default().push(r);
    }
    for name in &names {
        let entries = &by_name[name];
        for (pos, _) in text.match_indices(name) {
            let span = Span { start: pos, end: pos + name.len() };
            if out.iter().any(|o| o.span.overlaps(&span)) {
                continue;
            }
            // 앞 글자가 한글이면 다른 낱말의 일부 ("박홍길동" 같은 경우)로 보고 건너뛴다
            if text[..pos].chars().next_back().is_some_and(is_hangul) || !name_ends_here(text, span.end, char_len(name)) {
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
                written_class: None,
            });
        }
    }

    // 3) 성을 뺀 이름 ("길동이가 발표함") — 명단에서 하나로 정해질 때만
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
            if text[..pos].chars().next_back().is_some_and(is_hangul) {
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
                written_class: None,
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

/// "OO중 교무부장", "OO고 교무부장"처럼 다른 학교 이름 바로 뒤에 온 낱말은 그 학교 선생님의 직함이다
/// ("교육지원청 교무부장 회의"는 내 업무와 관련된 모임일 수 있어 학교 이름만 본다)
static RE_ORG_BEFORE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[가-힣A-Za-z○◯]{1,10}(?:중|고|초|여중|여고|중학교|고등학교|초등학교)\s*$").unwrap());

/// 낱말이 원문에 '내 업무'로 쓰였는가: 다른 학교 이름 뒤에 붙은 직함으로만 나오면 아니다
pub fn mentions_as_own(s: &str, term: &str) -> bool {
    s.match_indices(term).any(|(pos, _)| !RE_ORG_BEFORE.is_match(&s[..pos]))
}

pub fn match_duties(s: &str, duties: &[DutyEntry]) -> Vec<i64> {
    let mut out = Vec::new();
    for d in duties {
        if d.match_terms().iter().any(|t| mentions_as_own(s, t)) {
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
        // "오늘 3교시 수업중 잠 → 지도함"처럼 이미 있었던 일을 적은 문장의 오늘·지난 날짜는 일정이 아니다
        if is_past(c.sentence_span.slice(&self.text)) && c.dates.iter().all(|d| d.date <= self.today) {
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
        assert_eq!(attendance_fact("3교시 홍길동 지각"), Some(AttendanceKind::Late));
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
        let m = find_students("오늘 3교시 2-1 홍길동 지각.", &[]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "홍길동");
        assert_eq!(m[0].class_label.as_deref(), Some("2-1"));
        let m = find_students("2학년 3반 12번 성춘향은 발표를 잘함", &[]);
        assert_eq!(m[0].name, "성춘향");
        assert_eq!(m[0].class_label.as_deref(), Some("2-3"));
        assert_eq!(m[0].number, Some(12));
        let m = find_students("2-1 홍길동지각", &[]);
        assert_eq!(m[0].name, "홍길동");
        let m = find_students("2-1 학생들 전체 지도", &[]);
        assert!(m.is_empty());
    }

    /// 학년마다 한 반인 학교의 명렬표 (가짜 이름) + 학급 구성 항목
    fn one_class_school() -> Vec<RosterEntry> {
        let s = |id, name: &str, l: &str, n| RosterEntry { id, name: name.into(), class_label: Some(l.into()), number: Some(n) };
        let c = |l: &str| RosterEntry { id: 0, name: String::new(), class_label: Some(l.into()), number: None };
        vec![s(1, "심청", "1-1", 1), s(2, "전우치", "1-1", 2), s(3, "장화", "2-1", 1), c("1-1"), c("2-1"), c("3-1")]
    }

    fn ids(m: &[StudentMention]) -> Vec<(Option<i64>, Option<String>)> {
        m.iter().map(|x| (x.roster_id, x.class_label.clone())).collect()
    }

    #[test]
    fn grade_context_and_comma_lists() {
        let r = one_class_school();
        let both = vec![(Some(1), Some("1-1".to_string())), (Some(2), Some("1-1".to_string()))];
        assert_eq!(ids(&find_students("1학년 심청, 전우치 수업 중 잠", &r)), both);
        assert_eq!(ids(&find_students("심청 전우치 지각", &r)), both, "학년 없이도 명렬표에서 학급을 찾는다");
        for t in ["1학년 심청 발표", "1학년1반 심청 발표", "1학년 1반 심청 발표", "1-1 심청 발표", "1년 심청 발표"] {
            assert_eq!(ids(&find_students(t, &r)), vec![(Some(1), Some("1-1".to_string()))], "{t}");
        }
        // 명단에 없는 학생도 학년만 적으면 그 학년의 하나뿐인 반으로
        let m = find_students("3학년 이영희, 김영수 지각", &r);
        assert_eq!(m.iter().map(|x| (x.name.as_str(), x.class_label.as_deref())).collect::<Vec<_>>(), vec![("이영희", Some("3-1")), ("김영수", Some("3-1"))]);
        // 이름 뒤의 낱말을 이름으로 보지 않는다
        let m = find_students("1학년 전우치 지도함", &r);
        assert_eq!(m.len(), 1);
        assert!(find_students("2026학년도 교육과정 계획", &r).is_empty());
        assert!(find_students("10-2 11:00 회의", &r).is_empty());
    }

    #[test]
    fn two_char_names_need_boundaries() {
        let r = one_class_school();
        assert!(find_students("심청기 발표", &r).is_empty(), "다른 낱말의 일부");
        assert!(find_students("장화비 완료", &r).is_empty());
        for t in ["심청 발표", "심청은 발표함", "심청이가 발표", "심청, 장화 청소", "(심청) 발표"] {
            assert!(find_students(t, &r).iter().any(|m| m.roster_id == Some(1)), "{t}");
        }
    }

    #[test]
    fn written_class_differs_from_roster() {
        let r = one_class_school();
        let m = find_students("2학년 심청 지각", &r);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].roster_id, m[0].class_label.as_deref(), m[0].written_class.as_deref()), (Some(1), Some("1-1"), Some("2-1")));
    }

    #[test]
    fn past_observation_is_not_event() {
        assert!(is_past("수업중 잠 → 지도함"));
        assert!(is_past("3교시 수업 중 잠."));
        assert!(!is_past("잠자리 안전 교육"));
        let a = analyze("오늘 3교시 수업중 잠 → 지도함", today(), &[], &[]);
        assert!((0..a.clauses.len()).all(|i| !a.clause_is_event(i)));
    }
    #[test]
    fn other_school_titles_are_not_my_duty() {
        let duties = vec![DutyEntry { id: 7, name: "교무부장".into(), detail: "국제교류, 학사 운영".into(), guide: String::new(), keywords: vec![] }];
        assert!(match_duties("OO중 교무부장 연락옴", &duties).is_empty());
        assert!(match_duties("OO고 교무부장님과 통화함", &duties).is_empty());
        assert_eq!(match_duties("OO교육지원청 교무부장 회의 안내", &duties), vec![7]);
        assert_eq!(match_duties("교무부장 회의 자료 준비", &duties), vec![7]);
        assert_eq!(match_duties("한일국제교류 컨설팅", &duties), vec![7]);
    }

    #[test]
    fn roster_and_homonyms() {
        let roster = vec![
            RosterEntry { id: 1, name: "홍길동".into(), class_label: Some("2-1".into()), number: Some(3) },
            RosterEntry { id: 2, name: "홍길동".into(), class_label: Some("2-3".into()), number: Some(5) },
            RosterEntry { id: 3, name: "성춘향".into(), class_label: Some("2-1".into()), number: Some(7) },
        ];
        let m = find_students("2-1 홍길동 지각", &roster);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].roster_id, Some(1));
        let m = find_students("홍길동 발표 우수", &roster);
        assert_eq!(m[0].roster_id, None);
        assert_eq!(m[0].candidates, vec![1, 2]);
        let m = find_students("춘향이가 친구를 도와줌", &roster);
        assert_eq!(m[0].roster_id, Some(3));
    }

    #[test]
    fn tasks_and_events() {
        let t = "오늘 3교시 2-1 홍길동 지각. 담임에게 알리고 금요일까지 보호자 연락.";
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
        assert!(match_duties("홍길동 지각", &duties).is_empty());

        // 세부업무 하나에 낱말·구·문장을 섞어 적은 경우: 짧은 낱말·구만 규칙에 쓰고, 긴 문장은 AI 몫
        let merged = vec![DutyEntry {
            id: 9,
            name: "교무기획".into(),
            detail: "학사일정 관리, 수업일수, 교육청에 학사일정 변경을 보고하고 교직원에게 안내하는 일".into(),
            guide: String::new(),
            keywords: vec![],
        }];
        assert_eq!(match_duties("수업일수 계산", &merged), vec![9]);
        assert!(match_duties("교육청 공문 회신", &merged).is_empty(), "긴 문장 속 흔한 낱말로 분류하지 않는다");
        assert_eq!(merged[0].label(), "교무기획 / 학사일정 관리");
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
