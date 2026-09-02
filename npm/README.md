# @riquito/yarn-why

Find out why a package ended up in your `yarn.lock`, from JavaScript.

This is the [yarn-why](https://github.com/riquito/yarn-why) engine compiled to
WebAssembly, so the answers are exactly the ones the CLI prints. It works in
Node and in the browser, takes the lockfile as a string, and never touches the
filesystem itself.

```sh
npm install @riquito/yarn-why
```

## Usage

```js
import { readFile } from "node:fs/promises";
import { why, whyText, records } from "@riquito/yarn-why";

const lockfile = await readFile("yarn.lock", "utf8");

await why(lockfile, "lodash");
// [
//   {
//     descriptor: ["vite", "^5.2.0"],
//     version: "5.2.4",
//     children: [{ descriptor: ["lodash", "^4.17.0"], version: "4.17.21" }],
//   },
// ]

console.log(await whyText(lockfile, "lodash"));
// └─ vite@5.2.4 (via ^5.2.0)
//    └─ lodash@4.17.21 (via ^4.17.0)
```

Both v1 and berry lockfiles are understood.

## API

Every function compiles the WebAssembly module on first use. Call `init()`
yourself only to pay that cost (~10ms) up front.

### `why(lockfile, packageName, options?): Promise<WhyNode[]>`

The paths through which `packageName` is installed. Each `WhyNode` is:

```ts
interface WhyNode {
  /** the package name, and the range whoever depends on it asked for */
  descriptor: [name: string, range: string];
  /** the version that range resolved to */
  version: string;
  /** absent on leaves */
  children?: WhyNode[];
}
```

The queried package is the *leaf* of each path, not the root — you are looking
at who pulls it in. Returns `[]` when the package is not in the lockfile, or is
there but outside `options.range`.

### `whyText(lockfile, packageName, options?): Promise<string | undefined>`

The same answer as the ASCII tree the CLI prints, or `undefined` when the
package is not found.

### `records(lockfile): Promise<LockRecord[]>`

Every package in the lockfile, one record per descriptor
(`{ name, version, descriptor }`). The CLI's `--print-records` emits the same
records as JSONL.

### Options

| option         | default | meaning                                              |
| -------------- | ------- | ---------------------------------------------------- |
| `maxDepth`     | `10`    | truncate paths at this depth                          |
| `noMaxDepth`   | `false` | follow paths all the way up, ignoring `maxDepth`      |
| `dedup`        | `true`  | show each package at most once                        |
| `fullTree`     | `false` | return every dependency instead of paths to one       |
| `range`        | —       | only consider versions matching this semver range     |
| `maxPkgVisits` | `20`    | give up on a package after this many visits           |

`dedup` is also what stops the walk from going around dependency cycles
forever. Turning it off on a large lockfile can use a lot of memory.

## Errors

A lockfile that cannot be parsed, or an invalid `range`, rejects the promise.
A package that simply is not there is not an error: `why` returns `[]` and
`whyText` returns `undefined`.

## Requirements

Node 20 or later. In the browser, any bundler that understands
`new URL(..., import.meta.url)` will pick the `.wasm` file up.

## License

GPL-3.0-or-later, same as yarn-why itself.
