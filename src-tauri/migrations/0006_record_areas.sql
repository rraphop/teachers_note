-- 0.9: 학생부 자료 — 영역 7개(창체 자율·자치, 교과, 행특, 동아리, 진로, 자유학기, 일상생활)
--   영역 값 제한(CHECK)을 넓히려면 표를 다시 만들어야 한다. (마이그레이션은 외래키를 끈 채 실행하고, 끝에서 외래키를 검사한다)
--   id 는 그대로 옮기므로 초안 근거(record_draft_sources)와 기록 연결은 바뀌지 않는다.

-- 관찰 기록(학생부 참고자료)
--   subject  : 교과는 과목(예: 사회), 자유학기는 활동(주제선택 등). 다른 영역은 빈 값
--   excluded : 교사가 학생부 참고자료에서 뺌 (기록은 남는다)
CREATE TABLE observations_new (
  id          INTEGER PRIMARY KEY,
  note_id     INTEGER REFERENCES notes(id) ON DELETE CASCADE,
  student_id  INTEGER REFERENCES students(id) ON DELETE CASCADE,
  area        TEXT NOT NULL CHECK (area IN ('attendance','subject','autonomous','career','volunteer','club','behavior','free_semester','daily','none')),
  content     TEXT NOT NULL,
  observed_on TEXT NOT NULL,
  source      TEXT NOT NULL CHECK (source IN ('ai','manual')),
  edited      INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  subject     TEXT NOT NULL DEFAULT '',
  excluded    INTEGER NOT NULL DEFAULT 0
);
INSERT INTO observations_new(id, note_id, student_id, area, content, observed_on, source, edited, created_at, updated_at)
  SELECT id, note_id, student_id, area, content, observed_on, source, edited, created_at, updated_at FROM observations;
DROP TABLE observations;
ALTER TABLE observations_new RENAME TO observations;
CREATE INDEX idx_obs_student_area ON observations(student_id, area);
CREATE INDEX idx_obs_note ON observations(note_id);

-- 학생부 초안·최종안: 영역(과목)마다 가장 최근 것이 그 칸의 글이다. reviewed = 1 이면 '최종안'
CREATE TABLE record_drafts_new (
  id           INTEGER PRIMARY KEY,
  student_id   INTEGER NOT NULL REFERENCES students(id) ON DELETE CASCADE,
  area         TEXT NOT NULL CHECK (area IN ('attendance','subject','autonomous','career','volunteer','club','behavior','free_semester','daily')),
  body         TEXT NOT NULL,
  generated_by TEXT NOT NULL CHECK (generated_by IN ('template','ai','manual')),
  reviewed     INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL,
  subject      TEXT NOT NULL DEFAULT ''
);
INSERT INTO record_drafts_new(id, student_id, area, body, generated_by, reviewed, created_at, updated_at)
  SELECT id, student_id, area, body, generated_by, reviewed, created_at, updated_at FROM record_drafts;
DROP TABLE record_drafts;
ALTER TABLE record_drafts_new RENAME TO record_drafts;
CREATE INDEX idx_drafts_student ON record_drafts(student_id, area);
