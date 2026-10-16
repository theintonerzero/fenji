// Almost ready (launcher spec §3.5; the Repair board): the last check's counts and each file it
// found, with its problem. On Linux, "Repair from Steam" fetches only those files again; the
// Gameforge line tells that copy's owner to copy them over and check again. The blade stands at
// the share of files that are fine.
import type { Ctx, Screen } from "../app";
import { h } from "../dom";
import { groundFor } from "../route";
import type { FileProblem, Snapshot } from "../types";
import { blade } from "../ui/blade";
import { button } from "../ui/button";
import type { Dialog } from "../ui/dialog";
import { icons } from "../ui/icons";
import { clientColumn, commandDialog } from "./client-parts";
import { noticeSlot, renew } from "./parts";

function problem(file: FileProblem, build: string): HTMLElement {
  const warning = icons.warning(16);
  switch (file.problem) {
    case "missing":
      return h("div", {}, h("div", { class: "problem" }, warning, "Missing"));
    case "wrongSize":
      return h(
        "div",
        {},
        h("div", { class: "problem" }, warning, "Wrong size"),
        h("div", { class: "problem-detail" }, `Doesn't match build ${build}`),
      );
    case "wrongHash":
      return h("div", {}, h("div", { class: "problem" }, warning, `Doesn't match build ${build}`));
  }
}

export function clientRepair(root: HTMLElement, ctx: Ctx): Screen {
  const art = blade({ kind: "unlit" });
  const lede = h("p", { class: "lede" });
  const slot = noticeSlot(ctx);
  const results = h("section", { class: "card results", "aria-label": "Check results" });
  const actions = h("div", { class: "actions" });
  const notes = h("div", {});
  const side = clientColumn();
  let open: Dialog | null = null;
  root.append(
    h(
      "main",
      { class: "main" },
      art,
      h(
        "div",
        { class: "hero hero-wide" },
        h("h1", { class: "headline" }, "Almost ready"),
        lede,
        slot.el,
        results,
        actions,
        notes,
      ),
    ),
    side.el,
  );
  return {
    update(s: Snapshot) {
      const { client } = s;
      const check = client.check;
      const files = check.kind === "needsRepair" ? check.files : [];
      const fine = check.kind === "needsRepair" ? check.fine : 0;
      const total = client.files;
      art.update(
        { kind: "climbing", percent: total === 0 ? 0 : (fine / total) * 100, value: `${fine} of ${total}`, label: "files fine" },
        groundFor(s),
      );
      lede.textContent = `Fenji checked your ${total} game files against build ${client.build}.`;
      const one = files.length === 1;
      renew(results, JSON.stringify([fine, files, client.build]), () => [
        h(
          "div",
          { class: "results-counts" },
          h("p", {}, h("span", { class: "dot dot-ok", "aria-hidden": "true" }), h("span", {}, h("strong", {}, String(fine)), " files fine")),
          h(
            "p",
            {},
            h("span", { class: "dot dot-warn", "aria-hidden": "true" }),
            h("span", {}, h("strong", {}, String(files.length)), one ? " needs repair" : " need repair"),
          ),
        ),
        h(
          "div",
          { class: "results-files" },
          h(
            "table",
            { "aria-label": "Files that need repair" },
            h("thead", {}, h("tr", {}, h("th", { scope: "col" }, "File"), h("th", { scope: "col" }, "Problem"))),
            h(
              "tbody",
              {},
              files.map((file) => h("tr", {}, h("td", { class: "mono" }, file.path), h("td", {}, problem(file, client.build)))),
            ),
          ),
        ),
      ]);
      // Off Linux there's no download to repair from (spec §3.6, L2). A copy in a Steam library is
      // Steam's to repair, and then its files are checked (Amendment A, LA2): a quick check, which
      // hashes the files that needed repair in full.
      const inSteam = s.client.inSteam;
      renew(actions, `${s.platform.canDownload}\n${inSteam}`, () => [
        inSteam
          ? button.primary("Check files", () => void ctx.send({ type: "check", level: "quick" }))
          : s.platform.canDownload
            ? button.primary("Repair from Steam", () => void ctx.send({ type: "repair" }))
            : null,
        button.secondary("Choose another folder", () => void ctx.send({ type: "pickFolder", purpose: "game" })),
      ]);
      const these = one ? "this file" : "these files";
      const commands = s.client.manual;
      const command = commands?.repair ?? null;
      renew(notes, `${s.platform.canDownload}\n${one}\n${inSteam}\n${JSON.stringify(commands)}`, () => [
        inSteam
          ? h(
              "p",
              { class: "repair-note" },
              `This copy is in your Steam library, which Steam looks after. In Steam, open the game's Properties, then Installed Files, and choose Verify integrity of game files. Then check the files here.`,
            )
          : null,
        s.platform.canDownload && !inSteam
          ? h(
              "p",
              { class: "repair-note" },
              `Repair downloads just ${these} again with DepotDownloader. Any Steam sign-in happens in its own window, never in Fenji.`,
            )
          : null,
        // Without a terminal Fenji can open, the player runs it in their own (#348).
        command === null
          ? null
          : h(
              "p",
              { class: "manual" },
              button.link("No Steam app? Run DepotDownloader yourself", () => {
                open?.close();
                open = commandDialog(ctx, command, commands?.get ?? null, "check", () => {
                  open = null;
                });
                document.body.append(open.el);
              }),
            ),
        inSteam
          ? null
          : h(
          "p",
          { class: "gameforge" },
          icons.info(16),
          h(
            "span",
            {},
            `Got the game from Gameforge's launcher? Copy ${these} over again from that install, then `,
            h(
              "button",
              // A quick check, which hashes the files that needed repair in full (#348).
              { class: "inline-link", type: "button", onclick: () => void ctx.send({ type: "check", level: "quick" }) },
              "check again",
            ),
            ".",
          ),
        ),
      ]);
      slot.update(s);
      side.update(s);
    },
    destroy() {
      open?.close();
      root.replaceChildren();
    },
  };
}
