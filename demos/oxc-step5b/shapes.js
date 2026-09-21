// `!` (TSNonNullExpression) とオプショナルチェーンの絡みを、oxc の木と tsc の木で並べて出す。
//   oxc : `--estree` の JSON から。Chain = ChainExpression、Member? / Call? = optional なメンバー/呼び出し
//   tsc : TypeScript の AST。⛓ = そのノードに NodeFlags.OptionalChain が付いている (tsc には ChainExpression は無い)
// 使い方: TS_PATH=<typescript のパス> OXC_PARSER=<oxc の parser example> node shapes.js
const fs = require('fs'), path = require('path'), cp = require('child_process');
const ts = require(process.env.TS_PATH || 'typescript');
const OXC = process.env.OXC_PARSER;
const dir = __dirname;
const oxcShape = (n) => {
  if (!n || typeof n !== 'object') return '?';
  switch (n.type) {
    case 'ChainExpression': return 'Chain(' + oxcShape(n.expression) + ')';
    case 'TSNonNullExpression': return 'NonNull(' + oxcShape(n.expression) + ')';
    case 'ParenthesizedExpression': return 'Paren(' + oxcShape(n.expression) + ')';
    case 'MemberExpression': return 'Member' + (n.optional ? '?' : '') + '(' + oxcShape(n.object) + ', ' + (n.computed ? '[' + (n.property.raw || n.property.name) + ']' : n.property.name) + ')';
    case 'CallExpression': return 'Call' + (n.optional ? '?' : '') + '(' + oxcShape(n.callee) + ')';
    case 'Identifier': return n.name;
    default: return n.type;
  }
};
const tscShape = (n) => {
  const oc = (n.flags & ts.NodeFlags.OptionalChain) ? '⛓' : '';
  if (ts.isIdentifier(n)) return n.text;
  if (ts.isNonNullExpression(n)) return 'NonNull' + oc + '(' + tscShape(n.expression) + ')';
  if (ts.isParenthesizedExpression(n)) return 'Paren(' + tscShape(n.expression) + ')';
  if (ts.isPropertyAccessExpression(n)) return 'Prop' + (n.questionDotToken ? '?' : '') + oc + '(' + tscShape(n.expression) + ', ' + n.name.text + ')';
  if (ts.isElementAccessExpression(n)) return 'Elem' + (n.questionDotToken ? '?' : '') + oc + '(' + tscShape(n.expression) + ', ' + n.argumentExpression.getText() + ')';
  if (ts.isCallExpression(n)) return 'Call' + (n.questionDotToken ? '?' : '') + oc + '(' + tscShape(n.expression) + ')';
  return ts.SyntaxKind[n.kind];
};
console.log('typescript', ts.version, '\n');
for (const f of fs.readdirSync(dir).filter((n) => /^demo.*\.(ts|js)$/.test(n)).sort((a, b) => parseInt(a.slice(4)) - parseInt(b.slice(4)))) {
  const src = fs.readFileSync(path.join(dir, f), 'utf8');
  const isTs = f.endsWith('.ts');
  const sf = ts.createSourceFile(f, src, ts.ScriptTarget.Latest, true, isTs ? ts.ScriptKind.TS : ts.ScriptKind.JS);
  const tsc = sf.statements.map((s) => (ts.isExpressionStatement(s) ? tscShape(s.expression) : ts.SyntaxKind[s.kind])).join('  ;  ');
  let oxc = '(OXC_PARSER 未指定)';
  if (OXC) {
    const r = cp.spawnSync(OXC, [path.join(dir, f), '--estree'], { encoding: 'utf8' });
    const out = r.stdout + r.stderr;
    if (/Parsed with Errors/.test(out)) oxc = 'エラー: ' + (((out.match(/^\s+x (.*)$/m)) || [])[1] || '?');
    else { const j = JSON.parse(out.slice(out.indexOf('{'), out.lastIndexOf('}') + 1)); oxc = j.body.map((s) => (s.expression ? oxcShape(s.expression) : s.type)).join('  ;  '); }
  }
  console.log('## ' + f + ': ' + JSON.stringify(src.trim()));
  console.log('   oxc : ' + oxc);
  console.log('   tsc : ' + tsc + '\n');
}
