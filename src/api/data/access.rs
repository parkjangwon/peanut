use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowAccessAction {
    Read,
    Update,
    Delete,
}

pub(crate) fn evaluate_rule(
    rule: Option<&str>,
    claims: &Claims,
    owner_user_id: Option<&str>,
) -> bool {
    match rule {
        Some(RULE_PUBLIC) => true,
        Some(RULE_AUTHENTICATED) => true,
        Some(RULE_ADMIN) => claims.is_admin,
        Some(RULE_OWNER) => {
            if let Some(owner_user_id) = owner_user_id {
                owner_user_id == claims.sub
            } else {
                true
            }
        }
        _ => false,
    }
}

pub(crate) fn can_read_table(claims: &Claims, policy: &AccessPolicy) -> bool {
    if let Some(rules) = &policy.rules {
        if let Some(read_rule) = rules.read.as_deref() {
            return evaluate_rule(Some(read_rule), claims, None);
        }
    }

    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => claims.is_admin,
        POLICY_OWNER_PRIVATE | POLICY_AUTHENTICATED_SHARED_RW => true,
        _ => false,
    }
}

pub(crate) fn can_write_table(claims: &Claims, policy: &AccessPolicy) -> bool {
    if let Some(rules) = &policy.rules {
        if let Some(create_rule) = rules.create.as_deref() {
            return evaluate_rule(Some(create_rule), claims, None);
        }
    }

    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => claims.is_admin,
        POLICY_OWNER_PRIVATE | POLICY_AUTHENTICATED_SHARED_RW => true,
        _ => false,
    }
}

pub(crate) fn can_access_row(
    claims: &Claims,
    policy: &AccessPolicy,
    owner_user_id: Option<&str>,
    action: RowAccessAction,
) -> bool {
    if claims.is_admin {
        return true;
    }

    if let Some(rules) = &policy.rules {
        let rule = match action {
            RowAccessAction::Read => rules.read.as_deref(),
            RowAccessAction::Update => rules.update.as_deref(),
            RowAccessAction::Delete => rules.delete.as_deref(),
        };
        if let Some(rule) = rule {
            return evaluate_rule(Some(rule), claims, owner_user_id);
        }
    }

    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => false,
        POLICY_OWNER_PRIVATE => owner_user_id == Some(claims.sub.as_str()),
        POLICY_AUTHENTICATED_SHARED_RW => true,
        _ => false,
    }
}
