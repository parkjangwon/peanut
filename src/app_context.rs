pub const DEFAULT_APP_ID: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppContext {
    pub app_id: String,
}

impl AppContext {
    pub fn default_app() -> Self {
        Self {
            app_id: DEFAULT_APP_ID.to_string(),
        }
    }

    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
        }
    }
}

impl Default for AppContext {
    fn default() -> Self {
        Self::default_app()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_app_context_uses_default_app_id() {
        assert_eq!(AppContext::default_app().app_id, DEFAULT_APP_ID);
        assert_eq!(AppContext::default().app_id, DEFAULT_APP_ID);
    }
}
