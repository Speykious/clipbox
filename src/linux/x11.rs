use std::error::Error;
use std::ffi::{c_int, c_ulong, CStr};
use std::ptr::{self, NonNull};

use loki_linux::x11::{
    errcode, et, prop_mode, xevent_mask, Atom, LibX11, XDisplay, XErrorEvent, XEvent,
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

pub unsafe fn intern_atom(x11: &LibX11, display: NonNull<XDisplay>, name: &CStr) -> Atom {
    // let name = CString::new(name).expect("Hey! Don't put a nul char in the atom name >:v");
    (x11.XInternAtom)(display.as_ptr(), name.as_ptr() as _, 0)
}

pub unsafe fn get_atom_name(x11: &LibX11, display: NonNull<XDisplay>, atom: Atom) -> &CStr {
    CStr::from_ptr((x11.XGetAtomName)(display.as_ptr(), atom))
}

pub unsafe fn next_event(x11: &LibX11, display: NonNull<XDisplay>) -> XEvent {
    let mut xevent = XEvent { type_id: 0 };
    (x11.XNextEvent)(display.as_ptr(), &mut xevent);
    xevent
}

pub unsafe extern "C" fn x11_error_handler(
    _display: *mut XDisplay,
    event: *mut XErrorEvent,
) -> i32 {
    if let Some(event) = event.as_ref() {
        eprintln!("X11: error (code {})", event.error_code);
    } else {
        eprintln!("X11 called the error handler without an error event or a display, somehow");
    }

    0
}

pub const PROPERTY_BUFFER_LEN: usize = 8192;

pub unsafe fn get_selection_text(selection: &CStr) -> String {
    let bytes = get_selection(selection, atom_names::STRING);

    todo!()
}

pub unsafe fn get_selection(selection: &CStr, target: &CStr) -> Result<Box<[u8]>, Box<dyn Error>> {
    let x11 = LibX11::new()?;

    (x11.XSetErrorHandler)(Some(x11_error_handler));

    // Open the default X11 display
    let display = (x11.XOpenDisplay)(std::ptr::null());
    let display = NonNull::new(display).ok_or("cannot open display :(")?;

    let root = (x11.XDefaultRootWindow)(display.as_ptr());

    // Create a window to trap events
    let window = (x11.XCreateSimpleWindow)(display.as_ptr(), root, 0, 0, 1, 1, 0, 0, 0);

    let atom_selection = intern_atom(&x11, display, selection);
    let atom_target = intern_atom(&x11, display, target);
    let atom_clipbox = intern_atom(&x11, display, atom_names::CLIPBOX);

    // Select property change events
    (x11.XSelectInput)(display.as_ptr(), window, xevent_mask::PROPERTY_CHANGE);

    // Get a compliant timestamp for the selection request
    let when_everything_started = {
        // Send dummy change property request to obtain a timestamp from its resulting event
        // This is because it is disincentivized to use CurrentTime when sending a ConvertSelection request
        (x11.XChangeProperty)(
            display.as_ptr(),
            window,
            atom_clipbox,
            atom_clipbox,
            8,
            prop_mode::APPEND,
            std::ptr::null(),
            0,
        );

        loop {
            let xevent = next_event(&x11, display);

            if xevent.type_id == et::PROPERTY_NOTIFY {
                let xevent = xevent.xproperty;

                if xevent.atom == atom_clipbox {
                    break xevent.time;
                }
            }
        }
    };

    // Send a ConvertSelection request
    println!("Sending a convert selection request");
    (x11.XConvertSelection)(
        display.as_ptr(),
        atom_selection,
        atom_target,
        atom_clipbox,
        window,
        when_everything_started,
    );

    // Receive selection from request
    {
        let xevent = loop {
            let xevent = next_event(&x11, display);

            if xevent.type_id == et::SELECTION_NOTIFY {
                let xevent = xevent.xselection;

                if xevent.requestor == window
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

        if xevent.property != atom_clipbox {
            let property = get_atom_name(&x11, display, xevent.property);
            eprintln!("We got {:?} instead of \"CLIPBOX\"", property);
        }

        let selection = get_atom_name(&x11, display, xevent.selection);
        let target = get_atom_name(&x11, display, xevent.target);
        let property = get_atom_name(&x11, display, xevent.property);

        println!(
            "Selection notify at {}ms: s{:?} t{:?} p{:?}",
            xevent.time, selection, target, property
        );
    }

    // get property data in raw bytes
    let prop = {
        let mut ty: Atom = 0;
        let mut format: c_int = 8;
        let mut nitems: c_ulong = 0;
        let mut bytes_remaining: c_ulong = 0;
        let mut prop: *mut u8 = std::ptr::null_mut();

        let status = (x11.XGetWindowProperty)(
            display.as_ptr(),
            window,
            atom_clipbox,
            0,
            PROPERTY_BUFFER_LEN as i64,
            0,
            0,
            &mut ty,
            &mut format,
            &mut nitems,
            &mut bytes_remaining,
            &mut prop,
        );

        if status != errcode::SUCCESS {
            return Err(format!("Error: Couldn't get property! D: (code {})", status).into());
        }

        dbg!(status, format, nitems, bytes_remaining);

        let total_len = (nitems * format as c_ulong / 8) as usize;
        ptr::slice_from_raw_parts_mut(prop, total_len)
    };

    // Disconnect from the X server
    (x11.XCloseDisplay)(display.as_ptr());

    Ok(Box::from_raw(prop))
}
