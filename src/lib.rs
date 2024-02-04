#![allow(unused)]

mod linux;

use std::error::Error;
use std::fs;

use crate::linux::x11::{atom_names, mime_types, X11Clipboard};

const MYSELF: &[u8] = "hello I'm really new (I swear) UTF8 text: 日本語".as_bytes();

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

    // println!("[[Getting targets]]");
    // let targets = clipboard.get_targets(atom_names::CLIPBOARD)?;
    // dbg!(&targets);

    // println!("[[Getting selection]]");
    // if targets.contains(&mime_types::IMAGE_PNG) {
    //     let selection = clipboard.get_selection(atom_names::CLIPBOARD, mime_types::IMAGE_PNG)?;
    //     println!("[[Writing image]]");
    //     fs::write("image.png", selection)?;
    // } else {
    //     let selection = clipboard.get_selection(atom_names::CLIPBOARD, atom_names::STRING)?;
    //     println!("[[Writing text]]");
    //     fs::write("string.txt", selection)?;
    // }

    println!("[[Copying myself into clipboard]]");
    clipboard.set_selection(atom_names::CLIPBOARD, atom_names::UTF8_STRING, MYSELF)?;

    // println!("[[Copying image into clipboard]]");
    // clipboard.set_selection(atom_names::CLIPBOARD, mime_types::IMAGE_PNG, IMAGE)?;

    Ok(())
}
