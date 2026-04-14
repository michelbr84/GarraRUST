//! `can(&principal, action)` — central capability check.
//!
//! The matrix is a direct Rust mirror of the 63 `role_permissions` rows
//! seeded by `crates/garraia-workspace/migrations/002_rbac_and_audit.sql`:
//!
//! - Owner: all 22 permissions.
//! - Admin: all 22 except `group.delete` and `export.group` (= 20).
//! - Member: 11 — files.{read,write}, chats.{read,write}, memory.{read,write},
//!   tasks.{read,write}, docs.{read,write}, export.self.
//! - Guest: 6 — files.read, chats.{read,write}, tasks.read, docs.read, export.self.
//! - Child: 4 — chats.{read,write}, tasks.{read,write}.
//!
//! A principal with no group context (`group_id` or `role` is `None`) has
//! zero group permissions — `can` returns `false` unconditionally.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::action::Action;
use crate::role::Role;
use crate::types::Principal;

/// Static `Role × HashSet<Action>` map built once at first access. Mirrors
/// the 63 `role_permissions` rows from migration 002.
static ROLE_PERMISSIONS: LazyLock<HashMap<Role, HashSet<Action>>> = LazyLock::new(build_matrix);

fn build_matrix() -> HashMap<Role, HashSet<Action>> {
    use Action::*;
    let mut m: HashMap<Role, HashSet<Action>> = HashMap::new();

    // Owner: all 22.
    m.insert(Role::Owner, Action::ALL.iter().copied().collect());

    // Admin: all except GroupDelete and ExportGroup.
    m.insert(
        Role::Admin,
        [
            FilesRead,
            FilesWrite,
            FilesDelete,
            FilesShare,
            ChatsRead,
            ChatsWrite,
            ChatsModerate,
            MemoryRead,
            MemoryWrite,
            MemoryDelete,
            TasksRead,
            TasksWrite,
            TasksAssign,
            TasksDelete,
            DocsRead,
            DocsWrite,
            DocsDelete,
            MembersManage,
            GroupSettings,
            ExportSelf,
        ]
        .into_iter()
        .collect(),
    );

    // Member: 11.
    m.insert(
        Role::Member,
        [
            FilesRead, FilesWrite, ChatsRead, ChatsWrite, MemoryRead, MemoryWrite, TasksRead,
            TasksWrite, DocsRead, DocsWrite, ExportSelf,
        ]
        .into_iter()
        .collect(),
    );

    // Guest: 6.
    m.insert(
        Role::Guest,
        [
            FilesRead, ChatsRead, ChatsWrite, TasksRead, DocsRead, ExportSelf,
        ]
        .into_iter()
        .collect(),
    );

    // Child: 4.
    m.insert(
        Role::Child,
        [ChatsRead, ChatsWrite, TasksRead, TasksWrite]
            .into_iter()
            .collect(),
    );

    m
}

/// Returns `true` iff the principal's active group role grants `action`.
/// Returns `false` if the principal is not in any group context.
pub fn can(principal: &Principal, action: Action) -> bool {
    let Some(role) = principal.role else {
        return false;
    };
    ROLE_PERMISSIONS
        .get(&role)
        .map(|set| set.contains(&action))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn p(role: Option<Role>) -> Principal {
        Principal {
            user_id: Uuid::nil(),
            group_id: role.map(|_| Uuid::nil()),
            role,
        }
    }

    /// Table-driven test: expected_matrix[role_index][action_index] = bool.
    /// This is the authoritative mirror of migration 002's 63 rows.
    /// Any divergence = build failure.
    #[test]
    fn can_matrix_matches_seed() {
        // Expected permissions per role, hand-transcribed from
        // migrations/002_rbac_and_audit.sql lines 78-111.
        let expected: &[(Role, &[&str])] = &[
            (
                Role::Owner,
                &[
                    "files.read",
                    "files.write",
                    "files.delete",
                    "files.share",
                    "chats.read",
                    "chats.write",
                    "chats.moderate",
                    "memory.read",
                    "memory.write",
                    "memory.delete",
                    "tasks.read",
                    "tasks.write",
                    "tasks.assign",
                    "tasks.delete",
                    "docs.read",
                    "docs.write",
                    "docs.delete",
                    "members.manage",
                    "group.settings",
                    "group.delete",
                    "export.self",
                    "export.group",
                ],
            ),
            (
                Role::Admin,
                &[
                    "files.read",
                    "files.write",
                    "files.delete",
                    "files.share",
                    "chats.read",
                    "chats.write",
                    "chats.moderate",
                    "memory.read",
                    "memory.write",
                    "memory.delete",
                    "tasks.read",
                    "tasks.write",
                    "tasks.assign",
                    "tasks.delete",
                    "docs.read",
                    "docs.write",
                    "docs.delete",
                    "members.manage",
                    "group.settings",
                    "export.self",
                ],
            ),
            (
                Role::Member,
                &[
                    "files.read",
                    "files.write",
                    "chats.read",
                    "chats.write",
                    "memory.read",
                    "memory.write",
                    "tasks.read",
                    "tasks.write",
                    "docs.read",
                    "docs.write",
                    "export.self",
                ],
            ),
            (
                Role::Guest,
                &[
                    "files.read",
                    "chats.read",
                    "chats.write",
                    "tasks.read",
                    "docs.read",
                    "export.self",
                ],
            ),
            (
                Role::Child,
                &["chats.read", "chats.write", "tasks.read", "tasks.write"],
            ),
        ];

        // Sanity: counts match the plan (22/20/11/6/4 = 63).
        let counts: Vec<usize> = expected.iter().map(|(_, v)| v.len()).collect();
        assert_eq!(counts, vec![22, 20, 11, 6, 4]);
        assert_eq!(counts.iter().sum::<usize>(), 63);

        // Build expected sets per role.
        let expected_map: HashMap<Role, HashSet<Action>> = expected
            .iter()
            .map(|(r, ids)| {
                let set: HashSet<Action> = ids
                    .iter()
                    .map(|id| Action::from_str(id).unwrap_or_else(|| panic!("bad id: {id}")))
                    .collect();
                (*r, set)
            })
            .collect();

        // Walk the full 5 × 22 = 110 matrix.
        let mut checks = 0;
        let mut trues = 0;
        for role in [
            Role::Owner,
            Role::Admin,
            Role::Member,
            Role::Guest,
            Role::Child,
        ] {
            let expected_set = &expected_map[&role];
            for action in Action::ALL {
                let expected_bool = expected_set.contains(&action);
                let actual = can(&p(Some(role)), action);
                assert_eq!(
                    actual,
                    expected_bool,
                    "mismatch: role={:?} action={} expected={} got={}",
                    role,
                    action.as_str(),
                    expected_bool,
                    actual
                );
                checks += 1;
                if actual {
                    trues += 1;
                }
            }
        }
        assert_eq!(checks, 110, "must cover all 5 × 22 = 110 pairs");
        assert_eq!(trues, 63, "must total exactly 63 true outcomes");
    }

    #[test]
    fn no_role_means_no_permission() {
        let principal = p(None);
        for a in Action::ALL {
            assert!(!can(&principal, a), "no-role principal must not have {:?}", a);
        }
    }
}
