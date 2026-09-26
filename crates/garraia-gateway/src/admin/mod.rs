pub mod audit;
/// #1381: o registro de capacidades para o console.
pub mod capabilities;
pub mod handlers;
pub mod mcp;
pub mod mcp_templates;
pub mod middleware;
pub mod observability;
pub mod providers;
pub mod rbac;
pub mod recovery;
pub mod routes;
pub mod secrets;
pub mod shared;
pub mod store;
pub mod totp;
pub mod users;
/// ADR 0025: a Access Policy v2 do WhatsApp pela API admin (#1402).
pub mod whatsapp_access;
/// #1420: `GET /admin/api/whatsapp/doctor` — o "Test WhatsApp" do console,
/// pelo mesmo motor do `garraia doctor whatsapp`.
pub mod whatsapp_doctor;
