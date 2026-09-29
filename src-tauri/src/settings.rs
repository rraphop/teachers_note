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
    /// AI 계산에 쓸 CPU 스레드 (None = 자동)
    pub threads: Option<u32>,
    /// 처음 실행 안내를 마쳤는지
    pub onboarding_done: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            ai_auto_classify: true,
            selected_model: None,
            idle_unload_minutes: 10,
            remind_default: ReminderSpec { enabled: true, offsets: vec![1], time: "08:30".into(), custom_at: None },
            close_to_tray: false,
            threads: None,
            onboarding_done: false,
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
    db::set_setting(conn, KEY, &serde_json::to_string(&s)?)
}

pub fn update(conn: &Connection, f: impl FnOnce(&mut AppSettings)) -> AppResult<AppSettings> {
    let mut s = load(conn)?;
    f(&mut s);
    save(conn, &s)?;
    load(conn)
}
