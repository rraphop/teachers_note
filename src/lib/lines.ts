// 기록 글을 화면에 줄 단위로 나눈다 (3차 4번). 원문은 고치지 않고 표시만 바꾼다.
// - 사용자가 Enter 로 나눈 줄은 그대로
// - 한 줄 안에서 새 날짜·시각("10.6. 오전 9:40", "09:46", "10월 6일")이 나오면 그 앞에서 줄을 바꾼다
//   (다만 "10월 6일까지", "9:30~10:30" 처럼 기한·범위로 쓰인 것은 나누지 않는다)
// - 줄 맨 앞의 날짜·시각은 굵게 보이도록 따로 돌려준다

export interface TextLine {
  /** 줄 맨 앞의 날짜·시각 (굵게) */
  lead: string | null;
  rest: string;
}

const MONTH = "(?:1[0-2]|0?[1-9])";
const DAY = "(?:3[01]|[12]\\d|0?[1-9])";
const WEEKDAY = "(?:\\s?\\(?[월화수목금토일](?:요일)?\\)?)?";
// 날짜: "10.6." "10. 6." "10월 6일" "10/6" (점 형식은 끝의 점까지 있어야 소수 "3.5"와 헷갈리지 않는다)
const DATE = `(?:${MONTH}\\.\\s?${DAY}\\.|${MONTH}월\\s?${DAY}일|${MONTH}\\/${DAY}(?![\\d/]))${WEEKDAY}`;
// 시각: "09:46" "오전 9:40" "오후 3시" "오후 3시 30분" "오후 3시 반"
const TIME = "(?:(?:오전|오후)\\s?)?(?:[01]?\\d|2[0-3]):[0-5]\\d|(?:오전|오후)\\s?(?:1[0-2]|0?[1-9])시(?:\\s?[0-5]?\\d분|\\s?반)?";
// 표시 하나 = 날짜(+시각) 또는 시각
const MARK = new RegExp(`(?<![\\d.:/])(?:${DATE}(?:\\s+(?:${TIME}))?|${TIME})(?![\\d:])`, "g");
// 뒤에 이런 말이 붙으면 기한·범위라서 줄을 나누지 않는다
const DEADLINE_AFTER = /^\s*(?:까지|부터|마감|전에|전까지|이내|안에|사이|중에|에서|~|-|–)/;
// 앞에 이런 표시가 있으면 범위의 뒷부분
const RANGE_BEFORE = /(?:~|-|–|부터)\s*$/;

/** 한 줄 안에서 줄을 바꿀 자리 (표시가 시작하는 위치들) */
function breakPoints(line: string): number[] {
  const out: number[] = [];
  MARK.lastIndex = 0;
  for (let m = MARK.exec(line); m; m = MARK.exec(line)) {
    const at = m.index;
    const before = line.slice(0, at);
    const after = line.slice(at + m[0].length);
    if (!before.trim()) continue; // 줄 맨 앞
    if (DEADLINE_AFTER.test(after) || RANGE_BEFORE.test(before)) continue;
    out.push(at);
  }
  return out;
}

/** 줄 맨 앞이 날짜·시각이면 그 부분 */
function leadOf(line: string): TextLine {
  MARK.lastIndex = 0;
  const m = MARK.exec(line);
  if (m && m.index === 0 && !DEADLINE_AFTER.test(line.slice(m[0].length))) return { lead: m[0], rest: line.slice(m[0].length) };
  return { lead: null, rest: line };
}

export function splitLines(text: string): TextLine[] {
  const out: TextLine[] = [];
  for (const raw of text.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    const points = breakPoints(line);
    let start = 0;
    for (const p of [...points, line.length]) {
      const part = line.slice(start, p).trim();
      if (part) out.push(leadOf(part));
      start = p;
    }
  }
  return out;
}
