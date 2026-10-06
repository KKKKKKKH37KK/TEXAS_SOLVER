import { useCallback, useRef, useState } from 'react';
import { SolverClient } from './solver/client';
import type { Estimate, NodeView, Report, ResultHeader, Source, SpotIn } from './solver/protocol';
import { toComboWeights } from './ui/cards';
import { libraryUrl } from './ui/library';
import { NodeBrowser, type PathItem } from './ui/NodeBrowser';
import { type Handoff, PreflopPanel } from './ui/PreflopPanel';
import { SpotForm, type SolveSettings } from './ui/SpotForm';

/** Browser memory budget for one solve (PRD §5): wasm32 tops out at 4 GB. */
const BUDGET_BYTES = 3e9;

type Status = 'idle' | 'working' | 'solving' | 'ready';

interface Browse {
  path: PathItem[];
  view: NodeView;
}

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
  const [mode, setMode] = useState<Source>('solve');
  const [solved, setSolved] = useState<Browse | null>(null);
  const [imported, setImported] = useState<Browse | null>(null);
  const [header, setHeader] = useState<ResultHeader | null>(null);
  const [subgame, setSubgame] = useState<string | null>(null);
  const [page, setPage] = useState<'postflop' | 'preflop'>('postflop');
  const [incoming, setIncoming] = useState<(Handoff & { id: number }) | null>(null);

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
    async (source: Source, path: PathItem[]) => {
      const view = await run(() => client.current!.view(path.map((x) => x.step), source));
      if (!view) return;
      (source === 'solve' ? setSolved : setImported)({ path, view });
    },
    [run],
  );

  const onEstimate = async (spot: SpotIn) => {
    setStatus('working');
    const e = await run(() => client.current!.estimate(spot));
    if (e) setEstimate(e);
    setStatus('idle');
  };

  const solve = async (spot: SpotIn, s: SolveSettings, label: string | null) => {
    setStatus('working');
    const e = await run(() => client.current!.estimate(spot));
    if (!e) return setStatus('idle');
    setEstimate(e);
    if (e.bytes > BUDGET_BYTES) {
      setError(`估算 ${mb(e.bytes)}，超過瀏覽器的 ${mb(BUDGET_BYTES)} 預算。請減少下注尺寸，或改用本機 CLI 求解。`);
      return setStatus('idle');
    }
    setSolved(null);
    setProgress(null);
    setMode('solve');
    setSubgame(label);
    const created = await run(() => client.current!.create(spot));
    if (!created) return setStatus('idle');
    setStatus('solving');
    client.current!.onProgress = (iteration, report, elapsed) =>
      setProgress((p) => ({ iteration, report: report ?? p?.report ?? null, elapsed }));
    const r = await run(() => client.current!.solve(s.maxIter, s.targetPct));
    if (r) setProgress((p) => ({ iteration: r.iteration, report: r, elapsed: p?.elapsed ?? 0 }));
    setStatus('idle');
    await go('solve', []);
  };

  const onOpen = async (file: File) => {
    setStatus('working');
    const res = await run(async () => client.current!.load(await file.arrayBuffer()));
    setStatus('idle');
    if (!res) return;
    setHeader(res.header);
    setMode('import');
    await go('import', []);
  };

  /** Opens a library flop: the file of its suit-isomorphic representative, shown in real suits. */
  const onLibrary = async (line: string, flop: string, label: string) => {
    setStatus('working');
    const res = await run(async () => {
      const c = await client.current!.canonicalFlop(flop);
      const r = await fetch(libraryUrl(line, c.name));
      if (!r.ok) {
        throw new Error(
          `解庫裡還沒有這個翻牌（${line} / ${c.name}，HTTP ${r.status}）。解庫還在建置或上傳中。`,
        );
      }
      const loaded = await client.current!.load(await r.arrayBuffer());
      await client.current!.importMap(c.map);
      return loaded;
    });
    setStatus('idle');
    if (!res) return;
    setHeader(res.header);
    setSubgame(label);
    setMode('import');
    setPage('postflop');
    await go('import', []);
  };

  /** Re-solves the street that starts at an imported node, from both players' reach there. */
  const resolveHere = (view: NodeView) => {
    if (!header) return;
    const spot: SpotIn = {
      ...header.spot,
      board: view.board.join(''),
      oop: '',
      ip: '',
      oopWeights: toComboWeights(view.hands[0], view.reach[0]),
      ipWeights: toComboWeights(view.hands[1], view.reach[1]),
      pot: view.pot,
      stack: Math.min(view.stacks[0], view.stacks[1]),
      // The PRD turn/river preset (33/66/100/125 %, up to 3 raises), finer than the library's
      // single 66 % size: the browser can afford it for one street.
      sizes: undefined,
    };
    const path = imported?.path.map((p) => ('c' in p.step ? p.step.c : p.label)).join(' › ') ?? '';
    void solve(spot, { targetPct: 1.0, maxIter: 1000 }, `子局：${path}`);
  };

  const busy = status === 'working' || status === 'solving';
  const shown = mode === 'solve' ? solved : imported;

  const pages = (
    <div className="tabs pages">
      <button type="button" className={page === 'postflop' ? 'on' : ''} onClick={() => setPage('postflop')}>
        翻後 Postflop
      </button>
      <button type="button" className={page === 'preflop' ? 'on' : ''} onClick={() => setPage('preflop')}>
        翻前 Preflop
      </button>
    </div>
  );

  return (
    <main>
      {/* Both pages stay mounted so switching keeps their results. */}
      <div hidden={page !== 'preflop'}>
        <header className="top">
          <div className="titlebar">
            <h1>HEXAS Solver</h1>
            {pages}
          </div>
        </header>
        <PreflopPanel
          client={client.current}
          onPostflop={(h) => {
            setIncoming({ ...h, id: Date.now() });
            setPage('postflop');
          }}
          onLibrary={onLibrary}
        />
        {error && <p className="error">{error}</p>}
      </div>
      <div hidden={page !== 'postflop'}>
      <header className="top">
        <div className="titlebar">
          <h1>HEXAS Solver</h1>
          {pages}
          <label className="file">
            開啟結果檔（.hxs）
            <input
              type="file"
              accept=".hxs"
              disabled={busy}
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) void onOpen(f);
                e.target.value = '';
              }}
            />
          </label>
        </div>
        <p className="muted small">
          6-max NLHE 翻後 GTO solver（HU 底池）。僅供賽後研究，禁止在牌桌上即時使用（RTA）。瀏覽器內目前單執行緒求解；
          翻牌局面請用本機 CLI <code>hexas solve … --out 檔名.hxs</code> 解完再開啟。
        </p>
      </header>

      <SpotForm
        busy={busy}
        incoming={incoming}
        onEstimate={onEstimate}
        onSolve={(spot, s) => solve(spot, s, incoming?.label ?? null)}
        onError={setError}
      />

      {error && <p className="error">{error}</p>}

      {(estimate || progress) && (
        <section className="card status">
          {subgame && <span className="muted">{subgame}</span>}
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

      {header && (
        <section className="card status">
          <span>
            結果檔：<b>{header.spot.board}</b> · 底池 {header.spot.pot} bb · 籌碼 {header.spot.stack} bb ·{' '}
            {header.iterations} iterations · exploitability {header.exploitabilityPct.toFixed(3)}% pot · EV OOP{' '}
            {header.ev[0].toFixed(2)} / IP {header.ev[1].toFixed(2)} bb
          </span>
          <span className="muted small">
            {header.maxBoard === 3
              ? '解庫只存翻牌策略（檔名是花色同構的代表翻牌）；轉牌和河牌在瀏覽器用當下範圍重解。'
              : `策略存到${header.maxBoard === 4 ? '轉牌' : '河牌'}；河牌可在瀏覽器重解。`}
          </span>
        </section>
      )}

      {solved && header && (
        <div className="tabs modes">
          <button type="button" className={mode === 'solve' ? 'on' : ''} onClick={() => setMode('solve')}>
            瀏覽器求解結果
          </button>
          <button type="button" className={mode === 'import' ? 'on' : ''} onClick={() => setMode('import')}>
            結果檔
          </button>
        </div>
      )}

      {shown && !busy && (
        <NodeBrowser
          key={mode}
          view={shown.view}
          path={shown.path}
          onGo={(p) => go(mode, p)}
          onResolve={mode === 'import' ? resolveHere : undefined}
        />
      )}
      </div>
    </main>
  );
}
