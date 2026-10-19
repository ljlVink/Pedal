// 原始源码与恢复代码的完整迭代器行为对照；不修改恢复函数体。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ts = require(path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));
const files = process.argv.slice(2);
if (!files.length) throw new Error('usage: node compare.cjs <recovered.ets> ...');

function snapshot(value) {
  if (value === undefined) return { undefined: true };
  if (value === null || typeof value !== 'object') return value;
  if (typeof value.name === 'string' && value.name.endsWith('Error')) return { error: value.name };
  if (Array.isArray(value)) return Array.from(value, snapshot);
  return Object.fromEntries(Object.keys(value).map(key => [key, snapshot(value[key])]));
}

const scenarios = [
  ['match', {}], ['missing', { target: 'absent' }], ['empty', { pairs: [] }],
  ['falsy request', { target: '' }], ['zero request', { target: 0 }],
  ['entries throws', { entries: 'throw' }], ['iterator getter throws', { iteratorGetter: 'throw' }],
  ['iterator method throws', { iterator: 'throw' }], ['iterator missing', { iterator: 'missing' }],
  ['next getter throws', { nextGetter: 'throw' }], ['next throws', { next: 'throw' }],
  ['next result false', { next: 'false' }], ['next result null', { next: 'null' }],
  ['next result function', { next: 'function' }], ['done getter throws', { done: 'throw' }],
  ['value getter throws', { value: 'throw' }], ['pair name getter throws', { pairName: 'throw' }],
  ['queue getter throws', { pairQueue: 'throw' }], ['request getter throws', { request: 'throw' }],
  ['body throws undefined', { request: 'undefined' }],
  ['return missing', { close: 'missing' }], ['return null', { close: 'null' }],
  ['return not callable', { close: 'noncallable' }], ['return function result', { close: 'function' }],
  ['return primitive result', { close: 'primitive' }], ['return undefined result', { close: 'undefinedResult' }],
  ['return getter throws', { closeGetter: 'throw' }], ['return call throws', { close: 'throw' }],
  ['return throws undefined', { close: 'undefinedThrow' }],
  ['return throws string resembling object', { close: 'stringThrow' }],
  ['original body exception wins over close exception', { request: 'throw', close: 'throw' }],
  ['original undefined wins over close getter exception', { request: 'undefined', closeGetter: 'throw' }],
  ['original body exception wins over invalid close result', { request: 'throw', close: 'primitive' }],
  ['done function object', { next: 'doneFunction' }],
  ['iterator own call must not be read', { poisonCall: 'iterator' }],
  ['next own call must not be read', { poisonCall: 'next' }],
  ['return own call must not be read', { poisonCall: 'return' }],
  ['exception close own call must not be read', { poisonCall: 'return', request: 'throw' }],
];

function environment(spec, method) {
  const events = [], log = [];
  const failure = kind => ({ failure: kind });
  function checkCallProperty(kind, fn) {
    if (spec.poisonCall === kind) Object.defineProperty(fn, 'call', { get() {
      events.push(`${kind}:unexpected-call-property`);
      throw failure('ownCall');
    } });
    return fn;
  }
  const pairs = spec.pairs || [['first', null], ['winner', { requestId: 'request' }], ['last', {}]];
  let cursor = 0;
  let iterator;
  const source = {};
  Object.defineProperty(source, Symbol.iterator, { get() {
    events.push('iterator:get');
    if (spec.iteratorGetter === 'throw') throw failure('iteratorGetter');
    if (spec.iterator === 'missing') return undefined;
    return checkCallProperty('iterator', function() {
      assert.equal(this, source, 'iterator method receiver');
      events.push('iterator:call');
      if (spec.iterator === 'throw') throw failure('iterator');
      return iterator;
    });
  } });
  function pairValue(pair) {
    const queue = { get requestId() {
      events.push('request:get');
      if (spec.request === 'throw') throw failure('body');
      if (spec.request === 'undefined') throw undefined;
      return 'request';
    } };
    return {
      get 0() { events.push('pair:name'); if (spec.pairName === 'throw') throw failure('pairName'); return pair[0]; },
      get 1() {
        events.push('pair:queue');
        if (spec.pairQueue === 'throw') throw failure('pairQueue');
        return spec.request ? queue : pair[1];
      },
    };
  }
  iterator = {
    get next() {
      events.push('next:get');
      if (spec.nextGetter === 'throw') throw failure('nextGetter');
      return checkCallProperty('next', function() {
        assert.equal(this, iterator, 'next receiver');
        events.push('next:call');
        if (++cursor > 10) throw new Error('step limit exceeded');
        if (spec.next === 'throw') throw failure('next');
        if (spec.next === 'false') return false;
        if (spec.next === 'null') return null;
        const result = spec.next === 'function' || spec.next === 'doneFunction' ? function() {} : {};
        Object.defineProperties(result, {
          done: { get() {
            events.push('done:get');
            if (spec.done === 'throw') throw failure('done');
            return spec.next === 'doneFunction' || cursor > pairs.length;
          } },
          value: { get() {
            events.push('value:get');
            if (spec.value === 'throw') throw failure('value');
            return method === 'query' ? pairValue(pairs[cursor - 1]) : [0, 2, 3][cursor - 1];
          } },
        });
        return result;
      });
    },
    get return() {
      events.push('return:get');
      if (spec.closeGetter === 'throw') throw failure('closeGetter');
      if (spec.close === 'missing') return undefined;
      if (spec.close === 'null') return null;
      if (spec.close === 'noncallable') return 3;
      return checkCallProperty('return', function() {
        assert.equal(this, iterator, 'return receiver');
        events.push('return:call');
        if (spec.close === 'throw') throw failure('close');
        if (spec.close === 'undefinedThrow') throw undefined;
        if (spec.close === 'stringThrow') throw '[object Object]';
        if (spec.close === 'primitive') return 0;
        if (spec.close === 'undefinedResult') return undefined;
        return spec.close === 'function' ? function() {} : {};
      });
    },
  };
  const queues = { entries() {
    assert.equal(this, queues, 'entries receiver');
    events.push('entries:call');
    if (spec.entries === 'throw') throw failure('entries');
    return source;
  } };
  return { events, log, queues, source, target: Object.hasOwn(spec, 'target') ? spec.target : 'request' };
}

function load(file) {
  let source = fs.readFileSync(file, 'utf8');
  const ast = ts.createSourceFile(file, source, ts.ScriptTarget.ES2022, true);
  const production = ast.statements.find(n => ts.isClassDeclaration(n) && n.name?.text === 'QueueManager');
  if (production) {
    const method = production.members.find(n => n.name?.getText(ast) === 'findQueueNameByRequestId');
    assert(method, 'real method exists');
    // 保留真实方法逐字文本，仅提供外部 import 和模块字符串绑定的宿主桩。
    const bindings = ast.statements.filter(ts.isVariableStatement).flatMap(n => n.declarationList.declarations)
      .filter(n => n.initializer && ts.isStringLiteral(n.initializer));
    source = bindings.map(n => `const ${n.getText(ast)};`).join('\n')
      + `\nexport class QueueProbe { ${method.getText(ast)} }`;
  }
  const output = ts.transpileModule(source, { compilerOptions: {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS,
  }, reportDiagnostics: true });
  assert.equal(output.diagnostics.filter(d => d.category === ts.DiagnosticCategory.Error).length, 0, 'syntax errors');
  const context = vm.createContext({ exports: {} });
  vm.runInContext(output.outputText, context, { timeout: 2000 });
  return { context, production: !!production };
}

function execute(loaded, spec, method) {
  const e = environment(spec, method);
  Object.assign(loaded.context, e, {
    Logger: { info(tag, message) {
      assert.equal(typeof tag, 'string', 'module TAG must be resolved');
      assert(message.startsWith('findQueueNameByRequestId error: '));
      e.log.push(['QueueProbe', JSON.parse(message.slice('findQueueNameByRequestId error: '.length))]);
    } },
    SafeJsonUtils: { stringify(error) { return JSON.stringify(snapshot(error)); } },
  });
  const call = method === 'query'
    ? 'exports.QueueProbe.prototype.findQueueNameByRequestId.call({ queues, log }, target)'
    : `exports.${method}(source, log)`;
  let value, thrown;
  try { value = vm.runInContext(call, loaded.context, { timeout: 1000 }); }
  catch (error) { thrown = snapshot(error); }
  return { value: snapshot(value), thrown, events: e.events, log: snapshot(e.log) };
}

const original = load(path.join(__dirname, 'IteratorRecovery.ts'));
for (const file of files) {
  const recovered = load(file);
  let count = 0;
  for (const method of recovered.production ? ['query'] : ['query', 'breakAfterFirst', 'continueThenReturn']) {
    for (const [name, spec] of scenarios) {
      const expected = execute(original, spec, method);
      assert.equal(expected.thrown, undefined, `${name}: source outer catch must catch all errors`);
      const actual = execute(recovered, spec, method);
      assert.deepEqual(actual, expected, `${file}: ${method}/${name}`);
      count++;
    }
  }
  console.log(`PASS ${file}: ${count} iterator/exception comparisons (receivers, return counts, exception precedence, falsy thrown values)`);
}
