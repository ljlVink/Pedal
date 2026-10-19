// 编译源码与反编译输出的独立 Node 行为对照；每个样例隔离并设置超时。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { Worker, isMainThread, parentPort, workerData } = require('node:worker_threads');

const cases = [
  ['awaitSuccess', [3], { value: 8, trace: ['start', 'resolved'] }],
  ['awaitFailure', [], { value: 2, trace: ['rejected'] }],
  ['catchFinally', [false], { value: 3, trace: ['try', 'finally'] }],
  ['catchFinally', [true], { value: 4, trace: ['try', 'rejected', 'finally'] }],
  ['finallyOverridesReturn', [], { value: 6, trace: ['try', 'finally'] }],
  ['nestedFinally', [false], { value: 7, trace: ['inner', 'inner finally', 'outer finally'] }],
  ['nestedFinally', [true], { error: 'nested', trace: ['inner', 'inner finally', 'outer finally'] }],
  ['catchThrows', [], { error: 'second', trace: ['first', 'finally'] }],
  ['awaitLoop', [0], { value: 0, trace: [] }],
  ['awaitLoop', [3], { value: 3, trace: ['body', 'iteration finally', 'iteration finally', 'body', 'iteration finally'] }],
  ['awaitInFinally', [false], { value: 9, trace: ['awaited finally'] }],
  ['awaitInFinally', [true], { error: 'pending', trace: ['awaited finally'] }],
  ['scopedNames', [3], { value: 8, trace: [] }],
  ['scopedNames', [-3], { value: 2, trace: [] }],
  ['closureNames', [4], { value: [6, 8], trace: [] }],
  ['classOwnership', [], { value: [4, 6, 'n:3!', 'n:sibling', 5, 6], trace: [] }],
  ['wholeRecord', [], { value: [8, 6, 42, 8, 6, 'n:3!', 'n:sibling', 5, 6], trace: ['start', 'resolved'] }],
  ['catchFinally', [false], { value: 4, trace: ['try', 'injected', 'finally'] }, { rejectResolve: 'all' }],
  ['finallyOverridesReturn', [], { value: 6, trace: ['try', 'finally'] }, { rejectResolve: 'all' }],
  ['nestedFinally', [false], { error: 'injected', trace: ['inner', 'inner finally', 'outer finally'] }, { rejectResolve: 'all' }],
  ['awaitLoop', [3], { error: 'injected', trace: ['body', 'iteration finally', 'iteration finally'] }, { rejectResolve: 1 }],
  ['catchFinally', [true], { error: 'cleanup', trace: ['try', 'rejected', 'finally'] }, { throwOnTrace: 'finally' }],
  ['awaitInFinally', [false], { error: 'injected', trace: [] }, { rejectResolve: 'all' }],
  ['awaitInFinally', [true], { error: 'injected', trace: [] }, { rejectResolve: 'all' }],
];

if (!isMainThread) {
  (async () => {
    const ts = require(workerData.typescript);
    const ast = ts.createSourceFile('probe.ts', workerData.source, ts.ScriptTarget.ES2020, true);
    const names = workerData.name === 'classOwnership'
      ? ['RecoveryBase', 'RecoveryChild', 'RecoverySibling']
      : [workerData.name];
    // 仅载入待验证声明；模块初始化与 ArkUI 的宿主服务不属于纯函数行为对照。
    const declarations = ast.statements.filter(statement =>
      (ts.isFunctionDeclaration(statement) || ts.isClassDeclaration(statement)) &&
      statement.name && names.includes(statement.name.text));
    if (workerData.name !== 'wholeRecord' && declarations.length !== names.length) {
      throw new Error(`expected declarations ${names}, found ${declarations.map(item => item.name.text)}`);
    }
    const selected = workerData.name === 'wholeRecord'
      ? workerData.source
      : declarations.map(statement => statement.getText(ast)).join('\n');
    const compiled = ts.transpileModule(selected, {
      compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS },
      reportDiagnostics: true,
    });
    const syntaxErrors = compiled.diagnostics.filter(item => item.category === ts.DiagnosticCategory.Error);
    if (syntaxErrors.length) {
      throw new Error(`${syntaxErrors.length} syntax diagnostic(s):\n` + ts.formatDiagnosticsWithColorAndContext(syntaxErrors.slice(0, 3), {
        getCurrentDirectory: () => '', getNewLine: () => '\n', getCanonicalFileName: name => name,
      }));
    }
    const globals = { exports: {}, console };
    const environment = workerData.environment || {};
    if (environment.rejectResolve !== undefined) {
      globals.Promise = {
        resolve(value) {
          return environment.rejectResolve === 'all' || value === environment.rejectResolve
            ? Promise.reject(new Error('injected')) : Promise.resolve(value);
        },
        reject: value => Promise.reject(value),
      };
    }
    const context = vm.createContext(globals);
    const expose = workerData.name === 'wholeRecord'
      ? '\nthis.probe = exports;'
      : `\nthis.probe = { ${names.join(', ')} };`;
    vm.runInContext(compiled.outputText + expose, context, { timeout: 1000 });
    const trace = [];
    if (environment.throwOnTrace) {
      Object.defineProperty(trace, 'push', { value(value) {
        const length = Array.prototype.push.call(this, value);
        if (value === environment.throwOnTrace) { throw new Error('cleanup'); }
        return length;
      } });
    }
    let outcome;
    try {
      let value;
      if (workerData.name === 'wholeRecord') {
        const module = context.probe;
        const answer = await module.awaitSuccess(3, trace);
        const first = module.closureNames(4);
        const second = module.closureNames(40);
        const child = new module.RecoveryChild(3, '!');
        value = [answer, first(), second(), first(), child.doubled, child.describe('n:'),
          new module.RecoverySibling().describe('n:'), module.RecoveryBase.identity(5), module.RecoverySibling.identity(5)];
      } else if (workerData.name === 'classOwnership') {
        const { RecoveryBase, RecoveryChild, RecoverySibling } = context.probe;
        const base = new RecoveryBase(2);
        base.value = 4;
        const child = new RecoveryChild(3, '!');
        value = [base.value, child.doubled, child.describe('n:'), new RecoverySibling().describe('n:'),
          RecoveryBase.identity(5), RecoverySibling.identity(5)];
      } else if (workerData.name === 'closureNames') {
        const counter = context.probe.closureNames(...workerData.args);
        value = [counter(), counter()];
      } else {
        value = await context.probe[workerData.name](...workerData.args, trace);
      }
      outcome = { value, trace };
    } catch (error) {
      outcome = { error: error.message, trace };
    }
    parentPort.postMessage(outcome);
  })().catch(error => parentPort.postMessage({ harnessError: error.message }));
} else {
  async function run(source, typescript, name, args, environment) {
    return new Promise(resolve => {
      const worker = new Worker(__filename, { workerData: { source, typescript, name, args, environment } });
      const timer = setTimeout(() => {
        worker.terminate();
        resolve({ harnessError: 'execution exceeded 3000 ms' });
      }, 3000);
      worker.once('message', result => { clearTimeout(timer); worker.terminate(); resolve(result); });
      worker.once('error', error => { clearTimeout(timer); resolve({ harnessError: error.message }); });
    });
  }

  (async () => {
    const recoveredPath = process.argv[2];
    if (!recoveredPath) {
      throw new Error('usage: node compare.cjs <recovered.ets> [typescript.js]');
    }
    const typescript = process.argv[3] || path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
      'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js');
    const original = fs.readFileSync(path.join(__dirname, 'AdvancedRecovery.ets'), 'utf8');
    const recovered = fs.readFileSync(recoveredPath, 'utf8');
    let failed = 0;
    for (const [name, args, expected, environment] of cases) {
      const label = `${name}(${args})${environment ? ' ' + JSON.stringify(environment) : ''}`;
      const sourceResult = await run(original, typescript, name, args, environment);
      assert.deepEqual(sourceResult, expected, `source oracle: ${label}`);
      const actual = await run(recovered, typescript, name, args, environment);
      try {
        assert.deepEqual(actual, sourceResult);
        console.log(`PASS ${label}`);
      } catch {
        failed++;
        console.log(`FAIL ${label} expected=${JSON.stringify(sourceResult)} actual=${JSON.stringify(actual)}`);
      }
    }
    console.log(`${cases.length - failed}/${cases.length} behavior cases passed`);
    process.exitCode = failed ? 1 : 0;
  })().catch(error => { console.error(error); process.exitCode = 1; });
}
