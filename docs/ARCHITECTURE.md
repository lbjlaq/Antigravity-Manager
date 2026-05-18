# Arquitectura del fork `antigravity-manager-src`

## Capas principales

- Frontend de escritorio: React + Vite
- Backend de aplicacion: Tauri
- Logica de proxy y modulos internos: Rust
- Documentacion tecnica especializada: `docs/`

## Regla de trabajo

Cuando una tarea afecte una integracion concreta, usa el documento tecnico mas
cercano dentro de `docs/` antes de redisenar el flujo.
