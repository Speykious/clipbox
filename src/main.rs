use std::error::Error;
use std::ffi::CStr;
use std::ptr::NonNull;

use loki_linux::x11::{et, prop_mode, xevent_mask, Atom, LibX11, XDisplay, XErrorEvent, XEvent};

unsafe extern "C" fn x11_error_handler(_display: *mut XDisplay, event: *mut XErrorEvent) -> i32 {
    if let Some(event) = event.as_ref() {
        println!("X11: error (code {})", event.error_code);
    } else {
        println!("X11 called the error handler without an error event or a display, somehow");
    }

    0
}

unsafe fn intern_atom(x11: &LibX11, display: NonNull<XDisplay>, name: &[u8]) -> Atom {
    (x11.XInternAtom)(display.as_ptr(), name.as_ptr() as _, 0)
}

unsafe fn get_atom_name(x11: &LibX11, display: NonNull<XDisplay>, atom: Atom) -> &CStr {
    CStr::from_ptr((x11.XGetAtomName)(display.as_ptr(), atom))
}

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

    // Register interest in the delete window message
    let wm_delete_message = intern_atom(&x11, display, b"WM_DELETE_WINDOW\0");
    (x11.XSetWMProtocols)(display.as_ptr(), window, &wm_delete_message, 1);

    // All your selections are belong to us
    let selec_primary = intern_atom(&x11, display, b"PRIMARY\0");
    let selec_secondary = intern_atom(&x11, display, b"SECONDARY\0");
    let selec_clipboard = intern_atom(&x11, display, b"CLIPBOARD\0");
    dbg!(selec_primary, selec_secondary, selec_clipboard);

    // Select property change events
    (x11.XSelectInput)(display.as_ptr(), window, xevent_mask::PROPERTY_CHANGE);

    (x11.XChangeProperty)(
        display.as_ptr(),
        window,
        selec_primary,
        selec_primary,
        8,
        prop_mode::APPEND,
        std::ptr::null(),
        0,
    );

    let mut selected = false;
    let mut request_time = 0;

    // do the actual thing here
    'actual_thing: loop {
        let count = (x11.XPending)(display.as_ptr());

        // get all pending events or wait for the next one
        for _ in 0..count.max(1) {
            let mut xevent = XEvent { type_id: 0 };
            (x11.XNextEvent)(display.as_ptr(), &mut xevent);

            match xevent.type_id {
                et::PROPERTY_NOTIFY => {
                    let xevent = xevent.xproperty;

                    let atom_name = get_atom_name(&x11, display, xevent.atom);
                    println!("Property {:?} changed at {}ms", atom_name, xevent.time);

                    if !selected {
                        println!("setting selection owner");

                        // become owner of selection
                        (x11.XSetSelectionOwner)(
                            display.as_ptr(),
                            selec_clipboard,
                            window,
                            xevent.time,
                        );

                        // verify that we did indeed become owner of selection
                        let owner = (x11.XGetSelectionOwner)(display.as_ptr(), selec_clipboard);
                        if owner != window {
                            // \(T-T)/
                            println!("You will own nothing, and you will be happy >:3c");
                        } else {
                            println!("OUR selection /( =_=)/");
                        }

                        println!("owner set!");
                        selected = true;
                    }
                }

                et::SELECTION_REQUEST => {
                    let xevent = xevent.xselectionrequest;

                    let selection = get_atom_name(&x11, display, xevent.selection);
                    let target = get_atom_name(&x11, display, xevent.target);
                    let property = get_atom_name(&x11, display, xevent.property);

                    request_time = xevent.time;
                    println!(
                        "Selection request at {}ms: s{:?} t{:?} p{:?}",
                        request_time, selection, target, property
                    );

                    // println!("Sending a convert selection request");
                    (x11.XConvertSelection)(
                        display.as_ptr(),
                        xevent.selection,
                        xevent.target,
                        xevent.property,
                        window,
                        request_time,
                    );
                }

                et::SELECTION_NOTIFY => {
                    let xevent = xevent.xselection;

                    let selection = get_atom_name(&x11, display, xevent.selection);
                    let target = get_atom_name(&x11, display, xevent.target);
                    let property = get_atom_name(&x11, display, xevent.property);

                    println!(
                        "Selection notify at {}ms: s{:?} t{:?} p{:?}",
                        xevent.time, selection, target, property
                    );
                }

                _ => break 'actual_thing,
            }
        }
    }

    // Disconnect from the X server
    (x11.XCloseDisplay)(display.as_ptr());

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    unsafe { main_fuckery() }
}
