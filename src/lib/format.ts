const WD = ["일", "월", "화", "수", "목", "금", "토"];

export function pad(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

/** "YYYY-MM-DD" → 지역 시간 Date */
export function parseDate(s: string): Date {
  const [y, m, d] = s.slice(0, 10).split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
}

export function isoDate(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function todayISO(): string {
  return isoDate(new Date());
}

export function addDays(d: Date, n: number): Date {
  const x = new Date(d);
  x.setDate(x.getDate() + n);
  return x;
}

export function addMonths(d: Date, n: number): Date {
  const x = new Date(d.getFullYear(), d.getMonth() + n, 1);
  const last = new Date(x.getFullYear(), x.getMonth() + 1, 0).getDate();
  x.setDate(Math.min(d.getDate(), last));
  return x;
}

/** 월요일 시작 주 */
export function startOfWeek(d: Date): Date {
  const x = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const diff = (x.getDay() + 6) % 7;
  return addDays(x, -diff);
}

export function weekdayKo(d: Date): string {
  return WD[d.getDay()] ?? "";
}

/** "10월 2일 (금)" */
export function fmtDate(s: string | null | undefined): string {
  if (!s) return "";
  const d = parseDate(s);
  return `${d.getMonth() + 1}월 ${d.getDate()}일 (${weekdayKo(d)})`;
}

/** "2026년 10월 2일 (금)" */
export function fmtDateLong(s: string): string {
  const d = parseDate(s);
  return `${d.getFullYear()}년 ${d.getMonth() + 1}월 ${d.getDate()}일 (${weekdayKo(d)})`;
}

/** "오후 3:05" */
export function fmtTime(t: string | null | undefined): string {
  if (!t) return "";
  const [hh, mm] = t.split(":").map(Number);
  const h = hh ?? 0;
  const ap = h < 12 ? "오전" : "오후";
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return `${ap} ${h12}:${pad(mm ?? 0)}`;
}

/** "9월 29일 (화) 오전 10:15" */
export function fmtDateTime(s: string | null | undefined): string {
  if (!s) return "";
  return `${fmtDate(s.slice(0, 10))} ${fmtTime(s.slice(11, 16))}`;
}

/** 오늘 기준 상대 표현: "오늘", "내일", "3일 남음", "2일 지남" */
export function relDays(date: string, today: string = todayISO()): string {
  const diff = Math.round((parseDate(date).getTime() - parseDate(today).getTime()) / 86400000);
  if (diff === 0) return "오늘";
  if (diff === 1) return "내일";
  if (diff === 2) return "모레";
  if (diff === -1) return "어제";
  if (diff < 0) return `${-diff}일 지남`;
  return `${diff}일 남음`;
}

export function daysBetween(a: string, b: string): number {
  return Math.round((parseDate(b).getTime() - parseDate(a).getTime()) / 86400000);
}

export function bytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(n >= 1e10 ? 0 : 1)}GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(0)}MB`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(0)}KB`;
  return `${n}B`;
}

export function gib(n: number): string {
  return `${Math.round(n / 1073741824)}GB`;
}

export function percent(a: number, b: number): number {
  if (b <= 0) return 0;
  return Math.max(0, Math.min(100, Math.floor((a / b) * 100)));
}
