use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub status: u16,
    pub code: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(status: u16, code: &str, message: &str) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn invalid(message: &str) -> Self {
        Self::new(400, "invalid_request", message)
    }
    pub fn unauthorized() -> Self {
        Self::new(
            401,
            "unauthorized",
            "A valid OpenBao bearer token is required",
        )
    }
    pub fn forbidden() -> Self {
        Self::new(403, "permission_denied", "OpenBao denied this operation")
    }
    pub fn unavailable() -> Self {
        Self::new(503, "backend_unavailable", "OpenBao is unavailable")
    }
    pub fn internal() -> Self {
        Self::new(
            500,
            "internal_error",
            "The operation could not be completed",
        )
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
