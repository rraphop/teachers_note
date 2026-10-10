import { useEffect, useMemo, useState } from "react";
import { api, errorText } from "../api";
import { Empty, ErrorBox } from "../components/Fields";
import { IconSpark } from "../components/Icons";
import { NoteCard } from "../components/NoteCard";
import { useLoad, useStore } from "../lib/store";
import { parseTerms, type TermMode } from "../lib/terms";
import type { NoteTrack } from "../types";

// 탭 이름은 화면에만 쓰는 이름이다. 저장된 분류 값(general·work)은 그대로 둔다.
type Tab = "all" | NoteTrack;
const TABS: Array<[Tab, string]> = [
  ["all", "모든 기록"],
  ["general", "기타 기록"],
  ["work", "정리된 기록"],
];
const TAB_DESC: Record<Tab, string> = {
  all: "적은 기록을 모두 최근 것부터 보여 줍니다. 카드 오른쪽 위에 기타 기록인지 정리된 기록인지 표시됩니다.",
  general: "AI가 할 일·일정·업무·학생 관찰을 찾지 못한 기록입니다. 메모로 남겨 두거나, 업무에 관한 기록이면 [정리된 기록으로 보내기]로 옮기세요.",
  work: "AI가 할 일·일정·업무·학생 관찰·시간표로 정리한 기록입니다. 잘못 들어왔으면 [기타 기록으로 되돌리기]를 누르세요. 직접 옮긴 기록은 AI가 다시 분류해도 그대로 있습니다.",
};
const asTab = (s: string | undefined): Tab | null => (s === "all" || s === "general" || s === "work" ? s : null);

export function NotesPage() {
  const { params, toast, bump } = useStore();
  const [tab, setTab] = useState<Tab>(asTab(params.section) ?? "all");
  const [onlyUnclassified, setOnlyUnclassified] = useState(params.section === "unclassified");
  const [date, setDate] = useState<string>("");
  const [limit, setLimit] = useState(100);
  // 기록 안 검색: 띄어 쓴 낱말은 모두 들어 있는 기록, '구 그대로'나 "따옴표"는 적은 그대로
  const [q, setQ] = useState("");
  const [mode, setMode] = useState<TermMode>("words");
  const [typed, setTyped] = useState("");
  useEffect(() => {
    const t = window.setTimeout(() => setTyped(q), 250);
    return () => window.clearTimeout(t);
  }, [q]);
  const terms = useMemo(() => parseTerms(typed, mode), [typed, mode]);
  const termsKey = terms.join("\u0001");
  const searching = terms.length > 0;
  useEffect(() => {
    if (params.section === "unclassified") {
      setTab("all");
      setOnlyUnclassified(true);
    } else {
      const t = asTab(params.section);
      if (t) setTab(t);
    }
  }, [params.section]);
  const status = tab !== "work" && onlyUnclassified ? "unclassified" : "all";
  const track = tab === "all" ? null : tab;
  const notes = useLoad(
    () => api.listNotes({ track, status, date: date || null, limit, terms: searching ? terms : null }),
    [tab, status, date, limit, termsKey],
  );
  const counts = useLoad(
    async () => {
      const t = searching ? terms : null;
      const [g, w] = await Promise.all([
        api.listNotes({ track: "general", date: date || null, limit: 500, terms: t }),
        api.listNotes({ track: "work", date: date || null, limit: 500, terms: t }),
      ]);
      return { all: g.length + w.length, general: g.length, work: w.length, unclassified: g.filter((n) => n.status === "pending" || n.status === "failed").length };
    },
    [date, termsKey],
  );
  const focus = params.noteId;

  useEffect(() => {
    if (!focus || !notes.data) return;
    document.getElementById(`note-${focus}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [focus, notes.data]);

  // 링크로 들어온 기록이 목록 밖(다른 탭·오래된 기록)이면 그 기록만 따로 불러온다
  const focusMissing = focus !== undefined && notes.data !== null && !notes.data.some((n) => n.id === focus);
  const single = useLoad(async () => (focusMissing && focus !== undefined ? (await api.noteDetail(focus)).note : null), [focusMissing, focus]);

  const reclassifyAll = async () => {
    try {
      const n = await api.reclassifyUnclassified();
      toast(`미분류 기록 ${n}건을 AI가 다시 정리합니다.`);
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const count = (k: Tab) => (counts.data ? counts.data[k] : null);
  const emptyTitle = searching
    ? "찾는 말이 들어 있는 기록이 없습니다."
    : status === "unclassified"
      ? "미분류 기록이 없습니다."
      : tab === "general"
        ? "기타 기록이 없습니다."
        : tab === "work"
          ? "정리된 기록이 없습니다."
          : "기록이 없습니다.";

  return (
    <div className="page">
      <div className="page-head">
        <h1>기록</h1>
        <div className="head-actions">
          <div className="seg">
            {TABS.map(([k, l]) => (
              <button key={k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}>
                {l}
                {count(k) !== null && <small className="seg-count">{count(k)}</small>}
              </button>
            ))}
          </div>
          <label className="inline">
            날짜
            <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          {date && <button onClick={() => setDate("")}>날짜 풀기</button>}
        </div>
      </div>
      <p className="page-desc">{TAB_DESC[tab]}</p>
      <div className="notes-search">
        <input
          type="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") setQ("");
          }}
          placeholder={mode === "words" ? "기록 안에서 찾기 — 띄어 쓴 낱말이 모두 들어 있는 기록" : "기록 안에서 찾기 — 적은 그대로 들어 있는 기록"}
          aria-label="기록 안에서 찾기"
        />
        <div className="seg" role="group" aria-label="찾는 방법">
          <button className={mode === "words" ? "on" : ""} onClick={() => setMode("words")} title="띄어 쓴 낱말이 모두 들어 있는 기록 (붙어 있지 않아도 됩니다)">
            낱말 모두
          </button>
          <button className={mode === "phrase" ? "on" : ""} onClick={() => setMode("phrase")} title="띄어쓰기까지 적은 그대로 들어 있는 기록">
            구 그대로
          </button>
        </div>
        {q && <button onClick={() => setQ("")}>지우기</button>}
      </div>
      {searching && (
        <p className="hint notes-search-hint">
          {terms.length > 1 ? "다음 낱말이 모두 들어 있는 기록" : "다음 말이 들어 있는 기록"}: {terms.map((t) => `‘${t}’`).join(" ")}
          {notes.data && ` — ${notes.data.length >= limit ? `${limit}건 넘게` : `${notes.data.length}건`}`}
          {mode === "words" && " · 띄어쓰기까지 그대로 찾으려면 “따옴표”로 묶거나 [구 그대로]를 고르세요."}
        </p>
      )}
      {tab !== "work" && (counts.data?.unclassified ?? 0) > 0 && (
        <div className="callout soft">
          <span>
            아직 AI가 정리하지 않은 기록이 <b>{counts.data?.unclassified}건</b> 있습니다. 정리되면 할 일·일정·업무가 나온 기록은 정리된 기록으로 옮겨집니다.
          </span>
          <label className="check">
            <input type="checkbox" checked={onlyUnclassified} onChange={(e) => setOnlyUnclassified(e.target.checked)} /> 미분류만 보기
          </label>
          <button className="primary" onClick={() => void reclassifyAll()}>
            <IconSpark size={15} /> 모두 AI로 분류
          </button>
        </div>
      )}
      {notes.error && <ErrorBox error={notes.error} onRetry={notes.reload} />}
      {single.data && <NoteCard note={single.data} highlight />}
      {notes.data && notes.data.length === 0 && (
        <Empty title={emptyTitle}>{searching ? "낱말을 줄이거나, 다른 탭이나 날짜를 골라 보세요." : "화면 아래 입력창에 적으면 AI가 정리해 알맞은 탭에 넣습니다."}</Empty>
      )}
      <div className="note-list">{notes.data?.map((n) => <NoteCard key={n.id} note={n} highlight={n.id === focus} terms={terms} />)}</div>
      {notes.data && notes.data.length >= limit && (
        <button className="more-btn" onClick={() => setLimit((l) => l + 100)}>
          이전 기록 더 보기
        </button>
      )}
    </div>
  );
}
