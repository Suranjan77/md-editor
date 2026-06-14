use super::*;

impl Shell {
    pub(super) fn sync_status(&mut self) {
        let Some(tab) = self.ws.focused_tab() else {
            self.position_status.clear();
            return;
        };
        let Some(doc) = self.tab_document(tab) else {
            self.position_status.clear();
            return;
        };
        if let Some(session) = self.sessions.md.get(&doc) {
            let head = session.doc.buffer().primary().head;
            let (line, col) = session.doc.buffer().offset_to_line_col(head);
            let dirty = if session.doc.buffer().is_dirty() {
                " ●"
            } else {
                ""
            };
            self.position_status =
                format!("{}{dirty} — Ln {}, Col {}", session.rel_path, line + 1, col);
        } else if let Some(session) = self.sessions.pdf.get(&doc)
            && session.layout.is_some()
        {
            let section = session
                .current_section()
                .map(|t| format!(" · § {t}"))
                .unwrap_or_default();
            self.position_status = format!(
                "{} — p. {}/{} · {:.0}%{section}",
                session.rel_path,
                session.current_page() + 1,
                session.page_count(),
                session.zoom * 100.0
            );
        }
    }
}
