import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { api, errorText } from "../api";
import { useStore } from "../lib/store";
import type { TodayLayout, TodayPanelId } from "../types";

// 오늘 화면 칸 배치.
// - 자동: 화면 너비에 맞춰 1~4열을 정하고, 열마다 정해 둔 칸을 위에서부터 채운다. 열 높이를 맞춰 마지막 칸이 아래까지 늘어난다.
// - 직접: 칸 위 ⠿ 를 끌거나 화살표로 열·순서를 바꾸고, 열 수(1~4)와 숨길 칸을 고른다. 화면이 좁으면 열을 접어 보여 준다.

export const PANEL_LABEL: Record<TodayPanelId, string> = {
  tasks: "할 일",
  briefing: "오늘 업무 요약",
  memo: "메모",
  timetable: "오늘 시간표",
  meals: "급식",
  notes: "오늘의 기록",
  extras: "생일·최근 알림",
};

type Zone = number | "wide";
interface Arrangement {
  columns: TodayPanelId[][];
  wide: TodayPanelId[];
}

/** 자동 배치의 열 수: 열 하나가 400px 안팎이 되도록 */
export function autoColumnCount(width: number): number {
  if (width < 700) return 1;
  if (width < 1250) return 2;
  if (width < 1700) return 3;
  return 4;
}

/** 자동 배치: 열 수마다 칸 자리 (오늘의 기록은 글이 길어 맨 아래 넓은 줄) */
export const AUTO_TEMPLATES: Record<number, Arrangement> = {
  1: { columns: [["briefing", "tasks", "memo", "timetable", "meals", "extras"]], wide: ["notes"] },
  2: { columns: [["tasks", "memo", "timetable"], ["briefing", "meals", "extras"]], wide: ["notes"] },
  3: { columns: [["tasks", "memo"], ["briefing", "timetable"], ["meals", "extras"]], wide: ["notes"] },
  4: { columns: [["tasks"], ["briefing", "extras"], ["memo", "timetable"], ["meals"]], wide: ["notes"] },
};

/** 직접 배치에서 열이 이보다 좁아지면 열을 접는다 */
const MIN_COLUMN = 340;
const GAP = 14;

/** 열 n개를 m개로 접는다 (순서는 그대로) */
export function foldColumns<T>(cols: T[][], m: number): T[][] {
  if (cols.length <= m) return cols;
  const out: T[][] = Array.from({ length: m }, () => []);
  cols.forEach((c, i) => out[Math.floor((i * m) / cols.length)]!.push(...c));
  return out;
}

const without = (v: TodayPanelId[], id: TodayPanelId) => v.filter((x) => x !== id);

/** 지금 보이는 배치를 직접 배치로 옮겨 적는다 (자동에서 처음 옮길 때) */
function toCustom(l: TodayLayout, autoN: number): TodayLayout {
  if (l.mode === "custom") return l;
  const t = AUTO_TEMPLATES[autoN]!;
  return { mode: "custom", columns: t.columns.map((c) => c.filter((p) => !l.hidden.includes(p))), wide: t.wide.filter((p) => !l.hidden.includes(p)), hidden: l.hidden };
}

function shortest(cols: TodayPanelId[][]): number {
  let best = 0;
  cols.forEach((c, i) => {
    if (c.length < cols[best]!.length) best = i;
  });
  return best;
}

export function TodayBoard({ panels, editing, onDone }: { panels: Record<TodayPanelId, ReactNode>; editing: boolean; onDone: () => void }) {
  const { toast } = useStore();
  const [layout, setLayout] = useState<TodayLayout | null>(null);
  const [width, setWidth] = useState(0);
  const [dragId, setDragId] = useState<TodayPanelId | null>(null);
  const [dropAt, setDropAt] = useState<{ zone: Zone; index: number } | null>(null);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api
      .getTodayLayout()
      .then(setLayout)
      .catch((e) => {
        toast(errorText(e), "error");
        setLayout({ mode: "auto", columns: [], wide: [], hidden: [] });
      });
  }, [toast]);
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  const autoN = autoColumnCount(width || 1200);
  const fitN = Math.max(1, Math.floor(((width || 1200) + GAP) / (MIN_COLUMN + GAP)));
  const l = layout ?? { mode: "auto" as const, columns: [], wide: [], hidden: [] };
  const hidden = l.hidden;
  let shown: Arrangement;
  let folded = false;
  if (l.mode === "auto") {
    const t = AUTO_TEMPLATES[autoN]!;
    shown = { columns: t.columns.map((c) => c.filter((p) => !hidden.includes(p))), wide: t.wide.filter((p) => !hidden.includes(p)) };
  } else {
    const cols = l.columns.map((c) => c.filter((p) => !hidden.includes(p)));
    folded = cols.length > fitN;
    shown = { columns: foldColumns(cols, fitN), wide: l.wide.filter((p) => !hidden.includes(p)) };
  }
  // 고치는 중이 아니면 내용이 없는 칸(오늘 생일·알림이 없을 때 등)과 빈 열은 빼고 나머지 열로 꽉 채운다
  if (!editing) {
    const present = (p: TodayPanelId) => panels[p] !== null && panels[p] !== undefined && panels[p] !== false;
    const filled = shown.columns.map((c) => c.filter(present)).filter((c) => c.length > 0);
    shown = { columns: filled.length > 0 ? filled : [[]], wide: shown.wide.filter(present) };
  }

  const save = async (next: TodayLayout) => {
    setLayout(next);
    try {
      setLayout(await api.setTodayLayout(next));
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  /** 칸을 zone 의 index 자리로 (index 는 그 칸을 뺀 목록 기준) */
  const moveTo = (id: TodayPanelId, zone: Zone, index: number) => {
    const c = toCustom(l, autoN);
    const columns = c.columns.map((col) => without(col, id));
    let wide = without(c.wide, id);
    if (zone === "wide") {
      wide = [...wide.slice(0, index), id, ...wide.slice(index)];
    } else {
      const col = columns[Math.min(zone, columns.length - 1)]!;
      col.splice(Math.max(0, Math.min(index, col.length)), 0, id);
    }
    void save({ ...c, columns, wide });
  };
  const where = (id: TodayPanelId): { zone: Zone; index: number } | null => {
    const w = shown.wide.indexOf(id);
    if (w >= 0) return { zone: "wide", index: w };
    for (let i = 0; i < shown.columns.length; i++) {
      const k = shown.columns[i]!.indexOf(id);
      if (k >= 0) return { zone: i, index: k };
    }
    return null;
  };
  const step = (id: TodayPanelId, dir: "up" | "down" | "left" | "right") => {
    const at = where(id);
    if (!at) return;
    if (dir === "up" || dir === "down") moveTo(id, at.zone, at.index + (dir === "up" ? -1 : 1));
    else if (at.zone !== "wide") moveTo(id, at.zone + (dir === "left" ? -1 : 1), at.index);
  };
  const toggleWide = (id: TodayPanelId) => {
    const at = where(id);
    if (!at) return;
    if (at.zone === "wide") {
      const c = toCustom(l, autoN);
      const k = shortest(c.columns);
      moveTo(id, k, c.columns[k]!.length);
    } else moveTo(id, "wide", shown.wide.length);
  };
  const hide = (id: TodayPanelId) => {
    if (l.mode === "auto") void save({ ...l, hidden: [...hidden, id] });
    else void save({ ...l, columns: l.columns.map((c) => without(c, id)), wide: without(l.wide, id), hidden: [...hidden, id] });
  };
  const unhide = (id: TodayPanelId) => {
    if (l.mode === "auto") void save({ ...l, hidden: without(hidden, id) });
    else {
      const columns = l.columns.map((c) => [...c]);
      columns[shortest(columns)]!.push(id);
      void save({ ...l, columns, hidden: without(hidden, id) });
    }
  };
  const setColumns = (n: number) => {
    const c = toCustom(l, autoN);
    const columns = c.columns.length > n ? foldColumns(c.columns, n) : [...c.columns.map((x) => [...x]), ...Array.from({ length: n - c.columns.length }, () => [] as TodayPanelId[])];
    void save({ ...c, columns });
  };

  // ── 끌어서 옮기기 (창 안 끌어 놓기는 파일 끌어 놓기와 겹치므로 포인터로 직접 처리) ──
  const findDrop = (x: number, y: number, id: TodayPanelId): { zone: Zone; index: number } | null => {
    const el = root.current;
    if (!el) return null;
    const zones = [...el.querySelectorAll<HTMLElement>("[data-zone]")];
    const inside = (z: HTMLElement) => {
      const r = z.getBoundingClientRect();
      return x >= r.left && x <= r.right && y >= r.top - 10 && y <= r.bottom + 10;
    };
    const zoneEl =
      zones.find((z) => z.dataset.zone === "wide" && inside(z)) ??
      zones.find((z) => z.dataset.zone !== "wide" && inside(z)) ??
      zones.find((z) => {
        const r = z.getBoundingClientRect();
        return z.dataset.zone !== "wide" && x >= r.left && x <= r.right;
      });
    if (!zoneEl) return null;
    const slots = [...zoneEl.querySelectorAll<HTMLElement>(":scope > .board-slot")].filter((s) => s.dataset.panel !== id);
    let index = slots.length;
    for (let i = 0; i < slots.length; i++) {
      const r = slots[i]!.getBoundingClientRect();
      if (y < r.top + r.height / 2) {
        index = i;
        break;
      }
    }
    return { zone: zoneEl.dataset.zone === "wide" ? "wide" : Number(zoneEl.dataset.zone), index };
  };
  const startDrag = (id: TodayPanelId, e: ReactPointerEvent) => {
    if (folded) return;
    e.preventDefault();
    setDragId(id);
    const move = (ev: PointerEvent) => setDropAt(findDrop(ev.clientX, ev.clientY, id));
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      const at = findDrop(ev.clientX, ev.clientY, id);
      setDragId(null);
      setDropAt(null);
      if (at) moveTo(id, at.zone, at.index);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const slot = (id: TodayPanelId, zone: Zone, i: number, count: number) => (
    <div key={id} className={`board-slot ${dragId === id ? "dragging" : ""}`} data-panel={id}>
      {editing && (
        <div className="edit-strip">
          <span className="drag-handle" onPointerDown={(e) => startDrag(id, e)} title={folded ? "화면이 좁아 옮길 수 없습니다" : "끌어서 옮기기"} aria-hidden>
            ⠿
          </span>
          <b className="grow">{PANEL_LABEL[id]}</b>
          <button className="icon-btn tiny" onClick={() => step(id, "left")} disabled={folded || zone === "wide" || zone === 0} aria-label="왼쪽 열로" title="왼쪽 열로">
            ←
          </button>
          <button className="icon-btn tiny" onClick={() => step(id, "up")} disabled={folded || i === 0} aria-label="위로" title="위로">
            ↑
          </button>
          <button className="icon-btn tiny" onClick={() => step(id, "down")} disabled={folded || i === count - 1} aria-label="아래로" title="아래로">
            ↓
          </button>
          <button className="icon-btn tiny" onClick={() => step(id, "right")} disabled={folded || zone === "wide" || zone === shown.columns.length - 1} aria-label="오른쪽 열로" title="오른쪽 열로">
            →
          </button>
          <button className="tiny-btn" onClick={() => toggleWide(id)} disabled={folded} title={zone === "wide" ? "열 안으로 넣기" : "화면 너비 전체로 (맨 아래 넓은 줄)"}>
            {zone === "wide" ? "열로" : "넓게"}
          </button>
          <button className="tiny-btn" onClick={() => hide(id)}>
            숨기기
          </button>
        </div>
      )}
      {panels[id]}
    </div>
  );
  const mark = (zone: Zone, index: number) => (dropAt && dropAt.zone === zone && dropAt.index === index ? <div className="drop-mark" key={`mark-${zone}-${index}`} /> : null);
  const zoneItems = (ids: TodayPanelId[], zone: Zone) => {
    const visible = ids.filter((p) => p !== dragId);
    const out: ReactNode[] = [];
    let k = 0;
    for (const id of ids) {
      if (id !== dragId) out.push(mark(zone, k++));
      out.push(slot(id, zone, ids.indexOf(id), ids.length));
    }
    out.push(mark(zone, visible.length));
    return out;
  };

  return (
    <div className={`board ${editing ? "editing" : ""}`} ref={root} data-columns={shown.columns.length} data-mode={l.mode}>
      {editing && (
        <div className="board-bar" role="group" aria-label="칸 배치">
          <div className="seg" role="radiogroup" aria-label="배치 방식">
            <button className={l.mode === "auto" ? "on" : ""} onClick={() => void save({ ...l, mode: "auto" })} aria-pressed={l.mode === "auto"}>
              자동 배치
            </button>
            <button className={l.mode === "custom" ? "on" : ""} onClick={() => void save(toCustom(l, autoN))} aria-pressed={l.mode === "custom"}>
              직접 배치
            </button>
          </div>
          {l.mode === "custom" ? (
            <span className="row gap">
              <span className="small">열</span>
              <span className="seg small-seg" role="radiogroup" aria-label="열 수">
                {[1, 2, 3, 4].map((n) => (
                  <button key={n} className={l.columns.length === n ? "on" : ""} onClick={() => setColumns(n)} aria-pressed={l.columns.length === n}>
                    {n}
                  </button>
                ))}
              </span>
            </span>
          ) : (
            <span className="small muted">화면 너비에 맞춰 {autoN}열로 꽉 채웁니다.</span>
          )}
          <span className="small muted">⠿ 를 끌거나 화살표로 옮기면 직접 배치가 됩니다.</span>
          <span className="grow" />
          <button className="primary" onClick={onDone}>
            완료
          </button>
          {hidden.length > 0 && (
            <div className="board-hidden">
              <span className="small">숨긴 칸:</span>
              {hidden.map((id) => (
                <button key={id} className="tiny-btn" onClick={() => unhide(id)} title="다시 보이기">
                  + {PANEL_LABEL[id]}
                </button>
              ))}
            </div>
          )}
          {folded && <p className="small warn-text board-note">화면이 좁아 {l.columns.length}열을 {shown.columns.length}열로 접어 보여 줍니다. 옮기려면 창을 넓히거나 열 수를 줄이세요.</p>}
        </div>
      )}
      <div className="board-cols" style={{ gridTemplateColumns: `repeat(${shown.columns.length}, minmax(0, 1fr))` }}>
        {shown.columns.map((col, ci) => (
          <div className="board-col" data-zone={ci} key={ci}>
            {zoneItems(col, ci)}
            {editing && col.length === 0 && <div className="board-empty">여기로 끌어 놓기</div>}
          </div>
        ))}
      </div>
      {(shown.wide.length > 0 || editing) && (
        <div className="board-wide" data-zone="wide">
          {zoneItems(shown.wide, "wide")}
          {editing && shown.wide.length === 0 && <div className="board-empty">넓은 줄: 여기로 끌어 놓으면 화면 너비 전체를 씁니다</div>}
        </div>
      )}
    </div>
  );
}
