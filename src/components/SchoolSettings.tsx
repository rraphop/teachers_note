import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { fmtDateTime } from "../lib/format";
import { DAY_KIND_LABEL, DAY_PALETTE, dayHex } from "../lib/palette";
import { useLoad, useStore } from "../lib/store";
import type { AppSettings, DayColors, DayRules, FetchInfo, SchoolInfo } from "../types";

const STATUS_TEXT: Record<string, string> = { ok: "받음", empty: "받을 자료 없음", need_key: "인증키 필요", error: "받지 못함" };

function FetchLine({ label, info, onSync, busy }: { label: string; info: FetchInfo | null | undefined; onSync?: () => void; busy: boolean }) {
  return (
    <div className="neis-line">
      <span className="neis-label">{label}</span>
      <span className={`neis-state st-${info?.status ?? "none"}`}>{info ? STATUS_TEXT[info.status] ?? info.status : "아직 받지 않음"}</span>
      {info && <small className="muted">{fmtDateTime(info.fetched_at)}</small>}
      {info?.message && info.status !== "ok" && <small className="muted neis-msg">{info.message}</small>}
      {info?.message && info.status === "ok" && <small className="warn-text neis-msg">{info.message}</small>}
      <span className="grow" />
      {onSync && (
        <button onClick={onSync} disabled={busy}>
          {busy ? "받는 중…" : "지금 받기"}
        </button>
      )}
    </div>
  );
}

/** 쉼표·줄바꿈으로 적은 낱말 목록 */
function WordsField({ label, value, onSave }: { label: string; value: string[]; onSave: (v: string[]) => void }) {
  const [text, setText] = useState(value.join(", "));
  useEffect(() => setText(value.join(", ")), [value]);
  return (
    <label className="field">
      <span>{label}</span>
      <textarea
        rows={2}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => {
          const v = text
            .split(/[,\n]/)
            .map((x) => x.trim())
            .filter(Boolean);
          if (v.join("|") !== value.join("|")) onSave(v);
        }}
      />
    </label>
  );
}

/** 설정 → 학교: 나이스 학교·담임반·인증키, 받기 상태, 내 과목, 학사력 색·종류 규칙 */
export function SchoolPanel({ settings, onSave }: { settings: AppSettings; onSave: (s: AppSettings) => Promise<void> }) {
  const { toast, bump } = useStore();
  const status = useLoad(() => api.neisStatus(), [settings.school?.code, settings.neis_key, settings.my_subjects.join("|")]);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SchoolInfo[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [keyDraft, setKeyDraft] = useState(settings.neis_key);
  const [showKey, setShowKey] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [newSubject, setNewSubject] = useState("");
  useEffect(() => setKeyDraft(settings.neis_key), [settings.neis_key]);
  const st = status.data;

  const sync = async (what: "timetable" | "schedule" | "classes" | "all") => {
    setBusy(what);
    try {
      const r = await api.neisSync(what);
      const msgs = r.filter((x) => x.message && x.status !== "ok").map((x) => x.message);
      const got = r.filter((x) => x.status === "ok" && x.count > 0).length;
      if (msgs.length) toast(msgs.join(" / "), r.some((x) => x.status === "error") ? "error" : "info");
      else toast(got ? "나이스에서 받았습니다." : "새로 받을 것이 없습니다.");
      status.reload();
      bump();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(null);
    }
  };
  const search = async () => {
    setSearching(true);
    try {
      setResults(await api.neisSearchSchools(query));
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setSearching(false);
    }
  };
  const choose = async (s: SchoolInfo) => {
    await onSave({ ...settings, school: s });
    setResults(null);
    setQuery("");
    toast(`${s.name}(으)로 정했습니다. 나이스에서 학급·시간표·학사일정을 받습니다.`);
    await sync("all");
  };
  const saveKey = async () => {
    const k = keyDraft.trim();
    await onSave({ ...settings, neis_key: k });
    toast(k ? "인증키를 저장했습니다." : "인증키를 지웠습니다.");
    if (k && settings.school) await sync("all");
  };
  const toggleSubject = (s: string, on: boolean) =>
    void onSave({ ...settings, my_subjects: on ? [...settings.my_subjects, s] : settings.my_subjects.filter((x) => x !== s) });
  const toggleClass = (c: string, on: boolean) =>
    void onSave({ ...settings, my_classes_off: on ? settings.my_classes_off.filter((x) => x !== c) : [...settings.my_classes_off, c] });
  const subjects = Array.from(new Set([...(st?.subjects ?? []), ...settings.my_subjects]));
  const classes = st?.classes ?? [];

  return (
    <section className="panel" id="set-school">
      <h2>학교 (나이스)</h2>
      <p className="small">
        학교를 정하면 나이스 교육정보 개방 포털에서 시간표·급식·학사일정을 받습니다. 이때 <b>학교 코드만</b> 보내며 학생 정보는 보내지 않습니다. 앱을 켤 때 하루 한 번 새로 받고, 받은 자료는 인터넷이 없어도 보입니다.
      </p>

      {settings.school ? (
        <div className="school-card">
          <div>
            <strong>{settings.school.name}</strong> <span className="muted">{settings.school.kind}</span>
            <div className="small muted">
              {settings.school.region} · {settings.school.office_name}
              {settings.school.address && ` · ${settings.school.address}`}
            </div>
          </div>
          <button onClick={() => setResults([])}>학교 바꾸기</button>
        </div>
      ) : (
        <p className="hint">아직 학교를 정하지 않았습니다. 학교 이름으로 찾아 고르세요.</p>
      )}

      {(!settings.school || results !== null) && (
        <div className="school-search">
          <div className="row gap">
            <input value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && query.trim().length >= 2 && void search()} placeholder="학교 이름 (예: OO중학교)" />
            <button className="primary" onClick={() => void search()} disabled={query.trim().length < 2 || searching}>
              {searching ? "찾는 중…" : "찾기"}
            </button>
            {settings.school && <button onClick={() => setResults(null)}>취소</button>}
          </div>
          {results && results.length > 0 && (
            <ul className="school-results">
              {results.map((s) => (
                <li key={`${s.office_code}-${s.code}`}>
                  <div>
                    <strong>{s.name}</strong> <span className="muted">{s.kind}</span>
                    <div className="small muted">
                      {s.region} · {s.office_name}
                      {s.address && ` · ${s.address}`}
                    </div>
                  </div>
                  <button onClick={() => void choose(s)}>이 학교로</button>
                </li>
              ))}
            </ul>
          )}
          {results && results.length === 0 && query && !searching && <p className="small muted">찾은 학교가 없습니다. 이름을 줄여 보세요 (예: ‘OO중’).</p>}
        </div>
      )}

      {settings.school && (
        <>
          <div className="row gap wrap">
            <label className="field inline">
              <span>담임반</span>
              <select value={settings.homeroom} onChange={(e) => void onSave({ ...settings, homeroom: e.target.value })}>
                <option value="">없음</option>
                {Array.from(new Set([...classes, ...(settings.homeroom ? [settings.homeroom] : [])])).map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="neis-key">
            <label className="field">
              <span>나이스 인증키 (선택)</span>
              <div className="row gap">
                <input type={showKey ? "text" : "password"} value={keyDraft} onChange={(e) => setKeyDraft(e.target.value)} placeholder="인증키를 붙여 넣으세요" autoComplete="off" spellCheck={false} />
                <button onClick={() => setShowKey((x) => !x)}>{showKey ? "가리기" : "보기"}</button>
                <button className="primary" onClick={() => void saveKey()} disabled={keyDraft.trim() === settings.neis_key}>
                  저장
                </button>
              </div>
            </label>
            <p className="small muted">
              인증키가 없으면 나이스가 한 번에 5줄만 줍니다. 그래서 <b>급식</b>은 받을 수 있지만 <b>시간표·학사일정</b>은 받지 않습니다(일부만 받아 틀린 시간표를 만들지 않도록). 인증키는 나이스 교육정보 개방 포털(open.neis.go.kr)에 회원가입한 뒤 ‘인증키 신청’에서 무료로 바로 받을 수 있습니다.
            </p>
          </div>

          <div className="neis-status">
            <FetchLine label="시간표" info={st?.timetable} onSync={() => void sync("timetable")} busy={busy === "timetable" || busy === "all"} />
            <FetchLine label="학사일정" info={st?.schedule} onSync={() => void sync("schedule")} busy={busy === "schedule" || busy === "all"} />
            <FetchLine label="급식 (이번 주)" info={st?.meals} busy={false} />
            <div className="neis-line">
              <span className="neis-label">학급</span>
              <span>{classes.length ? classes.join(", ") : "아직 모름"}</span>
              <span className="grow" />
              <button onClick={() => void sync("classes")} disabled={!!busy}>
                학급 다시 받기
              </button>
            </div>
          </div>

          <h3>내 과목 → 개인 시간표</h3>
          <p className="small muted">학교 시간표에서 고른 과목 칸만 모아 개인 시간표를 만듭니다. 내 과목이지만 다른 선생님이 가르치는 학급은 빼세요. 시간표 화면에서 직접 입력한 칸은 그대로 둡니다.</p>
          {subjects.length === 0 ? (
            <p className="small muted">학교 시간표를 받으면 과목 목록이 나옵니다. 아래에 직접 적어 둘 수도 있습니다.</p>
          ) : (
            <div className="check-grid">
              {subjects.map((s) => (
                <label key={s} className="check">
                  <input type="checkbox" checked={settings.my_subjects.includes(s)} onChange={(e) => toggleSubject(s, e.target.checked)} /> {s}
                </label>
              ))}
            </div>
          )}
          <div className="row gap">
            <input value={newSubject} onChange={(e) => setNewSubject(e.target.value)} placeholder="과목 직접 추가 (예: 정보)" />
            <button
              onClick={() => {
                const s = newSubject.trim();
                if (s && !settings.my_subjects.includes(s)) toggleSubject(s, true);
                setNewSubject("");
              }}
              disabled={!newSubject.trim()}
            >
              추가
            </button>
          </div>
          {classes.length > 0 && settings.my_subjects.length > 0 && (
            <div className="check-grid">
              <span className="small muted">개인 시간표에 넣을 학급:</span>
              {classes.map((c) => (
                <label key={c} className="check">
                  <input type="checkbox" checked={!settings.my_classes_off.includes(c)} onChange={(e) => toggleClass(c, e.target.checked)} /> {c}
                </label>
              ))}
            </div>
          )}
        </>
      )}

      <h3>학사력 색과 종류</h3>
      <div className="row gap wrap">
        {(Object.keys(DAY_KIND_LABEL) as Array<keyof DayColors>).map((k) => (
          <label key={k} className="field inline day-color">
            <span>
              <i className="swatch" style={{ background: dayHex(settings.day_colors[k]) }} /> {DAY_KIND_LABEL[k]}
            </span>
            <select value={settings.day_colors[k]} onChange={(e) => void onSave({ ...settings, day_colors: { ...settings.day_colors, [k]: e.target.value } })}>
              {DAY_PALETTE.map((c) => (
                <option key={c.key} value={c.key}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
        ))}
      </div>
      <p className="small muted">나이스가 ‘공휴일·휴업일’로 알려 주면 그대로 따르고, 그렇지 않으면 행사 이름에 아래 낱말이 있는지로 정합니다. 그 밖의 행사는 ‘업무’입니다. 고치면 이미 있는 학사력에도 적용됩니다.</p>
      {(["holiday", "closure", "vacation"] as Array<keyof DayRules>).map((k) => (
        <WordsField key={k} label={`${DAY_KIND_LABEL[k]}로 볼 행사 이름`} value={settings.day_rules[k]} onSave={(v) => void onSave({ ...settings, day_rules: { ...settings.day_rules, [k]: v } })} />
      ))}
    </section>
  );
}
