import { useMemo, useState } from 'react';
import type { NodeView, PathStep } from '../solver/protocol';
import { actionColors, aggregate, pct } from './grid';
import { RangeGrid } from './RangeGrid';

const PLAYER = ['OOP', 'IP'];
const SUITS: Record<string, string> = { s: '♠', h: '♥', d: '♦', c: '♣' };
const RED = new Set(['h', 'd']);

export function CardText({ card }: { card: string }) {
  return (
    <span className={`cardtext${RED.has(card[1]) ? ' red' : ''}`}>
      {card[0]}
      {SUITS[card[1]]}
    </span>
  );
}

export interface PathItem {
  step: PathStep;
  label: string;
}

interface Props {
  view: NodeView;
  path: PathItem[];
  onGo: (path: PathItem[]) => void;
  /** Offered where a result file stores no strategy: re-solve this street in the browser. */
  onResolve?: (view: NodeView) => void;
}

/** Shows one node: where we are, what the acting player does, and the range grid. */
export function NodeBrowser({ view, path, onGo, onResolve }: Props) {
  const acting = view.kind === 'action' ? view.player! : null;
  const [shown, setShown] = useState<number | null>(null);
  const player = shown ?? acting ?? 0;
  const [selected, setSelected] = useState<string | null>(null);
  const agg = useMemo(() => aggregate(view, player), [view, player]);
  const colors = actionColors(view.actions, view.pot);
  const isActing = acting === player;

  const detail = selected ? agg.byClass.get(selected) : undefined;
  const hands = view.hands[player];
  const n = hands.length;

  return (
    <section className="card browser">
      <nav className="crumbs">
        <button type="button" className="link" onClick={() => onGo([])}>
          開始
        </button>
        {path.map((p, i) => (
          <span key={i}>
            <span className="muted">›</span>
            <button type="button" className="link" onClick={() => onGo(path.slice(0, i + 1))}>
              {'c' in p.step ? <CardText card={p.step.c} /> : p.label}
            </button>
          </span>
        ))}
      </nav>

      <div className="nodehead">
        <span className="board">
          {view.board.map((c) => (
            <CardText key={c} card={c} />
          ))}
        </span>
        <span>
          底池 <b>{view.pot.toFixed(2)}</b> bb
        </span>
        <span>
          籌碼 OOP {view.stacks[0].toFixed(2)} / IP {view.stacks[1].toFixed(2)}
        </span>
        <span className="muted">
          {view.kind === 'action' && `${PLAYER[acting!]} 行動`}
          {view.kind === 'chance' && '發牌：選一張牌'}
          {view.kind === 'fold' && `${PLAYER[view.player!]} 棄牌，牌局結束`}
          {view.kind === 'showdown' && '攤牌，牌局結束'}
        </span>
      </div>

      {view.kind === 'action' && view.strategy === null && (
        <div className="notice">
          結果檔沒有存這個節點的策略（河牌只在需要時重解）。
          {onResolve && view.street[0] === 0 && view.street[1] === 0 && (
            <button type="button" className="primary" onClick={() => onResolve(view)}>
              用這裡的範圍在瀏覽器重解這條街
            </button>
          )}
        </div>
      )}

      {view.kind === 'action' && view.strategy !== null && (
        <div className="actionbar">
          {view.actions.map((a, i) => (
            <button
              key={a.label}
              type="button"
              className="act"
              style={{ borderLeftColor: colors[i] }}
              onClick={() => onGo([...path, { step: { a: i }, label: `${PLAYER[acting!]} ${a.label}` }])}
            >
              {a.label}
              <span className="freq">{pct(agg.total[i] ?? 0)}</span>
            </button>
          ))}
        </div>
      )}

      {view.kind === 'chance' && (
        <div className="deck">
          {view.dealable.map((c) => (
            <button key={c} type="button" onClick={() => onGo([...path, { step: { c }, label: c }])}>
              <CardText card={c} />
            </button>
          ))}
        </div>
      )}

      <div className="tabs">
        {[0, 1].map((p) => (
          <button
            key={p}
            type="button"
            className={p === player ? 'on' : ''}
            onClick={() => setShown(p === acting ? null : p)}
          >
            {PLAYER[p]} 範圍{p === acting ? '（行動中）' : ''}
          </button>
        ))}
      </div>

      <div className="split">
        <RangeGrid agg={agg} colors={isActing ? colors : []} selected={selected} onSelect={setSelected} />
        <div className="detail">
          {!detail || detail.weight === 0 ? (
            <p className="muted small">點格子看每組 combo 的策略與 EV。顏色條是行動頻率，淡色代表到這裡的機率低。</p>
          ) : (
            <>
              <h3>
                {selected}{' '}
                <span className="muted small">
                  {detail.combos} combos{detail.ev !== null && ` · EV ${detail.ev.toFixed(2)} bb`}
                </span>
              </h3>
              <table className="combos">
                <thead>
                  <tr>
                    <th>Combo</th>
                    <th>權重</th>
                    {isActing && view.actions.map((a) => <th key={a.label}>{a.label}</th>)}
                    {view.ev && <th>EV</th>}
                  </tr>
                </thead>
                <tbody>
                  {detail.hands
                    .filter((h) => view.reach[player][h] > 0)
                    .map((h) => (
                      <tr key={hands[h]}>
                        <td>
                          <CardText card={hands[h].slice(0, 2)} />
                          <CardText card={hands[h].slice(2)} />
                        </td>
                        <td>{view.reach[player][h].toFixed(3)}</td>
                        {isActing &&
                          view.actions.map((a, i) => <td key={a.label}>{pct(view.strategy![i * n + h], 0)}</td>)}
                        {view.ev && <td>{view.ev[player][h].toFixed(2)}</td>}
                      </tr>
                    ))}
                </tbody>
              </table>
            </>
          )}
        </div>
      </div>
    </section>
  );
}
