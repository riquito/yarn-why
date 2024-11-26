# Changelog

## [Unreleased]

## [1.2.0] - 2024-11-25

- add --full-tree option

## [1.1.0] - 2024-09-20

- yarn-why now takes a second optional argument to limit which versions to search for
- fixed a bug detecting dependencies on newer versions of yarn.lock
- fix duplication caused by dependencies using patch protocol
- support monorepos
- add a new option --print-records to get a JSONL list of what's installed

## [1.0.0-rc3] - 2022-08-01

### Added

- display the output as an ASCII tree
- new option `--dedup` to remove duplicate results
- colorize output when using a TTY
- env var MAX_PKG_VISITS can be set to bypass hypotetical infinite loops.
  It stops searching children of a package when it was visited already more than
  MAX_PKG_VISITS times

## [1.0.0-rc2] - 2022-05-10

### Added

- add option -y to to read yarn.lock from a given path

### Doc

- add some benchmarks
- better help output

### CI

- automatically merge dependencies that pass tests

### Chore

- updated dependencies

## [1.0.0-rc1] - 2022-05-02

- first release
