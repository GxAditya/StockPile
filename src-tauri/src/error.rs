use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    Conflict(String),

    #[error("insufficient stock for {product}: requested {requested}, available {available}")]
    Shortfall {
        product: String,
        requested: i64,
        available: i64,
    },

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("agent error: {0}")]
    Agent(String),

    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::Validation(_) => "VALIDATION",
            AppError::Conflict(_) => "CONFLICT",
            AppError::Shortfall { .. } => "SHORTFALL",
            AppError::Db(_) => "DB",
            AppError::Agent(_) => "AGENT",
            AppError::Internal(_) => "INTERNAL",
        }
    }

    /// Structured payload for sale shortfalls so the UI can render the exact gap.
    pub fn details(&self) -> Option<serde_json::Value> {
        match self {
            AppError::Shortfall {
                product,
                requested,
                available,
            } => Some(serde_json::json!({
                "product": product,
                "requested": requested,
                "available": available,
                "shortBy": requested - available,
            })),
            _ => None,
        }
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        AppError::Validation(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal(msg.into())
    }

    pub fn agent(msg: impl Into<String>) -> Self {
        AppError::Agent(msg.into())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Internal(format!("json: {e}"))
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 3)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.serialize_field("details", &self.details())?;
        st.end()
    }
}

impl From<csv::Error> for AppError {
    fn from(e: csv::Error) -> Self {
        AppError::Internal(format!("csv: {e}"))
    }
}