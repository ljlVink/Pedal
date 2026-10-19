// 原始 TypeScript 与反编译输出的行为对照；对象展开访问器另按 VM 描述符协议验证。
// @author sky
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ts = require(path.join(process.env.OHOS_HOME || 'C:/harmony_sdk/default/openharmony',
  'ets/build-tools/ets-loader/node_modules/typescript/lib/typescript.js'));

function load(file) {
  const output = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
  }).outputText;
  const module = { exports: {} };
  vm.runInNewContext(output, { module, exports: module.exports }, { filename: file, timeout: 1000 });
  return module.exports;
}

function fields(factory, key, value) {
  const object = factory.computedFields(key, value);
  assert.equal(Object.getOwnPropertyDescriptor(object, key).writable, true);
  assert.equal(Object.getOwnPropertyDescriptor(object, key).enumerable, true);
  assert.equal(Object.getOwnPropertyDescriptor(object, key).configurable, true);
  return Reflect.ownKeys(object).map(name => [name, object[name]]);
}

function methods(factory, key, seed) {
  const object = factory.computedMethods(key, seed);
  const method = object[key];
  const proto = Object.getOwnPropertyDescriptor(object, '__proto__').value;
  const descriptor = Object.getOwnPropertyDescriptor(method, 'name');
  assert.equal(descriptor.writable, false);
  assert.equal(descriptor.enumerable, false);
  assert.equal(descriptor.configurable, true);
  return [method(5), method.name, proto(5), proto.name, Object.keys(object)];
}

async function main() {
  const files = process.argv.slice(2);
  if (!files.length) throw new Error('usage: node compare.cjs <recovered.ets> ...');
  const original = load(path.join(__dirname, 'PropertyRecovery.ts'));
  for (const file of files) {
    const recovered = load(path.resolve(file));
    for (const key of ['route', '__proto__', 17, Symbol('route'), Symbol()]) {
      assert.deepEqual(fields(recovered, key, 8), fields(original, key, 8));
      assert.deepEqual(methods(recovered, key, 8), methods(original, key, 8));
    }
    const hints = [];
    const key = { [Symbol.toPrimitive](hint) { hints.push(hint); return 'converted'; } };
    methods(recovered, key, 8);
    // 两次外部 object[key] 读取也会转换；写入指令自身只能转换一次。
    assert.deepEqual(hints, ['string', 'string']);
    const symbol = Symbol('data');
    const source = Object.create({ inherited: 4 });
    Object.defineProperties(source, {
      hidden: { value: 6, enumerable: false },
      frozen: { value: 7, enumerable: true, writable: false },
      ['__proto__']: { value: 9, enumerable: true },
      before: { value: 10, enumerable: true },
      after: { value: 11, enumerable: true },
    });
    source[symbol] = 12;
    for (const value of [source, null, undefined, 'ab', 3]) {
      assert.deepEqual(Reflect.ownKeys(recovered.copyFields(value)), Reflect.ownKeys(original.copyFields(value)));
      assert.deepEqual(Object.getOwnPropertyDescriptors(recovered.copyFields(value)), Object.getOwnPropertyDescriptors(original.copyFields(value)));
    }
    // HarmonyOS 5 RuntimeCopyDataProperties 复制 getter，不提前调用它。
    let reads = 0;
    const getter = function () { reads++; return 13; };
    const accessor = Object.create(null, { item: { get: getter, enumerable: true } });
    const copied = recovered.copyFields(accessor);
    assert.equal(reads, 0);
    assert.equal(Object.getOwnPropertyDescriptor(copied, 'item').get, getter);
    assert.equal(copied.item, 13);
    assert.equal(reads, 1);
    for (const args of [[], [3], [3, 5, 7]]) {
      assert.equal(await recovered.asyncRest(...args), await original.asyncRest(...args));
      assert.equal(await recovered.asyncRestNoAwait(...args), await original.asyncRestNoAwait(...args));
      assert.equal(await recovered.asyncRestRejected(...args), await original.asyncRestRejected(...args));
    }
    assert.equal(recovered.nestedObjectMethods(9).outer().inner().leaf(), 12);
    assert.deepEqual(Object.values(recovered.manyObjectMethods(9)).map(method => method()),
      Object.values(original.manyObjectMethods(9)).map(method => method()));
    process.stdout.write(`${path.basename(file)}: properties, function names, copy descriptors, async rest and deep methods match\n`);
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
