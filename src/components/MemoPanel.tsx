import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, errorText } from "../api";
import { fmtDateTime } from "../lib/format";
import { useStore } from "../lib/store";
import type { Memo, MemoColor } from "../types";
import { IconLeft, IconPlus, IconRight, IconTrash } from "./Icons";

/** 메모 색 (윈도우 스티커 메모처럼) */
export const MEMO_COLORS: Array<{ key: MemoColor; label: string }> = [
  { key: "yellow", label: "노랑" },
  { key: "green", label: "초록" },
  { key: "pink", label: "분홍" },
  { key: "purple", label: "보라" },
  { key: "blue", label: "파랑" },
  { key: "gray", label: "회색" },
];

type SaveState = "saved" | "dirty" | "saving" | "error";

/** 메모 한 장: 쓰는 대로(0.6초 쉬면) 저장, 창을 떠나거나 칸을 벗어날 때도 저장 */
function MemoCard({ memo, first, last, autoFocus, onMove, onDelete }: { memo: Memo; first: boolean; last: boolean; autoFocus: boolean; onMove: (dir: -1 | 1) => void; onDelete: (m: Memo) => void }) {
  const { toast } = useStore();
  const [text, setText] = useState(memo.body);
  const [color, setColor] = useState<MemoColor>(memo.color);
  const [updatedAt, setUpdatedAt] = useState(memo.updated_at);
  const [state, setState] = useState<SaveState>("saved");
  const latest = useRef(memo.body);
  const timer = useRef<number | null>(null);
  const area = useRef<HTMLTextAreaElement>(null);

  // 글 길이에 맞춰 높이를 늘린다
  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.max(el.scrollHeight, 72)}px`;
  }, [text]);
  useEffect(() => {
    if (autoFocus) area.current?.focus();
  }, [autoFocus]);

  const save = useCallback(async () => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    const body = latest.current;
    setState("saving");
    try {
      const m = await api.updateMemo(memo.id, { body });
      setUpdatedAt(m.updated_at);
      setState(latest.current === body ? "saved" : "dirty");
    } catch (e) {
      setState("error");
      toast(errorText(e), "error");
    }
  }, [memo.id, toast]);

  // 다른 화면으로 가면서 아직 저장하지 않은 글이 있으면 저장한다
  useEffect(
    () => () => {
      if (timer.current !== null) {
        window.clearTimeout(timer.current);
        void api.updateMemo(memo.id, { body: latest.current });
      }
    },
    [memo.id],
  );

  const change = (v: string) => {
    setText(v);
    latest.current = v;
    setState("dirty");
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void save(), 600);
  };
  const recolor = async (c: MemoColor) => {
    setColor(c);
    try {
      const m = await api.updateMemo(memo.id, { color: c });
      setUpdatedAt(m.updated_at);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  return (
    <div className={`memo-card memo-${color}`} data-memo={memo.id}>
      <div className="memo-bar">
        <div className="memo-colors" role="group" aria-label="메모 색">
          {MEMO_COLORS.map((c) => (
            <button key={c.key} className={`memo-dot memo-${c.key} ${c.key === color ? "on" : ""}`} title={c.label} aria-label={`${c.label} 메모`} aria-pressed={c.key === color} onClick={() => void recolor(c.key)} />
          ))}
        </div>
        <span className="grow" />
        <button className="icon-btn tiny" onClick={() => onMove(-1)} disabled={first} aria-label="앞으로" title="앞으로">
          <IconLeft size={14} />
        </button>
        <button className="icon-btn tiny" onClick={() => onMove(1)} disabled={last} aria-label="뒤로" title="뒤로">
          <IconRight size={14} />
        </button>
        <button
          className="icon-btn tiny"
          onClick={() => {
            if (timer.current !== null) window.clearTimeout(timer.current);
            timer.current = null;
            onDelete({ ...memo, body: latest.current, color });
          }}
          aria-label="메모 지우기"
          title="메모 지우기"
        >
          <IconTrash size={14} />
        </button>
      </div>
      <textarea
        ref={area}
        className="memo-text"
        value={text}
        onChange={(e) => change(e.target.value)}
        onBlur={() => {
          if (state === "dirty" || timer.current !== null) void save();
        }}
        placeholder="여기에 적으세요. 쓰는 대로 저장되고, 날이 바뀌어도 그대로 남습니다."
        aria-label="메모 내용"
        spellCheck={false}
      />
      <div className={`memo-foot ${state === "error" ? "err" : ""}`}>
        {state === "saving" ? "저장 중…" : state === "dirty" ? "쓰는 중…" : state === "error" ? "저장하지 못했습니다" : `${fmtDateTime(updatedAt)} 저장`}
      </div>
    </div>
  );
}

/** 오늘 화면 메모 칸 (윈도우 스티커 메모처럼 날이 바뀌어도 남는다) */
export function MemoPanel() {
  const { toast } = useStore();
  const [memos, setMemos] = useState<Memo[] | null>(null);
  const [focusId, setFocusId] = useState<number | null>(null);
  const [undo, setUndo] = useState<Memo | null>(null);

  // 글을 쓰는 동안 화면 전체를 다시 읽지 않도록, 처음 한 번만 읽고 바꾼 것은 여기서 맞춘다
  useEffect(() => {
    api
      .listMemos()
      .then(setMemos)
      .catch((e) => toast(errorText(e), "error"));
  }, [toast]);

  const add = async () => {
    try {
      const m = await api.createMemo();
      setMemos((v) => [m, ...(v ?? [])]);
      setFocusId(m.id);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const remove = async (m: Memo) => {
    try {
      // 마지막으로 쓴 글까지 저장한 뒤 지운다 (되돌리면 그대로 돌아오게)
      await api.updateMemo(m.id, { body: m.body });
      await api.deleteMemo(m.id);
      setMemos((v) => (v ?? []).filter((x) => x.id !== m.id));
      // 빈 메모는 되돌릴 필요가 없다
      setUndo(m.body.trim() ? m : null);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const restore = async () => {
    if (!undo) return;
    try {
      await api.restoreMemo(undo.id);
      setMemos(await api.listMemos());
      setUndo(null);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const move = async (id: number, dir: -1 | 1) => {
    try {
      setMemos(await api.shiftMemo(id, dir));
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  return (
    <section className="panel memo-panel" id="today-memo">
      <div className="section-head">
        <h2>
          메모 <span className="count">{memos?.length ?? 0}</span>
        </h2>
        <button onClick={() => void add()}>
          <IconPlus size={15} /> 새 메모
        </button>
      </div>
      {undo && (
        <div className="memo-undo" role="status">
          메모를 지웠습니다. <span className="muted">“{undo.body.trim().slice(0, 20)}{undo.body.trim().length > 20 ? "…" : ""}”</span>
          <button className="link" onClick={() => void restore()}>
            되돌리기
          </button>
        </div>
      )}
      {memos && memos.length === 0 ? (
        <p className="muted small">날이 바뀌어도 그대로 남는 메모입니다. [새 메모]를 눌러 적어 보세요.</p>
      ) : (
        <div className="memo-grid">
          {memos?.map((m, i) => (
            <MemoCard key={m.id} memo={m} first={i === 0} last={i === memos.length - 1} autoFocus={focusId === m.id} onMove={(d) => void move(m.id, d)} onDelete={(x) => void remove(x)} />
          ))}
        </div>
      )}
    </section>
  );
}
