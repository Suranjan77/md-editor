//! Slide table and text-body builders for PowerPoint slides.

use super::*;

impl<'a, 'i> SlideBuilder<'a, 'i, '_> {
    pub(super) fn table(&mut self, table: Node<'a, 'i>, rels: &Relationships) -> Table {
        let properties = child(table, "tblPr");
        let flag = |name| properties.and_then(|p| attr_bool(p, name)).unwrap_or(false);
        let (first_row, last_row, first_col, last_col, band_row) = (
            flag("firstRow"),
            flag("lastRow"),
            flag("firstCol"),
            flag("lastCol"),
            flag("bandRow"),
        );
        let look = table_look(
            properties
                .and_then(|p| child(p, "tableStyleId"))
                .and_then(|id| id.text())
                .unwrap_or(self.default_table_style)
                .trim(),
        );

        let columns: Vec<f32> = child(table, "tblGrid")
            .into_iter()
            .flat_map(|grid| children_named(grid, "gridCol"))
            .map(|col| emu_attr(col, "w").unwrap_or(0.0))
            .collect();
        let row_nodes: Vec<Node> = children_named(table, "tr").collect();
        let row_count = row_nodes.len();

        let mut rows = Vec::with_capacity(row_count);
        for (row_index, row) in row_nodes.into_iter().enumerate() {
            let mut cells = Vec::new();
            for (col_index, cell) in children_named(row, "tc").enumerate() {
                let emphasized = (first_row && row_index == 0)
                    || (last_row && row_index + 1 == row_count)
                    || (first_col && col_index == 0)
                    || (last_col && col_index + 1 == columns.len());
                let data_row = row_index.saturating_sub(usize::from(first_row));
                let banded = band_row && !emphasized && data_row % 2 == 0;
                cells.push(self.table_cell(cell, rels, look, emphasized, banded));
            }
            rows.push(TableRow {
                height: emu_attr(row, "h").unwrap_or(0.0),
                cells,
            });
        }
        Table { columns, rows }
    }

    pub(super) fn table_cell(
        &mut self,
        cell: Node<'a, 'i>,
        rels: &Relationships,
        look: TableLook,
        emphasized: bool,
        banded: bool,
    ) -> TableCell {
        let properties = child(cell, "tcPr");
        let (style_fill, style_border, style_text) =
            self.table_cell_look_defaults(look, emphasized, banded);

        let fill = match properties.and_then(find_fill) {
            Some(spec) => self.fill(&spec, rels, None),
            None => style_fill,
        };
        let borders = self.table_cell_borders(properties, rels, style_border.as_ref());

        let mut levels = self.default_style.clone();
        if let Some(text_props) = &style_text {
            for level in &mut levels.levels {
                level.run.overlay(text_props);
            }
        }
        let inset = |name, default: f32| {
            properties
                .and_then(|p| emu_attr(p, name))
                .unwrap_or(default)
        };
        let body_props = BodyProps {
            insets: [
                Some(inset("marL", 7.2)),
                Some(inset("marT", 3.6)),
                Some(inset("marR", 7.2)),
                Some(inset("marB", 3.6)),
            ],
            anchor: properties
                .and_then(|p| p.attribute("anchor"))
                .map(parse_anchor),
            wrap: Some(true),
            autofit: None,
        };
        let text = child(cell, "txBody").and_then(|body| {
            if let Some(list) = child(body, "lstStyle") {
                levels.overlay(&parse_list_style(list));
            }
            self.text_body(body, &levels, &body_props)
        });

        TableCell {
            text,
            fill,
            borders,
            column_span: attr_u32(cell, "gridSpan").unwrap_or(1).max(1) as usize,
            row_span: attr_u32(cell, "rowSpan").unwrap_or(1).max(1) as usize,
            merged: attr_bool(cell, "hMerge").unwrap_or(false)
                || attr_bool(cell, "vMerge").unwrap_or(false),
        }
    }

    fn table_cell_look_defaults(
        &self,
        look: TableLook,
        emphasized: bool,
        banded: bool,
    ) -> (Fill, Option<Line>, Option<RunProps>) {
        match look {
            TableLook::Plain => (Fill::None, None, None),
            TableLook::Grid => (
                Fill::None,
                Some(Line {
                    color: self.scheme_color("tx1"),
                    width: 1.0,
                    dash: Dash::Solid,
                    head_arrow: false,
                    tail_arrow: false,
                }),
                None,
            ),
            TableLook::Medium2 { accent } => {
                let accent_color = self.scheme_color(accent);
                let tint = |amount| apply_mods(accent_color, &[ColorMod::Tint(amount)]);
                let fill = if emphasized {
                    accent_color
                } else if banded {
                    tint(0.4)
                } else {
                    tint(0.2)
                };
                let text = emphasized.then(|| RunProps {
                    bold: Some(true),
                    fill: Some(FillSpec::Solid(ColorSpec {
                        base: ColorBase::Scheme("lt1".to_string()),
                        mods: Vec::new(),
                    })),
                    ..RunProps::default()
                });
                (
                    Fill::Solid(fill),
                    Some(Line {
                        color: self.scheme_color("lt1"),
                        width: 1.0,
                        dash: Dash::Solid,
                        head_arrow: false,
                        tail_arrow: false,
                    }),
                    text,
                )
            }
        }
    }

    fn table_cell_borders(
        &mut self,
        properties: Option<Node<'a, 'i>>,
        rels: &Relationships,
        style_border: Option<&Line>,
    ) -> Borders {
        let mut border = |name| match properties.and_then(|p| child(p, name)) {
            Some(ln) => {
                let spec = parse_line(ln);
                match spec.fill {
                    Some(fill_spec) => match self.fill(&fill_spec, rels, None) {
                        Fill::Solid(color) => Some(Line {
                            color,
                            width: spec.width.unwrap_or(DEFAULT_LINE_WIDTH),
                            dash: spec.dash.unwrap_or_default(),
                            head_arrow: false,
                            tail_arrow: false,
                        }),
                        _ => None,
                    },
                    None => style_border.cloned(),
                }
            }
            None => style_border.cloned(),
        };
        Borders {
            left: border("lnL"),
            top: border("lnT"),
            right: border("lnR"),
            bottom: border("lnB"),
        }
    }

    /// Resolve a text body's paragraphs over the list style they inherit.
    pub(super) fn text_body(
        &mut self,
        body: Node,
        levels: &ListStyle,
        props: &BodyProps,
    ) -> Option<TextBody> {
        let (font_scale, spacing_reduction) = props.autofit.unwrap_or((1.0, 0.0));
        let mut counters: [Option<u32>; 9] = [None; 9];
        let mut paragraphs = Vec::new();
        let mut has_text = false;

        for paragraph in children_named(body, "p") {
            let own = child(paragraph, "pPr");
            let level = own.and_then(|p| attr_u32(p, "lvl")).unwrap_or(0).min(8) as usize;
            let mut para = levels.levels[level].clone();
            if let Some(own) = own {
                para.overlay(&parse_para_props(own));
            }

            let runs = self.paragraph_runs(paragraph, &para, font_scale);

            let mut end_props = para.run.clone();
            if let Some(end) = child(paragraph, "endParaRPr") {
                end_props.overlay(&parse_run_props(end));
            }
            let visible = runs.iter().any(|run| !run.text.trim().is_empty());
            has_text |= visible;

            let bullet = resolve_paragraph_bullet(&para, level, visible, &mut counters)
                .map(|text| self.bullet(text, &para, runs.first()));

            let line_spacing = match para.line_spacing.unwrap_or(Spacing::Lines(1.0)) {
                Spacing::Lines(lines) => Spacing::Lines((lines - spacing_reduction).max(0.1)),
                points => points,
            };
            paragraphs.push(Paragraph {
                align: para.align.unwrap_or_default(),
                margin_left: para.margin_left.unwrap_or(0.0),
                indent: para.indent.unwrap_or(0.0),
                space_before: para.space_before.unwrap_or(Spacing::Points(0.0)),
                space_after: para.space_after.unwrap_or(Spacing::Points(0.0)),
                line_spacing,
                bullet,
                runs,
                end_size: end_props.size.unwrap_or(DEFAULT_FONT_SIZE) * font_scale,
            });
        }

        if !has_text {
            return None;
        }
        let [left, top, right, bottom] = props.insets;
        Some(TextBody {
            insets: Insets {
                left: left.unwrap_or(7.2),
                top: top.unwrap_or(3.6),
                right: right.unwrap_or(7.2),
                bottom: bottom.unwrap_or(3.6),
            },
            anchor: props.anchor.unwrap_or_default(),
            wrap: props.wrap.unwrap_or(true),
            paragraphs,
        })
    }

    fn paragraph_runs(&mut self, paragraph: Node, para: &ParaProps, font_scale: f32) -> Vec<Run> {
        let mut runs = Vec::new();
        for node in paragraph.children().filter(Node::is_element) {
            let name = node.tag_name().name();
            if !matches!(name, "r" | "fld" | "br") {
                continue;
            }
            let mut run_props = para.run.clone();
            if let Some(rpr) = child(node, "rPr") {
                run_props.overlay(&parse_run_props(rpr));
            }
            let text = match name {
                "br" => "\n".to_string(),
                "fld" if node.attribute("type") == Some("slidenum") => self.number.to_string(),
                _ => child(node, "t")
                    .and_then(|t| t.text())
                    .unwrap_or("")
                    .replace(['\t', '\u{b}'], "    "),
            };
            if text.is_empty() {
                continue;
            }
            runs.push(self.run(text, &run_props, font_scale));
        }
        runs
    }

    pub(super) fn font_name(&self, typeface: Option<&str>) -> String {
        match typeface {
            Some(face) if face.starts_with("+mj") => self.theme.major_font.clone(),
            Some(face) if face.starts_with("+mn") || face.is_empty() => {
                self.theme.minor_font.clone()
            }
            Some(face) => face.to_string(),
            None => self.theme.minor_font.clone(),
        }
    }

    pub(super) fn run(&mut self, text: String, props: &RunProps, font_scale: f32) -> Run {
        let color = match &props.fill {
            Some(FillSpec::Solid(color)) => self.color(color, None),
            Some(FillSpec::Gradient { stops, .. }) => stops
                .first()
                .map_or(Rgba::BLACK, |(_, color)| self.color(color, None)),
            Some(FillSpec::None) => Rgba::TRANSPARENT,
            _ => self.scheme_color("tx1"),
        };
        let text = if props.all_caps == Some(true) {
            text.to_uppercase()
        } else {
            text
        };
        Run {
            text,
            style: RunStyle {
                size: props.size.unwrap_or(DEFAULT_FONT_SIZE) * font_scale,
                bold: props.bold.unwrap_or(false),
                italic: props.italic.unwrap_or(false),
                underline: props.underline.unwrap_or(false),
                strike: props.strike.unwrap_or(false),
                color,
                font: self.font_name(props.font.as_deref()),
                baseline: props.baseline.unwrap_or(0.0),
                highlight: props.highlight.as_ref().map(|c| self.color(c, None)),
                spacing: props.spacing.unwrap_or(0.0) * font_scale,
            },
        }
    }

    pub(super) fn bullet(&self, text: String, para: &ParaProps, first_run: Option<&Run>) -> Bullet {
        let text_size = first_run.map_or(DEFAULT_FONT_SIZE, |run| run.style.size);
        Bullet {
            text,
            color: match &para.bullet_color {
                Some(Some(color)) => self.color(color, None),
                _ => first_run.map_or(Rgba::BLACK, |run| run.style.color),
            },
            size: match para.bullet_size {
                Some(BulletSize::Points(points)) => points,
                Some(BulletSize::Relative(ratio)) => text_size * ratio,
                None => text_size,
            },
            font: match &para.bullet_font {
                Some(Some(face)) => self.font_name(Some(face)),
                _ => first_run.map_or_else(
                    || self.theme.minor_font.clone(),
                    |run| run.style.font.clone(),
                ),
            },
        }
    }
}

fn resolve_paragraph_bullet(
    para: &ParaProps,
    level: usize,
    visible: bool,
    counters: &mut [Option<u32>; 9],
) -> Option<String> {
    match &para.bullet {
        Some(BulletKind::AutoNumber { scheme, start }) if visible => {
            let n = counters[level].map_or(*start, |n| n + 1);
            counters[level] = Some(n);
            counters[level + 1..].fill(None);
            Some(autonumber_label(scheme, n))
        }
        Some(BulletKind::Char(text)) if visible => {
            counters[level..].fill(None);
            Some(text.clone())
        }
        _ => {
            if visible {
                counters[level..].fill(None);
            }
            None
        }
    }
}
