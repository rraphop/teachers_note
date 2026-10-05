-- 오늘 화면 메모 (윈도우 스티커 메모처럼 날이 바뀌어도 그대로 남는 메모)
--   pos        : 작을수록 앞 (새 메모는 맨 앞)
--   deleted_at : 지운 메모 (바로 [되돌리기] 할 수 있게 표에는 남겨 둔다)
CREATE TABLE memos (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  body       TEXT NOT NULL DEFAULT '',
  color      TEXT NOT NULL DEFAULT 'yellow',
  pos        INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE INDEX idx_memos_live ON memos(deleted_at, pos);
