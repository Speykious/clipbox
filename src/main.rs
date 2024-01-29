use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    unsafe { clipbox::main_fuckery() }
}
