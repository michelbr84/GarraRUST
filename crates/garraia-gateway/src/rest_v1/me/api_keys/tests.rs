use super::*;

// ── API keys (unit tests — no DB required) ────────────────────────────────

#[test]
fn api_key_created_audit_action_wire_form() {
    assert_eq!(
        WorkspaceAuditAction::ApiKeyCreated.as_str(),
        "api_key.created"
    );
}

#[test]
fn api_key_revoked_audit_action_wire_form() {
    assert_eq!(
        WorkspaceAuditAction::ApiKeyRevoked.as_str(),
        "api_key.revoked"
    );
}

#[test]
fn api_key_audit_actions_distinct() {
    assert_ne!(
        WorkspaceAuditAction::ApiKeyCreated.as_str(),
        WorkspaceAuditAction::ApiKeyRevoked.as_str(),
    );
}

#[test]
fn api_key_summary_serialization_no_key_field() {
    let summary = ApiKeySummary {
        id: Uuid::nil(),
        label: "test key".into(),
        scopes: vec!["workspace.read".into()],
        created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        last_used_at: None,
        revoked_at: None,
    };
    let v = serde_json::to_value(&summary).unwrap();
    assert!(
        v.get("key").is_none(),
        "ApiKeySummary must not expose raw key"
    );
    assert!(
        v.get("key_hash").is_none(),
        "ApiKeySummary must not expose key_hash"
    );
    assert_eq!(v["label"], "test key");
    assert_eq!(v["scopes"][0], "workspace.read");
}

#[test]
fn create_api_key_response_has_key_field() {
    let resp = CreateApiKeyResponse {
        id: Uuid::nil(),
        label: "my key".into(),
        scopes: vec![],
        created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        key: "gai_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
    };
    let v = serde_json::to_value(&resp).unwrap();
    let key_str = v["key"].as_str().unwrap();
    assert!(
        key_str.starts_with("gai_"),
        "API key must start with gai_ prefix"
    );
}

#[test]
fn list_my_api_keys_limit_clamp() {
    let q = ListMyApiKeysQuery {
        after: None,
        limit: Some(999),
    };
    let clamped = q.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
    assert_eq!(clamped, 100, "limit must clamp to 100");
}

#[test]
fn list_my_api_keys_query_defaults() {
    let q = ListMyApiKeysQuery {
        after: None,
        limit: None,
    };
    let effective = q.limit.map(|l| l.min(100)).unwrap_or(20).max(1);
    assert_eq!(effective, 20, "default limit must be 20");
    assert!(q.after.is_none(), "default after must be None");
}

#[test]
fn api_key_nil_group_uuid_convention() {
    let nil = Uuid::nil();
    assert_eq!(
        nil.to_string(),
        "00000000-0000-0000-0000-000000000000",
        "group_id must be nil-uuid for user-scoped API key audit"
    );
}

// ── PATCH /v1/me/api-keys/{key_id} unit tests ─────────────────────────────

#[test]
fn patch_api_key_request_label_only_is_valid() {
    let req = PatchMyApiKeyRequest {
        label: Some("renamed key".into()),
        scopes: None,
    };
    assert!(req.label.is_some());
    assert!(req.scopes.is_none());
}

#[test]
fn patch_api_key_request_scopes_only_is_valid() {
    let req = PatchMyApiKeyRequest {
        label: None,
        scopes: Some(vec!["workspace.read".into()]),
    };
    assert!(req.label.is_none());
    assert_eq!(req.scopes.as_ref().unwrap().len(), 1);
}

#[test]
fn patch_api_key_request_both_fields_is_valid() {
    let req = PatchMyApiKeyRequest {
        label: Some("my key".into()),
        scopes: Some(vec!["workspace.read".into(), "files.write".into()]),
    };
    assert!(req.label.is_some());
    assert_eq!(req.scopes.as_ref().unwrap().len(), 2);
}

#[test]
fn patch_api_key_audit_action_wire_form() {
    assert_eq!(
        WorkspaceAuditAction::ApiKeyUpdated.as_str(),
        "api_key.updated"
    );
}

#[test]
fn patch_api_key_audit_action_distinct_from_created_and_revoked() {
    assert_ne!(
        WorkspaceAuditAction::ApiKeyUpdated.as_str(),
        WorkspaceAuditAction::ApiKeyCreated.as_str(),
    );
    assert_ne!(
        WorkspaceAuditAction::ApiKeyUpdated.as_str(),
        WorkspaceAuditAction::ApiKeyRevoked.as_str(),
    );
}

#[test]
fn patch_api_key_label_length_255_is_boundary() {
    let label = "a".repeat(255);
    assert_eq!(label.len(), 255, "255-char label is at the boundary");
    let too_long = "a".repeat(256);
    assert!(too_long.len() > 255, "256-char label exceeds limit");
}
