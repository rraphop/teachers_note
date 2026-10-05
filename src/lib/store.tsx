import { listen } from "@tauri-apps/api/event";
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

export type PageId = "today" | "calendar" | "timetable" | "notes" | "students" | "records" | "duties" | "tasks" | "search" | "settings";

export interface NavParams {
  studentId?: number;
  noteId?: number;
  dutyId?: number;
  date?: string;
  query?: string;
  section?: string;
}

export interface ToastMsg {
  id: number;
  text: string;
  kind: "ok" | "error" | "info";
}

interface Store {
  /** 백엔드 데이터가 바뀔 때마다 1씩 오른다 — 화면은 이 값을 의존성으로 다시 읽는다 */
  version: number;
  bump: () => void;
  page: PageId;
  params: NavParams;
  navigate: (page: PageId, params?: NavParams) => void;
  toasts: ToastMsg[];
  toast: (text: string, kind?: ToastMsg["kind"]) => void;
  dismissToast: (id: number) => void;
}

const Ctx = createContext<Store | null>(null);

export function StoreProvider({ children }: { children: ReactNode }) {
  const [version, setVersion] = useState(0);
  const [page, setPage] = useState<PageId>("today");
  const [params, setParams] = useState<NavParams>({});
  const [toasts, setToasts] = useState<ToastMsg[]>([]);
  const seq = useRef(1);
  const timer = useRef<number | null>(null);

  // 이벤트가 몰려도 화면을 한 번만 다시 읽도록 묶는다
  const bump = useCallback(() => {
    if (timer.current !== null) return;
    timer.current = window.setTimeout(() => {
      timer.current = null;
      setVersion((v) => v + 1);
    }, 120);
  }, []);

  const dismissToast = useCallback((id: number) => setToasts((t) => t.filter((x) => x.id !== id)), []);

  const toast = useCallback(
    (text: string, kind: ToastMsg["kind"] = "ok") => {
      const id = seq.current++;
      setToasts((t) => [...t.slice(-3), { id, text, kind }]);
      window.setTimeout(() => dismissToast(id), kind === "error" ? 7000 : 3500);
    },
    [dismissToast],
  );

  const navigate = useCallback((p: PageId, prm: NavParams = {}) => {
    setPage(p);
    setParams(prm);
    window.scrollTo({ top: 0 });
  }, []);

  useEffect(() => {
    const unlisten: Array<() => void> = [];
    let alive = true;
    const events = ["data-changed", "note-updated", "models-changed", "settings-changed", "device-info"];
    for (const ev of events) {
      void listen(ev, () => bump()).then((u) => (alive ? unlisten.push(u) : u()));
    }
    void listen<{ title?: string; label?: string; missed?: number }>("reminder-fired", (e) => {
      const p = e.payload;
      if (p.missed) toast(`놓친 알림 ${p.missed}개가 있습니다. 오늘 화면에서 확인하세요.`, "info");
      else if (p.title) toast(`🔔 [${p.label ?? "알림"}] ${p.title}`, "info");
      bump();
    }).then((u) => (alive ? unlisten.push(u) : u()));
    return () => {
      alive = false;
      unlisten.forEach((u) => u());
    };
  }, [bump, toast]);

  const value = useMemo(
    () => ({ version, bump, page, params, navigate, toasts, toast, dismissToast }),
    [version, bump, page, params, navigate, toasts, toast, dismissToast],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useStore(): Store {
  const s = useContext(Ctx);
  if (!s) throw new Error("StoreProvider 가 없습니다");
  return s;
}

/** 비동기 읽기 + 오류 표시 + version 이 바뀌면 다시 읽기 */
export function useLoad<T>(load: () => Promise<T>, deps: unknown[]): { data: T | null; error: string | null; loading: boolean; reload: () => void } {
  const { version } = useStore();
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    let alive = true;
    setLoading(true);
    load()
      .then((d) => {
        if (!alive) return;
        setData(d);
        setError(null);
      })
      .catch((e: unknown) => {
        if (!alive) return;
        setError(typeof e === "string" ? e : e instanceof Error ? e.message : "불러오지 못했습니다");
      })
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version, nonce, ...deps]);
  return { data, error, loading, reload: () => setNonce((n) => n + 1) };
}
