# Registro de decisiones tecnicas

Este archivo se usa para registrar decisiones nuevas del fork local de
`Antigravity-Manager`.

## Decision 1 - Mantener docs especializados y una capa minima de estado

- Contexto: el repo ya trae muchos documentos tecnicos por tema.
- Decision: conservar esos documentos y sumar esta capa corta de estado:
  `ARCHITECTURE.md`, `DECISIONS.md`, `PROGRESS.md`, `HANDOVER.md`.

## Decision 2 - Separar remoto de push y remoto base

- Contexto: este repo trabaja como fork local de `Antigravity-Manager`, con
  `origin` apuntando al upstream y `fork` apuntando al repo del usuario.
- Decision: la automatizacion usa `git.push_remote = fork` y
  `git.base_remote = origin` para evitar pushes al remoto equivocado y para
  preparar PRs contra el upstream correcto.
