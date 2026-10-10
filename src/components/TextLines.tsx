import { useMemo } from "react";
import { splitLines } from "../lib/lines";
import { splitHits } from "../lib/terms";

/** 검색어가 있으면 찾은 곳을 표시한다 */
export function Marked({ text, terms }: { text: string; terms?: string[] }) {
  if (!terms || terms.length === 0) return <>{text}</>;
  return (
    <>
      {splitHits(text, terms).map((p, i) => (p.hit ? <mark key={i} className="hit">{p.text}</mark> : <span key={i}>{p.text}</span>))}
    </>
  );
}

/** 기록 글을 줄 단위로 보여 준다: Enter 로 나눈 줄과 새 날짜·시각 앞에서 줄을 바꾸고, 줄 맨 앞 시각은 굵게 (원문은 그대로) */
export function TextLines({ text, terms }: { text: string; terms?: string[] }) {
  const lines = useMemo(() => splitLines(text), [text]);
  if (lines.length <= 1 && !lines[0]?.lead) return <Marked text={text.trim()} terms={terms} />;
  return (
    <>
      {lines.map((l, i) => (
        <span key={i} className="tline">
          {l.lead && <b className="tline-lead">{l.lead}</b>}
          <Marked text={l.rest} terms={terms} />
        </span>
      ))}
    </>
  );
}
