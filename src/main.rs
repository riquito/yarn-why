use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::io::{Read, Write};
use yarn_lock_parser::{parse_str, Entry};

const HELP: &str = concat!(
    "yarn-why ",
    env!("CARGO_PKG_VERSION"),
    r#"

Usage:
    yarn-why [OPTIONS] package[@range] # read ./yarn.lock
    yarn-why [OPTIONS] package[@range] < /path/to/yarn.lock

Example:
    yarn-why lodash@^4.17.15

OPTIONS:
    -V, --version            Prints version information
    -h, --help               Prints this help and exit

ARGS:
    package[@range]          Package to search for, with or without range.
                             The range must match one in yarn.lock

LICENSE: GPL-3.0-or-later
"#
);

#[derive(Debug)]
struct Opt {
    version: bool,
    query: Option<String>,
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
        let mut res = curr_path.clone();
        res.reverse();
        paths.push(res);
        curr_path.pop();
    }
}

fn why<'a>(
    queries: Vec<&'a Pkg>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, Vec<&'a Pkg<'a>>>,
    entries: &Vec<Entry>,
) -> Vec<Vec<&'a Pkg<'a>>> {
    if queries.is_empty() {
        return Vec::new();
    }

    let mut paths: Vec<Vec<&Pkg>> = Vec::new();
    for q in queries.iter() {
        let mut curr_path: Vec<&Pkg> = Vec::new();
        tree(q, pkg2parents, &mut curr_path, &mut paths);
    }

    if paths.len() == 1 && paths.get(0).unwrap().len() == 1 {
        // Worst case, we have to search again, O(n).
        // There are two scenarios:
        // 1) the package does not exist in yarn.lock (maybe there is one with
        // that name but definitely not with the searched descriptor version)
        // 2) the package exists in yarn.lock, but is a direct dependency in
        // package.json and we couldn't find it in pkg2parents (since that map
        // contains only packages that are dependencies of something else)
        let q = queries.get(0).unwrap();
        for e in entries {
            if e.name == q.0 && e.descriptors.contains(q) {
                return paths;
            }
        }

        paths = Vec::new();
    }

    paths
}

fn get_descriptor_from_cli_arg(arg: &str) -> Option<(&str, &str)> {
    if let Some(idx) = arg.rfind('@') {
        // skip @foo/bar, keep @foo/bar@1.0.0
        if idx > 0 {
            return Some((&arg[0..idx], &arg[idx + 1..]));
        }
    }

    None
}

fn main() -> Result<()> {
    let mut pargs = pico_args::Arguments::from_env();

    if pargs.contains(["-h", "--help"]) {
        print!("{}", HELP);
        std::process::exit(0);
    }

    let args = Opt {
        version: pargs.contains(["-V", "--version"]),
        query: pargs.free_from_str().ok(),
    };

    let remaining = pargs.finish();

    if !remaining.is_empty() {
        eprintln!("Error: unexpected arguments {:?}", remaining);
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

    let query = {
        if args.query.is_none() {
            print!("{}", HELP);
            std::process::exit(1);
        }
        &args.query.unwrap()
    };

    let mut yarn_lock_text: Vec<u8> = Vec::new();

    if atty::is(atty::Stream::Stdin) {
        let mut f = std::fs::File::open("yarns.lock")
            .map_err(|e| anyhow!("Cannot open yarn.lock: {}", e))?;
        f.read_to_end(&mut yarn_lock_text)?;
    } else {
        let stdin = std::io::stdin();
        let mut stdin = std::io::BufReader::with_capacity(32 * 1024, stdin.lock());
        stdin.read_to_end(&mut yarn_lock_text)?;
    }

    let stdout = std::io::stdout();
    let mut stdout = std::io::BufWriter::with_capacity(32 * 1024, stdout.lock());

    let mut queries: Vec<&(&str, &str)> = Vec::new();

    let maybe_cli_descriptor = get_descriptor_from_cli_arg(query);
    let search_for_descriptors = maybe_cli_descriptor.is_none();
    let cli_descriptor: (&str, &str);

    if let Some(d) = maybe_cli_descriptor {
        cli_descriptor = d;
        queries.push(&cli_descriptor);
    }

    let entries = parse_str(std::str::from_utf8(&yarn_lock_text)?)?;

    // Build a map descriptor => parent
    let mut pkg2parents: HashMap<&(&str, &str), Vec<&(&str, &str)>> = HashMap::new();
    for e in entries.iter() {
        for dep in e.dependencies.iter() {
            let mut dep_parents: Vec<&(&str, &str)> = Vec::new();
            for d in e.descriptors.iter() {
                dep_parents.push(d);
            }
            pkg2parents.insert(dep, dep_parents);
        }

        // "reuse the cycle" to find the descriptors used for the package
        // we are searching for (the package could have multiple entries)
        if search_for_descriptors && e.name == query {
            for d in e.descriptors.iter() {
                queries.push(d);
            }
        }
    }

    let paths = why(queries, &pkg2parents, &entries);

    if paths.is_empty() {
        println!("Package not found");
        std::process::exit(1);
    }

    for p in paths.iter() {
        let mut depth = 0;
        for elem in p.iter() {
            println!("{:indent$}{}@{}", "", elem.0, elem.1, indent = depth);
            depth += 3;
        }
    }

    stdout.flush()?;

    Ok(())
}
