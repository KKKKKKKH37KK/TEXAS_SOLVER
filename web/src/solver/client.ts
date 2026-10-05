/** Promise-based access to the solver worker. */
import type {
  Estimate,
  FromWorker,
  NodeView,
  PathStep,
  Report,
  Request,
  ResultHeader,
  Source,
  SpotIn,
  ToWorker,
} from './protocol';

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void };

export class SolverClient {
  private worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
  private pending = new Map<number, Pending>();
  private nextId = 1;
  onProgress: ((iteration: number, r: Report | null, elapsed: number) => void) | null = null;

  constructor() {
    this.worker.onmessage = (e: MessageEvent<FromWorker>) => {
      const m = e.data;
      if ('type' in m) {
        this.onProgress?.(m.iteration, m.report, m.elapsed);
        return;
      }
      const p = this.pending.get(m.id);
      if (!p) return;
      this.pending.delete(m.id);
      if (m.ok) p.resolve(m.reply);
      else p.reject(new Error(m.error));
    };
  }

  private send<T>(msg: ToWorker & { id: number }): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      this.pending.set(msg.id, { resolve: resolve as (v: unknown) => void, reject });
      this.worker.postMessage(msg);
    });
  }

  private call<T>(req: Request): Promise<T> {
    return this.send<T>({ id: this.nextId++, type: 'call', req });
  }

  estimate(spot: SpotIn) {
    return this.call<Estimate>({ cmd: 'estimate', spot });
  }

  create(spot: SpotIn) {
    return this.call<{ bytes: number; hands: [number, number] }>({ cmd: 'create', spot });
  }

  solve(maxIter: number, targetPct: number) {
    return this.send<Report>({ id: this.nextId++, type: 'solve', maxIter, targetPct });
  }

  stop() {
    this.worker.postMessage({ type: 'stop' } satisfies ToWorker);
  }

  view(path: PathStep[], source: Source = 'solve', ev = true) {
    return this.call<NodeView>({ cmd: 'view', path, ev, source });
  }

  /** Loads a result file (.hxs) into the import session. The buffer is transferred. */
  load(bytes: ArrayBuffer) {
    const id = this.nextId++;
    return new Promise<{ header: ResultHeader; storedNodes: number }>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.worker.postMessage({ id, type: 'load', bytes } satisfies ToWorker, [bytes]);
    });
  }
}
