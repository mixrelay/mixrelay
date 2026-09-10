//! Cross-platform outer windows for third-party processor editors.
//!
//! The plug-in remains in its isolated DSP worker. The desktop/control plane only brokers
//! idempotent open and close requests, while this module owns the native parent window in the
//! same process as the plug-in instance. That is required on macOS (`NSView` pointers cannot
//! cross processes) and avoids unsupported cross-process child-window behavior.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorHostFailure {
    DisplayUnavailable,
    HostWindow,
    PluginRejected,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct EditorHostEvents {
    pub close_requested: bool,
    pub resized: Option<(u32, u32)>,
}

impl EditorHostEvents {
    fn merge(&mut self, incoming: Self) {
        self.close_requested |= incoming.close_requested;
        if self.close_requested {
            self.resized = None;
        } else if incoming.resized.is_some() {
            self.resized = incoming.resized;
        }
    }
}

#[cfg(target_os = "macos")]
#[path = "native_editor/macos.rs"]
mod platform;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use tao::{
        dpi::{LogicalSize, PhysicalSize},
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        platform::run_return::EventLoopExtRunReturn,
        rwh_06::{HasWindowHandle, RawWindowHandle},
        window::{Window, WindowBuilder},
    };
    use truce_rack_core::WindowHandle;

    use super::{EditorHostEvents, EditorHostFailure};
    use crate::rack_formats::{EditorSnapshot, RackWorkerPlugin};

    const DEFAULT_WIDTH: u32 = 720;
    const DEFAULT_HEIGHT: u32 = 480;

    pub(crate) struct NativeEditorHost {
        event_loop: EventLoop<()>,
        window: Option<Window>,
        pub(super) pending_events: EditorHostEvents,
    }

    impl NativeEditorHost {
        pub(crate) fn new(
            _shared: std::sync::Arc<crate::shared_transport::WorkerControl>,
        ) -> Result<Self, EditorHostFailure> {
            let event_loop = std::panic::catch_unwind(EventLoop::new)
                .map_err(|_| EditorHostFailure::DisplayUnavailable)?;
            Ok(Self {
                event_loop,
                window: None,
                pending_events: EditorHostEvents::default(),
            })
        }

        pub(crate) fn open(
            &mut self,
            plugin: &mut RackWorkerPlugin,
            title: &str,
        ) -> Result<EditorSnapshot, EditorHostFailure> {
            if let Some(window) = &self.window {
                window.set_visible(true);
                window.set_focus();
                return Ok(plugin.tick_editor());
            }
            let window = WindowBuilder::new()
                .with_title(title)
                .with_inner_size(LogicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT))
                .with_visible(false)
                .build(&self.event_loop)
                .map_err(|_| EditorHostFailure::HostWindow)?;
            let parent = window_handle(&window).map_err(|()| EditorHostFailure::HostWindow)?;
            let snapshot = plugin
                .open_editor(parent, window.scale_factor())
                .map_err(|_| EditorHostFailure::PluginRejected)?;
            if let Some((width, height)) = snapshot.size {
                window.set_inner_size(LogicalSize::new(width.max(1), height.max(1)));
            }
            window.set_resizable(snapshot.resizable);
            window.set_visible(true);
            window.set_focus();
            self.window = Some(window);
            Ok(snapshot)
        }

        pub(crate) fn close(&mut self, plugin: &mut RackWorkerPlugin) {
            plugin.close_editor();
            self.window.take();
            self.pending_events = EditorHostEvents::default();
        }

        pub(super) fn collect_events(&mut self) -> EditorHostEvents {
            let Some(window) = self.window.as_ref() else {
                return EditorHostEvents::default();
            };
            let window_id = window.id();
            let mut close_requested = false;
            let mut resized = None;
            let _ = self.event_loop.run_return(|event, _, control_flow| {
                *control_flow = ControlFlow::Poll;
                match event {
                    Event::WindowEvent {
                        event: WindowEvent::CloseRequested,
                        window_id: event_window,
                        ..
                    } if event_window == window_id => close_requested = true,
                    Event::WindowEvent {
                        event: WindowEvent::Resized(size),
                        window_id: event_window,
                        ..
                    } if event_window == window_id => resized = Some(size),
                    Event::MainEventsCleared => *control_flow = ControlFlow::Exit,
                    _ => {}
                }
            });
            EditorHostEvents {
                close_requested,
                resized: resized
                    .map(|PhysicalSize { width, height }| (width.max(1), height.max(1))),
            }
        }

        pub(crate) fn refresh(&mut self, snapshot: EditorSnapshot) {
            if !snapshot.open {
                self.window.take();
                self.pending_events = EditorHostEvents::default();
            }
        }
    }

    impl Drop for NativeEditorHost {
        fn drop(&mut self) {
            self.window.take();
        }
    }

    fn window_handle(window: &Window) -> Result<WindowHandle, ()> {
        let handle = window.window_handle().map_err(|_| ())?;
        match handle.as_raw() {
            RawWindowHandle::AppKit(handle) => Ok(WindowHandle::NSView(handle.ns_view.as_ptr())),
            RawWindowHandle::Win32(handle) => Ok(WindowHandle::HWND(
                handle.hwnd.get().cast_unsigned() as *mut _,
            )),
            _ => Err(()),
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{ffi::CString, ptr};

    use truce_rack_core::WindowHandle;
    use x11_dl::xlib;

    use super::{EditorHostEvents, EditorHostFailure};
    use crate::rack_formats::{EditorSnapshot, RackWorkerPlugin};

    const DEFAULT_WIDTH: u32 = 720;
    const DEFAULT_HEIGHT: u32 = 480;

    pub(crate) struct NativeEditorHost {
        xlib: xlib::Xlib,
        display: *mut xlib::Display,
        window: Option<xlib::Window>,
        wm_delete_window: xlib::Atom,
        pub(super) pending_events: EditorHostEvents,
    }

    impl NativeEditorHost {
        pub(crate) fn new(
            _shared: std::sync::Arc<crate::shared_transport::WorkerControl>,
        ) -> Result<Self, EditorHostFailure> {
            if std::env::var_os("DISPLAY").is_none() {
                return Err(EditorHostFailure::DisplayUnavailable);
            }
            let xlib = xlib::Xlib::open().map_err(|_| EditorHostFailure::DisplayUnavailable)?;
            // SAFETY: XOpenDisplay copies DISPLAY and returns a connection closed once in Drop.
            let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
            if display.is_null() {
                return Err(EditorHostFailure::DisplayUnavailable);
            }
            let atom_name = CString::new("WM_DELETE_WINDOW").expect("static atom has no NUL");
            // SAFETY: display is live and atom_name is NUL-terminated for the duration of the call.
            let wm_delete_window =
                unsafe { (xlib.XInternAtom)(display, atom_name.as_ptr(), xlib::False) };
            Ok(Self {
                xlib,
                display,
                window: None,
                wm_delete_window,
                pending_events: EditorHostEvents::default(),
            })
        }

        pub(crate) fn open(
            &mut self,
            plugin: &mut RackWorkerPlugin,
            title: &str,
        ) -> Result<EditorSnapshot, EditorHostFailure> {
            if let Some(window) = self.window {
                // SAFETY: the XID belongs to this live display connection.
                unsafe {
                    (self.xlib.XMapRaised)(self.display, window);
                    (self.xlib.XSetInputFocus)(
                        self.display,
                        window,
                        xlib::RevertToParent,
                        xlib::CurrentTime,
                    );
                    (self.xlib.XFlush)(self.display);
                }
                return Ok(plugin.tick_editor());
            }
            // SAFETY: the display remains live for the full host lifetime.
            let screen = unsafe { (self.xlib.XDefaultScreen)(self.display) };
            let root = unsafe { (self.xlib.XRootWindow)(self.display, screen) };
            let background = unsafe { (self.xlib.XBlackPixel)(self.display, screen) };
            // SAFETY: all arguments are derived from the live display and its default screen.
            let window = unsafe {
                (self.xlib.XCreateSimpleWindow)(
                    self.display,
                    root,
                    0,
                    0,
                    DEFAULT_WIDTH,
                    DEFAULT_HEIGHT,
                    0,
                    background,
                    background,
                )
            };
            if window == 0 {
                return Err(EditorHostFailure::HostWindow);
            }
            let title = CString::new(title).unwrap_or_else(|_| {
                CString::new("MixRelay plug-in").expect("static title has no NUL")
            });
            let mut protocols = [self.wm_delete_window];
            // SAFETY: the new window is owned by this display. Xlib copies title/protocol data.
            unsafe {
                (self.xlib.XStoreName)(self.display, window, title.as_ptr());
                (self.xlib.XSelectInput)(self.display, window, xlib::StructureNotifyMask);
                (self.xlib.XSetWMProtocols)(self.display, window, protocols.as_mut_ptr(), 1);
            }
            let Ok(snapshot) = plugin.open_editor(WindowHandle::X11(window), 1.0) else {
                // SAFETY: the window was created above and has not been destroyed.
                unsafe { (self.xlib.XDestroyWindow)(self.display, window) };
                return Err(EditorHostFailure::PluginRejected);
            };
            if let Some((width, height)) = snapshot.size {
                // SAFETY: the XID belongs to this display.
                unsafe {
                    (self.xlib.XResizeWindow)(self.display, window, width.max(1), height.max(1))
                };
            }
            // SAFETY: the XID belongs to this display and XFlush does not retain pointers.
            unsafe {
                (self.xlib.XMapRaised)(self.display, window);
                (self.xlib.XFlush)(self.display);
            }
            self.window = Some(window);
            Ok(snapshot)
        }

        pub(crate) fn close(&mut self, plugin: &mut RackWorkerPlugin) {
            plugin.close_editor();
            self.destroy_window();
        }

        pub(super) fn collect_events(&mut self) -> EditorHostEvents {
            let Some(window) = self.window else {
                return EditorHostEvents::default();
            };
            let mut result = EditorHostEvents::default();
            // SAFETY: events are read only from this host's live display on its owning thread.
            unsafe {
                while (self.xlib.XPending)(self.display) > 0 {
                    let mut event = std::mem::MaybeUninit::<xlib::XEvent>::uninit();
                    (self.xlib.XNextEvent)(self.display, event.as_mut_ptr());
                    let event = event.assume_init();
                    match event.get_type() {
                        xlib::ClientMessage => {
                            let client = xlib::XClientMessageEvent::from(event);
                            if client.window == window
                                && client.format == 32
                                && client.data.get_long(0).cast_unsigned() == self.wm_delete_window
                            {
                                result.close_requested = true;
                            }
                        }
                        xlib::ConfigureNotify => {
                            let configure = xlib::XConfigureEvent::from(event);
                            if configure.window == window {
                                result.resized = Some((
                                    u32::try_from(configure.width).unwrap_or(1).max(1),
                                    u32::try_from(configure.height).unwrap_or(1).max(1),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
            result
        }

        pub(crate) fn refresh(&mut self, snapshot: EditorSnapshot) {
            if !snapshot.open {
                self.destroy_window();
            }
        }

        fn destroy_window(&mut self) {
            self.pending_events = EditorHostEvents::default();
            if let Some(window) = self.window.take() {
                // SAFETY: this XID belongs to the live display and is destroyed at most once.
                unsafe {
                    (self.xlib.XDestroyWindow)(self.display, window);
                    (self.xlib.XFlush)(self.display);
                }
            }
        }
    }

    impl Drop for NativeEditorHost {
        fn drop(&mut self) {
            self.destroy_window();
            // SAFETY: display was returned by XOpenDisplay and has not been closed yet.
            unsafe { (self.xlib.XCloseDisplay)(self.display) };
        }
    }
}

pub(crate) use platform::NativeEditorHost;

impl NativeEditorHost {
    /// Native tracking loops may block until mouse-up; never hold the DSP mutex here.
    pub(crate) fn poll_events(&mut self) {
        let events = self.collect_events();
        self.pending_events.merge(events);
    }

    /// Consume only after the broker has acquired the plug-in for lifecycle operations.
    pub(crate) fn take_events(&mut self) -> EditorHostEvents {
        std::mem::take(&mut self.pending_events)
    }
}

#[cfg(test)]
mod tests {
    use super::EditorHostEvents;

    #[test]
    fn close_survives_contention_and_is_consumed_once() {
        let plugin = std::sync::Mutex::new(());
        let guard = plugin.lock().unwrap();
        let mut pending = EditorHostEvents::default();
        pending.merge(EditorHostEvents {
            close_requested: true,
            resized: None,
        });
        for _ in 0..20 {
            pending.merge(EditorHostEvents::default());
            assert!(plugin.try_lock().is_err());
            assert!(pending.close_requested);
        }
        drop(guard);
        let _guard = plugin.try_lock().unwrap();
        assert!(std::mem::take(&mut pending).close_requested);
        assert_eq!(pending, EditorHostEvents::default());
    }

    #[test]
    fn latest_resize_wins_until_close_supersedes_it() {
        let mut pending = EditorHostEvents::default();
        for size in [(400, 200), (800, 600)] {
            pending.merge(EditorHostEvents {
                close_requested: false,
                resized: Some(size),
            });
        }
        assert_eq!(pending.resized, Some((800, 600)));
        pending.merge(EditorHostEvents {
            close_requested: true,
            resized: None,
        });
        pending.merge(EditorHostEvents {
            close_requested: false,
            resized: Some((900, 700)),
        });
        assert!(pending.close_requested);
        assert_eq!(pending.resized, None);
    }
}
