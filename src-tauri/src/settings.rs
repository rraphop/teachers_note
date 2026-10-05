//! 앱 설정 (DB의 settings 표에 JSON 으로 저장)

use crate::classify::ReminderSpec;
use crate::db;
use crate::error::AppResult;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

const KEY: &str = "app_settings";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppSettings {
    /// 기록 저장 후 AI 자동 분류
    pub ai_auto_classify: bool,
    /// 선택한 모델 id
    pub selected_model: Option<String>,
    /// AI 엔진을 쓰지 않은 채 이 시간이 지나면 메모리에서 내린다 (0 = 계속 유지)
    pub idle_unload_minutes: u32,
    /// 새 할 일·일정의 기본 알림
    pub remind_default: ReminderSpec,
    /// 창을 닫으면 알림 영역(트레이)에서 계속 실행
    pub close_to_tray: bool,
    /// 창을 다른 프로그램 창보다 항상 위에 둔다 (set_always_on_top 으로만 바꾼다)
    pub always_on_top: bool,
    /// 창 투명도(%) 0~70 (set_window_transparency 로만 바꾼다)
    pub window_transparency: u8,
    /// AI 계산에 쓸 CPU 스레드 (None = 자동)
    pub threads: Option<u32>,
    /// 처음 실행 안내를 마쳤는지
    pub onboarding_done: bool,
    /// 학급명렬표에서 켠 항목 (기본은 모두 꺼짐 — 필요한 것만 켠다)
    pub roster_columns: RosterColumns,
    /// 학생 생일을 일정표(학사력)에 표시
    pub birthdays_on_calendar: bool,
    /// 나이스 명렬표의 주민등록번호를 저장 (기본 끔. 켜도 뒷자리를 가린 모양으로만 저장)
    pub store_rrn: bool,
    /// 우리 학교 (나이스 교육정보 개방 포털의 교육청·학교 코드)
    pub school: Option<crate::neis::SchoolInfo>,
    /// 담임반 ("1-1", 없으면 빈 값)
    pub homeroom: String,
    /// 나이스 인증키 (선택. 없으면 나이스가 처음 5줄만 주어 시간표·학사일정은 받지 못한다)
    pub neis_key: String,
    /// 내 과목: 학교 시간표에서 이 과목 칸만 모아 개인 시간표를 만든다
    pub my_subjects: Vec<String>,
    /// 개인 시간표에서 뺄 학급 (내 과목이지만 다른 선생님이 가르치는 학급)
    pub my_classes_off: Vec<String>,
    /// 학사력 날짜 색 (휴일·휴업일·방학·업무)
    pub day_colors: DayColors,
    /// 학사력 종류 규칙 (나이스 구분값이 없을 때 행사 이름으로 정한다)
    pub day_rules: DayRules,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DayColors {
    pub holiday: String,
    pub closure: String,
    pub vacation: String,
    pub work: String,
}

impl Default for DayColors {
    fn default() -> Self {
        DayColors { holiday: "red".into(), closure: "red".into(), vacation: "green".into(), work: "black".into() }
    }
}

/// 학사력에서 고를 수 있는 색 (10가지)
pub const DAY_COLOR_NAMES: [&str; 10] = ["red", "orange", "amber", "green", "teal", "blue", "indigo", "purple", "gray", "black"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DayRules {
    /// 휴일 (이름에 이 낱말이 들어 있으면)
    pub holiday: Vec<String>,
    /// 휴업일
    pub closure: Vec<String>,
    /// 방학
    pub vacation: Vec<String>,
}

impl Default for DayRules {
    fn default() -> Self {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        DayRules {
            holiday: v(&["3·1절", "삼일절", "대체공휴일", "어린이날", "부처님 오신 날", "부처님오신날", "현충일", "제헌절", "광복절", "지방선거일", "노동절"]),
            closure: v(&["토요휴업일", "재량휴업일", "개교기념일"]),
            vacation: v(&["여름방학", "겨울방학", "방학선언", "방학식"]),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RosterColumns {
    pub phone: bool,
    pub guardian_phone: bool,
    pub address: bool,
    pub gender: bool,
    pub birthday: bool,
    pub memo: bool,
    /// 나이스 명렬표 항목
    pub student_no: bool,
    pub prev_record: bool,
    pub remark: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            ai_auto_classify: true,
            selected_model: None,
            idle_unload_minutes: 10,
            remind_default: ReminderSpec { enabled: true, offsets: vec![1], time: "08:30".into(), custom_at: None },
            close_to_tray: false,
            always_on_top: false,
            window_transparency: 0,
            threads: None,
            onboarding_done: false,
            roster_columns: RosterColumns::default(),
            birthdays_on_calendar: false,
            store_rrn: false,
            school: None,
            homeroom: String::new(),
            neis_key: String::new(),
            my_subjects: vec![],
            my_classes_off: vec![],
            day_colors: DayColors::default(),
            day_rules: DayRules::default(),
        }
    }
}

pub fn load(conn: &Connection) -> AppResult<AppSettings> {
    Ok(db::get_setting(conn, KEY)?
        .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
        .unwrap_or_default())
}

pub fn save(conn: &Connection, s: &AppSettings) -> AppResult<()> {
    let mut s = s.clone();
    s.idle_unload_minutes = s.idle_unload_minutes.min(240);
    s.remind_default.offsets.retain(|o| (0..=30).contains(o));
    s.remind_default.custom_at = None;
    if crate::util::parse_t(&s.remind_default.time).is_none() {
        s.remind_default.time = "08:30".into();
    }
    s.neis_key = s.neis_key.trim().chars().filter(|c| c.is_ascii_alphanumeric()).take(64).collect();
    s.homeroom = match crate::students::parse_class_label(&s.homeroom) {
        Some((g, n)) => format!("{g}-{n}"),
        None => String::new(),
    };
    let clean = |v: &mut Vec<String>, max_len: usize| {
        let mut out: Vec<String> = Vec::new();
        for x in v.iter().map(|x| x.trim().chars().take(max_len).collect::<String>()).filter(|x| !x.is_empty()) {
            if !out.contains(&x) {
                out.push(x);
            }
        }
        out.truncate(60);
        *v = out;
    };
    clean(&mut s.my_subjects, 30);
    clean(&mut s.my_classes_off, 10);
    clean(&mut s.day_rules.holiday, 30);
    clean(&mut s.day_rules.closure, 30);
    clean(&mut s.day_rules.vacation, 30);
    for c in [&mut s.day_colors.holiday, &mut s.day_colors.closure, &mut s.day_colors.vacation, &mut s.day_colors.work] {
        if !DAY_COLOR_NAMES.contains(&c.as_str()) {
            *c = "black".into();
        }
    }
    db::set_setting(conn, KEY, &serde_json::to_string(&s)?)?;
    // 저장을 끄면 이미 저장한 주민등록번호도 지운다
    if !s.store_rrn {
        conn.execute("UPDATE students SET rrn='' WHERE rrn<>''", [])?;
    }
    Ok(())
}

pub fn update(conn: &Connection, f: impl FnOnce(&mut AppSettings)) -> AppResult<AppSettings> {
    let mut s = load(conn)?;
    f(&mut s);
    save(conn, &s)?;
    load(conn)
}
