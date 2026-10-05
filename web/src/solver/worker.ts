/// <reference lib="webworker" />
// Runs the wasm solver off the main thread. Replies to calls by id; a solve loop posts progress and
// yields between chunks so a 'stop' message can get through.
import wasmUrl from '../wasm/hexas.wasm?url';
import type { FromWorker, Report, Request, ToWorker } from './protocol';

interface Exports {
  memory: WebAssembly.Memory;
  hx_alloc(len: number): number;
  hx_free(ptr: number, len: number): void;
  hx_call(ptr: number, len: number): number;
  hx_load(ptr: number, len: number): number;
  hx_out_ptr(): number;
  hx_out_len(): number;
}

const ctx = self as unknown as DedicatedWorkerGlobalScope;
const enc = new TextEncoder();
const dec = new TextDecoder();
let wasm: Promise<Exports> | null = null;
let stopRequested = false;

function load(): Promise<Exports> {
  wasm ??= WebAssembly.instantiateStreaming(fetch(wasmUrl), {}).then((r) => r.instance.exports as unknown as Exports);
  return wasm;
}

/** Copies `data` into wasm memory, runs `entry` on it, and parses the JSON reply. */
async function invoke<T>(data: Uint8Array, entry: (x: Exports, ptr: number, len: number) => number): Promise<T> {
  const x = await load();
  const ptr = x.hx_alloc(data.length);
  new Uint8Array(x.memory.buffer, ptr, data.length).set(data);
  const code = entry(x, ptr, data.length);
  x.hx_free(ptr, data.length);
  const reply = JSON.parse(dec.decode(new Uint8Array(x.memory.buffer, x.hx_out_ptr(), x.hx_out_len())));
  if (code !== 0) throw new Error(reply.error);
  return reply as T;
}

const call = <T>(req: Request) => invoke<T>(enc.encode(JSON.stringify(req)), (x, p, n) => x.hx_call(p, n));
const loadFile = <T>(bytes: ArrayBuffer) => invoke<T>(new Uint8Array(bytes), (x, p, n) => x.hx_load(p, n));

const yieldToEvents = () => new Promise((r) => setTimeout(r, 0));

/**
 * Iterates in chunks of about 300 ms until the target, the iteration cap, or a stop request.
 * Exploitability costs about two iterations to compute, so it is measured at most every 5× its own
 * cost (under ~20 % of the time); progress in between reports just the iteration count.
 */
async function solve(maxIter: number, targetPct: number): Promise<Report> {
  stopRequested = false;
  const t0 = performance.now();
  const elapsed = () => (performance.now() - t0) / 1000;
  const measure = async () => {
    const t = performance.now();
    const r = await call<Report>({ cmd: 'report' });
    return { r, cost: performance.now() - t };
  };
  let { r: report, cost } = await measure();
  let iteration = report.iteration;
  let lastReport = performance.now();
  let chunk = 1;
  while (iteration < maxIter && !stopRequested) {
    const n = Math.min(chunk, maxIter - iteration);
    const t = performance.now();
    iteration = (await call<{ iteration: number }>({ cmd: 'step', n })).iteration;
    const perIter = (performance.now() - t) / n;
    chunk = Math.max(1, Math.min(500, Math.round(300 / Math.max(perIter, 0.01))));
    const due = performance.now() - lastReport > Math.max(250, 5 * cost) || iteration >= maxIter;
    if (due) {
      ({ r: report, cost } = await measure());
      lastReport = performance.now();
    }
    ctx.postMessage({ type: 'progress', iteration, report: due ? report : null, elapsed: elapsed() } satisfies FromWorker);
    if (due && report.exploitabilityPct <= targetPct) break;
    await yieldToEvents();
  }
  if (report.iteration !== iteration) {
    report = (await measure()).r;
    ctx.postMessage({ type: 'progress', iteration, report, elapsed: elapsed() } satisfies FromWorker);
  }
  return report;
}

ctx.onmessage = async (e: MessageEvent<ToWorker>) => {
  const m = e.data;
  if (m.type === 'stop') {
    stopRequested = true;
    return;
  }
  try {
    const reply =
      m.type === 'call' ? await call(m.req) : m.type === 'load' ? await loadFile(m.bytes) : await solve(m.maxIter, m.targetPct);
    ctx.postMessage({ id: m.id, ok: true, reply } satisfies FromWorker);
  } catch (err) {
    ctx.postMessage({ id: m.id, ok: false, error: err instanceof Error ? err.message : String(err) } satisfies FromWorker);
  }
};
