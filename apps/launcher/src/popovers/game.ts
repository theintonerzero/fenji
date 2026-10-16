// The Game popover (the redesign's Your game boards): the player's copy, as it is. Verified, with
// where it is, when, Verify integrity and Change folder. Set up: the Steam library first, then
// another folder, then a download. Downloading or verifying, with its progress. Needs repair, with
// each file and Repair. Its actions' refusals show in it, under its sections.
import type { Ctx, PopoverContent } from "../app";
import { h, type Child } from "../dom";
import type { FileProblem, Intent, JobView, Snapshot } from "../types";
import { button } from "../ui/button";
import { commandDialog } from "../ui/command-dialog";
import { formError, pathEnd, refused, renew } from "../screens/parts";
import { percent, when } from "../words";

export const GAME = "Swords of Legends Online";

type View = "working" | "setup" | "repair" | "missing" | "ready" | "unchecked";

function viewFor(s: Snapshot): View {
  // umu-launcher's fetch on a first start is PLAY's, not the copy's.
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
        line: job.waitingForSteam ? "Sign in to Steam in its own window. Fenji never sees it." : null,
      };
  }
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
  // While DepotDownloader waits for a Steam sign-in, the player can run it in a terminal of their own.
  let runCommand: (() => void) | null = null;
  const runLink = button.link("Run it yourself", () => runCommand?.());
  const working = section(h("p", { class: "work-head" }, workTitle, workPercent), bar, workLine, acts(stop, runLink));

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
        const waiting = job.kind === "downloading" && job.waitingForSteam && manual !== null;
        runLink.hidden = !waiting;
        runCommand = null;
        if (waiting) {
          // A repair's own command fetches only the files that need it, then the files are verified.
          const repair = job.kind === "downloading" && job.repair && manual.repair !== null;
          const command = repair ? manual.repair! : manual.download;
          runCommand = () =>
            ctx.show((done) => commandDialog(ctx, command, manual.get, repair ? "check" : "chooseFolder", done));
        }
        renew(content, "working", () => [working]);
        return;
      }
      const key = JSON.stringify([view, s.client, s.platform.canDownload, s.platform.home, view === "ready" ? Math.floor(Date.now() / 60000) : 0]);
      renew(content, key, () => build(view as Exclude<View, "working">, s));
    },
  };
}
