//! Rasterises SVG files to PNG with resvg (anti-aliased, exact).
//!
//! Usage: manta-icon-render <in.svg> <out.png> <width> [height]
//!        manta-icon-render --batch < jobs.txt   (one "in out width [height]" per line)

use std::process::ExitCode;

fn render(input: &str, output: &str, w: u32, h: u32) -> Result<(), String> {
    let data = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default())
        .map_err(|e| format!("{input}: {e}"))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("bad size")?;
    let size = tree.size();
    let t = resvg::tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, t, &mut pixmap.as_mut());
    pixmap.save_png(output).map_err(|e| format!("{output}: {e}"))
}

fn job(parts: &[&str]) -> Result<(), String> {
    let (i, o, w) = match parts {
        [i, o, w, ..] => (*i, *o, w.parse::<u32>().map_err(|e| e.to_string())?),
        _ => return Err(format!("bad job: {parts:?}")),
    };
    let h = parts.get(3).map(|h| h.parse::<u32>()).transpose().map_err(|e| e.to_string())?.unwrap_or(w);
    render(i, o, w, h)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = if args.first().map(String::as_str) == Some("--batch") {
        std::io::read_to_string(std::io::stdin())
            .map_err(|e| e.to_string())
            .and_then(|s| {
                s.lines()
                    .filter(|l| !l.trim().is_empty())
                    .try_for_each(|l| job(&l.split_whitespace().collect::<Vec<_>>()))
            })
    } else {
        job(&args.iter().map(String::as_str).collect::<Vec<_>>())
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
