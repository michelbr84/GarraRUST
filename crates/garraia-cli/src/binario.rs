//! O nome pelo qual o usuario chamou este programa (#1329).
//!
//! A implementacao vive em [`garraia_common::executavel`], compartilhada com o
//! gateway e com o `next_step` de `garraia-channels` — todos rodam no mesmo
//! executavel e tem de nomear o mesmo binario. Os testes estao la.

pub use garraia_common::executavel::nome;
