use super::*;

impl Shell {
    pub(super) fn ensure_session_store(&mut self) {
        if self.session.is_some() {
            return;
        }
        let opened = self
            .open_sidecar_dir()
            .and_then(|_| SessionStore::open(&self.sidecar_path()));
        match opened {
            Ok(store) => self.session = Some(store),
            Err(e) => self.status = format!("session persistence unavailable: {e}"),
        }
    }

    fn capture_session(&self) -> SessionSnapshot {
        let layout = self.snapshot_node(&self.ws.panes.layout());
        let mut views = std::collections::BTreeMap::new();
        for s in self.sessions.md.values() {
            let head = s.doc.buffer().primary().head;
            let caret = s.doc.buffer().offset_to_line_col(head);
            views.insert(
                s.rel_path.clone(),
                ViewSnapshot {
                    scroll: s.scroll,
                    zoom: None,
                    caret: Some(caret),
                    toc_open: false,
                    toc_width: None,
                    annotations_open: false,
                    annotations_width: None,
                    outline_open: s.outline_open,
                    outline_width: Some(s.outline_width),
                    find_open: s.find_open,
                    find_query: Some(s.find_query.clone()),
                    replace_text: Some(s.replace_text.clone()),
                },
            );
        }
        for s in self.sessions.pdf.values() {
            views.insert(
                s.rel_path.clone(),
                ViewSnapshot {
                    scroll: s.scroll,
                    zoom: Some(s.zoom),
                    caret: None,
                    toc_open: s.toc_open,
                    toc_width: Some(s.toc_width),
                    annotations_open: s.annotations_open,
                    annotations_width: Some(s.annotations_width),
                    outline_open: false,
                    outline_width: None,
                    find_open: false,
                    find_query: None,
                    replace_text: None,
                },
            );
        }
        SessionSnapshot {
            layout,
            views,
            tree_open: self.tree_open,
            tree_width: self.tree_width,
            tree_expanded: self.tree_expanded.iter().cloned().collect(),
            tracker_open: self.tracker_open,
            tracker_active_tab: Some(match self.tracker_active_tab {
                tracker_view::TrackerTab::Dashboard => "Dashboard".to_string(),
                tracker_view::TrackerTab::Log => "Log".to_string(),
                tracker_view::TrackerTab::Projects => "Projects".to_string(),
                tracker_view::TrackerTab::Gates => "Gates".to_string(),
                tracker_view::TrackerTab::Reading => "Reading".to_string(),
                tracker_view::TrackerTab::Config => "Config".to_string(),
            }),
            theme: self.theme_name.clone(),
            reduce_motion: self.reduce_motion,
        }
    }

    fn snapshot_node(&self, node: &Layout<'_>) -> NodeSnapshot {
        match node {
            Layout::Pane(pane) => {
                let active = pane
                    .active_tab()
                    .and_then(|a| pane.tabs().iter().position(|t| t.id == a.id))
                    .unwrap_or(0);
                let tabs = pane
                    .tabs()
                    .iter()
                    .filter_map(|t| {
                        let doc = self.ws.docs.get(t.document)?;
                        Some(TabSnapshot {
                            path: doc.path.clone(),
                            focused: self.ws.focused_tab() == Some(t.id),
                        })
                    })
                    .collect();
                NodeSnapshot::Pane { tabs, active }
            }
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => NodeSnapshot::Split {
                vertical: matches!(axis, SplitAxis::Vertical),
                ratio: *ratio,
                first: Box::new(self.snapshot_node(first)),
                second: Box::new(self.snapshot_node(second)),
            },
        }
    }

    pub(super) fn save_session(&mut self) {
        let snapshot = self.capture_session();
        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(e) => {
                self.status = format!("session save failed: {e}");
                return;
            }
        };
        self.ensure_session_store();
        if let Some(store) = self.session.as_mut()
            && let Err(e) = store.save(&json)
        {
            self.status = format!("session save failed: {e}");
        }
    }

    pub(super) fn restore_session(&mut self) -> Task<Message> {
        self.ensure_session_store();
        let Some(json) = self.session.as_ref().and_then(|s| s.load().ok().flatten()) else {
            return Task::none();
        };
        let Ok(snap) = serde_json::from_str::<SessionSnapshot>(&json) else {
            self.status = "saved session was unreadable — starting fresh".to_string();
            return Task::none();
        };

        let root_pane = self.ws.panes.first_pane();
        let mut focus = None;
        let task = self.restore_node(&snap.layout, root_pane, &mut focus);
        self.ws.panes.collapse_empty_panes();

        let root = self.vault_root.clone();
        let worker = self.pdf_worker.clone();
        for (path, view) in &snap.views {
            if let Some(s) = self.sessions.md.values_mut().find(|s| &s.rel_path == path) {
                if let Some((line, col)) = view.caret {
                    s.apply(Command::SetCursor { line, col });
                }
                s.scroll = view.scroll;
                s.find_open = view.find_open;
                if let Some(ref q) = view.find_query {
                    s.find_query = q.clone();
                }
                if let Some(ref r) = view.replace_text {
                    s.replace_text = r.clone();
                }
                s.scroll_by(0.0);
            }
            if let Some(s) = self.sessions.pdf.values_mut().find(|s| &s.rel_path == path) {
                if let Some(zoom) = view.zoom {
                    s.set_zoom(zoom);
                }
                s.scroll = view.scroll;
                s.toc_open = view.toc_open;
                if let Some(w) = view.toc_width {
                    s.toc_width = w;
                }
                s.annotations_open = view.annotations_open;
                if let Some(w) = view.annotations_width {
                    s.annotations_width = w;
                }
                if s.layout.is_some() {
                    s.scroll_by(0.0);
                }
                let abs = root.join(&s.rel_path);
                pdf_view::ensure_tiles(s, &abs, worker.as_ref());
            }
        }

        if let Some(tab) = focus {
            let _ = self.ws.focus_tab(tab);
        }
        self.tree_open = snap.tree_open;
        self.tree_width = snap.tree_width.clamp(160.0, 480.0);
        self.tree_expanded = snap.tree_expanded.into_iter().collect();
        self.tracker_open = snap.tracker_open;
        self.theme_name = snap.theme;
        self.reduce_motion = snap.reduce_motion;
        if let Some(tab_str) = snap.tracker_active_tab {
            self.tracker_active_tab = match tab_str.as_str() {
                "Dashboard" => tracker_view::TrackerTab::Dashboard,
                "Log" => tracker_view::TrackerTab::Log,
                "Projects" => tracker_view::TrackerTab::Projects,
                "Gates" => tracker_view::TrackerTab::Gates,
                "Reading" => tracker_view::TrackerTab::Reading,
                "Config" => tracker_view::TrackerTab::Config,
                _ => tracker_view::TrackerTab::Dashboard,
            };
        }
        self.sync_status();
        if let Some(s) = self.focused_pdf()
            && s.layout.is_some()
        {
            self.status = format!("resumed at p. {}/{}", s.current_page() + 1, s.page_count());
        }
        task
    }

    fn restore_node(&mut self, node: &NodeSnapshot, pane: PaneId, focus: &mut Option<TabId>) -> Task<Message> {
        let mut tasks = Vec::new();
        match node {
            NodeSnapshot::Pane { tabs, active } => {
                let mut opened = Vec::new();
                for t in tabs {
                    if !self.vault_root.join(&t.path).exists() {
                        continue;
                    }
                    let (id_opt, task) = self.open_document_in(pane, &t.path);
                    if let Some(id) = id_opt {
                        opened.push((id, t.focused));
                    }
                    tasks.push(task);
                }
                if let Some(&(id, _)) = opened.get((*active).min(opened.len().saturating_sub(1))) {
                    let _ = self.ws.panes.set_active_tab(pane, id);
                }
                if let Some(&(id, _)) = opened.iter().find(|(_, focused)| *focused) {
                    *focus = Some(id);
                }
            }
            NodeSnapshot::Split {
                vertical,
                ratio,
                first,
                second,
            } => {
                let axis = if *vertical {
                    SplitAxis::Vertical
                } else {
                    SplitAxis::Horizontal
                };
                match self.ws.panes.split_with_ratio(pane, axis, *ratio) {
                    Ok(sibling) => {
                        tasks.push(self.restore_node(first, pane, focus));
                        tasks.push(self.restore_node(second, sibling, focus));
                    }
                    Err(_) => {
                        tasks.push(self.restore_node(first, pane, focus));
                        tasks.push(self.restore_node(second, pane, focus));
                    }
                }
            }
        }
        Task::batch(tasks)
    }
}
