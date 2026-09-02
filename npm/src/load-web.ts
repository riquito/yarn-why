import init, { setPanicHook } from "../wasm/yarn_why.js";

export async function load(): Promise<void> {
  const wasm = new URL("../wasm/yarn_why_bg.wasm", import.meta.url);
  await init({ module_or_path: wasm });
  setPanicHook();
}
