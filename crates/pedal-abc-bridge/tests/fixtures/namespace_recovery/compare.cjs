// 完整命名空间模块的源码/反编译行为对照，不重写反编译函数。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const recovered = process.argv.slice(2);
if (!recovered.length) throw new Error('usage: node compare.cjs <recovered-debug.ets> [recovered-release.ets]');
const ts = require(path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));

function load(file) {
  const source = fs.readFileSync(file, 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS,
  }, reportDiagnostics: true });
  const errors = compiled.diagnostics.filter(d => d.category === ts.DiagnosticCategory.Error);
  assert.equal(errors.length, 0, `${file}: TypeScript syntax errors`);
  const context = vm.createContext({ exports: {} });
  vm.runInContext(compiled.outputText, context, { timeout: 3000 });
  return context.exports;
}

async function exercise(file) {
  const api = load(file);
  assert.equal(typeof api.shaderHandler, 'object', 'namespace must be an ordinary initialized object');
  const properties = Object.keys(api.shaderHandler).sort();
  assert.deepEqual(properties, ['makeCounter', 'resolve'], 'private captured constants must stay private');
  const counters = [api.firstCounter(), api.secondCounter(), api.firstCounter(), api.secondCounter(), api.firstCounter()];
  const fresh = api.shaderHandler.makeCounter(1);
  const freshCounter = [fresh(), fresh()];
  const trace = [];
  const resolved = await api.shaderHandler.resolve(async name => { trace.push(name); return 6; });
  const immediate = await api.shaderHandler.resolve(name => { trace.push(name); return 2; });
  let rejected;
  try { await api.shaderHandler.resolve(async name => { trace.push(name); throw new Error('rejected'); }); }
  catch (error) { rejected = error.message; }
  let synchronousThrow;
  try { await api.shaderHandler.resolve(name => { trace.push(name); throw new Error('synchronous'); }); }
  catch (error) { synchronousThrow = error.message; }
  return { properties, counters, freshCounter, resolved, immediate, rejected, synchronousThrow, trace };
}

const oracle = {
  properties: ['makeCounter', 'resolve'], counters: [7, 14, 11, 18, 15], freshCounter: [5, 9],
  resolved: 'shader:10', immediate: 'shader:6', rejected: 'rejected', synchronousThrow: 'synchronous',
  trace: ['shader', 'shader', 'shader', 'shader'],
};
(async () => {
  const timer = setTimeout(() => { console.error('FAIL namespace comparison timed out'); process.exit(1); }, 5000);
  try {
    const original = await exercise(path.join(__dirname, 'NamespaceRecovery.ts'));
    assert.deepEqual(original, oracle, 'source must satisfy explicit oracles');
    for (const file of recovered) {
      assert.deepEqual(await exercise(file), original, `${file}: whole-record behavior differs`);
      console.log(`PASS ${file}: namespace object, private captures, independent counters, async completion and rejection`);
    }
  } finally { clearTimeout(timer); }
})().catch(error => { console.error(error); process.exitCode = 1; });
