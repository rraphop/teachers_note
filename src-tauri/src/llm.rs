//! AI 요청 만들기: 분류 프롬프트·JSON 스키마, 학생부 초안 프롬프트.
//! 날짜 계산과 사실 검증은 AI에게 맡기지 않는다 (classify::guard_llm 이 한다).

use crate::analysis::{Area, DutyEntry};
use crate::classify::{self, Ctx, LlmOutput, Plan};
use crate::runtime::{FailKind, RuntimeError};
use crate::text;
use serde_json::{Value, json};
use std::time::Duration;

pub trait ChatBackend {
    fn chat(&self, body: Value, timeout: Duration) -> Result<String, RuntimeError>;
}

const SYSTEM: &str = "너는 중학교 교사의 업무 메모를 정리하는 도우미다. 메모에 적힌 사실만 옮긴다.

규칙
1. 메모에 없는 행동, 의도, 평가, 제안을 절대 만들지 않는다.
2. text에는 메모의 구절을 그대로 옮겨 적는다. 바꿔 쓰지 않는다.
3. 일정은 \"언제 무슨 일\" 그대로 적는다. 해석하거나 꾸미지 않는다.
4. area를 attendance(출결)로 하는 것은 결석·지각·조퇴·결과가 실제로 적힌 경우뿐이다. 수업 중 태도나 장난은 subject(교과)다.
5. 앞으로 해야 할 행동(연락, 제출, 확인, 준비 등)이나 기한이 적혀 있을 때만 kind를 task로 한다. 이미 한 일(물어봄, 통화함, 연락함, 안내함, 제출함, ~했음)은 task가 아니다. 한 메모에 섞여 있으면 앞으로 할 부분만 task로 한다.
6. 담당업무 목록과 관련된 내용이면 kind를 duty로 하고 duty에 그 번호를 쓴다. 관련이 없으면 duty는 0이다.
7. students에는 메모에 적힌 학생 이름만 쓴다. 이름이 없으면 빈 목록이다.
8. date에는 메모에 적힌 날짜 표현을 그대로 쓴다(예: \"다음 주 월요일까지\"). 없으면 \"\"이다.
9. summary는 메모를 40자 안으로 줄인 것이다. 메모에 없는 말은 쓰지 않는다.

kind: student(학생 관찰), task(할 일), event(일정), duty(담당업무 기록), memo(기타)
area: attendance(출결), subject(교과·수업), autonomous(자율·자치활동), career(진로), volunteer(봉사), club(동아리), behavior(행동특성), free_semester(자유학기: 주제선택·진로탐색·예술체육 활동), none(학생 관찰이 아님)";

const EXAMPLE_IN: &str = "어제 1-3 임꺽정 조퇴함. 다음 주 월요일까지 진단서 받기. 11월 3일 학부모 공개수업";
const EXAMPLE_OUT: &str = r#"{"summary":"1-3 임꺽정 조퇴, 진단서 받기, 학부모 공개수업","items":[{"kind":"student","text":"어제 1-3 임꺽정 조퇴함","students":["임꺽정"],"area":"attendance","duty":0,"date":"어제"},{"kind":"task","text":"다음 주 월요일까지 진단서 받기","students":["임꺽정"],"area":"none","duty":0,"date":"다음 주 월요일까지"},{"kind":"event","text":"11월 3일 학부모 공개수업","students":[],"area":"none","duty":0,"date":"11월 3일"}]}"#;

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string", "maxLength": 80 },
            "items": {
                "type": "array",
                "maxItems": 10,
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["student", "task", "event", "duty", "memo"] },
                        "text": { "type": "string", "maxLength": 200 },
                        "students": { "type": "array", "maxItems": 10, "items": { "type": "string", "maxLength": 20 } },
                        "area": { "type": "string", "enum": ["attendance", "subject", "autonomous", "career", "volunteer", "club", "behavior", "free_semester", "none"] },
                        "duty": { "type": "integer", "minimum": 0, "maximum": 50 },
                        "date": { "type": "string", "maxLength": 30 }
                    },
                    "required": ["kind", "text", "students", "area", "duty", "date"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["summary", "items"],
        "additionalProperties": false
    })
}

fn duty_block(duties: &[DutyEntry]) -> String {
    if duties.is_empty() {
        return "담당업무 목록: (등록된 업무 없음 — duty는 항상 0)".to_string();
    }
    let mut s = String::from("담당업무 목록 (번호. 업무명 — 세부업무):\n");
    for (i, d) in duties.iter().enumerate() {
        s.push_str(&format!("{}. {}", i + 1, d.name));
        let detail = d.detail.trim();
        if !detail.is_empty() {
            s.push_str(&format!(" — {}", text::truncate_chars(&detail.replace('\n', ", "), 200)));
        }
        let extra: Vec<&str> = [d.guide.trim()].into_iter().filter(|x| !x.is_empty()).collect();
        if !extra.is_empty() {
            s.push_str(&format!(", {}", extra.join(", ")));
        }
        if !d.keywords.is_empty() {
            s.push_str(&format!(", {}", d.keywords.join(", ")));
        }
        s.push('\n');
    }
    s
}

pub fn classify_body(memo: &str, duties: &[DutyEntry]) -> Value {
    let system = format!("{SYSTEM}\n\n{}", duty_block(duties));
    json!({
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": format!("메모:\n{EXAMPLE_IN}") },
            { "role": "assistant", "content": EXAMPLE_OUT },
            { "role": "user", "content": format!("메모:\n{memo}") }
        ],
        "temperature": 0.1,
        "top_p": 0.9,
        "seed": 7,
        "max_tokens": 1000,
        "cache_prompt": true,
        "chat_template_kwargs": { "enable_thinking": false },
        "response_format": { "type": "json_schema", "json_schema": { "name": "notebook", "strict": true, "schema": schema() } }
    })
}

/// 긴 기록은 문장 단위로 나누어 처리한다 (작은 모델의 문맥 한도 보호)
pub fn chunks(raw: &str, max_chars: usize) -> Vec<String> {
    if raw.chars().count() <= max_chars {
        return vec![raw.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for s in text::sentences(raw) {
        let piece = s.slice(raw);
        if !cur.is_empty() && cur.chars().count() + piece.chars().count() + 1 > max_chars {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        if piece.chars().count() > max_chars {
            // 한 문장이 너무 길면 글자 수로 자른다
            let chars: Vec<char> = piece.chars().collect();
            for c in chars.chunks(max_chars) {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur = c.iter().collect();
            }
        } else {
            cur.push_str(piece);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

pub const CHUNK_CHARS: usize = 1200;

pub struct ClassifyOutcome {
    pub plan: Plan,
    pub raw_output: String,
}

/// AI로 분류하고 안전 규칙으로 검증한 계획을 돌려준다.
pub fn classify_note(backend: &dyn ChatBackend, raw: &str, ctx: &Ctx, timeout: Duration) -> Result<ClassifyOutcome, RuntimeError> {
    let mut merged = LlmOutput::default();
    let mut raw_outputs: Vec<String> = Vec::new();
    for (i, part) in chunks(raw, CHUNK_CHARS).iter().enumerate() {
        let out = backend.chat(classify_body(part, ctx.duties), timeout)?;
        let parsed = classify::parse_llm_json(&out).ok_or_else(|| RuntimeError {
            kind: FailKind::Request,
            message: "AI 응답을 이해하지 못했습니다. 다시 분류하거나 직접 분류해 주세요.".into(),
        })?;
        if i == 0 {
            merged.summary = parsed.summary;
        }
        merged.items.extend(parsed.items);
        raw_outputs.push(out);
    }
    let plan = classify::guard_llm(raw, &merged, ctx);
    Ok(ClassifyOutcome { plan, raw_output: raw_outputs.join("\n") })
}

pub fn draft_body(area: Area, subject: &str, observations: &[String]) -> Value {
    let list = observations.iter().map(|o| format!("- {o}")).collect::<Vec<_>>().join("\n");
    let what = if subject.trim().is_empty() { area.label().to_string() } else { format!("{}({})", area.label(), subject.trim()) };
    let limit = area.char_limit().map(|n| format!("\n5. {n}자를 넘지 않는다.")).unwrap_or_default();
    let system = format!(
        "너는 교사의 학생부 기재를 돕는 도우미다. 아래 참고자료(교사의 관찰 기록)에 적힌 사실만 사용해 '{what}' 문장 초안을 쓴다.
규칙
1. 참고자료에 없는 활동, 성과, 성격, 평가, 칭찬을 덧붙이지 않는다.
2. 학생 이름은 쓰지 않는다.
3. 문장은 '~함', '~임', '~보임'처럼 끝낸다.
4. 기록이 적으면 짧게 쓴다. 초안 문장만 출력한다.{limit}"
    );
    json!({
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": format!("관찰 기록:\n{list}") }
        ],
        "temperature": 0.3,
        "top_p": 0.9,
        "seed": 7,
        "max_tokens": 600,
        "chat_template_kwargs": { "enable_thinking": false }
    })
}

/// 모델이 덧붙이는 머리말·따옴표·생각 표시를 걷어 낸다
pub fn clean_draft(s: &str) -> String {
    let mut t = s.to_string();
    if let (Some(a), Some(b)) = (t.find("<think>"), t.find("</think>")) {
        if b > a {
            t.replace_range(a..b + "</think>".len(), "");
        }
    }
    let t = t.trim().trim_matches(['"', '“', '”', '`']).trim();
    let t = t.strip_prefix("초안:").unwrap_or(t).trim();
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::ItemKind;
    use chrono::NaiveDate;

    struct Fake(String);
    impl ChatBackend for Fake {
        fn chat(&self, body: Value, _t: Duration) -> Result<String, RuntimeError> {
            // 요청에 JSON 스키마와 생각 모드 끄기가 들어 있어야 한다
            assert_eq!(body["response_format"]["type"], "json_schema");
            assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
            Ok(self.0.clone())
        }
    }

    #[test]
    fn classify_with_fake_backend() {
        let raw = "10월 15일 교육장 공감소통 방문";
        let fake = Fake(r#"{"summary":"교육장 방문","items":[{"kind":"event","text":"10월 15일 교육장 공감소통 방문","students":[],"area":"none","duty":0,"date":"10월 15일"}]}"#.into());
        let ctx = Ctx { today: NaiveDate::from_ymd_opt(2026, 9, 29).unwrap(), roster: &[], duties: &[] };
        let out = classify_note(&fake, raw, &ctx, Duration::from_secs(5)).unwrap();
        assert_eq!(out.plan.items.len(), 1);
        assert_eq!(out.plan.items[0].kind, ItemKind::Event);
        assert_eq!(out.plan.items[0].date.as_deref(), Some("2026-10-15"));
    }

    #[test]
    fn garbage_output_is_an_error_not_a_crash() {
        let fake = Fake("모르겠습니다".into());
        let ctx = Ctx { today: NaiveDate::from_ymd_opt(2026, 9, 29).unwrap(), roster: &[], duties: &[] };
        let e = match classify_note(&fake, "내일 회의", &ctx, Duration::from_secs(5)) {
            Err(e) => e,
            Ok(_) => panic!("should fail"),
        };
        assert_eq!(e.kind, FailKind::Request);
    }

    #[test]
    fn chunking_keeps_all_text() {
        let raw = "가나다라. ".repeat(500);
        let parts = chunks(&raw, 300);
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|p| p.chars().count() <= 300));
        let joined: String = parts.join("").chars().filter(|c| !c.is_whitespace()).collect();
        let orig: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(joined, orig);
    }

    #[test]
    fn draft_cleanup() {
        assert_eq!(clean_draft("<think>음</think>\n\"실험에 성실히 참여함.\""), "실험에 성실히 참여함.");
    }
}
