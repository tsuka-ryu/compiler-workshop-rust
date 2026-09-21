// as / satisfies の「消せない」問題を、3つの見方で比べる。
//   ① tsc: 実際にどう読み、どんな JS を出し、値はいくつか
//   ② 素朴な除去: `as T` / `satisfies T` の文字を空白に置き換えるだけ (Node の type stripping のような実装) → 値はいくつか
//   ③ oxc: パースが通るか、エラーか
// 使い方: TS_PATH=<typescript のパス> OXC_PARSER=<oxc の parser example のバイナリ> node compare.js
const fs = require('fs'), path = require('path'), cp = require('child_process');
const ts = require(process.env.TS_PATH || 'typescript');
const OXC = process.env.OXC_PARSER;
const dir = __dirname;
const ev = (code) => { try { return String(Function('"use strict"; return (' + code.trim().replace(/;\s*$/, '') + ')')()); } catch (e) { return 'ERR ' + e.message; } };
console.log('typescript', ts.version, '\n');
const rows = [];
for (const f of fs.readdirSync(dir).filter((n) => /^demo.*\.ts$/.test(n)).sort((a, b) => parseInt(a.slice(4)) - parseInt(b.slice(4)))) {
  const src = fs.readFileSync(path.join(dir, f), 'utf8').trim();
  // ① tsc
  const emitted = ts.transpileModule(src, { compilerOptions: { target: ts.ScriptTarget.ES2020 } }).outputText.replace(/^"use strict";\s*/, '').trim();
  // ② 素朴な除去: as/satisfies の右側 (式の終わりから node の終わりまで) を空白にする
  const sf = ts.createSourceFile('x.ts', src, ts.ScriptTarget.Latest, true);
  const ranges = [];
  (function visit(n) { if (ts.isAsExpression(n) || ts.isSatisfiesExpression(n)) ranges.push([n.expression.end, n.end]); ts.forEachChild(n, visit); })(sf);
  let blank = src; for (const [a, b] of ranges.sort((x, y) => y[0] - x[0])) blank = blank.slice(0, a) + ' '.repeat(b - a) + blank.slice(b);
  // erasableSyntaxOnly での tsc の診断
  const prog = ts.createProgram([path.join(dir, f)], { erasableSyntaxOnly: true, noEmit: true });
  const diags = ts.getPreEmitDiagnostics(prog).filter((d) => d.file && d.file.fileName.endsWith(f)).map((d) => 'TS' + d.code);
  // ③ oxc
  let oxc = '(OXC_PARSER 未指定)';
  if (OXC) {
    const r = cp.spawnSync(OXC, [path.join(dir, f)], { encoding: 'utf8' });
    const out = r.stdout + r.stderr;
    if (/Parsed Successfully/.test(out)) oxc = 'OK (エラーなし)';
    else {
      // 診断のキャレット (^) の位置にある文字を出す (式が途中で止まり、その位置から先が宙に浮く)
      const lines = out.split('\n');
      const si = lines.findIndex((l) => /^\s*\d+ \| /.test(l));
      const srcLine = lines[si], caret = lines[si + 1] || '';
      const col = caret.indexOf('^') - srcLine.indexOf('| ') - 1;
      oxc = 'エラー (位置 ' + col + ' の `' + src[col] + '` で止まる)';
    }
  }
  const vT = ev(emitted), vB = ev(blank);
  rows.push({ f, src, emitted, vT, blank, vB, same: vT === vB, diags: diags.join(',') || 'なし', oxc });
}
for (const r of rows) {
  console.log(`## ${r.f}: ${r.src}`);
  console.log(`  ① tsc の出力         : ${r.emitted}   → 値 ${r.vT}`);
  console.log(`  ② 空白で消しただけ   : ${r.blank}   → 値 ${r.vB}   ${r.same ? '(一致)' : '★食い違う'}`);
  console.log(`  tsc erasableSyntaxOnly の診断: ${r.diags}`);
  console.log(`  ③ oxc                : ${r.oxc}\n`);
}
