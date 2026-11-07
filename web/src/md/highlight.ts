// Syntax highlighting (lazy chunk). refractor turns code into hast spans;
// each grammar is its own chunk, fetched and registered on first use (the
// grammar modules register what they build on, e.g. tsx pulls in jsx).
//
// Grammars that other grammars import (c, javascript, jsx, markup,
// typescript) load through one-line re-exports in ./lang. If a module were
// both a dynamic-import target and a static dependency, the bundler would
// build its namespace with a runtime helper and split that runtime out of
// the initial chunk, so the new-tab bundle would grow.
import { refractor, type Syntax } from "refractor/core";
import type { Grammar } from "./langs";

type Loader = () => Promise<{ default: Syntax }>;

export const LOADERS: Record<Grammar, Loader> = {
  bash: () => import("refractor/bash"),
  c: () => import("./lang/c"),
  cpp: () => import("refractor/cpp"),
  css: () => import("refractor/css"),
  diff: () => import("refractor/diff"),
  docker: () => import("refractor/docker"),
  go: () => import("refractor/go"),
  java: () => import("refractor/java"),
  javascript: () => import("./lang/javascript"),
  json: () => import("refractor/json"),
  jsx: () => import("./lang/jsx"),
  markup: () => import("./lang/markup"),
  python: () => import("refractor/python"),
  rust: () => import("refractor/rust"),
  sql: () => import("refractor/sql"),
  toml: () => import("refractor/toml"),
  tsx: () => import("refractor/tsx"),
  typescript: () => import("./lang/typescript"),
  yaml: () => import("refractor/yaml"),
};

export async function highlight(code: string, grammar: Grammar) {
  if (!refractor.registered(grammar)) refractor.register((await LOADERS[grammar]()).default);
  return refractor.highlight(code, grammar);
}
