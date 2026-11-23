# Page override — Study tracker

## Placement

The tracker becomes a **view** in the main area (like the graph), reached from the toolbar
or palette, not a full-screen modal overlay (skill: `modal-vs-navigation`). The sidebar
stays visible; Escape returns to the previous document.

## Layout

- Header: view title (`title`), tabs (`Tabs`: Dashboard · Log · Projects · Gates · Reading ·
  Settings), and the single primary action **Start / Stop timer** with elapsed time in
  JetBrains Mono tabular figures.
- Dashboard: 4 stat `Card`s in a row (value `heading`, label `caption`), weekly activity
  chart, recent sessions list, curriculum roadmap.

## Data display

- Durations formatted `1h 05m`, never negative zero (today shows `-0.0h`).
- Weekly chart: bar chart (skill: comparison → bar); bars `accent`, gridlines `border.subtle`,
  each bar labelled on hover/focus with the exact value; day labels `caption`.
  Empty state instead of a flat axis: "No sessions this week — start the timer to log one."
- Progress uses `ProgressBar` with a text percentage beside it (not colour alone).
- Config tab: the JSON editor shows parse errors inline under the field with line/column,
  and Save stays enabled (skill: `submit-not-pre-disabled`).
