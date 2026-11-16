# Backlog

P2 findings left open for 1.0.0, each with the reason (PLAN: "P2 items are fixed or moved to BACKLOG.md with a
reason, time-boxed to one day per app"). Ids are from `docs/v1/SPEC.md` and `docs/v1/audit/lens.md`.

| Id | What | Why it is not in 1.0.0 |
|---|---|---|
| LN-10 | In colour-pick mode the loupe's label can show a dark colour (`#161B1D`) instead of the hovered pixel, seen once through Link's `lens.capture_and_act` on Xvfb. The panel and the picked result are correct. | The loupe already samples the frozen capture (`gui/overlay.rs` `paint_loupe`), so the wrong value comes from the hover coordinates, not the image. It was not reproduced since, and Tier 1 cannot read what egui draws to check it. Only the hover label is affected; the colour the user picks is right. |
| RD-LENS-4 | A pin's context menu is clipped by the pin's small window. | egui draws menus inside the window; showing the whole menu needs a separate popup surface or a minimum pin size, a design change to pins. The menu stays usable (it scrolls with the pointer and Escape closes it, RD-LENS-3). |
