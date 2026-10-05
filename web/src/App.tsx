import { useCallback, useRef, useState } from 'react';
import { SolverClient } from './solver/client';
import type { Estimate, NodeView, Report, SpotIn } from './solver/protocol';
import { NodeBrowser, type PathItem } from './ui/NodeBrowser';
import { SpotForm, type SolveSettings } from './ui/SpotForm';

/** Browser memory budget for one solve (PRD §5): wasm32 tops out at 4 GB. */
const BUDGET_BYTES = 3e9;

type Status = 'idle' | 'working' | 'solving' | 'ready';

function mb(bytes: number) {
  return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(2)} GB` : `${(bytes / 1e6).toFixed(1)} MB`;
}

export function App() {
  const client = useRef<SolverClient | null>(null);
  client.current ??= new SolverClient();
  const [status, setStatus] = useState<Status>('idle');
  const [error, setError] = useState<string | null>(null);
  const [estimate, setEstimate] = useState<Estimate | null>(null);
  const [progress, setProgress] = useState<{ iteration: number; report: Report | null; elapsed: number } | null>(
    null,
  );
  const [path, setPath] = useState<PathItem[]>([]);
  const [view, setView] = useState<NodeView | null>(null);

  const run = useCallback(async <T,>(f: () => Promise<T>): Promise<T | null> => {
    setError(null);
    try {
      return await f();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return null;
    }
  }, []);

  const go = useCallback(
    async (p: PathItem[]) => {
      const v = await run(() => client.current!.view(p.map((x) => x.step)));
      if (v) {
        setPath(p);
        setView(v);
      }
    },
    [run],
  );

  const onEstimate = async (spot: SpotIn) => {
    setStatus('working');
    const e = await run(() => client.current!.estimate(spot));
    if (e) setEstimate(e);
    setStatus(view ? 'ready' : 'idle');
  };

  const onSolve = async (spot: SpotIn, s: SolveSettings) => {
    setStatus('working');
    const e = await run(() => client.current!.estimate(spot));
    if (!e) return setStatus(view ? 'ready' : 'idle');
    setEstimate(e);
    if (e.bytes > BUDGET_BYTES) {
      setError(`估算 ${mb(e.bytes)}，超過瀏覽器的 ${mb(BUDGET_BYTES)} 預算。請減少下注尺寸，或改用本機 CLI 求解。`);
      return setStatus(view ? 'ready' : 'idle');
    }
    setView(null);
    setProgress(null);
    const created = await run(() => client.current!.create(spot));
    if (!created) return setStatus('idle');
    setStatus('solving');
    client.current!.onProgress = (iteration, report, elapsed) =>
      setProgress((p) => ({ iteration, report: report ?? p?.report ?? null, elapsed }));
    const r = await run(() => client.current!.solve(s.maxIter, s.targetPct));
    if (r) setProgress((p) => ({ iteration: r.iteration, report: r, elapsed: p?.elapsed ?? 0 }));
    setStatus('ready');
    await go([]);
  };

  const busy = status === 'working' || status === 'solving';

  return (
    <main>
      <header className="top">
        <h1>HEXAS Solver</h1>
        <p className="muted small">
          6-max NLHE 翻後 GTO solver（HU 底池）。僅供賽後研究，禁止在牌桌上即時使用（RTA）。瀏覽器內目前單執行緒求解。
        </p>
      </header>

      <SpotForm busy={busy} onEstimate={onEstimate} onSolve={onSolve} onError={setError} />

      {error && <p className="error">{error}</p>}

      {(estimate || progress) && (
        <section className="card status">
          {estimate && (
            <span>
              樹：{estimate.nodes.toLocaleString()} 節點（{estimate.actionNodes.toLocaleString()} 行動）· 手牌 OOP{' '}
              {estimate.hands[0]} / IP {estimate.hands[1]} · 記憶體 <b>{mb(estimate.bytes)}</b>
              {estimate.bytes > BUDGET_BYTES && <span className="bad">（超過預算）</span>}
            </span>
          )}
          {progress && (
            <span>
              iteration {progress.iteration} · {progress.elapsed.toFixed(1)} 秒
              {progress.report && (
                <>
                  {' '}
                  · exploitability <b>{progress.report.exploitabilityPct.toFixed(3)}%</b> pot（iteration{' '}
                  {progress.report.iteration}）· EV OOP {progress.report.ev[0].toFixed(2)} / IP{' '}
                  {progress.report.ev[1].toFixed(2)} bb
                </>
              )}
            </span>
          )}
          {status === 'solving' && (
            <button type="button" onClick={() => client.current!.stop()}>
              停止（保留目前結果）
            </button>
          )}
        </section>
      )}

      {view && status === 'ready' && <NodeBrowser view={view} path={path} onGo={go} />}
    </main>
  );
}
