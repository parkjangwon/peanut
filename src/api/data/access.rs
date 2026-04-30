use super::*;

pub(crate) fn can_read_table(claims: &Claims, policy: &AccessPolicy) -> bool {
    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => claims.is_admin,
        POLICY_OWNER_PRIVATE | POLICY_AUTHENTICATED_SHARED_RW => true,
        _ => false,
    }
}

pub(crate) fn can_write_table(claims: &Claims, policy: &AccessPolicy) -> bool {
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
) -> bool {
    if claims.is_admin {
        return true;
    }

    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => false,
        POLICY_OWNER_PRIVATE => owner_user_id == Some(claims.sub.as_str()),
        POLICY_AUTHENTICATED_SHARED_RW => true,
        _ => false,
    }
}
