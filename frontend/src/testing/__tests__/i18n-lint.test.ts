// i18n catalog + literal-string lint (G-11).
//
// Purpose:
//  - Confirm every extracted ES message has a matching EN translation.
//  - Best-effort scan: catch JSX that contains a bare text node like
//    `<div>Some visible text</div>` where no <Trans/> wrapper is in scope.
//    Aria-labels / test-ids that are constant strings are ignored —
//    getting precision perfect is out of scope for T14a; regressions will
//    tend to show up as new catalog entries, which we'll review in PR.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = path.resolve(__dirname, '..', '..');
const LOCALES = path.join(SRC, 'locales');

function readPo(file: string): Map<string, string> {
  const txt = readFileSync(file, 'utf8');
  const entries = new Map<string, string>();
  const re = /^msgid "(.*)"\nmsgstr "(.*)"$/gm;
  let m: RegExpExecArray | null;
  while ((m = re.exec(txt)) !== null) {
    const [, id, str] = m;
    if (id === '' || id === undefined) continue;
    entries.set(id, str ?? '');
  }
  return entries;
}

describe('i18n catalogs (G-11)', () => {
  it('every ES msgid has a non-empty EN translation', () => {
    const es = readPo(path.join(LOCALES, 'es-ES', 'messages.po'));
    const en = readPo(path.join(LOCALES, 'en', 'messages.po'));
    const missing: string[] = [];
    for (const id of es.keys()) {
      const enStr = en.get(id);
      if (!enStr) missing.push(id);
    }
    expect(missing, `Missing EN translations:\n${missing.join('\n')}`).toEqual([]);
  });
});

function listTsxRecursive(dir: string, acc: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name.startsWith('.')) continue;
    const full = path.join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (name === '__tests__' || name === 'testing' || name === 'locales') continue;
      listTsxRecursive(full, acc);
    } else if (name.endsWith('.tsx')) {
      acc.push(full);
    }
  }
  return acc;
}

// Heuristic: flag lines that look like `> Some long text <` where the
// surrounding JSX lacks a <Trans> / t`...` marker nearby. Kept strict enough
// to catch "<div>Hello world</div>" regressions but not aria-label="Foo".
const LITERAL_RE = />\s*([A-Z][A-Za-z0-9 ,.;:?!'’"()\-–—]{8,})\s*</;
const SAFE_SUBSTR = [
  'Trans',
  "i18n._",
  "t`",
  "aria-",
  "data-",
  'role=',
  'alt=',
  'title=',
  'className',
  '//',
];

describe('Bare JSX string literals (G-11 best-effort)', () => {
  it('does not introduce unwrapped visible text in src components/routes', () => {
    const files = listTsxRecursive(path.join(SRC, 'components'))
      .concat(listTsxRecursive(path.join(SRC, 'routes')));
    const offenders: Array<{ file: string; line: number; text: string }> = [];
    for (const file of files) {
      const lines = readFileSync(file, 'utf8').split('\n');
      for (let i = 0; i < lines.length; i++) {
        const line = lines[i]!;
        const prev = lines[i - 1] ?? '';
        const next = lines[i + 1] ?? '';
        const context = `${prev}\n${line}\n${next}`;
        if (SAFE_SUBSTR.some((s) => context.includes(s))) continue;
        const m = LITERAL_RE.exec(line);
        if (m) offenders.push({ file: path.relative(SRC, file), line: i + 1, text: m[1]! });
      }
    }
    expect(
      offenders,
      `Potentially-untranslated strings (wrap them in <Trans>):\n${offenders
        .map((o) => `  ${o.file}:${o.line}  "${o.text}"`)
        .join('\n')}`,
    ).toEqual([]);
  });
});
