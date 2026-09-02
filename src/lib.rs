//! The engine behind the `yarn-why` CLI.
//!
//! The entry points are [`parse_lockfile`], which turns the text of a
//! `yarn.lock` into normalized entries, and [`why`], which explains why a
//! package is installed.
//!
//! Everything borrows from the lockfile text, so it has to outlive the
//! entries built out of it.

use anyhow::Result;
use once_cell::sync::Lazy;
use semver::{Version, VersionReq};
use serde::ser::SerializeTuple;
use serde::{Serialize, Serializer};
use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;
use yarn_lock_parser::{parse_str, Entry};

pub mod records;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

extern crate fxhash;
use fxhash::FxHashMap as HashMap;

/// How many times a package can be visited while walking the graph before we
/// give up on it. It's a safety net against pathological (cyclic) graphs.
pub const MAX_PKG_VISITS_DEFAULT: usize = 20;

/// The default value of [`Options::max_depth`].
pub const MAX_DEPTH_DEFAULT: usize = 10;

/// A package, as (name, descriptor).
pub type Pkg<'a> = (&'a str, &'a str);

/// How to render the resulting tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// An ASCII tree, the one the CLI prints.
    Text,
    /// A JSON array of nodes.
    Json,
}

/// Knobs for [`why`], mirroring the CLI options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Truncate the paths at that level. `None` means no limit.
    pub max_depth: Option<usize>,
    /// Display packages at most once.
    pub dedup: bool,
    /// Render every dependency rather than the paths to a package.
    pub full_tree: bool,
    /// Only consider versions matching this range.
    pub range: Option<VersionReq>,
    /// Wrap the output in ANSI color codes ([`Format::Text`] only).
    pub color: bool,
    /// See [`MAX_PKG_VISITS_DEFAULT`].
    pub max_pkg_visits: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_depth: Some(MAX_DEPTH_DEFAULT),
            dedup: true,
            full_tree: false,
            range: None,
            color: false,
            max_pkg_visits: MAX_PKG_VISITS_DEFAULT,
        }
    }
}

/// The outcome of [`why`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// The rendered tree, in the requested [`Format`].
    Found(String),
    /// The package is not in the lockfile (or not with the requested range).
    NotFound,
}

/// Normalize the version part of an entry descriptor.
///
/// Up to yarn-lock-parser 0.7 the parser did this itself, since 0.8 it
/// returns the descriptor verbatim, so we keep doing it here to preserve
/// the expected output.
///
/// The version is what comes after the last `@` (aliased dependencies embed
/// another `name@version` pair, e.g. `foo@npm:bar@^1.0.0`) minus the
/// protocol, if any (e.g. `npm:^1.0.0` => `^1.0.0`, `workspace:.` => `.`).
fn strip_descriptor_protocol(version: &str) -> &str {
    let version = match version.rfind('@') {
        Some(idx) => &version[idx + 1..],
        None => version,
    };

    match version.rfind(':') {
        Some(idx) => &version[idx + 1..],
        None => version,
    }
}

/// Normalize the version part of a dependency of an entry.
///
/// Same story as [`strip_descriptor_protocol`], but here the parser used to
/// drop just the aliased package name (e.g. `npm:bar@^1.0.0` => `^1.0.0`),
/// leaving any protocol in place (it's stripped further down).
fn strip_dependency_alias(version: &str) -> &str {
    version.rsplit_once('@').map_or(version, |(_, v)| v)
}

/// Parse a `yarn.lock` and normalize its entries.
///
/// When `range` is given, entries named `query` whose version doesn't match
/// are dropped (entries with an unparseable version are kept).
pub fn parse_lockfile<'a>(
    lockfile: &'a str,
    query: &str,
    range: Option<&VersionReq>,
) -> Result<Vec<Entry<'a>>> {
    let mut entries = parse_str(lockfile)?.entries;

    if let Some(req) = range {
        entries.retain(|e| {
            if e.name == query {
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
            *dep = (dep.0, strip_dependency_alias(dep.1));

            // XXX here we just check for npm: but there are other protocols
            // out there. In general, we should stop stripping it in yarn-lock-parser
            *dep = (dep.0, dep.1.strip_prefix("npm:").unwrap_or(dep.1));

            // XXX Turns out we need to also strip `workspace:`
            *dep = (dep.0, dep.1.strip_prefix("workspace:").unwrap_or(dep.1));

            // hacky way to detect patch protocol (we must drop them from entries
            // otherwise we will get duplicates)
            !dep.1.contains('#') || dep.1.contains("git")
        });

        e.descriptors.retain_mut(|descriptor| {
            *descriptor = (descriptor.0, strip_descriptor_protocol(descriptor.1));

            // hacky way to detect patch protocol (we must drop them from entries
            // otherwise we will get duplicates)
            !descriptor.1.contains('#') || descriptor.1.contains("git")
        });

        !e.descriptors.is_empty()
    });

    Ok(entries)
}

/// Explain why `query` is installed.
///
/// This is [`parse_lockfile`] followed by [`explain`]; use those directly to
/// reuse the parsed entries across several queries.
pub fn why(lockfile: &str, query: &str, opts: &Options, format: Format) -> Result<Report> {
    let entries = parse_lockfile(lockfile, query, opts.range.as_ref())?;
    explain(&entries, query, opts, format)
}

/// Explain why `query` is installed, given already parsed entries.
pub fn explain(entries: &[Entry], query: &str, opts: &Options, format: Format) -> Result<Report> {
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

    let paths = if opts.full_tree {
        Vec::new()
    } else {
        let mut paths = find_paths(queries, &pkg2parents, entries, opts.max_pkg_visits);

        paths.sort();

        if paths.is_empty() {
            return Ok(Report::NotFound);
        }

        // A bit convoluted, but allow us to have both a sensible default
        // and yet let users ask to go all the way down.
        if let Some(max_depth) = opts.max_depth {
            for p in paths.iter_mut() {
                p.truncate(max_depth);
            }
        }
        paths
    };

    let owned_tree = if opts.full_tree {
        full_tree(entries, &pkg2entry)
    } else {
        convert_paths_to_tree(paths.as_slice(), &pkg2entry)
    };
    let mut tree = &owned_tree;

    let dedup_tree;
    let borrowed_dedup_tree;
    let single_workspace_tree;
    let borrowed_single_workspace_tree;

    if opts.dedup {
        dedup_tree = build_tree_with_no_duplicates(tree);
        borrowed_dedup_tree = dedup_tree.borrow();
        tree = &borrowed_dedup_tree.children;
    }

    if tree.len() == 1 && tree.first().unwrap().borrow().pkg.1 == "." {
        single_workspace_tree = tree[0].to_owned();
        borrowed_single_workspace_tree = single_workspace_tree.borrow();
        tree = &borrowed_single_workspace_tree.children;
    }

    let output = match format {
        Format::Json => print_tree_as_json(tree)?,
        Format::Text => print_tree(tree, opts.color),
    };

    Ok(Report::Found(output))
}

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
    max_pkg_visits: usize,
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
            if *visited.get(p).unwrap_or(&0_usize) < max_pkg_visits {
                _build_path_to_dependency(
                    p,
                    pkg2parents,
                    curr_path,
                    paths,
                    visited,
                    max_pkg_visits,
                );
            }
        }
    }

    curr_path.pop();
}

/// Performs a depth-first traversal of the dependency graph
/// to build up all unique paths from dependencies to the
/// queried package.
/// `paths` contains the final output paths discovered.
pub fn build_path_to_dependency<'a>(
    pkg: &'a Pkg<'a>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, Parents>,
    paths: &mut Vec<Vec<&'a Pkg<'a>>>,
    max_pkg_visits: usize,
) {
    if !pkg2parents.contains_key(pkg) {
        return;
    }

    let mut visited: HashMap<&'a Pkg<'a>, usize> = HashMap::default();

    let mut path = Vec::new();
    _build_path_to_dependency(
        pkg,
        pkg2parents,
        &mut path,
        paths,
        &mut visited,
        max_pkg_visits,
    );
}

fn find_paths<'a>(
    queries: Vec<&'a Pkg>,
    pkg2parents: &'a HashMap<&'a Pkg<'a>, Parents>,
    entries: &[Entry],
    max_pkg_visits: usize,
) -> Vec<Vec<&'a Pkg<'a>>> {
    if queries.is_empty() {
        return Vec::new();
    }

    let mut paths: Vec<Vec<&Pkg>> = Vec::new();
    for q in queries.iter() {
        if pkg2parents.get(q).is_some() {
            build_path_to_dependency(q, pkg2parents, &mut paths, max_pkg_visits);
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
pub struct Parents<'a>(pub Vec<&'a (&'a str, &'a str)>);

// Build a tree out of all the entries.
// The children may contain duplicates and cycles
fn full_tree<'a>(
    entries: &'a [Entry<'a>],
    pkg2entry: &'a HashMap<&(&str, &str), &Entry<'a>>,
) -> Vec<Rc<RefCell<Node<'a>>>> {
    let mut nodes: HashMap<Pkg, Rc<RefCell<Node>>> = HashMap::default();
    let mut non_root_entries: Vec<&Entry> = Vec::new();

    // First we create a node for each entry
    for e in entries {
        let pkg = (e.name, e.version);
        let node = Node {
            pkg,
            e,
            children: Vec::new(),
        };
        nodes.insert(node.pkg, Rc::new(RefCell::new(node)));
    }

    // Then we addd the children to each node
    for e in entries {
        let node = nodes.get(&(e.name, e.version)).unwrap();
        e.dependencies.iter().for_each(|dep| {
            // Dependencies are defined using a descriptor and
            // different dependencies could resolve to the same entry
            let resolved_dep: &Entry = pkg2entry.get(dep).unwrap_or_else(|| {
                // They must have used `resolutions` so there's no entry.
                // We should be able to find a single entry with the same name but
                // a different version.
                let resolved_pkg = pkg2entry.keys().find(|(name, _)| name == &dep.0).unwrap();
                pkg2entry.get(resolved_pkg).unwrap()
            });
            let dep_node = nodes
                .get(&(resolved_dep.name, resolved_dep.version))
                .expect("missing node, we expected to have them all by now");
            node.borrow_mut().children.push(dep_node.clone());

            // this enty is a depedency of something else, so it's not a root entry
            non_root_entries.push(resolved_dep);
        });
    }

    // Create a new vector with the root entries
    let mut roots: Vec<Rc<RefCell<Node>>> = Vec::new();
    for e in entries {
        if !non_root_entries.contains(&e) {
            roots.push(nodes.get(&(e.name, e.version)).unwrap().clone());
        }
    }

    roots
}

#[inline(always)]
fn colorize(s: &str, (r, g, b): (usize, usize, usize), color: bool) -> Cow<'_, str> {
    if s.is_empty() || !color {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(format!("\x1b[38;2;{r};{g};{b}m{s}\x1b[0m"))
    }
}

fn print_tree_node(node: &Node, is_last: bool, cols: Vec<char>, output: &mut String, color: bool) {
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
        namespace = colorize(namespace, (215, 95, 0), color),
        name = colorize(name, (215, 135, 95), color),
        at = colorize(at, (135, 175, 255), color),
        pkg_version = colorize(pkg_version, (135, 175, 255), color),
        pkg_descriptor = colorize(pkg_descriptor, (3, 150, 150), color)
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
            color,
        );
    }
}

fn print_tree(tree: &[Rc<RefCell<Node>>], color: bool) -> String {
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
            color,
        );
    }

    output
}

fn print_tree_as_json(tree: &[Rc<RefCell<Node>>]) -> serde_json::Result<String> {
    serde_json::to_string(&tree)
}

#[derive(Debug, Serialize)]
struct SerializableNode<'a> {
    #[serde(skip_serializing_if = "serialize_skip_if_children_empty")]
    children: Vec<Rc<RefCell<Node<'a>>>>,
    #[serde(serialize_with = "serialize_pkg_as_string")]
    descriptor: Pkg<'a>,
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
pub struct Node<'a> {
    #[serde(skip_serializing_if = "serialize_skip_if_children_empty")]
    children: Vec<Rc<RefCell<Node<'a>>>>,
    #[serde(
        rename(serialize = "descriptor"),
        serialize_with = "serialize_pkg_as_string"
    )]
    pkg: Pkg<'a>,
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
    visited: &mut HashMap<Pkg<'a>, bool>,
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

        if next_child_is_leaf || !visited.contains_key(&ref_node.pkg) {
            visited.insert(ref_node.pkg, true);
            _build_tree_with_no_duplicates(&mut new_node, &ref_node.children, visited)
        }
    }
}

static ROOT_PKG: (&str, &str) = ("", "");
// `Entry` is #[non_exhaustive], so we can't build it with a struct literal
// (and `Default::default()` is not const).
static ROOT_ENTRY: Lazy<Entry> = Lazy::new(Entry::default);

fn build_tree_with_no_duplicates<'a>(children: &[Rc<RefCell<Node<'a>>>]) -> Rc<RefCell<Node<'a>>> {
    let mut visited: HashMap<Pkg, bool> = HashMap::default();

    let mut root = Rc::new(RefCell::new(Node {
        children: Vec::new(),
        pkg: ROOT_PKG,
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
                        pkg: **pkg,
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
                        pkg: **pkg,
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
                    .all(|c| &&c.borrow_mut().pkg != pkg)
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
    fn strip_descriptor_protocol_works() {
        assert_eq!(strip_descriptor_protocol("^1.0.0"), "^1.0.0");
        assert_eq!(strip_descriptor_protocol("npm:^1.0.0"), "^1.0.0");
        assert_eq!(
            strip_descriptor_protocol("1.2.3 || ^2.0.0"),
            "1.2.3 || ^2.0.0"
        );
        assert_eq!(strip_descriptor_protocol("workspace:."), ".");
        // aliased dependency, the version is the one of the aliased package
        assert_eq!(strip_descriptor_protocol("npm:bar@^1.0.0"), "^1.0.0");
    }

    #[test]
    fn strip_dependency_alias_works() {
        assert_eq!(strip_dependency_alias("^1.0.0"), "^1.0.0");
        // the protocol is stripped later on, not here
        assert_eq!(strip_dependency_alias("npm:^1.0.0"), "npm:^1.0.0");
        assert_eq!(strip_dependency_alias("1.2.3 || ^2.0.0"), "1.2.3 || ^2.0.0");
        assert_eq!(strip_dependency_alias("npm:bar@^1.0.0"), "^1.0.0");
    }

    #[test]
    fn pkg_not_found() {
        let query: Pkg = ("foo", "1");
        let pkg2parents = mock_pkg2parents_empty();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(&query, &pkg2parents, &mut paths, MAX_PKG_VISITS_DEFAULT);

        let output: Vec<Vec<&Pkg>> = Vec::new();

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_no_parents() {
        let query = &PKG_A;
        let pkg2parents = mock_pkg2parents_one_element();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths, MAX_PKG_VISITS_DEFAULT);

        let output = vec![vec![&PKG_A]];

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_one_parent() {
        let query: &Pkg = &PKG_A;
        let pkg2parents = mock_pkg2parents_ab();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths, MAX_PKG_VISITS_DEFAULT);

        let output: Vec<Vec<&Pkg>> = vec![vec![&PKG_B, &PKG_A]];

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_two_parents() {
        let query: &Pkg = &PKG_A;
        let pkg2parents = mock_pkg2parents_ab_ac();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths, MAX_PKG_VISITS_DEFAULT);

        let output: Vec<Vec<&Pkg>> = vec![vec![&PKG_B, &PKG_A], vec![&PKG_C, &PKG_A]];

        assert_eq!(paths, output);
    }

    #[test]
    fn pkg_two_branches() {
        let query: &Pkg = &PKG_C;
        let pkg2parents = mock_pkg2parents_abc_abdc();
        let mut paths: Vec<Vec<&Pkg>> = Vec::new();

        build_path_to_dependency(query, &pkg2parents, &mut paths, MAX_PKG_VISITS_DEFAULT);

        let output: Vec<Vec<&Pkg>> = vec![
            vec![&PKG_A, &PKG_B, &PKG_D, &PKG_C],
            vec![&PKG_A, &PKG_B, &PKG_C],
        ];

        assert_eq!(paths, output);
    }
}
