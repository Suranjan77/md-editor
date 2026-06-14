GUI automation setup and COSMIC sandbox constraints:
`docs/V3_GUI_TESTING.md`. Verify tools with
`python3 scripts/v3-gui-probe.py check` from repository root.

Run: cd v3 && cargo run -p md3-shell --features pdfium -- <a real vault with a real-world PDF>
 1. Quick-open (ctrl+p) a .md file; type; undo (ctrl+z); redo; save (ctrl+s) — dirty dot clears.
 2. Split (ctrl+\), open a PDF in the right pane. Both panes render.
 3. ctrl+z in the PDF pane opens zoom input (NOT editor undo); ctrl+z in the md pane undoes (Bug A check).
 4. PDF: mouse wheel scrolls; pgup/pgdn; ctrl+g jumps; zoom 150% re-renders crisply (no blur > a beat).
 5. Select text on ONE line mid-page: blue tint appears exactly under the cursor (the recurring bug).
 6. Select across three lines: three per-line tints.
 7. Hover text: I-beam. Hover a highlight: pointer.
 8. ctrl+h: highlight appears (yellow), persists after closing+reopening the tab.
 9. ctrl+n on a picked highlight: note saves; delete removes it.
10. ctrl+f: type a word visible on a later page; enter jumps there and tints it; ctrl+h highlights it.
11. ctrl+t: outline listed; enter jumps; status pill shows "· § <section>".
12. alt+left returns; alt+right re-jumps.
13. Quit and relaunch: layout, focus, PDF page and zoom restored ("resumed at p. N").
14. Status line never sticks on "⌘ command" — it settles to the caret/page pill.
 15. internal-links.pdf: right-click the link → popup shows page 2; esc closes; left-click navigates; alt+left returns.
 16. ctrl+shift+t toggles the Study Tracker right panel; log manual session → total hours updates; dashboard weekly chart and curriculums render; config tab edit JSON -> configuration saves.
 17. Overlay lists scroll: ctrl+t on a PDF with a 20+ entry outline — wheel scrolls the list; holding ↓ walks past row 12 and the highlighted row stays in view; enter opens the row that was highlighted. Same for ctrl+p in a vault with 20+ files.
 18. ctrl+f in a PDF: a two-word phrase that wraps across a line break still matches and jumps.
 19. Select PDF text, ctrl+c, paste into another app — the text arrives.
 20. On a picked highlight: palette → "Cycle Highlight Color" changes the tint (4-color cycle); palette → "Open Linked Note for Highlight" creates and opens <stem>-notes.md (second use reopens the same note).
 21. ctrl+shift+b in a note that other notes [[link]] to: referrers listed, enter opens one; a note with no referrers shows "no backlinks".
 22. Edit a PDF's bytes outside the app, then palette → "Orphaned Annotations Report": the old document is listed with its annotation count; esc closes.
 23. Fresh vault: file tree starts open; empty pane shows Open File, Browse Vault, Command Palette, and Keyboard Shortcuts buttons; each button works with the mouse.
 24. ctrl+/: every registered command appears with category and chord; typing filters; enter runs the selected command. Command messages stay on the left while caret/page position stays on the right.
 25. Mouse only: toolbar toggles files/tracker, opens quick-open/search/palette/help, splits focused document, and switches to markdown/PDF-specific controls with tooltips.
 26. Mouse only: File/Edit/View/PDF/Help menus expose registered commands, disable commands for the wrong focused surface, close on outside click or esc, and PDF +/- buttons rerender at the shown zoom.
 27. Open a real note: headings are visibly larger, prose is proportional, no gaps where ** or # hide.
 28. Click into a bold word: markers appear in place; the line reflows only itself; click away: they vanish.
 29. Caret-walk an entire document end to end: no overlap, no jumping, no stale lines.
 30. Table renders as a grid; click inside: source appears; edit a cell; click away: grid re-renders.
 31. Fenced code shows as a mono block; display math renders; click into math: TeX source appears in place.
 32. Reopen a document with images: layout is identical immediately; no pop when assets load.
 33. Click a checkbox: it toggles and undo restores it. Ctrl+click a wikilink opens the note; ctrl+click a URI opens the browser.
 34. Type fast in a 5k-line document: zero perceptible lag; undo always undoes.
 35. Drag-select forward and backward across Markdown lines; selection tint tracks the pointer. ctrl+c copies; ctrl+x cuts; ctrl+z restores the cut.
 36. Scroll a long Markdown note: wheel/page motion eases briefly. Move into markup: caret and revealed markers fade in. Enable Settings → Reduced motion: both become immediate and remain disabled after restart.
 37. Widen a Markdown pane past 1200 px: prose stays centered in a readable column; 17 px body text, heading hierarchy, and blockquote bar remain clear.
