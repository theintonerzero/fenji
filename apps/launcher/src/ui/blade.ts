// The blade (DESIGN.md, The blade): the 焚寂 sword, drawn from the boards' own geometry
// (EmberSword, Checking, Welcome, Maintenance), behind the main area's text. It shows the
// player's copy: unlit, climbing while it's checked or fetched, lit when it's verified, and in
// bloom when Play is open. The ground behind it shows the server (The Blade And Ground Rule):
// warm when it's up and open, cool otherwise.
import { h, s } from "../dom";
import { percent as percentText } from "../words";

export type BladeState =
  | { kind: "unlit" }
  | { kind: "climbing"; percent: number; label: string }
  | { kind: "lit" }
  | { kind: "bloom" };

export type Ground = "warm" | "cool";

const WIDTH = 860;
const HEIGHT = 664;
/** The blade runs from its base at the guard (y 520) to its tip (y 88); it narrows from y 104. */
const BASE = 520;
const SHOULDER = 104;
const TIP = 88;
const BLADE = "146.5,520 153.5,520 152,104 150,88 148,104";

const GROUNDS: Record<Ground, [string, string]> = {
  warm: ["var(--vignette-warm)", "var(--vignette-warm-mid)"],
  cool: ["var(--vignette-cool)", "var(--vignette-cool-mid)"],
};

let next = 0;

/** The height the light has climbed to, for a percentage. */
function lightAt(percent: number): number {
  const p = Math.min(100, Math.max(0, percent));
  return BASE - (p / 100) * (BASE - TIP);
}

/** The lit part of the blade, from the base up to `y`. */
function litPolygon(y: number): string {
  if (y >= SHOULDER) {
    const inset = ((BASE - y) / (BASE - SHOULDER)) * 1.5;
    return `146.5,520 153.5,520 ${153.5 - inset},${y} ${146.5 + inset},${y}`;
  }
  const inset = ((SHOULDER - y) / (SHOULDER - TIP)) * 2;
  return `146.5,520 153.5,520 152,104 ${152 - inset},${y} ${148 + inset},${y} 148,104`;
}

function art(state: BladeState, ground: Ground, id: string): SVGElement {
  const [near, mid] = GROUNDS[ground];
  const lit = `url(#${id}-lit)`;
  const climb = state.kind === "climbing" ? lightAt(state.percent) : null;
  return s(
    "svg",
    { "aria-hidden": "true", width: WIDTH, height: HEIGHT, viewBox: `0 0 ${WIDTH} ${HEIGHT}` },
    s(
      "defs",
      {},
      s(
        "radialGradient",
        { id: `${id}-ground`, cx: 160, cy: 140, r: 700, gradientUnits: "userSpaceOnUse" },
        s("stop", { offset: 0, style: `stop-color: ${near}` }),
        s("stop", { offset: 0.45, style: `stop-color: ${mid}` }),
        s("stop", { offset: 1, style: "stop-color: var(--ink-950)" }),
      ),
      s(
        "linearGradient",
        { id: `${id}-lit`, x1: 0, y1: 1, x2: 0, y2: 0 },
        s("stop", { offset: 0, style: "stop-color: var(--ember-500)", "stop-opacity": 0.15 }),
        s("stop", { offset: 0.7, style: "stop-color: var(--ember-300)" }),
        s("stop", { offset: 1, style: "stop-color: var(--gold-100)" }),
      ),
      s(
        "radialGradient",
        { id: `${id}-tip`, cx: 150, cy: 96, r: 70, gradientUnits: "userSpaceOnUse" },
        s("stop", { offset: 0, style: "stop-color: var(--ember-glow)", "stop-opacity": 0.75 }),
        s("stop", { offset: 1, "stop-color": "#ff6a3d", "stop-opacity": 0 }),
      ),
    ),
    s("rect", { width: WIDTH, height: HEIGHT, fill: `url(#${id}-ground)` }),
    state.kind === "bloom" ? s("circle", { cx: 150, cy: 96, r: 70, fill: `url(#${id}-tip)` }) : null,
    state.kind === "unlit" || state.kind === "climbing"
      ? s("polygon", { points: BLADE, style: "fill: var(--blade-steel)" })
      : s("polygon", { points: BLADE, fill: lit }),
    climb === null
      ? null
      : [
          s("polygon", { points: litPolygon(climb), fill: lit }),
          s("rect", { x: 137, y: climb, width: 7, height: 1, style: "fill: var(--gold-100)", opacity: 0.7 }),
        ],
    s("rect", { x: 128, y: 520, width: 44, height: 5, rx: 2.5, style: "fill: var(--brass)" }),
    s("rect", { x: 147.5, y: 525, width: 5, height: 44, rx: 1.5, style: "fill: var(--blade-grip)" }),
    s("circle", { cx: 150, cy: 574, r: 5, style: "fill: var(--brass)" }),
    state.kind === "bloom"
      ? s(
          "g",
          { style: "fill: var(--ember-glow)" },
          s("circle", { cx: 172, cy: 128, r: 1.6, opacity: 0.8 }),
          s("circle", { cx: 132, cy: 150, r: 1.2, opacity: 0.6 }),
          s("circle", { cx: 186, cy: 84, r: 1, opacity: 0.7 }),
          s("circle", { cx: 118, cy: 104, r: 1.4, opacity: 0.5 }),
          s("circle", { cx: 164, cy: 60, r: 1, opacity: 0.6 }),
        )
      : null,
  );
}

/** The readout left of the blade while it climbs: the percentage and what's being counted. */
function readout(state: { percent: number; label: string }): HTMLElement {
  return h(
    "div",
    { class: "readout", style: `top: ${Math.round(lightAt(state.percent)) - 12}px` },
    h("div", { class: "readout-value" }, percentText(state.percent)),
    h("div", { class: "readout-label" }, state.label),
  );
}

export function blade(state: BladeState, ground: Ground = "cool"): HTMLElement & {
  update(state: BladeState, ground?: Ground): void;
} {
  next += 1;
  const id = `blade-${next}`;
  const el = h("div", { class: "blade" });
  const update = (to: BladeState, on: Ground = ground): void => {
    for (const name of ["role", "aria-valuemin", "aria-valuemax", "aria-valuenow", "aria-valuetext", "aria-hidden"]) {
      el.removeAttribute(name);
    }
    if (to.kind === "climbing") {
      const now = Math.round(Math.min(100, Math.max(0, to.percent)));
      el.setAttribute("role", "progressbar");
      el.setAttribute("aria-valuemin", "0");
      el.setAttribute("aria-valuemax", "100");
      el.setAttribute("aria-valuenow", String(now));
      el.setAttribute("aria-valuetext", `${now}%, ${to.label}`);
      el.replaceChildren(art(to, on, id), readout(to));
    } else {
      el.setAttribute("aria-hidden", "true");
      el.replaceChildren(art(to, on, id));
    }
  };
  update(state);
  return Object.assign(el, { update });
}
