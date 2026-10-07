#!/usr/bin/env node
// Builds the editor the app bundles (into ../editor), from the website's sources:
// ISOTHRUM_WEBSITE, or the isothrum-website checkout next to this repository.
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";

const here = resolve(import.meta.dirname, "..");
const website = resolve(process.env.ISOTHRUM_WEBSITE ?? join(here, "../../../isothrum-website"));
execFileSync(process.execPath, [join(website, "scripts/build-desktop-editor.mjs"), join(here, "editor")], { stdio: "inherit" });
