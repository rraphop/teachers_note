// 시간표 칸 색(15가지)과 학사력 날짜 색(10가지). DB에는 이름만 저장한다.

export interface PaletteColor {
  key: string;
  label: string;
  hex: string;
}

/** 시간표 칸 색: 칸을 꽉 채우므로 밝은 색부터 진한 색까지 고루 */
export const TT_PALETTE: PaletteColor[] = [
  { key: "red", label: "빨강", hex: "#f28b82" },
  { key: "orange", label: "주황", hex: "#f9ab63" },
  { key: "amber", label: "호박", hex: "#fdd663" },
  { key: "yellow", label: "노랑", hex: "#fff3a3" },
  { key: "lime", label: "연두", hex: "#ccf0a3" },
  { key: "green", label: "초록", hex: "#81c995" },
  { key: "teal", label: "청록", hex: "#7ed3c4" },
  { key: "cyan", label: "옥색", hex: "#a8e6f0" },
  { key: "sky", label: "하늘", hex: "#aecbfa" },
  { key: "blue", label: "파랑", hex: "#4a7fd9" },
  { key: "indigo", label: "남색", hex: "#3949ab" },
  { key: "purple", label: "보라", hex: "#c58af9" },
  { key: "pink", label: "분홍", hex: "#fbb4d7" },
  { key: "brown", label: "갈색", hex: "#8d6e63" },
  { key: "gray", label: "회색", hex: "#c4c7c5" },
];

/** 학사력 날짜·일정 글자 색 */
export const DAY_PALETTE: PaletteColor[] = [
  { key: "red", label: "빨강", hex: "#d93025" },
  { key: "orange", label: "주황", hex: "#e8710a" },
  { key: "amber", label: "호박", hex: "#b06000" },
  { key: "green", label: "초록", hex: "#188038" },
  { key: "teal", label: "청록", hex: "#00796b" },
  { key: "blue", label: "파랑", hex: "#1a73e8" },
  { key: "indigo", label: "남색", hex: "#3949ab" },
  { key: "purple", label: "보라", hex: "#8430ce" },
  { key: "gray", label: "회색", hex: "#80868b" },
  { key: "black", label: "검정", hex: "#1f2a26" },
];

export const DAY_KIND_LABEL: Record<string, string> = { holiday: "휴일", closure: "휴업일", vacation: "방학", work: "업무" };

/** 같은 날 여러 종류가 겹치면 이 순서로 색을 정한다 */
export const DAY_KIND_ORDER = ["holiday", "vacation", "closure", "work"] as const;

export function ttHex(key: string | undefined | null): string | null {
  return TT_PALETTE.find((c) => c.key === key)?.hex ?? null;
}

export function dayHex(key: string | undefined | null): string {
  return DAY_PALETTE.find((c) => c.key === key)?.hex ?? "#1f2a26";
}

/** 배경색이 진하면 흰 글자, 밝으면 검은 글자 */
export function textOn(hex: string): string {
  const h = hex.replace("#", "");
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  const lum = 0.2126 * lin(r ?? 0) + 0.7152 * lin(g ?? 0) + 0.0722 * lin(b ?? 0);
  return lum < 0.36 ? "#ffffff" : "#1f2a26";
}
