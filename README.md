# yarn-why

`yarn why`, but fast

## Install

Download one of the [prebuilt binaries](https://github.com/riquito/yarn-why/releases)

or run

```
cargo install yarn-why
```

## Help

```
yarn-why 0.1.0

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
```


## LICENSE

yarn-why is distributed under the GNU GPL license (version 3 or any later version).

See [LICENSE](./LICENSE) file for details.
