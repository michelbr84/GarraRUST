//! Shared test harness for the GAR-392 RLS matrix suite.
//!
//! Plan 0013 path C — see:
//!   - `plans/0013-gar-391d-392-authz-suite.md` (amendment header)
//!   - `docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md`
//!
//! Submodules land across Tasks 2–8 of the plan. Task 2 seeds `cases`.

#![cfg(feature = "test-support")]
#![allow(dead_code)] // harness modules are consumed incrementally across tasks

pub mod cases;
pub mod harness;
pub mod oracle;
pub mod tenants;
