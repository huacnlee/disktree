//! Native Rust components; reactive updates commit once, animations run natively.
use crate::{
    model::*,
    state::{Removal, State},
};
use nsh_base::{MenuEvent, context_menu, menu, menu_item, menu_separator, nav_page, nav_stack};
use nsh_core::{
    AccessibilityRole, LayoutAnimation, LayoutCurve, LayoutTransition, keymap::When,
    ui::AnimatedValue, ui::StyleRefinement, ui::prelude::*,
};
use std::time::Duration;
const MONO: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else {
    "monospace"
};
const BG: u32 = 0x070708;
const PANEL: u32 = 0x101012;
const RAISED: u32 = 0x18181b;
const LINE: u32 = 0x29292d;
const INK: u32 = 0xededf0;
const MUTED: u32 = 0x85858f;
const GREEN: u32 = 0x65f193;
const RED: u32 = 0xff8078;
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
    colored_control(label, glyph, disabled, INK, action)
}
fn colored_control(
    label: impl IntoReactive<SharedString>,
    glyph: Option<&'static str>,
    disabled: impl IntoReactive<bool>,
    color: impl IntoReactive<u32>,
    action: impl Fn() + 'static,
) -> View {
    let disabled = disabled.into_reactive();
    let disabled = computed(move || disabled.get());
    chip(label, glyph, move || disabled.get(), color).on_press(move |_: &()| {
        if !disabled.get_untracked() {
            action()
        }
    })
}
/// A control's look without its action: menus press their own triggers.
fn chip(
    label: impl IntoReactive<SharedString>,
    glyph: Option<&'static str>,
    disabled: impl IntoReactive<bool>,
    color: impl IntoReactive<u32>,
) -> View {
    let color = color.into_reactive();
    let color = computed(move || color.get());
    let disabled = disabled.into_reactive();
    let disabled = computed(move || disabled.get());
    let label = label.into_reactive();
    let label = computed(move || label.get());
    view()
        .role(AccessibilityRole::Button)
        .label(move || label.get())
        .disabled(move || disabled.get())
        .window_drag(false)
        .when(glyph.is_some(), |v| v.tooltip(label.get_untracked()))
        .map(|v| {
            if glyph.is_some() {
                // Title bar icons are ghost buttons: no fill or border until
                // the pointer is on them.
                v.w(px(32.))
                    .h(px(32.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(rgb(0x1f1f24)))
                    .active(|s| s.bg(rgb(0x2a2a30)))
            } else {
                v.px(px(13.))
                    .py(px(10.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(rgb(0x36363d))
                    .bg(rgb(RAISED))
                    .hover(|s| s.bg(rgb(0x222227)))
            }
        })
        .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0_f32 })
        .items_center()
        .justify_center()
        .child(
            when(move || glyph.is_some(), move || icon(glyph.unwrap())).otherwise(move || {
                caption(move || label.get(), 11., INK)
                    .text_color(move || rgb(color.get()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .number_of_lines(1)
            }),
        )
}
fn icon(glyph: &str) -> impl IntoElement {
    let p = Painter::new()
        .stroke_color(rgb(if glyph == "⟳" { GREEN } else { INK }))
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
        "‹" => p.move_to(10., 3.).line_to(5., 8.).line_to(10., 13.),
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
fn row() -> View {
    view().flex_row().items_center().gap(px(8.))
}
fn meter(fraction: impl Fn() -> f32 + 'static, height: f32, color: u32) -> impl IntoElement {
    view()
        .h(px(height))
        .rounded(px(2.))
        .bg(rgb(0x28282d))
        .overflow_hidden()
        .child(
            view()
                .h(px(height))
                .rounded(px(2.))
                .bg(rgb(color))
                .w(move || relative(fraction().clamp(0., 1.))),
        )
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
/// The popup every menu shares: a raised panel over a hairline.
fn popup(s: StyleRefinement) -> StyleRefinement {
    s.min_w(px(220.))
        .p(px(5.))
        .gap(px(1.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(0x34343a))
        .bg(rgb(0x161619))
}
fn entry(
    label: impl IntoReactive<SharedString>,
    disabled: impl IntoReactive<bool>,
    color: u32,
    action: impl Fn() + 'static,
) -> impl IntoElement {
    let label = label.into_reactive();
    let label = computed(move || label.get());
    let disabled = disabled.into_reactive();
    menu_item()
        .accessibility_label(move || Some(label.get()))
        .disabled(move || disabled.get())
        .on_select(move |_: &()| action())
        .px(px(10.))
        .py(px(7.))
        .rounded(px(5.))
        .hover(|s| s.bg(rgb(0x24242a)))
        .disabled_style(|s| s.opacity(0.4))
        .child(
            caption(move || label.get(), 11., color)
                .font_weight(FontWeight::MEDIUM)
                .number_of_lines(1),
        )
}
fn check(on: bool, label: impl std::fmt::Display) -> String {
    format!("{} {label}", if on { "✓" } else { " " })
}
fn rule() -> impl IntoElement {
    menu_separator().h(px(1.)).my(px(4.)).bg(rgb(LINE))
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
/// Commands for unattended smoke tests: `wait <ms>`, `key mod+r`, `quit`.
fn run_script(s: State, script: String) {
    s.owner.run(move || {
        spawn(async move {
            for command in script.split(';').map(str::trim).filter(|c| !c.is_empty()) {
                let mut parts = command.split_whitespace();
                match (parts.next(), parts.next()) {
                    (Some("wait"), Some(ms)) => {
                        timer(Duration::from_millis(ms.parse().unwrap_or(0))).await
                    }
                    (Some("key"), Some("cmd+r" | "mod+r")) => s.scan(s.path.get_untracked()),
                    (Some("quit"), None) => std::process::exit(0),
                    _ => log::warn!("Unknown DISKTREE_UI_SCRIPT command: {command}"),
                }
            }
        })
    });
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
                s.close();
                s.editing.set(false);
                filter.blur();
            }
        },
    );
    s.scan(String::new());
    if let Ok(script) = std::env::var("DISKTREE_UI_SCRIPT") {
        run_script(s, script);
    }
    view()
        .flex_1()
        .bg(rgb(BG))
        .child(toolbar(s))
        .child(
            view().h(px(2.)).bg(rgb(BG)).child(
                view()
                    .h(px(2.))
                    .bg(move || {
                        rgb(if s.removing.get().is_some() {
                            RED
                        } else {
                            GREEN
                        })
                    })
                    .w(move || {
                        let percent = s
                            .removing
                            .get()
                            .map(|p| p.percent)
                            .or_else(|| s.progress.get().map(|p| p.percent));
                        relative(percent.map_or(0., |p| (p.max(2) as f32 / 100.).min(1.)))
                    }),
            ),
        )
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
fn view_menu(s: State) -> impl IntoElement {
    menu()
        .placement(Placement::Bottom)
        .align(Align::End)
        .offset(px(6.))
        .popup_style(popup)
        .trigger(chip("View options", Some("☷"), false, INK))
        .child(entry(
            move || check(s.hidden.get(), "Show hidden files"),
            false,
            INK,
            move || {
                batch(|| {
                    s.hidden.update(|v| *v = !*v);
                    s.page.set(0);
                })
            },
        ))
        .child(rule())
        .children([1, 2, 3].into_iter().map(move |depth| {
            entry(
                move || check(s.levels.get() == depth, format!("{depth} levels deep")),
                false,
                INK,
                move || s.levels.set(depth),
            )
        }))
        .child(rule())
        .child(entry(
            "Empty Trash…",
            move || s.locked(),
            RED,
            move || s.begin_review(Removal::Empty),
        ))
}
fn toolbar(s: State) -> impl IntoElement {
    row()
        .test_id("titlebar")
        .h(px(56.))
        .gap(px(10.))
        .pl(move || px((nsh_core::ui::desktop::title_bar().left.0 + 8.).max(16.)))
        // Native controls can occupy either edge (Linux decoration layouts,
        // Windows caption buttons, or macOS traffic lights).
        .pr(move || px((nsh_core::ui::desktop::title_bar().right.0 + 8.).max(16.)))
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
            row().gap(px(2.)).children(
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
                            .h(px(32.))
                            .px(px(10.))
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .bg(move || {
                                rgba(if s.metric.get() == metric {
                                    0x1f1f24ff
                                } else {
                                    0
                                })
                            })
                            .hover(|s| s.bg(rgb(0x1f1f24)))
                            .active(|s| s.bg(rgb(0x2a2a30)))
                            .child(caption(metric.label(), 11., INK).text_color(move || {
                                rgb(if s.metric.get() == metric {
                                    GREEN
                                } else {
                                    MUTED
                                })
                            }))
                    }),
            ),
        )
        .child(view_menu(s))
}
fn crumb(s: State, item: ReadSignal<(String, String)>) -> impl IntoElement {
    row()
        .min_w(px(0.))
        .flex_shrink(1.)
        .child(caption("›", 12., MUTED))
        .child(
            view()
                .p(px(4.))
                .min_w(px(0.))
                .flex_shrink(1.)
                .rounded(px(4.))
                .hover(|s| s.bg(rgb(0x18181c)))
                .role(AccessibilityRole::Button)
                .label(move || format!("Go to {}", item.get().1))
                .on_press(move |_: &()| s.navigate(item.get_untracked().1))
                .child(caption(move || item.get().0, 12., GREEN).number_of_lines(1)),
        )
}
fn path_bar(s: State, filter: ElementRef) -> impl IntoElement {
    let width = signal(400.0_f32);
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
                .child(when(
                    move || !s.editing.get(),
                    move || {
                        row()
                            .gap(px(3.))
                            .overflow_hidden()
                            .child(
                                button("/", move || s.locked(), move || s.navigate("/".into()))
                                    .p(px(4.)),
                            )
                            .child(when(
                                move || !crumbs.get().0.is_empty(),
                                move || {
                                    menu()
                                        .offset(px(4.))
                                        .popup_style(popup)
                                        .trigger(
                                            chip("…", None, move || s.locked(), INK)
                                                .p(px(4.))
                                                .label("Show hidden path segments"),
                                        )
                                        .child(each(
                                            move || crumbs.get().0,
                                            |item| item.1.clone(),
                                            move |item, _| {
                                                entry(
                                                    move || item.get().1,
                                                    false,
                                                    INK,
                                                    move || s.navigate(item.get_untracked().1),
                                                )
                                            },
                                        ))
                                },
                            ))
                            .child(each(
                                move || crumbs.get().1,
                                |item| item.1.clone(),
                                move |item, _| crumb(s, item),
                            ))
                    },
                ))
                .child(when(
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
}
/// A context menu for the item under the pointer (the deepest hovered tile
/// or row), captured as the menu opens so moving onto it changes nothing.
fn node_menu(s: State, trigger: impl IntoElement) -> impl IntoElement {
    let target = signal(Node::default());
    // Component roots take no layout of their own: wrap only content whose
    // size does not depend on its parent's (the map's frame, the rows).
    context_menu()
        .popup_style(popup)
        .on_open_change(move |event: &MenuEvent| {
            if let MenuEvent::OpenChange(true, _) = event {
                let node = s
                    .hovered
                    .get_untracked()
                    .into_iter()
                    .max_by_key(|n| n.path.len())
                    .unwrap_or_else(|| s.selection.get_untracked());
                target.set(node);
            }
        })
        .trigger(trigger)
        .child(
            caption(move || target.get().name, 10., MUTED)
                .px(px(10.))
                .py(px(5.))
                .number_of_lines(1),
        )
        .child(entry(
            "Show details",
            move || target.get().aggregate,
            INK,
            move || s.selection.set(target.get_untracked()),
        ))
        .child(entry(
            "Open folder",
            move || s.locked() || !target.get().directory,
            INK,
            move || s.enter(target.get_untracked()),
        ))
        .child(rule())
        .child(entry(
            move || {
                let marked = s.marked.get().iter().any(|n| n.path == target.get().path);
                if marked { "Unmark" } else { "Mark for removal" }
            },
            move || s.locked() || target.get().aggregate,
            INK,
            move || s.mark(target.get_untracked()),
        ))
        .child(entry(
            move || format!("Review {} marked…", s.marked.get().len()),
            move || s.locked() || s.marked.get().is_empty(),
            INK,
            move || s.begin_review(Removal::Trash),
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
        .child(node_menu(
            s,
            view()
                .w(move || px(frame.get().0))
                .h(move || px(frame.get().1))
                .bind_scale(&scale)
                .bind_translate_x(&pan_x)
                .bind_translate_y(&pan_y)
                .child(each(
                    move || tiles.get(),
                    |tile| tile.node.path.clone(),
                    move |tile, _| map_tile(s, tile),
                )),
        ))
        .child(when(
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
        .child(when(
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
                    .child(when(
                        move || tile.get().node.directory && tile.get().width * s.zoom.get() > 140.,
                        move || {
                            caption("↗", 13., INK)
                                .on_press(move |_: &()| s.enter(tile.get_untracked().node))
                        },
                    ))
            },
        ))
        .child(when(
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
        .child(when(
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
/// A back button's label and action.
type Back = (&'static str, Box<dyn Fn()>);
/// The header the app draws over each sidebar page on the desktops: a back
/// button (pages above the first), the title, and an optional trailing item.
fn page_header(
    title: impl IntoReactive<SharedString>,
    back: Option<Back>,
    disabled: impl IntoReactive<bool>,
) -> impl IntoElement {
    let disabled = disabled.into_reactive();
    let disabled = computed(move || disabled.get());
    row()
        .h(px(44.))
        .px(px(10.))
        .gap(px(6.))
        .border_b_1()
        .border_color(rgb(LINE))
        .bg(rgb(PANEL))
        .when_some(back, |v, (label, back)| {
            v.child(
                row()
                    .gap(px(2.))
                    .pl(px(2.))
                    .pr(px(8.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(rgb(0x1d1d21)))
                    .role(AccessibilityRole::Button)
                    .label(label)
                    .tooltip(label)
                    .disabled(move || disabled.get())
                    .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0_f32 })
                    .on_press(move |_: &()| back())
                    .child(icon("‹"))
                    .child(caption("Back", 11., MUTED)),
            )
        })
        .child(
            caption(title, 12., INK)
                .font_weight(FontWeight::SEMIBOLD)
                .flex_1()
                .number_of_lines(1),
        )
}
fn sidebar(s: State) -> impl IntoElement {
    // The stack follows state: a selection other than the folder shown pushes
    // its details, a review pushes over both. Going back clears that state.
    let details = computed(move || {
        let selection = s.selection.get();
        !selection.path.is_empty() && selection.path != s.path.get()
    });
    let contents = move || s.selection.set(s.listing.get_untracked().node);
    view()
        .w(px(284.))
        .flex_shrink(0.)
        .bg(rgb(PANEL))
        .border_l_1()
        .border_color(rgb(LINE))
        .child(
            nav_stack()
                .flex_1()
                .child(
                    nav_page()
                        .key("contents")
                        .title(move || folder_name(&s.listing.get().node))
                        .header(page_header(
                            move || folder_name(&s.listing.get().node),
                            None,
                            false,
                        ))
                        .child(contents_page(s)),
                )
                .child(when(
                    move || details.get(),
                    move || {
                        nav_page()
                            .key("details")
                            .title(move || s.selection.get().name)
                            .on_dismiss(contents)
                            .header(page_header(
                                move || s.selection.get().name,
                                Some(("Back to contents", Box::new(contents))),
                                false,
                            ))
                            .child(details_page(s))
                    },
                ))
                .child(when(
                    move || s.review.get(),
                    move || {
                        nav_page()
                            .key("review")
                            .title(move || review_title(s))
                            .pop_on_back(move || !s.busy.get())
                            .on_dismiss(move || s.review.set(false))
                            .header(page_header(
                                move || review_title(s),
                                Some(("Back to explorer", Box::new(move || s.close()))),
                                move || s.busy.get(),
                            ))
                            .child(review_page(s))
                    },
                )),
        )
        .child(when(
            move || !s.review.get() && !s.marked.get().is_empty(),
            move || marked_bar(s),
        ))
        .child(disk_space(s))
}
fn folder_name(node: &Node) -> String {
    if node.path.is_empty() {
        "Contents".into()
    } else if node.name.is_empty() {
        node.path.clone()
    } else {
        node.name.clone()
    }
}
fn review_title(s: State) -> &'static str {
    match s.removal.get() {
        Removal::Empty => "Empty Trash",
        Removal::Trash => "Move to Trash",
        Removal::Delete => "Delete permanently",
    }
}
fn marked_bar(s: State) -> impl IntoElement {
    row()
        .p(px(12.))
        .gap(px(10.))
        .border_t_1()
        .border_color(rgb(LINE))
        .bg(rgb(0x17150f))
        .child(
            view()
                .flex_1()
                .gap(px(3.))
                .child(caption(
                    move || format!("{} MARKED", s.marked.get().len()),
                    10.,
                    0xd29123,
                ))
                .child(caption(
                    move || format_size(s.marked.get().iter().map(|n| n.size).sum::<u64>() as f64),
                    15.,
                    INK,
                )),
        )
        .child(
            button(
                "Review marked items →",
                move || s.locked(),
                move || s.begin_review(Removal::Trash),
            )
            .py(px(8.))
            .px(px(10.)),
        )
}
fn disk_space(s: State) -> impl IntoElement {
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
        .child(meter(
            move || {
                let snapshot = s.snapshot.get();
                if snapshot.total > 0 {
                    1. - snapshot.free as f32 / snapshot.total as f32
                } else {
                    0.
                }
            },
            5.,
            0x56b675,
        ))
        .child(
            row()
                .justify_between()
                .child(caption(
                    move || {
                        let snapshot = s.snapshot.get();
                        format!(
                            "{} used",
                            format_size(snapshot.total.saturating_sub(snapshot.free) as f64)
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
        )
}
/// The folder shown, summarized above its contents, so the first page says
/// something before anything is selected.
fn folder_summary(s: State) -> impl IntoElement {
    let folder = move || s.listing.get().node;
    view()
        .gap(px(10.))
        .pb(px(12.))
        .border_b_1()
        .border_color(rgb(LINE))
        .child(caption(move || folder().path, 11., MUTED).number_of_lines(2))
        .child(caption(
            move || format_size(s.listing_metric.get().weight(&folder()) as f64),
            30.,
            INK,
        ))
        .child(meter(
            move || folder().size as f32 / s.snapshot.get().root.size.max(1) as f32,
            4.,
            0x56b675,
        ))
        .child(
            row()
                .gap(px(16.))
                .child(stat("OF SCAN", move || {
                    format!(
                        "{:.1}%",
                        100. * folder().size as f64 / s.snapshot.get().root.size.max(1) as f64
                    )
                }))
                .child(stat("FILES", move || count(folder().files))),
        )
        .child(
            row()
                .gap(px(16.))
                .child(stat("MODIFIED", move || modified(folder().modified)))
                .child(stat("LARGEST", move || {
                    s.listing
                        .get()
                        .children
                        .first()
                        .map_or_else(|| "—".to_owned(), |n| n.name.clone())
                })),
        )
        .child(caption(
            "Select a tile or row for details · right-click for actions",
            10.,
            MUTED,
        ))
}
/// What the list says when it has no rows: scanning, nothing scanned yet,
/// no names matching the filter, or an empty folder.
fn empty_contents(s: State) -> impl IntoElement {
    let scanning = move || s.busy.get() && s.listing.get().node.path.is_empty();
    view()
        .py(px(24.))
        .items_center()
        .gap(px(6.))
        .child(caption(
            move || {
                if scanning() {
                    "Scanning…".to_owned()
                } else if s.listing.get().node.path.is_empty() {
                    "Nothing scanned yet".to_owned()
                } else if !s.query.get().is_empty() {
                    format!("No names match “{}”", s.query.get())
                } else {
                    "This folder is empty".to_owned()
                }
            },
            12.,
            INK,
        ))
        .child(
            caption(
                move || {
                    if scanning() {
                        let p = s.progress.get().unwrap_or_default();
                        format!(
                            "{} folders · {} files · {} found",
                            count(p.directories),
                            count(p.files),
                            format_size(p.bytes as f64)
                        )
                    } else if s.listing.get().node.path.is_empty() {
                        "Scan a folder with ⟳ or edit its path with ✎.".to_owned()
                    } else if !s.query.get().is_empty() {
                        "Clear the filter to see every item.".to_owned()
                    } else if !s.hidden.get() {
                        "Hidden files are not shown; see View options.".to_owned()
                    } else {
                        String::new()
                    }
                },
                10.,
                MUTED,
            )
            .text_center(),
        )
}
fn contents_page(s: State) -> impl IntoElement {
    scroll().flex_1().child(
        view()
            .p(px(14.))
            .gap(px(10.))
            .child(when(
                move || !s.listing.get().node.path.is_empty(),
                move || folder_summary(s),
            ))
            .child(
                row()
                    .justify_between()
                    .child(caption(
                        move || format!("{} ITEMS", count(s.listing.get().count as u64)),
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
            .child(when(
                move || s.listing.get().count > 80,
                move || {
                    row()
                        .justify_between()
                        .child(
                            button(
                                "‹ Previous",
                                move || s.locked() || s.page.get() == 0,
                                move || s.page.update(|p| *p = p.saturating_sub(80)),
                            )
                            .py(px(4.))
                            .px(px(8.)),
                        )
                        .child(
                            button(
                                "Next ›",
                                move || {
                                    let v = s.listing.get();
                                    s.locked() || v.offset + v.page_size >= v.count
                                },
                                move || s.page.update(|p| *p += 80),
                            )
                            .py(px(4.))
                            .px(px(8.)),
                        )
                },
            ))
            .child(node_menu(
                s,
                view().child(each(
                    move || s.listing.get().children,
                    |node| node.path.clone(),
                    move |node, _| directory_row(s, node),
                )),
            ))
            .child(when(
                move || s.listing.get().children.is_empty(),
                move || empty_contents(s),
            )),
    )
}
fn details_page(s: State) -> impl IntoElement {
    scroll().flex_1().child(
        view()
            .p(px(16.))
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
                        caption(move || s.selection.get().name, 20., INK)
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
            .child(meter(
                move || s.selection.get().size as f32 / s.snapshot.get().root.size.max(1) as f32,
                4.,
                0x56b675,
            ))
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
                        move || {
                            let marked = s
                                .marked
                                .get()
                                .iter()
                                .any(|n| n.path == s.selection.get().path);
                            if marked { "Unmark" } else { "Mark" }
                        },
                        move || s.locked(),
                        move || s.mark(s.selection.get_untracked()),
                    )),
            ),
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
    let departing = node.get_untracked();
    cleanup(move || s.hover(departing, false));
    view()
        .py(px(7.))
        .px(px(6.))
        .gap(px(5.))
        .rounded(px(4.))
        .bg(move || {
            rgba(if s.selection.get().path == node.get().path {
                0x1b2a20ff
            } else {
                0
            })
        })
        .hover(|s| s.bg(rgb(0x1a1a1e)))
        .role(AccessibilityRole::Button)
        .label(move || format!("Select {}", node.get().name))
        .on_press(move |_: &()| s.choose(node.get_untracked()))
        .on_hover_in(move |_: &()| s.hover(node.get_untracked(), true))
        .on_hover_out(move |_: &()| s.hover(node.get_untracked(), false))
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
        .child(meter(
            move || {
                s.listing_metric.get().weight(&node.get()) as f32
                    / s.listing_metric.get().weight(&s.listing.get().node).max(1) as f32
            },
            2.,
            0x3d6b4c,
        ))
}
fn removal_progress(s: State) -> impl IntoElement {
    let progress = move || s.removing.get().unwrap_or_default();
    view()
        .test_id("removal-progress")
        .gap(px(8.))
        .p(px(12.))
        .rounded(px(7.))
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(0x141416))
        .child(
            row()
                .justify_between()
                .child(caption(
                    move || match s.removal.get() {
                        Removal::Trash => "Moving to Trash",
                        Removal::Delete => "Deleting",
                        Removal::Empty => "Emptying Trash",
                    },
                    11.,
                    INK,
                ))
                .child(caption(
                    move || {
                        if s.removal.get() == Removal::Empty {
                            String::new()
                        } else {
                            format!("{}%", progress().percent)
                        }
                    },
                    11.,
                    INK,
                )),
        )
        .child(meter(
            move || {
                if s.removal.get() == Removal::Empty {
                    1.
                } else {
                    progress().percent as f32 / 100.
                }
            },
            4.,
            RED,
        ))
        .child(when(
            move || s.removal.get() != Removal::Empty,
            move || {
                view()
                    .gap(px(5.))
                    .child(caption(
                        move || {
                            let p = progress();
                            format!(
                                "{} of {} items · {} of {} files",
                                p.items,
                                p.total_items,
                                count(p.files),
                                count(p.total_files)
                            )
                        },
                        10.,
                        MUTED,
                    ))
                    .child(caption(
                        move || {
                            let p = progress();
                            format!(
                                "{} of {}",
                                format_size(p.bytes as f64),
                                format_size(p.total_bytes as f64)
                            )
                        },
                        10.,
                        MUTED,
                    ))
                    .child(caption(move || progress().current, 10., MUTED).number_of_lines(1))
            },
        ))
}
fn review_page(s: State) -> impl IntoElement {
    scroll().flex_1().child(
        view()
            .p(px(16.))
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
            .child(when(
                move || s.removing.get().is_some(),
                move || removal_progress(s),
            ))
            .child(each(
                move || {
                    if s.removal.get() == Removal::Empty {
                        Vec::new()
                    } else {
                        s.reviewed.get()
                    }
                },
                |n| n.path.clone(),
                move |node, _| {
                    row()
                        .gap(px(8.))
                        .py(px(6.))
                        .border_b_1()
                        .border_color(rgb(0x242428))
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.))
                                .gap(px(3.))
                                .child(
                                    caption(move || node.get().name, 11., INK).number_of_lines(1),
                                )
                                .child(caption(
                                    move || format_size(node.get().size as f64),
                                    10.,
                                    MUTED,
                                )),
                        )
                        .child(
                            button(
                                "Exclude",
                                move || s.busy.get(),
                                move || {
                                    s.reviewed.update(|nodes| {
                                        nodes.retain(|n| n.path != node.get_untracked().path)
                                    })
                                },
                            )
                            .label(move || format!("Exclude {}", node.get().path))
                            .py(px(5.))
                            .px(px(8.)),
                        )
                },
            ))
            .child(
                colored_control(
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
                    None,
                    move || {
                        s.locked()
                            || s.removal.get() != Removal::Empty && s.reviewed.get().is_empty()
                    },
                    move || {
                        if s.removal.get() == Removal::Trash {
                            INK
                        } else {
                            RED
                        }
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
            .child(when(
                move || s.removal.get() == Removal::Trash,
                move || {
                    colored_control(
                        "Delete permanently",
                        None,
                        move || s.locked() || s.reviewed.get().is_empty(),
                        RED,
                        move || s.removal.set(Removal::Delete),
                    )
                    .bg(rgb(0x38201e))
                    .border_color(rgb(0xa65a50))
                },
            )),
    )
}
fn statusbar(s: State) -> impl IntoElement {
    row()
        .test_id("statusbar")
        .h(px(28.))
        .flex_shrink(0.)
        .gap(px(16.))
        .px(px(12.))
        .border_t_1()
        .border_color(rgb(LINE))
        .bg(rgb(PANEL))
        .child(when(
            move || s.progress.get().is_some(),
            move || {
                row()
                    .test_id("scan-progress")
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(12.))
                    .child(caption(
                        move || {
                            format!(
                                "Scanning folders · ~{}%",
                                s.progress.get().unwrap_or_default().percent
                            )
                        },
                        10.,
                        GREEN,
                    ))
                    .child(
                        caption(
                            move || {
                                let p = s.progress.get().unwrap_or_default();
                                format!(
                                    "{} folders · {} files · {} found · {}s elapsed{}",
                                    count(p.directories),
                                    count(p.files),
                                    format_size(p.bytes as f64),
                                    s.seconds.get(),
                                    if p.errors > 0 {
                                        format!(" · {} skipped", p.errors)
                                    } else {
                                        String::new()
                                    }
                                )
                            },
                            10.,
                            MUTED,
                        )
                        .number_of_lines(1)
                        .flex_1(),
                    )
            },
        ))
        .child(when(
            move || s.removing.get().is_some(),
            move || {
                caption(
                    move || {
                        let p = s.removing.get().unwrap_or_default();
                        match s.removal.get() {
                            Removal::Empty => "Emptying Trash…".to_owned(),
                            mode => format!(
                                "{} · {}% · {} of {} files · {} of {}",
                                if mode == Removal::Trash {
                                    "Moving to Trash"
                                } else {
                                    "Deleting"
                                },
                                p.percent,
                                count(p.files),
                                count(p.total_files),
                                format_size(p.bytes as f64),
                                format_size(p.total_bytes as f64)
                            ),
                        }
                    },
                    10.,
                    RED,
                )
                .number_of_lines(1)
                .flex_1()
            },
        ))
        .child(when(
            move || s.progress.get().is_none() && s.removing.get().is_none(),
            move || {
                row()
                    .flex_1()
                    .gap(px(16.))
                    .child(caption(
                        "Pinch / ⌘± Zoom   ⌘0 Fit   Click selected Enter   Space Mark   Right-click Menu   ⌥↑ Parent   ⌘R Rescan   Esc Back",
                        10.,
                        MUTED,
                    ))
                    .child(
                        caption(move || s.message.get(), 10., MUTED)
                            .number_of_lines(1)
                            .flex_1()
                            .text_right(),
                    )
            },
        ))
}
