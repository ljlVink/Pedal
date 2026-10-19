// 从 hvigor 降低后的源码提取最小组件 fixture，避免依赖完整本地项目。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const ts = require(process.argv[4] || path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));
if (!process.argv[2] || !process.argv[3]) {
  throw new Error('usage: node extract_arkui.cjs <hvigor Index.ts> <ArkUIProbe.ts> [typescript.js]');
}
const ast = ts.createSourceFile('Index.ts', fs.readFileSync(process.argv[2], 'utf8'), ts.ScriptTarget.ES2020, true);
const names = ['Circle', 'ProbePage', 'Shape', 'ProbePage_Params', 'MathUtil'];
const statements = ast.statements.filter(statement =>
  (statement.name && names.includes(statement.name.text)) || ts.isIfStatement(statement) ||
  (ts.isExpressionStatement(statement) && statement.getText(ast).startsWith('registerNamedRoute(')));
fs.writeFileSync(process.argv[3], '// hvigor 降低后的最小 ArkUI 组件回归样例。\n// @author sky\n\n' +
  statements.map(statement => statement.getText(ast)).join('\n\n') + '\n');
