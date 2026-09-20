// tsc に「型エイリアスが最終的にどんな型になるか」を文字列で出させる (A 以外のエイリアスを表示)。
// 使い方: TS_PATH=<typescript のパス> node tsc_type_of.js <file.ts>
const ts = require(process.env.TS_PATH || 'typescript');
const f = process.argv[2];
const p = ts.createProgram([f], { noEmit: true });
const c = p.getTypeChecker();
const sf = p.getSourceFile(f);
console.log('version', ts.version);
for (const st of sf.statements) {
  if (ts.isTypeAliasDeclaration(st) && st.name.text !== 'A') {
    console.log(st.name.text + ' =', c.typeToString(c.getTypeAtLocation(st.name), undefined, ts.TypeFormatFlags.NoTruncation));
  }
}
console.log('diagnostics:', ts.getPreEmitDiagnostics(p).filter((d) => d.file).map((d) => 'TS' + d.code).join(',') || 'none');
