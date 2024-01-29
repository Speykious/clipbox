use std::error::Error;
use std::ffi::{c_int, c_ulong, c_void, CStr};
use std::fmt;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use loki_linux::x11::{
    errcode, et, prop_mode, xevent_mask, Atom, LibX11, XDisplay, XErrorEvent, XEvent,
    XSelectionEvent, XWindow,
};

/// Just a boilerplate function to construct a const `&CStr`.
/// This is not safe at all, but in my defense, it's not public...
///
/// # Safety
///
/// Make sure to append the `\0` at the end!!
const fn const_cstr(bytes: &[u8]) -> &CStr {
    unsafe { CStr::from_bytes_with_nul_unchecked(bytes) }
}

pub mod atom_names {
    use std::ffi::CStr;

    use super::const_cstr;

    /// The primary clipboard
    pub const PRIMARY: &CStr = const_cstr(b"PRIMARY\0");
    /// The secondary clipboard
    pub const SECONDARY: &CStr = const_cstr(b"SECONDARY\0");
    /// The actual clipboard that most apps use
    pub const CLIPBOARD: &CStr = const_cstr(b"CLIPBOARD\0");
    /// Our custom dummy atom
    pub const CLIPBOX: &CStr = const_cstr(b"CLIPBOX\0");

    /// Property type: ASCII string
    pub const STRING: &CStr = const_cstr(b"STRING\0");
    /// Property type: text
    pub const TEXT: &CStr = const_cstr(b"TEXT\0");
    /// Property type: UTF8 string
    pub const UTF8_STRING: &CStr = const_cstr(b"UTF8_STRING\0");
    /// Property type: targets (list of atoms)
    pub const TARGETS: &CStr = const_cstr(b"TARGETS\0");
}

/// Some commonly used mime types. They're literally infinite so the list cannot be exclusive.
pub mod mime_types {
    use std::ffi::CStr;

    use super::const_cstr;

    pub const TEXT_PLAIN: &CStr = const_cstr(b"text/plain\0");
    pub const TEXT_PLAIN_CHARSET_UTF8: &CStr = const_cstr(b"text/plain;charset=utf-8\0");
    pub const TEXT_HTML: &CStr = const_cstr(b"text/html\0");

    pub const IMAGE_PNG: &CStr = const_cstr(b"image/png\0");
    pub const IMAGE_JPG: &CStr = const_cstr(b"image/jpg\0");
    pub const IMAGE_JPEG: &CStr = const_cstr(b"image/jpeg\0");
}

#[derive(Debug)]
pub struct Atoms {
    /// The primary clipboard
    pub primary: Atom,
    /// The secondary clipboard
    pub secondary: Atom,
    /// The actual clipboard that most apps use
    pub clipboard: Atom,
    /// Our custom dummy atom
    pub clipbox: Atom,

    /// Property type: ASCII string
    pub string: Atom,
    /// Property type: text
    pub text: Atom,
    /// Property type: UTF8 string
    pub utf8_string: Atom,

    /// Property type: targets (list of atoms)
    pub targets: Atom,
}

pub unsafe extern "C" fn x11_error_handler(
    _display: *mut XDisplay,
    event: *mut XErrorEvent,
) -> i32 {
    match event.as_ref() {
        Some(event) => eprintln!("X11: error (code {})", event.error_code),
        None => {
            eprintln!("X11 called the error handler without an error event or a display, somehow")
        }
    }

    0
}

unsafe fn intern_atom(x: &LibX11, display: NonNull<XDisplay>, name: &CStr) -> Atom {
    (x.XInternAtom)(display.as_ptr(), name.as_ptr() as _, 0)
}

unsafe fn get_atom_name(x: &LibX11, display: NonNull<XDisplay>, atom: Atom) -> &CStr {
    CStr::from_ptr((x.XGetAtomName)(display.as_ptr(), atom))
}

struct XWindowProperty<'a> {
    // This is here to make sure we free the prop when dropping
    x11: &'a LibX11,

    pub ty: Atom,
    pub format: c_int,
    pub nitems: c_ulong,
    pub bytes_remaining: c_ulong,
    pub prop: NonNull<c_void>,
}

impl<'a> XWindowProperty<'a> {
    /// Checks that the format size is compatible with the size of `T`
    fn check_format_compatible<T>(&self) -> Result<(), Box<dyn Error>> {
        match self.format as usize == 8 * std::mem::size_of::<T>() {
            true => Ok(()),
            false => Err(format!("Invalid format ({} bits instead of 8).", self.format).into()),
        }
    }

    /// Converts this property into a vec
    fn into_vec<T>(self) -> Result<Vec<T>, Box<dyn Error>> {
        self.check_format_compatible::<T>()?;

        let n_items = self.nitems as usize;

        let mut prop = Vec::with_capacity(n_items);
        unsafe {
            // SAFETY: we trust Xlib that the source is aligned and valid,
            // and `Vec::with_capacity` ensures that we have usable space to write them.
            ptr::copy_nonoverlapping(
                self.prop.as_ptr().cast::<T>().cast_const(),
                prop.as_mut_ptr(),
                n_items,
            );

            // SAFETY: We created it with this much capacity earlier,
            // and the previous `copy` has initialized these elements.
            prop.set_len(n_items);
        }

        Ok(prop)
    }
}

impl<'a> Drop for XWindowProperty<'a> {
    fn drop(&mut self) {
        unsafe {
            // The data is free \o/
            (self.x11.XFree)(self.prop.as_ptr());
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

pub struct X11Clipboard {
    x: LibX11,
    display: NonNull<XDisplay>,
    window: XWindow,
    atoms: Atoms,
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

            let atoms = Atoms {
                primary: intern_atom(&x, display, atom_names::PRIMARY),
                secondary: intern_atom(&x, display, atom_names::SECONDARY),
                clipboard: intern_atom(&x, display, atom_names::CLIPBOARD),
                clipbox: intern_atom(&x, display, atom_names::CLIPBOX),
                string: intern_atom(&x, display, atom_names::STRING),
                text: intern_atom(&x, display, atom_names::TEXT),
                utf8_string: intern_atom(&x, display, atom_names::UTF8_STRING),
                targets: intern_atom(&x, display, atom_names::TARGETS),
            };

            Ok(Self {
                x,
                display,
                window,
                atoms,
            })
        }
    }

    unsafe fn next_event(&self) -> XEvent {
        let mut xevent = XEvent { type_id: 0 };
        (self.x.XNextEvent)(self.display.as_ptr(), &mut xevent);
        xevent
    }

    /// Get a compliant timestamp for selection requests
    unsafe fn get_compliant_timestamp(&self) -> c_ulong {
        // Select property change events
        (self.x.XSelectInput)(
            self.display.as_ptr(),
            self.window,
            xevent_mask::PROPERTY_CHANGE,
        );

        // Send dummy change property request to obtain a timestamp from its resulting event
        // This is because it is disincentivized to use CurrentTime when sending a ConvertSelection request
        (self.x.XChangeProperty)(
            self.display.as_ptr(),
            self.window,
            self.atoms.clipbox,
            self.atoms.clipbox,
            8,
            prop_mode::APPEND,
            std::ptr::null(),
            0,
        );

        loop {
            let xevent = self.next_event();

            if xevent.type_id == et::PROPERTY_NOTIFY {
                let xevent = xevent.xproperty;

                if xevent.atom == self.atoms.clipbox {
                    return xevent.time;
                }
            }
        }
    }

    unsafe fn get_selection_event(
        &self,
        atom_selection: Atom,
        atom_target: Atom,
    ) -> Result<XSelectionEvent, Box<dyn Error>> {
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
                    return Err("(why are we getting a selection that's not ours??)".into());
                }
            }
        };

        if xevent.property == 0 {
            return Err("HOUSTON WE LOST THE SELECTION D:".into());
        }

        if xevent.property != self.atoms.clipbox {
            let property = get_atom_name(&self.x, self.display, xevent.property);
            eprintln!("We got {:?} instead of \"CLIPBOX\"", property);
        }

        let selection = get_atom_name(&self.x, self.display, xevent.selection);
        let target = get_atom_name(&self.x, self.display, xevent.target);
        let property = get_atom_name(&self.x, self.display, xevent.property);

        eprintln!(
            "Selection notify at {}ms: s{:?} t{:?} p{:?}",
            xevent.time, selection, target, property
        );

        Ok(xevent)
    }

    fn get_clipbox_property(&self) -> Result<XWindowProperty, Box<dyn Error>> {
        const PROPERTY_BUFFER_LEN: i64 = 8192;

        let mut ty: Atom = 0;
        let mut format: c_int = 8;
        let mut nitems: c_ulong = 0;
        let mut bytes_remaining: c_ulong = 0;
        let mut prop: *mut c_void = std::ptr::null_mut();

        let status = unsafe {
            (self.x.XGetWindowProperty)(
                self.display.as_ptr(),
                self.window,
                self.atoms.clipbox,
                0,
                PROPERTY_BUFFER_LEN,
                0,
                0,
                &mut ty,
                &mut format,
                &mut nitems,
                &mut bytes_remaining,
                &mut prop,
            )
        };

        if status != errcode::SUCCESS {
            return Err(format!("Error: Couldn't get property! D: (code {})", status).into());
        }

        let Some(prop) = NonNull::new(prop) else {
            return Err("Wdym there's no data??".into());
        };

        Ok(XWindowProperty {
            x11: &self.x,
            ty,
            format,
            nitems,
            bytes_remaining,
            prop,
        })
    }

    pub fn get_selection(
        &self,
        selection: &CStr,
        target: &CStr,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        if target == atom_names::TARGETS {
            let emsg = concat!(
                "TARGETS is a special selection target, this method doesn't support it.",
                "Try `X11Clipboard::get_targets` instead!"
            );
            return Err(emsg.into());
        }

        unsafe {
            let atom_selection = intern_atom(&self.x, self.display, selection);
            let atom_target = intern_atom(&self.x, self.display, target);
            self.get_selection_event(atom_selection, atom_target)?
        };

        let clipbox_prop = self.get_clipbox_property()?;
        dbg!(&clipbox_prop);

        clipbox_prop.into_vec()
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
