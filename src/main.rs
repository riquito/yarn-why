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

static MAX_PKG_VISITS: usize = 20;

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

fn get_parents<'a>(
    pkg: &'a Pkg<'a>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, ParentsNode>,
) -> Cow<'a, Vec<&'a Pkg<'a>>> {
    if let Some(parents_node) = pkg2parents.get(pkg) {
        Cow::Borrowed(&parents_node.desc)
    } else {
        Cow::Owned(Vec::new())
    }
}

fn _build_path_to_dependency<'a>(
    pkg: &'a Pkg<'a>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, ParentsNode>,
    curr_path: &mut Vec<&'a Pkg<'a>>,
    paths: &mut Vec<Vec<&'a Pkg<'a>>>,
    visited: &mut HashMap<&'a Pkg<'a>, usize>,
) {
    *visited.entry(pkg).or_insert(0) += 1;
    curr_path.push(pkg);
    let parents = get_parents(pkg, pkg2parents);

    if parents.is_empty() {
        // root dependency (no parents, it must have been defined in package.json)
        let mut complete_path = curr_path.clone();
        complete_path.reverse();

        // If the queried package had cycles, we keep just the leftmost occurrence
        let queried_package = complete_path.last().unwrap();
        let first_idx_of_queried_package = complete_path
            .iter()
            .position(|n| n == queried_package)
            .unwrap();
        complete_path.truncate(first_idx_of_queried_package + 1);

        paths.push(complete_path);
    } else {
        for p in parents.iter() {
            if *visited.get(p).unwrap_or(&0_usize) < MAX_PKG_VISITS {
                _build_path_to_dependency(p, pkg2parents, curr_path, paths, visited);
            }
        }
    }

    curr_path.pop();
}

fn build_path_to_dependency<'a>(
    pkg: &'a Pkg<'a>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, ParentsNode>,
    paths: &mut Vec<Vec<&'a Pkg<'a>>>,
) {
    if !pkg2parents.contains_key(pkg) {
        return;
    }

    let mut visited: HashMap<&'a Pkg<'a>, usize> = HashMap::new();

    let mut path = Vec::new();
    _build_path_to_dependency(pkg, pkg2parents, &mut path, paths, &mut visited);
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
        if pkg2parents.get(q).is_some() {
            build_path_to_dependency(q, pkg2parents, &mut paths);
        }
    }

    if paths.is_empty() {
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
                paths = vec![vec![q]];
            }
        }
    }

    paths
}

#[derive(Debug)]
struct ParentsNode<'a> {
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
                let parents_node = ParentsNode { desc: Vec::new() };
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

    let symbol = if is_last { '└' } else { '├' };
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
        "{prefix}{symbol}─ {namespace}{name}@{pkg_descriptor}",
        namespace = colorize(namespace, (215, 95, 0)),
        name = colorize(name, (215, 135, 95)),
        pkg_descriptor = colorize(pkg_descriptor, (135, 175, 255))
    )
    .expect("Failed to write to stdout");

    for (i, child) in node.children.iter().enumerate() {
        let mut child_levels = Vec::new();
        let mut new_cols = cols.clone();
        child_levels.append(&mut new_cols);
        child_levels.push(if is_last { ' ' } else { '│' });

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
            writeln!(stdout, "│").expect("Failed to write to stdout");
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

    for (paths_idx, path) in paths.iter().enumerate() {
        let mut prev_pkg: Option<&Pkg> = None;

        for (path_idx, pkg) in path.iter().enumerate() {
            if prev_pkg == None {
                // It's a new root, did we already add it?
                if !nodes.contains_key(pkg) {
                    let node = Rc::new(RefCell::new(Node {
                        children: Vec::new(),
                        pkg,
                    }));

                    output.push(node.clone());
                    nodes.insert(pkg, node);
                }
            } else if !(paths_idx > 0
                && path.get(..path_idx + 1) == paths[paths_idx - 1].get(..path_idx + 1))
            {
                // so.. not a root package, path different from previous

                let node = nodes.entry(pkg).or_insert_with(|| {
                    Rc::new(RefCell::new(Node {
                        children: Vec::new(),
                        pkg,
                    }))
                });

                // it must have a parent at this point
                let cloned_node = node.clone();
                let parent = nodes.get(prev_pkg.unwrap()).unwrap();

                // did we already add this pkg as children of its parent?
                if parent
                    .borrow_mut()
                    .children
                    .iter()
                    .all(|c| &c.borrow_mut().pkg != pkg)
                {
                    parent.borrow_mut().children.push(cloned_node);
                }
            }

            prev_pkg = Some(pkg);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const PKG_A: Pkg = ("a", "v1");
    const PKG_B: Pkg = ("b", "v1");
    const PKG_C: Pkg = ("c", "v1");
    const PKG_D: Pkg = ("d", "v1");

    fn mock_pkg2parents_empty<'a>() -> HashMap<&'a Pkg<'a>, ParentsNode<'a>> {
        let pkg2parents: HashMap<&Pkg, ParentsNode> = HashMap::new();
        pkg2parents
    }

    fn mock_pkg2parents_one_element<'a>() -> HashMap<&'a Pkg<'a>, ParentsNode<'a>> {
        let mut pkg2parents: HashMap<&Pkg, ParentsNode> = HashMap::new();
        let p = ParentsNode { desc: Vec::new() };
        pkg2parents.insert(&PKG_A, p);
        pkg2parents
    }

    fn mock_pkg2parents_ab<'a>() -> HashMap<&'a Pkg<'a>, ParentsNode<'a>> {
        let mut pkg2parents: HashMap<&Pkg, ParentsNode> = HashMap::new();
        let p = ParentsNode { desc: vec![&PKG_B] };
        pkg2parents.insert(&PKG_A, p);
        pkg2parents
    }

    fn mock_pkg2parents_ab_ac<'a>() -> HashMap<&'a Pkg<'a>, ParentsNode<'a>> {
        let mut pkg2parents: HashMap<&Pkg, ParentsNode> = HashMap::new();
        let p = ParentsNode {
            desc: vec![&PKG_B, &PKG_C],
        };
        pkg2parents.insert(&PKG_A, p);
        pkg2parents
    }

    fn mock_pkg2parents_abc_abdc<'a>() -> HashMap<&'a Pkg<'a>, ParentsNode<'a>> {
        let mut pkg2parents: HashMap<&Pkg, ParentsNode> = HashMap::new();
        let c_parents = ParentsNode {
            desc: vec![&PKG_D, &PKG_B],
        };
        let d_parents = ParentsNode { desc: vec![&PKG_B] };
        let b_parents = ParentsNode { desc: vec![&PKG_A] };
        pkg2parents.insert(&PKG_C, c_parents);
        pkg2parents.insert(&PKG_D, d_parents);
        pkg2parents.insert(&PKG_B, b_parents);
        pkg2parents
    }

    #[test]
    fn pkg_not_found() {
        let query: Pkg = ("foo", "1");
        let pkg2parents = mock_pkg2parents_empty();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(&query, &pkg2parents, &mut paths);

        let output: Vec<Vec<&Pkg>> = Vec::new();

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_no_parents() {
        let query = &PKG_A;
        let pkg2parents = mock_pkg2parents_one_element();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths);

        let output = vec![vec![&PKG_A]];

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_one_parent() {
        let query: &Pkg = &PKG_A;
        let pkg2parents = mock_pkg2parents_ab();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths);

        let output: Vec<Vec<&Pkg>> = vec![vec![&PKG_B, &PKG_A]];

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_two_parents() {
        let query: &Pkg = &PKG_A;
        let pkg2parents = mock_pkg2parents_ab_ac();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths);

        let output: Vec<Vec<&Pkg>> = vec![vec![&PKG_B, &PKG_A], vec![&PKG_C, &PKG_A]];

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_two_branches() {
        let query: &Pkg = &PKG_C;
        let pkg2parents = mock_pkg2parents_abc_abdc();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths);

        let output: Vec<Vec<&Pkg>> = vec![
            vec![&PKG_A, &PKG_B, &PKG_D, &PKG_C],
            vec![&PKG_A, &PKG_B, &PKG_C],
        ];

        assert_eq!(paths, output);
    }
}
