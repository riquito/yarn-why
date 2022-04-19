use anyhow::Result;
use std::collections::HashMap;
use std::io::{Read, Write};
use yarn_lock_parser::{parse_str, Entry};

const HELP: &str = concat!(
    "Like `yarn why`, but fast",
    env!("CARGO_PKG_VERSION"),
    r#"
USAGE:
    yarn-why [FLAGS] [OPTIONS] package < yarn.lock

FLAGS:
    -V, --version                 Prints version information
    -h, --help                    Prints this help and exit

ARGS:
    package                       Package to query for

LICENSE: GPL-3.0-or-later
"#
);

#[derive(Debug)]
struct Opt {
    version: bool,
    query: String,
}

type Pkg<'a> = (&'a str, &'a str);

fn tree<'a>(
    pkg: &'a Pkg<'a>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, Vec<&'a Pkg<'a>>>,
    curr_path: &mut Vec<&'a Pkg<'a>>,
    paths: &mut Vec<Vec<&'a Pkg<'a>>>,
) {
    curr_path.push(pkg);
    let parents = pkg2parents.get(pkg);
    if parents.is_some() && !parents.unwrap().is_empty() {
        for p in pkg2parents[pkg].iter() {
            tree(p, pkg2parents, curr_path, paths);
        }
    } else {
        paths.push(curr_path.clone());
        curr_path.pop();
    }
}

fn main() -> Result<()> {
    let mut pargs = pico_args::Arguments::from_env();

    if pargs.contains(["-h", "--help"]) {
        print!("{}", HELP);
        std::process::exit(0);
    }

    let args = Opt {
        version: pargs.contains(["-V", "--version"]),
        query: pargs.free_from_str()?,
    };

    let remaining = pargs.finish();

    if !remaining.is_empty() {
        eprintln!("yarn-why: unexpected arguments {:?}", remaining);
        eprintln!("Try 'yarn-why --help' for more information.");
        std::process::exit(1);
    }

    if args.version {
        println!("yarn-why {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let stdin = std::io::stdin();
    let mut stdin = std::io::BufReader::with_capacity(32 * 1024, stdin.lock());

    let stdout = std::io::stdout();
    let mut stdout = std::io::BufWriter::with_capacity(32 * 1024, stdout.lock());

    let mut yarn_lock_text: Vec<u8> = Vec::new();
    stdin.read_to_end(&mut yarn_lock_text)?;

    let entries = parse_str(std::str::from_utf8(&yarn_lock_text)?)?;
    let mut descriptor2entry: HashMap<&(&str, &str), &Entry> = HashMap::new();

    // Build a map descriptor => entry
    // (entries can have multiple descriptors).
    for e in entries.iter() {
        for d in e.descriptors.iter() {
            descriptor2entry.insert(d, e);
        }
    }

    let mut parents: HashMap<&(&str, &str), Vec<&(&str, &str)>> = HashMap::new();
    for e in entries.iter() {
        for dep in e.dependencies.iter() {
            let mut dep_parents: Vec<&(&str, &str)> = Vec::new();
            for d in e.descriptors.iter() {
                dep_parents.push(d);
            }
            parents.insert(dep, dep_parents);
        }
    }

    let q = args
        .query
        .rsplit_once('@')
        .expect("query format is package@version");
    let mut curr_path: Vec<&Pkg> = Vec::new();
    let mut paths: Vec<Vec<&Pkg>> = Vec::new();
    tree(&q, &parents, &mut curr_path, &mut paths);
    for k in paths.iter() {
        println!("{:?}", k);
    }

    stdout.flush()?;

    Ok(())
}
