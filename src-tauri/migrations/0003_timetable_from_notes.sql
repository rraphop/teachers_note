-- 0.5: 기록 입력창에서 찾은 시간표
--   regular: 정규 시간표 문장 → 개인 시간표 칸 (applied 반영함 / conflict 다른 수업이 있어 교사 확인 대기 / replaced 바꿈 / kept 그대로 둠)
--   change : 하루짜리 변경 (보강·교체·자습·휴강·대강·변경) → 원래 시간표는 두고 그 날짜에만 표시
-- 기록을 지우면 반영 기록도 지워진다. 이미 채운 시간표 칸은 교사의 시간표이므로 지우지 않는다.
CREATE TABLE tt_entries (
  id           INTEGER PRIMARY KEY,
  note_id      INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  kind         TEXT NOT NULL CHECK (kind IN ('regular', 'change')),
  school_year  INTEGER,
  semester     INTEGER,
  weekday      INTEGER,
  date         TEXT,
  period       INTEGER NOT NULL,
  class_label  TEXT NOT NULL DEFAULT '',
  subject      TEXT NOT NULL DEFAULT '',
  room         TEXT NOT NULL DEFAULT '',
  change_type  TEXT NOT NULL DEFAULT '',
  text         TEXT NOT NULL,
  status       TEXT NOT NULL CHECK (status IN ('applied', 'conflict', 'replaced', 'kept', 'change')),
  previous     TEXT NOT NULL DEFAULT '',
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE INDEX idx_tt_entries_note ON tt_entries(note_id);
CREATE INDEX idx_tt_entries_date ON tt_entries(kind, date);
CREATE INDEX idx_tt_entries_status ON tt_entries(status);
