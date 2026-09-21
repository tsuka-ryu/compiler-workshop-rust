// 三項演算子とアロー関数の戻り値型の `:` がぶつかる形を、oxc の木と tsc の木で並べて出す。
//   Cond(test, then, else) = 三項演算子、Arrow([引数], ret=戻り値型) = アロー関数
// 使い方: TS_PATH=<typescript のパス> OXC_PARSER=<oxc の parser example> node shapes.js
const fs = require('fs'), path = require('path'), cp = require('child_process');
const ts = require(process.env.TS_PATH || 'typescript');
const OXC = process.env.OXC_PARSER;
const dir = __dirname;
const oxcType = (t) => (t && t.typeAnnotation ? oxcType(t.typeAnnotation) : !t ? null : t.type === 'TSStringKeyword' ? 'string' : t.type === 'TSVoidKeyword' ? 'void' : t.type === 'TSTypeReference' ? t.typeName.name : t.type);
const oxc = (n) => {
  if (!n || typeof n !== 'object') return '?';
  switch (n.type) {
    case 'ConditionalExpression': return 'Cond(' + oxc(n.test) + ', ' + oxc(n.consequent) + ', ' + oxc(n.alternate) + ')';
    case 'ArrowFunctionExpression': return 'Arrow([' + n.params.map((p) => p.type === 'Identifier' ? p.name : p.type === 'ObjectPattern' ? '{..}' : p.type).join(',') + ']' + (n.returnType ? ', ret=' + oxcType(n.returnType) : '') + ') => ' + oxc(n.body);
    case 'Identifier': return n.name;
    case 'Literal': return String(n.raw);
    case 'ObjectExpression': return '{..}';
    case 'CallExpression': return 'Call(' + oxc(n.callee) + ')';
    case 'ParenthesizedExpression': return 'Paren(' + oxc(n.expression) + ')';
    case 'AssignmentExpression': return 'Assign';
    default: return n.type;
  }
};
const tsc = (n) => {
  if (ts.isConditionalExpression(n)) return 'Cond(' + tsc(n.condition) + ', ' + tsc(n.whenTrue) + ', ' + tsc(n.whenFalse) + ')';
  if (ts.isArrowFunction(n)) return 'Arrow([' + n.parameters.map((p) => p.name.getText()).join(',') + ']' + (n.type ? ', ret=' + n.type.getText() : '') + ') => ' + tsc(n.body);
  if (ts.isParenthesizedExpression(n)) return 'Paren(' + tsc(n.expression) + ')';
  if (ts.isIdentifier(n)) return n.text;
  if (ts.isCallExpression(n)) return 'Call(' + tsc(n.expression) + ')';
  if (ts.isObjectLiteralExpression(n)) return '{..}';
  if (ts.isNumericLiteral(n) || n.kind === ts.SyntaxKind.NullKeyword) return n.getText();
  return ts.SyntaxKind[n.kind];
};
console.log('typescript', ts.version, '\n');
for (const f of fs.readdirSync(dir).filter((n) => /^demo.*\.ts$/.test(n)).sort((a, b) => parseInt(a.slice(4)) - parseInt(b.slice(4)))) {
  const src = fs.readFileSync(path.join(dir, f), 'utf8');
  const sf = ts.createSourceFile(f, src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const t = sf.statements.map((s) => (ts.isExpressionStatement(s) ? tsc(s.expression) : ts.SyntaxKind[s.kind])).join(' ; ');
  let o = '(OXC_PARSER 未指定)';
  if (OXC) {
    const r = cp.spawnSync(OXC, [path.join(dir, f), '--estree'], { encoding: 'utf8' });
    const out = r.stdout + r.stderr;
    if (/Parsed with Errors/.test(out)) o = 'エラー';
    else { const j = JSON.parse(out.slice(out.indexOf('{'), out.lastIndexOf('}') + 1)); o = j.body.map((s) => (s.expression ? oxc(s.expression) : s.type)).join(' ; '); }
  }
  console.log('## ' + f + ': ' + JSON.stringify(src.trim()));
  console.log('   oxc : ' + o);
  console.log('   tsc : ' + t + '\n');
}
