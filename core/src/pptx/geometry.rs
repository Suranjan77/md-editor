//! Preset and custom shape geometry, converted to paths.

use super::*;

// ── Geometry ─────────────────────────────────────────────────────────

/// A 2D affine map from a shape tree's coordinate space (EMUs) to slide
/// points, with the rotation and scale it applies kept alongside so an
/// element's size and angle can be carried through nested groups.
#[derive(Debug, Clone, Copy)]
pub(super) struct Transform {
    pub(super) a: f64,
    pub(super) b: f64,
    pub(super) c: f64,
    pub(super) d: f64,
    pub(super) e: f64,
    pub(super) f: f64,
    pub(super) rotation: f64,
    pub(super) scale_x: f64,
    pub(super) scale_y: f64,
}

impl Transform {
    pub(super) const SLIDE: Transform = Transform {
        a: 1.0 / EMU_PER_POINT,
        b: 0.0,
        c: 0.0,
        d: 1.0 / EMU_PER_POINT,
        e: 0.0,
        f: 0.0,
        rotation: 0.0,
        scale_x: 1.0 / EMU_PER_POINT,
        scale_y: 1.0 / EMU_PER_POINT,
    };

    pub(super) fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// `self` after `local`: maps a point through `local`, then `self`.
    pub(super) fn then(&self, local: &Transform) -> Transform {
        Transform {
            a: self.a * local.a + self.c * local.b,
            b: self.b * local.a + self.d * local.b,
            c: self.a * local.c + self.c * local.d,
            d: self.b * local.c + self.d * local.d,
            e: self.a * local.e + self.c * local.f + self.e,
            f: self.b * local.e + self.d * local.f + self.f,
            rotation: self.rotation + local.rotation,
            scale_x: self.scale_x * local.scale_x,
            scale_y: self.scale_y * local.scale_y,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Xfrm {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) rotation: f64,
    pub(super) flip_h: bool,
    pub(super) flip_v: bool,
    /// `chOff`/`chExt` of a group: the child coordinate space.
    pub(super) children: Option<(f64, f64, f64, f64)>,
}

pub(super) fn parse_xfrm(xfrm: Node) -> Option<Xfrm> {
    let offset = child(xfrm, "off");
    let extent = child(xfrm, "ext")?;
    let children = child(xfrm, "chOff")
        .zip(child(xfrm, "chExt"))
        .map(|(off, ext)| {
            (
                attr_f64(off, "x").unwrap_or(0.0),
                attr_f64(off, "y").unwrap_or(0.0),
                attr_f64(ext, "cx").unwrap_or(0.0),
                attr_f64(ext, "cy").unwrap_or(0.0),
            )
        });
    Some(Xfrm {
        x: offset.and_then(|o| attr_f64(o, "x")).unwrap_or(0.0),
        y: offset.and_then(|o| attr_f64(o, "y")).unwrap_or(0.0),
        width: attr_f64(extent, "cx").unwrap_or(0.0).max(0.0),
        height: attr_f64(extent, "cy").unwrap_or(0.0).max(0.0),
        rotation: attr_f64(xfrm, "rot").unwrap_or(0.0) / 60_000.0,
        flip_h: attr_bool(xfrm, "flipH").unwrap_or(false),
        flip_v: attr_bool(xfrm, "flipV").unwrap_or(false),
        children,
    })
}

/// The transform a group's children are placed with.
pub(super) fn group_transform(parent: &Transform, xfrm: &Xfrm) -> Transform {
    let (child_x, child_y, child_w, child_h) =
        xfrm.children
            .unwrap_or((xfrm.x, xfrm.y, xfrm.width, xfrm.height));
    let scale_x = if child_w > 0.0 {
        xfrm.width / child_w
    } else {
        1.0
    };
    let scale_y = if child_h > 0.0 {
        xfrm.height / child_h
    } else {
        1.0
    };
    let (center_x, center_y) = (xfrm.x + xfrm.width / 2.0, xfrm.y + xfrm.height / 2.0);
    let (sin, cos) = xfrm.rotation.to_radians().sin_cos();
    let flip_x = if xfrm.flip_h { -1.0 } else { 1.0 };
    let flip_y = if xfrm.flip_v { -1.0 } else { 1.0 };
    // Scale into the group's box, then flip and rotate about its centre.
    let tx = xfrm.x - center_x - scale_x * child_x;
    let ty = xfrm.y - center_y - scale_y * child_y;
    let local = Transform {
        a: cos * flip_x * scale_x,
        b: sin * flip_x * scale_x,
        c: -sin * flip_y * scale_y,
        d: cos * flip_y * scale_y,
        e: cos * flip_x * tx - sin * flip_y * ty + center_x,
        f: sin * flip_x * tx + cos * flip_y * ty + center_y,
        rotation: xfrm.rotation,
        scale_x,
        scale_y,
    };
    parent.then(&local)
}

/// Bounds and rotation on the slide of an element placed by `xfrm`.
pub(super) fn place(xfrm: &Xfrm, transform: &Transform) -> (Rect, f32) {
    let (center_x, center_y) =
        transform.apply(xfrm.x + xfrm.width / 2.0, xfrm.y + xfrm.height / 2.0);
    let width = xfrm.width * transform.scale_x.abs();
    let height = xfrm.height * transform.scale_y.abs();
    (
        Rect {
            x: (center_x - width / 2.0) as f32,
            y: (center_y - height / 2.0) as f32,
            width: width as f32,
            height: height as f32,
        },
        ((xfrm.rotation + transform.rotation) % 360.0) as f32,
    )
}

/// Bezier handle length for a quarter ellipse, as a fraction of its radius.
pub(super) const KAPPA: f32 = 0.552_284_8;

pub(super) fn polygon(points: &[(f32, f32)]) -> ShapePath {
    let mut commands = Vec::with_capacity(points.len() + 1);
    for (i, &(x, y)) in points.iter().enumerate() {
        commands.push(if i == 0 {
            PathCommand::MoveTo(x, y)
        } else {
            PathCommand::LineTo(x, y)
        });
    }
    commands.push(PathCommand::Close);
    ShapePath {
        commands,
        filled: true,
        stroked: true,
    }
}

pub(super) fn ellipse_path(w: f32, h: f32) -> ShapePath {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    ShapePath {
        commands: vec![
            PathCommand::MoveTo(w, ry),
            PathCommand::CubicTo(w, ry + ky, rx + kx, h, rx, h),
            PathCommand::CubicTo(rx - kx, h, 0.0, ry + ky, 0.0, ry),
            PathCommand::CubicTo(0.0, ry - ky, rx - kx, 0.0, rx, 0.0),
            PathCommand::CubicTo(rx + kx, 0.0, w, ry - ky, w, ry),
            PathCommand::Close,
        ],
        filled: true,
        stroked: true,
    }
}

pub(super) fn round_rect_path(w: f32, h: f32, radius: f32) -> ShapePath {
    let r = radius.clamp(0.0, w.min(h) / 2.0);
    let k = r * (1.0 - KAPPA);
    ShapePath {
        commands: vec![
            PathCommand::MoveTo(r, 0.0),
            PathCommand::LineTo(w - r, 0.0),
            PathCommand::CubicTo(w - k, 0.0, w, k, w, r),
            PathCommand::LineTo(w, h - r),
            PathCommand::CubicTo(w, h - k, w - k, h, w - r, h),
            PathCommand::LineTo(r, h),
            PathCommand::CubicTo(k, h, 0.0, h - k, 0.0, h - r),
            PathCommand::LineTo(0.0, r),
            PathCommand::CubicTo(0.0, k, k, 0.0, r, 0.0),
            PathCommand::Close,
        ],
        filled: true,
        stroked: true,
    }
}

/// Outline of a preset shape in a `w` × `h` box. Presets not listed draw as
/// their bounding rectangle.
pub(super) fn preset_geometry(
    name: &str,
    adjust: &HashMap<String, f32>,
    w: f32,
    h: f32,
) -> Vec<ShapePath> {
    let short = w.min(h);
    let adj = |key: &str, default: f32| adjust.get(key).copied().unwrap_or(default) / 100_000.0;
    let rect = || polygon(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]);
    let path = match name {
        "roundRect" | "flowChartAlternateProcess" => {
            round_rect_path(w, h, short * adj("adj", 16_667.0))
        }
        "flowChartTerminator" => round_rect_path(w, h, short / 2.0),
        "ellipse" | "flowChartConnector" => ellipse_path(w, h),
        "line" | "straightConnector1" | "bentConnector2" | "bentConnector3" | "bentConnector4"
        | "bentConnector5" | "curvedConnector2" | "curvedConnector3" | "curvedConnector4"
        | "curvedConnector5" => ShapePath {
            commands: vec![PathCommand::MoveTo(0.0, 0.0), PathCommand::LineTo(w, h)],
            filled: false,
            stroked: true,
        },
        "triangle" | "flowChartExtract" => {
            polygon(&[(w * adj("adj", 50_000.0), 0.0), (w, h), (0.0, h)])
        }
        "rtTriangle" => polygon(&[(0.0, 0.0), (w, h), (0.0, h)]),
        "diamond" | "flowChartDecision" => {
            polygon(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)])
        }
        "parallelogram" | "flowChartInputOutput" => {
            let o = short * adj("adj", 25_000.0);
            polygon(&[(o, 0.0), (w, 0.0), (w - o, h), (0.0, h)])
        }
        "trapezoid" => {
            let o = short * adj("adj", 25_000.0);
            polygon(&[(o, 0.0), (w - o, 0.0), (w, h), (0.0, h)])
        }
        "pentagon" => polygon(&[
            (w / 2.0, 0.0),
            (w, h * 0.382),
            (w * 0.809, h),
            (w * 0.191, h),
            (0.0, h * 0.382),
        ]),
        "hexagon" => {
            let o = short * adj("adj", 25_000.0);
            polygon(&[
                (o, 0.0),
                (w - o, 0.0),
                (w, h / 2.0),
                (w - o, h),
                (o, h),
                (0.0, h / 2.0),
            ])
        }
        "octagon" => {
            let o = short * adj("adj", 29_289.0);
            polygon(&[
                (o, 0.0),
                (w - o, 0.0),
                (w, o),
                (w, h - o),
                (w - o, h),
                (o, h),
                (0.0, h - o),
                (0.0, o),
            ])
        }
        "homePlate" => {
            let o = short * adj("adj", 50_000.0);
            polygon(&[(0.0, 0.0), (w - o, 0.0), (w, h / 2.0), (w - o, h), (0.0, h)])
        }
        "chevron" => {
            let o = short * adj("adj", 50_000.0);
            polygon(&[
                (0.0, 0.0),
                (w - o, 0.0),
                (w, h / 2.0),
                (w - o, h),
                (0.0, h),
                (o, h / 2.0),
            ])
        }
        "rightArrow" | "leftArrow" => {
            let shaft = h * adj("adj1", 50_000.0);
            let head = short * adj("adj2", 50_000.0);
            let (top, bottom) = ((h - shaft) / 2.0, (h + shaft) / 2.0);
            let points = [
                (0.0, top),
                (w - head, top),
                (w - head, 0.0),
                (w, h / 2.0),
                (w - head, h),
                (w - head, bottom),
                (0.0, bottom),
            ];
            if name == "leftArrow" {
                polygon(&points.map(|(x, y)| (w - x, y)))
            } else {
                polygon(&points)
            }
        }
        "upArrow" | "downArrow" => {
            let shaft = w * adj("adj1", 50_000.0);
            let head = short * adj("adj2", 50_000.0);
            let (left, right) = ((w - shaft) / 2.0, (w + shaft) / 2.0);
            let points = [
                (left, h),
                (left, head),
                (0.0, head),
                (w / 2.0, 0.0),
                (w, head),
                (right, head),
                (right, h),
            ];
            if name == "downArrow" {
                polygon(&points.map(|(x, y)| (x, h - y)))
            } else {
                polygon(&points)
            }
        }
        "plus" | "flowChartSummingJunction" => {
            let o = short * adj("adj", 25_000.0);
            polygon(&[
                (o, 0.0),
                (w - o, 0.0),
                (w - o, o),
                (w, o),
                (w, h - o),
                (w - o, h - o),
                (w - o, h),
                (o, h),
                (o, h - o),
                (0.0, h - o),
                (0.0, o),
                (o, o),
            ])
        }
        _ => rect(),
    };
    vec![path]
}

pub(super) fn adjust_values(prst_geom: Node) -> HashMap<String, f32> {
    child(prst_geom, "avLst")
        .into_iter()
        .flat_map(|list| children_named(list, "gd"))
        .filter_map(|gd| {
            let value = gd
                .attribute("fmla")?
                .strip_prefix("val ")?
                .trim()
                .parse()
                .ok()?;
            Some((gd.attribute("name")?.to_string(), value))
        })
        .collect()
}

/// Outline from a `custGeom`. Coordinates that are guide names rather than
/// numbers are not evaluated; a path using them is dropped.
pub(super) fn custom_geometry(cust: Node, w: f32, h: f32) -> Vec<ShapePath> {
    let Some(list) = child(cust, "pathLst") else {
        return Vec::new();
    };
    children_named(list, "path")
        .filter_map(|path| custom_path(path, w, h))
        .collect()
}

pub(super) fn custom_path(path: Node, w: f32, h: f32) -> Option<ShapePath> {
    let path_w = attr_f32(path, "w").filter(|v| *v > 0.0);
    let path_h = attr_f32(path, "h").filter(|v| *v > 0.0);
    let scale_x = path_w.map_or(1.0 / EMU_PER_POINT as f32, |pw| w / pw);
    let scale_y = path_h.map_or(1.0 / EMU_PER_POINT as f32, |ph| h / ph);
    let point = |pt: Node| -> Option<(f32, f32)> {
        Some((attr_f32(pt, "x")? * scale_x, attr_f32(pt, "y")? * scale_y))
    };
    let mut commands = Vec::new();
    let mut current = (0.0, 0.0);
    for command in path.children().filter(Node::is_element) {
        let points: Vec<Node> = children_named(command, "pt").collect();
        match command.tag_name().name() {
            "moveTo" => {
                current = point(*points.first()?)?;
                commands.push(PathCommand::MoveTo(current.0, current.1));
            }
            "lnTo" => {
                current = point(*points.first()?)?;
                commands.push(PathCommand::LineTo(current.0, current.1));
            }
            "cubicBezTo" => {
                let [a, b, c] = [
                    point(*points.first()?)?,
                    point(*points.get(1)?)?,
                    point(*points.get(2)?)?,
                ];
                commands.push(PathCommand::CubicTo(a.0, a.1, b.0, b.1, c.0, c.1));
                current = c;
            }
            "quadBezTo" => {
                let [a, b] = [point(*points.first()?)?, point(*points.get(1)?)?];
                commands.push(PathCommand::QuadTo(a.0, a.1, b.0, b.1));
                current = b;
            }
            "arcTo" => {
                let radius_x = attr_f32(command, "wR")? * scale_x;
                let radius_y = attr_f32(command, "hR")? * scale_y;
                let start = attr_f32(command, "stAng")? / 60_000.0;
                let sweep = attr_f32(command, "swAng")? / 60_000.0;
                current = arc_to(&mut commands, current, radius_x, radius_y, start, sweep);
            }
            "close" => commands.push(PathCommand::Close),
            _ => {}
        }
    }
    (!commands.is_empty()).then(|| ShapePath {
        commands,
        filled: path.attribute("fill") != Some("none"),
        stroked: attr_bool(path, "stroke") != Some(false),
    })
}

/// Append an elliptical arc that starts at `from`, as cubic segments of at
/// most 90°. Angles are clockwise degrees, as DrawingML measures them.
pub(super) fn arc_to(
    commands: &mut Vec<PathCommand>,
    from: (f32, f32),
    radius_x: f32,
    radius_y: f32,
    start: f32,
    sweep: f32,
) -> (f32, f32) {
    let at = |angle: f32| {
        let radians = angle.to_radians();
        (radius_x * radians.cos(), radius_y * radians.sin())
    };
    let start_offset = at(start);
    let center = (from.0 - start_offset.0, from.1 - start_offset.1);
    let segments = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
    let step = sweep / segments as f32;
    let handle = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
    let mut angle = start;
    let mut end = from;
    for _ in 0..segments {
        let (a0, a1) = (angle.to_radians(), (angle + step).to_radians());
        let p0 = (
            center.0 + radius_x * a0.cos(),
            center.1 + radius_y * a0.sin(),
        );
        let p1 = (
            center.0 + radius_x * a1.cos(),
            center.1 + radius_y * a1.sin(),
        );
        let c0 = (
            p0.0 - handle * radius_x * a0.sin(),
            p0.1 + handle * radius_y * a0.cos(),
        );
        let c1 = (
            p1.0 + handle * radius_x * a1.sin(),
            p1.1 - handle * radius_y * a1.cos(),
        );
        commands.push(PathCommand::CubicTo(c0.0, c0.1, c1.0, c1.1, p1.0, p1.1));
        angle += step;
        end = p1;
    }
    end
}
