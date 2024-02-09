<p align="center">
  <h1 align="center">Clipbox</h1>
  <div align="center">

📋 **[WIP]** A cross-platform clipboard library written in Rust. 🦀
    &nbsp;
  </div>
</p>

## Why do this?

There are a lot of clipboard libraries written in Rust already, but I still wanted to make one. Part of why is because someone I know tells me they're all either bad or incomplete. The other is that I wanted to contribute a Linux clipboard to osu!lazer that can actually copy images, which is an important ergonomic when taking screenshots in-game.

## Status

**Heavily WIP**.

Half of a platform works right now (X11). The priority is to get Linux fully working on both X11 and Wayland, then move on to other platforms. I suspect the other platforms are gonna be drastically easier to implement though.

- [ ] Cross-platform Rust API
- [ ] FFI bindings
- [x] Mimetype support for pasting
- [x] Mimetype support for copying
- [ ] Multi-mimetype copying (pain)
- [ ] Complete platform support
  - [ ] Linux
    - [x] X11
    - [ ] Wayland
  - [ ] MacOS
  - [ ] Windows

## Multi-mimetype copy? (not yet)

Some applications will copy text in multiple formats, which can be useful. For example, VSCode will copy code both as plain text and HTML. I use Mailspring as an email client, and when I paste in some code from there into an email I'm writing, it's all colored according to my VSCode theme and in monospace. This is achievable by having multiple mime targets in the same selection, something I tried but utterly failed to achieve (for some reason KDE's clipboard manager just doesn't ask for other mimetypes, which is really weird considering that VSCode (ultimately, Gtk) can do it).

Ultimately though we can copy text and images without this feature, so it's more of an exploration right now.

## License

This library is licensed under the MIT license.
