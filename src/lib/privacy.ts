import { useEffect, useState } from "react";

// 연락처·주소 가리기 (학생 명렬표의 '연락처·주소 가리기'를 켜면 기록 화면·오늘의 기록에서도 가린다)

const KEY = "tn.roster.mask";
const EVENT = "tn-mask-changed";

const PHONE = /01[016789][-.\s]?\d{3,4}[-.\s]?\d{4}|0(?:2|[3-6]\d)-\d{3,4}-\d{4}/g;
const ADDRESS =
  /(?:(?:서울|부산|대구|인천|광주|대전|울산|세종|경기|강원|충북|충남|전북|전남|경북|경남|제주)[가-힣]*|[가-힣A-Za-z○]+도)\s+[가-힣A-Za-z○0-9]+(?:시|군|구)(?:\s+[가-힣A-Za-z○0-9]+(?:구|읍|면|동|리))*(?:\s+[가-힣A-Za-z○0-9]+(?:로|길))?(?:\s*\d+(?:-\d+)?(?:번지)?)?/g;
const ADDRESS_AFTER_WORD = /(주소\s*(?:는|은|:)?\s*)([^\n.,]+)/g;

export function maskPhone(v: string): string {
  const d = v.replace(/\D/g, "");
  return d.length >= 4 ? `●●●-●●●●-${d.slice(-4)}` : "●●●●";
}

export function maskAddress(v: string): string {
  const head = v.trim().split(/\s+/).slice(0, 2).join(" ");
  return head ? `${head} ●●●` : "";
}

/** 글 속 전화번호·주소를 가린다 */
export function maskText(s: string): string {
  return s
    .replace(PHONE, (m) => maskPhone(m))
    .replace(ADDRESS_AFTER_WORD, (_, w: string, v: string) => w + maskAddress(v))
    .replace(ADDRESS, (m) => maskAddress(m));
}

export function getMask(): boolean {
  try {
    return window.localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function setMask(on: boolean) {
  try {
    window.localStorage.setItem(KEY, on ? "1" : "0");
  } catch {
    /* 저장 못 해도 동작 */
  }
  window.dispatchEvent(new CustomEvent(EVENT, { detail: on }));
}

/** 가리기 켬/끔 (다른 화면에서 바꿔도 바로 따라간다) */
export function useMaskContacts(): boolean {
  const [on, setOn] = useState(getMask);
  useEffect(() => {
    const f = (e: Event) => setOn(!!(e as CustomEvent).detail);
    window.addEventListener(EVENT, f);
    return () => window.removeEventListener(EVENT, f);
  }, []);
  return on;
}
