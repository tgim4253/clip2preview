use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process;

use clip2preview::{Preview, extract_preview};

const USAGE: &str = "\
Usage:
  clip2preview <input.clip> [output]

Examples:
  clip2preview artwork.clip
  clip2preview artwork.clip preview.png";

fn main() {
    match run(env::args_os()) {
        Ok(output_path) => {
            println!("{}", output_path.display());
        }
        Err(message) => {
            eprintln!("{message}");
            process::exit(2);
        }
    }
}

fn run<I>(args: I) -> Result<PathBuf, String>
where
    I: IntoIterator<Item = OsString>,
{
    let cli = parse_args(args)?;
    let preview = extract_preview(&cli.input).map_err(|error| {
        format!(
            "failed to extract preview from {}: {error}",
            cli.input.display()
        )
    })?;

    let output_path = cli
        .output
        .unwrap_or_else(|| default_output_path(&cli.input, &preview));

    preview
        .save(&output_path)
        .map_err(|error| format!("failed to write {}: {error}", output_path.display()))?;

    print_summary(&output_path, &preview);
    Ok(output_path)
}

#[derive(Debug, PartialEq, Eq)]
struct CliArgs {
    input: PathBuf,
    output: Option<PathBuf>,
}

fn parse_args<I>(args: I) -> Result<CliArgs, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let _program = args.next();

    let input = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| USAGE.to_string())?;
    let output = args.next().map(PathBuf::from);

    if args.next().is_some() {
        return Err(USAGE.to_string());
    }

    Ok(CliArgs { input, output })
}

fn default_output_path(input: &Path, preview: &Preview) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let mut file_name = input
        .file_stem()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("preview"));
    file_name.push(OsStr::new(".preview."));
    file_name.push(OsStr::new(preview.format().extension()));
    parent.join(file_name)
}

fn print_summary(output_path: &Path, preview: &Preview) {
    let dimensions = preview
        .dimensions()
        .map(|(width, height)| format!("{width}x{height}"))
        .unwrap_or_else(|| "unknown-size".to_string());

    eprintln!(
        "saved {} ({}, {}, {} bytes)",
        output_path.display(),
        dimensions,
        preview.format().media_type(),
        preview.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_accepts_input_only() {
        let parsed = parse_args([
            OsString::from("clip2preview"),
            OsString::from("sample.clip"),
        ])
        .unwrap();

        assert_eq!(
            parsed,
            CliArgs {
                input: PathBuf::from("sample.clip"),
                output: None,
            }
        );
    }

    #[test]
    fn parse_args_accepts_explicit_output() {
        let parsed = parse_args([
            OsString::from("clip2preview"),
            OsString::from("sample.clip"),
            OsString::from("preview.png"),
        ])
        .unwrap();

        assert_eq!(
            parsed,
            CliArgs {
                input: PathBuf::from("sample.clip"),
                output: Some(PathBuf::from("preview.png")),
            }
        );
    }

    #[test]
    fn parse_args_rejects_missing_input() {
        let error = parse_args([OsString::from("clip2preview")]).unwrap_err();
        assert_eq!(error, USAGE);
    }

    #[test]
    fn parse_args_rejects_extra_arguments() {
        let error = parse_args([
            OsString::from("clip2preview"),
            OsString::from("sample.clip"),
            OsString::from("preview.png"),
            OsString::from("extra"),
        ])
        .unwrap_err();

        assert_eq!(error, USAGE);
    }

    #[test]
    fn default_output_path_uses_preview_extension() {
        let preview = Preview::new(clip2preview::PreviewFormat::Png, vec![1, 2, 3]);
        let output = default_output_path(Path::new("/tmp/sample.clip"), &preview);

        assert_eq!(output, PathBuf::from("/tmp/sample.preview.png"));
    }
}
