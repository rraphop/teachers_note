import { useEffect, useRef, useState } from "react";
import { api, errorText } from "../api";
import { useStore } from "../lib/store";

const DRAFT_KEY = "tn.capture.draft";

function readDraft(): string {
  try {
    return window.localStorage.getItem(DRAFT_KEY) ?? "";
  } catch {
    return "";
  }
}
function writeDraft(v: string) {
  try {
    if (v) window.localStorage.setItem(DRAFT_KEY, v);
    else window.localStorage.removeItem(DRAFT_KEY);
  } catch {
    /* 저장 못 해도 입력은 계속 된다 */
  }
}

/** 모든 화면 위에 있는 빠른 기록 입력창. 메뉴를 고르지 않고 바로 적는다. */
export function QuickCapture() {
  const { toast, bump } = useStore();
  const [text, setText] = useState(readDraft);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === "n" || e.key === "N" || e.key === "ㅜ")) {
        e.preventDefault();
        ref.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(220, Math.max(52, el.scrollHeight))}px`;
    writeDraft(text);
  }, [text]);

  const save = async () => {
    const v = text.trim();
    if (!v || busy) return;
    setBusy(true);
    try {
      await api.createNote(text);
      setText("");
      const ai = await api.aiAvailable().catch(() => false);
      toast(
        ai
          ? "기록을 저장했습니다. AI가 정리하는 동안 계속 쓰셔도 됩니다."
          : "기록을 저장했습니다. AI 모델이 없어 ‘미분류’로 두었습니다. 기록 카드에서 직접 분류할 수 있습니다.",
      );
      bump();
      ref.current?.focus();
    } catch (e) {
      toast(`저장하지 못했습니다: ${errorText(e)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="capture">
      <textarea
        ref={ref}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            void save();
          }
        }}
        placeholder="무엇이든 적어 두세요. 예) 오늘 3교시 2-1 김민수 지각. 금요일까지 보호자 연락"
        aria-label="빠른 기록"
        rows={1}
      />
      <div className="capture-side">
        <button className="primary" onClick={() => void save()} disabled={busy || !text.trim()}>
          {busy ? "저장 중…" : "기록"}
        </button>
        <small>Ctrl+Enter</small>
      </div>
    </div>
  );
}
