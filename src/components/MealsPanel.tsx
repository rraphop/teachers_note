import { useState } from "react";
import { api } from "../api";
import { addDays, fmtDate, fmtDateTime, isoDate, parseDate, todayISO } from "../lib/format";
import { useLoad, useStore } from "../lib/store";
import type { MealInfo } from "../types";

/** 식품의약품안전처 알레르기 유발 식품 번호 */
const ALLERGENS: Record<number, string> = {
  1: "난류",
  2: "우유",
  3: "메밀",
  4: "땅콩",
  5: "대두",
  6: "밀",
  7: "고등어",
  8: "게",
  9: "새우",
  10: "돼지고기",
  11: "복숭아",
  12: "토마토",
  13: "아황산류",
  14: "호두",
  15: "닭고기",
  16: "쇠고기",
  17: "오징어",
  18: "조개류",
  19: "잣",
};

function MealBlock({ meal, showLabel, showAllergy }: { meal: MealInfo; showLabel: boolean; showAllergy: boolean }) {
  return (
    <div className="meal">
      {showLabel && <h3>{meal.meal}</h3>}
      <ul className="meal-dishes">
        {meal.dishes.map((d, i) => (
          <li key={i}>
            {d.name}
            {showAllergy && d.allergens.length > 0 && (
              <small className="allergens" title={d.allergens.map((n) => `${n} ${ALLERGENS[n] ?? ""}`).join(", ")}>
                {d.allergens.join(".")}
              </small>
            )}
          </li>
        ))}
      </ul>
      {meal.calories && <div className="meal-cal">{meal.calories}</div>}
    </div>
  );
}

/** 오늘 화면의 급식 (나이스, 한 주씩 받아 둠) */
export function MealsPanel() {
  const { navigate } = useStore();
  const today = todayISO();
  const [date, setDate] = useState(today);
  const [showAllergy, setShowAllergy] = useState(false);
  const v = useLoad(() => api.getMeals(date), [date]);
  const d = v.data;
  const shift = (n: number) => setDate(isoDate(addDays(parseDate(date), n)));
  const isToday = date === today;
  const hasAllergens = !!d && [...d.meals, ...d.next_meals].some((m) => m.dishes.some((x) => x.allergens.length > 0));

  return (
    <section className="panel meals-panel" id="today-meals">
      <div className="section-head">
        <h2>급식</h2>
        <div className="date-finder" role="group" aria-label="급식 날짜">
          <button className="icon-btn" onClick={() => shift(-1)} aria-label="하루 전" title="하루 전">
            ‹
          </button>
          <span className="meal-date">{isToday ? `오늘 ${fmtDate(date)}` : fmtDate(date)}</span>
          <button className="icon-btn" onClick={() => shift(1)} aria-label="하루 뒤" title="하루 뒤">
            ›
          </button>
          {!isToday && <button onClick={() => setDate(today)}>오늘</button>}
        </div>
      </div>
      {v.error && <p className="small err">{String(v.error)}</p>}
      {!d ? (
        <p className="muted small">불러오는 중…</p>
      ) : d.school_name === null ? (
        <div className="meal-empty">
          <p className="small">학교를 정하면 나이스에서 급식을 받아 보여 줍니다.</p>
          <button onClick={() => navigate("settings", { section: "school" })}>설정에서 학교 정하기</button>
        </div>
      ) : (
        <>
          {d.meals.length > 0 ? (
            d.meals.map((m) => <MealBlock key={m.meal} meal={m} showLabel={d.meals.length > 1 || m.meal !== "중식"} showAllergy={showAllergy} />)
          ) : (
            <div className="meal-empty">
              <p>{isToday ? "오늘은 급식이 없습니다." : "이날은 급식이 없습니다."}</p>
              {d.next_date && d.next_meals.length > 0 && (
                <div className="meal-next">
                  <button className="link" onClick={() => setDate(d.next_date ?? date)}>
                    다음 급식: {fmtDate(d.next_date)}
                  </button>
                  {d.next_meals.map((m) => (
                    <MealBlock key={m.meal} meal={m} showLabel={d.next_meals.length > 1 || m.meal !== "중식"} showAllergy={showAllergy} />
                  ))}
                </div>
              )}
            </div>
          )}
          <div className="meal-foot">
            {hasAllergens && (
              <button className="link" onClick={() => setShowAllergy((x) => !x)} aria-pressed={showAllergy}>
                {showAllergy ? "알레르기 번호 숨기기" : "알레르기 번호 보기"}
              </button>
            )}
            <span className="grow" />
            {d.message ? <small className="muted">{d.message}</small> : d.fetched_at && <small className="muted">나이스 {fmtDateTime(d.fetched_at)}</small>}
          </div>
        </>
      )}
    </section>
  );
}
