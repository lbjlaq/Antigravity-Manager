# Antigravity Manager - AI Instructions

Este es el núcleo de gestión del ecosistema Antigravity. Siga estas reglas estrictamente.

## Comandos de Ejecución
- **Build (UI):** `npm run build`
- **Build (Tauri):** `npm run tauri build`
- **Dev:** `npm run dev` / `npm run tauri dev`

## Reglas de Desarrollo
- **Tauri/React:** La aplicación usa Tauri para el backend (Rust) y React/Vite para el frontend (TS).
- **Consistencia:** Mantenga las interfaces limpias y el estado sincronizado con el backend de Rust.
- **Documentación Local:** Antes de modificar el core de Tauri, lea `docs/` para entender la comunicación via comandos.

## Estado del Proyecto
- **Versión Actual:** Revisa `package.json`.
- **Progreso:** Revisa `docs/PROGRESS.md`.
