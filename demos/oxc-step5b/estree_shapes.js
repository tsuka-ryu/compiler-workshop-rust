// typescript-estree (ESLint 向けの AST 変換) での同じ入力の形。oxc の --estree が合わせている相手との比較用。
// 使い方: TE_PATH=<@typescript-eslint/typescript-estree のパス> node estree_shapes.js
const te = require(process.env.TE_PATH || '@typescript-eslint/typescript-estree');
const fs = require('fs'), path = require('path');
const shape = (n) => {
  if (!n || typeof n !== 'object') return '?';
  switch (n.type) {
    case 'ChainExpression': return 'Chain(' + shape(n.expression) + ')';
    case 'TSNonNullExpression': return 'NonNull(' + shape(n.expression) + ')';
    case 'MemberExpression': return 'Member' + (n.optional ? '?' : '') + '(' + shape(n.object) + ', ' + (n.computed ? '[' + (n.property.raw || n.property.name) + ']' : n.property.name) + ')';
    case 'CallExpression': return 'Call' + (n.optional ? '?' : '') + '(' + shape(n.callee) + ')';
    case 'Identifier': return n.name;
    default: return n.type;
  }
};
for (const f of fs.readdirSync(__dirname).filter((n) => /^demo.*\.ts$/.test(n)).sort((a, b) => parseInt(a.slice(4)) - parseInt(b.slice(4)))) {
  const src = fs.readFileSync(path.join(__dirname, f), 'utf8');
  const ast = te.parse(src, { filePath: f });
  console.log(f.padEnd(34), ast.body.map((s) => (s.expression ? shape(s.expression) : s.type)).join('  ;  '));
}
