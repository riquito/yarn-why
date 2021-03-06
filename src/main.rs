use anyhow::{anyhow, Result};
use serde::{Serialize, Serializer};
use serde_json::Result as SerdeJsonResult;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
use yarn_lock_parser::{parse_str, Entry};

const HELP: &str = concat!(
    "yarn-why ",
    env!("CARGO_PKG_VERSION"),
    r#"

Usage:
    yarn-why [OPTIONS] package[@range] # read ./yarn.lock
    yarn-why [OPTIONS] package[@range] < /path/to/yarn.lock
    yarn-why [OPTIONS] package[@range] -y /path/to/yarn.lock

Example:
    yarn-why lodash@^4.17.15

OPTIONS:
    -d, --max-depth [depth]  Truncate dependencies at that level [Default: 10]
    -D, --no-max-depth       Ignore max-depth (both default and user defined)
    -j, --json               Format the output as JSON
    -h, --help               Prints this help and exit
    -V, --version            Prints version information
    -y, --yarn-lock-file     Path to a yarn.lock file to parse

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
    no_max_depth: bool,
    query: Option<String>,
    yarn_lock_path: Option<PathBuf>,
}

type Pkg<'a> = (&'a str, &'a str);

#[allow(clippy::ptr_arg)]
fn build_path_to_dependency<'a>(
    pkg: &'a Pkg<'a>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, ParentsNode>,
    curr_path: &mut Vec<&'a Pkg<'a>>,
    paths: &mut Vec<Vec<&'a Pkg<'a>>>,
) {
    curr_path.push(pkg);

    let parents_node = pkg2parents.get(pkg);
    if parents_node.is_some() && !parents_node.unwrap().desc.is_empty() {
        // color the parent while traversing. If we already covered it in a
        // previous path, drop it. This is actually necessary not only to
        // reduce the overall output, but to solve dependency loops.
        if *pkg2parents[pkg].traversed.borrow() {
            curr_path.pop();
        } else {
            *pkg2parents[pkg].traversed.borrow_mut() = true;
            for p in pkg2parents[pkg].desc.iter() {
                build_path_to_dependency(p, pkg2parents, curr_path, paths);
            }
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
    pkg2parents: &'a HashMap<&'a Pkg<'a>, ParentsNode>,
    entries: &Vec<Entry>,
) -> Vec<Vec<&'a Pkg<'a>>> {
    if queries.is_empty() {
        return Vec::new();
    }

    let mut paths: Vec<Vec<&Pkg>> = Vec::new();
    for q in queries.iter() {
        let mut curr_path: Vec<&Pkg> = Vec::new();
        build_path_to_dependency(q, pkg2parents, &mut curr_path, &mut paths);
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

#[derive(Debug)]
struct ParentsNode<'a> {
    traversed: RefCell<bool>,
    desc: Vec<&'a (&'a str, &'a str)>,
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

fn parse_path(s: &std::ffi::OsStr) -> Result<std::path::PathBuf, &'static str> {
    Ok(s.into())
}

fn main() -> Result<()> {
    let mut pargs = pico_args::Arguments::from_env();

    if pargs.contains(["-h", "--help"]) {
        print!("{}", HELP);
        std::process::exit(0);
    }

    let args = Opt {
        version: pargs.contains(["-V", "--version"]),
        json: pargs.contains(["-j", "--json"]),
        no_max_depth: pargs.contains(["-D", "--no-max-depth"]),
        max_depth: pargs
            .opt_value_from_str(["-d", "--max-depth"])?
            .or(Some(10)),
        yarn_lock_path: pargs.opt_value_from_os_str(["-y", "--yarn-lock-path"], parse_path)?,
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

    let must_read_yarn_lock = args.yarn_lock_path.is_some();
    let yarn_lock_path = if let Some(path) = args.yarn_lock_path {
        path
    } else {
        PathBuf::from("yarn.lock")
    };

    if must_read_yarn_lock || atty::is(atty::Stream::Stdin) {
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
    let mut pkg2parents: HashMap<&(&str, &str), ParentsNode> = HashMap::new();
    for e in entries.iter() {
        for dep in e.dependencies.iter() {
            if !pkg2parents.contains_key(dep) {
                let parents_node = ParentsNode {
                    traversed: RefCell::new(false),
                    desc: Vec::new(),
                };
                pkg2parents.insert(dep, parents_node);
            }

            let dep_parents = pkg2parents.get_mut(dep).unwrap();

            for d in e.descriptors.iter() {
                dep_parents.desc.push(d);
            }
        }

        // "reuse the cycle" to find the descriptors used for the package
        // we are searching for (the package could have multiple entries)
        if search_for_descriptors && e.name == query {
            for d in e.descriptors.iter() {
                queries.push(d);
            }
        }
    }

    let mut paths = why(queries, &pkg2parents, &entries);

    // A bit convoluted, but allow us to have both a sensible default
    // and yet let users ask to go all the way down.
    if !args.no_max_depth {
        if let Some(max_depth) = args.max_depth {
            for p in paths.iter_mut() {
                p.truncate(max_depth);
            }
        }
    }

    paths.sort();

    if paths.is_empty() {
        println!("Package not found");
        std::process::exit(1);
    }

    let tree = convert_paths_to_tree(paths.as_slice());

    if args.json {
        print_tree_as_json(&tree)?;
    } else {
        print_tree(&tree, &mut stdout);
    }

    stdout.flush()?;

    Ok(())
}

fn colorize(s: &str, (r, g, b): (usize, usize, usize)) -> Cow<'_, str> {
    if s.is_empty() || !atty::is(atty::Stream::Stdout) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(format!("\x1b[38;2;{r};{g};{b}m{s}\x1b[0m"))
    }
}

fn print_tree_node<W: Write>(
    node: &Node,
    level: usize,
    is_last: bool,
    cols: Vec<char>,
    stdout: &mut W,
) {
    let mut prefix = String::new();

    for c in cols.iter() {
        prefix.push(*c);
        prefix.push_str("  ");
    }

    let symbol = if is_last { '???' } else { '???' };
    let pkg_name = node.pkg.0;
    let pkg_descriptor = node.pkg.1;

    let mut namespace = "";
    let mut name = pkg_name;
    if pkg_name.starts_with('@') && pkg_name.contains('/') {
        let idx = pkg_name.find('/').unwrap();
        (namespace, name) = (&pkg_name[..idx], &pkg_name[idx..]);
    }

    writeln!(
        stdout,
        "{prefix}{symbol}??? {namespace}{name}@{pkg_descriptor}",
        namespace = colorize(namespace, (215, 95, 0)),
        name = colorize(name, (215, 135, 95)),
        pkg_descriptor = colorize(pkg_descriptor, (135, 175, 255))
    )
    .expect("Failed to write to stdout");

    for (i, child) in node.children.iter().enumerate() {
        let mut child_levels = Vec::new();
        let mut new_cols = cols.clone();
        child_levels.append(&mut new_cols);
        child_levels.push(if is_last { ' ' } else { '???' });

        print_tree_node(
            &child.as_ref().borrow(),
            level + 1,
            i == node.children.len() - 1,
            child_levels,
            stdout,
        );
    }
}

fn print_tree<W: Write>(tree: &[Rc<RefCell<Node>>], stdout: &mut W) {
    for (i, wrapped_node) in tree.iter().enumerate() {
        if i > 0 && i < tree.len() {
            writeln!(stdout, "???").expect("Failed to write to stdout");
        }
        print_tree_node(
            &wrapped_node.as_ref().borrow(),
            0,
            i == tree.len() - 1,
            Vec::new(),
            stdout,
        );
    }
}

fn print_tree_as_json(tree: &[Rc<RefCell<Node>>]) -> SerdeJsonResult<()> {
    let j = serde_json::to_string(&tree)?;
    println!("{}", j);
    Ok(())
}

#[derive(Debug, Serialize)]
struct Node<'a> {
    #[serde(skip_serializing_if = "serialize_skip_if_children_empty")]
    children: Vec<Rc<RefCell<Node<'a>>>>,
    #[serde(
        rename(serialize = "descriptor"),
        serialize_with = "serialize_pkg_as_string"
    )]
    pkg: &'a Pkg<'a>,
}

fn serialize_skip_if_children_empty<T>(x: &[T]) -> bool {
    x.is_empty()
}

fn serialize_pkg_as_string<'a, S>(x: &'a Pkg<'a>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(&format!("{}@{}", x.0, x.1))
}

fn convert_paths_to_tree<'a>(paths: &'a [Vec<&Pkg<'a>>]) -> Vec<Rc<RefCell<Node<'a>>>> {
    let mut nodes: HashMap<&Pkg, Rc<RefCell<Node>>> = HashMap::new();
    let mut output: Vec<Rc<RefCell<Node>>> = Vec::new();

    for path in paths {
        let mut prev_pkg: Option<&Pkg> = None;

        for pkg in path.iter() {
            if nodes.contains_key(pkg) {
                prev_pkg = Some(pkg);
                continue;
            }

            let node = Rc::new(RefCell::new(Node {
                children: Vec::new(),
                pkg,
            }));

            nodes.insert(pkg, node.clone());

            if let Some(parent_key) = prev_pkg {
                let mut parent = nodes.get(parent_key).unwrap().borrow_mut();
                parent.children.push(node);
            } else {
                // first item in the path, is a root parent
                output.push(node);
            }
            prev_pkg = Some(pkg);
        }
    }
    output
}
