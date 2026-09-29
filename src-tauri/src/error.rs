use serde::{Serialize, Serializer};

/// 화면에 그대로 보여 줄 수 있는 쉬운 한국어 오류
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("데이터 저장소 오류: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("파일 작업 오류: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Msg(String),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Msg(s.into())
    }
    pub fn invalid(s: impl Into<String>) -> Self {
        AppError::Invalid(s.into())
    }
    pub fn not_found(s: impl Into<String>) -> Self {
        AppError::NotFound(s.into())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Msg(format!("데이터 형식 오류: {e}"))
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
