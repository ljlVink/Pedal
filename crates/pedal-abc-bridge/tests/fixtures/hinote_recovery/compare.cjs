// HiNote 指令回归的原始源码/恢复源码行为对照。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ts = require(path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));

function load(file) {
  const output = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const module = { exports: {} };
  vm.runInNewContext(output, { module, exports: module.exports, Sendable: value => value },
    { filename: file, timeout: 1000 });
  return module.exports;
}

async function main() {
  const original = load(path.join(__dirname, 'HiNoteRecovery.ts'));
  for (const file of process.argv.slice(2)) {
    const recovered = load(path.resolve(file));
    for (const api of [original, recovered]) {
      const first = new api.Counter(3);
      const second = new api.Counter(9);
      assert.equal(first.add(4), 7);
      assert.equal(first.value, 7);
      assert.equal(first.other, 2);
      assert.equal(second.value, 9);
      assert.deepEqual(Reflect.ownKeys(first), []);
      assert.throws(() => api.Counter.prototype.add.call({}, 1), { name: 'TypeError' });
      const box = new api.Models.Box();
      box.value = 17;
      assert.equal(box.value, 17);
      assert.equal(api.Transferable.make(), 7);
      assert.equal(new api.Transferable().get(), 8);
      assert.equal(await api.asyncClosure(5), 6);
      const child = new api.Child();
      assert.equal(child.read(), 4);
      assert.equal(child.computed(), 4);
      assert.equal(child.call(), 7);
      assert.equal(child.write(5), 7);
      assert.equal(child.read(), 8);
      const object = Object.create({ inherited: 2 });
      object.own = 1;
      Object.defineProperty(object, 'hidden', { value: 3 });
      object[Symbol('ignored')] = 4;
      assert.deepEqual(Array.from(api.enumerableKeys(object)), ['own', 'inherited']);
      const result = api.construct(function (a, b) { this.sum = a + b; }, [3, 4]);
      assert.equal(result.sum, 7);
    }
    console.log(`${file}: source behavior matches`);
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
