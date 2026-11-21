# Pages on thumbnails

Bring the T8b work of `feat/pages` (rotate, delete and reorder in the core; rotate and delete in the CLI) onto `feat/thumbnails`, keeping the thumbnails' drawing engine in `danonpdf-core` and leaving `danonpdf-render` out.
Branch: `feat/pages-on-thumbnails`, from `origin/feat/thumbnails` (262efa5). PR against `feat/thumbnails`, not merged here. Checks: the Docker commands of `AGENTS.md`.

status: in-progress

## Specs

- S1: "Hay una rama nueva desde `origin/feat/thumbnails` con rotar, borrar y reordenar en el núcleo y rotar y borrar en la CLI, sin `danonpdf-render`."
- S2: "Siguen todas las funciones de `feat/thumbnails`: miniaturas, orden `render` y vista \"Pages\"."
- S3: "Las comprobaciones del `CLAUDE.md` de DanonPDF pasan en local con Docker, también el `.exe` de Windows." The repository has no `CLAUDE.md`; its build and test rules are in `AGENTS.md` (Build and test).
- S4: "Está subida, con una PR abierta contra `feat/thumbnails`, que dice qué se llevó, qué se adaptó y qué se dejó fuera."
- S5: Commits to carry: `ddf3b64 feat(core): rotate, delete and reorder the pages of a PDF` and `64a93c0 feat(cli): add rotate and delete to danonpdf-cli`. Not carried: `5804a01 feat(render): add danonpdf-render…`. "Si algo de esos dos commits depende de `danonpdf-render`, se adapta al dibujo de `danonpdf-core` o se quita, y se dice en la PR."
- S6: `feat/pages` and `feat/pages-core` are not touched. The window part of T8b is out of scope.

## Tasks

| ID | Specs | Route | Status | Evidence |
| --- | --- | --- | --- | --- |
| T1 | S1, S5 | inline | done | `09ce4f0` (cherry-pick of `ddf3b64`) |
| T2 | S1, S5 | inline | done | `1fd88df` (cherry-pick of `64a93c0`) |
| T3 | S2, S3 | inline | done | Docker checks on `1fd88df`, see L6 |
| T4 | S4 | inline | pending | PR |

Review boundary: the branch point, `262efa5` (head of `origin/feat/thumbnails`).

## Log

- L1 (2026-11-21, dispatch from the DanonAA central session): "Llevar a una rama nueva, que sale de `feat/thumbnails`, lo que ya hay de T8b en `feat/pages`: rotar, borrar y reordenar páginas en el núcleo, y rotar y borrar en la CLI." Peru decided on 2026-11-21 that the drawing engine of `feat/thumbnails` stays and `danonpdf-render` stays out.
- L2: Neither commit names `danonpdf-render`: they touch only `danonpdf-core` (arrange, turn, errors, split, test_support) and `danonpdf-cli`. Nothing had to be adapted to the engine or removed.
- L3: T1 conflicts: `AGENTS.md`, `cli.rs` (error-code lists) and `test_support.rs` (new helpers on both sides). Kept both sides; `page_out_of_range` keeps the thumbnails' wording, which names `--page` of `render`.
- L4: T2 conflicts in 8 files, all because `render` and `rotate`/`delete` were added at the same places (command enum, operations, reports, texts, help, docs, tests). Kept both. Adapted to `render` being there: the help and `AGENTS.md` now say `page_out_of_range` also comes from `render`, that `renderer_unavailable` and `render_failed` come from `render` only, that `rotate` and `delete` do not need PDFium, and that every command but `split` writes everything or nothing. Two expectations of the help test changed with that text (`page_out_of_range comes from extract, render, rotate and`; `merge, info, extract, split, rotate and delete do not need it`).
- L5 (2026-11-21): Docker was not reachable as the user (not in the `docker` group). Asked the central session; Peru answered "(a) Sí, con sudo", for the checks of `AGENTS.md` only.
- L6 (2026-11-21): Checks with `sudo docker compose` on `1fd88df`: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` all green (cli unit 81, cli integration 79, core 203 with 2 ignored as on `feat/thumbnails`, the three PDFium loading programs 1 each, desktop 200); `bash scripts/build-windows.sh` built `dist/danonpdf.exe`, `dist/danonpdf-cli.exe`, `dist/pdfium.dll` and `dist/licenses/pdfium/`. No test was removed, skipped or weakened; the two help-test strings of L4 follow the help text.
