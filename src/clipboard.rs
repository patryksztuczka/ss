//! The general pasteboard. Images go on as the exact PNG bytes
//! (`public.png`); `arboard::set_image` would re-encode them as TIFF.

use std::path::Path;

use color_eyre::eyre::{Result, eyre};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypePNG};
use objc2_foundation::{NSData, NSString, NSURL};

/// Replaces the clipboard with a PNG.
pub fn write_png(bytes: &[u8]) -> Result<()> {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    let data = NSData::with_bytes(bytes);
    // SAFETY: NSPasteboardTypePNG is an immutable AppKit constant.
    let png_type = unsafe { NSPasteboardTypePNG };
    if pasteboard.setData_forType(Some(&data), png_type) {
        Ok(())
    } else {
        Err(eyre!("the pasteboard rejected the PNG"))
    }
}

/// Replaces the clipboard with plain text.
pub fn write_text(text: &str) -> Result<()> {
    arboard::Clipboard::new()?.set_text(text)?;
    Ok(())
}

/// Puts a file reference on the clipboard, to paste into Finder, Slack or
/// Mail (what `osascript … set the clipboard to POSIX file` produced). The
/// URL is written eagerly: `writeObjects` with an `NSURL` provides it lazily,
/// which is lost if the process exits first.
pub fn write_file_url(path: &Path) -> Result<()> {
    let path = path
        .to_str()
        .ok_or_else(|| eyre!("path is not valid UTF-8: {}", path.display()))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let url = url
        .absoluteString()
        .ok_or_else(|| eyre!("cannot form a file URL for {path}"))?;
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: NSPasteboardTypeFileURL is an immutable AppKit constant.
    let file_url = unsafe { NSPasteboardTypeFileURL };
    if pasteboard.setString_forType(&url, file_url) {
        Ok(())
    } else {
        Err(eyre!("the pasteboard rejected the file reference"))
    }
}
