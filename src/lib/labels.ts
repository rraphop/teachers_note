import type { Area, ItemKind, NoteStatus } from "../types";

/** 관찰 기록 영역 (학생 상세·수정 창). 학생부 자료 화면의 탭 7개는 RECORD_TABS */
export const AREA_ORDER: Area[] = ["attendance", "subject", "autonomous", "career", "volunteer", "club", "behavior", "free_semester", "daily"];

/** 학생부 자료 영역 7개 (창체 자율·자치, 교과, 행특, 동아리, 진로, 자유학기, 일상생활) */
export const RECORD_TABS: Area[] = ["autonomous", "subject", "behavior", "club", "career", "free_semester", "daily"];

export const AREA_LABEL: Record<Area, string> = {
  attendance: "출결특기사항",
  subject: "교과특기사항",
  autonomous: "창체 — 자율·자치",
  career: "창체 — 진로",
  volunteer: "창체 — 봉사",
  club: "창체 — 동아리",
  behavior: "행동특성 및 종합의견",
  free_semester: "자유학기활동",
  daily: "일상생활 활동상황",
  none: "영역 미지정",
};

export const AREA_SHORT: Record<Area, string> = {
  attendance: "출결",
  subject: "교과",
  autonomous: "자율·자치",
  career: "진로",
  volunteer: "봉사",
  club: "동아리",
  behavior: "행특",
  free_semester: "자유학기",
  daily: "일상생활",
  none: "미지정",
};

export function areaLabel(a: string | null | undefined): string {
  return AREA_LABEL[(a ?? "none") as Area] ?? "영역 미지정";
}

export function areaShort(a: string | null | undefined): string {
  return AREA_SHORT[(a ?? "none") as Area] ?? "미지정";
}

export const KIND_LABEL: Record<ItemKind, string> = {
  student: "학생 관찰",
  task: "할 일",
  event: "일정",
  duty: "업무 기록",
  memo: "기타",
};

export const LAYER_LABEL: Record<string, string> = {
  annual: "학사력",
  monthly: "월간 계획",
  weekly: "주간 계획",
  note: "기록 일정",
  task: "할 일",
  duty: "업무",
  birthday: "생일",
  tt_change: "시간표 변경",
};

export const LAYER_HELP: Record<string, string> = {
  birthday: "학급명렬표의 학생 생일 — 연간·월간·주간 표에 보입니다",
  annual: "연간·월간·주간 표에 모두 보입니다",
  monthly: "월간·주간 표에 보입니다",
  weekly: "주간 표에만 보입니다",
  note: "기록에서 찾은 일정 — 월간·주간 표에 보입니다",
  tt_change: "기록에 적은 하루짜리 시간표 변경(보강·교체·자습·휴강) — 월간·주간 표에 보입니다",
};

export const STATUS_LABEL: Record<NoteStatus, string> = {
  pending: "분류 대기",
  processing: "AI 정리 중",
  classified: "AI 정리됨",
  failed: "미분류",
  manual: "직접 정리함",
};

export const OFFSET_CHOICES = [7, 3, 2, 1, 0];

export function offsetLabel(o: number): string {
  return o === 0 ? "당일" : `${o}일 전`;
}

export function sourceLabel(source: string): string {
  return source === "manual" ? "직접" : "AI";
}
