//! 문장·절 나누기와 "원문 근거" 측정.
//!
//! AI가 만든 문구가 원문에 실제로 있는 내용인지 확인할 때 글자 2-gram 포함률을 쓴다.
//! 원문에 없는 행동·평가를 지어낸 문구는 포함률이 낮아 걸러진다.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn slice<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }
    pub fn contains(&self, pos: usize) -> bool {
        self.start <= pos && pos < self.end
    }
    pub fn overlaps(&self, other: &Span) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    pub span: Span,
    pub sentence: usize,
}

static RE_DOTTED_DATE_END: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d{1,2}\.\s?\d{1,2}$").unwrap());

/// 공백만 제거한 앞뒤 정리 범위
fn trimmed(src: &str, start: usize, end: usize) -> Option<Span> {
    let s = &src[start..end];
    let lead = s.len() - s.trim_start().len();
    let trail = s.len() - s.trim_end().len();
    let (a, b) = (start + lead, end - trail);
    if a < b { Some(Span { start: a, end: b }) } else { None }
}

/// 문장 단위로 나눈다. 끝 문장부호는 문장에 포함한다.
pub fn sentences(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (i, &(pos, ch)) in chars.iter().enumerate() {
        let boundary = match ch {
            '\n' | '\r' | '。' | '!' | '?' | '！' | '？' => true,
            '.' => {
                let prev = i.checked_sub(1).map(|j| chars[j].1);
                let next = chars.get(i + 1).map(|x| x.1);
                let decimal = prev.is_some_and(|c| c.is_ascii_digit()) && next.is_some_and(|c| c.is_ascii_digit());
                // "10.15." 의 마지막 점은 날짜의 일부
                let dotted_date = RE_DOTTED_DATE_END.is_match(&text[start..pos]);
                // "10. 2." / "2026. 10. 2." 의 앞쪽 점(년·월 뒤)도 날짜의 일부 — 여기서 자르면 제목이 "10"이 된다
                let date_head = {
                    let before = &text[start..pos];
                    let nd = before.chars().rev().take_while(|c| c.is_ascii_digit()).count();
                    let after = text[pos + 1..].trim_start_matches(' ');
                    let na = after.chars().take_while(|c| c.is_ascii_digit()).count();
                    matches!(nd, 1 | 2 | 4) && (1..=2).contains(&na) && after[na..].starts_with('.')
                };
                !(decimal || dotted_date || date_head)
            }
            _ => false,
        };
        if boundary {
            let end = pos + ch.len_utf8();
            if let Some(sp) = trimmed(text, start, end) {
                // 문장부호만 남은 조각은 버린다
                if sp.slice(text).chars().any(|c| c.is_alphanumeric()) {
                    out.push(sp);
                }
            }
            start = end;
        }
    }
    if let Some(sp) = trimmed(text, start, text.len()) {
        if sp.slice(text).chars().any(|c| c.is_alphanumeric()) {
            out.push(sp);
        }
    }
    out
}

/// 절을 잇는 연결 어미 (왼쪽 절에 포함한다). "보고", "학교" 같은 명사가 잘리지 않도록 목록으로 한정한다.
static RE_CONNECTIVE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(알리고|알려주고|보내고|받고|드리고|전하고|전달하고|연락하고|확인하고|제출하고|준비하고|작성하고|처리하고|안내하고|공지하고|점검하고|회신하고|신청하고|정리하고|했고|하였고|되었고|됐고|했으며|하였으며|하며|였고|었고|았고|였으며|었으며|았으며)\s+",
    )
    .unwrap()
});

/// 문장을 절로 나눈다 (쉼표, 세미콜론, 연결 어미).
pub fn clauses(text: &str) -> Vec<Clause> {
    let mut out = Vec::new();
    for (si, sent) in sentences(text).into_iter().enumerate() {
        let s = sent.slice(text);
        let mut cuts: Vec<usize> = Vec::new();
        for (i, ch) in s.char_indices() {
            if matches!(ch, ',' | '，' | ';' | '；') {
                cuts.push(i + ch.len_utf8());
            }
        }
        for m in RE_CONNECTIVE.find_iter(s) {
            cuts.push(m.end());
        }
        cuts.sort_unstable();
        cuts.dedup();
        let mut prev = 0usize;
        for cut in cuts.into_iter().chain(std::iter::once(s.len())) {
            if cut <= prev {
                continue;
            }
            if let Some(sp) = trimmed(text, sent.start + prev, sent.start + cut) {
                if sp.slice(text).chars().any(|c| c.is_alphanumeric()) {
                    out.push(Clause { span: sp, sentence: si });
                }
            }
            prev = cut;
        }
    }
    out
}

fn norm(s: &str) -> Vec<char> {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn bigrams(chars: &[char]) -> Vec<(char, char)> {
    chars.windows(2).map(|w| (w[0], w[1])).collect()
}

/// 후보 문구의 글자 2-gram 중 원문에 있는 비율 (0.0 ~ 1.0)
pub fn coverage(candidate: &str, source: &str) -> f32 {
    let c = norm(candidate);
    let s = norm(source);
    if c.is_empty() {
        return 0.0;
    }
    if c.len() == 1 {
        return if s.contains(&c[0]) { 1.0 } else { 0.0 };
    }
    let sb: HashSet<(char, char)> = bigrams(&s).into_iter().collect();
    let cb = bigrams(&c);
    let hit = cb.iter().filter(|b| sb.contains(b)).count();
    hit as f32 / cb.len() as f32
}

/// 후보 문구가 원문에 없는 2-gram을 가진 위치를 표시하기 위한 목록 (학생부 자료 검토용)
pub fn novel_words(candidate: &str, source: &str) -> Vec<String> {
    let s = norm(source);
    let sb: HashSet<(char, char)> = bigrams(&s).into_iter().collect();
    let mut out = Vec::new();
    for word in candidate.split_whitespace() {
        let w = norm(word);
        if w.len() < 2 {
            continue;
        }
        let bg = bigrams(&w);
        let hit = bg.iter().filter(|b| sb.contains(b)).count();
        if (hit as f32) / (bg.len() as f32) < 0.5 {
            out.push(word.trim_matches(|c: char| !c.is_alphanumeric()).to_string());
        }
    }
    out
}

/// 원문의 절·문장 중 후보 문구와 가장 많이 겹치는 것을 찾는다.
pub fn best_source_span(candidate: &str, source: &str) -> Option<(Span, f32)> {
    let mut best: Option<(Span, f32)> = None;
    let mut consider = |sp: Span| {
        let seg = sp.slice(source);
        // 후보가 이 구간에 얼마나 들어 있는지와 구간이 후보에 얼마나 들어 있는지의 조화평균.
        // 긴 문장보다 딱 맞는 절이 높은 점수를 받는다.
        let a = coverage(candidate, seg);
        let b = coverage(seg, candidate);
        let score = if a + b > 0.0 { 2.0 * a * b / (a + b) } else { 0.0 };
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((sp, score));
        }
    };
    for c in clauses(source) {
        consider(c.span);
    }
    for s in sentences(source) {
        consider(s);
    }
    best
}

/// 문장부호·공백 정리
pub fn tidy(s: &str) -> String {
    let t = s.trim().trim_end_matches(['.', '。', ',', '，', ';']).trim();
    let mut out = String::with_capacity(t.len());
    let mut prev_space = false;
    for ch in t.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

/// 글자 수 기준으로 자른다.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str, spans: &[Span]) -> Vec<String> {
        spans.iter().map(|s| s.slice(src).to_string()).collect()
    }

    #[test]
    fn split_sentences() {
        let t = "오늘 3교시 2-1 홍길동 지각. 담임에게 알리고 금요일까지 보호자 연락.";
        assert_eq!(texts(t, &sentences(t)), vec!["오늘 3교시 2-1 홍길동 지각.", "담임에게 알리고 금요일까지 보호자 연락."]);
    }

    #[test]
    fn dotted_dates_do_not_split() {
        let t = "10.15. 교육장 방문. 점수 3.5점";
        assert_eq!(texts(t, &sentences(t)), vec!["10.15. 교육장 방문.", "점수 3.5점"]);
    }

    #[test]
    fn split_clauses() {
        let t = "오늘 3교시 2-1 홍길동 지각. 담임에게 알리고 금요일까지 보호자 연락.";
        let cs: Vec<String> = clauses(t).iter().map(|c| c.span.slice(t).to_string()).collect();
        assert_eq!(cs, vec!["오늘 3교시 2-1 홍길동 지각.", "담임에게 알리고", "금요일까지 보호자 연락."]);
        let t = "학생들이 수업 중 손장난을 했고 지적을 받았습니다.";
        let cs: Vec<String> = clauses(t).iter().map(|c| c.span.slice(t).to_string()).collect();
        assert_eq!(cs, vec!["학생들이 수업 중 손장난을 했고", "지적을 받았습니다."]);
    }

    #[test]
    fn nouns_are_not_split() {
        let t = "결과 보고 준비, 학교 행사 안내";
        let cs: Vec<String> = clauses(t).iter().map(|c| c.span.slice(t).to_string()).collect();
        assert_eq!(cs, vec!["결과 보고 준비,", "학교 행사 안내"]);
    }

    #[test]
    fn coverage_detects_invention() {
        let raw = "10월 15일 교육장 공감소통 방문";
        assert!(coverage("10월 15일 교육장 공감소통 방문 일정", raw) > 0.8);
        assert!(coverage("교육장에게 교육 방안을 제안한다", raw) < 0.4);
        assert!((coverage("교육장 공감소통 방문", raw) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn best_span_recovers_clause() {
        let raw = "오늘 3교시 2-1 홍길동 지각. 담임에게 알리고 금요일까지 보호자 연락.";
        let (sp, score) = best_source_span("보호자에게 연락하기", raw).unwrap();
        assert_eq!(sp.slice(raw), "금요일까지 보호자 연락.");
        assert!(score > 0.2);
    }

    #[test]
    fn novel_word_listing() {
        let src = "수업 중 발표를 적극적으로 함";
        let n = novel_words("수업 중 발표를 적극적으로 하며 리더십을 발휘함", src);
        assert!(n.iter().any(|w| w.contains("리더십")));
        assert!(!n.iter().any(|w| w.contains("발표")));
    }

    #[test]
    fn tidy_and_truncate() {
        assert_eq!(tidy("  보호자  연락. "), "보호자 연락");
        assert_eq!(truncate_chars("가나다라마", 3), "가나다…");
    }
}
