// 完整执行原始和恢复模块，检查导入继承、参数转发与构造器副作用顺序。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');

const [recoveredPath, compilerPath] = process.argv.slice(2);
if (!recoveredPath) {
  throw new Error('usage: node compare_inheritance.cjs <recovered.ets> [typescript.js]');
}
const ts = require(compilerPath || path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));
const classNames = ['ImportedSheet', 'NamespaceSheet', 'SpreadSheet', 'ModifiedSheet'];

function compile(file) {
  const compiled = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
    reportDiagnostics: true,
  });
  const errors = compiled.diagnostics.filter(item => item.category === ts.DiagnosticCategory.Error);
  assert.equal(errors.length, 0,
    `${file}: ${errors.map(item => ts.flattenDiagnosticMessageText(item.messageText, '\n')).join('\n')}`);
  return compiled.outputText;
}

function execute(code, className, mode, argsCount) {
  const calls = [];
  const events = [];
  const marker = { identity: 'forwarded object' };
  const replacement = { replacement: true };
  const failure = new Error('base constructor failure');
  class Base {
    constructor(...args) {
      calls.push(args);
      if (className === 'ModifiedSheet') {
        events.push(`base:${args[0]}`);
      }
      if (mode === 'throw') { throw failure; }
      if (mode === 'replace') { return replacement; }
      this.baseInitialized = true;
    }
    inherited() { return 'from Base'; }
  }
  const imports = [];
  const exports = {};
  const context = vm.createContext({
    exports,
    require(request) {
      imports.push(request);
      assert.equal(request, './Base');
      return { Base };
    },
  });
  vm.runInContext(code, context, { timeout: 1000 });
  assert.deepEqual(Object.keys(exports).sort(), [...classNames].sort(), 'full module exports');
  assert.ok(imports.length > 0, 'parent class must come from the import');
  const Child = exports[className];
  assert.equal(Object.getPrototypeOf(Child), Base, `${className}: static inheritance`);
  assert.equal(Object.getPrototypeOf(Child.prototype), Base.prototype, `${className}: instance inheritance`);
  if (mode === 'poison_push') {
    // 只修改被测模块所属 realm，宿主断言/跟踪数组及下一次用例不受影响。
    vm.runInContext(`Array.prototype.push = function () {
      throw new Error('super spread must not depend on Array.prototype.push');
    };`, context, { timeout: 1000 });
  }
  const supplied = className === 'ModifiedSheet'
    ? [10, events]
    : Array.from({ length: argsCount }, (_, index) =>
      index === 2 ? marker : index === 3 ? undefined : `argument:${index}`);
  let result;
  let thrown;
  try { result = Reflect.construct(Child, supplied); } catch (error) { thrown = error; }
  assert.equal(calls.length, 1, `${className}: super executes exactly once`);
  const expectedArgs = className === 'ModifiedSheet' ? [11, events] : supplied;
  assert.equal(calls[0].length, expectedArgs.length, `${className}: forwarded argument count`);
  for (let i = 0; i < expectedArgs.length; i++) {
    assert.equal(calls[0][i], expectedArgs[i], `${className}: argument ${i} identity/value`);
  }
  if (className === 'ModifiedSheet') {
    assert.deepEqual(events, mode === 'throw' ? ['before', 'base:11'] : ['before', 'base:11', 'after'],
      'side effects before/inside/after super');
  }
  if (mode === 'throw') {
    assert.equal(thrown, failure, `${className}: preserves the actual base exception`);
    return { className, mode, argsCount, events: [...events], baseCalls: calls.length, thrown: true };
  }
  if (thrown) { throw thrown; }
  const ownField = Object.getOwnPropertyDescriptor(result, 'isSheetShow');
  assert.deepEqual(ownField, { value: false, writable: true, enumerable: true, configurable: true },
    `${className}: initializes an own field after super`);
  let accessors = false;
  if (mode === 'replace') {
    assert.equal(result, replacement, `${className}: adopts the object returned by the parent`);
    assert.equal(Object.getPrototypeOf(result), Object.prototype, 'returned object keeps its own prototype');
  } else {
    assert.equal(Object.getPrototypeOf(result), Child.prototype);
    assert.ok(result instanceof Child);
    assert.ok(result instanceof Base);
    assert.equal(result.baseInitialized, true);
    assert.equal(result.inherited(), 'from Base');
    if (className === 'ImportedSheet' || className === 'NamespaceSheet') {
      const property = Object.getOwnPropertyDescriptor(Child.prototype, 'visible');
      assert.equal(typeof property.get, 'function', 'getter belongs to the class');
      assert.equal(typeof property.set, 'function', 'setter belongs to the class');
      assert.equal(result.visible, false);
      result.visible = true;
      assert.equal(result.visible, true);
      assert.equal(result.isSheetShow, true);
      accessors = true;
    }
    if (className === 'ImportedSheet') {
      assert.equal(result.getIsShow(), true);
      result.setIsShow(false);
      assert.equal(result.getIsShow(), false);
    }
  }
  return { className, mode, argsCount, events: [...events], baseCalls: calls.length,
    accessors, replaced: result === replacement, initialField: ownField.value };
}

const reference = compile(path.join(__dirname, 'ImportedInheritance.ts'));
const recovered = compile(recoveredPath);
let count = 0;
for (const className of classNames) {
  // ModifiedSheet 源码明确调用 events.push，其余类的参数展开不应引入 push。
  const modes = className === 'ModifiedSheet'
    ? ['normal', 'replace', 'throw'] : ['normal', 'replace', 'throw', 'poison_push'];
  for (const mode of modes) {
    const arities = className === 'ModifiedSheet' ? [2] : [0, 1, 4, 12, 1000];
    for (const argsCount of arities) {
      const expected = execute(reference, className, mode, argsCount);
      const actual = execute(recovered, className, mode, argsCount);
      assert.deepEqual(actual, expected, `${className}/${mode}/${argsCount}`);
      count++;
    }
  }
}
console.log(`PASS imported inheritance (${count} source/recovered comparisons: aliases, namespace, forwarding through 1000 arguments, accessors, replacement objects, exceptions, evaluation order, poisoned Array.prototype.push)`);
