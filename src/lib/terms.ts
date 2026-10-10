// 기록 화면 검색어 나누기와 찾은 곳 표시
// - 띄어 쓴 낱말은 낱말마다 나눈다: 모든 낱말이 들어 있는 기록을 찾는다 (붙어 있지 않아도)
// - "따옴표"로 묶은 말과 '구 그대로' 방식은 띄어쓰기까지 그대로 한 덩어리로 찾는다

export type TermMode = "words" | "phrase";

export function parseTerms(q: string, mode: TermMode): string[] {
  const text = q.trim();
  if (!text) return [];
  if (mode === "phrase") return [text.replace(/["“”]/g, "").replace(/\s+/g, " ").trim()].filter(Boolean);
  const out: string[] = [];
  const re = /["“”]([^"“”]*)["“”]?|(\S+)/g;
  for (const m of text.matchAll(re)) {
    const t = (m[1] ?? m[2] ?? "").replace(/\s+/g, " ").trim();
    if (t && !out.includes(t)) out.push(t);
  }
  return out.slice(0, 10);
}

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** 글을 찾은 곳(hit)과 아닌 곳으로 나눈다. 긴 낱말을 먼저 맞춘다. */
export function splitHits(text: string, terms: string[]): Array<{ text: string; hit: boolean }> {
  const ts = terms.filter(Boolean).sort((a, b) => b.length - a.length);
  if (ts.length === 0 || !text) return [{ text, hit: false }];
  const re = new RegExp(`(${ts.map(escapeRe).join("|")})`, "gi");
  return text
    .split(re)
    .filter((p) => p !== "")
    .map((p) => ({ text: p, hit: ts.some((t) => t.toLowerCase() === p.toLowerCase()) }));
}
