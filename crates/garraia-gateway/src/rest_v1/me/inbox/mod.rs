//! Caixas de entrada paginadas por cursor do `/v1/me/*`, cada uma restrita a um
//! grupo (`X-Group-Id`) e lida sob FORCE RLS com `app.current_group_id`.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod chats;
pub mod doc_page_mentions;
pub mod doc_pages;
pub mod files;
pub mod memory;
pub mod mentions;
pub mod reactions;
pub mod tasks;
pub mod threads;
