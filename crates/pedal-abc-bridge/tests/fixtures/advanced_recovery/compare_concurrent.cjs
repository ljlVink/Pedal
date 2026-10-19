// 并发函数的普通 JS 返回、异常、接收者和副作用对照；通知本身另由 SDK 往返验证。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ts = require(process.argv[3] || path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));
if (!process.argv[2]) throw new Error('usage: node compare_concurrent.cjs <recovered.ets> [typescript.js]');
function load(source) {
  const compiled = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS },
    reportDiagnostics: true,
  });
  assert.equal(compiled.diagnostics.filter(d => d.category === ts.DiagnosticCategory.Error).length, 0);
  const context = { exports: {} };
  vm.runInNewContext(compiled.outputText, context, { timeout: 2000 });
  return context.exports;
}
const cases = [
  ['fulfilled', 'concurrentFulfilled', () => [Promise.resolve(9)], { value: 9, trace: [] }],
  ['rejected await', 'concurrentFulfilled', () => [Promise.reject(new Error('rejected'))], { error: 'rejected', trace: [] }],
  ['thenable', 'concurrentFulfilled', trace => [{ get then() { trace.push('get then');
    return resolve => { trace.push('then'); resolve(7); }; } }], { value: 7, trace: ['get then', 'then'] }],
  ['throwing then getter', 'concurrentFulfilled', trace => [{ get then() { trace.push('get then');
    throw new Error('then getter'); } }], { error: 'then getter', trace: ['get then'] }],
  ['explicit rejection', 'concurrentRejected', () => [new Error('explicit')], { error: 'explicit', trace: [] }],
  ['primitive rejection', 'concurrentRejected', () => ['primitive'], { error: 'primitive', trace: [] }],
  ['identity', 'concurrentValue', (_, token) => [token], { value: { id: 'token' }, identity: true, trace: [] }],
  ['implicit return', 'concurrentImplicit', trace => [() => trace.push('effect')], { value: undefined, trace: ['effect'] }],
  ['implicit failure', 'concurrentImplicit', trace => [() => { trace.push('effect'); throw new Error('effect'); }],
    { error: 'effect', trace: ['effect'] }],
  ['choose left', 'concurrentChoose', trace => [true, () => { trace.push('left'); return 1; },
    () => { trace.push('right'); return 2; }], { value: 1, trace: ['left'] }],
  ['choose right', 'concurrentChoose', trace => [false, () => { trace.push('left'); return 1; },
    () => { trace.push('right'); return 2; }], { value: 2, trace: ['right'] }],
  ['choose failure', 'concurrentChoose', trace => [true, () => { trace.push('left'); throw new Error('left'); },
    () => { trace.push('right'); return 2; }], { error: 'left', trace: ['left'] }],
  ['branch left receiver', 'concurrentBranch', trace => [true, { tag: 'owner',
    left() { trace.push('left'); return this.tag; }, right() { trace.push('right'); return this.tag; } }],
    { value: 'owner', trace: ['left'] }],
  ['branch right receiver', 'concurrentBranch', trace => [false, { tag: 'owner',
    left() { trace.push('left'); return this.tag; }, right() { trace.push('right'); return this.tag; } }],
    { value: 'owner', trace: ['right'] }],
  ['branch failure', 'concurrentBranch', trace => [false, {
    left() { trace.push('left'); return 1; }, right() { trace.push('right'); throw new Error('right'); } }],
    { error: 'right', trace: ['right'] }],
];
async function run(functions, name, argumentsFor) {
  const trace = [], token = { id: 'token' };
  try {
    const value = await functions[name](...argumentsFor(trace, token));
    return { value, ...(name === 'concurrentValue' ? { identity: value === token } : {}), trace };
  } catch (error) {
    return { error: error && typeof error === 'object' && 'message' in error ? error.message : String(error), trace };
  }
}
(async () => {
  const original = load(fs.readFileSync(path.join(__dirname, 'ConcurrentRecovery.ts'), 'utf8'));
  const recovered = load(fs.readFileSync(process.argv[2], 'utf8'));
  for (const [label, name, argumentsFor, expected] of cases) {
    assert.deepEqual(await run(original, name, argumentsFor), expected, `${label}: source oracle`);
    assert.deepEqual(await run(recovered, name, argumentsFor), expected, `${label}: recovered behavior`);
  }
  console.log(`${cases.length} concurrent function behavior cases passed`);
})().catch(error => { console.error(error); process.exitCode = 1; });
