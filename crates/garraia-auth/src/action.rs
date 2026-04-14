//! `Action` — typed enum of the 22 capability strings seeded in migration 002.
//!
//! Each variant maps 1:1 to a `permissions.id` row. The seed is authoritative
//! — this enum must cover exactly the 22 rows and nothing else. See
//! `crates/garraia-workspace/migrations/002_rbac_and_audit.sql`.

/// Capability action. Variant names are PascalCase of the dot-format
/// `permissions.id` (e.g. `files.read` → `FilesRead`). Use
/// [`Action::as_str`] to get the dot-format id for DB/log emission and
/// [`Action::from_str`] for the inverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    // Files (Fase 3.5)
    FilesRead,
    FilesWrite,
    FilesDelete,
    FilesShare,
    // Chats (Fase 3.6)
    ChatsRead,
    ChatsWrite,
    ChatsModerate,
    // Memory (Fase 3.7)
    MemoryRead,
    MemoryWrite,
    MemoryDelete,
    // Tasks (Fase 3.8 Tier 1)
    TasksRead,
    TasksWrite,
    TasksAssign,
    TasksDelete,
    // Docs (Fase 3.8 Tier 2)
    DocsRead,
    DocsWrite,
    DocsDelete,
    // Group admin
    MembersManage,
    GroupSettings,
    GroupDelete,
    // Export / compliance
    ExportSelf,
    ExportGroup,
}

impl Action {
    /// Return the dot-format `permissions.id` string for this action.
    pub fn as_str(self) -> &'static str {
        match self {
            Action::FilesRead => "files.read",
            Action::FilesWrite => "files.write",
            Action::FilesDelete => "files.delete",
            Action::FilesShare => "files.share",
            Action::ChatsRead => "chats.read",
            Action::ChatsWrite => "chats.write",
            Action::ChatsModerate => "chats.moderate",
            Action::MemoryRead => "memory.read",
            Action::MemoryWrite => "memory.write",
            Action::MemoryDelete => "memory.delete",
            Action::TasksRead => "tasks.read",
            Action::TasksWrite => "tasks.write",
            Action::TasksAssign => "tasks.assign",
            Action::TasksDelete => "tasks.delete",
            Action::DocsRead => "docs.read",
            Action::DocsWrite => "docs.write",
            Action::DocsDelete => "docs.delete",
            Action::MembersManage => "members.manage",
            Action::GroupSettings => "group.settings",
            Action::GroupDelete => "group.delete",
            Action::ExportSelf => "export.self",
            Action::ExportGroup => "export.group",
        }
    }

    /// Parse a dot-format `permissions.id` string back into this enum.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "files.read" => Action::FilesRead,
            "files.write" => Action::FilesWrite,
            "files.delete" => Action::FilesDelete,
            "files.share" => Action::FilesShare,
            "chats.read" => Action::ChatsRead,
            "chats.write" => Action::ChatsWrite,
            "chats.moderate" => Action::ChatsModerate,
            "memory.read" => Action::MemoryRead,
            "memory.write" => Action::MemoryWrite,
            "memory.delete" => Action::MemoryDelete,
            "tasks.read" => Action::TasksRead,
            "tasks.write" => Action::TasksWrite,
            "tasks.assign" => Action::TasksAssign,
            "tasks.delete" => Action::TasksDelete,
            "docs.read" => Action::DocsRead,
            "docs.write" => Action::DocsWrite,
            "docs.delete" => Action::DocsDelete,
            "members.manage" => Action::MembersManage,
            "group.settings" => Action::GroupSettings,
            "group.delete" => Action::GroupDelete,
            "export.self" => Action::ExportSelf,
            "export.group" => Action::ExportGroup,
            _ => return None,
        })
    }

    /// All 22 actions in a fixed order. Used by the `can` matrix test.
    pub const ALL: [Action; 22] = [
        Action::FilesRead,
        Action::FilesWrite,
        Action::FilesDelete,
        Action::FilesShare,
        Action::ChatsRead,
        Action::ChatsWrite,
        Action::ChatsModerate,
        Action::MemoryRead,
        Action::MemoryWrite,
        Action::MemoryDelete,
        Action::TasksRead,
        Action::TasksWrite,
        Action::TasksAssign,
        Action::TasksDelete,
        Action::DocsRead,
        Action::DocsWrite,
        Action::DocsDelete,
        Action::MembersManage,
        Action::GroupSettings,
        Action::GroupDelete,
        Action::ExportSelf,
        Action::ExportGroup,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_has_22_unique_variants() {
        assert_eq!(Action::ALL.len(), 22);
        let mut seen = std::collections::HashSet::new();
        for a in Action::ALL {
            assert!(seen.insert(a.as_str()), "duplicate: {}", a.as_str());
        }
        assert_eq!(seen.len(), 22);
    }

    #[test]
    fn from_str_roundtrip_all() {
        for a in Action::ALL {
            assert_eq!(Action::from_str(a.as_str()), Some(a));
        }
        assert_eq!(Action::from_str("nope.invalid"), None);
    }
}
