use std::error::Error;
use std::ffi::{c_int, c_ulong, CStr};
use std::ptr::{self, NonNull};

use loki_linux::x11::{
    errcode, et, prop_mode, xevent_mask, Atom, LibX11, XDisplay, XErrorEvent, XEvent,
};

unsafe extern "C" fn x11_error_handler(_display: *mut XDisplay, event: *mut XErrorEvent) -> i32 {
    if let Some(event) = event.as_ref() {
        eprintln!("X11: error (code {})", event.error_code);
    } else {
        eprintln!("X11 called the error handler without an error event or a display, somehow");
    }

    0
}

unsafe fn intern_atom(x11: &LibX11, display: NonNull<XDisplay>, name: &[u8]) -> Atom {
    (x11.XInternAtom)(display.as_ptr(), name.as_ptr() as _, 0)
}

unsafe fn get_atom_name(x11: &LibX11, display: NonNull<XDisplay>, atom: Atom) -> &CStr {
    CStr::from_ptr((x11.XGetAtomName)(display.as_ptr(), atom))
}

unsafe fn poll_event(x11: &LibX11, display: NonNull<XDisplay>) -> XEvent {
    let mut xevent = XEvent { type_id: 0 };
    (x11.XNextEvent)(display.as_ptr(), &mut xevent);
    xevent
}

const PROPERTY_BUFFER_LEN: usize = 8192;

unsafe fn main_fuckery() -> Result<(), Box<dyn Error>> {
    println!("Hello world!");

    let x11 = LibX11::new()?;

    (x11.XSetErrorHandler)(Some(x11_error_handler));

    // Open the default X11 display
    let display = (x11.XOpenDisplay)(std::ptr::null());
    let display = NonNull::new(display).ok_or("cannot open display :(")?;

    let root = (x11.XDefaultRootWindow)(display.as_ptr());

    // Create a window to trap events
    let window = (x11.XCreateSimpleWindow)(display.as_ptr(), root, 0, 0, 1, 1, 0, 0, 0);

    let atom_string = intern_atom(&x11, display, b"STRING\0");
    let atom_targets = intern_atom(&x11, display, b"TARGETS\0");

    // All your selections are belong to us
    let atom_primary = intern_atom(&x11, display, b"PRIMARY\0");
    let atom_secondary = intern_atom(&x11, display, b"SECONDARY\0");
    let atom_clipboard = intern_atom(&x11, display, b"CLIPBOARD\0");

    // Custom property atom to pass around
    let atom_clipbox = intern_atom(&x11, display, b"CLIPBOX\0");

    dbg!(atom_primary, atom_secondary, atom_clipboard, atom_clipbox);

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
            let xevent = poll_event(&x11, display);

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
        atom_clipboard,
        atom_string,
        atom_clipbox,
        window,
        when_everything_started,
    );

    // Receive selection from request
    {
        let xevent = loop {
            let xevent = poll_event(&x11, display);

            if xevent.type_id == et::SELECTION_NOTIFY {
                let xevent = xevent.xselection;

                if xevent.requestor == window
                    && xevent.selection == atom_clipboard
                    && xevent.target == atom_string
                {
                    break xevent;
                } else {
                    eprintln!("(why are we getting a selection that's not ours??)")
                }
            }
        };

        if xevent.property == 0 {
            panic!("HOUSTON WE LOST THE SELECTION D:");
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
        let mut prop: *const u8 = std::ptr::null();

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
            panic!("Error: Couldn't get property! D: (code {})", status);
        }

        dbg!(status, format, nitems, bytes_remaining);
        let total_len = (nitems * format as c_ulong / 8) as usize;

        &*ptr::slice_from_raw_parts(prop, total_len)
    };

    println!("prop: {:?}", std::str::from_utf8(prop).unwrap());

    // Disconnect from the X server
    (x11.XCloseDisplay)(display.as_ptr());

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    unsafe { main_fuckery() }
}
