use std::error::Error;
use std::ffi::{c_int, c_long, c_ulong, c_void, CStr};
use std::fmt;
use std::ptr::{self, NonNull};
use std::time::{Duration, Instant};

use loki_linux::x11::{
    self, errcode, et, prop_mode, property, xevent_mask, Atom, Bool, LibX11, XDisplay, XErrorEvent,
    XEvent, XSelectionEvent, XSelectionRequestEvent, XWindow,
};

pub mod atom_names {
    use std::ffi::CStr;

    /// The primary clipboard
    pub const PRIMARY: &CStr = c"PRIMARY";
    /// The secondary clipboard
    pub const SECONDARY: &CStr = c"SECONDARY";
    /// The actual clipboard that most apps use
    pub const CLIPBOARD: &CStr = c"CLIPBOARD";
    /// Our custom dummy atom
    pub const CLIPBOX: &CStr = c"CLIPBOX";
    /// Our custom dummy atom
    pub const CLIPBOX_DUMMY: &CStr = c"CLIPBOX_DUMMY";

    /// Property type: ASCII string
    pub const STRING: &CStr = c"STRING";
    /// Property type: text
    pub const TEXT: &CStr = c"TEXT";
    /// Property type: UTF8 string
    pub const UTF8_STRING: &CStr = c"UTF8_STRING";
    /// Property type: targets (list of atoms)
    pub const TARGETS: &CStr = c"TARGETS";
    /// Property type: incremental data fetching
    pub const INCR: &CStr = c"INCR";
    /// Property type: atom
    pub const ATOM: &CStr = c"ATOM";
}

/// Some commonly used mime types. They're literally infinite so the list cannot be exclusive.
pub mod mime_types {
    #![allow(unused)]

    use std::ffi::CStr;

    pub const TEXT_PLAIN: &CStr = c"text/plain";
    pub const TEXT_PLAIN_CHARSET_UTF8: &CStr = c"text/plain;charset=utf-8";
    pub const TEXT_HTML: &CStr = c"text/html";

    pub const IMAGE_PNG: &CStr = c"image/png";
    pub const IMAGE_JPG: &CStr = c"image/jpg";
    pub const IMAGE_JPEG: &CStr = c"image/jpeg";
}

#[derive(Debug)]
pub struct Atoms {
    /// The primary clipboard
    pub primary: Atom,
    /// The secondary clipboard
    pub secondary: Atom,
    /// The actual clipboard that most apps use
    pub clipboard: Atom,
    /// Our custom data atom
    pub clipbox: Atom,
    /// Our custom dummy atom, to get a compliant timestamp
    pub clipbox_dummy: Atom,

    /// Property type: ASCII string
    pub string: Atom,
    /// Property type: text
    pub text: Atom,
    /// Property type: UTF8 string
    pub utf8_string: Atom,

    /// Property type: targets (list of atoms)
    pub targets: Atom,
    /// Property type: incremental data fetching
    pub incr: Atom,
    /// Property type: atom
    pub atom: Atom,
}

unsafe fn intern_atom(x: &LibX11, display: NonNull<XDisplay>, name: &CStr) -> Atom {
    (x.XInternAtom)(display.as_ptr(), name.as_ptr() as _, 0)
}

unsafe fn get_atom_name(x: &LibX11, display: NonNull<XDisplay>, atom: Atom) -> &CStr {
    CStr::from_ptr((x.XGetAtomName)(display.as_ptr(), atom))
}

#[derive(Debug)]
pub struct PropertyInvalidFormatError {
    pub wanted: u8,
    pub actual: u8,
}

impl Error for PropertyInvalidFormatError {}

impl fmt::Display for PropertyInvalidFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid format: property is {} bits, but type wanted is {} bits",
            self.actual, self.wanted
        )
    }
}

struct XWindowProperty<'a> {
    // This is here to make sure we free the prop when dropping
    x11: &'a LibX11,

    pub ty: Atom,
    pub format: c_int,
    pub nitems: c_ulong,
    pub bytes_remaining: c_ulong,
    pub data: NonNull<c_void>,
}

impl<'a> XWindowProperty<'a> {
    /// Checks that the format size is compatible with the size of `T`
    fn check_format_compatible<T>(&self) -> Result<(), PropertyInvalidFormatError> {
        let t_format: usize = 8 * std::mem::size_of::<T>();
        match self.format as usize == t_format {
            true => Ok(()),
            false => Err(PropertyInvalidFormatError {
                wanted: t_format as u8,
                actual: self.format as u8,
            }),
        }
    }

    /// Writes this property into a vec
    fn write_into_vec<T>(self, buf: &mut Vec<T>) -> Result<(), PropertyInvalidFormatError> {
        self.check_format_compatible::<T>()?;

        let prev_len = buf.len();
        let n_items = self.nitems as usize;
        buf.reserve(n_items);

        unsafe {
            // SAFETY: we trust Xlib that the source is aligned and valid,
            // and `buf.reserve` ensures that we have usable space to write them.
            ptr::copy_nonoverlapping(
                self.data.as_ptr().cast::<T>().cast_const(),
                buf.as_mut_ptr().add(prev_len),
                n_items,
            );

            // SAFETY: We created it with this much capacity earlier,
            // and the previous `copy` has initialized these elements.
            buf.set_len(prev_len + n_items);
        }

        Ok(())
    }

    /// Converts this property into a vec
    fn into_vec<T>(self) -> Result<Vec<T>, PropertyInvalidFormatError> {
        let mut prop = Vec::new();
        self.write_into_vec(&mut prop)?;
        Ok(prop)
    }
}

impl<'a> Drop for XWindowProperty<'a> {
    fn drop(&mut self) {
        unsafe {
            // The data is free \o/
            (self.x11.XFree)(self.data.as_ptr());
        }
    }
}

impl<'a> fmt::Debug for XWindowProperty<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("XWindowProperty")
            .field("ty", &self.ty)
            .field("format", &self.format)
            .field("nitems", &self.nitems)
            .field("bytes_remaining", &self.bytes_remaining)
            .field("prop", &"[FILTERED]")
            .finish()
    }
}

unsafe extern "C" fn x11_error_handler(_display: *mut XDisplay, event: *mut XErrorEvent) -> i32 {
    if let Some(event) = event.as_ref() {
        println!("X11: error (code {})", event.error_code);
    } else {
        println!("X11 called the error handler without an error event or a display, somehow");
    }

    0
}

pub struct X11Clipboard {
    x: LibX11,
    display: NonNull<XDisplay>,
    window: XWindow,
    atoms: Atoms,
    max_request_size: usize,
}

impl X11Clipboard {
    pub fn init() -> Result<Self, Box<dyn Error>> {
        unsafe {
            let x = LibX11::new()?;

            (x.XSetErrorHandler)(Some(x11_error_handler));

            // Open the default X11 display
            let display = (x.XOpenDisplay)(std::ptr::null());
            let display = NonNull::new(display).ok_or("cannot open display :(")?;

            let root = (x.XDefaultRootWindow)(display.as_ptr());

            // Create a window to trap events
            let window = (x.XCreateSimpleWindow)(display.as_ptr(), root, 0, 0, 1, 1, 0, 0, 0);

            // Select property change events
            (x.XSelectInput)(display.as_ptr(), window, xevent_mask::PROPERTY_CHANGE);

            let atoms = Atoms {
                primary: intern_atom(&x, display, atom_names::PRIMARY),
                secondary: intern_atom(&x, display, atom_names::SECONDARY),
                clipboard: intern_atom(&x, display, atom_names::CLIPBOARD),
                clipbox: intern_atom(&x, display, atom_names::CLIPBOX),
                clipbox_dummy: intern_atom(&x, display, atom_names::CLIPBOX_DUMMY),
                string: intern_atom(&x, display, atom_names::STRING),
                text: intern_atom(&x, display, atom_names::TEXT),
                utf8_string: intern_atom(&x, display, atom_names::UTF8_STRING),
                targets: intern_atom(&x, display, atom_names::TARGETS),
                incr: intern_atom(&x, display, atom_names::INCR),
                atom: intern_atom(&x, display, atom_names::ATOM),
            };

            let max_request_size = (x.XMaxRequestSize)(display.as_ptr()) as usize;

            Ok(Self {
                x,
                display,
                window,
                atoms,
                max_request_size,
            })
        }
    }

    unsafe fn next_event(&self) -> XEvent {
        let mut xevent = XEvent { type_id: 0 };
        (self.x.XNextEvent)(self.display.as_ptr(), &mut xevent);
        xevent
    }

    /// Tries to get the next event before the timeout.
    /// It will look for pending events every 100µs.
    unsafe fn next_event_timeout(&self, timeout: Duration) -> Option<XEvent> {
        let start = Instant::now();
        loop {
            let pending = (self.x.XPending)(self.display.as_ptr());

            if pending == 0 {
                let elapsed = start.elapsed();
                if elapsed > timeout {
                    return None;
                }

                print!(
                    "\x1b[2K\rWaiting for next event... {}µs",
                    elapsed.as_micros()
                );

                std::thread::sleep(Duration::from_micros(100));
                continue;
            }

            println!("\x1b[2K\rPending: {}", pending);
            break;
        }

        Some(self.next_event())
    }

    /// Get a compliant timestamp for selection requests
    ///
    /// # Convention
    ///
    /// *Clients attempting to acquire a selection must set the time value of the
    /// `SetSelectionOwner` request to the timestamp of the event triggering the
    /// acquisition attempt, not to `CurrentTime`. A zero-length append to a property
    /// is a way to obtain a timestamp for this purpose; the timestamp is in the
    /// corresponding `PropertyNotify` event.*
    ///
    /// [ICCCM - Acquiring Selection Ownership](https://tronche.com/gui/x/icccm/sec-2.html#s-2.1)
    unsafe fn get_compliant_timestamp(&self) -> c_ulong {
        // Send dummy change property request to obtain a timestamp from its resulting event
        // This is because it is disincentivized to use CurrentTime when sending a ConvertSelection request
        (self.x.XChangeProperty)(
            self.display.as_ptr(),
            self.window,
            self.atoms.clipbox_dummy,
            self.atoms.string,
            8,
            prop_mode::APPEND,
            std::ptr::null(),
            0,
        );

        loop {
            let xevent = self.next_event();

            if xevent.type_id == et::PROPERTY_NOTIFY {
                let xevent = xevent.xproperty;

                if xevent.atom == self.atoms.clipbox_dummy {
                    return xevent.time;
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum GetSelectionError {
    BadSelection,
    SelectionLost,
    GetPropertyFailed(i32),
    NoDataInProperty,
    PropertyInvalidFormat(PropertyInvalidFormatError),
}

impl Error for GetSelectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PropertyInvalidFormat(err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for GetSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSelection => write!(f, "Received notification for a selection that wasn't requested. The X server is probably mad"),
            Self::SelectionLost => write!(f, "Selection lost (probably a bug with the clipbox library, how did that happen?)"),
            Self::GetPropertyFailed(status) => write!(f, "Couldn't get property (error code: {})", status),
            Self::NoDataInProperty => write!(f, "No data in our dedicated X11 property (how even)"),
            Self::PropertyInvalidFormat(err) => err.fmt(f),
        }
    }
}

impl From<PropertyInvalidFormatError> for GetSelectionError {
    fn from(value: PropertyInvalidFormatError) -> Self {
        Self::PropertyInvalidFormat(value)
    }
}

// Paste (get selection)
impl X11Clipboard {
    unsafe fn get_selection_event(
        &self,
        atom_selection: Atom,
        atom_target: Atom,
    ) -> Result<XSelectionEvent, GetSelectionError> {
        let when_everything_started = self.get_compliant_timestamp();

        // Send a ConvertSelection request
        (self.x.XConvertSelection)(
            self.display.as_ptr(),
            atom_selection,
            atom_target,
            self.atoms.clipbox,
            self.window,
            when_everything_started,
        );

        let xevent = loop {
            let xevent = self.next_event();

            if xevent.type_id == et::SELECTION_NOTIFY {
                let xevent = xevent.xselection;

                if xevent.requestor == self.window
                    && xevent.selection == atom_selection
                    && xevent.target == atom_target
                {
                    break xevent;
                } else {
                    return Err(GetSelectionError::BadSelection);
                }
            }
        };

        if xevent.property == 0 {
            return Err(GetSelectionError::SelectionLost);
        }

        Ok(xevent)
    }

    fn get_clipbox_property(&self) -> Result<XWindowProperty, GetSelectionError> {
        let mut ty: Atom = 0;
        let mut format: c_int = 8;
        let mut nitems: c_ulong = 0;
        let mut bytes_remaining: c_ulong = 0;
        let mut data: *mut c_void = std::ptr::null_mut();

        let status = unsafe {
            let long_offset: c_long = 0;
            let long_length: c_long = c_long::MAX;
            let delete: Bool = x11::bool::FALSE;
            let req_type: Atom = 0;

            (self.x.XGetWindowProperty)(
                self.display.as_ptr(),
                self.window,
                self.atoms.clipbox,
                long_offset,
                long_length,
                delete,
                req_type,
                &mut ty,
                &mut format,
                &mut nitems,
                &mut bytes_remaining,
                &mut data,
            )
        };

        if status != errcode::SUCCESS {
            return Err(GetSelectionError::GetPropertyFailed(status));
        }

        let Some(data) = NonNull::new(data) else {
            return Err(GetSelectionError::NoDataInProperty);
        };

        Ok(XWindowProperty {
            x11: &self.x,
            ty,
            format,
            nitems,
            bytes_remaining,
            data,
        })
    }

    pub fn get_targets(&self, selection: &CStr) -> Result<Vec<&CStr>, GetSelectionError> {
        unsafe {
            let atom_selection = intern_atom(&self.x, self.display, selection);
            self.get_selection_event(atom_selection, self.atoms.targets)?
        };

        let clipbox_prop = self.get_clipbox_property()?;

        let targets = clipbox_prop
            .into_vec::<u32>()?
            .into_iter()
            .filter(|&atom| atom != 0)
            .map(|atom| unsafe { get_atom_name(&self.x, self.display, atom as Atom) })
            .collect::<Vec<_>>();

        Ok(targets)
    }

    pub fn get_selection(
        &self,
        selection: &CStr,
        target: &CStr,
    ) -> Result<Vec<u8>, GetSelectionError> {
        if target == atom_names::TARGETS {
            panic!(concat!(
                "TARGETS is a special selection target, this method doesn't support it.",
                " Try X11Clipboard::get_targets instead!"
            ));
        }

        unsafe {
            let atom_selection = intern_atom(&self.x, self.display, selection);
            let atom_target = intern_atom(&self.x, self.display, target);
            self.get_selection_event(atom_selection, atom_target)?
        };

        let clipbox_prop = self.get_clipbox_property()?;

        if clipbox_prop.ty == self.atoms.incr {
            // We got an INCR atom, fetch property incrementally
            let mut data = Vec::new();

            loop {
                unsafe {
                    // First delete the INCR property
                    (self.x.XDeleteProperty)(
                        self.display.as_ptr(),
                        self.window,
                        self.atoms.clipbox,
                    );

                    // Waiting for a `PropertyNotify` with the state argument `NewValue`
                    loop {
                        let xevent = self.next_event();

                        if xevent.type_id == et::PROPERTY_NOTIFY {
                            let xevent = xevent.xproperty;

                            if xevent.state == property::NEW_VALUE {
                                break;
                            }
                        }
                    }

                    let clipbox_prop = self.get_clipbox_property()?;

                    if clipbox_prop.nitems == 0 {
                        break;
                    }

                    clipbox_prop.write_into_vec(&mut data)?;
                }
            }

            Ok(data)
        } else {
            Ok(clipbox_prop.into_vec()?)
        }
    }
}

#[derive(Debug)]
pub enum SetSelectionError {
    NotOwner,
}

impl Error for SetSelectionError {}

impl fmt::Display for SetSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotOwner => write!(f, "Could not become the selection owner"),
        }
    }
}

// Copy (set selection)
impl X11Clipboard {
    pub fn set_selection(
        &self,
        selection: &CStr,
        target: &CStr,
        data: &[u8],
    ) -> Result<(), SetSelectionError> {
        let when_everything_started = unsafe { self.get_compliant_timestamp() };

        unsafe {
            let atom_selection = intern_atom(&self.x, self.display, selection);

            // Become owner of selection
            (self.x.XSetSelectionOwner)(
                self.display.as_ptr(),
                atom_selection,
                self.window,
                when_everything_started,
            );

            // Verify that we did indeed become owner of selection
            let owner = (self.x.XGetSelectionOwner)(self.display.as_ptr(), atom_selection);
            if owner != self.window {
                // \(T-T)/
                return Err(SetSelectionError::NotOwner);
            }

            let target_atoms = &[
                self.atoms.targets,
                intern_atom(&self.x, self.display, target),
            ];

            const INCR_CHUNK_SIZE: usize = 4096;
            let mut incr_bytes_sent: usize = 0;
            let mut incr_start_xevent: Option<XSelectionRequestEvent> = None;
            loop {
                let Some(xevent) = self.next_event_timeout(Duration::from_millis(100)) else {
                    // we're not receiving any event immediately, consider the operation finished
                    return Ok(());
                };

                if xevent.type_id == et::SELECTION_REQUEST {
                    let mut xevent = xevent.xselectionrequest;

                    // "If the specified property is None, the requestor is an obsolete client.
                    // Owners are encouraged to support these clients by using the specified target
                    // atom as the property name to be used for the reply."
                    xevent.property = match xevent.property {
                        0 => xevent.target,
                        _ => xevent.property,
                    };

                    if xevent.owner != self.window {
                        continue;
                    }

                    if xevent.selection != atom_selection {
                        continue;
                    }

                    if target_atoms.contains(&xevent.target) {
                        if xevent.target == self.atoms.targets {
                            // Send our available targets
                            (self.x.XChangeProperty)(
                                xevent.display,
                                xevent.requestor,
                                xevent.property,
                                self.atoms.atom,
                                32,
                                prop_mode::REPLACE,
                                target_atoms.as_ptr().cast(),
                                target_atoms.len() as i32,
                            );
                        } else if data.len() < self.max_request_size - 24 {
                            // ^ Taken from this line: https://github.com/quininer/x11-clipboard/blob/704cfd3ebf7297e4cd3b5ef00d2e2527e9b633f2/src/run.rs#L122
                            // I don't know why it's -24 specifically, but the Tronche guide does say this:
                            // "The size should be less than the maximum-request-size in the connection handshake".

                            (self.x.XChangeProperty)(
                                xevent.display,
                                xevent.requestor,
                                xevent.property,
                                xevent.target,
                                8,
                                prop_mode::REPLACE,
                                data.as_ptr().cast(),
                                data.len() as i32,
                            );
                        } else {
                            // change the attributes of the requestor window against its will (wtf)
                            (self.x.XSelectInput)(
                                xevent.display,
                                xevent.requestor,
                                xevent_mask::PROPERTY_CHANGE,
                            );

                            // send data incrementally
                            (self.x.XChangeProperty)(
                                xevent.display,
                                xevent.requestor,
                                xevent.property,
                                self.atoms.incr,
                                32,
                                prop_mode::REPLACE,
                                std::ptr::null(),
                                0,
                            );

                            incr_start_xevent = Some(xevent);
                        }
                    } else {
                        // Refuse conversion
                        xevent.property = 0;
                    }

                    let mut selection_event = XEvent {
                        xselection: XSelectionEvent {
                            type_id: et::SELECTION_NOTIFY,
                            serial: 0,
                            send_event: 1,
                            display: xevent.display,
                            requestor: xevent.requestor,
                            selection: xevent.selection,
                            target: xevent.target,
                            property: xevent.property,
                            time: xevent.time,
                        },
                    };

                    (self.x.XSendEvent)(
                        xevent.display,
                        xevent.requestor,
                        0,
                        0,
                        &mut selection_event,
                    );

                    (self.x.XFlush)(self.display.as_ptr());
                } else if xevent.type_id == et::PROPERTY_NOTIFY {
                    let xevent = xevent.xproperty;
                    if xevent.state != property::DELETE {
                        // Not a Delete - move on
                        continue;
                    }

                    let Some(xevent) = incr_start_xevent else {
                        // there's no incremental data to send
                        continue;
                    };

                    let incr_data_slice = {
                        let end = (incr_bytes_sent + INCR_CHUNK_SIZE).min(data.len());
                        &data[incr_bytes_sent..end]
                    };

                    if incr_data_slice.is_empty() {
                        incr_start_xevent = None;
                    }

                    (self.x.XChangeProperty)(
                        xevent.display,
                        xevent.requestor,
                        xevent.property,
                        xevent.target,
                        8,
                        prop_mode::REPLACE,
                        incr_data_slice.as_ptr().cast(),
                        incr_data_slice.len() as i32,
                    );

                    incr_bytes_sent += incr_data_slice.len();
                } else if xevent.type_id == et::SELECTION_CLEAR {
                    // No longer our selection \(=_= )\
                    return Ok(());
                }
            }
        }
    }
}

impl Drop for X11Clipboard {
    fn drop(&mut self) {
        unsafe {
            // Disconnect from the X server
            (self.x.XCloseDisplay)(self.display.as_ptr());
        }
    }
}
