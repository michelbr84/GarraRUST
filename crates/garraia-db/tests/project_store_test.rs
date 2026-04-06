//! Integration tests for project_store (Phase 2.1, 2.3, 2.4, 7.4).
//!
//! Tests project CRUD, RAG file indexing, templates, and GDPR operations
//! using an in-memory SQLite database.

use garraia_db::SessionStore;

fn make_store() -> SessionStore {
    SessionStore::in_memory().expect("in-memory store should open")
}

// ── Phase 2.1: Project CRUD ─────────────────────────────────────────────────

#[test]
fn create_project_returns_valid_fields() {
    let store = make_store();
    let project = store
        .create_project("my-project", "/home/user/project", Some("desc"), Some("owner-1"), None)
        .expect("should create project");

    assert!(!project.id.is_empty(), "id should be non-empty UUID");
    assert_eq!(project.name, "my-project");
    assert_eq!(project.path, "/home/user/project");
    assert_eq!(project.description.as_deref(), Some("desc"));
    assert_eq!(project.owner_id.as_deref(), Some("owner-1"));
    assert!(!project.created_at.is_empty());
}

#[test]
fn get_project_returns_none_for_missing_id() {
    let store = make_store();
    let result = store.get_project("nonexistent").expect("should not error");
    assert!(result.is_none());
}

#[test]
fn list_projects_filters_by_owner() {
    let store = make_store();
    store.create_project("a", "/a", None, Some("alice"), None).unwrap();
    store.create_project("b", "/b", None, Some("bob"), None).unwrap();
    store.create_project("c", "/c", None, Some("alice"), None).unwrap();

    let alice_projects = store.list_projects(Some("alice")).unwrap();
    assert_eq!(alice_projects.len(), 2);
    assert!(alice_projects.iter().all(|p| p.owner_id.as_deref() == Some("alice")));

    let all = store.list_projects(None).unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn update_project_partial_fields() {
    let store = make_store();
    let project = store.create_project("original", "/orig", Some("old desc"), None, None).unwrap();

    // Update only name
    let updated = store
        .update_project(&project.id, Some("renamed"), None, None, None)
        .unwrap()
        .expect("should return updated project");

    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.path, "/orig", "unchanged fields should persist");
    assert_eq!(updated.description.as_deref(), Some("old desc"));
}

#[test]
fn update_project_with_settings() {
    let store = make_store();
    let project = store.create_project("proj", "/p", None, None, None).unwrap();

    let new_settings = serde_json::json!({"model": "gpt-4o", "temperature": 0.7});
    let updated = store
        .update_project(&project.id, None, None, None, Some(&new_settings))
        .unwrap()
        .expect("should return updated project");

    assert_eq!(updated.settings["model"], "gpt-4o");
    assert_eq!(updated.settings["temperature"], 0.7);
}

#[test]
fn update_nonexistent_project_returns_none() {
    let store = make_store();
    let result = store
        .update_project("nonexistent", Some("name"), None, None, None)
        .unwrap();
    assert!(result.is_none());
}

#[test]
fn delete_project_returns_false_for_missing() {
    let store = make_store();
    assert!(!store.delete_project("nonexistent").unwrap());
}

#[test]
fn delete_project_removes_it() {
    let store = make_store();
    let project = store.create_project("del-me", "/tmp", None, None, None).unwrap();
    assert!(store.delete_project(&project.id).unwrap());
    assert!(store.get_project(&project.id).unwrap().is_none());
}

// ── Phase 2.3: RAG File Indexing ────────────────────────────────────────────

#[test]
fn index_file_and_retrieve() {
    let store = make_store();
    let project = store.create_project("rag-proj", "/rag", None, None, None).unwrap();

    let file = store
        .index_project_file(&project.id, "src/main.rs", Some("abc123"), None, Some(2048))
        .unwrap();

    assert_eq!(file.project_id, project.id);
    assert_eq!(file.file_path, "src/main.rs");
    assert_eq!(file.content_hash.as_deref(), Some("abc123"));
    assert_eq!(file.file_size, Some(2048));

    let files = store.get_project_files(&project.id).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file_path, "src/main.rs");
}

#[test]
fn index_file_upserts_on_conflict() {
    let store = make_store();
    let project = store.create_project("upsert-proj", "/up", None, None, None).unwrap();

    store.index_project_file(&project.id, "a.rs", Some("hash1"), None, Some(100)).unwrap();
    store.index_project_file(&project.id, "a.rs", Some("hash2"), None, Some(200)).unwrap();

    let files = store.get_project_files(&project.id).unwrap();
    assert_eq!(files.len(), 1, "should upsert, not duplicate");
    assert_eq!(files[0].content_hash.as_deref(), Some("hash2"));
    assert_eq!(files[0].file_size, Some(200));
}

#[test]
fn needs_reindex_detects_changes() {
    let store = make_store();
    let project = store.create_project("reindex-proj", "/r", None, None, None).unwrap();

    // Not indexed -> needs reindex
    assert!(store.needs_reindex(&project.id, "foo.rs", "hash1").unwrap());

    // Index it
    store.index_project_file(&project.id, "foo.rs", Some("hash1"), None, None).unwrap();

    // Same hash -> no reindex needed
    assert!(!store.needs_reindex(&project.id, "foo.rs", "hash1").unwrap());

    // Different hash -> needs reindex
    assert!(store.needs_reindex(&project.id, "foo.rs", "hash_changed").unwrap());
}

// ── Phase 2.4: Templates ───────────────────────────────────────────────────

#[test]
fn template_crud_lifecycle() {
    let store = make_store();

    let template = store
        .create_template(
            "rust-cli",
            Some("A Rust CLI template"),
            Some("You are a Rust expert."),
            Some("bash,edit"),
            Some("code"),
        )
        .unwrap();

    assert_eq!(template.name, "rust-cli");
    assert_eq!(template.system_prompt.as_deref(), Some("You are a Rust expert."));

    // List
    let all = store.list_templates().unwrap();
    assert_eq!(all.len(), 1);

    // Get
    let fetched = store.get_template(&template.id).unwrap().expect("should exist");
    assert_eq!(fetched.name, "rust-cli");

    // Delete
    assert!(store.delete_template(&template.id).unwrap());
    assert!(store.get_template(&template.id).unwrap().is_none());
}

#[test]
fn create_project_from_template_inherits_settings() {
    let store = make_store();
    let template = store
        .create_template("tmpl", Some("desc"), Some("system prompt"), Some("bash"), Some("code"))
        .unwrap();

    let project = store
        .create_project_from_template(&template.id, "from-tmpl", "/ft", Some("user-1"))
        .unwrap();

    assert_eq!(project.name, "from-tmpl");
    assert_eq!(project.owner_id.as_deref(), Some("user-1"));
    assert_eq!(
        project.settings["template_id"].as_str(),
        Some(template.id.as_str())
    );
    assert_eq!(project.settings["system_prompt"].as_str(), Some("system prompt"));
    assert_eq!(project.settings["tools_enabled"].as_str(), Some("bash"));
    assert_eq!(project.settings["default_mode"].as_str(), Some("code"));
}

#[test]
fn create_project_from_nonexistent_template_errors() {
    let store = make_store();
    let result = store.create_project_from_template("nonexistent", "proj", "/p", None);
    assert!(result.is_err());
}

// ── Phase 7.4: GDPR ────────────────────────────────────────────────────────

#[test]
fn data_retention_record_lifecycle() {
    let store = make_store();

    let record = store
        .create_data_retention("user", "u-123", "2020-01-01 00:00:00")
        .unwrap();

    assert_eq!(record.entity_type, "user");
    assert_eq!(record.entity_id, "u-123");
    assert!(record.deleted_at.is_none());

    // Should appear in expired list (since 2020 is in the past)
    let expired = store.list_expired_retention_records().unwrap();
    assert_eq!(expired.len(), 1);

    // Mark deleted
    store.mark_retention_deleted(&expired[0].id).unwrap();

    // Should no longer appear
    let expired_after = store.list_expired_retention_records().unwrap();
    assert!(expired_after.is_empty());
}
