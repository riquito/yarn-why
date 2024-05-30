use anyhow::{anyhow, Result};
use once_cell::sync::OnceCell;
use semver::{Version, VersionReq};
use serde::ser::SerializeTuple;
use serde::{Serialize, Serializer};
use serde_json::Result as SerdeJsonResult;
use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
use yarn_lock_parser::{parse_str, Entry};

extern crate fxhash;
use fxhash::FxHashMap as HashMap;

const HELP: &str = concat!(
    "yarn-why ",
    env!("CARGO_PKG_VERSION"),
    r#"

Usage:
    yarn-why [OPTIONS] package [range] # read ./yarn.lock
    yarn-why [OPTIONS] package [range] < /path/to/yarn.lock
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

ARGS:
    package[@range]          Package to search for, with or without range.
                             The range must match one in yarn.lock

LICENSE: GPL-3.0-or-later
"#
);

static MAX_PKG_VISITS_DEFAULT: usize = 20;
static MAX_PKG_VISITS: OnceCell<usize> = OnceCell::new();
static IS_STDIN_TTY: OnceCell<bool> = OnceCell::new();

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
}

type Pkg<'a> = (&'a str, &'a str);

/// get_parents(...) returns the parent packages for a
/// given package by looking it up in a hashmap.
///
/// If found, it returns a borrowed reference to the stored
/// parent packages. If not found, it returns an owned empty vector.
fn get_parents<'a>(
    pkg: &'a Pkg<'a>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, Parents>,
) -> Cow<'a, Vec<&'a Pkg<'a>>> {
    if let Some(parents_node) = pkg2parents.get(pkg) {
        Cow::Borrowed(&parents_node.0)
    } else {
        Cow::Owned(Vec::new())
    }
}

fn _build_path_to_dependency<'a>(
    pkg: &'a Pkg<'a>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, Parents>,
    curr_path: &mut Vec<&'a Pkg<'a>>,
    paths: &mut Vec<Vec<&'a Pkg<'a>>>,
    visited: &mut HashMap<&'a Pkg<'a>, usize>,
) {
    *visited.entry(pkg).or_insert(0) += 1;
    curr_path.push(pkg);
    let parents = get_parents(pkg, pkg2parents);

    if parents.is_empty() {
        // root package (either we recursed into the root, or we immediately
        // searched for a package that is in package.json and installed
        // directly - nothing had it as dependency)
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
            if *visited.get(p).unwrap_or(&0_usize) < *MAX_PKG_VISITS.get().unwrap() {
                _build_path_to_dependency(p, pkg2parents, curr_path, paths, visited);
            }
        }
    }

    curr_path.pop();
}

/// Performs a depth-first traversal of the dependency graph
/// to build up all unique paths from dependencies to the
/// queried package.
/// `paths` contains the final output paths discovered.
fn build_path_to_dependency<'a>(
    pkg: &'a Pkg<'a>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, Parents>,
    paths: &mut Vec<Vec<&'a Pkg<'a>>>,
) {
    if !pkg2parents.contains_key(pkg) {
        return;
    }

    let mut visited: HashMap<&'a Pkg<'a>, usize> = HashMap::default();

    let mut path = Vec::new();
    _build_path_to_dependency(pkg, pkg2parents, &mut path, paths, &mut visited);
}

fn why<'a>(
    queries: Vec<&'a Pkg>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, Parents>,
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
        let q = queries.first().unwrap();
        for e in entries {
            if e.name == q.0 && e.descriptors.contains(q) {
                paths = vec![vec![q]];
            }
        }
    }

    paths
}

#[derive(Debug)]
struct Parents<'a>(Vec<&'a (&'a str, &'a str)>);

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

    let args = Opt {
        version: pargs.contains(["-V", "--version"]),
        json: pargs.contains(["-j", "--json"]),
        dedup,
        no_max_depth: pargs.contains(["-D", "--no-max-depth"]),
        max_depth: pargs
            .opt_value_from_str(["-d", "--max-depth"])?
            .or(Some(10)),
        yarn_lock_path: pargs.opt_value_from_os_str(["-y", "--yarn-lock-path"], parse_path)?,
        query: pargs.free_from_str().ok(),
        filter: pargs
            .opt_free_from_str::<String>()?
            .as_deref()
            .map(VersionReq::parse)
            .transpose()?,
    };

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

    if let Ok(max_pgk_visit_as_str) = std::env::var("MAX_PKG_VISITS") {
        let max_pgk_visit = max_pgk_visit_as_str
            .parse::<usize>()
            .expect("MAX_PKG_VISITS is not a number");
        MAX_PKG_VISITS.set(max_pgk_visit).unwrap();
    } else {
        MAX_PKG_VISITS.set(MAX_PKG_VISITS_DEFAULT).unwrap();
    }

    IS_STDIN_TTY.set(atty::is(atty::Stream::Stdin)).unwrap();

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

    if must_read_yarn_lock || *IS_STDIN_TTY.get().unwrap() {
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
    let mut entries = parse_str(std::str::from_utf8(&yarn_lock_text)?)?;

    if args.filter.is_some() {
        let req = args.filter.as_ref().unwrap();
        entries.retain(|e| {
            if e.name == query.as_str() {
                let v = Version::parse(e.version);
                // if we can't parse e.version, let's keep the entry
                return v.is_err() || req.matches(&v.unwrap());
            }

            true
        })
    }

    // In yarn-lock-parser the dependencies were meant to contain
    // just (name, descriptor), with the descriptor being without the
    // protocol. Turns out it's not always the case, so we adjuts it here.
    // In addition, the patch protocol contains duplicates for the purpose
    // of yarn-why, so we must drop them.
    entries.retain_mut(|e| {
        e.dependencies.retain_mut(|dep| {
            // XXX here we just check for npm: but there are other protocols
            // out there. In general, we should stop stripping it in yarn-lock-parser
            *dep = (dep.0, dep.1.strip_prefix("npm:").unwrap_or(dep.1));

            // XXX Turns out we need to also strip `worskpace:`
            *dep = (dep.0, dep.1.strip_prefix("workspace:").unwrap_or(dep.1));

            // hacky way to detect patch protocol (we must drop them from entries
            // otherwise we will get duplicates)
            !dep.1.contains('#') || dep.1.contains("git")
        });

        e.descriptors.retain_mut(|descriptor| {
            // hacky way to detect patch protocol (we must drop them from entries
            // otherwise we will get duplicates)
            !descriptor.1.contains('#') || descriptor.1.contains("git")
        });

        !e.descriptors.is_empty()
    });

    // Build a map descriptor => parent
    let mut pkg2parents: HashMap<&(&str, &str), Parents> = HashMap::default();
    // Keep which descriptors are used for the package we are searching for
    let mut queries: Vec<&(&str, &str)> = Vec::new();

    for e in entries.iter() {
        for dep in e.dependencies.iter() {
            let dep_parents = pkg2parents.entry(dep).or_insert(Parents(Vec::new()));

            for d in e.descriptors.iter() {
                dep_parents.0.push(d);
            }
        }

        // "reuse the cycle" to find the descriptors used for the package
        // we are searching for (the package could have multiple entries)
        // Descriptors are used at most once among all the entries of a package,
        // so we can't find duplicates.
        if e.name == query {
            for d in e.descriptors.iter() {
                queries.push(d);
            }
        }
    }

    let mut pkg2entry = HashMap::default();
    entries.iter().for_each(|e| {
        for d in e.descriptors.iter() {
            pkg2entry.insert(d, e);
        }
    });

    let mut paths = why(queries, &pkg2parents, &entries);

    paths.sort();

    if paths.is_empty() {
        println!("Package not found");
        std::process::exit(1);
    }

    // A bit convoluted, but allow us to have both a sensible default
    // and yet let users ask to go all the way down.
    if !args.no_max_depth {
        if let Some(max_depth) = args.max_depth {
            for p in paths.iter_mut() {
                p.truncate(max_depth);
            }
        }
    }

    let owned_tree = convert_paths_to_tree(paths.as_slice(), &pkg2entry);
    let mut tree = &owned_tree;
    let dedup_tree;
    let borrowed_dedup_tree;
    let single_workspace_tree;
    let borrowed_single_workspace_tree;

    if args.dedup {
        dedup_tree = build_tree_with_no_duplicates(tree);
        borrowed_dedup_tree = dedup_tree.borrow();
        tree = &borrowed_dedup_tree.children;
    }

    if tree.len() == 1 && tree.first().unwrap().borrow().pkg.1 == "." {
        single_workspace_tree = tree[0].to_owned();
        borrowed_single_workspace_tree = single_workspace_tree.borrow();
        tree = &borrowed_single_workspace_tree.children;
    }

    let output = if args.json {
        print_tree_as_json(tree)?
    } else {
        print_tree(tree)
    };

    stdout
        .write_all(output.as_bytes())
        .expect("Failed to write to stdout");

    Ok(())
}

#[inline(always)]
fn colorize(s: &str, (r, g, b): (usize, usize, usize)) -> Cow<'_, str> {
    if s.is_empty() || !*IS_STDIN_TTY.get().unwrap() {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(format!("\x1b[38;2;{r};{g};{b}m{s}\x1b[0m"))
    }
}

fn print_tree_node(node: &Node, is_last: bool, cols: Vec<char>, output: &mut String) {
    let mut prefix = String::new();

    for c in cols.iter() {
        prefix.push(*c);
        prefix.push_str("  ");
    }

    let symbol = if is_last { '└' } else { '├' };
    let pkg_name = node.pkg.0;
    let pkg_descriptor = node.pkg.1;
    let pkg_version = node.e.version;

    let mut namespace = "";
    let at = "@";
    let mut name = pkg_name;
    if pkg_name.starts_with('@') {
        if let Some(idx) = pkg_name.find('/') {
            (namespace, name) = (&pkg_name[..idx], &pkg_name[idx..]);
        }
    }

    writeln!(
        output,
        "{prefix}{symbol}─ {namespace}{name}{at}{pkg_version} (via {pkg_descriptor})",
        namespace = colorize(namespace, (215, 95, 0)),
        name = colorize(name, (215, 135, 95)),
        at = colorize(at, (135, 175, 255)),
        pkg_version = colorize(pkg_version, (135, 175, 255)),
        pkg_descriptor = colorize(pkg_descriptor, (3, 150, 150))
    )
    .expect("Failed to write to string");

    for (i, child) in node.children.iter().enumerate() {
        let mut child_levels = Vec::with_capacity(cols.len() + 1);
        let mut new_cols = cols.clone();
        child_levels.append(&mut new_cols);
        child_levels.push(if is_last { ' ' } else { '│' });

        print_tree_node(
            &child.as_ref().borrow(),
            i == node.children.len() - 1,
            child_levels,
            output,
        );
    }
}

fn print_tree(tree: &[Rc<RefCell<Node>>]) -> String {
    let mut output = String::new();

    for (i, wrapped_node) in tree.iter().enumerate() {
        if i > 0 && i < tree.len() {
            output.push_str("│\n");
        }
        print_tree_node(
            &wrapped_node.as_ref().borrow(),
            i == tree.len() - 1,
            Vec::new(),
            &mut output,
        );
    }

    output
}

fn print_tree_as_json(tree: &[Rc<RefCell<Node>>]) -> SerdeJsonResult<String> {
    serde_json::to_string(&tree)
}

#[derive(Debug, Serialize)]
struct SerializableNode<'a> {
    #[serde(skip_serializing_if = "serialize_skip_if_children_empty")]
    children: Vec<Rc<RefCell<Node<'a>>>>,
    #[serde(serialize_with = "serialize_pkg_as_string")]
    descriptor: &'a Pkg<'a>,
    version: &'a str,
}

impl<'a> From<Node<'a>> for SerializableNode<'a> {
    fn from(node: Node<'a>) -> Self {
        Self {
            children: node.children,
            descriptor: node.pkg,
            version: node.e.version,
        }
    }
}

// TODO We can avoid the somewhat expensive cloning by
// not using SerializableNode
#[derive(Debug, Serialize, Clone)]
#[serde(into = "SerializableNode")]
struct Node<'a> {
    #[serde(skip_serializing_if = "serialize_skip_if_children_empty")]
    children: Vec<Rc<RefCell<Node<'a>>>>,
    #[serde(
        rename(serialize = "descriptor"),
        serialize_with = "serialize_pkg_as_string"
    )]
    pkg: &'a Pkg<'a>,
    e: &'a Entry<'a>,
}

fn serialize_skip_if_children_empty<T>(x: &[T]) -> bool {
    x.is_empty()
}

fn serialize_pkg_as_string<'a, S>(x: &'a Pkg<'a>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut tup = s.serialize_tuple(2)?;
    tup.serialize_element(x.0)?;
    tup.serialize_element(x.1)?;
    tup.end()
}

fn _build_tree_with_no_duplicates<'a>(
    parent: &mut Rc<RefCell<Node<'a>>>,
    children: &[Rc<RefCell<Node<'a>>>],
    visited: &mut HashMap<&'a Pkg<'a>, bool>,
) {
    for node in children.iter() {
        let ref_node = node.as_ref().borrow();

        let mut new_node = Rc::new(RefCell::new(Node {
            children: Vec::new(),
            pkg: ref_node.pkg,
            e: ref_node.e,
        }));
        parent.borrow_mut().children.push(new_node.clone());

        // Usually we drop children we already visited, but if the children
        // is a leaf we keep it (so the final user won't have to find which
        // version of the searched package was being deduplicated.
        let next_child_is_leaf = ref_node.children.len() == 1
            && ref_node
                .children
                .first()
                .unwrap()
                .borrow()
                .children
                .is_empty();

        if next_child_is_leaf || !visited.contains_key(ref_node.pkg) {
            visited.insert(ref_node.pkg, true);
            _build_tree_with_no_duplicates(&mut new_node, &ref_node.children, visited)
        }
    }
}

static ROOT_PKG: (&str, &str) = ("", "");
static ROOT_ENTRY: Entry = Entry {
    name: "",
    version: "",
    integrity: "",
    dependencies: Vec::new(),
    descriptors: Vec::new(),
};

fn build_tree_with_no_duplicates<'a>(children: &[Rc<RefCell<Node<'a>>>]) -> Rc<RefCell<Node<'a>>> {
    let mut visited: HashMap<&Pkg, bool> = HashMap::default();

    let mut root = Rc::new(RefCell::new(Node {
        children: Vec::new(),
        pkg: &ROOT_PKG,
        e: &ROOT_ENTRY,
    }));

    _build_tree_with_no_duplicates(&mut root, children, &mut visited);

    root
}

fn convert_paths_to_tree<'a>(
    paths: &'a [Vec<&Pkg<'a>>],
    pkg2entry: &'a HashMap<&(&str, &str), &Entry<'a>>,
) -> Vec<Rc<RefCell<Node<'a>>>> {
    let mut nodes: HashMap<&Pkg, Rc<RefCell<Node>>> = HashMap::default();
    let mut output: Vec<Rc<RefCell<Node>>> = Vec::new();

    for (paths_idx, path) in paths.iter().enumerate() {
        let mut prev_pkg: Option<&Pkg> = None;

        for (path_idx, pkg) in path.iter().enumerate() {
            if prev_pkg.is_none() {
                // It's a new root, did we already add it?
                if !nodes.contains_key(pkg) {
                    let node = Rc::new(RefCell::new(Node {
                        children: Vec::new(),
                        pkg,
                        e: pkg2entry.get(pkg).unwrap(),
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
                        e: pkg2entry.get(pkg).unwrap(),
                    }))
                });

                // it must have a parent at this point
                let cloned_node = node.clone();
                #[allow(clippy::unnecessary_unwrap)]
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

    fn init() {
        MAX_PKG_VISITS.get_or_init(|| MAX_PKG_VISITS_DEFAULT);
    }

    fn mock_pkg2parents_empty<'a>() -> HashMap<&'a Pkg<'a>, Parents<'a>> {
        let pkg2parents: HashMap<&Pkg, Parents> = HashMap::default();
        pkg2parents
    }

    fn mock_pkg2parents_one_element<'a>() -> HashMap<&'a Pkg<'a>, Parents<'a>> {
        let mut pkg2parents: HashMap<&Pkg, Parents> = HashMap::default();
        let p = Parents(Vec::new());
        pkg2parents.insert(&PKG_A, p);
        pkg2parents
    }

    fn mock_pkg2parents_ab<'a>() -> HashMap<&'a Pkg<'a>, Parents<'a>> {
        let mut pkg2parents: HashMap<&Pkg, Parents> = HashMap::default();
        let p = Parents(vec![&PKG_B]);
        pkg2parents.insert(&PKG_A, p);
        pkg2parents
    }

    fn mock_pkg2parents_ab_ac<'a>() -> HashMap<&'a Pkg<'a>, Parents<'a>> {
        let mut pkg2parents: HashMap<&Pkg, Parents> = HashMap::default();
        let p = Parents(vec![&PKG_B, &PKG_C]);
        pkg2parents.insert(&PKG_A, p);
        pkg2parents
    }

    fn mock_pkg2parents_abc_abdc<'a>() -> HashMap<&'a Pkg<'a>, Parents<'a>> {
        let mut pkg2parents: HashMap<&Pkg, Parents> = HashMap::default();
        let c_parents = Parents(vec![&PKG_D, &PKG_B]);
        let d_parents = Parents(vec![&PKG_B]);
        let b_parents = Parents(vec![&PKG_A]);
        pkg2parents.insert(&PKG_C, c_parents);
        pkg2parents.insert(&PKG_D, d_parents);
        pkg2parents.insert(&PKG_B, b_parents);
        pkg2parents
    }

    #[test]
    fn pkg_not_found() {
        init();
        let query: Pkg = ("foo", "1");
        let pkg2parents = mock_pkg2parents_empty();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(&query, &pkg2parents, &mut paths);

        let output: Vec<Vec<&Pkg>> = Vec::new();

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_no_parents() {
        init();
        let query = &PKG_A;
        let pkg2parents = mock_pkg2parents_one_element();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths);

        let output = vec![vec![&PKG_A]];

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_one_parent() {
        init();
        let query: &Pkg = &PKG_A;
        let pkg2parents = mock_pkg2parents_ab();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths);

        let output: Vec<Vec<&Pkg>> = vec![vec![&PKG_B, &PKG_A]];

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_two_parents() {
        init();
        let query: &Pkg = &PKG_A;
        let pkg2parents = mock_pkg2parents_ab_ac();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths);

        let output: Vec<Vec<&Pkg>> = vec![vec![&PKG_B, &PKG_A], vec![&PKG_C, &PKG_A]];

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_two_branches() {
        init();
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
