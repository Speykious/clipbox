mod linux;

use std::error::Error;
use std::fs;

use crate::linux::x11::{atom_names, mime_types, X11Clipboard};

/// The main fuckery.
///
/// # Panics
///
/// Panics if something wrong happened.
///
/// # Errors
///
/// This function will return an error if something wrong happened. Don't ask what the difference with panics is.
///
/// # Safety
///
/// Safety? What's that?
pub unsafe fn main_fuckery() -> Result<(), Box<dyn Error>> {
    println!("[[Init X11 clipboard]]");
    let clipboard = X11Clipboard::init()?;

    println!("[[Getting selection]]");
    let selection = clipboard.get_selection(atom_names::CLIPBOARD, mime_types::IMAGE_PNG)?;

    println!("[[Writing image]]");
    fs::write("image.png", selection)?;

    Ok(())
}
