# garraia-auth

Authentication and authorization for the GarraIA Group Workspace.

> **Status:** skeleton (GAR-391a). Real `verify_credential`, JWT issuance,
> Axum extractor, and authz suite arrive in GAR-391b/c/d. See
> [`docs/adr/0005-identity-provider.md`](../../docs/adr/0005-identity-provider.md)
> for the normative design.

## What ships in 391a

- `IdentityProvider` trait — frozen shape, four methods.
- `InternalProvider` struct — stub bodies returning `AuthError::NotImplemented`.
- `LoginPool` newtype — dedicated `garraia_login` BYPASSRLS pool, validated at
  construction time via `SELECT current_user`. **Cannot** be built from a raw
  `PgPool`; the boundary is enforced at compile time.
- `Principal`, `Identity`, `Credential`, `RequestCtx`, `AuthError` types.
- Migration `008_login_role.sql` (in `garraia-workspace`) creating the
  `garraia_login NOLOGIN BYPASSRLS` role with the four exact GRANTs from
  ADR 0005 §"Login role specification".

## Pool boundaries

| Pool | Role | RLS | Allowed callers |
|---|---|---|---|
| `garraia-workspace::Workspace` | `garraia_app` | enforced | every request handler |
| `garraia-auth::LoginPool` | `garraia_login` | bypassed | login endpoint only (391b) |

Mixing the two is a compile-time error: `LoginPool` is the only constructor
path for a credential-verification-capable pool, and it refuses any role
other than `garraia_login`.

## Tests

```
cargo test -p garraia-auth
```

Three smoke tests, each spinning its own `pgvector/pgvector:pg16` testcontainer:

1. `login_pool_rejects_non_login_role` — superuser connection → `AuthError::WrongRole`.
2. `login_pool_accepts_garraia_login_role` — promoted `garraia_login` → `Ok`.
3. `internal_provider_methods_return_not_implemented` — every stub method.
