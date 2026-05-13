# Todo items
---

## 1. PDF Viewer
[x] Remember last viewed pdf and page, (save the information in the sqlite db). Since, the application is used from portable device, the path to the files will keep on changing, so we will have to use some kind of relative path or just save the file name and page number. Last opened file might be pdf or markdown or images, which needs to be considered.

## 2. Overall application
[x] Vertical Split pane view feature needs to be added, Justification: User wants to view pdf file in one pane and write notes in markdown editor in another pane. Lightweight library must be used with minimal features.

## 3. UI/UX improvements
[ ] The user experience right now is not that great, its confusing sometimes, the UI needs to be analysed and a plan needs to be created for improvements.
- Progress: Added `docs/UI_UX_PLAN.md` with a phased roadmap and success metrics.

## 4. Documentation
[x] A feature list and user guide needs to be created.
- Completed: Added `docs/FEATURES.md` and `docs/USER_GUIDE.md`.

## 5. Ready the application for version 1.0.0
[ ] Check for memory leaks and other performance issues.
- Progress: Added lightweight readiness script `npm run perf:check` (`scripts/perf-check.mjs`) to run build and tests consistently.
