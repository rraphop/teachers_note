import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { Empty, ErrorBox } from "../components/Fields";
import { IconSpark } from "../components/Icons";
import { NoteCard } from "../components/NoteCard";
import { useLoad, useStore } from "../lib/store";

type Tab = "all" | "unclassified" | "classified";

export function NotesPage() {
  const { params, toast, bump } = useStore();
  const [tab, setTab] = useState<Tab>(params.section === "unclassified" ? "unclassified" : "all");
  const [date, setDate] = useState<string>("");
  const [limit, setLimit] = useState(100);
  const notes = useLoad(() => api.listNotes({ status: tab, date: date || null, limit }), [tab, date, limit]);
  const focus = params.noteId;

  useEffect(() => {
    if (!focus || !notes.data) return;
    document.getElementById(`note-${focus}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [focus, notes.data]);

  // 링크로 들어온 기록이 목록 밖(오래된 기록)이면 그 기록만 따로 불러온다
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

  return (
    <div className="page">
      <div className="page-head">
        <h1>기록</h1>
        <div className="head-actions">
          <div className="seg">
            {(
              [
                ["all", "전체"],
                ["unclassified", "미분류"],
                ["classified", "정리됨"],
              ] as Array<[Tab, string]>
            ).map(([k, l]) => (
              <button key={k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}>
                {l}
              </button>
            ))}
          </div>
          <label className="inline">
            날짜
            <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          {date && <button onClick={() => setDate("")}>날짜 풀기</button>}
          {tab === "unclassified" && (notes.data?.length ?? 0) > 0 && (
            <button className="primary" onClick={() => void reclassifyAll()}>
              <IconSpark size={15} /> 모두 AI로 분류
            </button>
          )}
        </div>
      </div>
      <p className="page-desc">원문은 AI 처리와 관계없이 먼저 저장됩니다. AI가 틀렸으면 ‘분류 수정’으로 고치고, AI가 없을 때는 ‘직접 분류’를 쓰세요.</p>
      {notes.error && <ErrorBox error={notes.error} onRetry={notes.reload} />}
      {single.data && <NoteCard note={single.data} highlight />}
      {notes.data && notes.data.length === 0 && (
        <Empty title={tab === "unclassified" ? "미분류 기록이 없습니다." : "기록이 없습니다."}>맨 위 입력창에 적으면 여기에 쌓입니다.</Empty>
      )}
      <div className="note-list">{notes.data?.map((n) => <NoteCard key={n.id} note={n} highlight={n.id === focus} />)}</div>
      {notes.data && notes.data.length >= limit && (
        <button className="more-btn" onClick={() => setLimit((l) => l + 100)}>
          이전 기록 더 보기
        </button>
      )}
    </div>
  );
}
