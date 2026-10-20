//! Native Rust components; reactive updates commit once, animations run natively.
use crate::{
    model::*,
    state::{Removal, State},
};
use nsh_core::{
    AccessibilityRole, LayoutAnimation, LayoutCurve, LayoutTransition, keymap::When,
    ui::AnimatedValue, ui::prelude::*,
};
const MONO: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else {
    "monospace"
};
const BG: u32 = 0x070708;
const PANEL: u32 = 0x101012;
const LINE: u32 = 0x29292d;
const INK: u32 = 0xededf0;
const MUTED: u32 = 0x85858f;
const GREEN: u32 = 0x65f193;
fn caption(value: impl IntoReactive<SharedString>, size: f32, color: u32) -> Text {
    text(value)
        .font_family(MONO)
        .text_size(px(size))
        .text_color(rgb(color))
}
fn button(
    label: impl IntoReactive<SharedString>,
    disabled: impl IntoReactive<bool>,
    action: impl Fn() + 'static,
) -> View {
    control(label, None, disabled, action)
}
fn control(
    label: impl IntoReactive<SharedString>,
    glyph: Option<&'static str>,
    disabled: impl IntoReactive<bool>,
    action: impl Fn() + 'static,
) -> View {
    let disabled = disabled.into_reactive();
    let disabled = computed(move || disabled.get());
    let label = label.into_reactive();
    let label = computed(move || label.get());
    view()
        .role(AccessibilityRole::Button)
        .label(move || label.get())
        .disabled(move || disabled.get())
        .window_drag(false)
        .on_press(move |_: &()| {
            if !disabled.get_untracked() {
                action()
            }
        })
        .px(px(if glyph.is_some() { 10. } else { 13. }))
        .py(px(10.))
        .rounded(px(7.))
        .border_1()
        .border_color(rgb(if glyph == Some("⟳") {
            0x68b985
        } else {
            0x36363d
        }))
        .bg(rgb(if glyph == Some("⟳") {
            0x14251c
        } else {
            0x18181b
        }))
        .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0_f32 })
        .items_center()
        .justify_center()
        .child(
            If::new(move || glyph.is_some(), move || icon(glyph.unwrap())).otherwise(move || {
                caption(move || label.get(), 11., INK)
                    .font_weight(FontWeight::SEMIBOLD)
                    .number_of_lines(1)
            }),
        )
}
fn icon(glyph: &str) -> impl IntoElement {
    let p = Painter::new()
        .stroke_color(rgb(INK))
        .line_width(1.6)
        .begin_path();
    let p = match glyph {
        "⟳" => p
            .arc(8., 8., 5.5, 0.1, std::f32::consts::PI * 1.7)
            .move_to(11., 1.)
            .line_to(12., 4.5)
            .line_to(8.5, 4.5),
        "↑" => p
            .move_to(8., 14.)
            .line_to(8., 2.)
            .move_to(3., 7.)
            .line_to(8., 2.)
            .line_to(13., 7.),
        "☷" => {
            let mut p = p;
            for (y, x) in [(4., 6.), (8., 10.), (12., 5.)] {
                p = p
                    .move_to(2., y)
                    .line_to(x - 1.5, y)
                    .move_to(x + 1.5, y)
                    .line_to(14., y)
                    .rect(bounds(
                        point(px(x - 1.5), px(y - 1.5)),
                        size(px(3.), px(3.)),
                    ));
            }
            p
        }
        _ => p
            .move_to(3., 10.)
            .line_to(10., 3.)
            .line_to(13., 6.)
            .line_to(6., 13.)
            .line_to(2., 14.)
            .close_path(),
    };
    canvas(p.stroke()).w(px(16.)).h(px(16.))
}
fn separator() -> impl IntoElement {
    view().h(px(1.)).bg(rgb(LINE))
}
fn row() -> View {
    view().flex_row().items_center().gap(px(8.))
}
fn app_mark() -> impl IntoElement {
    canvas(
        Painter::new()
            .stroke_color(rgb(GREEN))
            .line_width(1.8)
            .begin_path()
            .arc(12., 12., 7., 0.2, 5.9)
            .stroke()
            .begin_path()
            .move_to(8., 8.)
            .line_to(13., 8.)
            .move_to(8., 12.)
            .line_to(16., 12.)
            .move_to(8., 16.)
            .line_to(12., 16.)
            .stroke(),
    )
    .w(px(24.))
    .h(px(24.))
}
#[derive(Clone, Copy)]
struct Camera {
    scale: AnimatedValue,
    x: AnimatedValue,
    y: AnimatedValue,
}
impl Camera {
    fn set(self, s: State, value: f32) {
        let value = value.clamp(1., 8.);
        s.zoom.set(value);
        self.scale.set(value);
        if value == 1. {
            self.x.set(0.);
            self.y.set(0.);
        }
    }
}
/// Disktree's Rust entry point, shared by `nsh dev`, `nsh run`, and Cargo.
#[component]
pub fn App() -> impl IntoElement {
    nsh_core::ui::desktop::set_title_bar(nsh_core::desktop::TitleBarStyle::Hidden, Some(px(56.)));
    let s = State::new();
    let camera = Camera {
        scale: animated_value(1.),
        x: animated_value(0.),
        y: animated_value(0.),
    };
    let filter = element_ref();
    s.watch();
    effect(move || {
        s.path.get();
        s.page.get();
        s.listing_metric.get();
        camera.set(s, 1.);
    });
    let keys = KeyOptions {
        when: When::NotEditing,
        ..Default::default()
    };
    on_key_with("mod+r", keys, move |_| s.scan(s.path.get_untracked()));
    on_key_with("alt+up", keys, move |_| s.up());
    on_key_with("enter", keys, move |_| s.enter(s.selection.get_untracked()));
    on_key_with("space", keys, move |_| s.mark_pointer());
    for (key, delta) in [("down", 1_isize), ("up", -1)] {
        on_key_with(key, keys, move |_| {
            let list = s.listing.get_untracked().children;
            if list.is_empty() {
                return;
            }
            let index = list
                .iter()
                .position(|n| n.path == s.selection.get_untracked().path)
                .map(|i| i as isize)
                .unwrap_or(-1);
            s.selection
                .set(list[(index + delta).clamp(0, list.len() as isize - 1) as usize].clone());
        });
    }
    on_key_with("mod+plus", keys, move |_| {
        camera.set(s, s.zoom.get_untracked() * 1.5)
    });
    on_key_with("mod+minus", keys, move |_| {
        camera.set(s, s.zoom.get_untracked() / 1.5)
    });
    on_key_with("mod+0", keys, move |_| camera.set(s, 1.));
    on_key_with(
        "escape",
        KeyOptions {
            when: When::Always,
            ..Default::default()
        },
        move |_| {
            if !s.busy.get_untracked() {
                s.review.set(false);
                s.editing.set(false);
                s.options.set(false);
                filter.blur();
            }
        },
    );
    s.scan(String::new());
    view()
        .flex_1()
        .bg(rgb(BG))
        .child(toolbar(s))
        .child(view().h(px(2.)).bg(move || {
            rgb(if s.progress.get().is_some() {
                GREEN
            } else {
                BG
            })
        }))
        .child(
            view()
                .flex_1()
                .min_h(px(150.))
                .flex_row()
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.))
                        .p(px(10.))
                        .gap(px(7.))
                        .child(path_bar(s, filter))
                        .child(legend())
                        .child(treemap(s, camera, filter)),
                )
                .child(sidebar(s)),
        )
        .child(statusbar(s))
        .child(If::new(move || s.options.get(), move || options(s)))
}
fn legend() -> impl IntoElement {
    scroll()
        .horizontal()
        .shows_indicator(false)
        .h(px(20.))
        .flex_shrink(0.)
        .child(
            row()
                .gap(px(13.))
                .children(CATEGORIES.into_iter().map(|(name, color)| {
                    row()
                        .gap(px(4.))
                        .child(view().w(px(8.)).h(px(8.)).rounded(px(1.)).bg(rgb(color)))
                        .child(caption(name, 10., MUTED))
                })),
        )
}
fn options(s: State) -> impl IntoElement {
    view()
        .absolute()
        .right(px(12.))
        .top(px(58.))
        .w(px(230.))
        .p(px(12.))
        .gap(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(PANEL))
        .child(button(
            move || {
                if s.hidden.get() {
                    "✓ Show hidden files"
                } else {
                    "Show hidden files"
                }
            },
            false,
            move || {
                batch(|| {
                    s.hidden.update(|v| *v = !*v);
                    s.page.set(0);
                })
            },
        ))
        .children([1, 2, 3].into_iter().map(move |depth| {
            button(
                move || {
                    format!(
                        "{} {depth} levels",
                        if s.levels.get() == depth { "✓" } else { " " }
                    )
                },
                false,
                move || {
                    s.levels.set(depth);
                    s.options.set(false);
                },
            )
        }))
        .child(separator())
        .child(button(
            "Empty Trash…",
            move || s.locked(),
            move || {
                s.options.set(false);
                s.begin_review(Removal::Empty);
            },
        ))
}
fn toolbar(s: State) -> impl IntoElement {
    row()
        .test_id("titlebar")
        .h(px(56.))
        .gap(px(10.))
        .pl(move || px((nsh_core::ui::desktop::title_bar().left.0 + 8.).max(16.)))
        .pr(px(16.))
        .border_b_1()
        .border_color(rgb(LINE))
        .bg(rgb(0x111113))
        .window_drag(true)
        .child(app_mark())
        .child(caption("disktree", 17., INK).font_weight(FontWeight::SEMIBOLD))
        .child(view().flex_1())
        .child(control(
            "Edit folder path",
            Some("✎"),
            move || s.busy.get(),
            move || {
                s.draft.set(s.path.get_untracked());
                s.editing.update(|v| *v = !*v);
            },
        ))
        .child(control(
            "Scan folder",
            Some("⟳"),
            move || s.busy.get(),
            move || {
                let target = if s.editing.get_untracked() {
                    s.draft.get_untracked()
                } else {
                    s.path.get_untracked()
                };
                s.editing.set(false);
                s.scan(target);
            },
        ))
        .child(control(
            "Parent folder",
            Some("↑"),
            move || s.locked() || s.path.get() == "/",
            move || s.up(),
        ))
        .child(
            row()
                .gap(px(0.))
                .p(px(3.))
                .rounded(px(19.))
                .bg(rgb(0x090c0a))
                .children(
                    [Metric::Size, Metric::Apparent, Metric::Files]
                        .into_iter()
                        .map(move |metric| {
                            view()
                                .role(AccessibilityRole::Tab)
                                .label(metric.label())
                                .accessibility_state(move || nsh_core::AccessibilityState {
                                    selected: s.metric.get() == metric,
                                    ..Default::default()
                                })
                                .window_drag(false)
                                .on_press(move |_: &()| {
                                    batch(|| {
                                        s.metric.set(metric);
                                        s.page.set(0);
                                    })
                                })
                                .w(px(76.))
                                .py(px(8.))
                                .items_center()
                                .rounded(px(15.))
                                .border_1()
                                .border_color(move || {
                                    rgba(if s.metric.get() == metric {
                                        0x68b985ff
                                    } else {
                                        0
                                    })
                                })
                                .bg(move || {
                                    rgba(if s.metric.get() == metric {
                                        0x111a14ff
                                    } else {
                                        0
                                    })
                                })
                                .child(caption(metric.label(), 11., INK))
                        }),
                ),
        )
        .child(control("View options", Some("☷"), false, move || {
            s.options.update(|v| *v = !*v)
        }))
}
fn path_bar(s: State, filter: ElementRef) -> impl IntoElement {
    let width = signal(400.0_f32);
    let overflow = signal(false);
    let crumbs = computed(move || {
        let path = s.path.get();
        let mut full = String::new();
        let mut all = vec![("/".to_string(), "/".to_string())];
        for name in path.split('/').filter(|p| !p.is_empty()) {
            full.push('/');
            full.push_str(name);
            all.push((name.to_owned(), full.clone()));
        }
        let mut budget = (width.get() - 65.).max(40.);
        let mut split = all.len();
        for i in (1..all.len()).rev() {
            let needed = (all[i].0.chars().count() as f32 * 7.2 + 25.).min(200.);
            if budget < needed && split < all.len() {
                break;
            }
            budget -= needed;
            split = i;
        }
        (all[1..split].to_vec(), all[split..].to_vec())
    });
    row()
        .h(px(28.))
        .flex_shrink(0.)
        .child(
            view()
                .flex_1()
                .min_w(px(80.))
                .overflow_hidden()
                .on_layout(move |b: &Bounds<Points>| width.set(b.size.width.0))
                .child(If::new(
                    move || !s.editing.get(),
                    move || {
                        row()
                            .gap(px(3.))
                            .overflow_hidden()
                            .child(
                                button("/", move || s.locked(), move || s.navigate("/".into()))
                                    .p(px(4.)),
                            )
                            .child(If::new(
                                move || !crumbs.get().0.is_empty(),
                                move || {
                                    button(
                                        "…",
                                        move || s.locked(),
                                        move || overflow.update(|v| *v = !*v),
                                    )
                                    .p(px(4.))
                                    .label("Show hidden path segments")
                                },
                            ))
                            .child(Each::new(
                                move || crumbs.get().1,
                                |item| item.1.clone(),
                                move |item, _| {
                                    row()
                                        .min_w(px(0.))
                                        .flex_shrink(1.)
                                        .child(caption("›", 12., MUTED))
                                        .child(
                                            view()
                                                .p(px(4.))
                                                .min_w(px(0.))
                                                .flex_shrink(1.)
                                                .role(AccessibilityRole::Button)
                                                .label(move || format!("Go to {}", item.get().1))
                                                .on_press(move |_: &()| {
                                                    s.navigate(item.get_untracked().1)
                                                })
                                                .child(
                                                    caption(move || item.get().0, 12., GREEN)
                                                        .number_of_lines(1),
                                                ),
                                        )
                                },
                            ))
                    },
                ))
                .child(If::new(
                    move || s.editing.get(),
                    move || {
                        input()
                            .label("Folder path")
                            .auto_focus(true)
                            .value(move || s.draft.get())
                            .on_change(move |v: &str| s.draft.set(v.into()))
                            .on_submit(move |_: &str| {
                                s.editing.set(false);
                                s.scan(s.draft.get_untracked());
                            })
                            .font_family(MONO)
                            .text_color(rgb(INK))
                            .text_size(px(12.))
                            .p(px(6.))
                            .bg(rgb(0x18181c))
                    },
                )),
        )
        .child(
            view()
                .w(px(220.))
                .h(px(28.))
                .flex_shrink(0.)
                .justify_center()
                .child(
                    caption(
                        move || {
                            let n = s.listing.get().node;
                            if n.path.is_empty() {
                                String::new()
                            } else {
                                format!("{} · {} files", format_size(n.size as f64), count(n.files))
                            }
                        },
                        11.,
                        MUTED,
                    )
                    .number_of_lines(1)
                    .text_right(),
                ),
        )
        .child(
            input()
                .element_ref(&filter)
                .label("Filter names")
                .value(move || s.query.get())
                .on_change(move |v: &str| {
                    batch(|| {
                        s.query.set(v.into());
                        s.page.set(0);
                    })
                })
                .placeholder("Filter names…")
                .font_family(MONO)
                .w(px(130.))
                .flex_shrink(0.)
                .text_size(px(11.))
                .text_color(rgb(INK))
                .p(px(6.))
                .bg(rgb(0x18181c))
                .rounded(px(4.)),
        )
        .child(If::new(
            move || overflow.get(),
            move || {
                view()
                    .absolute()
                    .left(px(10.))
                    .top(px(94.))
                    .w(px(350.))
                    .p(px(10.))
                    .bg(rgb(PANEL))
                    .border_1()
                    .border_color(rgb(LINE))
                    .child(Each::new(
                        move || crumbs.get().0,
                        |item| item.1.clone(),
                        move |item, _| {
                            button(
                                move || item.get().1,
                                false,
                                move || {
                                    s.navigate(item.get_untracked().1);
                                    overflow.set(false);
                                },
                            )
                        },
                    ))
            },
        ))
}
fn treemap(s: State, camera: Camera, filter: ElementRef) -> impl IntoElement {
    let frame = signal((700.0_f32, 520.0_f32));
    let tiles = computed(move || {
        let (w, h) = frame.get();
        mosaic(
            &s.listing.get().map,
            w,
            h,
            s.listing_metric.get(),
            s.levels.get().max(if s.zoom.get() >= 1.8 { 3 } else { 1 }),
            s.zoom.get(),
        )
    });
    effect(move || {
        tiles.get();
        if !reduce_motion() {
            animate_layout(LayoutAnimation {
                create: Some(LayoutTransition::new(LayoutCurve::EaseOut, 320.)),
                update: Some(LayoutTransition::new(LayoutCurve::EaseInEaseOut, 320.)),
                delete: Some(LayoutTransition::new(LayoutCurve::EaseOut, 160.)),
            });
        }
    });
    let scale = camera.scale.interpolate(&[1., 8.], &[1., 8.]);
    let width = animated_value(700.);
    let height = animated_value(520.);
    let max_x = scale.sub(1.).mul(&width).mul(0.5);
    let max_y = scale.sub(1.).mul(&height).mul(0.5);
    let pan_x = camera
        .x
        .div(&max_x.add(0.001))
        .interpolate(&[-1., 1.], &[-1., 1.])
        .mul(&max_x);
    let pan_y = camera
        .y
        .div(&max_y.add(0.001))
        .interpolate(&[-1., 1.], &[-1., 1.])
        .mul(&max_y);

    // Native animated graphs clamp both scale and the pan bounds.
    view()
        .test_id("treemap")
        .flex_1()
        .overflow_hidden()
        .rounded(px(3.))
        .bg(rgb(0x050506))
        .on_layout(move |b: &Bounds<Points>| {
            frame.set((b.size.width.0, b.size.height.0));
            width.set(b.size.width.0);
            height.set(b.size.height.0);
        })
        .on_hover_in(move |_: &()| {
            s.in_map.set(true);
            filter.blur();
        })
        .on_hover_out(move |_: &()| {
            s.in_map.set(false);
            s.hovered.set(Vec::new());
        })
        .pinch_value(&camera.scale)
        .pan_values(Some(&camera.x), Some(&camera.y))
        .on_pinch_end(move |_: &GestureEvent| {
            s.owner.run(move || {
                spawn(async move {
                    camera.set(s, camera.scale.value().await);
                })
            });
        })
        .child(
            view()
                .w(move || px(frame.get().0))
                .h(move || px(frame.get().1))
                .bind_scale(&scale)
                .bind_translate_x(&pan_x)
                .bind_translate_y(&pan_y)
                .child(Each::new(
                    move || tiles.get(),
                    |tile| tile.node.path.clone(),
                    move |tile, _| map_tile(s, tile),
                )),
        )
        .child(If::new(
            move || tiles.get().is_empty(),
            move || {
                view()
                    .absolute()
                    .inset(px(0.))
                    .items_center()
                    .justify_center()
                    .gap(px(14.))
                    .child(caption(
                        move || {
                            if s.busy.get() {
                                "Mapping your space"
                            } else {
                                "See the shape of your storage."
                            }
                        },
                        24.,
                        INK,
                    ))
                    .child(caption(
                        move || {
                            if s.busy.get() {
                                "Reading folders and measuring their footprint…"
                            } else {
                                "Scan a folder. Find the weight. Make room."
                            }
                        },
                        12.,
                        MUTED,
                    ))
            },
        ))
}
fn map_tile(s: State, tile: ReadSignal<Tile>) -> impl IntoElement {
    let marked = computed(move || {
        s.marked.get().iter().any(|n| {
            tile.get().node.path == n.path
                || tile.get().node.path.starts_with(&(n.path.clone() + "/"))
        })
    });
    let departing = tile.get_untracked().node;
    cleanup(move || s.hover(departing, false));
    view()
        .absolute()
        .left(move || px(tile.get().x + 1. / s.zoom.get()))
        .top(move || px(tile.get().y + 1. / s.zoom.get()))
        .w(move || px((tile.get().width - 2. / s.zoom.get()).max(0.)))
        .h(move || px((tile.get().height - 2. / s.zoom.get()).max(0.)))
        .rounded(px(3.))
        .overflow_hidden()
        .border_1()
        .border_color(move || {
            let t = tile.get();
            let alpha = if marked.get() {
                255
            } else if s.selection.get().path == t.node.path {
                242
            } else {
                97
            };
            rgba(
                (if marked.get() {
                    0xffd36c
                } else {
                    tile_color(&t.node, t.depth)
                } << 8)
                    | alpha,
            )
        })
        .bg(move || {
            let t = tile.get();
            rgba(
                (if marked.get() {
                    0xd29123
                } else {
                    tile_color(&t.node, t.depth)
                } << 8)
                    | 35,
            )
        })
        .bg_linear_to_br()
        .gradient_from(move || {
            let t = tile.get();
            rgba((tile_color(&t.node, t.depth) << 8) | 18)
        })
        .gradient_to(rgba(0x0000001f))
        .p(move || px(5. / s.zoom.get()))
        .role(AccessibilityRole::Button)
        .label(move || tile.get().node.name)
        .on_press(move |_: &()| s.choose(tile.get_untracked().node))
        .on_hover_in(move |_: &()| s.hover(tile.get_untracked().node, true))
        .on_hover_out(move |_: &()| s.hover(tile.get_untracked().node, false))
        .child(If::new(
            move || tile.get().width * s.zoom.get() > 70. && tile.get().height * s.zoom.get() > 26.,
            move || {
                row()
                    .gap(px(5.))
                    .child(
                        caption(
                            move || {
                                format!(
                                    "{}{}",
                                    if marked.get() { "✓ " } else { "" },
                                    tile.get().node.name
                                )
                            },
                            11.,
                            INK,
                        )
                        .text_size(move || px(11. / s.zoom.get()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .flex_1()
                        .number_of_lines(1),
                    )
                    .child(If::new(
                        move || tile.get().node.directory && tile.get().width * s.zoom.get() > 140.,
                        move || {
                            caption("↗", 13., INK)
                                .on_press(move |_: &()| s.enter(tile.get_untracked().node))
                        },
                    ))
            },
        ))
        .child(If::new(
            move || {
                let t = tile.get();
                t.width * s.zoom.get() > 90.
                    && t.height * s.zoom.get() > 70.
                    && (t.depth >= s.levels.get() || t.node.children.is_empty())
            },
            move || {
                caption(
                    move || {
                        let n = tile.get().node;
                        if s.listing_metric.get() == Metric::Files {
                            format!("{} files", count(n.files))
                        } else {
                            format_size(s.listing_metric.get().weight(&n) as f64)
                        }
                    },
                    22.,
                    INK,
                )
                .mt(px(5.))
                .text_size(move || {
                    px(if tile.get().width * s.zoom.get() > 200. {
                        22.
                    } else {
                        13.
                    } / s.zoom.get())
                })
            },
        ))
        .child(If::new(
            move || {
                marked.get()
                    && tile.get().width * s.zoom.get() > 110.
                    && tile.get().height * s.zoom.get() > 50.
            },
            move || {
                caption("✓ MARKED", 9., 0x382305)
                    .absolute()
                    .right(px(5.))
                    .bottom(px(5.))
                    .p(px(3.))
                    .bg(rgb(0xffd36c))
                    .rounded(px(3.))
            },
        ))
}
fn stat(label: &'static str, value: impl IntoReactive<SharedString>) -> impl IntoElement {
    view()
        .flex_1()
        .gap(px(5.))
        .child(caption(label, 10., MUTED))
        .child(caption(value, 15., INK))
}
fn sidebar(s: State) -> impl IntoElement {
    view()
        .w(px(284.))
        .flex_shrink(0.)
        .bg(rgb(PANEL))
        .border_l_1()
        .border_color(rgb(LINE))
        .child(
            scroll().flex_1().child(
                view()
                    .p(px(16.))
                    .gap(px(12.))
                    .child(caption(
                        move || {
                            if s.review.get() {
                                if s.removal.get() == Removal::Empty {
                                    "EMPTY TRASH"
                                } else {
                                    "REVIEW REMOVAL"
                                }
                            } else {
                                "SELECTION"
                            }
                        },
                        10.,
                        MUTED,
                    ))
                    .child(If::new(move || !s.review.get(), move || selection_panel(s)))
                    .child(If::new(move || s.review.get(), move || review_panel(s)))
                    .child(separator())
                    .child(caption(
                        move || {
                            format!(
                                "{} marked · {}",
                                s.marked.get().len(),
                                format_size(
                                    s.marked.get().iter().map(|n| n.size).sum::<u64>() as f64
                                )
                            )
                        },
                        12.,
                        INK,
                    ))
                    .child(If::new(
                        move || !s.review.get(),
                        move || {
                            view()
                                .gap(px(12.))
                                .child(If::new(
                                    move || !s.marked.get().is_empty(),
                                    move || {
                                        button(
                                            "Review marked items →",
                                            move || s.locked(),
                                            move || s.begin_review(Removal::Trash),
                                        )
                                    },
                                ))
                                .child(separator())
                                .child(
                                    row()
                                        .justify_between()
                                        .child(caption(
                                            move || {
                                                format!(
                                                    "CONTENTS · {} ITEMS",
                                                    s.listing.get().count
                                                )
                                            },
                                            10.,
                                            MUTED,
                                        ))
                                        .child(caption(
                                            move || {
                                                let v = s.listing.get();
                                                if v.count == 0 {
                                                    String::new()
                                                } else {
                                                    format!(
                                                        "{}–{}",
                                                        v.offset + 1,
                                                        (v.offset + v.page_size).min(v.count)
                                                    )
                                                }
                                            },
                                            10.,
                                            MUTED,
                                        )),
                                )
                                .child(If::new(
                                    move || s.listing.get().count > 80,
                                    move || {
                                        row()
                                            .justify_between()
                                            .child(
                                                button(
                                                    "‹ Previous",
                                                    move || s.locked() || s.page.get() == 0,
                                                    move || {
                                                        s.page.update(|p| *p = p.saturating_sub(80))
                                                    },
                                                )
                                                .py(px(4.))
                                                .px(px(8.)),
                                            )
                                            .child(
                                                button(
                                                    "Next ›",
                                                    move || {
                                                        let v = s.listing.get();
                                                        s.locked()
                                                            || v.offset + v.page_size >= v.count
                                                    },
                                                    move || s.page.update(|p| *p += 80),
                                                )
                                                .py(px(4.))
                                                .px(px(8.)),
                                            )
                                    },
                                ))
                                .child(Each::new(
                                    move || s.listing.get().children,
                                    |node| node.path.clone(),
                                    move |node, _| directory_row(s, node),
                                ))
                        },
                    )),
            ),
        )
        .child(
            view()
                .p(px(16.))
                .gap(px(9.))
                .border_t_1()
                .border_color(rgb(LINE))
                .bg(rgb(0x141416))
                .child(caption("DISK SPACE", 10., MUTED))
                .child(caption(
                    move || format!("{} free", format_size(s.snapshot.get().free as f64)),
                    25.,
                    INK,
                ))
                .child(
                    view().h(px(5.)).rounded(px(2.)).bg(rgb(0x28282d)).child(
                        view()
                            .h(px(5.))
                            .rounded(px(2.))
                            .bg(rgb(0x56b675))
                            .w(move || {
                                relative(if s.snapshot.get().total > 0 {
                                    1. - s.snapshot.get().free as f32
                                        / s.snapshot.get().total as f32
                                } else {
                                    0.
                                })
                            }),
                    ),
                )
                .child(
                    row()
                        .justify_between()
                        .child(caption(
                            move || {
                                format!(
                                    "{} used",
                                    format_size(
                                        s.snapshot.get().total.saturating_sub(s.snapshot.get().free)
                                            as f64
                                    )
                                )
                            },
                            10.,
                            MUTED,
                        ))
                        .child(caption(
                            move || format!("{} total", format_size(s.snapshot.get().total as f64)),
                            10.,
                            MUTED,
                        )),
                ),
        )
}
fn selection_panel(s: State) -> impl IntoElement {
    view()
        .gap(px(12.))
        .child(
            row()
                .child(
                    view()
                        .w(px(3.))
                        .h(px(19.))
                        .bg(move || rgb(category(&s.selection.get().path).1)),
                )
                .child(
                    caption(
                        move || {
                            let n = s.selection.get();
                            if n.name.is_empty() {
                                "Nothing selected".into()
                            } else {
                                n.name
                            }
                        },
                        20.,
                        INK,
                    )
                    .font_weight(FontWeight::SEMIBOLD)
                    .flex_1()
                    .number_of_lines(1),
                ),
        )
        .child(caption(move || s.selection.get().path, 11., MUTED))
        .child(caption(
            move || format_size(s.selection.get().size as f64),
            38.,
            INK,
        ))
        .child(
            view().h(px(4.)).rounded(px(2.)).bg(rgb(0x26262b)).child(
                view()
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(move || rgb(category(&s.selection.get().path).1))
                    .w(move || {
                        relative(
                            (s.selection.get().size as f32
                                / s.snapshot.get().root.size.max(1) as f32)
                                .min(1.),
                        )
                    }),
            ),
        )
        .child(
            row()
                .gap(px(16.))
                .child(stat("OF SCAN", move || {
                    format!(
                        "{:.1}%",
                        100. * s.selection.get().size as f64
                            / s.snapshot.get().root.size.max(1) as f64
                    )
                }))
                .child(stat("FILES", move || count(s.selection.get().files))),
        )
        .child(
            row()
                .gap(px(16.))
                .child(stat("MODIFIED", move || {
                    modified(s.selection.get().modified)
                }))
                .child(stat("KIND", move || category(&s.selection.get().path).0)),
        )
        .child(
            row()
                .gap(px(6.))
                .child(button(
                    "Open folder",
                    move || s.locked() || !s.selection.get().directory,
                    move || s.enter(s.selection.get_untracked()),
                ))
                .child(button(
                    "Mark / unmark",
                    move || s.locked(),
                    move || s.mark(s.selection.get_untracked()),
                )),
        )
}
fn modified(seconds: f64) -> String {
    if seconds <= 0. {
        return "—".into();
    }
    let timestamp = seconds as libc::time_t;
    let mut date = std::mem::MaybeUninit::<libc::tm>::uninit();
    // localtime_r writes the supplied tm and never returns shared storage.
    unsafe {
        if libc::localtime_r(&timestamp, date.as_mut_ptr()).is_null() {
            return "—".into();
        }
        let date = date.assume_init();
        format!(
            "{}/{}/{}",
            date.tm_mon + 1,
            date.tm_mday,
            date.tm_year + 1900
        )
    }
}
fn directory_row(s: State, node: ReadSignal<Node>) -> impl IntoElement {
    view()
        .py(px(7.))
        .gap(px(5.))
        .border_b_1()
        .border_color(rgb(0x242428))
        .role(AccessibilityRole::Button)
        .label(move || format!("Select {}", node.get().name))
        .on_press(move |_: &()| s.choose(node.get_untracked()))
        .child(
            row()
                .gap(px(6.))
                .child(
                    view()
                        .w(px(3.))
                        .h(px(13.))
                        .bg(move || rgb(category(&node.get().path).1)),
                )
                .child(
                    caption(
                        move || {
                            format!(
                                "{}{}{}",
                                if s.marked.get().iter().any(|n| n.path == node.get().path) {
                                    "✓ "
                                } else {
                                    ""
                                },
                                if node.get().directory { "▸ " } else { "" },
                                node.get().name
                            )
                        },
                        11.,
                        INK,
                    )
                    .flex_1()
                    .number_of_lines(1)
                    .text_color(move || {
                        rgb(if s.selection.get().path == node.get().path {
                            GREEN
                        } else {
                            INK
                        })
                    }),
                )
                .child(caption(
                    move || {
                        if s.listing_metric.get() == Metric::Files {
                            count(node.get().files)
                        } else {
                            format_size(s.listing_metric.get().weight(&node.get()) as f64)
                        }
                    },
                    10.,
                    MUTED,
                )),
        )
        .child(
            view().h(px(2.)).bg(rgb(0x101012)).child(
                view()
                    .h(px(2.))
                    .bg(move || rgb(category(&node.get().path).1))
                    .w(move || {
                        relative(
                            s.listing_metric.get().weight(&node.get()) as f32
                                / s.listing_metric.get().weight(&s.listing.get().node).max(1)
                                    as f32,
                        )
                    }),
            ),
        )
}
fn review_panel(s: State) -> impl IntoElement {
    view()
        .gap(px(12.))
        .child(caption(
            move || match s.removal.get() {
                Removal::Trash => "Move these items to Trash?",
                Removal::Delete => "Permanently delete these items?",
                Removal::Empty => "Empty the system Trash?",
            },
            15.,
            INK,
        ))
        .child(caption(
            move || {
                if s.removal.get() == Removal::Trash {
                    "Recoverable from the system Trash."
                } else {
                    "This permanently removes files and cannot be undone."
                }
            },
            11.,
            MUTED,
        ))
        .child(Each::new(
            move || {
                if s.removal.get() == Removal::Empty {
                    Vec::new()
                } else {
                    s.reviewed.get()
                }
            },
            |n| n.path.clone(),
            move |node, _| {
                view()
                    .gap(px(7.))
                    .py(px(6.))
                    .child(caption(move || node.get().path, 11., INK))
                    .child(button(
                        move || format!("Exclude · {}", format_size(node.get().size as f64)),
                        move || s.busy.get(),
                        move || {
                            s.reviewed.update(|nodes| {
                                nodes.retain(|n| n.path != node.get_untracked().path)
                            })
                        },
                    ))
            },
        ))
        .child(
            button(
                move || {
                    if s.busy.get() {
                        "Working…"
                    } else {
                        match s.removal.get() {
                            Removal::Trash => "Confirm move to Trash",
                            Removal::Delete => "Confirm permanent deletion",
                            Removal::Empty => "Confirm empty Trash",
                        }
                    }
                },
                move || {
                    s.locked() || s.removal.get() != Removal::Empty && s.reviewed.get().is_empty()
                },
                move || s.commit(),
            )
            .bg(move || {
                rgb(if s.removal.get() == Removal::Trash {
                    0x14251c
                } else {
                    0x38201e
                })
            })
            .border_color(move || {
                rgb(if s.removal.get() == Removal::Trash {
                    0x68b985
                } else {
                    0xa65a50
                })
            }),
        )
        .child(If::new(
            move || s.removal.get() == Removal::Trash,
            move || {
                button(
                    "Delete permanently",
                    move || s.locked() || s.reviewed.get().is_empty(),
                    move || s.removal.set(Removal::Delete),
                )
            },
        ))
        .child(button(
            "Back to explorer",
            move || s.busy.get(),
            move || s.review.set(false),
        ))
}
fn statusbar(s: State) -> impl IntoElement {
    row().test_id("statusbar").h(px(28.)).flex_shrink(0.).gap(px(16.)).px(px(12.)).border_t_1().border_color(rgb(LINE)).bg(rgb(PANEL))
        .child(If::new(move||s.progress.get().is_some(),move||row().test_id("scan-progress").flex_1().min_w(px(0.)).gap(px(12.))
            .child(caption("Scanning folders",10.,GREEN))
            .child(caption(move||{let p=s.progress.get().unwrap_or_default();format!("{} folders · {} files · {} found · {}s elapsed{}",count(p.directories),count(p.files),format_size(p.bytes as f64),s.seconds.get(),if p.errors>0{format!(" · {} skipped",p.errors)}else{String::new()})},10.,MUTED).number_of_lines(1))
            .child(caption(move||s.progress.get().unwrap_or_default().path,10.,INK).number_of_lines(1).flex_1())))
        .child(If::new(move||s.progress.get().is_none(),move||row().flex_1().gap(px(16.)).child(caption("Pinch / ⌘± Zoom   ⌘0 Fit   Click selected Enter   Space Mark   ⌥↑ Parent   ⌘R Rescan   Esc Close review",10.,MUTED)).child(caption(move||s.message.get(),10.,MUTED).number_of_lines(1).flex_1().text_right())))
}
