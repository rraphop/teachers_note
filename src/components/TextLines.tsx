import { useMemo } from "react";
import { splitLines } from "../lib/lines";

/** 기록 글을 줄 단위로 보여 준다: Enter 로 나눈 줄과 새 날짜·시각 앞에서 줄을 바꾸고, 줄 맨 앞 시각은 굵게 (원문은 그대로) */
export function TextLines({ text }: { text: string }) {
  const lines = useMemo(() => splitLines(text), [text]);
  if (lines.length <= 1 && !lines[0]?.lead) return <>{text.trim()}</>;
  return (
    <>
      {lines.map((l, i) => (
        <span key={i} className="tline">
          {l.lead && <b className="tline-lead">{l.lead}</b>}
          {l.rest}
        </span>
      ))}
    </>
  );
}
