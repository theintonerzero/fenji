// Downloading (launcher spec §3.6; the Downloading board): DepotDownloader fetched on first use,
// then the player's Steam sign-in in its own window, then the files, the blade climbing with
// them. Cancel download ends it; the files fetched so far stay.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { bladeFor, groundFor } from "../route";
import type { JobView, Snapshot } from "../types";
import { blade, type BladeState } from "../ui/blade";
import { button } from "../ui/button";
import { icons } from "../ui/icons";
import { percent } from "../words";
import { clientColumn, GAME, path } from "./client-parts";
import { noticeSlot, renew } from "./parts";

/** The blade: the files' percentage, its readout's line short. It shows the player's copy (DESIGN.md,
 * The Blade And Ground Rule), so it stays as the copy is while the tool itself downloads: the lede
 * says how far that has come (the finish review's fix 3). */
function bladeState(job: JobView | null, s: Snapshot): BladeState {
  if (job?.kind === "downloading" && job.percent !== null) {
    return { kind: "climbing", percent: job.percent, label: "downloaded" };
  }
  return bladeFor(s);
}

export function clientWorking(root: HTMLElement, ctx: Ctx): Screen {
  const art = blade({ kind: "unlit" });
  const title = h("h1", { class: "headline" });
  const lede = h("p", { class: "lede" });
  const slot = noticeSlot(ctx);
  const card = h("div", {});
  const where = h("div", {});
  const side = clientColumn();
  root.append(
    h(
      "main",
      { class: "main" },
      art,
      h(
        "div",
        { class: "hero hero-wide" },
        title,
        lede,
        slot.el,
        card,
        h("div", { class: "actions" }, button.secondary("Cancel download", () => void ctx.send({ type: "cancelDownload" }))),
        where,
      ),
    ),
    side.el,
  );
  return {
    update(s) {
      const job = s.job;
      art.update(bladeState(job, s), groundFor(s));
      const repair = job?.kind === "downloading" && job.repair;
      const words = repair ? "Repairing" : "Downloading";
      if (title.textContent !== words) {
        title.textContent = words;
      }
      lede.textContent =
        job?.kind === "fetching"
          ? `Getting ${job.tool}…${job.percent === null ? "" : ` ${percent(job.percent)}`}`
          : `${GAME}, build ${s.client.build}, from your Steam library`;
      const waiting = job?.kind === "downloading" && job.waitingForSteam;
      renew(card, `${job?.kind}\n${waiting}`, () =>
        job?.kind === "downloading"
          ? [
              h(
                "section",
                { class: "card steam-card" },
                h("span", { class: "steam-icon", "aria-hidden": "true" }, icons.lock(20)),
                h(
                  "div",
                  {},
                  h("h2", { class: "steam-title" }, "Your Steam sign-in stays in DepotDownloader"),
                  h(
                    "p",
                    { class: "steam-text" },
                    waiting
                      ? "Sign in to Steam in DepotDownloader's window: scan its QR code with the Steam app."
                      : "DepotDownloader opened its own window for you to sign in to Steam. Fenji never asks for or sees that password.",
                  ),
                ),
              ),
            ]
          : [],
      );
      const folder = s.client.folder ?? s.client.defaultFolder;
      const file = job?.kind === "downloading" ? job.file : null;
      renew(where, `${folder}\n${file}\n${job?.kind}`, () => [
        job?.kind === "downloading"
          ? h(
              "p",
              { class: "meta" },
              icons.folder(16),
              h("span", {}, "Saving to ", path(folder), ". Fenji checks every file when the download finishes."),
            )
          : null,
        file === null ? null : h("p", { class: "meta" }, icons.download(16), path(file)),
      ]);
      slot.update(s);
      side.update(s);
    },
    destroy() {
      root.replaceChildren();
    },
  };
}
