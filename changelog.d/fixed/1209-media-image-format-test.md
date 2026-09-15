- **Deteccao de formato de imagem por magic bytes volta a ser testada (#1209).**
  `test_detect_format_from_bytes` estava `#[ignore]`d desde 2026-04-15,
  atribuido a drift do crate `image`. A funcao nem usa esse crate: e comparacao
  de magic bytes pura, com um piso de 12 bytes que existe porque o ramo do WEBP
  le `data[8..12]`. As fixtures do teste tinham 8 e 4 bytes, caiam no piso e
  voltavam "unknown". Com cabecalhos reais de 12 bytes o teste volta a rodar, e
  os ramos GIF, WEBP e BMP — que nunca tiveram cobertura — passam a ser
  exercitados, junto com o caso RIFF/WAVE, que e a unica condicao composta da
  funcao. Um teste novo fixa o piso de 12 bytes como comportamento deliberado.
  O codigo de producao nao muda.
