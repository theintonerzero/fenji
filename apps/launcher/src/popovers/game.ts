// The Game popover (the redesign's Your game boards): the player's copy, as it is. Verified, with
// where it is, when, Verify integrity and Change folder. Set up: the Steam library first, then
// another folder, then a download. Downloading or verifying, with its progress; while a download
// waits for its Steam sign-in, the code DepotDownloader drew, to scan (Amendment A, LA4), or the
// player's own command for a password sign-in (LA5). Needs repair, with each file and Repair. Its
// actions' refusals show in it, under its sections.
import type { Ctx, PopoverContent } from "../app";
import { h, s as svg, type Child } from "../dom";
import type { CodeView, FileProblem, Intent, JobView, Snapshot } from "../types";
import { button } from "../ui/button";
import { commandDialog } from "../ui/command-dialog";
import { formError, pathEnd, refused, renew } from "../screens/parts";
import { percent, when } from "../words";

export const GAME = "Swords of Legends Online";

type View = "working" | "setup" | "repair" | "missing" | "ready" | "unchecked";

function viewFor(s: Snapshot): View {
  // A start's fetches, umu-launcher's and GE-Proton's, are PLAY's, not the copy's.
  if (s.job !== null && !(s.job.kind === "fetching" && s.game.kind === "starting")) {
    return "working";
  }
  if (s.client.folder === null) {
    return "setup";
  }
  switch (s.client.check.kind) {
    case "needsRepair":
      return "repair";
    case "folderMissing":
      return "missing";
    case "verified":
      return "ready";
    case "none":
      return "unchecked";
  }
}

const PROBLEM: Record<FileProblem["problem"], string> = {
  missing: "Missing",
  wrongSize: "Wrong size",
  wrongHash: "Changed",
};

/** The files a repair names, at most this many; the rest as "and 4 more". */
const FILES = 6;

function section(...children: Child[]): HTMLElement {
  return h("section", { class: "sec" }, ...children);
}

function big(words: Child): HTMLElement {
  return h("p", { class: "big" }, words);
}

function line(...words: Child[]): HTMLElement {
  return h("p", { class: "line" }, ...words);
}

function acts(...buttons: Child[]): HTMLElement {
  return h("div", { class: "acts" }, ...buttons);
}

function dot(tone: "ok" | "quiet"): HTMLElement {
  return h("span", { class: `dot dot-${tone}`, "aria-hidden": "true" });
}

/** A job's words, its progress and what stops it. */
function work(job: JobView): { title: string; percent: number | null; stop: [string, Intent]; line: string | null } {
  switch (job.kind) {
    case "checking":
      return { title: "Verifying", percent: job.percent, stop: ["Stop", { type: "stopCheck" }], line: null };
    case "fetching":
      return { title: `Getting ${job.tool}`, percent: job.percent, stop: ["Cancel", { type: "cancelDownload" }], line: null };
    case "downloading":
      return {
        title: job.repair ? "Repairing" : "Downloading",
        percent: job.percent,
        stop: ["Cancel", { type: "cancelDownload" }],
        // A code to scan carries its own words.
        line: !job.waitingForSteam || job.code !== null ? null : "Connecting to Steam…",
      };
  }
}

/**
 * A sign-in code as DepotDownloader drew it, as squares (Amendment A, LA4): one path of each row's
 * runs of dark modules, `M x y h n v1 h-n z`, over the tile's light ground. Fenji never decodes it.
 */
function drawCode(code: CodeView): SVGElement {
  const { size, dark } = code;
  let d = "";
  for (let y = 0; y < size; y += 1) {
    let x = 0;
    while (x < size) {
      if (dark[y * size + x] !== "1") {
        x += 1;
        continue;
      }
      const start = x;
      while (x < size && dark[y * size + x] === "1") {
        x += 1;
      }
      d += `M${start} ${y}h${x - start}v1h-${x - start}z`;
    }
  }
  return svg(
    "svg",
    { class: "code", viewBox: `0 0 ${size} ${size}`, role: "img", "aria-label": "Steam sign-in code", "shape-rendering": "crispEdges" },
    svg("path", { d }),
  );
}

export function gamePopover(ctx: Ctx): PopoverContent {
  const problem = formError();
  const content = h("div", { class: "sections" });

  /** Sends an intent, and shows its refusal here. */
  const act = (intent: Intent) => async (): Promise<void> => {
    problem.set(null);
    const refusal = await ctx.send(intent);
    if (refusal !== null) {
      refused(ctx, null, refusal, (words) => problem.set(words));
    }
  };
  const pick = act({ type: "pickFolder", purpose: "game" });
  const runIt = (command: string, s: Snapshot, then: "chooseFolder" | "check"): HTMLButtonElement =>
    button.link("Run it yourself", () => ctx.show((done) => commandDialog(ctx, command, s.client.manual?.get ?? null, then, done)));

  // While a job runs, its section stays the same elements, so Cancel keeps focus as progress streams.
  let stopIntent: Intent | null = null;
  const workTitle = h("span", {});
  const workPercent = h("span", { class: "num" });
  const fill = h("i", {});
  const bar = h("div", { class: "bar", role: "progressbar", "aria-valuemin": "0", "aria-valuemax": "100" }, fill);
  const workLine = line();
  const stop = button.secondary("", () => {
    if (stopIntent !== null) {
      void act(stopIntent)();
    }
  }, true);
  // While DepotDownloader waits for a Steam sign-in, a player without the Steam Mobile App signs in
  // with their password through their own command, in their own terminal (L3, LA5).
  let passwordRoute: (() => void) | null = null;
  // While it waits, the code it drew, drawn again only when it changes (LA4), and the password
  // sign-in for a player without the Steam Mobile App (LA5).
  const codeTile = h("div", { class: "scan-code" });
  let drawn = "";
  const scan = h(
    "div",
    { class: "scan" },
    codeTile,
    h("p", { class: "scan-words" }, "Scan this with the Steam Mobile App."),
    line("Fenji never sees your sign-in."),
  );
  const password = line("No Steam Mobile App? ", button.link("Sign in with your password", () => passwordRoute?.()));
  const working = section(h("p", { class: "work-head" }, workTitle, workPercent), bar, scan, workLine, acts(stop), password);

  function build(view: Exclude<View, "working">, s: Snapshot): Child[] {
    const { client, platform } = s;
    const home = platform.home;
    switch (view) {
      case "setup": {
        const found = client.steam;
        const steamFirst = found !== null && found.pinned;
        const manual = client.manual;
        return [
          found === null
            ? null
            : section(
                big("Steam library"),
                h("p", { class: "path-line" }, pathEnd(found.folder, 46, home)),
                found.pinned
                  ? acts(button.primary("Use it", act({ type: "chooseFolder", path: found.folder }), true))
                  : line("It's another build. Update it in Steam first."),
              ),
          section(
            big("Another folder"),
            acts(steamFirst ? button.secondary("Choose folder", pick, true) : button.primary("Choose folder", pick, true)),
          ),
          section(
            big("Download"),
            line(platform.canDownload ? "With your Steam account" : "Works on Linux only for now."),
            platform.canDownload
              ? acts(
                  button.secondary("Download", act({ type: "download", folder: client.defaultFolder }), true),
                  manual === null ? null : runIt(manual.download, s, "chooseFolder"),
                )
              : null,
          ),
        ];
      }
      case "repair": {
        const check = client.check.kind === "needsRepair" ? client.check : null;
        const files = check?.files ?? [];
        const n = files.length;
        const canRepair = platform.canDownload && !client.inSteam;
        const command = client.manual?.repair ?? null;
        return [
          section(
            big(n === 1 ? "1 file needs repair" : `${n} files need repair`),
            h(
              "ul",
              { class: "files" },
              files.slice(0, FILES).map((file) => h("li", { class: "file" }, h("span", { class: "mono" }, file.path), h("b", {}, PROBLEM[file.problem]))),
            ),
            n > FILES ? line(`and ${n - FILES} more`) : null,
            client.inSteam
              ? line("Steam looks after this copy: in Steam, choose Verify integrity of game files. Then verify here.")
              : canRepair
                ? null
                : line("Copy these files over again, then verify."),
            acts(
              canRepair
                ? button.primary("Repair", act({ type: "repair" }), true)
                : button.primary("Verify", act({ type: "check", level: "quick" }), true),
              button.link("Change folder", pick),
              command === null || client.inSteam ? null : runIt(command, s, "check"),
            ),
            // Spec §3.5: a copy from Gameforge's launcher is put back by hand, then verified.
            canRepair
              ? line("Put them back another way? ", button.link("Verify", act({ type: "check", level: "quick" })))
              : null,
          ),
        ];
      }
      case "missing":
        return [
          section(
            big("Game folder missing"),
            h("p", { class: "path-line" }, pathEnd(client.folder ?? "", 46, home)),
            line("Fenji can't find the game there."),
            acts(button.primary("Find the game", pick, true)),
          ),
        ];
      case "ready":
      case "unchecked": {
        const check = client.check;
        return [
          section(
            big(GAME),
            h("p", { class: "path-line" }, pathEnd(client.folder ?? "", 46, home)),
            check.kind === "verified"
              ? [line(dot("ok"), "Verified"), h("p", { class: "line faint" }, when(check.atMs, Date.now()))]
              : line(dot("quiet"), "Not verified"),
            acts(
              check.kind === "verified"
                ? button.secondary("Verify integrity", act({ type: "check", level: "full" }), true)
                : button.primary("Verify", act({ type: "check", level: "quick" }), true),
              button.link("Change folder", pick),
            ),
          ),
        ];
      }
    }
  }

  return {
    title: "Game",
    body: h("div", {}, content, problem.el),
    update(s) {
      const view = viewFor(s);
      if (view === "working" && s.job !== null) {
        const now = work(s.job);
        stopIntent = now.stop[1];
        if (workTitle.textContent !== now.title) {
          workTitle.textContent = now.title;
        }
        workPercent.textContent = now.percent === null ? "" : percent(now.percent);
        bar.setAttribute("aria-label", now.title);
        if (now.percent === null) {
          bar.removeAttribute("aria-valuenow");
          fill.style.removeProperty("--p");
        } else {
          bar.setAttribute("aria-valuenow", String(Math.round(now.percent)));
          fill.style.setProperty("--p", `${now.percent}%`);
        }
        bar.classList.toggle("drift", now.percent === null);
        workLine.hidden = now.line === null;
        workLine.textContent = now.line ?? "";
        if (stop.textContent !== now.stop[0]) {
          stop.textContent = now.stop[0];
        }
        const manual = s.client.manual;
        const job = s.job;
        // The code is the next step, not progress: it takes the bar's place.
        const code = job.kind === "downloading" ? job.code : null;
        scan.hidden = code === null;
        bar.hidden = code !== null;
        if ((code?.dark ?? "") !== drawn) {
          codeTile.replaceChildren(...(code === null ? [] : [drawCode(code)]));
          drawn = code?.dark ?? "";
        }
        const waiting = job.kind === "downloading" && job.waitingForSteam && manual !== null;
        password.hidden = !waiting;
        passwordRoute = null;
        if (waiting) {
          // A repair's own command fetches only the files that need it, then the files are verified.
          const repair = job.kind === "downloading" && job.repair && manual.repair !== null;
          const command = repair ? manual.repair! : manual.download;
          // A password sign-in is the player's own command, with their own login (L3, LA5; the
          // final review, C2). Two DepotDownloaders would write one folder: Fenji's own, waiting,
          // goes first.
          passwordRoute = () => {
            void ctx.send({ type: "cancelDownload" });
            ctx.show((done) => commandDialog(ctx, command, manual.get, repair ? "check" : "chooseFolder", done));
          };
        }
        renew(content, "working", () => [working]);
        return;
      }
      // The Ready view's words change with the day, not the minute: a probe never redraws it under focus.
      const check = s.client.check;
      const verifiedWhen = check.kind === "verified" ? when(check.atMs, Date.now()) : "";
      const key = JSON.stringify([view, s.client, s.platform.canDownload, s.platform.home, verifiedWhen]);
      renew(content, key, () => build(view as Exclude<View, "working">, s));
    },
  };
}
