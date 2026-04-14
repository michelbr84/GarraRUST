//! `Role` — typed enum mirroring `group_members.role` + `roles` seed in
//! migration 002. Five variants ordered by tier (higher = more privilege).

use serde::{Deserialize, Serialize};

/// Capability tier for a user within a specific group. The runtime authority
/// is `group_members.role` (text column with CHECK constraint from migration
/// 001). This enum is the strongly-typed Rust mirror used by the `can`
/// function and by the `Principal` extractor (GAR-391c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Member,
    Guest,
    Child,
}

impl Role {
    /// Numeric tier matching the `roles` seed in migration 002.
    /// owner=100, admin=80, member=50, guest=20, child=10.
    pub fn tier(self) -> u8 {
        match self {
            Role::Owner => 100,
            Role::Admin => 80,
            Role::Member => 50,
            Role::Guest => 20,
            Role::Child => 10,
        }
    }

    /// Parse from the exact lowercase string stored in `group_members.role`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Role::Owner),
            "admin" => Some(Role::Admin),
            "member" => Some(Role::Member),
            "guest" => Some(Role::Guest),
            "child" => Some(Role::Child),
            _ => None,
        }
    }

    /// Serialize back to the lowercase text form used by the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Guest => "guest",
            Role::Child => "child",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_matches_seed() {
        assert_eq!(Role::Owner.tier(), 100);
        assert_eq!(Role::Admin.tier(), 80);
        assert_eq!(Role::Member.tier(), 50);
        assert_eq!(Role::Guest.tier(), 20);
        assert_eq!(Role::Child.tier(), 10);
    }

    #[test]
    fn from_str_roundtrip() {
        for r in [
            Role::Owner,
            Role::Admin,
            Role::Member,
            Role::Guest,
            Role::Child,
        ] {
            assert_eq!(Role::from_str(r.as_str()), Some(r));
        }
        assert_eq!(Role::from_str("nope"), None);
        assert_eq!(Role::from_str("Owner"), None); // case-sensitive
    }

    #[test]
    fn serde_snake_case() {
        let json = serde_json::to_string(&Role::Owner).unwrap();
        assert_eq!(json, "\"owner\"");
        let back: Role = serde_json::from_str("\"child\"").unwrap();
        assert_eq!(back, Role::Child);
    }
}
