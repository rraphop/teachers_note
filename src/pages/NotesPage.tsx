import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { Empty, ErrorBox } from "../components/Fields";
import { IconSpark } from "../components/Icons";
import { NoteCard } from "../components/NoteCard";
import { useLoad, useStore } from "../lib/store";
import type { NoteTrack } from "../types";

const TAB_DESC: Record<NoteTrack, string> = {
  general: "AI가 할 일·일정·업무·학생 관찰을 찾지 못한 기록입니다. 메모로 남겨 두거나, 업무에 관한 기록이면 [업무기록으로 보내기]로 옮기세요.",
  work: "AI가 할 일·일정·업무·학생 관찰·시간표로 정리한 기록입니다. 잘못 들어왔으면 [일반으로 되돌리기]를 누르세요. 직접 옮긴 기록은 AI가 다시 분류해도 그대로 있습니다.",
};

export function NotesPage() {
  const { params, toast, bump } = useStore();
  const [tab, setTab] = useState<NoteTrack>(params.section === "work" ? "work" : "general");
  const [onlyUnclassified, setOnlyUnclassified] = useState(params.section === "unclassified");
  const [date, setDate] = useState<string>("");
  const [limit, setLimit] = useState(100);
  useEffect(() => {
    if (params.section === "unclassified") {
      setTab("general");
      setOnlyUnclassified(true);
    } else if (params.section === "work" || params.section === "general") {
      setTab(params.section);
    }
  }, [params.section]);
  const status = tab === "general" && onlyUnclassified ? "unclassified" : "all";
  const notes = useLoad(() => api.listNotes({ track: tab, status, date: date || null, limit }), [tab, status, date, limit]);
  const counts = useLoad(
    async () => {
      const [g, w] = await Promise.all([api.listNotes({ track: "general", date: date || null, limit: 500 }), api.listNotes({ track: "work", date: date || null, limit: 500 })]);
      return { general: g.length, work: w.length, unclassified: g.filter((n) => n.status === "pending" || n.status === "failed").length };
    },
    [date],
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
  const count = (k: NoteTrack) => (counts.data ? counts.data[k] : null);

  return (
    <div className="page">
      <div className="page-head">
        <h1>기록</h1>
        <div className="head-actions">
          <div className="seg">
            {(
              [
                ["general", "일반 기록"],
                ["work", "정리된 업무기록"],
              ] as Array<[NoteTrack, string]>
            ).map(([k, l]) => (
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
      {tab === "general" && (counts.data?.unclassified ?? 0) > 0 && (
        <div className="callout soft">
          <span>
            아직 AI가 정리하지 않은 기록이 <b>{counts.data?.unclassified}건</b> 있습니다. 정리되면 할 일·일정·업무가 나온 기록은 업무기록으로 옮겨집니다.
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
        <Empty title={tab === "general" ? (onlyUnclassified ? "미분류 기록이 없습니다." : "일반 기록이 없습니다.") : "정리된 업무기록이 없습니다."}>
          화면 아래 입력창에 적으면 AI가 정리해 알맞은 탭에 넣습니다.
        </Empty>
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