use serde::{Deserialize, Serialize};

/// Admin user roles with increasing privilege levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Operator,
    Admin,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Operator => "operator",
            Role::Admin => "admin",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "viewer" => Some(Role::Viewer),
            "operator" => Some(Role::Operator),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }

    pub fn privilege_level(&self) -> u8 {
        match self {
            Role::Viewer => 0,
            Role::Operator => 1,
            Role::Admin => 2,
        }
    }
}

/// Resources that can be acted upon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Secrets,
    Config,
    Providers,
    Memory,
    Tools,
    Channels,
    Sessions,
    AuditLog,
    Users,
    Alerts,
    Metrics,
}

/// Actions that can be performed on resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Create,
    Update,
    Delete,
    Execute,
}

/// Check whether a given role has permission to perform an action on a resource.
///
/// Permissions matrix:
///   - **Viewer**: read-only on most resources (no secrets write, no user mgmt, no danger zone)
///   - **Operator**: read + write on operational resources (providers, config, memory, tools, channels, sessions)
///   - **Admin**: full access including users, danger zone actions, and audit logs
pub fn check_permission(role: Role, resource: Resource, action: Action) -> bool {
    match role {
        Role::Admin => true,
        Role::Operator => match resource {
            Resource::Users => false,
            Resource::AuditLog => matches!(action, Action::Read),
            _ => true,
        },
        Role::Viewer => match resource {
            Resource::Users | Resource::AuditLog => false,
            _ => matches!(action, Action::Read),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_has_full_access() {
        assert!(check_permission(
            Role::Admin,
            Resource::Users,
            Action::Delete
        ));
        assert!(check_permission(
            Role::Admin,
            Resource::Secrets,
            Action::Create
        ));
        assert!(check_permission(
            Role::Admin,
            Resource::AuditLog,
            Action::Read
        ));
    }

    #[test]
    fn operator_cannot_manage_users() {
        assert!(!check_permission(
            Role::Operator,
            Resource::Users,
            Action::Create
        ));
        assert!(!check_permission(
            Role::Operator,
            Resource::Users,
            Action::Delete
        ));
    }

    #[test]
    fn operator_can_manage_operational_resources() {
        assert!(check_permission(
            Role::Operator,
            Resource::Secrets,
            Action::Create
        ));
        assert!(check_permission(
            Role::Operator,
            Resource::Config,
            Action::Update
        ));
        assert!(check_permission(
            Role::Operator,
            Resource::Providers,
            Action::Delete
        ));
    }

    #[test]
    fn operator_can_read_audit_log() {
        assert!(check_permission(
            Role::Operator,
            Resource::AuditLog,
            Action::Read
        ));
        assert!(!check_permission(
            Role::Operator,
            Resource::AuditLog,
            Action::Delete
        ));
    }

    #[test]
    fn viewer_is_read_only() {
        assert!(check_permission(
            Role::Viewer,
            Resource::Secrets,
            Action::Read
        ));
        assert!(!check_permission(
            Role::Viewer,
            Resource::Secrets,
            Action::Create
        ));
        assert!(!check_permission(
            Role::Viewer,
            Resource::Config,
            Action::Update
        ));
        assert!(!check_permission(
            Role::Viewer,
            Resource::Users,
            Action::Read
        ));
    }

    #[test]
    fn role_round_trip() {
        for role in [Role::Viewer, Role::Operator, Role::Admin] {
            assert_eq!(Role::from_str(role.as_str()), Some(role));
        }
    }
}
