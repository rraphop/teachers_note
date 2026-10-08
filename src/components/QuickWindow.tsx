import { useEffect, useRef, useState } from "react";
import { api, errorText } from "../api";

/** 트레이 '기록하기' 작은 입력창 (3차 2번): 적고 Enter → 저장 후 닫힘. 저장한 기록은 평소처럼 AI가 정리한다 */
export function QuickWindow() {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    document.title = "교무수첩 — 빠른 기록";
    ref.current?.focus();
  }, []);
  const save = async () => {
    if (!text.trim() || busy) return;
    setBusy(true);
    setErr(null);
    try {
      await api.createNote(text);
      setText("");
      await api.closeQuickWindow();
    } catch (e) {
      setErr(errorText(e));
      setBusy(false);
    }
  };
  return (
    <div className="quick-win">
      <textarea
        ref={ref}
        value={text}
        disabled={busy}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          // 한글 입력 중(조합 중) Enter 는 글자 확정이라 저장하지 않는다
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            void save();
          }
          if (e.key === "Escape") void api.closeQuickWindow();
        }}
        placeholder="무엇이든 적어 두세요. 예) 오늘 3교시 2-1 홍길동 지각. 금요일까지 보호자 연락"
        aria-label="빠른 기록"
      />
      <p className="quick-hint">
        {err ? <span className="err">{err}</span> : <>Enter 저장 · Shift+Enter 줄바꿈 · Esc 닫기 — 원문은 바로 저장되고 AI가 정리합니다</>}
      </p>
    </div>
  );
}
