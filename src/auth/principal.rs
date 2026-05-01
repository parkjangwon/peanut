#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorKind {
    User,
    ServiceToken,
    AppKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub actor_id: String,
    pub actor_kind: ActorKind,
    pub app_id: Option<String>,
    pub is_admin: bool,
    pub scopes: Vec<String>,
}

impl Principal {
    pub fn user(user_id: impl Into<String>, is_admin: bool) -> Self {
        Self {
            actor_id: user_id.into(),
            actor_kind: ActorKind::User,
            app_id: None,
            is_admin,
            scopes: if is_admin {
                vec!["admin:all".to_string()]
            } else {
                Vec::new()
            },
        }
    }

    pub fn service_token(user_id: impl Into<String>, is_admin: bool) -> Self {
        Self {
            actor_id: user_id.into(),
            actor_kind: ActorKind::ServiceToken,
            app_id: Some(crate::app_context::DEFAULT_APP_ID.to_string()),
            is_admin,
            scopes: if is_admin {
                vec!["admin:all".to_string()]
            } else {
                Vec::new()
            },
        }
    }

    pub fn app_key(
        key_id: impl Into<String>,
        app_id: impl Into<String>,
        is_admin: bool,
        scopes: Vec<String>,
    ) -> Self {
        Self {
            actor_id: key_id.into(),
            actor_kind: ActorKind::AppKey,
            app_id: Some(app_id.into()),
            is_admin,
            scopes,
        }
    }

    #[allow(dead_code)]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes
            .iter()
            .any(|value| value == "admin:all" || value == scope)
    }
}
