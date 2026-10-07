import { useEffect, useRef, useState } from "react";
import { api, errorText } from "../api";
import { useStore } from "../lib/store";
import { AiProgressRing } from "./AiProgressRing";

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

/** 모든 화면 아래에 붙어 있는 빠른 기록 입력창 (채팅 입력창처럼). 메뉴를 고르지 않고 바로 적는다. */
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
    el.style.height = `${Math.min(200, Math.max(26, el.scrollHeight))}px`;
    writeDraft(text);
  }, [text]);

  const save = async () => {
    const v = text.trim();
    if (!v || busy) return;
    setBusy(true);
    try {
      const note = await api.createNote(text);
      setText("");
      const cells = note.chips.filter((c) => c.kind === "timetable").length;
      const changes = note.chips.filter((c) => c.kind === "tt_change").length;
      const conflicts = note.chips.filter((c) => c.kind === "tt_conflict").length;
      const tt = cells + changes;
      if (note.status === "classified" && note.classified_by === "rule") {
        // 시간표 문장만 적은 기록: AI 없이 바로 시간표에 반영됨
        const parts = [cells > 0 ? `시간표 ${cells}칸을 채웠습니다.` : "", changes > 0 ? `하루짜리 시간표 변경 ${changes}건을 그날에 표시합니다.` : ""].filter(Boolean);
        if (conflicts > 0) parts.push(`다른 수업이 있는 ${conflicts}칸은 기록 카드에서 ‘바꾸기/그대로’를 골라 주세요.`);
        toast(`시간표에 반영했습니다. ${parts.join(" ")}`);
      } else {
        const ai = await api.aiAvailable().catch(() => false);
        const ttNote = tt + conflicts > 0 ? " 시간표 문장은 시간표에 바로 반영했습니다." : "";
        toast(
          ai
            ? `기록을 저장했습니다. AI가 정리하는 동안 계속 쓰셔도 됩니다.${ttNote}`
            : `기록을 저장했습니다. AI 모델이 없어 ‘미분류’로 두었습니다. 기록 카드에서 직접 분류할 수 있습니다.${ttNote}`,
        );
      }
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
      <div className="capture-box">
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
          placeholder="무엇이든 적어 두세요. 예) 오늘 3교시 2-1 홍길동 지각. 금요일까지 보호자 연락"
          aria-label="빠른 기록"
          rows={1}
        />
        <AiProgressRing />
        <button className="primary capture-send" onClick={() => void save()} disabled={busy || !text.trim()} title="기록 저장 (Ctrl+Enter)">
          {busy ? "저장 중…" : "기록"}
        </button>
      </div>
      <p className="capture-hint">
        <kbd>Ctrl</kbd>+<kbd>Enter</kbd> 저장 · <kbd>Ctrl</kbd>+<kbd>N</kbd> 여기로 이동 · 원문은 바로 저장되고, AI가 학생·할 일·일정·업무로 정리합니다
      </p>
    </div>
  );
}
