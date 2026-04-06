//! Role-Based Access Control for the GarraIA admin plane.
//!
//! Phase 7.1 enhancement: granular permissions system with built-in and custom roles.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ── Permissions ───────────────────────────────────────────────────────────────

/// Fine-grained permissions that can be assigned to a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    // User management
    ManageUsers,
    // Channel management
    ManageChannels,
    // Plugin / MCP server management
    ManagePlugins,
    // Read-only access to logs and audit trail
    ViewLogs,
    // Send messages via the admin API
    SendMessages,
    // Global settings (gateway config, flags)
    ManageSettings,
    // Secret / credential management
    ManageSecrets,
    // LLM provider management
    ManageProviders,
    // Session management (disconnect, inspect)
    ManageSessions,
    // Read-only metrics and Prometheus endpoint
    ViewMetrics,
    // Danger-zone actions (purge, full reset)
    DangerZone,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::ManageUsers => "manage_users",
            Permission::ManageChannels => "manage_channels",
            Permission::ManagePlugins => "manage_plugins",
            Permission::ViewLogs => "view_logs",
            Permission::SendMessages => "send_messages",
            Permission::ManageSettings => "manage_settings",
            Permission::ManageSecrets => "manage_secrets",
            Permission::ManageProviders => "manage_providers",
            Permission::ManageSessions => "manage_sessions",
            Permission::ViewMetrics => "view_metrics",
            Permission::DangerZone => "danger_zone",
        }
    }

    /// All defined permissions.
    pub fn all() -> &'static [Permission] {
        &[
            Permission::ManageUsers,
            Permission::ManageChannels,
            Permission::ManagePlugins,
            Permission::ViewLogs,
            Permission::SendMessages,
            Permission::ManageSettings,
            Permission::ManageSecrets,
            Permission::ManageProviders,
            Permission::ManageSessions,
            Permission::ViewMetrics,
            Permission::DangerZone,
        ]
    }
}

// ── Built-in roles ────────────────────────────────────────────────────────────

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

    #[allow(clippy::should_implement_trait)]
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

    /// Return the canonical permission set for this built-in role.
    pub fn default_permissions(&self) -> HashSet<Permission> {
        match self {
            Role::Admin => Permission::all().iter().copied().collect(),
            Role::Operator => [
                Permission::ManageChannels,
                Permission::ManagePlugins,
                Permission::ViewLogs,
                Permission::SendMessages,
                Permission::ManageSettings,
                Permission::ManageSecrets,
                Permission::ManageProviders,
                Permission::ManageSessions,
                Permission::ViewMetrics,
            ]
            .iter()
            .copied()
            .collect(),
            Role::Viewer => [Permission::ViewLogs, Permission::ViewMetrics]
                .iter()
                .copied()
                .collect(),
        }
    }
}

// ── Custom roles ──────────────────────────────────────────────────────────────

/// A named role with an explicit permission set (used for custom / project-specific roles).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomRole {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub permissions: HashSet<Permission>,
}

impl CustomRole {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        permissions: impl IntoIterator<Item = Permission>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            permissions: permissions.into_iter().collect(),
        }
    }

    pub fn has_permission(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }
}

// ── Legacy resource/action model (kept for backwards compatibility) ───────────

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
    McpServers,
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

/// Check whether a role has a specific fine-grained [`Permission`].
pub fn has_permission(role: Role, permission: Permission) -> bool {
    role.default_permissions().contains(&permission)
}

/// Check whether a custom role has a specific fine-grained [`Permission`].
pub fn custom_role_has_permission(role: &CustomRole, permission: Permission) -> bool {
    role.has_permission(permission)
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

    #[test]
    fn admin_has_all_permissions() {
        for &perm in Permission::all() {
            assert!(has_permission(Role::Admin, perm), "admin missing {perm:?}");
        }
    }

    #[test]
    fn viewer_has_only_view_permissions() {
        assert!(has_permission(Role::Viewer, Permission::ViewLogs));
        assert!(has_permission(Role::Viewer, Permission::ViewMetrics));
        assert!(!has_permission(Role::Viewer, Permission::ManageUsers));
        assert!(!has_permission(Role::Viewer, Permission::DangerZone));
    }

    #[test]
    fn operator_cannot_danger_zone_or_manage_users() {
        assert!(!has_permission(Role::Operator, Permission::DangerZone));
        assert!(!has_permission(Role::Operator, Permission::ManageUsers));
        assert!(has_permission(Role::Operator, Permission::ManageProviders));
        assert!(has_permission(Role::Operator, Permission::ManageSecrets));
    }

    #[test]
    fn custom_role_permission_check() {
        let role = CustomRole::new(
            "custom-1",
            "Limited Ops",
            [Permission::ViewLogs, Permission::SendMessages],
        );
        assert!(custom_role_has_permission(&role, Permission::ViewLogs));
        assert!(custom_role_has_permission(&role, Permission::SendMessages));
        assert!(!custom_role_has_permission(&role, Permission::ManageUsers));
        assert!(!custom_role_has_permission(&role, Permission::DangerZone));
    }
}
