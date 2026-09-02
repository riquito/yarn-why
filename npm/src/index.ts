/**
 * yarn-why as a library: work out why a package ended up in a `yarn.lock`.
 *
 * The analysis runs in WebAssembly, compiled from the same Rust code as the
 * `yarn-why` CLI, so the answers match what the CLI prints.
 */

import { load } from "#wasm-loader";
import { whyJson, whyText as whyTextRaw, records as recordsRaw } from "../wasm/yarn_why.js";

/** A package in a dependency path. */
export interface WhyNode {
  /** The package name and the range its parent asked for. */
  descriptor: [name: string, range: string];
  /** The version that range resolved to. */
  version: string;
  /** Absent on leaves. */
  children?: WhyNode[];
}

/** One (name, version, descriptor) triple from the lockfile. */
export interface LockRecord {
  name: string;
  version: string;
  descriptor: string;
}

export interface WhyOptions {
  /**
   * Truncate paths at this depth. Defaults to 10; pass `noMaxDepth` to
   * follow them all the way up.
   */
  maxDepth?: number;
  /** Ignore `maxDepth` entirely. Defaults to false. */
  noMaxDepth?: boolean;
  /**
   * Show each package at most once. Defaults to true.
   *
   * This is also what stops the walk from going around dependency cycles
   * forever, so turning it off on a large lockfile can use a lot of memory.
   */
  dedup?: boolean;
  /** Return every dependency rather than the paths to one package. */
  fullTree?: boolean;
  /** Only consider versions matching this semver range, e.g. `"^4.17.0"`. */
  range?: string;
  /**
   * How many times one package may be visited before the walk gives up on
   * it, a safety net for cyclic graphs. Defaults to 20.
   */
  maxPkgVisits?: number;
}

let ready: Promise<void> | undefined;

/**
 * Compile and instantiate the WebAssembly module.
 *
 * Every other function does this for you; call it directly only to front-load
 * the cost (~10ms) before a latency-sensitive stretch.
 */
export function init(): Promise<void> {
  ready ??= load();
  return ready;
}

/**
 * `JSON.stringify` drops `undefined` values, so absent options simply fall
 * back to the defaults on the Rust side.
 */
function encodeOptions(options: WhyOptions = {}): string {
  return JSON.stringify({
    maxDepth: options.maxDepth,
    noMaxDepth: options.noMaxDepth,
    dedup: options.dedup,
    fullTree: options.fullTree,
    range: options.range,
    maxPkgVisits: options.maxPkgVisits,
  });
}

/**
 * The paths through which `packageName` is installed, as a tree.
 *
 * Returns an empty array when the package is not in the lockfile, or is
 * there but outside `options.range`.
 *
 * @param lockfile the contents of a `yarn.lock` (v1 or berry)
 */
export async function why(
  lockfile: string,
  packageName: string,
  options?: WhyOptions,
): Promise<WhyNode[]> {
  await init();
  const json = whyJson(lockfile, packageName, encodeOptions(options));
  return json === undefined ? [] : (JSON.parse(json) as WhyNode[]);
}

/**
 * The same answer as {@link why}, rendered as the ASCII tree the CLI prints.
 *
 * Returns `undefined` when the package is not found.
 */
export async function whyText(
  lockfile: string,
  packageName: string,
  options?: WhyOptions,
): Promise<string | undefined> {
  await init();
  return whyTextRaw(lockfile, packageName, encodeOptions(options));
}

/** Every package in the lockfile, flattened to one record per descriptor. */
export async function records(lockfile: string): Promise<LockRecord[]> {
  await init();
  return JSON.parse(recordsRaw(lockfile)) as LockRecord[];
}
