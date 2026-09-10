use super::{EditorHostEvents, EditorHostFailure};
use crate::{
    BASE64, ProcessorStateError,
    editor_surface::{EDITOR_FRAME, PointerEvent},
    publish_state_response,
    rack_formats::{EditorSnapshot, RackWorkerPlugin},
    shared_header_unchecked,
};
use base64::Engine as _;
use block2::RcBlock;
use dispatch2::MainThreadBound;
use memmap2::MmapMut;
use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSBitmapImageFileType,
    NSEvent, NSEventMask, NSEventModifierFlags, NSEventTrackingRunLoopMode, NSEventType, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    NSDate, NSDefaultRunLoopMode, NSDictionary, NSPoint, NSRect, NSRunLoop, NSSize, NSTimer,
};
use std::{
    cell::Cell,
    ptr::NonNull,
    sync::{Arc, atomic::Ordering},
};
use tonenari_control_protocol::{EditorPointerKind, ProcessorEditorFrame};
use truce_rack_core::WindowHandle;

pub(crate) struct NativeEditorHost {
    shared: Arc<MmapMut>,
    app: Retained<NSApplication>,
    surface: Option<Surface>,
    target: Option<Retained<NSView>>,
    pub(super) pending_events: EditorHostEvents,
}

struct Surface {
    timer: Retained<NSTimer>,
    state: Arc<MainThreadBound<SurfaceState>>,
}

struct SurfaceState {
    window: Retained<NSWindow>,
    view: Retained<NSView>,
    shared: Arc<MmapMut>,
    session: u64,
    size: Cell<(u32, u32)>,
    pressed: Cell<bool>,
    point: Cell<NSPoint>,
    capturing: Cell<bool>,
}

impl NativeEditorHost {
    pub(crate) fn new(shared: Arc<MmapMut>) -> Result<Self, EditorHostFailure> {
        let main = MainThreadMarker::new().ok_or(EditorHostFailure::DisplayUnavailable)?;
        let app = NSApplication::sharedApplication(main);
        app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
        app.finishLaunching();
        // AppKit initializes its coordinate conversion on the first event read.
        for _ in 0..32 {
            let Some(event) = next_event(&app, NSEventMask::Any) else {
                break;
            };
            app.sendEvent(&event);
        }
        Ok(Self {
            shared,
            app,
            surface: None,
            target: None,
            pending_events: EditorHostEvents::default(),
        })
    }

    pub(crate) fn open(
        &mut self,
        plugin: &mut RackWorkerPlugin,
        _title: &str,
    ) -> Result<EditorSnapshot, EditorHostFailure> {
        let main = MainThreadMarker::new().ok_or(EditorHostFailure::DisplayUnavailable)?;
        if let Some(surface) = &self.surface {
            if surface.state.get(main).session
                == shared_header_unchecked(&self.shared)
                    .editor_sequence
                    .load(Ordering::Acquire)
            {
                return Ok(plugin.tick_editor());
            }
            self.close(plugin);
        }
        let window = parent_window(main);
        let view = window.contentView().ok_or(EditorHostFailure::HostWindow)?;
        let result = (|| {
            let snapshot = plugin
                .open_editor(
                    WindowHandle::NSView(Retained::as_ptr(&view).cast_mut().cast()),
                    1.0,
                )
                .map_err(|_| EditorHostFailure::PluginRejected)?;
            let (width, height) = snapshot.size.ok_or(EditorHostFailure::PluginRejected)?;
            let scale = window.backingScaleFactor();
            if !valid_size((width, height), scale) {
                return Err(EditorHostFailure::PluginRejected);
            }
            window.setContentSize(NSSize::new(f64::from(width), f64::from(height)));
            let state = Arc::new(MainThreadBound::new(
                SurfaceState {
                    window: window.clone(),
                    view: view.clone(),
                    shared: Arc::clone(&self.shared),
                    session: shared_header_unchecked(&self.shared)
                        .editor_sequence
                        .load(Ordering::Acquire),
                    size: Cell::new((width, height)),
                    pressed: Cell::new(false),
                    point: Cell::new(NSPoint::new(0.0, 0.0)),
                    capturing: Cell::new(false),
                },
                main,
            ));
            self.surface = Some(Surface::new(state, main));
            Ok(snapshot)
        })();
        if result.is_err() {
            plugin.close_editor();
        }
        result
    }

    pub(crate) fn close(&mut self, plugin: &mut RackWorkerPlugin) {
        // Called only after event dispatch/tracking has unwound on this main thread.
        let surface = self.surface.take();
        if let Some(surface) = &surface {
            surface.timer.invalidate();
        }
        plugin.close_editor();
        drop(surface);
        self.target = None;
        self.pending_events = EditorHostEvents::default();
        reject_pending_frame(&self.shared);
    }

    pub(super) fn collect_events(&mut self) -> EditorHostEvents {
        let Some(surface) = &self.surface else {
            return EditorHostEvents::default();
        };
        let Some(main) = MainThreadMarker::new() else {
            return EditorHostEvents::default();
        };
        let state = surface.state.get(main);
        state.tick(main);
        for _ in 0..64 {
            let Some(event) = next_event(&self.app, NSEventMask::Any) else {
                break;
            };
            let point = event.locationInWindow();
            if matches!(
                event.r#type(),
                NSEventType::LeftMouseDown
                    | NSEventType::LeftMouseDragged
                    | NSEventType::LeftMouseUp
            ) && event.windowNumber() != state.window.windowNumber()
            {
                continue;
            }
            match event.r#type() {
                NSEventType::LeftMouseDown => {
                    self.target = state.view.hitTest(point);
                    if let Some(target) = &self.target {
                        target.mouseDown(&event);
                    }
                }
                NSEventType::LeftMouseDragged => {
                    if let Some(target) = &self.target {
                        target.mouseDragged(&event);
                    }
                }
                NSEventType::LeftMouseUp => {
                    if let Some(target) = self.target.take() {
                        target.mouseUp(&event);
                    }
                }
                _ => self.app.sendEvent(&event),
            }
        }
        EditorHostEvents::default()
    }

    pub(crate) fn refresh(&mut self, snapshot: EditorSnapshot) {
        if !snapshot.open {
            self.surface.take();
            self.target = None;
            self.pending_events = EditorHostEvents::default();
            reject_pending_frame(&self.shared);
            return;
        }
        if let (Some(surface), Some(size), Some(main)) =
            (&self.surface, snapshot.size, MainThreadMarker::new())
        {
            let state = surface.state.get(main);
            if !valid_size(size, state.window.backingScaleFactor()) {
                self.pending_events.close_requested = true;
            } else if state.size.get() != size {
                state
                    .window
                    .setContentSize(NSSize::new(f64::from(size.0), f64::from(size.1)));
                state.size.set(size);
            }
        }
    }
}

impl Surface {
    fn new(state: Arc<MainThreadBound<SurfaceState>>, main: MainThreadMarker) -> Self {
        let captured = Arc::clone(&state);
        let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            if let Some(main) = MainThreadMarker::new() {
                captured.get(main).tick(main);
            }
        });
        // SAFETY: captured state is Send through MainThreadBound; access and destruction of
        // Cocoa objects remain on main. This timer is scheduled only on the main run loop.
        let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.008, true, &block) };
        let run_loop = NSRunLoop::mainRunLoop();
        // SAFETY: main-thread scheduling; immutable process-lifetime Foundation mode names.
        unsafe {
            run_loop.addTimer_forMode(&timer, NSDefaultRunLoopMode);
            run_loop.addTimer_forMode(&timer, NSEventTrackingRunLoopMode);
        }
        let _ = state.get(main);
        Self { timer, state }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // Surface is main-thread-owned by NativeEditorHost; callbacks cannot run
        // concurrently. Invalidating releases the block before the view is detached.
        self.timer.invalidate();
    }
}

impl SurfaceState {
    fn tick(&self, main: MainThreadMarker) {
        let header = shared_header_unchecked(&self.shared);
        let closing = header.editor_sequence.load(Ordering::Acquire) != self.session
            || header.shutdown.load(Ordering::Acquire) != 0;
        let cancelled = header.editor_input.take_cancel() == self.session;
        for _ in 0..64 {
            let Some(event) = header.editor_input.pop() else {
                break;
            };
            if !closing && !cancelled && event.session == self.session {
                self.post(event, main);
            }
        }
        if (closing || cancelled) && self.pressed.replace(false) {
            self.post_release(main);
        }
        let request = header.state_request.load(Ordering::Acquire);
        if request == header.state_response.load(Ordering::Acquire)
            || header.state_kind.load(Ordering::Relaxed) != EDITOR_FRAME
        {
            return;
        }
        if self.capturing.get() {
            return;
        }
        if closing {
            reject_pending_frame(&self.shared);
            return;
        }
        if self.capturing.replace(true) {
            return;
        }
        let result = self.frame();
        self.capturing.set(false);
        publish_state_response(&self.shared, request, result);
    }

    fn post(&self, event: PointerEvent, main: MainThreadMarker) {
        let kind = match event.kind {
            EditorPointerKind::Down => {
                self.pressed.set(true);
                NSEventType::LeftMouseDown
            }
            EditorPointerKind::Move if self.pressed.get() => NSEventType::LeftMouseDragged,
            EditorPointerKind::Move => return,
            EditorPointerKind::Up | EditorPointerKind::Cancel => {
                self.pressed.set(false);
                NSEventType::LeftMouseUp
            }
        };
        self.point.set(NSPoint::new(
            event.x,
            f64::from(self.size.get().1) - event.y,
        ));
        self.post_event(kind, event.modifiers, main);
    }

    fn post_release(&self, main: MainThreadMarker) {
        self.post_event(NSEventType::LeftMouseUp, 0, main);
    }

    fn post_event(&self, kind: NSEventType, modifiers: u8, main: MainThreadMarker) {
        let mut flags = NSEventModifierFlags::empty();
        for (bit, flag) in [
            (1, NSEventModifierFlags::Shift),
            (2, NSEventModifierFlags::Control),
            (4, NSEventModifierFlags::Option),
            (8, NSEventModifierFlags::Command),
        ] {
            if modifiers & bit != 0 {
                flags |= flag;
            }
        }
        if let Some(event) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind, self.point.get(), flags, 0.0, self.window.windowNumber(), None, 0, 1, 1.0,
        ) { NSApplication::sharedApplication(main).postEvent_atStart(&event, false); }
    }

    fn frame(&self) -> Result<Vec<u8>, ProcessorStateError> {
        let bounds = self.view.bounds();
        let scale = self.window.backingScaleFactor();
        if !scale.is_finite()
            || scale <= 0.0
            || !bounds.size.width.is_finite()
            || !bounds.size.height.is_finite()
            || bounds.size.width <= 0.0
            || bounds.size.height <= 0.0
            || bounds.size.width * bounds.size.height * scale * scale > 4_000_000.0
        {
            return Err(ProcessorStateError::TooLarge);
        }
        let bitmap = self
            .view
            .bitmapImageRepForCachingDisplayInRect(bounds)
            .ok_or(ProcessorStateError::NativeFailure)?;
        self.view
            .cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
        // SAFETY: empty properties dictionary selects PNG defaults; NSData owns its bytes.
        let png = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        }
        .ok_or(ProcessorStateError::NativeFailure)?;
        if png.len() > 3_000_000 {
            return Err(ProcessorStateError::TooLarge);
        }
        serde_json::to_vec(&ProcessorEditorFrame {
            instance_id: shared_header_unchecked(&self.shared)
                .instance_id
                .to_string(),
            session: self.session,
            width: self.size.get().0,
            height: self.size.get().1,
            png_base64: BASE64.encode(png.to_vec()),
        })
        .map_err(|_| ProcessorStateError::InvalidResponse)
    }
}

fn valid_size((width, height): (u32, u32), scale: f64) -> bool {
    width > 0
        && height > 0
        && scale.is_finite()
        && scale > 0.0
        && f64::from(width) * f64::from(height) * scale * scale <= 4_000_000.0
}

fn reject_pending_frame(shared: &MmapMut) {
    let header = shared_header_unchecked(shared);
    let request = header.state_request.load(Ordering::Acquire);
    if request != header.state_response.load(Ordering::Acquire)
        && header.state_kind.load(Ordering::Relaxed) == EDITOR_FRAME
    {
        publish_state_response(shared, request, Err(ProcessorStateError::Unavailable));
    }
}

fn next_event(app: &NSApplication, mask: NSEventMask) -> Option<Retained<NSEvent>> {
    // SAFETY: immutable process-lifetime Foundation constant, read on main thread.
    let mode = unsafe { NSDefaultRunLoopMode };
    app.nextEventMatchingMask_untilDate_inMode_dequeue(
        mask,
        Some(&NSDate::distantPast()),
        mode,
        true,
    )
}

fn parent_window(main: MainThreadMarker) -> Retained<NSWindow> {
    // SAFETY: main-thread construction with a finite rectangle. The host never shows this
    // window; its retained view stays alive through editor.close and timer invalidation.
    unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(main),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(500.0, 200.0)),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    }
}
