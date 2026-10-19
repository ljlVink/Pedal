// 将编译器降低的 ArkUI 类与恢复类作成员及宿主桩行为对照。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');

const [referencePath, recoveredPath, compilerPath] = process.argv.slice(2);
if (!referencePath || !recoveredPath) {
  throw new Error('usage: node compare_arkui.cjs <compiler-generated Index.ts> <recovered Index.ets> [typescript.js]');
}
const ts = require(compilerPath || path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));

function load(file) {
  const text = fs.readFileSync(file, 'utf8');
  const ast = ts.createSourceFile('Index.ts', text, ts.ScriptTarget.ES2020, true);
  const classes = ast.statements.filter(statement => ts.isClassDeclaration(statement) &&
    ['Circle', 'ProbePage'].includes(statement.name?.text));
  assert.equal(classes.length, 2, 'Circle and ProbePage must both be class declarations');
  const page = classes.find(item => item.name.text === 'ProbePage');
  const members = page.members.filter(member => !ts.isPropertyDeclaration(member)).map(member => {
    const name = ts.isConstructorDeclaration(member) ? 'constructor' : member.name?.getText(ast);
    const kind = ts.isGetAccessorDeclaration(member) ? 'get ' : ts.isSetAccessorDeclaration(member) ? 'set ' : '';
    const prefix = member.modifiers?.some(modifier => modifier.kind === ts.SyntaxKind.StaticKeyword) ? 'static ' : '';
    return prefix + kind + name;
  }).sort();
  for (const statement of ast.statements) {
    if (ts.isFunctionDeclaration(statement) && statement.name &&
        ['ProbePage', 'initialRender', 'rerender', 'aboutToBeDeleted'].includes(statement.name.text)) {
      throw new Error(`detached component member: ${statement.name.text}`);
    }
  }
  const source = classes.map(item => item.getText(ast)).join('\n');
  const compiled = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS }, reportDiagnostics: true,
  });
  const diagnostics = compiled.diagnostics.filter(item => item.category === ts.DiagnosticCategory.Error);
  assert.equal(diagnostics.length, 0, diagnostics.map(item => item.messageText).join('\n'));
  return { members, code: compiled.outputText };
}

function execute(code) {
  const trace = [];
  const render = [];
  const itemKeys = [];
  const listeners = new Map();
  class ViewPU {
    finalizeConstruction() { trace.push('finalize'); }
    updateDirtyElements() { trace.push('rerender'); }
    id__() { return 12; }
    aboutToBeDeletedInternal() { trace.push('delete internal'); }
    observeComponentCreation2(callback) { callback(1, true); }
    forEachUpdateFunction(id, items, generate, key) {
      for (const item of items) { itemKeys.push(key(item)); generate(item); }
    }
  }
  class Property {
    constructor(value, owner, name) { this.value = value; this.name = name; }
    get() { return this.value; }
    set(value) { this.value = value; }
    reset(value) { this.value = value; }
    purgeDependencyOnElmtId(id) { trace.push(`purge ${this.name} ${id}`); }
    aboutToBeDeleted() { trace.push(`delete ${this.name}`); }
  }
  function component(name) {
    return Object.fromEntries(['create', 'createWithLabel', 'width', 'height', 'fontSize', 'pop', 'onClick'].map(method =>
      [method, value => {
        render.push(`${name}.${method}${value === undefined || typeof value === 'function' ? '' : ':' + value}`);
        if (method === 'onClick') { listeners.set(name, value); }
      }]));
  }
  const context = vm.createContext({
    exports: {}, ViewPU, ObservedPropertySimplePU: Property, SynchedPropertySimpleOneWayPU: Property,
    SubscriberManager: { Get: () => ({ delete: id => trace.push(`delete subscriber ${id}`) }) },
    Column: component('Column'), Text: component('Text'), ForEach: component('ForEach'), Button: component('Button'),
  });
  vm.runInContext(code + '\nthis.Page = ProbePage;', context, { timeout: 1000 });
  const page = new context.Page(undefined, {});
  const initial = [page.counter, page.title, page.circles.map(circle => circle.radius)];
  page.counter = 3;
  page.title = 'edited';
  const updated = [page.counter, page.title];
  page.updateStateVars({ title: 'reset' });
  const reset = page.title;
  page.initialRender();
  listeners.get('Button')();
  const counterAfterClick = page.counter;
  page.purgeVariableDependenciesOnElmtId(4);
  page.rerender();
  page.aboutToBeDeleted();
  return JSON.parse(JSON.stringify({ initial, updated, reset, trace, render, itemKeys, counterAfterClick,
    entry: context.Page.getEntryName() }));
}

const reference = load(referencePath);
const recovered = load(recoveredPath);
assert.deepEqual(recovered.members, reference.members, 'complete ArkUI component member ownership');
console.log(`PASS ArkUI component member ownership (${reference.members.length} members)`);
const expected = execute(reference.code);
assert.deepEqual(expected, {
  initial: [0, 'probe', [1, 2]], updated: [3, 'edited'], reset: 'reset', entry: 'ProbePage',
  trace: ['finalize', 'purge counter 4', 'purge title 4', 'rerender', 'delete counter', 'delete title',
    'delete subscriber 12', 'delete internal'],
  render: ['Column.create', 'Column.width:100%', 'Column.height:100%', 'Text.create:reset 3', 'Text.fontSize:20',
    'Text.onClick', 'Text.pop', 'ForEach.create', 'Text.create:c-none: r=1, d=2', 'Text.fontSize:14', 'Text.pop',
    'Text.create:c-big: r=2, d=4', 'Text.fontSize:14', 'Text.pop', 'ForEach.pop', 'Button.createWithLabel:reset',
    'Button.onClick', 'Button.pop', 'Column.pop'],
  itemKeys: ['1', '2'], counterAfterClick: 0,
});
assert.deepEqual(execute(recovered.code), expected, 'component constructor, accessors and lifecycle behavior');
console.log('PASS ArkUI constructor/accessors/state/render callbacks/lifecycle with explicit host stubs');
