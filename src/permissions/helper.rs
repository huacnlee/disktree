//! Floating drag helper for System Settings, ported from tt v1
//! `tt-permission-flow`. A non-activating panel docks in the bottom-right corner
//! of the System Settings window and offers the running app bundle as a file
//! drag, so the user can drop it into the Full Disk Access list.

use std::{cell::RefCell, path::PathBuf, ptr::NonNull, time::Instant};

use block2::RcBlock;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSColor, NSDragOperation, NSDraggingContext, NSDraggingItem,
    NSDraggingSession, NSDraggingSource, NSEvent, NSFloatingWindowLevel, NSFont, NSImage,
    NSImageScaling, NSImageView, NSPanel, NSRunningApplication, NSScreen, NSTextField, NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{
    NSArray, NSDictionary, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSRunLoop,
    NSRunLoopCommonModes, NSSize, NSString, NSTimer, NSURL,
};

const WIDTH: f64 = 380.;
const HEIGHT: f64 = 112.;
const CONTENT_X: f64 = 42.;
const ROW: NSRect = NSRect::new(
    NSPoint::new(CONTENT_X, 14.),
    NSSize::new(WIDTH - CONTENT_X - 18., 44.),
);
const ARROW: NSRect = NSRect::new(
    NSPoint::new(CONTENT_X + 4., HEIGHT - 40.),
    NSSize::new(20., 20.),
);

pub struct RowIvars {
    bundle: PathBuf,
}

define_class!(
    // SAFETY: NSView has no subclass requirements. The class is main-thread only.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = RowIvars]
    struct DragRow;

    impl DragRow {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        // The panel moves by its background. The row must start a file drag instead.
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        // Send clicks on the icon and name to the row, not to the subviews.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            // SAFETY: Reading the superview on the main thread.
            let superview = unsafe { self.superview() };
            let local = self.convertPoint_fromView(point, superview.as_deref());
            if objc2_foundation::NSPointInRect(local, self.bounds()) {
                self as *const Self as *mut NSView
            } else {
                std::ptr::null_mut()
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.begin_drag(event);
        }

        #[unsafe(method(closeHelper:))]
        fn close_helper(&self, _sender: Option<&AnyObject>) {
            dismiss();
        }
    }

    unsafe impl NSObjectProtocol for DragRow {}

    unsafe impl NSDraggingSource for DragRow {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation(&self, _: &NSDraggingSession, context: NSDraggingContext) -> NSDragOperation {
            if context == NSDraggingContext::OutsideApplication {
                NSDragOperation::Copy | NSDragOperation::Link
            } else {
                NSDragOperation::empty()
            }
        }
    }
);

impl DragRow {
    fn new(bundle: PathBuf, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RowIvars { bundle });
        // SAFETY: NSView designated initializer.
        unsafe { msg_send![super(this), initWithFrame: ROW] }
    }

    fn begin_drag(&self, event: &NSEvent) {
        let path = NSString::from_str(&self.ivars().bundle.to_string_lossy());
        let url = NSURL::fileURLWithPath_isDirectory(&path, true);
        let item = NSDraggingItem::initWithPasteboardWriter(
            NSDraggingItem::alloc(),
            ProtocolObject::from_ref(&*url),
        );
        let Some(bitmap) = self.bitmapImageRepForCachingDisplayInRect(self.bounds()) else {
            return;
        };
        self.cacheDisplayInRect_toBitmapImageRep(self.bounds(), &bitmap);
        let preview = NSImage::initWithSize(NSImage::alloc(), self.bounds().size);
        preview.addRepresentation(&bitmap);
        // SAFETY: NSImage is a valid dragging item contents object.
        unsafe { item.setDraggingFrame_contents(self.bounds(), Some(&preview)) };
        let session = self.beginDraggingSessionWithItems_event_source(
            &NSArray::from_retained_slice(&[item]),
            event,
            ProtocolObject::from_ref(self),
        );
        session.setAnimatesToStartingPositionsOnCancelOrFail(true);
    }
}

define_class!(
    // SAFETY: NSView has no subclass requirements. The class is main-thread only.
    // Moves the panel from the card background only, so the app row can drag.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    struct CardView;

    impl CardView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(window) = self.window() {
                window.performWindowDragWithEvent(event);
            }
        }
    }

    unsafe impl NSObjectProtocol for CardView {}
);

struct Helper {
    panel: Retained<NSPanel>,
    _row: Retained<DragRow>,
    arrow: Retained<NSImageView>,
    timer: Retained<NSTimer>,
    seen: bool,
    missing: u32,
    ticks: u32,
    reduced: bool,
    /// Flight from the opening position to the Settings corner.
    source: NSPoint,
    started: Instant,
    finished: bool,
    returning: Option<(Instant, NSPoint)>,
    /// After the user moves the card, it stays where they put it.
    manual: bool,
    placed: Option<NSPoint>,
    landed: Option<Instant>,
    movement: Option<(Instant, f32)>,
    previous: Option<(Instant, f64)>,
}

thread_local! {
    static HELPER: RefCell<Option<Helper>> = const { RefCell::new(None) };
}

fn schedule(
    seconds: f64,
    repeats: bool,
    f: impl Fn(NonNull<NSTimer>) + 'static,
) -> Retained<NSTimer> {
    let block = RcBlock::new(f);
    // SAFETY: The block is 'static and runs on the main run loop.
    // SAFETY: The block is 'static. Common modes keep it running during drags.
    let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(seconds, repeats, &block) };
    // SAFETY: Called on the main thread with a live timer.
    unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
    timer
}

/// The running app bundle. Never offer another installed copy.
fn current_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_owned())
}

fn label(
    text: &str,
    size: f64,
    bold: bool,
    color: &NSColor,
    frame: NSRect,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    let font = if bold {
        NSFont::boldSystemFontOfSize(size)
    } else {
        NSFont::systemFontOfSize(size)
    };
    label.setFont(Some(&font));
    label.setTextColor(Some(color));
    label.setFrame(frame);
    label
}

fn symbol_button(
    symbol: &str,
    title: &str,
    row: &DragRow,
    frame: NSRect,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(symbol),
        Some(&NSString::from_str(title)),
    );
    // SAFETY: closeHelper: is defined on DragRow and the row outlives the button.
    let button = unsafe {
        match image {
            Some(image) => NSButton::buttonWithImage_target_action(
                &image,
                Some(row),
                Some(sel!(closeHelper:)),
                mtm,
            ),
            None => NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(row),
                Some(sel!(closeHelper:)),
                mtm,
            ),
        }
    };
    button.setBordered(false);
    button.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    button.setFrame(frame);
    button
}

pub fn show() {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("permission helper: not on the main thread");
        return;
    };
    let Some(bundle) = current_bundle() else {
        log::warn!("permission helper: not running from an app bundle");
        return;
    };
    close();

    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, HEIGHT)),
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    // SAFETY: We keep the panel retained in HELPER and close it ourselves.
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setLevel(NSFloatingWindowLevel);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );

    let content: Retained<NSView> = {
        // SAFETY: NSView designated initializer.
        let card: Retained<CardView> = unsafe {
            msg_send![CardView::alloc(mtm), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, HEIGHT))]
        };
        Retained::into_super(card)
    };
    panel.setContentView(Some(&content));
    content.setWantsLayer(true);
    if let Some(layer) = content.layer() {
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(0.11, 0.11, 0.12, 0.98);
        layer.setBackgroundColor(Some(&color.CGColor()));
        layer.setCornerRadius(22.);
        layer.setBorderWidth(1.);
        layer.setBorderColor(Some(&NSColor::colorWithWhite_alpha(1., 0.12).CGColor()));
    }

    let row = DragRow::new(bundle.clone(), mtm);

    // Back on the left, as in tt v1.
    content.addSubview(&symbol_button(
        "chevron.left",
        "Back",
        &row,
        NSRect::new(
            NSPoint::new(10., (HEIGHT - 24.) / 2.),
            NSSize::new(24., 24.),
        ),
        mtm,
    ));

    // Animated up arrow and instruction.
    let arrow = NSImageView::initWithFrame(NSImageView::alloc(mtm), ARROW);
    if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str("arrow.up"),
        None,
    ) {
        arrow.setImage(Some(&image));
    }
    arrow.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    arrow.setContentTintColor(Some(&NSColor::controlAccentColor()));
    content.addSubview(&arrow);
    let name = bundle
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    content.addSubview(&label(
        &format!("Drag {name} to the list above to allow Full Disk Access"),
        12.,
        false,
        &NSColor::labelColor(),
        NSRect::new(
            NSPoint::new(CONTENT_X + 32., HEIGHT - 44.),
            NSSize::new(WIDTH - CONTENT_X - 32. - 30., 32.),
        ),
        mtm,
    ));

    // Full-width app row.
    row.setWantsLayer(true);
    if let Some(layer) = row.layer() {
        layer.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(1., 0.08).CGColor()));
        layer.setBorderWidth(1.);
        layer.setBorderColor(Some(&NSColor::colorWithWhite_alpha(1., 0.12).CGColor()));
        layer.setCornerRadius(7.);
    }
    let path = NSString::from_str(&bundle.to_string_lossy());
    let icon = NSWorkspace::sharedWorkspace().iconForFile(&path);
    let image = NSImageView::initWithFrame(
        NSImageView::alloc(mtm),
        NSRect::new(NSPoint::new(10., 6.), NSSize::new(32., 32.)),
    );
    image.setImage(Some(&icon));
    row.addSubview(&image);
    row.addSubview(&label(
        &name,
        13.,
        true,
        &NSColor::whiteColor(),
        NSRect::new(
            NSPoint::new(52., 12.),
            NSSize::new(ROW.size.width - 62., 20.),
        ),
        mtm,
    ));
    content.addSubview(&row);

    // Close at the upper right.
    content.addSubview(&symbol_button(
        "xmark",
        "Close",
        &row,
        NSRect::new(
            NSPoint::new(WIDTH - 26., HEIGHT - 26.),
            NSSize::new(18., 18.),
        ),
        mtm,
    ));

    // Start where the pointer is, then fly to the Settings corner.
    let mouse = NSEvent::mouseLocation();
    let source = NSPoint::new(mouse.x - WIDTH / 2., mouse.y - HEIGHT / 2.);
    panel.setFrameOrigin(source);
    panel.orderFrontRegardless();

    let timer = schedule(1. / 30., true, |_| tick());
    HELPER.with_borrow_mut(|helper| {
        *helper = Some(Helper {
            panel,
            _row: row,
            arrow,
            timer,
            seen: false,
            missing: 0,
            ticks: 0,
            reduced: NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion(),
            source,
            started: Instant::now(),
            finished: false,
            returning: None,
            manual: false,
            placed: None,
            landed: None,
            movement: None,
            previous: None,
        });
    });
}

/// Fly back to the opening position, then close. Used by Back and Close.
fn dismiss() {
    let close_now = HELPER.with_borrow_mut(|helper| {
        let Some(helper) = helper else {
            return false;
        };
        if helper.reduced || !helper.finished {
            return true;
        }
        if helper.returning.is_none() {
            helper.returning = Some((Instant::now(), helper.panel.frame().origin));
        }
        false
    });
    if close_now {
        schedule(0., false, |_| close());
    }
}

pub fn close() {
    if let Some(helper) = HELPER.with_borrow_mut(Option::take) {
        helper.timer.invalidate();
        helper.panel.orderOut(None);
    }
}

fn tick() {
    let (returning, poll) = HELPER.with_borrow_mut(|helper| match helper {
        Some(helper) => {
            helper.ticks += 1;
            // Follow Settings at up to ten updates per second after the flight.
            (
                helper.returning.is_some(),
                !helper.finished || helper.ticks % 3 == 0,
            )
        }
        None => (false, false),
    });
    if returning {
        let done = HELPER.with_borrow_mut(|helper| {
            let Some(helper) = helper else { return true };
            let (started, from) = helper.returning.expect("returning");
            let t = (started.elapsed().as_secs_f64() / 0.6).min(1.);
            let (x, y) = flight_position((from.x, from.y), (helper.source.x, helper.source.y), t);
            helper.panel.setFrameOrigin(NSPoint::new(x, y));
            t >= 1.
        });
        if done {
            close();
        }
        return;
    }
    let granted = poll && super::full_disk_access() == Some(true);
    let frame = if poll { settings_frame() } else { None };
    let done = HELPER.with_borrow_mut(|helper| {
        let Some(helper) = helper else {
            return false;
        };
        if granted {
            return true;
        }
        // A moved card stays where the user put it.
        let own = helper.panel.frame().origin;
        if helper.finished
            && helper
                .placed
                .is_some_and(|p| (p.x - own.x).abs() > 1. || (p.y - own.y).abs() > 1.)
        {
            helper.manual = true;
            helper.landed.get_or_insert_with(Instant::now);
        }
        if poll {
            match frame {
                Some(frame) => {
                    helper.seen = true;
                    helper.missing = 0;
                    if helper.finished {
                        helper.landed.get_or_insert_with(Instant::now);
                        let now = Instant::now();
                        if let Some((last, y)) = helper.previous {
                            let velocity = ((y - frame.origin.y)
                                / now.duration_since(last).as_secs_f64().max(0.001))
                                as f32;
                            if !helper.manual && velocity.abs() > 1. {
                                helper.movement = Some((now, -(velocity / 2200.).clamp(-1., 1.)));
                            }
                        }
                        helper.previous = Some((now, frame.origin.y));
                    }
                    if !helper.manual {
                        place(helper, frame);
                    }
                }
                // Close two seconds after Settings closes, or if it never opens.
                None if helper.seen => helper.missing += 3,
                None => {}
            }
        }
        animate_arrow(helper);
        helper.missing >= 60 || (!helper.seen && helper.ticks >= 300)
    });
    if done {
        close();
    }
}

fn animate_arrow(helper: &Helper) {
    let now = Instant::now();
    let entrance = if helper.reduced {
        1.
    } else {
        helper.landed.map_or(0.2, |start| {
            entrance_scale(now.duration_since(start).as_secs_f32())
        })
    };
    let recoil = if helper.reduced {
        1.
    } else {
        helper.movement.map_or(1., |(start, amount)| {
            recoil_scale(now.duration_since(start).as_secs_f32(), amount)
        })
    };
    let width = ARROW.size.width * f64::from(entrance);
    let height = ARROW.size.height * f64::from(entrance * recoil);
    let center_x = ARROW.origin.x + ARROW.size.width / 2.;
    // Scale from the bottom so the arrow springs up.
    let frame = NSRect::new(
        NSPoint::new(center_x - width / 2., ARROW.origin.y),
        NSSize::new(width, height),
    );
    if helper.arrow.frame() != frame {
        helper.arrow.setFrame(frame);
    }
}

/// System Settings main window bounds in top-left screen coordinates.
/// Reads window metadata only. It does not capture pixels or need a grant.
fn settings_frame() -> Option<NSRect> {
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        "com.apple.systempreferences",
    ));
    let pid = f64::from(apps.firstObject()?.processIdentifier());
    let list = CGWindowListCopyWindowInfo(
        // Settings can be on another Space, where it is not "on screen".
        CGWindowListOption::OptionAll | CGWindowListOption::ExcludeDesktopElements,
        0,
    )?;
    // SAFETY: CFArray is toll-free bridged to NSArray, and the list stays alive.
    let windows: &NSArray = unsafe { &*(&*list as *const _ as *const NSArray) };
    let number = |dict: &NSDictionary, key: &str| -> Option<f64> {
        let key = NSString::from_str(key);
        let key: &AnyObject = &key;
        Some(
            dict.objectForKey(key)?
                .downcast_ref::<NSNumber>()?
                .doubleValue(),
        )
    };
    // Settings also owns small layer-0 menu bar windows. Take the largest one.
    windows
        .iter()
        .filter_map(|entry| {
            let dict = entry.downcast_ref::<NSDictionary>()?;
            if number(dict, "kCGWindowOwnerPID")? != pid || number(dict, "kCGWindowLayer")? != 0. {
                return None;
            }
            let key = NSString::from_str("kCGWindowBounds");
            let key: &AnyObject = &key;
            let bounds = dict.objectForKey(key)?;
            let bounds = bounds.downcast_ref::<NSDictionary>()?;
            let rect = NSRect::new(
                NSPoint::new(number(bounds, "X")?, number(bounds, "Y")?),
                NSSize::new(number(bounds, "Width")?, number(bounds, "Height")?),
            );
            (rect.size.width > 200. && rect.size.height > 200.).then_some(rect)
        })
        .max_by(|a, b| (a.size.width * a.size.height).total_cmp(&(b.size.width * b.size.height)))
}

/// Dock inside the bottom-right corner of Settings, as tt v1 does, flying there
/// on a parabolic arc the first time.
fn place(helper: &mut Helper, settings: NSRect) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let screens = NSScreen::screens(mtm);
    let Some(primary) = screens.firstObject() else {
        return;
    };
    let primary_top = primary.frame().origin.y + primary.frame().size.height;
    let target = (
        settings.origin.x + settings.size.width - WIDTH - 16.,
        primary_top - (settings.origin.y + settings.size.height) + 16.,
    );
    let t = if helper.reduced {
        1.
    } else {
        (helper.started.elapsed().as_secs_f64() / 0.78).min(1.)
    };
    let (x, y) = flight_position((helper.source.x, helper.source.y), target, t);
    let own = helper.panel.frame().origin;
    if (own.x - x).abs() > 0.5 || (own.y - y).abs() > 0.5 {
        helper.panel.setFrameOrigin(NSPoint::new(x, y));
    }
    helper.placed = Some(NSPoint::new(x, y));
    helper.finished = t >= 1.;
}

fn flight_position(source: (f64, f64), target: (f64, f64), t: f64) -> (f64, f64) {
    let t = t.clamp(0., 1.);
    let eased = if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    };
    let u = 1. - eased;
    let control_y = source.1.max(target.1) + 160.;
    (
        source.0 + (target.0 - source.0) * eased,
        u * u * source.1 + 2. * u * eased * control_y + eased * eased * target.1,
    )
}

fn entrance_scale(t: f32) -> f32 {
    // Underdamped spring: stiffness 200, damping 11, from 0.2 with velocity 18.
    let decay = 5.5;
    let frequency = (200_f32 - decay * decay).sqrt();
    let displacement = -0.8;
    let sine = (18. + decay * displacement) / frequency;
    1. + (-decay * t).exp() * (displacement * (frequency * t).cos() + sine * (frequency * t).sin())
}

fn recoil_scale(t: f32, amount: f32) -> f32 {
    let settle = (t - 0.15).max(0.);
    let relaxation = amount * (-7. * settle).exp() * (131_f32.sqrt() * settle).cos();
    let punctuation = if (2.15..3.6).contains(&t) {
        let t = t - 2.15;
        let rise = 1. - (-6.5 * t).exp() * (177.75_f32.sqrt() * t).cos();
        let release = if t > 0.22 {
            (-7. * (t - 0.22)).exp()
        } else {
            1.
        };
        rise * release
    } else {
        0.
    };
    (1. + relaxation + punctuation).max(0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flight_reaches_both_endpoints_and_lifts_at_midpoint() {
        assert_eq!(flight_position((10., 20.), (110., 220.), 0.), (10., 20.));
        assert_eq!(flight_position((10., 20.), (110., 220.), 1.), (110., 220.));
        assert_eq!(flight_position((10., 20.), (110., 220.), 0.5), (60., 250.));
    }

    #[test]
    fn arrow_springs_start_and_settle() {
        assert!((entrance_scale(0.) - 0.2).abs() < 0.0001);
        assert!((entrance_scale(2.) - 1.).abs() < 0.001);
        assert!(recoil_scale(0., 1.) > 1.);
        assert!((recoil_scale(4., 1.) - 1.).abs() < 0.001);
    }
}
