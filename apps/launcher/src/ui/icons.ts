// The launcher's icons, copied from the boards (docs/design/launcher/boards/): 1.8 px outline
// strokes on a 24 px grid, coloured by currentColor (DESIGN.md, Shapes). Each is decorative: the
// control or the words beside it carry the meaning.
import { s, type Child } from "../dom";

type Paths = (string | [tag: string, attrs: Record<string, string | number>])[];

function draw(paths: Paths, size: number, extra: Record<string, string | number> = {}): SVGElement {
  const children: Child[] = paths.map((p) => (typeof p === "string" ? s("path", { d: p }) : s(p[0], p[1])));
  return s(
    "svg",
    {
      "aria-hidden": "true",
      width: size,
      height: size,
      viewBox: "0 0 24 24",
      fill: "none",
      stroke: "currentColor",
      "stroke-width": 1.8,
      "stroke-linecap": "round",
      "stroke-linejoin": "round",
      class: "icon",
      ...extra,
    },
    ...children,
  );
}

/** The window controls and the check use the boards' 12 px grid. */
function small(d: string, size: number, width: number, extra: Record<string, string> = {}): SVGElement {
  return s(
    "svg",
    { "aria-hidden": "true", width: size, height: size, viewBox: "0 0 12 12", class: "icon" },
    s("path", { d, fill: "none", stroke: "currentColor", "stroke-width": width, ...extra }),
  );
}

export const icons = {
  /** Settings: an eight-toothed cog round its hub. A hub with rays reads as light and dark mode. */
  gear: (size = 18) =>
    draw(
      [
        ["circle", { cx: 12, cy: 12, r: 3 }],
        "M10.13 5.25L10.42 2.53L13.58 2.53L13.87 5.25A7.0 7.0 0 0 1 15.45 5.91L17.57 4.18L19.82 6.43L18.09 8.55A7.0 7.0 0 0 1 18.75 10.13L21.47 10.42L21.47 13.58L18.75 13.87A7.0 7.0 0 0 1 18.09 15.45L19.82 17.57L17.57 19.82L15.45 18.09A7.0 7.0 0 0 1 13.87 18.75L13.58 21.47L10.42 21.47L10.13 18.75A7.0 7.0 0 0 1 8.55 18.09L6.43 19.82L4.18 17.57L5.91 15.45A7.0 7.0 0 0 1 5.25 13.87L2.53 13.58L2.53 10.42L5.25 10.13A7.0 7.0 0 0 1 5.91 8.55L4.18 6.43L6.43 4.18L8.55 5.91A7.0 7.0 0 0 1 10.13 5.25Z",
      ],
      size,
    ),
  person: (size = 16) => draw([["circle", { cx: 12, cy: 8.5, r: 3.5 }], "M5 20c.8-3.6 3.6-5.5 7-5.5s6.2 1.9 7 5.5"], size),
  eye: (size = 18) =>
    draw(["M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z", ["circle", { cx: 12, cy: 12, r: 3 }]], size),
  eyeOff: (size = 18) =>
    draw(
      [
        "M10.6 5.6A9 9 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a16 16 0 0 1-2.6 3.4M6.6 6.6C4 8.3 2.5 12 2.5 12S6 18.5 12 18.5a9 9 0 0 0 4.4-1.1",
        "M9.9 9.9a3 3 0 0 0 4.2 4.2",
        "M3 3l18 18",
      ],
      size,
    ),
  settings: (size = 16) =>
    draw(
      [
        "M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12",
        ["circle", { cx: 16, cy: 6, r: 2 }],
        ["circle", { cx: 10, cy: 12, r: 2 }],
        ["circle", { cx: 18, cy: 18, r: 2 }],
      ],
      size,
      { "stroke-linecap": "butt", "stroke-linejoin": "miter" },
    ),
  minimise: () => small("M1 6h10", 12, 1.4),
  close: () => small("M1 1l10 10M11 1L1 11", 12, 1.4),
  check: (size = 13) => small("M2 6.4l2.6 2.4L10 3.4", size, 1.8),
  chevron: (size = 14) => draw(["M9 6l6 6-6 6"], size),
  refresh: (size = 16) => draw(["M20 12a8 8 0 1 1-2.34-5.66", "M20 4v5h-5"], size),
  plus: (size = 16) => draw(["M12 5v14M5 12h14"], size),
  trash: (size = 15) =>
    draw(["M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"], size),
  lock: (size = 14) =>
    draw([["rect", { x: 5, y: 11, width: 14, height: 10, rx: 2 }], "M8 11V8a4 4 0 0 1 8 0v3"], size),
  clock: (size = 16) => draw([["circle", { cx: 12, cy: 12, r: 8.5 }], "M12 7.5V12l3 2"], size),
  external: (size = 13) =>
    draw(["M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"], size),
  update: (size = 20) =>
    draw([["circle", { cx: 12, cy: 12, r: 9 }], "M12 16.5V8M8.5 11.5L12 8l3.5 3.5"], size),
  folder: (size = 16) =>
    draw(
      ["M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2.5h8a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 18z"],
      size,
    ),
  servers: (size = 18) =>
    draw(
      [
        ["rect", { x: 4, y: 4, width: 16, height: 7, rx: 1.5 }],
        ["rect", { x: 4, y: 13, width: 16, height: 7, rx: 1.5 }],
        "M8 7.5h.01M8 16.5h.01",
      ],
      size,
    ),
  download: (size = 18) => draw(["M12 4v11", "M7.5 10.5L12 15l4.5-4.5", "M5 20h14"], size),
  info: (size = 16) => draw([["circle", { cx: 12, cy: 12, r: 9 }], "M12 11v5.5M12 7.5v.01"], size),
  /** Errors: the circled exclamation, always with words (The Status Never Alone Rule). */
  error: (size = 16) =>
    draw([["circle", { cx: 12, cy: 12, r: 9 }], "M12 7.5v5.5M12 16.6v0.01"], size, { "stroke-width": 2 }),
  warning: (size = 16) => draw(["M12 4.5L20.5 19.5h-17z", "M12 10v4.5", "M12 17v.01"], size),
  signOut: (size = 16) =>
    draw(["M15 4h3a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-3", "M10 16l-4-4 4-4M6 12h10"], size),
  box: (size = 16) => draw(["M3 7.5L12 3l9 4.5v9L12 21l-9-4.5z", "M3 7.5l9 4.5 9-4.5M12 12v9"], size),
  shield: (size = 16) =>
    draw(
      ["M12 3l7 3v5.5c0 4.2-2.9 7.9-7 9.5-4.1-1.6-7-5.3-7-9.5V6l7-3z", "M8.8 12.2l2.2 2.2 4.3-4.6"],
      size,
    ),
  circleCheck: (size = 16) =>
    draw([["circle", { cx: 12, cy: 12, r: 8.5 }], "M8.5 12.2l2.4 2.3 4.6-4.8"], size),
};
