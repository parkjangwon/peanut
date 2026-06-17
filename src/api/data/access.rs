use super::*;

pub(crate) fn can_read_table(claims: &Claims, policy: &AccessPolicy) -> bool {
    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => claims.is_admin,
        POLICY_OWNER_PRIVATE | POLICY_AUTHENTICATED_SHARED_RW => true,
        POLICY_CUSTOM => can_apply_rule(
            claims,
            policy.rules.as_ref().and_then(|rules| rules.read.as_ref()),
            None,
        ),
        _ => false,
    }
}

pub(crate) fn can_create_row(claims: &Claims, policy: &AccessPolicy) -> bool {
    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => claims.is_admin,
        POLICY_OWNER_PRIVATE | POLICY_AUTHENTICATED_SHARED_RW => true,
        POLICY_CUSTOM => can_apply_rule(
            claims,
            policy
                .rules
                .as_ref()
                .and_then(|rules| rules.create.as_ref()),
            None,
        ),
        _ => false,
    }
}

pub(crate) fn can_read_row(
    claims: &Claims,
    policy: &AccessPolicy,
    owner_user_id: Option<&str>,
) -> bool {
    can_access_row_for_rule(
        claims,
        policy,
        owner_user_id,
        policy.rules.as_ref().and_then(|rules| rules.read.as_ref()),
    )
}

pub(crate) fn can_update_row(
    claims: &Claims,
    policy: &AccessPolicy,
    owner_user_id: Option<&str>,
) -> bool {
    can_access_row_for_rule(
        claims,
        policy,
        owner_user_id,
        policy
            .rules
            .as_ref()
            .and_then(|rules| rules.update.as_ref()),
    )
}

pub(crate) fn can_delete_row(
    claims: &Claims,
    policy: &AccessPolicy,
    owner_user_id: Option<&str>,
) -> bool {
    can_access_row_for_rule(
        claims,
        policy,
        owner_user_id,
        policy
            .rules
            .as_ref()
            .and_then(|rules| rules.delete.as_ref()),
    )
}

pub(crate) fn is_owner_scoped_read(claims: &Claims, policy: &AccessPolicy) -> bool {
    !claims.is_admin
        && match policy.mode.as_str() {
            POLICY_OWNER_PRIVATE => true,
            POLICY_CUSTOM => policy
                .rules
                .as_ref()
                .and_then(|rules| rules.read.as_ref())
                .is_some_and(|rule| rule.allow == "owner"),
            _ => false,
        }
}

fn can_access_row_for_rule(
    claims: &Claims,
    policy: &AccessPolicy,
    owner_user_id: Option<&str>,
    custom_rule: Option<&AccessRule>,
) -> bool {
    if claims.is_admin {
        return true;
    }

    match policy.mode.as_str() {
        POLICY_ADMIN_ONLY => false,
        POLICY_OWNER_PRIVATE => owner_user_id == Some(claims.sub.as_str()),
        POLICY_AUTHENTICATED_SHARED_RW => true,
        POLICY_CUSTOM => can_apply_rule(claims, custom_rule, owner_user_id),
        _ => false,
    }
}

fn can_apply_rule(claims: &Claims, rule: Option<&AccessRule>, owner_user_id: Option<&str>) -> bool {
    let Some(rule) = rule else {
        return false;
    };

    match rule.allow.as_str() {
        "admin" => claims.is_admin,
        "authenticated" => true,
        "owner" => owner_user_id.map_or(true, |owner| owner == claims.sub),
        _ => false,
    }
}
