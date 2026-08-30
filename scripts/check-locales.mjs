#!/usr/bin/env node
/**
 * Vérifie que chaque traduction couvre exactement les clés de l'anglais.
 *
 * Une clé manquante ne casse rien à l'exécution — l'application retombe sur
 * l'anglais — mais elle passerait inaperçue. Une clé en trop signale presque
 * toujours une faute de frappe.
 */
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const directory = new URL("../src/i18n/locales/", import.meta.url).pathname;
const reference = JSON.parse(readFileSync(join(directory, "en.json"), "utf8"));
const referenceKeys = Object.keys(reference);

/** Repère les paramètres « {nom} » attendus par une chaîne. */
const placeholders = (text) =>
  [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort().join(",");

let failed = false;

for (const file of readdirSync(directory).sort()) {
  if (!file.endsWith(".json") || file === "en.json") continue;

  const locale = JSON.parse(readFileSync(join(directory, file), "utf8"));
  const keys = Object.keys(locale);
  const missing = referenceKeys.filter((key) => !keys.includes(key));
  const extra = keys.filter((key) => !referenceKeys.includes(key));

  // Un paramètre oublié dans une traduction produit un texte tronqué à l'écran.
  const mismatched = referenceKeys
    .filter((key) => locale[key] !== undefined)
    .filter((key) => placeholders(reference[key]) !== placeholders(locale[key]));

  if (missing.length || extra.length || mismatched.length) {
    failed = true;
    console.error(`✗ ${file}`);
    if (missing.length) console.error(`    manquantes : ${missing.join(", ")}`);
    if (extra.length) console.error(`    en trop : ${extra.join(", ")}`);
    if (mismatched.length)
      console.error(`    paramètres divergents : ${mismatched.join(", ")}`);
  } else {
    console.log(`✓ ${file} — ${keys.length} clés`);
  }
}

process.exit(failed ? 1 : 0);
