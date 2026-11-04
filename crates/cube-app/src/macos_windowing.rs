unsafe extern "C" {
    fn con_install_window_cycle_shortcuts();
    fn con_cycle_app_window(reverse: bool);
}

pub fn install_window_cycle_shortcuts() {
    unsafe { con_install_window_cycle_shortcuts() };
}

pub fn cycle_app_window(reverse: bool) {
    unsafe { con_cycle_app_window(reverse) };
}

/// AppKit can leave a window mostly off-screen after a display swap when it
/// still intersects a remaining screen. Recheck placement after the change.
pub fn install_offscreen_window_recovery(cx: &mut gpui::App) {
    recovery::install(cx);
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn area(&self) -> f64 {
        self.width.max(0.0) * self.height.max(0.0)
    }

    fn intersection_area(&self, other: &Rect) -> f64 {
        let width = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let height = (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
        width.max(0.0) * height.max(0.0)
    }
}

fn visible_area(frame: Rect, screens: &[Rect]) -> f64 {
    // Sweep clipped screen rectangles so mirrored/overlapping displays do not
    // count the same visible part of a window more than once.
    let clips: Vec<_> = screens
        .iter()
        .filter_map(|screen| {
            let left = frame.x.max(screen.x);
            let right = (frame.x + frame.width).min(screen.x + screen.width);
            let bottom = frame.y.max(screen.y);
            let top = (frame.y + frame.height).min(screen.y + screen.height);
            (left < right && bottom < top).then_some((left, right, bottom, top))
        })
        .collect();
    let mut edges: Vec<_> = clips.iter().flat_map(|&(l, r, _, _)| [l, r]).collect();
    edges.sort_by(f64::total_cmp);
    edges.dedup();
    edges
        .windows(2)
        .map(|edge| {
            let mut ranges: Vec<_> = clips
                .iter()
                .filter(|&&(l, r, _, _)| l < edge[1] && r > edge[0])
                .map(|&(_, _, bottom, top)| (bottom, top))
                .collect();
            ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut height = 0.0;
            let mut end = f64::NEG_INFINITY;
            for (bottom, top) in ranges {
                height += (top - bottom.max(end)).max(0.0);
                end = end.max(top);
            }
            (edge[1] - edge[0]) * height
        })
        .sum()
}

fn recovered_frame(frame: Rect, screens: &[Rect]) -> Option<Rect> {
    let area = frame.area();
    if area <= 0.0 {
        return None;
    }
    if visible_area(frame, screens) >= area * 0.5 {
        return None;
    }
    // NSScreen.screens lists the menu-bar screen first; prefer it on ties.
    let screen = screens.iter().rev().max_by(|a, b| {
        frame
            .intersection_area(a)
            .total_cmp(&frame.intersection_area(b))
    })?;

    let width = frame.width.min(screen.width);
    let height = frame.height.min(screen.height);
    Some(Rect {
        x: frame.x.clamp(screen.x, screen.x + screen.width - width),
        y: frame.y.clamp(screen.y, screen.y + screen.height - height),
        width,
        height,
    })
}

mod recovery {
    use std::{ffi::c_void, time::Duration};

    use cocoa::foundation::{NSPoint, NSRect, NSSize};
    use futures::{FutureExt as _, StreamExt as _};
    use gpui::{App, BackgroundExecutor};
    use objc::{
        class,
        declare::ClassDecl,
        msg_send,
        runtime::{BOOL, Class, NO, Object, Sel, YES},
        sel, sel_impl,
    };

    use super::{Rect, recovered_frame};

    const NS_WINDOW_STYLE_MASK_TITLED: usize = 1 << 0;
    const NS_WINDOW_STYLE_MASK_FULL_SCREEN: usize = 1 << 14;
    // Let AppKit finish its own relocation pass before inspecting frames.
    const SETTLE_DELAY: Duration = Duration::from_millis(300);

    type Sender = futures::channel::mpsc::UnboundedSender<()>;

    pub(super) async fn next_settled_change(
        rx: &mut futures::channel::mpsc::UnboundedReceiver<()>,
        executor: &BackgroundExecutor,
    ) -> bool {
        if rx.next().await.is_none() {
            return false;
        }
        loop {
            let delay = executor.timer(SETTLE_DELAY).fuse();
            futures::pin_mut!(delay);
            // A later notification restarts the quiet period, including when
            // it arrives just as the previous timer expires.
            futures::select_biased! {
                next = rx.next().fuse() => {
                    if next.is_none() {
                        return false;
                    }
                },
                _ = delay => return true,
            }
        }
    }

    extern "C" fn changed(this: &Object, _: Sel, _: *mut Object) {
        unsafe {
            let sender = *this.get_ivar::<*mut c_void>("sender") as *mut Sender;
            let _ = (&*sender).unbounded_send(());
        }
    }

    fn observer_class() -> &'static Class {
        static CLASS: std::sync::OnceLock<&'static Class> = std::sync::OnceLock::new();
        CLASS.get_or_init(|| {
            let mut decl = ClassDecl::new("ConScreenParametersObserver", class!(NSObject)).unwrap();
            decl.add_ivar::<*mut c_void>("sender");
            unsafe {
                decl.add_method(
                    sel!(screenParametersChanged:),
                    changed as extern "C" fn(&Object, Sel, *mut Object),
                );
            }
            decl.register()
        })
    }

    fn to_rect(rect: NSRect) -> Rect {
        Rect {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        }
    }

    fn recover_windows() {
        unsafe {
            let screens: *mut Object = msg_send![class!(NSScreen), screens];
            let screen_count: usize = msg_send![screens, count];
            let visible_frames: Vec<Rect> = (0..screen_count)
                .map(|index| {
                    let screen: *mut Object = msg_send![screens, objectAtIndex: index];
                    let frame: NSRect = msg_send![screen, visibleFrame];
                    to_rect(frame)
                })
                .collect();

            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let windows: *mut Object = msg_send![app, windows];
            let window_count: usize = msg_send![windows, count];
            for index in 0..window_count {
                let window: *mut Object = msg_send![windows, objectAtIndex: index];
                let visible: BOOL = msg_send![window, isVisible];
                let miniaturized: BOOL = msg_send![window, isMiniaturized];
                let style_mask: usize = msg_send![window, styleMask];
                // Borderless windows (the quick terminal) manage their own frame.
                if visible == NO
                    || miniaturized == YES
                    || style_mask & NS_WINDOW_STYLE_MASK_TITLED == 0
                    || style_mask & NS_WINDOW_STYLE_MASK_FULL_SCREEN != 0
                {
                    continue;
                }

                let frame: NSRect = msg_send![window, frame];
                if let Some(target) = recovered_frame(to_rect(frame), &visible_frames) {
                    let target = NSRect::new(
                        NSPoint::new(target.x, target.y),
                        NSSize::new(target.width, target.height),
                    );
                    let _: () = msg_send![window, setFrame: target display: YES];
                }
            }
        }
    }

    pub(super) fn install(cx: &mut App) {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
        cx.spawn(async move |cx| {
            while next_settled_change(&mut rx, cx.background_executor()).await {
                cx.update(|_| recover_windows());
            }
        })
        .detach();

        unsafe {
            // Lives for the whole app; the notification center keeps no strong reference.
            let observer: *mut Object = msg_send![observer_class(), new];
            (*observer).set_ivar("sender", Box::into_raw(Box::new(tx)) as *mut c_void);
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            let name: *mut Object = msg_send![class!(NSString), stringWithUTF8String:c"NSApplicationDidChangeScreenParametersNotification".as_ptr()];
            let _: () = msg_send![center, addObserver:observer selector:sel!(screenParametersChanged:) name:name object:std::ptr::null::<Object>()];
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc, time::Duration};

    use super::{Rect, recovered_frame};

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn keeps_window_that_is_mostly_visible() {
        let screens = [rect(0.0, 0.0, 1512.0, 945.0)];
        assert_eq!(
            recovered_frame(rect(-200.0, 100.0, 1200.0, 800.0), &screens),
            None
        );
    }

    #[test]
    fn keeps_window_spanning_two_screens() {
        let screens = [
            rect(0.0, 0.0, 1512.0, 945.0),
            rect(1512.0, 0.0, 1920.0, 1080.0),
        ];
        assert_eq!(
            recovered_frame(rect(912.0, 100.0, 1200.0, 800.0), &screens),
            None
        );
    }

    #[test]
    fn pulls_sliver_window_back_onto_its_screen() {
        // Only the right 80pt remain on the laptop after the external display changed.
        let screens = [rect(0.0, 0.0, 1512.0, 945.0)];
        let frame = rect(-1120.0, 50.0, 1200.0, 800.0);
        assert_eq!(
            recovered_frame(frame, &screens),
            Some(rect(0.0, 50.0, 1200.0, 800.0))
        );
    }

    #[test]
    fn moves_window_to_screen_with_largest_overlap_and_fits_it() {
        let screens = [
            rect(0.0, 0.0, 1512.0, 945.0),
            rect(1512.0, 0.0, 1280.0, 720.0),
        ];
        let frame = rect(2700.0, 600.0, 1600.0, 900.0);
        assert_eq!(
            recovered_frame(frame, &screens),
            Some(rect(1512.0, 0.0, 1280.0, 720.0))
        );
    }

    #[test]
    fn moves_fully_detached_window_to_the_menu_bar_screen() {
        let screens = [
            rect(0.0, 0.0, 1512.0, 945.0),
            rect(-1920.0, 0.0, 1920.0, 1080.0),
        ];
        let frame = rect(-3000.0, 2000.0, 1200.0, 800.0);
        assert_eq!(
            recovered_frame(frame, &screens),
            Some(rect(0.0, 145.0, 1200.0, 800.0))
        );
    }

    #[test]
    fn leaves_frames_alone_without_usable_geometry() {
        let screen = rect(0.0, 0.0, 1512.0, 945.0);
        assert_eq!(
            recovered_frame(rect(-2000.0, 0.0, 1200.0, 800.0), &[]),
            None
        );
        assert_eq!(recovered_frame(rect(0.0, 0.0, 0.0, 800.0), &[screen]), None);
        assert_eq!(
            recovered_frame(rect(0.0, 0.0, 1200.0, 0.0), &[screen]),
            None
        );
    }

    #[test]
    fn keeps_window_at_exactly_half_visibility() {
        assert_eq!(
            recovered_frame(
                rect(-600.0, 0.0, 1200.0, 800.0),
                &[rect(0.0, 0.0, 1512.0, 945.0)],
            ),
            None
        );
    }

    #[test]
    fn fits_window_on_screen_with_negative_coordinates() {
        assert_eq!(
            recovered_frame(
                rect(-2000.0, -300.0, 1200.0, 800.0),
                &[
                    rect(0.0, 0.0, 1512.0, 945.0),
                    rect(-1280.0, -200.0, 1280.0, 720.0),
                ],
            ),
            Some(rect(-1280.0, -200.0, 1200.0, 720.0))
        );
    }

    #[test]
    fn overlapping_screens_do_not_make_an_offscreen_window_look_visible() {
        let screen = rect(0.0, 0.0, 1512.0, 945.0);
        let frame = rect(-800.0, 0.0, 1200.0, 800.0);
        assert_eq!(
            recovered_frame(frame, &[screen, screen]),
            Some(rect(0.0, 0.0, 1200.0, 800.0))
        );
        assert_eq!(
            super::visible_area(
                rect(0.0, 0.0, 100.0, 100.0),
                &[rect(0.0, 0.0, 60.0, 60.0), rect(40.0, 40.0, 60.0, 60.0),],
            ),
            6800.0
        );
    }

    #[gpui::test]
    fn recovery_waits_for_the_last_display_notification(cx: &mut gpui::TestAppContext) {
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let recoveries = Rc::new(Cell::new(0));
        let result = recoveries.clone();
        let executor = cx.executor();
        let _task = cx.spawn(async move |_| {
            while super::recovery::next_settled_change(&mut rx, &executor).await {
                result.set(result.get() + 1);
            }
        });
        tx.unbounded_send(()).unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(250));
        tx.unbounded_send(()).unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        assert_eq!(recoveries.get(), 0);
        cx.executor().advance_clock(Duration::from_millis(50));
        cx.run_until_parked();
        assert_eq!(recoveries.get(), 1);
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert_eq!(recoveries.get(), 1);
        tx.unbounded_send(()).unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        assert_eq!(recoveries.get(), 2);
    }

    #[gpui::test]
    fn closing_notification_stream_cancels_pending_recovery(cx: &mut gpui::TestAppContext) {
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let result = Rc::new(Cell::new(None));
        let completed = result.clone();
        let executor = cx.executor();
        let _task = cx.spawn(async move |_| {
            completed.set(Some(
                super::recovery::next_settled_change(&mut rx, &executor).await,
            ));
        });
        tx.unbounded_send(()).unwrap();
        cx.run_until_parked();
        drop(tx);
        cx.run_until_parked();
        assert_eq!(result.get(), Some(false));
    }
}
