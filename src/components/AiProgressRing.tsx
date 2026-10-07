import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { api, errorText } from "../api";
import { useStore } from "../lib/store";

// 입력창 [기록] 버튼 왼쪽의 'AI 정리 중' 원형 게이지 (2차 수정목록 6번).
// LLM 은 정확한 진행률을 모르므로 단계로 센다: 원문 저장 10% → 명렬표 이름 대조 25% → AI 분류 25~85%(받은 글자 수로 짐작)
// → 안전 규칙 점검 95% → 메뉴 배치 100%. AI 엔진을 켜야 하면 "AI 켜는 중".

interface Progress {
  note_id: number;
  stage: "saved" | "names" | "engine" | "ai" | "guard" | "done" | "failed";
  percent: number;
  waiting: number;
}

const R = 15;
const LEN = 2 * Math.PI * R;

export function AiProgressRing() {
  const { navigate, toast, bump } = useStore();
  const [p, setP] = useState<Progress | null>(null);
  const [menu, setMenu] = useState(false);
  const hideTimer = useRef<number | null>(null);

  useEffect(() => {
    let alive = true;
    let un: (() => void) | null = null;
    void listen<Progress>("classify-progress", (e) => {
      const v = e.payload;
      if (hideTimer.current !== null) {
        window.clearTimeout(hideTimer.current);
        hideTimer.current = null;
      }
      setP((cur) => {
        // 엔진 켜는 중 표시는 AI 분류가 시작될 때까지 유지
        if (cur && cur.note_id === v.note_id && cur.stage === "engine" && v.stage === "names") return cur;
        return v;
      });
      if (v.stage === "done") {
        // 끝나면 잠깐 체크 표시 후 사라짐 (남은 건이 있으면 다음 건이 곧 이어진다)
        hideTimer.current = window.setTimeout(() => setP((cur) => (cur && cur.note_id === v.note_id && cur.stage === "done" ? null : cur)), 1600);
      }
    }).then((u) => (alive ? (un = u) : u()));
    return () => {
      alive = false;
      un?.();
      if (hideTimer.current !== null) window.clearTimeout(hideTimer.current);
    };
  }, []);

  if (!p) return null;
  const failed = p.stage === "failed";
  const done = p.stage === "done";
  const starting = p.stage === "engine";
  const pct = Math.max(0, Math.min(100, Math.round(p.percent)));
  const label = failed ? "AI 정리 실패" : done ? "AI 정리 끝" : starting ? "AI 켜는 중" : `AI 정리 중 ${pct}%`;
  const more = p.waiting > 0 ? `${p.waiting}건 남음` : "";
  const open = () => {
    if (failed) {
      setMenu((m) => !m);
      return;
    }
    navigate("notes", { noteId: p.note_id });
  };
  const retry = async () => {
    setMenu(false);
    try {
      await api.reclassifyNote(p.note_id);
      setP({ ...p, stage: "saved", percent: 10 });
      toast("AI가 다시 정리합니다.");
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <div className={`ai-ring ${failed ? "failed" : ""} ${done ? "done" : ""} ${starting ? "starting" : ""}`}>
      <button className="ai-ring-btn" onClick={open} title={`${label}${more ? ` · ${more}` : ""} — 눌러서 ${failed ? "다시 시도" : "기록 보기"}`} aria-label={`${label}${more ? `, ${more}` : ""}`}>
        <svg width="38" height="38" viewBox="0 0 38 38" aria-hidden>
          <circle cx="19" cy="19" r={R} className="ring-bg" />
          <circle
            cx="19"
            cy="19"
            r={R}
            className="ring-fg"
            strokeDasharray={LEN}
            strokeDashoffset={starting ? LEN * 0.75 : LEN * (1 - (failed ? Math.max(pct, 100) : pct) / 100)}
            transform="rotate(-90 19 19)"
          />
          <text x="19" y="19" className="ring-text" textAnchor="middle" dominantBaseline="central">
            {failed ? "!" : done ? "✓" : starting ? "…" : pct}
          </text>
        </svg>
      </button>
      <span className="ai-ring-label">
        {label}
        {more && <small> · {more}</small>}
      </span>
      {menu && failed && (
        <div className="ai-ring-menu" role="menu">
          <button className="primary" onClick={() => void retry()}>
            다시 시도
          </button>
          <button
            onClick={() => {
              setMenu(false);
              navigate("notes", { noteId: p.note_id });
            }}
          >
            기록 보기
          </button>
          <button className="ghost" onClick={() => setP(null)}>
            닫기
          </button>
        </div>
      )}
    </div>
  );
}
