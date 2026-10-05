-- 0.8: 나이스 공개 API(학교 시간표·급식·학사일정)와 시간표 칸 색

-- 시간표 칸 출처: '' = 교사가 입력, 'neis' = 나이스에서 받음 (다시 받을 때 'neis' 칸만 바꾸고, 교사가 입력한 칸은 그대로)
ALTER TABLE timetable_cells ADD COLUMN source TEXT NOT NULL DEFAULT '';

-- 시간표 색 (우선순위: 칸 → 학급 → 교실). 시간표를 다시 받아도 그대로 남는다.
--   scope='cell'  : target = "학년도-학기|시간표종류|시간표학급|행|요일|교시"
--   scope='class' : target = "2-1"
--   scope='room'  : target = "과학실"
CREATE TABLE timetable_colors (
  scope  TEXT NOT NULL CHECK (scope IN ('cell', 'class', 'room')),
  target TEXT NOT NULL,
  color  TEXT NOT NULL,
  PRIMARY KEY (scope, target)
);

-- 급식 (한 주씩 받아 둔다. 인터넷이 없어도 받아 둔 것은 보인다)
CREATE TABLE neis_meals (
  school_code TEXT NOT NULL,
  date        TEXT NOT NULL,             -- YYYY-MM-DD
  meal        TEXT NOT NULL,             -- 조식 | 중식 | 석식
  dishes      TEXT NOT NULL,             -- 메뉴 원문 (한 줄에 하나, 알레르기 번호 포함)
  calories    TEXT NOT NULL DEFAULT '',
  nutrition   TEXT NOT NULL DEFAULT '',
  origin      TEXT NOT NULL DEFAULT '',
  people      TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (school_code, date, meal)
);

-- 나이스에서 받은 기록 (급식이 없는 주도 '받았음'으로 기억해 다시 묻지 않는다)
CREATE TABLE neis_fetches (
  what       TEXT NOT NULL,              -- meals | timetable | schedule | classes
  scope      TEXT NOT NULL,              -- 학교 코드와 기간
  fetched_at TEXT NOT NULL,
  status     TEXT NOT NULL,              -- ok | empty | need_key | error
  message    TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (what, scope)
);

-- 학사력: 나이스에서 받은 일정 표시
--   ext_source: '' = 교사가 추가, 'neis' = 나이스 (다시 받으면 이것만 바뀐다), 'removed' = 교사가 지운 나이스 일정 (다시 받아도 되살리지 않음)
--   ext_key   : 다시 받을 때 같은 일정을 알아보는 값 (시작일|행사 이름)
--   day_kind  : holiday 휴일 | closure 휴업일 | vacation 방학 | work 업무 ('' = 학사력이 아닌 일정)
--   grades    : 학년 태그 ("1", "1,3"; 빈 값 = 전체)
--   hidden    : 목록에는 보이지 않고 날짜 색만 칠하는 일정 (토요휴업일)
ALTER TABLE events ADD COLUMN ext_source TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN ext_key    TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN day_kind   TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN grades     TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN hidden     INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_events_ext ON events(ext_source, ext_key);

-- 이미 있던 학사력은 업무로 본다 (이름 규칙은 설정을 저장하거나 나이스에서 받을 때 다시 적용)
UPDATE events SET day_kind = 'work' WHERE layer = 'annual';
