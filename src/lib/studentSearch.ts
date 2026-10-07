import type { Student } from "../types";

// 학생 명렬표 '이름·학급 찾기': 낱말마다 이름이나 학급에 맞아야 한다.
// 학급 표기: "1-1", "1학년"(1학년 모든 반), "1학년 1반"·"1학년1반", "1반", "3번" (기록의 학년 표기 규칙과 같게)

type Term = { kind: "class"; label: string } | { kind: "grade"; grade: string } | { kind: "room"; room: string } | { kind: "number"; no: number } | { kind: "text"; text: string };

function parse(query: string): Term[] {
  const q = query
    .trim()
    .replace(/(\d+)\s*학년\s*(\d+)\s*반/g, "$1-$2")
    .replace(/\s+/g, " ");
  if (!q) return [];
  return q.split(" ").map((t): Term => {
    if (/^\d+-\d+$/.test(t)) return { kind: "class", label: t };
    const g = t.match(/^(\d+)학년$/);
    if (g) return { kind: "grade", grade: g[1]! };
    const r = t.match(/^(\d+)반$/);
    if (r) return { kind: "room", room: r[1]! };
    const n = t.match(/^(\d+)번$/);
    if (n) return { kind: "number", no: Number(n[1]) };
    return { kind: "text", text: t };
  });
}

/** 검색어에 맞는 학생인지 (검색어가 비었으면 null — 거르지 않음) */
export function studentMatcher(query: string): ((s: Student) => boolean) | null {
  const terms = parse(query);
  if (terms.length === 0) return null;
  return (s) => {
    const cls = s.class_label ?? "";
    return terms.every((t) => {
      switch (t.kind) {
        case "class":
          return cls === t.label;
        case "grade":
          return cls.startsWith(`${t.grade}-`);
        case "room":
          return cls.endsWith(`-${t.room}`);
        case "number":
          return s.number === t.no;
        case "text":
          return s.name.includes(t.text) || cls.includes(t.text);
      }
    });
  };
}

/** 이름에서 검색어와 맞는 글자 범위 (강조 표시용) */
export function nameMarks(name: string, query: string): Array<[number, number]> {
  const out: Array<[number, number]> = [];
  for (const t of parse(query)) {
    if (t.kind !== "text") continue;
    let i = name.indexOf(t.text);
    while (i >= 0 && t.text) {
      out.push([i, i + t.text.length]);
      i = name.indexOf(t.text, i + t.text.length);
    }
  }
  return out.sort((a, b) => a[0] - b[0]);
}
