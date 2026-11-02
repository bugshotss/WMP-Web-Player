use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--test-skins") {
        let (Some(source), Some(out)) = (args.get(1), args.get(2)) else {
            eprintln!("usage: webskin --test-skins <folder|skin> <out-folder>");
            std::process::exit(2);
        };
        match webskin::runner::run(&PathBuf::from(source), &PathBuf::from(out)) {
            Ok(s) => std::process::exit(if s.failed + s.faults > 0 { 1 } else { 0 }),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        // --snapshot <out> [skin]: walk through the app's windows and save a picture of each (for CI).
        let mut skin = args.iter().find(|a| !a.starts_with('-')).map(PathBuf::from);
        if args.first().map(String::as_str) == Some("--snapshot") {
            let Some(out) = args.get(1) else {
                eprintln!("usage: webskin --snapshot <out-folder> [skin]");
                std::process::exit(2);
            };
            webskin::mac::snapshot::configure(PathBuf::from(out), args.get(2).map(PathBuf::from));
            skin = None;
        }
        webskin::mac::run(skin);
    }
}
