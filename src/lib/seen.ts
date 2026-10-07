import { useEffect, useState } from "react";
import { api } from "../api";
import { useStore } from "./store";

// 메뉴의 '새로 정리된 항목' (2차 수정목록 5번): 메뉴를 열면 숫자가 사라지고,
// 이번 방문 동안에는 지난번에 본 뒤 AI 가 넣은 항목에 '새' 표시를 한다 (다음 방문 때 사라짐).

export type SeenMenu = "records" | "duties" | "tasks" | "students";

/** 이 메뉴를 열었다고 알리고, '새' 표시 기준 시각(지난번에 본 시각)을 돌려준다 */
export function useNewSince(menu: SeenMenu): string | null {
  const { bump } = useStore();
  const [since, setSince] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .markSeen(menu)
      .then((prev) => {
        if (!alive) return;
        setSince(prev ?? null);
        bump();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menu]);
  return since;
}

/** AI 가 기준 시각 뒤에 만든 항목인가 */
export function isNewItem(item: { source: string; created_at: string }, since: string | null): boolean {
  return !!since && item.source === "ai" && item.created_at > since;
}

export function badgeText(n: number): string {
  return n > 99 ? "99+" : String(n);
}
