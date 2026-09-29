import { useState } from "react";
import { api, errorText } from "../api";
import { fmtDate } from "../lib/format";
import { OFFSET_CHOICES, offsetLabel } from "../lib/labels";
import type { ReminderSpec } from "../types";

/** 날짜 + 원래 날짜 표현 + 다시 해석 */
export function DateFields({
  date,
  time,
  expr,
  base,
  onChange,
  label = "날짜",
  allowTime = true,
}: {
  date: string | null;
  time: string | null;
  expr: string | null;
  base: string | null;
  onChange: (v: { date: string | null; time: string | null; expr: string | null }) => void;
  label?: string;
  allowTime?: boolean;
}) {
  const [hint, setHint] = useState<string | null>(null);
  const resolve = async () => {
    if (!expr?.trim()) return;
    try {
      const r = await api.resolveDate(expr, base);
      if (r) {
        onChange({ date: r.date, time, expr });
        setHint(`"${r.expr}" → ${r.label}${r.approximate ? " (대략)" : ""}`);
      } else {
        setHint("날짜로 이해하지 못했습니다. 달력에서 직접 골라 주세요.");
      }
    } catch (e) {
      setHint(errorText(e));
    }
  };
  return (
    <div className="field-group">
      <div className="row gap">
        <label className="field grow">
          <span>{label}</span>
          <input type="date" value={date ?? ""} onChange={(e) => onChange({ date: e.target.value || null, time, expr })} />
        </label>
        {allowTime && (
          <label className="field">
            <span>시각 (선택)</span>
            <input type="time" value={time ?? ""} onChange={(e) => onChange({ date, time: e.target.value || null, expr })} />
          </label>
        )}
      </div>
      <label className="field">
        <span>원래 날짜 표현</span>
        <div className="row gap">
          <input
            className="grow"
            value={expr ?? ""}
            placeholder="예: 금요일까지, 다음 주 수요일"
            onChange={(e) => onChange({ date, time, expr: e.target.value || null })}
            onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), void resolve())}
          />
          <button type="button" onClick={() => void resolve()} disabled={!expr?.trim()} title="날짜 표현을 기록한 날 기준으로 다시 계산합니다">
            날짜로 바꾸기
          </button>
        </div>
      </label>
      {hint && <p className="hint">{hint}</p>}
      {!hint && date && <p className="hint">{fmtDate(date)}</p>}
    </div>
  );
}

export function ReminderPicker({ value, onChange, hasDate }: { value: ReminderSpec; onChange: (v: ReminderSpec) => void; hasDate: boolean }) {
  const toggle = (o: number) => {
    const set = new Set(value.offsets);
    if (set.has(o)) set.delete(o);
    else set.add(o);
    onChange({ ...value, offsets: [...set].sort((a, b) => b - a) });
  };
  return (
    <fieldset className="reminder">
      <legend>
        <label className="check">
          <input type="checkbox" checked={value.enabled} onChange={(e) => onChange({ ...value, enabled: e.target.checked })} />
          Windows 알림 받기
        </label>
      </legend>
      {value.enabled && (
        <>
          {!hasDate && <p className="hint warn">날짜가 없으면 '며칠 전' 알림은 울리지 않습니다. 직접 정한 시각은 울립니다.</p>}
          <div className="chips-select" role="group" aria-label="알림 시점">
            {OFFSET_CHOICES.map((o) => (
              <button type="button" key={o} className={`chip-toggle ${value.offsets.includes(o) ? "on" : ""}`} onClick={() => toggle(o)} aria-pressed={value.offsets.includes(o)}>
                {offsetLabel(o)}
              </button>
            ))}
          </div>
          <div className="row gap">
            <label className="field">
              <span>알림 시각</span>
              <input type="time" value={value.time} onChange={(e) => onChange({ ...value, time: e.target.value || "08:30" })} />
            </label>
            <label className="field grow">
              <span>직접 정한 시각 (선택)</span>
              <div className="row gap">
                <input
                  className="grow"
                  type="datetime-local"
                  value={value.custom_at ? value.custom_at.slice(0, 16) : ""}
                  onChange={(e) => onChange({ ...value, custom_at: e.target.value ? `${e.target.value}:00` : null })}
                />
                {value.custom_at && (
                  <button type="button" onClick={() => onChange({ ...value, custom_at: null })}>
                    지우기
                  </button>
                )}
              </div>
            </label>
          </div>
        </>
      )}
    </fieldset>
  );
}

export function Empty({ title, children }: { title: string; children?: React.ReactNode }) {
  return (
    <div className="empty">
      <strong>{title}</strong>
      {children && <div className="empty-body">{children}</div>}
    </div>
  );
}

export function ErrorBox({ error, onRetry }: { error: string; onRetry?: () => void }) {
  return (
    <div className="error-box" role="alert">
      <span>{error}</span>
      {onRetry && <button onClick={onRetry}>다시 시도</button>}
    </div>
  );
}
