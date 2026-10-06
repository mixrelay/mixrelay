//! VST3 editor frame ownership and the Linux editor run loop.

use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "linux")]
use std::sync::{Mutex, MutexGuard};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use vst3::Steinberg::kResultFalse;
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewTrait, ViewRect, kInvalidArgument, kResultOk,
    kResultTrue,
};
use vst3::{Class, ComPtr, ComRef, ComWrapper};

#[cfg(target_os = "linux")]
use vst3::Steinberg::Linux::{
    IEventHandler, IEventHandlerTrait, IRunLoop, IRunLoopTrait, ITimerHandler, ITimerHandlerTrait,
};

#[cfg(target_os = "linux")]
const MAX_EVENT_HANDLERS: usize = 64;
#[cfg(target_os = "linux")]
const MAX_TIMER_HANDLERS: usize = 32;

/// A retained view and frame pair. Dropping it always removes the view, clears the frame,
/// disables callbacks, and releases the host's frame reference.
pub(super) struct EditorBinding {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<EditorFrame>,
    attached_attempted: bool,
}

impl EditorBinding {
    /// Set a retained host frame before attaching `view` to `parent`.
    ///
    /// `parent` and `platform_type` are provided by the active native editor host. Returns an
    /// error when the plug-in rejects frame binding or attachment; partial state is rolled back.
    pub(super) fn attach(
        view: ComPtr<IPlugView>,
        parent: *mut std::ffi::c_void,
        platform_type: vst3::Steinberg::FIDString,
    ) -> Result<Self, &'static str> {
        let frame = ComWrapper::new(EditorFrame::new());
        frame.bind_view(view.as_ptr());
        let mut binding = Self {
            view,
            frame,
            attached_attempted: false,
        };
        let frame_ref = binding
            .frame
            .as_com_ref::<IPlugFrame>()
            .ok_or("host frame does not expose IPlugFrame")?;
        // SAFETY: the frame is retained by `binding` through detach and outlives this call.
        let frame_status = unsafe { binding.view.setFrame(frame_ref.as_ptr()) };
        if frame_status != kResultOk && frame_status != kResultTrue {
            return Err("IPlugView::setFrame failed");
        }
        binding.attached_attempted = true;
        // SAFETY: the caller supplies the live parent window and a supported platform type.
        if unsafe { binding.view.attached(parent, platform_type) } != kResultOk {
            return Err("IPlugView::attached returned non-OK");
        }
        Ok(binding)
    }

    /// Return the retained plug-in view for the existing editor size APIs.
    pub(super) fn view(&self) -> &ComPtr<IPlugView> {
        &self.view
    }

    #[cfg(target_os = "linux")]
    /// Pump registered Linux editor events on the host's existing UI/control thread.
    pub(super) fn on_idle(&self) {
        self.frame.service_run_loop();
    }
}

impl Drop for EditorBinding {
    fn drop(&mut self) {
        if self.attached_attempted {
            // SAFETY: the retained view is still alive and any partial attachment must be removed.
            unsafe { self.view.removed() };
        }
        self.frame.unbind_view();
        // SAFETY: null unsets the borrowed host frame before its owning wrapper is released.
        unsafe { self.view.setFrame(ptr::null_mut()) };
        #[cfg(target_os = "linux")]
        self.frame.close_run_loop();
    }
}

struct EditorFrame {
    // Weak identity only: EditorBinding owns the view until after this pointer is cleared.
    view: AtomicPtr<IPlugView>,
    #[cfg(target_os = "linux")]
    run_loop: RunLoopState,
}

impl EditorFrame {
    fn new() -> Self {
        Self {
            view: AtomicPtr::new(ptr::null_mut()),
            #[cfg(target_os = "linux")]
            run_loop: RunLoopState::new(),
        }
    }

    fn bind_view(&self, view: *mut IPlugView) {
        self.view.store(view, Ordering::Release);
    }

    fn unbind_view(&self) {
        self.view.store(ptr::null_mut(), Ordering::Release);
    }

    #[cfg(target_os = "linux")]
    fn service_run_loop(&self) {
        self.run_loop.service();
    }

    #[cfg(target_os = "linux")]
    fn close_run_loop(&self) {
        self.run_loop.close();
    }
}

#[cfg(target_os = "linux")]
impl Class for EditorFrame {
    type Interfaces = (IPlugFrame, IRunLoop);
}

#[cfg(not(target_os = "linux"))]
impl Class for EditorFrame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for EditorFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> i32 {
        if view.is_null() || new_size.is_null() || self.view.load(Ordering::Acquire) != view {
            return kInvalidArgument;
        }
        // SAFETY: the SDK keeps the callback's view and rectangle valid for this call. The stored
        // pointer check also rejects stale callbacks after detach without retaining the view.
        let Some(view_ref) = (unsafe { ComRef::from_raw(view) }) else {
            return kInvalidArgument;
        };
        // SAFETY: the SDK supplies a readable rectangle for this synchronous resize request.
        let requested = unsafe { *new_size };
        if requested
            .right
            .checked_sub(requested.left)
            .is_none_or(|width| width <= 0)
            || requested
                .bottom
                .checked_sub(requested.top)
                .is_none_or(|height| height <= 0)
        {
            return kInvalidArgument;
        }
        let mut current = ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: the callback view remains live for the duration of resizeView.
        if unsafe { view_ref.getSize(&raw mut current) } == kResultOk
            && current.left == requested.left
            && current.top == requested.top
            && current.right == requested.right
            && current.bottom == requested.bottom
        {
            return kResultOk;
        }
        // The VST3 contract requires onSize in this same call stack after resizeView.
        unsafe { view_ref.onSize(new_size) }
    }
}

#[cfg(target_os = "linux")]
impl IRunLoopTrait for EditorFrame {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut IEventHandler,
        fd: vst3::Steinberg::Linux::FileDescriptor,
    ) -> i32 {
        self.run_loop.register_event_handler(handler, fd)
    }

    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> i32 {
        self.run_loop.unregister_event_handler(handler)
    }

    unsafe fn registerTimer(
        &self,
        handler: *mut ITimerHandler,
        milliseconds: vst3::Steinberg::Linux::TimerInterval,
    ) -> i32 {
        self.run_loop.register_timer(handler, milliseconds)
    }

    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> i32 {
        self.run_loop.unregister_timer(handler)
    }
}

#[cfg(target_os = "linux")]
struct EventRegistration {
    id: u64,
    fd: libc::c_int,
    handler: ComPtr<IEventHandler>,
}

#[cfg(target_os = "linux")]
struct TimerRegistration {
    id: u64,
    interval: Duration,
    due: Instant,
    handler: ComPtr<ITimerHandler>,
}

#[cfg(target_os = "linux")]
struct Registrations {
    events: Vec<EventRegistration>,
    timers: Vec<TimerRegistration>,
    next_id: u64,
    closed: bool,
}

#[cfg(target_os = "linux")]
impl Registrations {
    fn new() -> Self {
        Self {
            events: Vec::with_capacity(MAX_EVENT_HANDLERS),
            timers: Vec::with_capacity(MAX_TIMER_HANDLERS),
            next_id: 1,
            closed: false,
        }
    }

    fn next_id(&mut self) -> Option<u64> {
        let id = self.next_id;
        self.next_id = id.checked_add(1)?;
        Some(id)
    }
}

#[cfg(target_os = "linux")]
struct RunLoopState {
    registrations: Mutex<Registrations>,
    servicing: AtomicBool,
}

#[cfg(target_os = "linux")]
impl RunLoopState {
    fn new() -> Self {
        Self {
            registrations: Mutex::new(Registrations::new()),
            servicing: AtomicBool::new(false),
        }
    }

    fn registrations(&self) -> MutexGuard<'_, Registrations> {
        self.registrations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn register_event_handler(&self, handler: *mut IEventHandler, fd: libc::c_int) -> i32 {
        if handler.is_null() || fd < 0 {
            return kInvalidArgument;
        }
        // SAFETY: IRunLoop receives a live borrowed interface pointer for this call; upgrading
        // it retains the callback until unregister or editor teardown.
        let Some(handler_ref) = (unsafe { ComRef::from_raw(handler) }) else {
            return kInvalidArgument;
        };
        let owned_handler = handler_ref.to_com_ptr();
        let mut registrations = self.registrations();
        if registrations.closed {
            return kResultFalse;
        }
        if let Some(existing) = registrations
            .events
            .iter()
            .find(|registration| registration.handler.as_ptr() == handler)
        {
            return if existing.fd == fd {
                kResultOk
            } else {
                kInvalidArgument
            };
        }
        if registrations.events.len() == MAX_EVENT_HANDLERS {
            return kResultFalse;
        }
        let Some(id) = registrations.next_id() else {
            return kResultFalse;
        };
        registrations.events.push(EventRegistration {
            id,
            fd,
            handler: owned_handler,
        });
        kResultOk
    }

    fn unregister_event_handler(&self, handler: *mut IEventHandler) -> i32 {
        if handler.is_null() {
            return kInvalidArgument;
        }
        let removed = {
            let mut registrations = self.registrations();
            registrations
                .events
                .iter()
                .position(|registration| registration.handler.as_ptr() == handler)
                .map(|index| registrations.events.swap_remove(index))
        };
        drop(removed);
        kResultOk
    }

    fn register_timer(&self, handler: *mut ITimerHandler, milliseconds: u64) -> i32 {
        if handler.is_null() {
            return kInvalidArgument;
        }
        let interval = Duration::from_millis(milliseconds.max(1));
        let now = Instant::now();
        let Some(due) = now.checked_add(interval) else {
            return kInvalidArgument;
        };
        // SAFETY: IRunLoop receives a live borrowed interface pointer for this call; upgrading
        // it retains the callback until unregister or editor teardown.
        let Some(handler_ref) = (unsafe { ComRef::from_raw(handler) }) else {
            return kInvalidArgument;
        };
        let owned_handler = handler_ref.to_com_ptr();
        let mut registrations = self.registrations();
        if registrations.closed {
            return kResultFalse;
        }
        if registrations
            .timers
            .iter()
            .any(|registration| registration.handler.as_ptr() == handler)
        {
            return kResultOk;
        }
        if registrations.timers.len() == MAX_TIMER_HANDLERS {
            return kResultFalse;
        }
        let Some(id) = registrations.next_id() else {
            return kResultFalse;
        };
        registrations.timers.push(TimerRegistration {
            id,
            interval,
            due,
            handler: owned_handler,
        });
        kResultOk
    }

    fn unregister_timer(&self, handler: *mut ITimerHandler) -> i32 {
        if handler.is_null() {
            return kInvalidArgument;
        }
        let removed = {
            let mut registrations = self.registrations();
            registrations
                .timers
                .iter()
                .position(|registration| registration.handler.as_ptr() == handler)
                .map(|index| registrations.timers.swap_remove(index))
        };
        drop(removed);
        kResultOk
    }

    fn close(&self) {
        let (events, timers) = {
            let mut registrations = self.registrations();
            registrations.closed = true;
            (
                std::mem::take(&mut registrations.events),
                std::mem::take(&mut registrations.timers),
            )
        };
        drop((events, timers));
    }

    fn service(&self) {
        if self
            .servicing
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        let _guard = ServiceGuard(&self.servicing);
        let Some((events, due_timers)) = self.snapshot() else {
            return;
        };
        self.dispatch_events(&events);
        // Release event COM snapshots before invoking timer callbacks.
        drop(events);
        self.dispatch_timers(due_timers);
    }

    fn snapshot(&self) -> Option<(Vec<EventCall>, Vec<TimerCall>)> {
        let registrations = self.registrations();
        if registrations.closed {
            return None;
        }
        let now = Instant::now();
        let events = registrations
            .events
            .iter()
            .map(|registration| EventCall {
                id: registration.id,
                fd: registration.fd,
                handler: registration.handler.clone(),
            })
            .collect();
        let due_timers = registrations
            .timers
            .iter()
            .filter(|registration| registration.due <= now)
            .map(|registration| TimerCall {
                id: registration.id,
                due: registration.due,
                interval: registration.interval,
                handler: registration.handler.clone(),
            })
            .collect();
        Some((events, due_timers))
    }

    fn dispatch_events(&self, events: &[EventCall]) {
        let mut descriptors = events
            .iter()
            .map(|event| libc::pollfd {
                fd: event.fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect::<Vec<_>>();
        if !descriptors.is_empty() {
            // SAFETY: the initialized descriptor array is writable and the timeout is zero, so
            // this readiness check cannot block the plugin control/UI thread.
            let ready = unsafe {
                libc::poll(
                    descriptors.as_mut_ptr(),
                    match libc::nfds_t::try_from(descriptors.len()) {
                        Ok(count) => count,
                        Err(_) => return,
                    },
                    0,
                )
            };
            if ready > 0 {
                for (event, descriptor) in events.iter().zip(&descriptors) {
                    if descriptor.revents & libc::POLLIN != 0
                        && self.event_is_registered(event.id, event.fd)
                    {
                        // SAFETY: each snapshot owns a COM reference. No registry lock is held
                        // across plugin code, so callbacks may safely unregister themselves.
                        unsafe { event.handler.onFDIsSet(event.fd) };
                    }
                }
            }
        }
    }

    fn dispatch_timers(&self, timers: Vec<TimerCall>) {
        for timer in timers {
            let now = Instant::now();
            if self.advance_timer(timer.id, timer.due, timer.interval, now) {
                // SAFETY: the snapshot owns a COM reference and no registry lock is held.
                unsafe { timer.handler.onTimer() };
            }
        }
    }

    fn event_is_registered(&self, id: u64, fd: libc::c_int) -> bool {
        self.registrations()
            .events
            .iter()
            .any(|registration| registration.id == id && registration.fd == fd)
    }

    fn advance_timer(
        &self,
        id: u64,
        expected_due: Instant,
        interval: Duration,
        now: Instant,
    ) -> bool {
        let removed =
            {
                let mut registrations = self.registrations();
                let Some(index) = registrations.timers.iter().position(|registration| {
                    registration.id == id && registration.due == expected_due
                }) else {
                    return false;
                };
                if let Some(next_due) = now.checked_add(interval) {
                    registrations.timers[index].due = next_due;
                    None
                } else {
                    Some(registrations.timers.swap_remove(index))
                }
            };
        let advanced = removed.is_none();
        drop(removed);
        advanced
    }
}

#[cfg(target_os = "linux")]
struct EventCall {
    id: u64,
    fd: libc::c_int,
    handler: ComPtr<IEventHandler>,
}

#[cfg(target_os = "linux")]
struct TimerCall {
    id: u64,
    due: Instant,
    interval: Duration,
    handler: ComPtr<ITimerHandler>,
}

#[cfg(target_os = "linux")]
struct ServiceGuard<'a>(&'a AtomicBool);

#[cfg(target_os = "linux")]
impl Drop for ServiceGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::EditorBinding;
    #[cfg(target_os = "linux")]
    use super::EditorFrame;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::Mutex;
    use vst3::Steinberg::{
        FIDString, IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewTrait, ViewRect,
        kInvalidArgument, kResultOk,
    };
    use vst3::{Class, ComWrapper};

    struct FakeViewState {
        frame: *mut IPlugFrame,
        self_ptr: *mut IPlugView,
        set_frame_result: i32,
        attached_result: i32,
        attach_saw_frame: bool,
        removed_count: usize,
        set_frame_count: usize,
        on_size_count: usize,
        size: ViewRect,
    }

    struct FakeView(Mutex<FakeViewState>);

    impl Class for FakeView {
        type Interfaces = (IPlugView,);
    }

    impl IPlugViewTrait for FakeView {
        unsafe fn isPlatformTypeSupported(&self, _type: FIDString) -> i32 {
            kResultOk
        }

        unsafe fn attached(&self, _parent: *mut c_void, _type: FIDString) -> i32 {
            let (frame, status) = {
                let mut state = self.0.lock().expect("fake view state");
                state.attach_saw_frame = !state.frame.is_null();
                (state.frame, state.attached_result)
            };
            if !frame.is_null() {
                let mut rect = ViewRect {
                    left: 0,
                    top: 0,
                    right: 640,
                    bottom: 480,
                };
                // SAFETY: the stored frame is borrowed from the binding and remains live through
                // the synchronous attached call; this exercises resizeView's onSize contract.
                if let Some(frame_ref) = unsafe { vst3::ComRef::<IPlugFrame>::from_raw(frame) } {
                    let view = self.0.lock().expect("fake view state").self_ptr;
                    unsafe { frame_ref.resizeView(view, &raw mut rect) };
                }
            }
            status
        }

        unsafe fn removed(&self) -> i32 {
            self.0.lock().expect("fake view state").removed_count += 1;
            kResultOk
        }

        unsafe fn onWheel(&self, _distance: f32) -> i32 {
            kResultOk
        }

        unsafe fn onKeyDown(&self, _key: u16, _key_code: i16, _modifiers: i16) -> i32 {
            kResultOk
        }

        unsafe fn onKeyUp(&self, _key: u16, _key_code: i16, _modifiers: i16) -> i32 {
            kResultOk
        }

        unsafe fn getSize(&self, size: *mut ViewRect) -> i32 {
            if size.is_null() {
                return kInvalidArgument;
            }
            // SAFETY: VST3 requires this output pointer to be writable.
            unsafe { *size = self.0.lock().expect("fake view state").size };
            kResultOk
        }

        unsafe fn onSize(&self, new_size: *mut ViewRect) -> i32 {
            if new_size.is_null() {
                return kInvalidArgument;
            }
            // SAFETY: VST3 requires the supplied rectangle to remain readable for this call.
            let rect = unsafe { *new_size };
            let mut state = self.0.lock().expect("fake view state");
            state.on_size_count += 1;
            state.size = rect;
            kResultOk
        }

        unsafe fn onFocus(&self, _state: u8) -> i32 {
            kResultOk
        }

        unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> i32 {
            let mut state = self.0.lock().expect("fake view state");
            state.frame = frame;
            state.set_frame_count += 1;
            if frame.is_null() {
                kResultOk
            } else {
                state.set_frame_result
            }
        }

        unsafe fn canResize(&self) -> i32 {
            kResultOk
        }

        unsafe fn checkSizeConstraint(&self, _rect: *mut ViewRect) -> i32 {
            kResultOk
        }
    }

    fn fake_view(attached_result: i32, set_frame_result: i32) -> ComWrapper<FakeView> {
        let view = ComWrapper::new(FakeView(Mutex::new(FakeViewState {
            frame: ptr::null_mut(),
            self_ptr: ptr::null_mut(),
            set_frame_result,
            attached_result,
            attach_saw_frame: false,
            removed_count: 0,
            set_frame_count: 0,
            on_size_count: 0,
            size: ViewRect {
                left: 0,
                top: 0,
                right: 320,
                bottom: 240,
            },
        })));
        let interface = view.as_com_ref::<IPlugView>().expect("fake view interface");
        view.0.lock().expect("fake view state").self_ptr = interface.as_ptr();
        view
    }

    #[test]
    fn frame_is_set_before_attach_and_unset_after_close() {
        let view_wrapper = fake_view(kResultOk, kResultOk);
        let view = view_wrapper
            .to_com_ptr::<IPlugView>()
            .expect("fake view interface");
        let binding = EditorBinding::attach(view, ptr::null_mut(), c"test-host-parent".as_ptr())
            .expect("editor attaches");
        {
            let state = view_wrapper.0.lock().expect("fake view state");
            assert!(state.attach_saw_frame);
            assert_eq!(state.on_size_count, 1);
            assert_eq!(state.size.right, 640);
            assert_eq!(state.size.bottom, 480);
            assert!(!state.frame.is_null());
        }
        drop(binding);
        let state = view_wrapper.0.lock().expect("fake view state");
        assert_eq!(state.removed_count, 1);
        assert_eq!(state.set_frame_count, 2);
        assert!(state.frame.is_null());
    }

    #[test]
    fn rejected_attach_rolls_back_frame_and_partial_view() {
        let view_wrapper = fake_view(kInvalidArgument, kResultOk);
        let view = view_wrapper
            .to_com_ptr::<IPlugView>()
            .expect("fake view interface");
        let result = EditorBinding::attach(view, ptr::null_mut(), c"test-host-parent".as_ptr());
        assert!(result.is_err());
        let state = view_wrapper.0.lock().expect("fake view state");
        assert!(state.attach_saw_frame);
        assert_eq!(state.removed_count, 1);
        assert_eq!(state.set_frame_count, 2);
        assert!(state.frame.is_null());
    }

    #[test]
    fn rejected_frame_binding_is_cleared_without_removing_unattached_view() {
        let view_wrapper = fake_view(kResultOk, kInvalidArgument);
        let view = view_wrapper
            .to_com_ptr::<IPlugView>()
            .expect("fake view interface");
        let result = EditorBinding::attach(view, ptr::null_mut(), c"test-host-parent".as_ptr());
        assert!(result.is_err());
        let state = view_wrapper.0.lock().expect("fake view state");
        assert_eq!(state.removed_count, 0);
        assert_eq!(state.set_frame_count, 2);
        assert!(state.frame.is_null());
    }

    #[cfg(target_os = "linux")]
    mod linux_run_loop {
        use super::super::{MAX_EVENT_HANDLERS, MAX_TIMER_HANDLERS, RunLoopState};
        use super::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
        use std::time::Instant;
        use vst3::Steinberg::Linux::{
            IEventHandler, IEventHandlerTrait, IRunLoop, IRunLoopTrait, ITimerHandler,
            ITimerHandlerTrait,
        };

        struct ReentrantEventHandler {
            loop_ptr: AtomicPtr<IRunLoop>,
            self_ptr: AtomicPtr<IEventHandler>,
            calls: Arc<AtomicUsize>,
        }

        impl Class for ReentrantEventHandler {
            type Interfaces = (IEventHandler,);
        }

        impl IEventHandlerTrait for ReentrantEventHandler {
            unsafe fn onFDIsSet(&self, _fd: i32) {
                self.calls.fetch_add(1, Ordering::Relaxed);
                // SAFETY: the loop pointer comes from the test's live frame wrapper and remains
                // valid for this synchronous callback. `ComRef` does not change its refcount.
                let Some(run_loop) = (unsafe {
                    vst3::ComRef::<IRunLoop>::from_raw(self.loop_ptr.load(Ordering::Acquire))
                }) else {
                    return;
                };
                let handler = self.self_ptr.load(Ordering::Acquire);
                // The unregister call reenters the registry while service has no lock held.
                unsafe { run_loop.unregisterEventHandler(handler) };
            }
        }

        #[test]
        fn fd_callback_can_unregister_itself_without_holding_registry_lock() {
            let frame_wrapper = ComWrapper::new(EditorFrame::new());
            let run_loop = frame_wrapper
                .as_com_ref::<IRunLoop>()
                .expect("Linux run loop interface");
            let calls = Arc::new(AtomicUsize::new(0));
            let handler_wrapper = ComWrapper::new(ReentrantEventHandler {
                loop_ptr: AtomicPtr::new(run_loop.as_ptr()),
                self_ptr: AtomicPtr::new(ptr::null_mut()),
                calls: Arc::clone(&calls),
            });
            let handler = handler_wrapper
                .as_com_ref::<IEventHandler>()
                .expect("event handler interface");
            handler_wrapper
                .self_ptr
                .store(handler.as_ptr(), Ordering::Release);

            let mut pipe_fds = [-1; 2];
            // SAFETY: `pipe_fds` has room for the two descriptors returned by pipe(2).
            assert_eq!(unsafe { libc::pipe(pipe_fds.as_mut_ptr()) }, 0);
            let byte = [1_u8];
            // SAFETY: the write descriptor is open and the input points to one initialized byte.
            assert_eq!(
                unsafe { libc::write(pipe_fds[1], byte.as_ptr().cast(), 1) },
                1
            );
            assert_eq!(
                unsafe { run_loop.registerEventHandler(handler.as_ptr(), pipe_fds[0]) },
                kResultOk
            );

            frame_wrapper.service_run_loop();
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert!(frame_wrapper.run_loop.registrations().events.is_empty());

            assert_eq!(unsafe { libc::close(pipe_fds[0]) }, 0);
            assert_eq!(unsafe { libc::close(pipe_fds[1]) }, 0);
        }

        #[test]
        fn run_loop_registration_is_bounded_and_close_clears_callbacks() {
            let state = RunLoopState::new();
            let drops = Arc::new(AtomicUsize::new(0));
            for index in 0..MAX_EVENT_HANDLERS {
                let handler = ComWrapper::new(DropCountHandler(Arc::clone(&drops)));
                let event_handler = handler
                    .as_com_ref::<IEventHandler>()
                    .expect("event handler interface");
                let fd = i32::try_from(index).expect("handler slot fits file descriptor");
                assert_eq!(
                    state.register_event_handler(event_handler.as_ptr(), fd),
                    kResultOk
                );
                if index < MAX_TIMER_HANDLERS {
                    let timer_handler = handler
                        .as_com_ref::<ITimerHandler>()
                        .expect("timer handler interface");
                    assert_eq!(
                        state.register_timer(timer_handler.as_ptr(), 60_000),
                        kResultOk
                    );
                }
            }
            assert_eq!(state.registrations().events.len(), MAX_EVENT_HANDLERS);
            assert_eq!(state.registrations().timers.len(), MAX_TIMER_HANDLERS);
            assert_eq!(drops.load(Ordering::Relaxed), 0);

            let overflow = ComWrapper::new(DropCountHandler(Arc::clone(&drops)));
            let overflow_ref = overflow
                .as_com_ref::<IEventHandler>()
                .expect("event handler interface");
            assert_eq!(
                state.register_event_handler(overflow_ref.as_ptr(), 100),
                vst3::Steinberg::kResultFalse
            );
            drop(overflow);
            assert_eq!(drops.load(Ordering::Relaxed), 1);

            state.close();
            assert!(state.registrations().events.is_empty());
            assert!(state.registrations().timers.is_empty());
            assert_eq!(drops.load(Ordering::Relaxed), MAX_EVENT_HANDLERS + 1);
        }

        #[test]
        fn timer_registration_rejects_after_teardown() {
            let frame_wrapper = ComWrapper::new(EditorFrame::new());
            let run_loop = frame_wrapper
                .as_com_ref::<IRunLoop>()
                .expect("Linux run loop interface");
            let timer = ComWrapper::new(DropCountHandler(Arc::new(AtomicUsize::new(0))));
            let timer_ref = timer
                .as_com_ref::<ITimerHandler>()
                .expect("timer handler interface");
            frame_wrapper.close_run_loop();
            assert_eq!(
                unsafe { run_loop.registerTimer(timer_ref.as_ptr(), 1) },
                vst3::Steinberg::kResultFalse
            );
        }

        struct ReentrantTimerHandler {
            loop_ptr: AtomicPtr<IRunLoop>,
            self_ptr: AtomicPtr<ITimerHandler>,
            calls: Arc<AtomicUsize>,
        }

        impl Class for ReentrantTimerHandler {
            type Interfaces = (ITimerHandler,);
        }

        impl ITimerHandlerTrait for ReentrantTimerHandler {
            unsafe fn onTimer(&self) {
                self.calls.fetch_add(1, Ordering::Relaxed);
                // SAFETY: the loop pointer comes from the test's live frame wrapper and remains
                // valid for this synchronous callback. `ComRef` does not change its refcount.
                let Some(run_loop) = (unsafe {
                    vst3::ComRef::<IRunLoop>::from_raw(self.loop_ptr.load(Ordering::Acquire))
                }) else {
                    return;
                };
                let handler = self.self_ptr.load(Ordering::Acquire);
                // Timer callbacks may unregister themselves without reentering a held registry lock.
                unsafe { run_loop.unregisterTimer(handler) };
            }
        }

        #[test]
        fn due_timer_callback_can_unregister_itself() {
            let frame_wrapper = ComWrapper::new(EditorFrame::new());
            let run_loop = frame_wrapper
                .as_com_ref::<IRunLoop>()
                .expect("Linux run loop interface");
            let calls = Arc::new(AtomicUsize::new(0));
            let timer_wrapper = ComWrapper::new(ReentrantTimerHandler {
                loop_ptr: AtomicPtr::new(run_loop.as_ptr()),
                self_ptr: AtomicPtr::new(ptr::null_mut()),
                calls: Arc::clone(&calls),
            });
            let timer = timer_wrapper
                .as_com_ref::<ITimerHandler>()
                .expect("timer handler interface");
            timer_wrapper
                .self_ptr
                .store(timer.as_ptr(), Ordering::Release);
            assert_eq!(
                unsafe { run_loop.registerTimer(timer.as_ptr(), 60_000) },
                kResultOk
            );
            frame_wrapper.run_loop.registrations().timers[0].due = Instant::now();

            frame_wrapper.service_run_loop();

            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert!(frame_wrapper.run_loop.registrations().timers.is_empty());
        }

        struct DropCountHandler(Arc<AtomicUsize>);

        impl Drop for DropCountHandler {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        impl Class for DropCountHandler {
            type Interfaces = (IEventHandler, ITimerHandler);
        }

        impl IEventHandlerTrait for DropCountHandler {
            unsafe fn onFDIsSet(&self, _fd: i32) {}
        }

        impl ITimerHandlerTrait for DropCountHandler {
            unsafe fn onTimer(&self) {}
        }
    }
}
