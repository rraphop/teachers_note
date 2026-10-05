//! 나이스 교육정보 개방 포털(open.neis.go.kr) 공개 API: 학교 찾기, 학급, 시간표, 급식, 학사일정
//!
//! - 보내는 값은 교육청·학교 코드, 날짜, 학교 이름(찾을 때), 인증키(넣었을 때)뿐이다. 학생 정보는 보내지 않는다.
//! - 인증키가 없으면 나이스는 처음 5줄만 돌려준다(샘플). 그래서
//!   - 급식은 한 주씩 받는다 (중식만 주는 학교는 한 주 5줄).
//!   - 시간표·학사일정은 키가 있어야 모두 받을 수 있다. 키가 없으면 일부만 받아 틀린 시간표를 만들지 않고, 키가 필요하다고 알린다.
//! - 받은 자료는 DB에 넣어 둔다(인터넷이 없어도 보인다). 앱을 켤 때 하루 한 번 새로 받는다.

use crate::db::{self, Db};
use crate::error::{AppError, AppResult};
use crate::settings::{self, AppSettings, DayRules};
use crate::timetable::{self, Cell};
use crate::util;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

const BASE: &str = "https://open.neis.go.kr/hub";

/// 나이스 주소. 시험(E2E)에서만 TN_NEIS_BASE 로 이 PC 안의 시험 서버(127.0.0.1)를 쓸 수 있다.
fn base() -> String {
    std::env::var("TN_NEIS_BASE").ok().filter(|b| b.starts_with("http://127.0.0.1:")).unwrap_or_else(|| BASE.to_string())
}

/// 나이스에 GET 요청을 보내 본문을 받는 함수 (시험에서는 가짜로 바꾼다)
pub type Getter<'a> = &'a dyn Fn(&str) -> AppResult<String>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SchoolInfo {
    /// 교육청 코드 (ATPT_OFCDC_SC_CODE, 영문 1자 + 숫자 2자)
    pub office_code: String,
    pub office_name: String,
    /// 학교 코드 (SD_SCHUL_CODE, 숫자 7자리)
    pub code: String,
    pub name: String,
    /// 중학교 · 고등학교 …
    pub kind: String,
    /// 지역 (LCTN_SC_NM)
    pub region: String,
    pub address: String,
}

type Row = Map<String, Value>;

fn field(row: &Row, key: &str) -> String {
    match row.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn result_error(code: &str, message: &str) -> AppError {
    let m = match code {
        "ERROR-290" => "나이스 인증키가 올바르지 않습니다. 설정 → 학교에서 인증키를 확인해 주세요.".to_string(),
        "ERROR-337" => "오늘 나이스 이용 한도를 넘었습니다. 내일 다시 받습니다.".to_string(),
        "INFO-300" => "나이스 인증키 이용이 제한되었습니다. 나이스 교육정보 개방 포털에서 확인해 주세요.".to_string(),
        c if c.starts_with("ERROR-5") || c.starts_with("ERROR-6") => "나이스 서버에 문제가 있어 받지 못했습니다. 잠시 뒤 다시 해 주세요.".to_string(),
        _ => format!("나이스에서 받지 못했습니다 ({code} {message})"),
    };
    AppError::msg(m)
}

/// 나이스 응답 한 쪽
#[derive(Debug, Default)]
pub struct Page {
    pub rows: Vec<Row>,
    pub total: usize,
}

/// {"서비스":[{"head":[{"list_total_count":n},{"RESULT":{..}}]},{"row":[..]}]} 또는 {"RESULT":{"CODE":"INFO-200",..}} (자료 없음)
pub fn parse_page(service: &str, body: &str) -> AppResult<Page> {
    let v: Value = serde_json::from_str(body).map_err(|_| AppError::msg("나이스 응답을 읽지 못했습니다."))?;
    if let Some(arr) = v.get(service).and_then(|x| x.as_array()) {
        let head = arr.iter().find_map(|x| x.get("head")).and_then(|h| h.as_array());
        let total = head.and_then(|h| h.iter().find_map(|x| x.get("list_total_count")).and_then(|n| n.as_u64())).unwrap_or(0) as usize;
        if let Some(r) = head.and_then(|h| h.iter().find_map(|x| x.get("RESULT"))) {
            let code = r.get("CODE").and_then(|c| c.as_str()).unwrap_or("INFO-000");
            if code != "INFO-000" {
                return Err(result_error(code, r.get("MESSAGE").and_then(|m| m.as_str()).unwrap_or("")));
            }
        }
        let rows = arr
            .iter()
            .find_map(|x| x.get("row"))
            .and_then(|r| r.as_array())
            .map(|r| r.iter().filter_map(|x| x.as_object().cloned()).collect())
            .unwrap_or_default();
        return Ok(Page { rows, total });
    }
    let r = v.get("RESULT");
    let code = r.and_then(|r| r.get("CODE")).and_then(|c| c.as_str()).unwrap_or("");
    if code == "INFO-200" {
        return Ok(Page::default());
    }
    Err(result_error(code, r.and_then(|r| r.get("MESSAGE")).and_then(|m| m.as_str()).unwrap_or("")))
}

fn url(service: &str, params: &[(&str, String)], key: &str, p_index: usize, p_size: usize) -> String {
    let mut q: Vec<(String, String)> = vec![("Type".into(), "json".into()), ("pIndex".into(), p_index.to_string()), ("pSize".into(), p_size.to_string())];
    if !key.is_empty() {
        q.push(("KEY".into(), key.into()));
    }
    q.extend(params.iter().map(|(k, v)| (k.to_string(), v.clone())));
    let qs: Vec<String> = q.iter().map(|(k, v)| format!("{k}={}", urlencode(v))).collect();
    format!("{}/{service}?{}", base(), qs.join("&"))
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 서비스의 모든 줄을 받는다. 인증키가 없으면 첫 쪽(5줄)만 받고, 다 못 받았으면 limited = true.
pub fn fetch_all(service: &str, params: &[(&str, String)], key: &str, get: Getter) -> AppResult<(Vec<Row>, bool)> {
    if key.is_empty() {
        let p = parse_page(service, &get(&url(service, params, "", 1, 5))?)?;
        let limited = p.total > p.rows.len();
        return Ok((p.rows, limited));
    }
    let mut rows: Vec<Row> = Vec::new();
    for page in 1..=20 {
        let p = parse_page(service, &get(&url(service, params, key, page, 1000))?)?;
        let got = p.rows.len();
        rows.extend(p.rows);
        if got == 0 || rows.len() >= p.total {
            return Ok((rows, false));
        }
    }
    Ok((rows, true))
}

/// 실제 인터넷 요청 (나이스만)
pub fn http_get(url: &str) -> AppResult<String> {
    if !url.starts_with(&base()) {
        return Err(AppError::msg("나이스 주소가 아닙니다."));
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("LocalTeacherNotebook/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(25))
        .build()
        .map_err(|e| AppError::msg(format!("인터넷 연결을 준비하지 못했습니다: {e}")))?;
    let r = client.get(url).send().map_err(|_| AppError::msg("나이스에 연결하지 못했습니다. 인터넷 연결을 확인해 주세요. (받아 둔 자료는 그대로 보입니다)"))?;
    if !r.status().is_success() {
        return Err(AppError::msg(format!("나이스가 응답하지 않았습니다 (HTTP {}).", r.status().as_u16())));
    }
    r.text().map_err(|_| AppError::msg("나이스 응답을 받지 못했습니다."))
}

// ───────────────────────── 학교 찾기 ─────────────────────────

pub fn school_from_row(r: &Row) -> SchoolInfo {
    SchoolInfo {
        office_code: field(r, "ATPT_OFCDC_SC_CODE"),
        office_name: field(r, "ATPT_OFCDC_SC_NM"),
        code: field(r, "SD_SCHUL_CODE"),
        name: field(r, "SCHUL_NM"),
        kind: field(r, "SCHUL_KND_SC_NM"),
        region: field(r, "LCTN_SC_NM"),
        address: field(r, "ORG_RDNMA"),
    }
}

/// 학교 이름으로 찾기 (보내는 값: 학교 이름)
pub fn search_schools(name: &str, key: &str, get: Getter) -> AppResult<Vec<SchoolInfo>> {
    let n = name.trim();
    if n.chars().count() < 2 {
        return Err(AppError::invalid("학교 이름을 두 글자 이상 적어 주세요."));
    }
    let (rows, _) = fetch_all("schoolInfo", &[("SCHUL_NM", n.to_string())], key, get)?;
    Ok(rows.iter().map(school_from_row).filter(|s| !s.code.is_empty()).collect())
}

fn school_params(s: &SchoolInfo) -> Vec<(&'static str, String)> {
    vec![("ATPT_OFCDC_SC_CODE", s.office_code.clone()), ("SD_SCHUL_CODE", s.code.clone())]
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y%m%d").to_string()
}

fn from_ymd(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y%m%d").ok()
}

fn monday(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

// ───────────────────────── 받은 기록 ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct FetchInfo {
    pub what: String,
    pub scope: String,
    pub fetched_at: String,
    /// ok | empty | need_key | error
    pub status: String,
    pub message: String,
}

fn record(conn: &Connection, what: &str, scope: &str, status: &str, message: &str, now: NaiveDateTime) -> AppResult<()> {
    conn.execute(
        "INSERT INTO neis_fetches(what, scope, fetched_at, status, message) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(what, scope) DO UPDATE SET fetched_at=excluded.fetched_at, status=excluded.status, message=excluded.message",
        params![what, scope, util::fmt_dt(now), status, message],
    )?;
    Ok(())
}

fn last_fetch(conn: &Connection, what: &str, scope: &str) -> AppResult<Option<FetchInfo>> {
    Ok(conn
        .query_row(
            "SELECT what, scope, fetched_at, status, message FROM neis_fetches WHERE what=?1 AND scope=?2",
            params![what, scope],
            |r| Ok(FetchInfo { what: r.get(0)?, scope: r.get(1)?, fetched_at: r.get(2)?, status: r.get(3)?, message: r.get(4)? }),
        )
        .optional()?)
}

// ───────────────────────── 급식 ─────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Dish {
    pub name: String,
    /// 알레르기 유발 식품 번호 (1 난류 … 19 잣)
    pub allergens: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MealInfo {
    /// 조식 | 중식 | 석식
    pub meal: String,
    pub dishes: Vec<Dish>,
    pub calories: String,
    pub nutrition: Vec<String>,
    pub origin: Vec<String>,
    pub people: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MealsView {
    pub date: String,
    /// 학교를 정하지 않았으면 None
    pub school_name: Option<String>,
    pub meals: Vec<MealInfo>,
    /// 급식이 없는 날: 다음 급식일과 그날 급식
    pub next_date: Option<String>,
    pub next_meals: Vec<MealInfo>,
    /// 이 주를 마지막으로 받은 때
    pub fetched_at: Option<String>,
    /// 안내 (인터넷 연결 없음 등)
    pub message: Option<String>,
}

static RE_ALLERGEN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*\(?\s*((?:\d{1,2}\.)*\d{1,2})\.?\s*\)?\s*$").unwrap());

fn br_lines(s: &str) -> Vec<String> {
    s.replace("<br/>", "\n").replace("<br>", "\n").replace("<BR/>", "\n").lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// "육개장 (1.16)" → 육개장 + [1, 16]. 예전 형식 "육개장1.16." 도 읽는다.
pub fn parse_dish(line: &str) -> Dish {
    let t = line.trim();
    if let Some(m) = RE_ALLERGEN.captures(t) {
        let whole = m.get(0).unwrap();
        let name = t[..whole.start()].trim().trim_end_matches(['*', ' ']).to_string();
        let nums: Vec<u8> = m[1].split('.').filter_map(|x| x.parse::<u8>().ok()).filter(|n| (1..=19).contains(n)).collect();
        if !name.is_empty() && !nums.is_empty() {
            return Dish { name, allergens: nums };
        }
    }
    Dish { name: t.trim_end_matches(['*', ' ']).to_string(), allergens: vec![] }
}

#[derive(Debug, Clone)]
struct MealRow {
    date: String,
    meal: String,
    dishes: String,
    calories: String,
    nutrition: String,
    origin: String,
    people: String,
}

fn meal_rows(rows: &[Row]) -> Vec<MealRow> {
    rows.iter()
        .filter_map(|r| {
            let date = from_ymd(&field(r, "MLSV_YMD"))?;
            Some(MealRow {
                date: util::fmt_d(date),
                meal: field(r, "MMEAL_SC_NM"),
                dishes: br_lines(&field(r, "DDISH_NM")).join("\n"),
                calories: field(r, "CAL_INFO"),
                nutrition: br_lines(&field(r, "NTR_INFO")).join("\n"),
                origin: br_lines(&field(r, "ORPLC_INFO")).join("\n"),
                people: field(r, "MLSV_FGR"),
            })
        })
        .filter(|m| !m.dishes.is_empty())
        .collect()
}

fn store_meals(conn: &Connection, school: &str, rows: &[MealRow]) -> AppResult<()> {
    for m in rows {
        conn.execute(
            "INSERT INTO neis_meals(school_code, date, meal, dishes, calories, nutrition, origin, people) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(school_code, date, meal) DO UPDATE SET dishes=excluded.dishes, calories=excluded.calories, nutrition=excluded.nutrition,
               origin=excluded.origin, people=excluded.people",
            params![school, m.date, m.meal, m.dishes, m.calories, m.nutrition, m.origin, m.people],
        )?;
    }
    Ok(())
}

fn load_meals(conn: &Connection, school: &str, date: &str) -> AppResult<Vec<MealInfo>> {
    let mut st = conn.prepare(
        "SELECT meal, dishes, calories, nutrition, origin, people FROM neis_meals WHERE school_code=?1 AND date=?2
         ORDER BY CASE meal WHEN '조식' THEN 1 WHEN '중식' THEN 2 WHEN '석식' THEN 3 ELSE 4 END",
    )?;
    let rows = st
        .query_map(params![school, date], |r| {
            let dishes: String = r.get(1)?;
            let nutrition: String = r.get(3)?;
            let origin: String = r.get(4)?;
            Ok(MealInfo {
                meal: r.get(0)?,
                dishes: dishes.lines().map(parse_dish).collect(),
                calories: r.get(2)?,
                nutrition: nutrition.lines().map(String::from).collect(),
                origin: origin.lines().map(String::from).collect(),
                people: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn meal_scope(school: &SchoolInfo, mon: NaiveDate) -> String {
    format!("{}:{}", school.code, util::fmt_d(mon))
}

/// 그 주 급식을 받는다 (이미 받았고 하루가 안 지났으면 건너뛴다). 인터넷이 안 되면 받아 둔 것을 쓴다.
fn ensure_meal_week(db: &Db, school: &SchoolInfo, key: &str, mon: NaiveDate, now: NaiveDateTime, get: Getter) -> AppResult<Option<String>> {
    let scope = meal_scope(school, mon);
    let last = db.with(|c| last_fetch(c, "meals", &scope))?;
    let this_week = monday(now.date()) == mon;
    let fresh = last.as_ref().is_some_and(|l| {
        l.status != "error" && (!this_week || util::parse_dt(&l.fetched_at).is_some_and(|t| now - t < Duration::hours(20)))
    });
    if fresh {
        return Ok(None);
    }
    let mut p = school_params(school);
    p.push(("MLSV_FROM_YMD", ymd(mon)));
    p.push(("MLSV_TO_YMD", ymd(mon + Duration::days(6))));
    match fetch_all("mealServiceDietInfo", &p, key, get) {
        Ok((rows, limited)) => {
            let meals = meal_rows(&rows);
            db.with(|c| {
                store_meals(c, &school.code, &meals)?;
                let status = if meals.is_empty() { "empty" } else { "ok" };
                record(c, "meals", &scope, status, if limited { "partial" } else { "" }, now)
            })?;
            Ok(None)
        }
        Err(e) => {
            let msg = e.to_string();
            db.with(|c| record(c, "meals", &scope, "error", &msg, now))?;
            Ok(Some(msg))
        }
    }
}

/// 한 주가 5줄을 넘어(조식·석식 등) 다 받지 못한 경우, 보려는 날 하루만 받는다
fn ensure_meal_day(db: &Db, school: &SchoolInfo, key: &str, date: NaiveDate, now: NaiveDateTime, get: Getter) -> AppResult<()> {
    let scope = format!("{}:day:{}", school.code, util::fmt_d(date));
    if db.with(|c| last_fetch(c, "meals", &scope))?.is_some_and(|l| l.status != "error") {
        return Ok(());
    }
    let mut p = school_params(school);
    p.push(("MLSV_YMD", ymd(date)));
    if let Ok((rows, _)) = fetch_all("mealServiceDietInfo", &p, key, get) {
        let meals = meal_rows(&rows);
        db.with(|c| {
            store_meals(c, &school.code, &meals)?;
            record(c, "meals", &scope, if meals.is_empty() { "empty" } else { "ok" }, "", now)
        })?;
    }
    Ok(())
}

/// 그날 급식 (없으면 다음 급식일 미리 보기)
pub fn meals_for(db: &Db, date: NaiveDate, now: NaiveDateTime, get: Getter) -> AppResult<MealsView> {
    let s = db.with(|c| settings::load(c))?;
    let Some(school) = s.school.clone().filter(|x| !x.code.is_empty()) else {
        return Ok(MealsView { date: util::fmt_d(date), school_name: None, meals: vec![], next_date: None, next_meals: vec![], fetched_at: None, message: None });
    };
    let mon = monday(date);
    let mut message = ensure_meal_week(db, &school, &s.neis_key, mon, now, get)?;
    let scope = meal_scope(&school, mon);
    let partial = db.with(|c| last_fetch(c, "meals", &scope))?.is_some_and(|l| l.message == "partial");
    if partial {
        ensure_meal_day(db, &school, &s.neis_key, date, now, get)?;
    }
    let meals = db.with(|c| load_meals(c, &school.code, &util::fmt_d(date)))?;
    let (mut next_date, mut next_meals) = (None, vec![]);
    if meals.is_empty() {
        // 다음 급식일: 이번 주와 다음 주(필요하면 받는다)에서 찾는다
        for week in 0..2 {
            if week == 1 {
                if let Some(m) = ensure_meal_week(db, &school, &s.neis_key, mon + Duration::days(7), now, get)? {
                    message.get_or_insert(m);
                }
            }
            let found: Option<String> = db.with(|c| {
                Ok(c.query_row(
                    "SELECT MIN(date) FROM neis_meals WHERE school_code=?1 AND date>?2 AND date<=?3",
                    params![school.code, util::fmt_d(date), util::fmt_d(mon + Duration::days(13))],
                    |r| r.get(0),
                )?)
            })?;
            if let Some(d) = found {
                next_meals = db.with(|c| load_meals(c, &school.code, &d))?;
                next_date = Some(d);
                break;
            }
        }
    }
    let fetched_at = db.with(|c| last_fetch(c, "meals", &scope))?.filter(|l| l.status != "error").map(|l| l.fetched_at);
    Ok(MealsView { date: util::fmt_d(date), school_name: Some(school.name.clone()), meals, next_date, next_meals, fetched_at, message })
}

// ───────────────────────── 학급 ─────────────────────────

/// 학교의 학급 목록 (학년마다 반이 몇 개인지 판단에 쓴다). 다 받지 못했으면 저장하지 않는다.
pub fn sync_classes(db: &Db, school: &SchoolInfo, key: &str, year: i32, now: NaiveDateTime, get: Getter) -> AppResult<Vec<String>> {
    let mut p = school_params(school);
    p.push(("AY", year.to_string()));
    let (rows, limited) = fetch_all("classInfo", &p, key, get)?;
    let mut labels: Vec<(i64, i64)> = rows
        .iter()
        .filter_map(|r| Some((field(r, "GRADE").parse().ok()?, field(r, "CLASS_NM").parse().ok()?)))
        .collect();
    labels.sort();
    labels.dedup();
    let out: Vec<String> = labels.iter().map(|(g, c)| format!("{g}-{c}")).collect();
    if !limited && !out.is_empty() {
        db.with(|c| {
            db::set_setting(c, "neis_classes", &serde_json::to_string(&out)?)?;
            record(c, "classes", &format!("{}:{year}", school.code), "ok", "", now)
        })?;
    }
    Ok(out)
}

/// 저장해 둔 나이스 학급 목록
pub fn saved_classes(conn: &Connection) -> Vec<String> {
    db::get_setting(conn, "neis_classes").ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

// ───────────────────────── 시간표 ─────────────────────────

#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncReport {
    /// ok | empty | need_key | no_school | error
    pub status: String,
    pub message: String,
    pub count: usize,
    pub fetched_at: Option<String>,
}

fn clean_subject(s: &str) -> String {
    s.trim().trim_start_matches(['*', '-', '·', ' ']).trim().to_string()
}

/// 받은 날짜별 시간표 → 요일·교시별로 가장 많이 나온 과목 (같으면 최근 것). 학년도·학기는 가장 많이 나온 값.
pub fn weekly_template(rows: &[Row]) -> (Option<(i32, u8)>, BTreeMap<(String, i64, i64), String>) {
    let mut counts: BTreeMap<(String, i64, i64), BTreeMap<String, (usize, String)>> = BTreeMap::new();
    let mut sems: BTreeMap<(i32, u8), usize> = BTreeMap::new();
    for r in rows {
        let (Some(date), Ok(g), Ok(c), Ok(p)) =
            (from_ymd(&field(r, "ALL_TI_YMD")), field(r, "GRADE").parse::<i64>(), field(r, "CLASS_NM").parse::<i64>(), field(r, "PERIO").parse::<i64>())
        else {
            continue;
        };
        let subject = clean_subject(&field(r, "ITRT_CNTNT"));
        let wd = date.weekday().number_from_monday() as i64;
        if subject.is_empty() || wd > 6 || !(1..=10).contains(&p) {
            continue;
        }
        if let (Ok(ay), Ok(sem)) = (field(r, "AY").parse::<i32>(), field(r, "SEM").parse::<u8>()) {
            *sems.entry((ay, sem)).or_default() += 1;
        }
        let e = counts.entry((format!("{g}-{c}"), wd, p)).or_default().entry(subject).or_insert((0, String::new()));
        e.0 += 1;
        let d = util::fmt_d(date);
        if d > e.1 {
            e.1 = d;
        }
    }
    let sem = sems.into_iter().max_by_key(|(_, n)| *n).map(|(k, _)| k).filter(|(_, s)| *s == 1 || *s == 2);
    let template = counts
        .into_iter()
        .filter_map(|(k, subj)| subj.into_iter().max_by(|a, b| a.1.0.cmp(&b.1.0).then(a.1.1.cmp(&b.1.1))).map(|(s, _)| (k, s)))
        .collect();
    (sem, template)
}

/// 학교 시간표(나이스 칸)에서 내 과목 칸만 모아 개인 시간표를 다시 만든다 (교사가 직접 입력한 칸은 그대로). 겹친 칸 목록을 돌려준다.
pub fn rebuild_personal(conn: &Connection, s: &AppSettings, year: i32, sem: u8, now: NaiveDateTime) -> AppResult<(usize, Vec<String>)> {
    if s.my_subjects.is_empty() {
        // 내 과목을 모두 지우면 나이스에서 온 개인 시간표 칸도 지운다 (교사가 입력한 칸은 그대로)
        timetable::replace_neis_cells(conn, year, sem, "personal", "", &[], now)?;
        return Ok((0, vec![]));
    }
    let school = timetable::get_timetable(conn, year, sem, "school", "")?;
    let mut by_slot: BTreeMap<(i64, i64), Vec<&Cell>> = BTreeMap::new();
    for c in school.cells.iter().filter(|c| c.source == "neis") {
        if s.my_subjects.iter().any(|m| m == &c.subject) && !s.my_classes_off.contains(&c.row_label) {
            by_slot.entry((c.weekday, c.period)).or_default().push(c);
        }
    }
    let mut cells: Vec<Cell> = Vec::new();
    let mut overlaps: Vec<String> = Vec::new();
    for ((wd, p), v) in by_slot {
        if v.len() > 1 {
            overlaps.push(format!(
                "{}요일 {p}교시: {}",
                ["", "월", "화", "수", "목", "금", "토"][wd as usize],
                v.iter().map(|c| format!("{} {}", c.row_label, c.subject)).collect::<Vec<_>>().join(", ")
            ));
        }
        let c = v[0];
        cells.push(Cell { weekday: wd, period: p, subject: c.subject.clone(), class_label: c.row_label.clone(), source: "neis".into(), ..Default::default() });
    }
    let n = timetable::replace_neis_cells(conn, year, sem, "personal", "", &cells, now)?;
    Ok((n, overlaps))
}

/// 나이스 시간표를 받아 학교·학급·개인 시간표에 넣는다 (4주를 받아 요일·교시마다 가장 많이 나온 과목)
pub fn sync_timetable(db: &Db, now: NaiveDateTime, get: Getter) -> AppResult<SyncReport> {
    let s = db.with(|c| settings::load(c))?;
    let Some(school) = s.school.clone().filter(|x| !x.code.is_empty()) else {
        return Ok(SyncReport { status: "no_school".into(), message: "학교를 먼저 정해 주세요.".into(), ..Default::default() });
    };
    let scope = school.code.clone();
    if s.neis_key.is_empty() {
        let msg = "시간표는 나이스 인증키가 있어야 받을 수 있습니다 (키가 없으면 나이스가 5칸만 줍니다).";
        db.with(|c| record(c, "timetable", &scope, "need_key", msg, now))?;
        return Ok(SyncReport { status: "need_key".into(), message: msg.into(), ..Default::default() });
    }
    let from = monday(now.date());
    let mut p = school_params(&school);
    p.push(("TI_FROM_YMD", ymd(from)));
    p.push(("TI_TO_YMD", ymd(from + Duration::days(27))));
    let rows = match fetch_all("misTimetable", &p, &s.neis_key, get) {
        Ok((rows, _)) => rows,
        Err(e) => {
            let msg = e.to_string();
            db.with(|c| record(c, "timetable", &scope, "error", &msg, now))?;
            return Ok(SyncReport { status: "error".into(), message: msg, ..Default::default() });
        }
    };
    let (sem, template) = weekly_template(&rows);
    let Some((year, sem)) = sem.or(Some(timetable::semester_of(now.date()))).filter(|_| !template.is_empty()) else {
        let msg = "이 기간에는 나이스 시간표가 없습니다 (방학 등). 받아 둔 시간표를 그대로 씁니다.";
        db.with(|c| record(c, "timetable", &scope, "empty", msg, now))?;
        return Ok(SyncReport { status: "empty".into(), message: msg.into(), ..Default::default() });
    };
    let classes: BTreeSet<String> = template.keys().map(|k| k.0.clone()).collect();
    let report = db.with(|c| {
        let tx = c.transaction()?;
        let school_cells: Vec<Cell> = template
            .iter()
            .map(|((cls, wd, p), subj)| Cell { row_label: cls.clone(), weekday: *wd, period: *p, subject: subj.clone(), source: "neis".into(), ..Default::default() })
            .collect();
        let n = timetable::replace_neis_cells(&tx, year, sem, "school", "", &school_cells, now)?;
        for cls in &classes {
            crate::students::find_or_create_class(&tx, cls, year)?;
            let cells: Vec<Cell> = school_cells.iter().filter(|c| &c.row_label == cls).map(|c| Cell { row_label: String::new(), ..c.clone() }).collect();
            timetable::replace_neis_cells(&tx, year, sem, "class", cls, &cells, now)?;
        }
        let (_, overlaps) = rebuild_personal(&tx, &s, year, sem, now)?;
        let msg = if overlaps.is_empty() { String::new() } else { format!("개인 시간표에 겹친 칸이 있습니다: {}", overlaps.join(" / ")) };
        record(&tx, "timetable", &scope, "ok", &msg, now)?;
        tx.commit()?;
        Ok(SyncReport { status: "ok".into(), message: msg, count: n, fetched_at: Some(util::fmt_dt(now)) })
    })?;
    db.with(|c| db::set_setting(c, "neis_classes", &serde_json::to_string(&classes.iter().collect::<Vec<_>>())?))?;
    Ok(report)
}

/// 학교 시간표의 과목 목록 (설정의 '내 과목' 고르기용)
pub fn school_subjects(conn: &Connection, year: i32, sem: u8) -> AppResult<Vec<String>> {
    let v = timetable::get_timetable(conn, year, sem, "school", "")?;
    let mut s: Vec<String> = v.cells.iter().map(|c| c.subject.clone()).filter(|x| !x.is_empty()).collect();
    s.sort();
    s.dedup();
    Ok(s)
}

// ───────────────────────── 학사일정 ─────────────────────────

/// 학사력 종류: 나이스 구분값(공휴일·휴업일)과 행사 이름 규칙으로 정한다. 방학 이름이 먼저다 (방학은 휴업일로 오기도 한다).
pub fn day_kind_for(name: &str, subtract: &str, rules: &DayRules) -> &'static str {
    let has = |v: &[String]| v.iter().any(|w| !w.is_empty() && name.contains(w.as_str()));
    if has(&rules.vacation) || (name.contains("방학") && !name.contains("방학식")) {
        return "vacation";
    }
    if has(&rules.holiday) {
        return "holiday";
    }
    if has(&rules.closure) {
        return "closure";
    }
    match subtract {
        "공휴일" => "holiday",
        "휴업일" => "closure",
        _ => "work",
    }
}

/// 행사 이름 끝의 ①②③ → 학년 태그 ("중간고사①" → ("중간고사", "1"))
pub fn split_grade_marks(name: &str) -> (String, String) {
    let mut grades: Vec<u32> = Vec::new();
    let mut base = name.trim_end().to_string();
    while let Some(c) = base.chars().last() {
        let n = match c {
            '①'..='⑥' => c as u32 - '①' as u32 + 1,
            _ => break,
        };
        grades.push(n);
        base.pop();
        base = base.trim_end().to_string();
    }
    grades.sort_unstable();
    grades.dedup();
    (base, grades.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(","))
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchoolDay {
    pub date: NaiveDate,
    pub name: String,
    pub kind: &'static str,
    pub grades: String,
}

pub fn schedule_days(rows: &[Row], rules: &DayRules, max_grade: u32) -> Vec<SchoolDay> {
    let mut out = Vec::new();
    for r in rows {
        let Some(date) = from_ymd(&field(r, "AA_YMD")) else { continue };
        let raw = field(r, "EVENT_NM");
        if raw.is_empty() {
            continue;
        }
        let (name, mut grades) = split_grade_marks(&raw);
        if grades.is_empty() {
            let keys = ["ONE_GRADE_EVENT_YN", "TW_GRADE_EVENT_YN", "THREE_GRADE_EVENT_YN", "FR_GRADE_EVENT_YN", "FIV_GRADE_EVENT_YN", "SIX_GRADE_EVENT_YN"];
            let ys: Vec<u32> = (1..=max_grade).filter(|g| field(r, keys[*g as usize - 1]) == "Y").collect();
            if !ys.is_empty() && ys.len() < max_grade as usize {
                grades = ys.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(",");
            }
        }
        let kind = day_kind_for(&name, &field(r, "SBTR_DD_SC_NM"), rules);
        out.push(SchoolDay { date, name, kind, grades });
    }
    out.sort_by(|a, b| a.date.cmp(&b.date).then(a.name.cmp(&b.name)));
    out.dedup();
    out
}

/// 같은 이름·종류·학년이 이어지는 날은 기간 하나로 묶는다 ("여름방학" 7/24~8/16).
/// 주말 사이는 3일 이내, 방학은 성탄절·설날 같은 공휴일과 주말이 끼어도 이어지도록 7일 이내.
pub fn group_days(days: &[SchoolDay]) -> Vec<(NaiveDate, NaiveDate, SchoolDay)> {
    let mut out: Vec<(NaiveDate, NaiveDate, SchoolDay)> = Vec::new();
    for d in days {
        if let Some(last) = out.iter_mut().rev().find(|(_, _, x)| x.name == d.name && x.kind == d.kind && x.grades == d.grades) {
            let max_gap = if d.kind == "vacation" { 7 } else { 3 };
            if d.date > last.1 && (d.date - last.1).num_days() <= max_gap {
                last.1 = d.date;
                continue;
            }
        }
        out.push((d.date, d.date, d.clone()));
    }
    out
}

fn is_saturday_closure(name: &str) -> bool {
    name.contains("토요휴업")
}

/// 나이스 학사일정을 받아 학사력에 넣는다 (교사가 추가·고친 학사력은 그대로, 교사가 지운 나이스 일정은 되살리지 않는다)
pub fn sync_schedule(db: &Db, now: NaiveDateTime, get: Getter) -> AppResult<SyncReport> {
    let s = db.with(|c| settings::load(c))?;
    let Some(school) = s.school.clone().filter(|x| !x.code.is_empty()) else {
        return Ok(SyncReport { status: "no_school".into(), message: "학교를 먼저 정해 주세요.".into(), ..Default::default() });
    };
    let year = db.with(|c| Ok(crate::students::active_school_year(c, now.date())))?;
    let scope = format!("{}:{year}", school.code);
    if s.neis_key.is_empty() {
        let msg = "학사일정은 나이스 인증키가 있어야 받을 수 있습니다 (키가 없으면 나이스가 5건만 줍니다).";
        db.with(|c| record(c, "schedule", &scope, "need_key", msg, now))?;
        return Ok(SyncReport { status: "need_key".into(), message: msg.into(), ..Default::default() });
    }
    let start = NaiveDate::from_ymd_opt(year, 3, 1).unwrap_or(now.date());
    let end = NaiveDate::from_ymd_opt(year + 1, 3, 1).map(|d| d - Duration::days(1)).unwrap_or(now.date());
    let mut p = school_params(&school);
    p.push(("AA_FROM_YMD", ymd(start)));
    p.push(("AA_TO_YMD", ymd(end)));
    let rows = match fetch_all("SchoolSchedule", &p, &s.neis_key, get) {
        Ok((rows, _)) => rows,
        Err(e) => {
            let msg = e.to_string();
            db.with(|c| record(c, "schedule", &scope, "error", &msg, now))?;
            return Ok(SyncReport { status: "error".into(), message: msg, ..Default::default() });
        }
    };
    let max_grade = if school.kind.contains("초등") { 6 } else { 3 };
    let days = schedule_days(&rows, &s.day_rules, max_grade);
    if days.is_empty() {
        let msg = "나이스에 이 학년도 학사일정이 아직 없습니다.";
        db.with(|c| record(c, "schedule", &scope, "empty", msg, now))?;
        return Ok(SyncReport { status: "empty".into(), message: msg.into(), ..Default::default() });
    }
    let groups = group_days(&days);
    db.with(|c| {
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM events WHERE ext_source='neis' AND layer='annual' AND start_date>=?1 AND start_date<=?2",
            params![util::fmt_d(start), util::fmt_d(end)],
        )?;
        let now_s = util::fmt_dt(now);
        let mut n = 0usize;
        for (a, b, d) in &groups {
            let key = format!("{}|{}", util::fmt_d(*a), d.name);
            // 교사가 고친(또는 지운) 같은 일정이 있으면 넣지 않는다
            let taken: bool = tx.query_row("SELECT 1 FROM events WHERE ext_key=?1", params![key], |_| Ok(true)).optional()?.unwrap_or(false);
            if taken {
                continue;
            }
            let hidden = is_saturday_closure(&d.name) as i64;
            tx.execute(
                "INSERT INTO events(layer, title, start_date, end_date, memo, source, ext_source, ext_key, day_kind, grades, hidden,
                    remind_enabled, created_at, updated_at)
                 VALUES ('annual', ?1, ?2, ?3, '', 'manual', 'neis', ?4, ?5, ?6, ?7, 0, ?8, ?8)",
                params![d.name, util::fmt_d(*a), util::fmt_d(*b), key, d.kind, d.grades, hidden, now_s],
            )?;
            n += 1;
        }
        record(&tx, "schedule", &scope, "ok", "", now)?;
        tx.commit()?;
        Ok(SyncReport { status: "ok".into(), message: String::new(), count: n, fetched_at: Some(now_s) })
    })
}

/// 학사력 종류 규칙을 바꾸면 이미 있는 학사력에도 다시 적용한다 (나이스 구분값이 없으니 이름 규칙만)
pub fn reclassify_days(conn: &Connection, rules: &DayRules) -> AppResult<usize> {
    let rows: Vec<(i64, String, String)> = {
        let mut st = conn.prepare("SELECT id, title, day_kind FROM events WHERE layer='annual'")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    let mut n = 0;
    for (id, title, cur) in rows {
        // 나이스가 '공휴일·휴업일'로 준 날은 이름 규칙에 없어도 그 종류를 유지한다
        let base = if cur == "holiday" || cur == "closure" { if cur == "holiday" { "공휴일" } else { "휴업일" } } else { "" };
        let k = day_kind_for(&title, base, rules);
        if k != cur {
            conn.execute("UPDATE events SET day_kind=?2 WHERE id=?1", params![id, k])?;
            n += 1;
        }
    }
    Ok(n)
}

// ───────────────────────── 상태와 자동 받기 ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct NeisStatus {
    pub school: Option<SchoolInfo>,
    pub has_key: bool,
    pub timetable: Option<FetchInfo>,
    pub schedule: Option<FetchInfo>,
    pub meals: Option<FetchInfo>,
    /// 학교 시간표의 과목 (내 과목 고르기)
    pub subjects: Vec<String>,
    /// 학교의 학급
    pub classes: Vec<String>,
}

pub fn status(conn: &Connection, now: NaiveDateTime) -> AppResult<NeisStatus> {
    let s = settings::load(conn)?;
    let (year, sem) = timetable::active_semester(conn, now.date());
    let school = s.school.clone().filter(|x| !x.code.is_empty());
    let (tt, sch, meals) = match &school {
        Some(sc) => (
            last_fetch(conn, "timetable", &sc.code)?,
            last_fetch(conn, "schedule", &format!("{}:{}", sc.code, crate::students::active_school_year(conn, now.date())))?,
            last_fetch(conn, "meals", &meal_scope(sc, monday(now.date())))?,
        ),
        None => (None, None, None),
    };
    let mut classes = saved_classes(conn);
    if classes.is_empty() {
        classes = timetable::get_timetable(conn, year, sem, "school", "")?.rows;
    }
    Ok(NeisStatus { school, has_key: !s.neis_key.is_empty(), timetable: tt, schedule: sch, meals, subjects: school_subjects(conn, year, sem)?, classes })
}

/// 앱을 켤 때: 학교를 정했으면 하루 한 번 시간표·학사일정을 새로 받고, 이번 주 급식을 받아 둔다. 바뀐 것이 있으면 true.
pub fn auto_sync(db: &Db, now: NaiveDateTime, get: Getter) -> bool {
    let Ok(s) = db.with(|c| settings::load(c)) else { return false };
    let Some(school) = s.school.clone().filter(|x| !x.code.is_empty()) else { return false };
    let today = util::fmt_d(now.date());
    let mut changed = false;
    let due = |what: &str, scope: &str| -> bool {
        db.with(|c| last_fetch(c, what, scope)).ok().flatten().is_none_or(|l| !l.fetched_at.starts_with(&today))
    };
    if !s.neis_key.is_empty() {
        if due("timetable", &school.code) {
            if let Ok(r) = sync_timetable(db, now, get) {
                changed |= r.status == "ok";
                crate::logging::info(&format!("나이스 시간표: {} {}", r.status, r.count));
            }
        }
        let year = db.with(|c| Ok(crate::students::active_school_year(c, now.date()))).unwrap_or(now.year());
        if due("schedule", &format!("{}:{year}", school.code)) {
            if let Ok(r) = sync_schedule(db, now, get) {
                changed |= r.status == "ok";
                crate::logging::info(&format!("나이스 학사일정: {} {}", r.status, r.count));
            }
        }
    }
    if saved_classes(&db.lock()).is_empty() && due("classes", &format!("{}:{}", school.code, now.year())) {
        let year = db.with(|c| Ok(crate::students::active_school_year(c, now.date()))).unwrap_or(now.year());
        let _ = sync_classes(db, &school, &s.neis_key, year, now, get);
    }
    if ensure_meal_week(db, &school, &s.neis_key, monday(now.date()), now, get).is_ok_and(|m| m.is_none()) {
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        util::parse_dt("2026-10-01T08:00:00").unwrap()
    }

    fn school() -> SchoolInfo {
        SchoolInfo { office_code: "Z99".into(), office_name: "OO교육청".into(), code: "9999999".into(), name: "OO중학교".into(), kind: "중학교".into(), ..Default::default() }
    }

    fn body(service: &str, total: usize, rows: Vec<Value>) -> String {
        serde_json::json!({ service: [ { "head": [ { "list_total_count": total }, { "RESULT": { "CODE": "INFO-000", "MESSAGE": "정상" } } ] }, { "row": rows } ] }).to_string()
    }

    /// 실제 나이스 확인: TN_NEIS_LIVE=1, TN_NEIS_SCHOOL=학교 이름 일부, TN_NEIS_CODE=학교 코드.
    /// 학교 이름·코드는 코드에 남기지 않고 실행할 때만 넣는다. 나이스에는 학교 이름·학교 코드만 보낸다.
    #[test]
    #[ignore]
    fn live_check() {
        if std::env::var("TN_NEIS_LIVE").map(|v| v != "1").unwrap_or(true) {
            return;
        }
        let (Ok(name), Ok(code)) = (std::env::var("TN_NEIS_SCHOOL"), std::env::var("TN_NEIS_CODE")) else {
            println!("TN_NEIS_SCHOOL·TN_NEIS_CODE 가 없어 건너뜀");
            return;
        };
        let found = search_schools(&name, "", &http_get).unwrap();
        println!("학교 찾기 {}곳", found.len());
        let sch = found.into_iter().find(|s| s.code == code).expect("학교 코드");
        let db = Db::open_in_memory().unwrap();
        db.with(|c| settings::update(c, |s| s.school = Some(sch.clone())).map(|_| ())).unwrap();
        let now = util::now();
        let v = meals_for(&db, now.date(), now, &http_get).unwrap();
        println!(
            "{} 급식 {}끼 ({}), 다음 급식일 {:?}, 안내 {:?}",
            v.date,
            v.meals.len(),
            v.meals.iter().map(|m| format!("{} 메뉴 {}개 {}", m.meal, m.dishes.len(), m.calories)).collect::<Vec<_>>().join(", "),
            v.next_date,
            v.message
        );
        println!("학급: {:?}", sync_classes(&db, &sch, "", 2026, now, &http_get).unwrap());
        let r = sync_timetable(&db, now, &http_get).unwrap();
        println!("시간표: {} — {}", r.status, r.message);
        let r = sync_schedule(&db, now, &http_get).unwrap();
        println!("학사일정: {} — {}", r.status, r.message);
    }
    #[test]
    fn parses_pages_and_errors() {
        let p = parse_page("schoolInfo", &body("schoolInfo", 2, vec![serde_json::json!({"SD_SCHUL_CODE": "1", "SCHUL_NM": "가"})])).unwrap();
        assert_eq!((p.total, p.rows.len()), (2, 1));
        let empty = parse_page("misTimetable", r#"{"RESULT":{"CODE":"INFO-200","MESSAGE":"해당하는 데이터가 없습니다."}}"#).unwrap();
        assert!(empty.rows.is_empty());
        let e = parse_page("misTimetable", r#"{"RESULT":{"CODE":"ERROR-290","MESSAGE":"인증키가 유효하지 않습니다."}}"#).unwrap_err();
        assert!(e.to_string().contains("인증키"));
        assert!(parse_page("x", "<html>").is_err());
    }

    #[test]
    fn urls_carry_only_school_codes_and_dates() {
        let u = url("misTimetable", &school_params(&school()), "", 1, 5);
        assert!(u.starts_with("https://open.neis.go.kr/hub/misTimetable?Type=json&pIndex=1&pSize=5"));
        assert!(u.contains("ATPT_OFCDC_SC_CODE=Z99") && u.contains("SD_SCHUL_CODE=9999999") && !u.contains("KEY="));
        assert!(url("schoolInfo", &[("SCHUL_NM", "OO중".into())], "abc", 2, 1000).contains("SCHUL_NM=OO%EC%A4%91&") == false);
        assert!(url("schoolInfo", &[("SCHUL_NM", "OO중".into())], "abc", 2, 1000).ends_with("SCHUL_NM=OO%EC%A4%91"));
    }

    #[test]
    fn without_key_only_first_page() {
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let get = |u: &str| -> AppResult<String> {
            calls.borrow_mut().push(u.to_string());
            Ok(body("misTimetable", 32, (0..5).map(|i| serde_json::json!({"PERIO": (i + 1).to_string()})).collect()))
        };
        let (rows, limited) = fetch_all("misTimetable", &[], "", &get).unwrap();
        assert_eq!((rows.len(), limited, calls.borrow().len()), (5, true, 1), "키가 없으면 한 번만 묻고 나머지를 쪼개 받지 않는다");
    }

    #[test]
    fn with_key_pages_until_total() {
        let get = |u: &str| -> AppResult<String> {
            let page: usize = u.split("pIndex=").nth(1).unwrap().split('&').next().unwrap().parse().unwrap();
            let n = if page == 1 { 1000 } else { 200 };
            Ok(body("SchoolSchedule", 1200, (0..n).map(|_| serde_json::json!({})).collect()))
        };
        let (rows, limited) = fetch_all("SchoolSchedule", &[], "KEY1", &get).unwrap();
        assert_eq!((rows.len(), limited), (1200, false));
    }

    #[test]
    fn dishes_and_allergens() {
        assert_eq!(parse_dish("육개장 (1.16)"), Dish { name: "육개장".into(), allergens: vec![1, 16] });
        assert_eq!(parse_dish("감자채피망볶음 (1.2.5.6.10.15.16)").allergens.len(), 7);
        assert_eq!(parse_dish("찰보리밥 "), Dish { name: "찰보리밥".into(), allergens: vec![] });
        assert_eq!(parse_dish("배추김치9.13."), Dish { name: "배추김치".into(), allergens: vec![9, 13] });
        assert_eq!(parse_dish("삼색파프리카/오이/쌈장 (5.6)").name, "삼색파프리카/오이/쌈장");
        assert_eq!(br_lines("밥 <br/>국 (5.6)<br/>"), vec!["밥", "국 (5.6)"]);
    }

    #[test]
    fn meals_cache_and_next_day() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| settings::update(c, |s| s.school = Some(school())).map(|_| ())).unwrap();
        let calls = std::cell::RefCell::new(0);
        let get = |u: &str| -> AppResult<String> {
            *calls.borrow_mut() += 1;
            // 이번 주: 수·목 급식 / 다음 주: 월 급식
            if u.contains("MLSV_FROM_YMD=20260928") {
                Ok(body(
                    "mealServiceDietInfo",
                    2,
                    vec![
                        serde_json::json!({"MLSV_YMD":"20260930","MMEAL_SC_NM":"중식","DDISH_NM":"밥 <br/>국 (5.6)","CAL_INFO":"700 Kcal"}),
                        serde_json::json!({"MLSV_YMD":"20261001","MMEAL_SC_NM":"중식","DDISH_NM":"카레 (2.5.6)<br/>김치 (9)","CAL_INFO":"750 Kcal"}),
                    ],
                ))
            } else if u.contains("MLSV_FROM_YMD=20261005") {
                Ok(body("mealServiceDietInfo", 1, vec![serde_json::json!({"MLSV_YMD":"20261005","MMEAL_SC_NM":"중식","DDISH_NM":"비빔밥 (1.5)"})]))
            } else {
                Ok(r#"{"RESULT":{"CODE":"INFO-200","MESSAGE":"없음"}}"#.into())
            }
        };
        let d = |s: &str| util::parse_d(s).unwrap();
        let v = meals_for(&db, d("2026-10-01"), now(), &get).unwrap();
        assert_eq!(v.meals.len(), 1);
        assert_eq!(v.meals[0].dishes[0], Dish { name: "카레".into(), allergens: vec![2, 5, 6] });
        assert_eq!(v.meals[0].calories, "750 Kcal");
        assert_eq!(*calls.borrow(), 1);
        // 같은 주는 다시 묻지 않는다
        let _ = meals_for(&db, d("2026-09-30"), now(), &get).unwrap();
        assert_eq!(*calls.borrow(), 1);
        // 금요일(10/2)은 급식 없음 → 다음 주 월요일 미리 보기
        let v = meals_for(&db, d("2026-10-02"), now(), &get).unwrap();
        assert!(v.meals.is_empty());
        assert_eq!(v.next_date.as_deref(), Some("2026-10-05"));
        assert_eq!(v.next_meals[0].dishes[0].name, "비빔밥");
        // 인터넷이 끊겨도 받아 둔 것은 보인다
        let offline = |_: &str| -> AppResult<String> { Err(AppError::msg("나이스에 연결하지 못했습니다.")) };
        let v = meals_for(&db, d("2026-10-01"), now() + Duration::days(1), &offline).unwrap();
        assert_eq!(v.meals.len(), 1);
        assert!(v.message.is_some());
        // 학교를 정하지 않았으면 안내
        let db2 = Db::open_in_memory().unwrap();
        assert!(meals_for(&db2, d("2026-10-01"), now(), &get).unwrap().school_name.is_none());
    }

    #[test]
    fn timetable_template_and_personal() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            settings::update(c, |s| {
                s.school = Some(school());
                s.my_subjects = vec!["정보".into()];
            })
            .map(|_| ())
        })
        .unwrap();
        // 1-1 월 1교시: 정보 3번, 국어 1번 → 정보 / 2-1 월 1교시: 정보 / 3-1 화 2교시: 정보
        let mut rows = Vec::new();
        for (date, subj) in [("20260928", "정보"), ("20261005", "정보"), ("20261012", "* 정보"), ("20261019", "국어")] {
            rows.push(serde_json::json!({"AY":"2026","SEM":"2","ALL_TI_YMD":date,"GRADE":"1","CLASS_NM":"1","PERIO":"1","ITRT_CNTNT":subj}));
        }
        rows.push(serde_json::json!({"AY":"2026","SEM":"2","ALL_TI_YMD":"20260928","GRADE":"2","CLASS_NM":"1","PERIO":"1","ITRT_CNTNT":"수학"}));
        rows.push(serde_json::json!({"AY":"2026","SEM":"2","ALL_TI_YMD":"20260929","GRADE":"3","CLASS_NM":"1","PERIO":"2","ITRT_CNTNT":"정보"}));
        let total = rows.len();
        let get = move |_: &str| -> AppResult<String> { Ok(body("misTimetable", total, rows.clone())) };
        // 키가 없으면 받지 않고 안내
        let r = sync_timetable(&db, now(), &get).unwrap();
        assert_eq!(r.status, "need_key");
        db.with(|c| settings::update(c, |s| s.neis_key = "TESTKEY".into()).map(|_| ())).unwrap();
        // 교사가 직접 입력한 칸은 그대로
        db.with(|c| timetable::set_cell(c, 2026, 2, "personal", "", &Cell { weekday: 5, period: 3, subject: "동아리".into(), ..Default::default() }, now())).unwrap();
        let r = sync_timetable(&db, now(), &get).unwrap();
        assert_eq!(r.status, "ok", "{}", r.message);
        let c = db.lock();
        let school_tt = timetable::get_timetable(&c, 2026, 2, "school", "").unwrap();
        let cell = |wd, p, row: &str| school_tt.cells.iter().find(|x| x.weekday == wd && x.period == p && x.row_label == row).map(|x| x.subject.clone());
        assert_eq!(cell(1, 1, "1-1").as_deref(), Some("정보"), "가장 많이 나온 과목");
        assert_eq!(cell(1, 1, "2-1").as_deref(), Some("수학"));
        let personal = timetable::get_timetable(&c, 2026, 2, "personal", "").unwrap();
        let mut got: Vec<(i64, i64, String, String)> = personal.cells.iter().map(|x| (x.weekday, x.period, x.class_label.clone(), x.subject.clone())).collect();
        got.sort();
        assert_eq!(got, vec![(1, 1, "1-1".into(), "정보".into()), (2, 2, "3-1".into(), "정보".into()), (5, 3, String::new(), "동아리".into())]);
        assert_eq!(timetable::get_timetable(&c, 2026, 2, "class", "3-1").unwrap().cells.len(), 1);
        assert_eq!(school_subjects(&c, 2026, 2).unwrap(), vec!["수학".to_string(), "정보".to_string()]);
        // 학급 빼기
        let mut s = settings::load(&c).unwrap();
        s.my_classes_off = vec!["3-1".into()];
        rebuild_personal(&c, &s, 2026, 2, now()).unwrap();
        assert_eq!(timetable::get_timetable(&c, 2026, 2, "personal", "").unwrap().cells.len(), 2);
        assert_eq!(saved_classes(&c), vec!["1-1", "2-1", "3-1"]);
    }

    #[test]
    fn day_kinds_grades_and_groups() {
        let r = DayRules::default();
        assert_eq!(day_kind_for("3·1절", "공휴일", &r), "holiday");
        assert_eq!(day_kind_for("추석", "공휴일", &r), "holiday", "구분값");
        assert_eq!(day_kind_for("재량휴업일", "", &r), "closure");
        assert_eq!(day_kind_for("여름방학", "휴업일", &r), "vacation", "방학이 먼저");
        assert_eq!(day_kind_for("중간고사", "해당없음", &r), "work");
        assert_eq!(split_grade_marks("진로체험①"), ("진로체험".to_string(), "1".to_string()));
        assert_eq!(split_grade_marks("현장학습①③"), ("현장학습".to_string(), "1,3".to_string()));
        let rows: Vec<Row> = [
            ("20260720", "여름방학", "휴업일"),
            ("20260721", "여름방학", "휴업일"),
            ("20260724", "여름방학", "휴업일"),
            ("20261224", "겨울방학", "휴업일"),
            ("20261228", "겨울방학", "휴업일"),
            ("20260725", "토요휴업일", "휴업일"),
            ("20261013", "중간고사②", "해당없음"),
            ("20261014", "중간고사②", "해당없음"),
        ]
        .iter()
        .map(|(d, n, k)| serde_json::json!({"AA_YMD": d, "EVENT_NM": n, "SBTR_DD_SC_NM": k, "ONE_GRADE_EVENT_YN":"Y","TW_GRADE_EVENT_YN":"Y","THREE_GRADE_EVENT_YN":"Y"}).as_object().unwrap().clone())
        .collect();
        let days = schedule_days(&rows, &r, 3);
        let g = group_days(&days);
        let names: Vec<(String, String, String, &str, String)> =
            g.iter().map(|(a, b, d)| (util::fmt_d(*a), util::fmt_d(*b), d.name.clone(), d.kind, d.grades.clone())).collect();
        assert!(names.contains(&("2026-07-20".into(), "2026-07-24".into(), "여름방학".into(), "vacation", String::new())), "{names:?}");
        assert!(names.contains(&("2026-10-13".into(), "2026-10-14".into(), "중간고사".into(), "work", "2".into())), "{names:?}");
        assert!(names.iter().any(|n| n.2 == "토요휴업일" && n.3 == "closure"));
        assert!(names.contains(&("2026-12-24".into(), "2026-12-28".into(), "겨울방학".into(), "vacation", String::new())), "성탄절·주말이 끼어도 방학은 하나: {names:?}");
    }

    #[test]
    fn schedule_sync_keeps_teacher_events() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            settings::update(c, |s| {
                s.school = Some(school());
                s.neis_key = "K".into();
            })
            .map(|_| ())
        })
        .unwrap();
        let rows = vec![
            serde_json::json!({"AA_YMD":"20261009","EVENT_NM":"한글날","SBTR_DD_SC_NM":"공휴일"}),
            serde_json::json!({"AA_YMD":"20261010","EVENT_NM":"토요휴업일","SBTR_DD_SC_NM":"휴업일"}),
            serde_json::json!({"AA_YMD":"20261015","EVENT_NM":"체육대회","SBTR_DD_SC_NM":"해당없음"}),
        ];
        let get = move |_: &str| -> AppResult<String> { Ok(body("SchoolSchedule", rows.len(), rows.clone())) };
        // 교사가 추가한 학사력
        db.with(|c| {
            c.execute(
                "INSERT INTO events(layer, title, start_date, end_date, source, day_kind, created_at, updated_at) VALUES ('annual','학부모 상담 주간','2026-10-19','2026-10-23','manual','work','x','x')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let r = sync_schedule(&db, now(), &get).unwrap();
        assert_eq!((r.status.as_str(), r.count), ("ok", 3));
        let titles = |db: &Db| -> Vec<(String, String, i64)> {
            db.with(|c| {
                let mut st = c.prepare("SELECT title, day_kind, hidden FROM events WHERE layer='annual' ORDER BY start_date")?;
                Ok(st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap()
        };
        let t = titles(&db);
        assert_eq!(t.len(), 4);
        assert!(t.contains(&("한글날".into(), "holiday".into(), 0)));
        assert!(t.contains(&("토요휴업일".into(), "closure".into(), 1)), "토요휴업일은 숨김");
        // 교사가 나이스 일정을 지우면(removed) 다시 받아도 되살리지 않는다
        db.with(|c| {
            c.execute("UPDATE events SET ext_source='removed', hidden=1, day_kind='' WHERE title='체육대회'", [])?;
            Ok(())
        })
        .unwrap();
        sync_schedule(&db, now(), &get).unwrap();
        let t = titles(&db);
        assert_eq!(t.iter().filter(|x| x.0 == "체육대회").count(), 1);
        assert!(t.iter().any(|x| x.0 == "학부모 상담 주간"), "교사가 추가한 학사력은 그대로");
    }
}
