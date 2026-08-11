// SPDX-License-Identifier: GPL-3.0-or-later
//
// Parses every browser-side JS file. Nothing here type-checks or runs the
// code — it only asserts the files are syntactically valid.
//
//   node --test tests/unit/
//
// Cheap, but not pointless: most of this tree is loaded as ES modules straight
// from docs/ with no build step, so a syntax error ships and only surfaces as
// a blank page in someone's browser. There is no bundler to catch it first.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const REPO = fileURLToPath(new URL('../..', import.meta.url));

const SOURCE_DIRS = ['ft8-web/www', 'uvpacket-web/www'];

for (const dir of SOURCE_DIRS) {
  const files = readdirSync(new URL(`../../${dir}/`, import.meta.url))
    .filter((f) => f.endsWith('.js'))
    .sort();

  test(`${dir} has JS to check`, () => {
    assert.ok(files.length > 0, `no .js files found under ${dir}`);
  });

  for (const file of files) {
    test(`${dir}/${file} parses`, () => {
      // `node --check` exits non-zero and prints the location on a parse error.
      execFileSync(process.execPath, ['--check', `${dir}/${file}`], {
        cwd: REPO,
        stdio: 'pipe',
      });
    });
  }
}
