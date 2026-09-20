// tsc (チェッカー込み) の診断を出す。oxc のパーサー結果との比較用。
// 使い方: TS_PATH=<typescript の built/local/typescript.js か node_modules/typescript> node tsc_check.js <file.ts>
const ts = require(process.env.TS_PATH || 'typescript');
const f = process.argv[2];
const p = ts.createProgram([f], { noEmit: true });
const name = f.split('/').pop();
const ds = ts.getPreEmitDiagnostics(p).filter((d) => d.file && d.file.fileName.endsWith(name));
console.log(
  ts.version,
  ds.length
    ? ds.map((d) => 'TS' + d.code + ': ' + ts.flattenDiagnosticMessageText(d.messageText, ' ')).join(' | ')
    : 'no errors',
);
