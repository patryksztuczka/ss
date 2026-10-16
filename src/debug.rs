//! `ss debug …`: headless developer tools, available in debug builds only.

use std::fs;

use color_eyre::eyre::{Result, eyre};

use crate::accelerator;
use crate::capture_store::{new_id, png_size};
use crate::displays::{self, Display, Rect};
use crate::screencapture;
use crate::settings::{Settings, is_usable_accelerator};

pub fn default_settings() {
    println!("{}", Settings::default().to_json());
}

/// Captures the display under the cursor to a temp file and copies it.
pub fn capture_screen() -> Result<()> {
    let display = displays::under_cursor()?;
    let file = std::env::temp_dir().join(format!("ss-debug-{}.png", new_id()));
    screencapture::rect(&file, display.bounds)?;
    let png = fs::read(&file)?;
    crate::clipboard::write_png(&png)?;
    let (width, height) =
        png_size(&png).ok_or_else(|| eyre!("screencapture wrote a non-PNG file"))?;
    println!("{}", file.display());
    println!(
        "{width}×{height} px, {} bytes, display {}",
        png.len(),
        rect(display.bounds)
    );
    println!("copied to the clipboard as public.png");
    Ok(())
}

pub fn displays() -> Result<()> {
    let all = displays::all()?;
    let current = displays::under_cursor()?;
    for (index, display) in all.iter().enumerate() {
        let Display {
            bounds,
            work_area,
            number,
        } = display;
        let marker = if *display == current {
            "  ← cursor"
        } else {
            ""
        };
        let role = if index == 0 { " (primary)" } else { "" };
        println!("display {index}{role} (id {number}){marker}");
        println!("  bounds:    {}", rect(*bounds));
        println!("  work area: {}", rect(*work_area));
    }
    Ok(())
}

pub fn setup_state() {
    let state = crate::system::setup_state();
    println!(
        "bundled:                      {}",
        crate::system::is_bundled()
    );
    println!(
        "screen access:                {}",
        state.screen_access.as_str()
    );
    println!(
        "macOS screenshot shortcuts:   {}",
        if state.system_screenshot_shortcuts {
            "on"
        } else {
            "off"
        }
    );
    println!("launch at login:              {:?}", state.launch_at_login);
    println!("permission host:              {:?}", state.permission_host);
}

pub fn ocr(png: &std::path::Path, compare: Option<&std::path::Path>) -> Result<()> {
    let started = std::time::Instant::now();
    let mut layout = crate::ocr::recognize(png)?;
    crate::ocr::sort_lines(&mut layout);
    println!(
        "{}×{} px, {} lines in {:.2} s",
        layout.width,
        layout.height,
        layout.lines.len(),
        started.elapsed().as_secs_f64()
    );
    let Some(reference) = compare else {
        for line in &layout.lines {
            println!("  {}", line.text);
        }
        return Ok(());
    };
    let reference: crate::ocr::TextLayout = serde_json::from_slice(&fs::read(reference)?)?;
    match crate::ocr::compare(&layout, &reference) {
        Ok(worst) => println!(
            "matches {} lines and {} words; largest box difference {worst:e}",
            reference.lines.len(),
            reference
                .lines
                .iter()
                .map(|line| line.words.len())
                .sum::<usize>()
        ),
        Err(difference) => println!("differs: {difference}"),
    }
    Ok(())
}

pub fn compose(
    paths: &crate::paths::Paths,
    png: &std::path::Path,
    out: Option<&std::path::Path>,
    copy: bool,
) -> Result<()> {
    let style = crate::settings::read(&paths.settings_file).settings.editor;
    let started = std::time::Instant::now();
    let source = crate::compose::Source::from_png(&fs::read(png)?)?;
    let edge = crate::compose::detect_edge_color(&source);
    println!(
        "{}×{} px, edge colour {}",
        source.width(),
        source.height(),
        crate::compose::hex_color(edge)
    );
    if copy {
        let export = crate::compose::export_png(&source, &style, edge)?;
        crate::clipboard::write_png(&export)?;
        println!(
            "copied {} bytes to the clipboard as public.png",
            export.len()
        );
    }
    if let Some(out) = out {
        let export = crate::compose::export_png(&source, &style, edge)?;
        fs::write(out, &export)?;
        println!(
            "wrote {} ({} bytes) in {:.2} s",
            out.display(),
            export.len(),
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

pub fn record_screen(seconds: u64, out: &std::path::Path) -> Result<()> {
    use crate::capture_store::{Capture, CaptureKind};
    use crate::recorder::Recorder;

    let display = displays::under_cursor()?;
    let recorder = Recorder::new(Default::default());
    let capture = Capture {
        id: new_id(),
        kind: CaptureKind::Video,
        path: out.to_path_buf(),
    };
    let finished = recorder.start(capture, |path| screencapture::record(path, display.bounds))?;
    println!("recording {} for {seconds} s…", rect(display.bounds));
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    let started = std::time::Instant::now();
    recorder.stop();
    let finished = futures::executor::block_on(finished)?;
    let bytes = fs::metadata(out)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    println!(
        "stopped in {:.2} s: exit {}, file {} ({bytes} bytes)",
        started.elapsed().as_secs_f64(),
        finished.exit_code,
        if finished.produced_file {
            "written"
        } else {
            "missing"
        },
    );
    Ok(())
}

pub fn poster(movie: &std::path::Path, out: &std::path::Path) -> Result<()> {
    let poster = crate::thumbnail::video_poster(movie, crate::thumbnail::PREVIEW_THUMBNAIL_WIDTH)?;
    poster.save(out)?;
    println!(
        "{}×{} poster at {}",
        poster.width(),
        poster.height(),
        out.display()
    );
    Ok(())
}

pub fn parse_accelerator(text: &str) -> Result<()> {
    let parsed = accelerator::parse(text)?;
    println!("accelerator:    {text}");
    println!("display:        {}", accelerator::format_accelerator(text));
    println!("usable:         {}", is_usable_accelerator(text));
    println!("modifiers:      {:?}", parsed.modifiers);
    println!("carbon mask:    0x{:04x}", parsed.modifiers.carbon());
    println!("key code:       0x{:02x}", parsed.key_code);
    println!("menu key:       {:?}", parsed.key_equivalent);
    Ok(())
}

fn rect(rect: Rect) -> String {
    format!(
        "x {} y {} w {} h {}",
        rect.x, rect.y, rect.width, rect.height
    )
}
