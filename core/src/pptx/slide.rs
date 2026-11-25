//! Reading a slide: the shape tree, fills, lines, placeholders, shapes, pictures, and graphic frames.

use super::*;

pub(super) struct SlideBuilder<'a, 'i, 'm> {
    pub(super) theme: &'a Theme,
    pub(super) default_style: &'a ListStyle,
    pub(super) text_styles: Option<&'a TextStyles>,
    pub(super) master: Option<TemplatePart<'a, 'i>>,
    pub(super) layout: Option<TemplatePart<'a, 'i>>,
    pub(super) default_table_style: &'a str,
    pub(super) color_map: HashMap<String, String>,
    pub(super) number: usize,
    pub(super) title: Option<String>,
    pub(super) media: &'m mut HashSet<String>,
}

impl<'a, 'i> SlideBuilder<'a, 'i, '_> {
    pub(super) fn build(mut self, slide: Node<'a, 'i>, rels: &Relationships) -> Slide {
        self.color_map = self.resolve_color_map(slide);
        let background = self.background(slide, rels);
        let mut elements = Vec::new();

        let slide_shows_templates = attr_bool(slide, "showMasterSp") != Some(false);
        if slide_shows_templates {
            let layout_shows_master = self
                .layout
                .as_ref()
                .is_none_or(|layout| attr_bool(layout.root, "showMasterSp") != Some(false));
            if layout_shows_master
                && let Some((root, template_rels)) = self
                    .master
                    .as_ref()
                    .map(|master| (master.root, master.rels))
                && let Some(tree) = descend(root, &["cSld", "spTree"])
            {
                self.walk(
                    tree,
                    template_rels,
                    &Transform::SLIDE,
                    0,
                    true,
                    &mut elements,
                );
            }
            if let Some((root, template_rels)) = self
                .layout
                .as_ref()
                .map(|layout| (layout.root, layout.rels))
                && let Some(tree) = descend(root, &["cSld", "spTree"])
            {
                self.walk(
                    tree,
                    template_rels,
                    &Transform::SLIDE,
                    0,
                    true,
                    &mut elements,
                );
            }
        }
        if let Some(tree) = descend(slide, &["cSld", "spTree"]) {
            self.walk(tree, rels, &Transform::SLIDE, 0, false, &mut elements);
        }

        Slide {
            number: self.number,
            hidden: attr_bool(slide, "show") == Some(false),
            title: self.title,
            background,
            elements,
        }
    }

    pub(super) fn resolve_color_map(&self, slide: Node) -> HashMap<String, String> {
        let mut map = HashMap::new();
        let mut apply = |node: Option<Node>| {
            if let Some(node) = node {
                for attr in node.attributes() {
                    map.insert(attr.name().to_string(), attr.value().to_string());
                }
            }
        };
        apply(self.master.as_ref().and_then(|m| child(m.root, "clrMap")));
        apply(
            self.layout
                .as_ref()
                .and_then(|l| descend(l.root, &["clrMapOvr", "overrideClrMapping"])),
        );
        apply(descend(slide, &["clrMapOvr", "overrideClrMapping"]));
        map
    }

    pub(super) fn scheme_color(&self, name: &str) -> Rgba {
        let mapped = self
            .color_map
            .get(name)
            .map(String::as_str)
            .unwrap_or(match name {
                "bg1" => "lt1",
                "tx1" => "dk1",
                "bg2" => "lt2",
                "tx2" => "dk2",
                other => other,
            });
        self.theme
            .colors
            .get(mapped)
            .copied()
            .unwrap_or(Rgba::BLACK)
    }

    pub(super) fn color(&self, spec: &ColorSpec, placeholder: Option<Rgba>) -> Rgba {
        let base = match &spec.base {
            ColorBase::Rgb(color) => *color,
            ColorBase::Scheme(name) => self.scheme_color(name),
            ColorBase::Placeholder => placeholder.unwrap_or(Rgba::BLACK),
        };
        apply_mods(base, &spec.mods)
    }

    pub(super) fn fill(
        &mut self,
        spec: &FillSpec,
        rels: &Relationships,
        placeholder: Option<Rgba>,
    ) -> Fill {
        match spec {
            FillSpec::None | FillSpec::Group => Fill::None,
            FillSpec::Solid(color) => Fill::Solid(self.color(color, placeholder)),
            FillSpec::Gradient {
                stops,
                angle,
                radial,
            } => Fill::Gradient(Gradient {
                stops: stops
                    .iter()
                    .map(|(offset, color)| GradientStop {
                        offset: offset.clamp(0.0, 1.0),
                        color: self.color(color, placeholder),
                    })
                    .collect(),
                angle: *angle,
                radial: *radial,
            }),
            FillSpec::Picture(id) => match rels.get(id) {
                Some(rel) if !rel.external => {
                    self.media.insert(rel.target.clone());
                    Fill::Picture {
                        media: rel.target.clone(),
                    }
                }
                _ => Fill::None,
            },
        }
    }

    pub(super) fn style_fill(&mut self, reference: Node, rels: &Relationships) -> Option<Fill> {
        let index = attr_u32(reference, "idx")? as usize;
        let spec = match index {
            0 => return Some(Fill::None),
            1..=999 => self.theme.fill_styles.get(index - 1),
            _ => self.theme.background_styles.get(index - 1001),
        }?
        .clone();
        let placeholder = first_color(reference).map(|color| self.color(&color, None));
        Some(self.fill(&spec, rels, placeholder))
    }

    pub(super) fn line(
        &mut self,
        chain: &[Node],
        style: Option<Node>,
        rels: &Relationships,
    ) -> Option<Line> {
        let mut spec = LineSpec::default();
        let mut placeholder = None;
        if let Some(reference) = style.and_then(|s| child(s, "lnRef"))
            && let Some(index) = attr_u32(reference, "idx").filter(|i| *i > 0)
            && let Some(theme_line) = self.theme.line_styles.get(index as usize - 1)
        {
            spec = theme_line.clone();
            placeholder = first_color(reference).map(|color| self.color(&color, None));
        }
        for properties in chain.iter().rev() {
            if let Some(ln) = child(*properties, "ln") {
                spec.overlay(&parse_line(ln));
            }
        }
        let color = match self.fill(spec.fill.as_ref()?, rels, placeholder) {
            Fill::Solid(color) => color,
            Fill::Gradient(gradient) => gradient.stops.first()?.color,
            _ => return None,
        };
        Some(Line {
            color,
            width: spec.width.unwrap_or(DEFAULT_LINE_WIDTH),
            dash: spec.dash.unwrap_or_default(),
            head_arrow: spec.head_arrow.unwrap_or(false),
            tail_arrow: spec.tail_arrow.unwrap_or(false),
        })
    }

    pub(super) fn background(&mut self, slide: Node<'a, 'i>, rels: &Relationships) -> Fill {
        let mut sources = vec![(slide, rels)];
        if let Some(layout) = &self.layout {
            sources.push((layout.root, layout.rels));
        }
        if let Some(master) = &self.master {
            sources.push((master.root, master.rels));
        }
        for (root, source_rels) in sources {
            let Some(bg) = descend(root, &["cSld", "bg"]) else {
                continue;
            };
            if let Some(spec) = child(bg, "bgPr").and_then(find_fill) {
                return self.fill(&spec, source_rels, None);
            }
            if let Some(fill) = child(bg, "bgRef").and_then(|r| self.style_fill(r, source_rels)) {
                return fill;
            }
        }
        Fill::Solid(Rgba::WHITE)
    }

    pub(super) fn walk(
        &mut self,
        tree: Node<'a, 'i>,
        rels: &Relationships,
        transform: &Transform,
        depth: usize,
        template: bool,
        out: &mut Vec<Element>,
    ) {
        if depth > MAX_TREE_DEPTH {
            return;
        }
        for node in tree.children().filter(Node::is_element) {
            let name = node.tag_name().name();
            if template
                && matches!(name, "sp" | "pic" | "graphicFrame")
                && placeholder(node).is_some()
            {
                continue;
            }
            if matches!(name, "sp" | "cxnSp" | "pic" | "graphicFrame" | "grpSp") && is_hidden(node)
            {
                continue;
            }
            let element = match name {
                "sp" | "cxnSp" => self.shape(node, rels, transform, template),
                "pic" => self.picture(node, rels, transform),
                "graphicFrame" => self.graphic_frame(node, rels, transform),
                "grpSp" => {
                    if let Some(xfrm) = descend(node, &["grpSpPr", "xfrm"]).and_then(parse_xfrm) {
                        let inner = group_transform(transform, &xfrm);
                        self.walk(node, rels, &inner, depth + 1, template, out);
                    } else {
                        self.walk(node, rels, transform, depth + 1, template, out);
                    }
                    None
                }
                "AlternateContent" => {
                    if let Some(branch) = child(node, "Fallback").or_else(|| child(node, "Choice"))
                    {
                        self.walk(branch, rels, transform, depth + 1, template, out);
                    }
                    None
                }
                _ => None,
            };
            out.extend(element);
        }
    }

    pub(super) fn inherited(
        &self,
        ph: Option<&Placeholder>,
    ) -> (Option<Node<'a, 'i>>, Option<Node<'a, 'i>>) {
        let Some(ph) = ph else {
            return (None, None);
        };
        let layout = self.layout.as_ref().and_then(|layout| {
            let by_idx = ph.idx.and_then(|idx| {
                layout
                    .placeholders
                    .iter()
                    .find(|(candidate, _)| candidate.idx == Some(idx))
            });
            by_idx
                .or_else(|| {
                    layout.placeholders.iter().find(|(candidate, _)| {
                        master_kind(&candidate.kind) == master_kind(&ph.kind)
                    })
                })
                .map(|(_, node)| *node)
        });
        let kind = layout
            .and_then(placeholder)
            .map(|layout_ph| layout_ph.kind)
            .unwrap_or_else(|| ph.kind.clone());
        let master = self.master.as_ref().and_then(|master| {
            master
                .placeholders
                .iter()
                .find(|(candidate, _)| master_kind(&candidate.kind) == master_kind(&kind))
                .map(|(_, node)| *node)
        });
        (layout, master)
    }

    pub(super) fn shape(
        &mut self,
        node: Node<'a, 'i>,
        rels: &Relationships,
        transform: &Transform,
        template: bool,
    ) -> Option<Element> {
        let ph = placeholder(node);
        let (layout_ph, master_ph) = self.inherited(ph.as_ref());
        let chain: Vec<Node> = [Some(node), layout_ph, master_ph]
            .into_iter()
            .flatten()
            .filter_map(|n| child(n, "spPr"))
            .collect();

        let xfrm = chain
            .iter()
            .find_map(|pr| child(*pr, "xfrm").and_then(parse_xfrm))?;
        let (bounds, rotation) = place(&xfrm, transform);
        let style = child(node, "style");

        let fill = match chain.iter().find_map(|pr| find_fill(*pr)) {
            Some(spec) => self.fill(&spec, rels, None),
            None => style
                .and_then(|s| child(s, "fillRef"))
                .and_then(|reference| self.style_fill(reference, rels))
                .unwrap_or(Fill::None),
        };
        let line = self.line(&chain, style, rels);

        let text = child(node, "txBody").and_then(|body| {
            let (levels, body_props) =
                self.shape_text_style(body, ph.as_ref(), layout_ph, master_ph, style);
            self.text_body(body, &levels, &body_props)
        });
        if !template
            && let (Some(ph), Some(text)) = (&ph, &text)
            && master_kind(&ph.kind) == "title"
            && self.title.is_none()
        {
            let title: String = text
                .paragraphs
                .iter()
                .flat_map(|p| {
                    p.runs
                        .iter()
                        .map(|r| if r.text == "\n" { " " } else { r.text.as_str() })
                })
                .collect();
            let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
            if !title.is_empty() {
                self.title = Some(title);
            }
        }

        if fill == Fill::None && line.is_none() && text.is_none() {
            return None;
        }
        let paths = chain
            .iter()
            .find_map(|pr| {
                if let Some(prst) = child(*pr, "prstGeom") {
                    let name = prst.attribute("prst").unwrap_or("rect");
                    Some(preset_geometry(
                        name,
                        &adjust_values(prst),
                        bounds.width,
                        bounds.height,
                    ))
                } else {
                    child(*pr, "custGeom")
                        .map(|cust| custom_geometry(cust, bounds.width, bounds.height))
                        .filter(|paths| !paths.is_empty())
                }
            })
            .unwrap_or_else(|| {
                preset_geometry("rect", &HashMap::new(), bounds.width, bounds.height)
            });

        Some(Element {
            bounds,
            rotation,
            flip_h: xfrm.flip_h,
            flip_v: xfrm.flip_v,
            kind: ElementKind::Shape(Shape {
                paths,
                fill,
                line,
                text,
            }),
        })
    }

    pub(super) fn shape_text_style(
        &self,
        body: Node,
        ph: Option<&Placeholder>,
        layout_ph: Option<Node>,
        master_ph: Option<Node>,
        style: Option<Node>,
    ) -> (ListStyle, BodyProps) {
        let mut levels = self.default_style.clone();
        if let (Some(ph), Some(styles)) = (ph, self.text_styles) {
            levels.overlay(match master_kind(&ph.kind) {
                "title" => &styles.title,
                "body" => &styles.body,
                _ => &styles.other,
            });
        }
        let mut body_props = BodyProps::default();
        for inherited in [master_ph, layout_ph].into_iter().flatten() {
            if let Some(inherited_body) = child(inherited, "txBody") {
                if let Some(list) = child(inherited_body, "lstStyle") {
                    levels.overlay(&parse_list_style(list));
                }
                if let Some(props) = child(inherited_body, "bodyPr") {
                    body_props.overlay(&parse_body_props(props));
                }
            }
        }
        if let Some(reference) = style.and_then(|s| child(s, "fontRef")) {
            let font_ref = RunProps {
                font: match reference.attribute("idx") {
                    Some("major") => Some("+mj-lt".to_string()),
                    Some("minor") => Some("+mn-lt".to_string()),
                    _ => None,
                },
                fill: first_color(reference).map(FillSpec::Solid),
                ..RunProps::default()
            };
            for level in &mut levels.levels {
                level.run.overlay(&font_ref);
            }
        }
        if let Some(list) = child(body, "lstStyle") {
            levels.overlay(&parse_list_style(list));
        }
        if let Some(props) = child(body, "bodyPr") {
            body_props.overlay(&parse_body_props(props));
        }
        (levels, body_props)
    }

    pub(super) fn picture(
        &mut self,
        node: Node<'a, 'i>,
        rels: &Relationships,
        transform: &Transform,
    ) -> Option<Element> {
        let ph = placeholder(node);
        let (layout_ph, master_ph) = self.inherited(ph.as_ref());
        let chain: Vec<Node> = [Some(node), layout_ph, master_ph]
            .into_iter()
            .flatten()
            .filter_map(|n| child(n, "spPr"))
            .collect();
        let xfrm = chain
            .iter()
            .find_map(|pr| child(*pr, "xfrm").and_then(parse_xfrm))?;
        let (bounds, rotation) = place(&xfrm, transform);
        let blip_fill = child(node, "blipFill")?;
        let target = child(blip_fill, "blip")
            .and_then(|blip| rel_attr(blip, "embed"))
            .and_then(|id| rels.get(id))
            .filter(|rel| !rel.external)?
            .target
            .clone();

        let extension = target.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let kind = if matches!(extension.as_str(), "emf" | "wmf" | "svg" | "pdf") {
            ElementKind::Unsupported {
                label: format!("{} picture", extension.to_uppercase()),
            }
        } else {
            self.media.insert(target.clone());
            let crop = child(blip_fill, "srcRect").map_or(Crop::default(), |src| Crop {
                left: fraction_attr(src, "l").unwrap_or(0.0),
                top: fraction_attr(src, "t").unwrap_or(0.0),
                right: fraction_attr(src, "r").unwrap_or(0.0),
                bottom: fraction_attr(src, "b").unwrap_or(0.0),
            });
            let line = self.line(&chain, child(node, "style"), rels);
            ElementKind::Picture(Picture {
                media: target,
                crop,
                line,
            })
        };
        Some(Element {
            bounds,
            rotation,
            flip_h: xfrm.flip_h,
            flip_v: xfrm.flip_v,
            kind,
        })
    }

    pub(super) fn graphic_frame(
        &mut self,
        node: Node<'a, 'i>,
        rels: &Relationships,
        transform: &Transform,
    ) -> Option<Element> {
        let xfrm = child(node, "xfrm").and_then(parse_xfrm).or_else(|| {
            let (layout_ph, master_ph) = self.inherited(placeholder(node).as_ref());
            [layout_ph, master_ph]
                .into_iter()
                .flatten()
                .find_map(|n| descend(n, &["spPr", "xfrm"]).and_then(parse_xfrm))
        })?;
        let (bounds, rotation) = place(&xfrm, transform);
        let data = descend(node, &["graphic", "graphicData"])?;
        let uri = data.attribute("uri").unwrap_or("");
        let kind = if let Some(table) = child(data, "tbl") {
            ElementKind::Table(self.table(table, rels))
        } else if uri.ends_with("/chart") || uri.contains("chart") {
            ElementKind::Unsupported {
                label: "Chart".to_string(),
            }
        } else if uri.contains("diagram") {
            ElementKind::Unsupported {
                label: "SmartArt graphic".to_string(),
            }
        } else {
            ElementKind::Unsupported {
                label: "Embedded object".to_string(),
            }
        };
        Some(Element {
            bounds,
            rotation,
            flip_h: false,
            flip_v: false,
            kind,
        })
    }
}
