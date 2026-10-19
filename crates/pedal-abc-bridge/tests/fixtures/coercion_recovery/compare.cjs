// 比较实际 valueOf/toString 调用、转换异常和受保护迭代器关闭。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ts = require(path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));
const files = process.argv.slice(2);
if (!files.length) throw new Error('usage: node compare.cjs <recovered-debug.ets> [recovered-release.ets]');

function load(file) {
  const result = ts.transpileModule(fs.readFileSync(file, 'utf8'), { compilerOptions: {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS,
  }, reportDiagnostics: true });
  assert.equal(result.diagnostics.filter(d => d.category === ts.DiagnosticCategory.Error).length, 0);
  const context = vm.createContext({ exports: {} });
  vm.runInContext(result.outputText, context, { timeout: 2000 });
  return context.exports;
}
function snapshot(value) {
  if (value === undefined) return { undefined: true };
  if (value && typeof value.name === 'string' && value.name.endsWith('Error')) return { error: value.name };
  return value;
}
const modes = ['valueOf', 'valueOfThrow', 'valueOfUndefined', 'toString', 'toStringThrow', 'toStringUndefined', 'bigint'];
function run(api, method, mode, closeThrows) {
  const trace = [];
  const value = {
    valueOf() {
      assert.equal(this, value);
      trace.push('valueOf');
      if (mode === 'valueOfThrow') throw 'valueOf failure';
      if (mode === 'valueOfUndefined') throw undefined;
      if (mode.startsWith('toString')) return {};
      if (mode === 'bigint') return 2n;
      return 2;
    },
    toString() {
      assert.equal(this, value);
      trace.push('toString');
      if (mode === 'toStringThrow') throw 'toString failure';
      if (mode === 'toStringUndefined') throw undefined;
      return '2';
    },
  };
  const iterator = {
    next() { assert.equal(this, iterator); trace.push('next'); return { done: false, value }; },
    return() {
      assert.equal(this, iterator); trace.push('return');
      if (closeThrows) throw 'close failure';
      return {};
    },
  };
  const iterable = { [Symbol.iterator]() { trace.push('iterator'); return iterator; } };
  try { return { value: snapshot(api[method](method === 'protectedLoop' ? iterable : value)), trace }; }
  catch (error) { return { thrown: snapshot(error), trace }; }
}
function oracle(method, mode, closeThrows) {
  const protectedLoop = method === 'protectedLoop';
  const trace = protectedLoop ? ['iterator', 'next', 'valueOf'] : ['valueOf'];
  if (mode.startsWith('toString')) trace.push('toString');
  if (protectedLoop) trace.push('return');
  let error;
  if (mode === 'valueOfThrow') error = 'valueOf failure';
  else if (mode === 'toStringThrow') error = 'toString failure';
  else if (mode.endsWith('Undefined')) error = { undefined: true };
  else if (mode === 'bigint' && ['discardedAdd', 'protectedLoop'].includes(method)) error = { error: 'TypeError' };
  else if (protectedLoop && closeThrows) error = 'close failure';
  return error === undefined ? { value: 'done', trace } : { [protectedLoop ? 'value' : 'thrown']: error, trace };
}

const original = load(path.join(__dirname, 'CoercionRecovery.ts'));
for (const file of files) {
  const recovered = load(file);
  let count = 0;
  for (const method of ['discardedAdd', 'discardedEquality', 'discardedUnary', 'protectedLoop']) {
    for (const mode of modes) {
      for (const closeThrows of method === 'protectedLoop' ? [false, true] : [false]) {
        const expected = run(original, method, mode, closeThrows);
        assert.deepEqual(expected, oracle(method, mode, closeThrows), `source oracle ${method}/${mode}/${closeThrows}`);
        assert.deepEqual(run(recovered, method, mode, closeThrows), expected, `${file}: ${method}/${mode}/${closeThrows}`);
        count++;
      }
    }
  }
  console.log(`PASS ${file}: ${count} coercion comparisons (effects, throws, undefined, BigInt, IteratorClose precedence)`);
}
