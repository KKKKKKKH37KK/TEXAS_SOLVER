import { useMemo, useState } from 'react';
import type { SolverClient } from '../solver/client';
import type { PreflopConfigIn, PreflopReport, PreflopView } from '../solver/protocol';
import { actionColors, pct } from './grid';
import { preflopActionInfo, preflopAggregate, rangeText } from './preflop';
import { RangeGrid } from './RangeGrid';

/** Ranges and pot of a heads-up flop, handed to the postflop page. */
export interface Handoff {
  oop: string;
  ip: string;
  pot: number;
  stack: number;
  label: string;
}

interface Props {
  client: SolverClient;
  onPostflop: (h: Handoff) => void;
}

const yieldToUi = () => new Promise((r) => setTimeout(r, 0));

/** 6-max preflop solve and range browser (PRD M4). */
export function PreflopPanel({ client, onPostflop }: Props) {
  const [config, setConfig] = useState<Required<PreflopConfigIn>>({
    stack: 100,
    rakePct: 5,
    rakeCap: 3,
    realizationIp: 1.0,
    realizationOop: 0.85,
  });
  const [iters, setIters] = useState(300);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<PreflopReport | null>(null);
  const [path, setPath] = useState<{ a: number; label: string }[]>([]);
  const [view, setView] = useState<PreflopView | null>(null);
  const [shown, setShown] = useState<number | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  const go = async (p: { a: number; label: string }[]) => {
    try {
      setView(await client.preflopView(p.map((x) => x.a)));
      setPath(p);
      setShown(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const solve = async () => {
    setBusy(true);
    setError(null);
    setView(null);
    try {
      await client.preflopCreate({ ...config, rakePct: config.rakePct / 100 });
      let done = 0;
      while (done < iters) {
        const n = Math.min(25, iters - done);
        done = (await client.preflopStep(n)).iteration;
        if (done % 100 === 0 || done === iters) setReport(await client.preflopReport());
        await yieldToUi();
      }
      await go([]);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };

  const field = (key: keyof PreflopConfigIn, label: string, step: number) => (
    <label>
      {label}
      <input
        type="number"
        step={step}
        value={config[key]}
        onChange={(e) => setConfig({ ...config, [key]: Number(e.target.value) })}
      />
    </label>
  );

  const acting = view?.kind === 'action' ? view.player : null;
  const player = shown ?? acting ?? 0;
  const agg = useMemo(() => (view ? preflopAggregate(view, player) : null), [view, player]);
  const infos = view ? preflopActionInfo(view.actions) : [];
  const colors = actionColors(infos, 1.5);
  const pos = view?.positions ?? [];

  const outcome = (v: PreflopView) => {
    const pot = (v.contrib ?? []).reduce((a, b) => a + b, 0);
    if (v.kind === 'fold') return `${pos[v.players[0]]} 拿下底池（${pot.toFixed(1)} bb）`;
    const [oop, ip] = v.players;
    const what = v.kind === 'allin' ? '全下比牌' : '看翻牌';
    return `${pos[oop]}（OOP）vs ${pos[ip]}（IP）${what}，底池 ${pot.toFixed(1)} bb`;
  };

  return (
    <>
      <section className="card form">
        <h2>翻前 6-max</h2>
        <p className="muted small">
          近似解（PRD §4.2）：沒有 limp、第一個 call 就結束（只會 2 人看翻牌）、看翻牌後用 equity realization
          係數估值。係數尚未校正，結果只適合看大方向。
        </p>
        <div className="row">
          {field('stack', '籌碼 Stack (bb)', 10)}
          {field('rakePct', '抽水 Rake %', 0.5)}
          {field('rakeCap', '上限 Cap (bb)', 0.5)}
          {field('realizationIp', 'IP realization', 0.05)}
          {field('realizationOop', 'OOP realization', 0.05)}
          <label>
            iteration
            <input type="number" step={100} value={iters} onChange={(e) => setIters(Number(e.target.value))} />
          </label>
          <button type="button" className="primary" disabled={busy} onClick={solve}>
            求解翻前
          </button>
        </div>
      </section>

      {error && <p className="error">{error}</p>}

      {report && (
        <section className="card status">
          <span>
            iteration {report.iteration} · 最大 best-response 增益{' '}
            <b>{Math.max(...report.brGainBb100).toFixed(3)}</b> bb/100
          </span>
          <span className="muted">
            EV（bb/手）：{pos.length ? pos.map((p, i) => `${p} ${report.ev[i].toFixed(3)}`).join(' · ') : ''}
          </span>
        </section>
      )}

      {view && agg && !busy && (
        <section className="card browser">
          <nav className="crumbs">
            <button type="button" className="link" onClick={() => go([])}>
              開始
            </button>
            {path.map((p, i) => (
              <span key={i}>
                <span className="muted">›</span>
                <button type="button" className="link" onClick={() => go(path.slice(0, i + 1))}>
                  {p.label}
                </button>
              </span>
            ))}
          </nav>
          <div className="nodehead">
            {view.kind === 'action' ? <b>{pos[acting!]} 行動</b> : <b>{outcome(view)}</b>}
            {view.kind === 'flop' && (
              <button
                type="button"
                className="primary"
                onClick={() => {
                  const [oop, ip] = view.players;
                  const contrib = view.contrib!;
                  onPostflop({
                    oop: rangeText(view, oop),
                    ip: rangeText(view, ip),
                    pot: Number(contrib.reduce((a, b) => a + b, 0).toFixed(2)),
                    stack: Number((config.stack - contrib[oop]).toFixed(2)),
                    label: `${pos[oop]} vs ${pos[ip]}：${path.map((p) => p.label).join(' › ')}`,
                  });
                }}
              >
                帶入翻後求解
              </button>
            )}
          </div>
          {view.kind === 'action' && (
            <div className="actionbar">
              {view.actions.map((a, i) => (
                <button
                  key={a}
                  type="button"
                  className="act"
                  style={{ borderLeftColor: colors[i] }}
                  onClick={() => go([...path, { a: i, label: `${pos[acting!]} ${a}` }])}
                >
                  {a}
                  <span className="freq">{pct(agg.total[i] ?? 0)}</span>
                </button>
              ))}
            </div>
          )}
          <div className="tabs">
            {pos.map((p, i) => (
              <button
                key={p}
                type="button"
                className={i === player ? 'on' : ''}
                onClick={() => setShown(i === acting ? null : i)}
              >
                {p}
                {i === acting ? '（行動中）' : ''}
              </button>
            ))}
          </div>
          <div className="split">
            <RangeGrid
              agg={agg}
              colors={player === acting ? colors : []}
              selected={selected}
              onSelect={setSelected}
            />
            <div className="detail">
              {selected && agg.byClass.get(selected) ? (
                <>
                  <h3>{selected}</h3>
                  <p className="muted small">還在範圍內的比例：{pct(agg.byClass.get(selected)!.weight)}</p>
                  {player === acting && (
                    <table className="combos">
                      <tbody>
                        {view.actions.map((a, i) => (
                          <tr key={a}>
                            <td>{a}</td>
                            <td>{pct(agg.byClass.get(selected)!.freq[i])}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                </>
              ) : (
                <p className="muted small">點格子看該手牌的行動頻率。淡色代表這手牌在這條線上已經很少出現。</p>
              )}
            </div>
          </div>
        </section>
      )}
    </>
  );
}
