// 比较属性定义的运行时行为；普通赋值会触发继承 setter 或只读属性错误。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const [recoveredPath, compilerPath] = process.argv.slice(2);
if (!recoveredPath) throw new Error('usage: node compare_properties.cjs <recovered.ets> [typescript.js]');
const ts = require(compilerPath || path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));

function run(file) {
  const text = fs.readFileSync(file, 'utf8');
  const ast = ts.createSourceFile('properties.ts', text, ts.ScriptTarget.ES2022, true);
  const classes = ast.statements.filter(ts.isClassDeclaration);
  assert.deepEqual(classes.map(node => node.name.text), ['DefinitionBase', 'DefinitionConstants', 'DefinitionDynamic']);
  const source = classes.map((node, index) => node.getText(ast) + (index === 0
    ? '\nObject.defineProperty(DefinitionBase, "label", { value: "inherited", writable: false });'
    : '')).join('\n');
  const result = ts.transpileModule(source, { compilerOptions: {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, useDefineForClassFields: true,
  } });
  const context = vm.createContext({ exports: {} });
  vm.runInContext(result.outputText, context, { timeout: 3000 });
  const json = vm.runInContext(`JSON.stringify([
    ...['answer', 'label', 'enabled'].map(key => Object.getOwnPropertyDescriptor(DefinitionConstants, key)),
    ...['values', 'label'].map(key => Object.getOwnPropertyDescriptor(DefinitionDynamic, key))
  ])`, context, { timeout: 3000 });
  return JSON.parse(json);
}

const expected = [42, 'ready', true, [1, 2, 3], 'after array'].map(value => ({
  value, writable: true, enumerable: true, configurable: true,
}));
assert.deepEqual(run(path.join(__dirname, 'PropertyDefinitions.ts')), expected);
assert.deepEqual(run(recoveredPath), expected);
console.log('PASS own static properties bypass inherited setters/read-only properties; all 5 values/descriptors match source');
