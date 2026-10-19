// 原始 TypeScript 与反编译输出的对象方法行为对照。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ts = require(path.join(
  process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js',
));

const recoveredFiles = process.argv.slice(2);
if (!recoveredFiles.length) throw new Error('usage: node compare.cjs <recovered.ets> ...');
const original = path.join(__dirname, 'ObjectMethods.ts');
const names = [
  ['alpha', 'beta', 'gamma'],
  ['delta', 'epsilon', 'zeta'],
  ['eta', 'theta', 'iota'],
];

function load(file) {
  const source = fs.readFileSync(file, 'utf8');
  const output = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
  }).outputText;
  const module = { exports: {} };
  vm.runInNewContext(output, { exports: module.exports, module }, { filename: file, timeout: 1000 });
  return module.exports;
}

function behavior(factory, seed) {
  return factory.makeObjectMethodGroups(seed).map((group, index) => ({
    names: Object.keys(group).sort(),
    values: names[index].map(name => group[name]()),
  }));
}

function deepCaptureBehavior(factory, seed) {
  let value = factory.makeDeepCapture(seed);
  for (let i = 0; i < 5; i++) value = value();
  return value;
}

const expected = load(original);
for (const recoveredFile of recoveredFiles) {
  const recovered = load(path.resolve(recoveredFile));
  for (const seed of [-7, 0, 19]) {
    assert.equal(JSON.stringify(behavior(recovered, seed)), JSON.stringify(behavior(expected, seed)),
      `${path.basename(recoveredFile)} differs for seed ${seed}`);
    assert.equal(deepCaptureBehavior(recovered, seed), deepCaptureBehavior(expected, seed),
      `${path.basename(recoveredFile)} deep capture differs for seed ${seed}`);
  }
  process.stdout.write(`${path.basename(recoveredFile)}: 9 object methods and deep captures match\n`);
}
