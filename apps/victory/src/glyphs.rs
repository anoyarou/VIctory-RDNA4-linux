//! `victory glyphs`: PlayStation button prompts (and, with `--xelu`, the
//! Settings controller picture) built from the user's own copy, written as a
//! `VICTORY_FILE_PATCH` list into the cache folder. Nothing of the title is
//! stored in the repository; see `victory_game::prompt_glyphs` and
//! `victory_game::settings_diagram`.

use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use victory_formats::{swf, zlib_exact};
use victory_game::{prompt_glyphs, settings_diagram};

/// The archive holding the prompt movie, and the Settings picture's file,
/// relative to the game folder.
const ARCHIVE: &str = "data.rpf";
const DIAGRAM: &str = "data/ttdb/ui/platform.ttpf";

pub fn run(game: Option<PathBuf>, xelu: Option<PathBuf>, out: Option<PathBuf>) -> Result<()> {
    let cfg = victory_core::Config::load()?;
    let game = match game {
        Some(g) => g,
        None => cfg.require_game_dir()?.to_path_buf(),
    };
    let out = out.unwrap_or_else(|| cfg.cache_dir.join("file-patches").join("playstation"));
    victory_core::ensure_outside_repo(&out)?;
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    let mut list = String::from(
        "# VICTORY_FILE_PATCH list made by `victory glyphs` from this copy of the game (keep it local).\n\
         # <file name> <offset> <blake3 of the original range> <new bytes>\n",
    );

    let archive = game.join(ARCHIVE);
    println!("looking for the prompt icons in {} ...", archive.display());
    let found = find_prompt_movies(&archive)?;
    if found.is_empty() {
        bail!("no prompt-icon movie found in {}", archive.display());
    }
    for (offset, stream_len, movie) in found {
        let new = prompt_glyphs::rebuild(&movie).map_err(anyhow::Error::msg)?;
        let stream = prompt_glyphs::pack(&new, stream_len).map_err(anyhow::Error::msg)?;
        let original = read_range(&archive, offset, stream_len)?;
        let name = format!("prompt-icons-{offset:x}.bin");
        std::fs::write(out.join(&name), &stream)?;
        writeln!(
            list,
            "{ARCHIVE} {offset:#x} {} {name}",
            blake3::hash(&original).to_hex()
        )?;
        println!("  prompt icons at {offset:#x} ({stream_len} bytes): rebuilt");
    }

    match xelu {
        Some(dir) => {
            let file = game.join(DIAGRAM);
            let bytes =
                std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
            let (offset, stream_len) = settings_diagram::find_stream(&bytes)
                .context("the Settings picture's texture was not found")?;
            let pic = settings_diagram::compose(
                &load_png(&dir.join("PS5_Diagram.png"))?,
                &load_png(&dir.join("PS5_Diagram_Simple.png"))?,
            )
            .map_err(anyhow::Error::msg)?;
            let tiled = settings_diagram::texture(&pic).map_err(anyhow::Error::msg)?;
            let stream = settings_diagram::pack(&tiled, stream_len).map_err(anyhow::Error::msg)?;
            let name = "settings-controller.bin";
            std::fs::write(out.join(name), &stream)?;
            let file_name = Path::new(DIAGRAM)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(DIAGRAM);
            writeln!(
                list,
                "{file_name} {offset:#x} {} {name}",
                blake3::hash(&bytes[offset..offset + stream_len]).to_hex()
            )?;
            println!(
                "  Settings controller picture ({stream_len} bytes): rebuilt from {}",
                dir.display()
            );
        }
        None => println!(
            "  Settings controller picture: skipped (pass --xelu <folder with Xelu's PS5_Diagram*.png>)"
        ),
    }

    let list_path = out.join("patches.txt");
    std::fs::write(&list_path, list)?;
    println!(
        "\nwritten: {}\nto use it: set VICTORY_FILE_PATCH={}",
        list_path.display(),
        list_path.display()
    );
    Ok(())
}

fn read_range(path: &Path, offset: usize, len: usize) -> Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset as u64))?;
    let mut buf = vec![0; len];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// Every zlib stream (at a 16-byte step) in `archive` that inflates to the
/// prompt-icon movie: its offset, stream length and the movie.
fn find_prompt_movies(archive: &Path) -> Result<Vec<(usize, usize, Vec<u8>)>> {
    const CHUNK: usize = 32 << 20;
    const READ_AHEAD: usize = 4 << 20;
    let mut f =
        std::fs::File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let size = f.metadata()?.len() as usize;
    let mut found = Vec::new();
    let mut buf = vec![0u8; CHUNK + 64];
    let mut base = 0usize;
    while base < size {
        let n = (CHUNK + 64).min(size - base);
        f.seek(SeekFrom::Start(base as u64))?;
        f.read_exact(&mut buf[..n])?;
        for at in (0..n.min(CHUNK)).step_by(16) {
            if buf[at] != 0x78 {
                continue;
            }
            let Some(head) = zlib_exact::inflate_prefix(&buf[at..n], 16) else {
                continue;
            };
            if head.len() < 16 || !swf::is_movie(&head) {
                continue;
            }
            let len = swf::declared_len(&head).unwrap_or(0);
            let stream = read_range(archive, base + at, READ_AHEAD.min(size - base - at))?;
            if let Some(inflated) = zlib_exact::inflate(&stream, len)
                && prompt_glyphs::is_prompt_movie(&inflated.data)
            {
                found.push((base + at, inflated.stream_len, inflated.data));
            }
        }
        base += CHUNK;
    }
    Ok(found)
}

fn load_png(path: &Path) -> Result<settings_diagram::Rgba> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info()?;
    let mut px = vec![0; reader.output_buffer_size().context("image too large")?];
    let info = reader.next_frame(&mut px)?;
    px.truncate(info.buffer_size());
    if info.color_type != png::ColorType::Rgba {
        bail!("{}: expected an RGBA picture", path.display());
    }
    Ok(settings_diagram::Rgba {
        width: info.width as usize,
        height: info.height as usize,
        px,
    })
}
