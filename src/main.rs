use anyhow::{anyhow, Result};
use semver::VersionReq;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use yarn_why::records::iter_flat_dependencies;
use yarn_why::{
    parse_lockfile, Format, Options, Report, MAX_DEPTH_DEFAULT, MAX_PKG_VISITS_DEFAULT,
};

const HELP: &str = concat!(
    "yarn-why ",
    env!("CARGO_PKG_VERSION"),
    r#"

Usage:
    yarn-why [OPTIONS] package [range] # read ./yarn.lock
    yarn-why [OPTIONS] package [range] < cat /path/to/yarn.lock
    yarn-why [OPTIONS] package [range] -y /path/to/yarn.lock

Example:
    yarn-why lodash 4.17.15
    yarn-why lodash ^4.17.0
    yarn-why lodash '>=4.0, <5.0'

OPTIONS:
    -d, --max-depth [depth]  Truncate dependencies at that level [Default: 10]
    -D, --no-max-depth       Ignore max-depth (both default and user defined)
        --(no)-dedup         Display packages at most once [Default: true]
    -j, --json               Format the output as JSON
    -h, --help               Prints this help and exit
    -V, --version            Prints version information
    -y, --yarn-lock-file     Path to a yarn.lock file to parse
        --print-records      Prints every dependency as JSONL
        --full-tree          Render the full tree of dependencies

ARGS:
    package[@range]          Package to search for, with or without range.
                             The range must match one in yarn.lock

LICENSE: GPL-3.0-or-later
"#
);

#[derive(Debug)]
struct Opt {
    version: bool,
    json: bool,
    max_depth: Option<usize>,
    dedup: bool,
    no_max_depth: bool,
    query: Option<String>,
    yarn_lock_path: Option<PathBuf>,
    filter: Option<VersionReq>,
    print_records: bool,
    full_tree: bool,
}

fn parse_path(s: &std::ffi::OsStr) -> Result<std::path::PathBuf, &'static str> {
    Ok(s.into())
}

fn main() -> Result<()> {
    let mut pargs = pico_args::Arguments::from_env();

    if pargs.contains(["-h", "--help"]) {
        print!("{HELP}");
        std::process::exit(0);
    }

    #[allow(unused_assignments)]
    let mut dedup: bool = pargs.contains(["-X", "--dedup"]);
    dedup = !pargs.contains(["-N", "--no-dedup"]);

    let mut args = Opt {
        version: pargs.contains(["-V", "--version"]),
        json: pargs.contains(["-j", "--json"]),
        dedup,
        no_max_depth: pargs.contains(["-D", "--no-max-depth"]),
        max_depth: pargs
            .opt_value_from_str(["-d", "--max-depth"])?
            .or(Some(MAX_DEPTH_DEFAULT)),
        yarn_lock_path: pargs.opt_value_from_os_str(["-y", "--yarn-lock-path"], parse_path)?,
        print_records: pargs.contains("--print-records"),
        full_tree: pargs.contains("--full-tree"),
        query: pargs.free_from_str().ok(),
        filter: pargs
            .opt_free_from_str::<String>()?
            .as_deref()
            .map(VersionReq::parse)
            .transpose()?,
    };

    if args.print_records || args.full_tree {
        // Set a dummy query, won't be used.
        // It's an hack until I find the time and will to refactor
        // the code to better separate print_records.
        args.query = Some("!dummy!".into());
    }

    let remaining = pargs.finish();

    if !remaining.is_empty() {
        eprintln!("Error: unexpected arguments {remaining:?}");
        eprintln!(
            "Try '{} --help' for more information.",
            env!("CARGO_PKG_NAME")
        );
        std::process::exit(1);
    }

    if args.version {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let max_pkg_visits = match std::env::var("MAX_PKG_VISITS") {
        Ok(v) => v.parse::<usize>().expect("MAX_PKG_VISITS is not a number"),
        Err(_) => MAX_PKG_VISITS_DEFAULT,
    };

    let is_stdin_tty = std::io::stdin().is_terminal();

    let query = {
        if args.query.is_none() {
            print!("{HELP}");
            std::process::exit(1);
        }
        &args.query.unwrap()
    };

    let mut yarn_lock_text: Vec<u8> = Vec::new();

    let must_read_yarn_lock = args.yarn_lock_path.is_some();
    let yarn_lock_path = if let Some(path) = args.yarn_lock_path {
        path
    } else {
        PathBuf::from("yarn.lock")
    };

    if must_read_yarn_lock || is_stdin_tty {
        let mut f = std::fs::File::open(yarn_lock_path)
            .map_err(|e| anyhow!("Cannot open yarn.lock: {}", e))?;
        f.read_to_end(&mut yarn_lock_text)?;
    } else {
        let stdin = std::io::stdin();
        let mut stdin = std::io::BufReader::with_capacity(32 * 1024, stdin.lock());
        stdin.read_to_end(&mut yarn_lock_text)?;
    }

    let stdout = std::io::stdout();
    let mut stdout = std::io::BufWriter::with_capacity(32 * 1024, stdout.lock());
    let yarn_lock_text = std::str::from_utf8(&yarn_lock_text)?;

    if args.print_records {
        let entries = parse_lockfile(yarn_lock_text, query, None)?;
        print_records(&mut stdout, &entries)?;
        return Ok(());
    }

    let opts = Options {
        // A bit convoluted, but allow us to have both a sensible default
        // and yet let users ask to go all the way down.
        max_depth: if args.no_max_depth {
            None
        } else {
            args.max_depth
        },
        dedup: args.dedup,
        full_tree: args.full_tree,
        range: args.filter,
        color: is_stdin_tty,
        max_pkg_visits,
    };

    let format = if args.json {
        Format::Json
    } else {
        Format::Text
    };

    match yarn_why::why(yarn_lock_text, query, &opts, format)? {
        Report::NotFound => {
            println!("Package not found");
            std::process::exit(1);
        }
        Report::Found(output) => {
            stdout
                .write_all(output.as_bytes())
                .expect("Failed to write to stdout");
        }
    }

    Ok(())
}

fn print_records<W: Write>(stdout: &mut W, entries: &[yarn_lock_parser::Entry]) -> Result<()> {
    for record in iter_flat_dependencies(entries) {
        writeln!(
            stdout,
            "{}",
            serde_json::to_string(&record).expect("could not serialize Entry to JSON")
        )?;
    }
    Ok(())
}
