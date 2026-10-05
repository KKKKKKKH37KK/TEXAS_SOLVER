/** Types shared by the UI, the worker and the wasm JSON interface (crates/wasm/src/lib.rs). */

export interface StreetSizes {
  /** Pot fractions, e.g. [0.33, 0.66]. */
  bets: number[];
  raiseMult: number;
  maxRaises: number;
}

export interface SpotIn {
  board: string;
  oop: string;
  ip: string;
  /** Explicit weights per combo (1326), e.g. reach from a parent solve; replace the range text. */
  oopWeights?: number[];
  ipWeights?: number[];
  pot: number;
  stack: number;
  /** Flop, turn, river. Omitted: the PRD preset for the board. */
  sizes?: [StreetSizes, StreetSizes, StreetSizes];
  donk?: boolean;
  rakePct?: number;
  rakeCap?: number;
  allinThreshold?: number;
  isomorphism?: boolean;
}

export interface Estimate {
  nodes: number;
  actionNodes: number;
  bytes: number;
  hands: [number, number];
}

export interface Report {
  iteration: number;
  ev: [number, number];
  exploitability: number;
  exploitabilityPct: number;
}

export type ActionKind = 'fold' | 'check' | 'call' | 'bet' | 'raise' | 'allin';

export interface ActionInfo {
  kind: ActionKind;
  amount: number;
  label: string;
}

/** A path step: take action `a`, or deal card `c` ("5h"). */
export type PathStep = { a: number } | { c: string };

export interface NodeView {
  kind: 'action' | 'chance' | 'fold' | 'showdown';
  player: number | null;
  actions: ActionInfo[];
  board: string[];
  pot: number;
  stacks: [number, number];
  street: [number, number];
  hands: [string[], string[]];
  reach: [number[], number[]];
  /** [action][hand] for the player to act. */
  strategy: number[] | null;
  ev: [number[], number[]] | null;
  dealable: string[];
}

export type Request =
  | { cmd: 'estimate'; spot: SpotIn }
  | { cmd: 'create'; spot: SpotIn }
  | { cmd: 'step'; n: number }
  | { cmd: 'report' }
  | { cmd: 'view'; path: PathStep[]; ev?: boolean; source?: Source }
  | { cmd: 'destroy' }
  | { cmd: 'preflopCreate'; config?: PreflopConfigIn }
  | { cmd: 'preflopStep'; n: number }
  | { cmd: 'preflopReport' }
  | { cmd: 'preflopView'; path: number[] }
  | { cmd: 'canonicalFlop'; board: string }
  | { cmd: 'importMap'; map: number[] };

export interface PreflopConfigIn {
  stack?: number;
  rakePct?: number;
  rakeCap?: number;
}

export interface PreflopReport {
  iteration: number;
  /** bb per hand, per position. */
  ev: number[];
  /** What each position gains by best responding, bb/100. */
  brGainBb100: number[];
}

export interface PreflopView {
  kind: 'action' | 'fold' | 'allin' | 'flop';
  player: number | null;
  positions: string[];
  /** Labels such as "Fold", "Call", "Raise 2.5", "All-in 100". */
  actions: string[];
  /** [action][class] for the player to act. */
  strategy: number[] | null;
  /** [position][class], combo-weighted. */
  reach: number[][];
  classes: string[];
  /** Chips put in per position, at terminals. */
  contrib: number[] | null;
  /** Winner (fold) or the two players left (allin / flop: [OOP, IP]). */
  players: number[];
}

/** Which session a view reads: the live solve or an imported result file. */
export type Source = 'solve' | 'import';

/** Header of a result file written by `hexas solve --out` (crates/core/src/export.rs). */
export interface ResultHeader {
  spot: SpotIn;
  iterations: number;
  exploitabilityPct: number;
  ev: [number, number];
  nodes: number;
  hands: [number, number];
  maxBoard: number;
}

export type ToWorker =
  | { id: number; type: 'call'; req: Request }
  | { id: number; type: 'load'; bytes: ArrayBuffer }
  | { id: number; type: 'solve'; maxIter: number; targetPct: number }
  | { type: 'stop' };

export type FromWorker =
  | { id: number; ok: true; reply: unknown }
  | { id: number; ok: false; error: string }
  /** `report` is null between (expensive) exploitability measurements. */
  | { type: 'progress'; iteration: number; report: Report | null; elapsed: number };
