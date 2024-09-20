# yarn-why
[![version](https://img.shields.io/crates/v/yarn-why.svg)](https://crates.io/crates/yarn-why)
![ci](https://github.com/riquito/yarn-why/actions/workflows/ci.yml/badge.svg)
[![license](https://img.shields.io/crates/l/yarn-why.svg)](https://crates.io/crates/yarn-why)

`yarn why`, but fast

## Install

Download one of the [prebuilt binaries](https://github.com/riquito/yarn-why/releases)

or run

```
cargo install yarn-why
```

## Help

```
yarn-why 1.1.0

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


ARGS:
    package[@range]          Package to search for, with or without range.
                             The range must match one in yarn.lock
```

Example output, searching for `fs-minipass`

```bash
└─ vite@5.2.4 (via ^5.2.0)
   ├─ fsevents@2.3.3 (via ~2.3.3)
   │  └─ node-gyp@10.0.1 (via latest)
   │     ├─ make-fetch-happen@13.0.0 (via ^13.0.0)
   │     │  └─ cacache@18.0.2 (via ^18.0.0)
   │     │     ├─ fs-minipass@3.0.3 (via ^3.0.0)
   │     │     └─ tar@6.2.1 (via ^6.1.11)
   │     │        └─ fs-minipass@2.1.0 (via ^2.0.0)
   │     └─ tar@6.2.1 (via ^6.1.2)
   │        └─ fs-minipass@2.1.0 (via ^2.0.0)
   └─ rollup@4.13.0 (via ^4.13.0)
      └─ fsevents@2.3.3 (via ~2.3.2)
         └─ node-gyp@10.0.1 (via latest)
```

Similar search, but filtered for `fs-minipass --filter '>=3.0, <4.0.0'`

```bash
└─ vite@5.2.4 (via ^5.2.0)
   ├─ fsevents@2.3.3 (via ~2.3.3)
   │  └─ node-gyp@10.0.1 (via latest)
   │     └─ make-fetch-happen@13.0.0 (via ^13.0.0)
   │        └─ cacache@18.0.2 (via ^18.0.0)
   │           └─ fs-minipass@3.0.3 (via ^3.0.0)
   └─ rollup@4.13.0 (via ^4.13.0)
      └─ fsevents@2.3.3 (via ~2.3.2)
         └─ node-gyp@10.0.1 (via latest)
```

Defaults:
- branches are truncated at 10 levels depth (see -d / -D to change it).
- branches already printed are not printed again (search up in the tree for their output). You can get the full tree with --no-dedup.
- circular dependencies are truncated after 20 iterations (set ENV MAX_PKG_VISITS to change it).

## Benchmarks

Benchmarks run on Framework Laptop 13 AMD Ryzen 7 7840U
- node 21.7.1
- yarn 1.22.22 / yarn 4.1.0
- using [renovate 35.45.5 yarn.lock file](https://github.com/renovatebot/renovate/blob/32.45.5/yarn.lock) (v1 first, then updating it)

```bash
$ hyperfine -w 3 './target/release/yarn-why lodash' # yarn.lock v1
Benchmark 1: ./target/release/yarn-why lodash
  Time (mean ± σ):       4.9 ms ±   0.6 ms    [User: 3.8 ms, System: 1.1 ms]
  Range (min … max):     2.3 ms …   7.8 ms    398 runs

$ hyperfine -w 3 'yarn why lodash' # yarn v1.22.22 / yarn.lock v1
Benchmark 1: yarn why lodash
  Time (mean ± σ):     416.0 ms ±  76.7 ms    [User: 691.7 ms, System: 75.0 ms]
  Range (min … max):   367.9 ms … 608.7 ms    10 runs

# again, after updating the same yarn.lock to v8 using `yarn 4.1.0`

$ hyperfine -w 3 './target/release/yarn-why lodash' # yarn.lock v8
Benchmark 1: ./target/release/yarn-why lodash
  Time (mean ± σ):       6.0 ms ±   0.6 ms    [User: 4.6 ms, System: 1.4 ms]
  Range (min … max):     3.6 ms …   8.5 ms    340 runs

$ hyperfine 'yarn why lodash' # yarn v4.1.0 / yarn.lock v8
Benchmark 1: yarn why lodash
  Time (mean ± σ):     295.0 ms ±  57.7 ms    [User: 316.1 ms, System: 58.1 ms]
  Range (min … max):   229.9 ms … 361.0 ms    10 runs
```

## LICENSE

yarn-why is distributed under the GNU GPL license (version 3 or any later version).

See [LICENSE](./LICENSE) file for details.
