// HiNote 注释中暴露的私有字段、嵌套类、枚举和异步闭包回归。
// @author sky

export class Counter {
  #value = 0;
  #other = 2;
  constructor(start: number) { this.#value = start; }
  add(delta: number): number { this.#value += delta; return this.#value; }
  get value(): number { return this.#value; }
  get other(): number { return this.#other; }
}

export namespace Models {
  export class Box {
    _value = 0;
    get value(): number { return this._value; }
    set value(value: number) { this._value = value; }
  }
}

@Sendable
export class Transferable {
  static make(): number { return 7; }
  get(): number { return 8; }
}

export function enumerableKeys(value: Object): string[] {
  const keys: string[] = [];
  for (const key in value) { keys.push(key); }
  return keys;
}

export function construct(ctor, args): Object { return new ctor(...args); }

export async function asyncClosure(value: number): Promise<number> {
  const callback = () => value + 1;
  return callback();
}

class Base {
  get value(): number { return this.marker + 1; }
  set value(value: number) { this.marker = value + 2; }
  method(delta: number): number { return this.marker + delta; }
}

export class Child extends Base {
  marker = 3;
  read(): number { return (() => super.value)(); }
  computed(): number { return (() => super["value"])(); }
  call(): number { return ((delta: number) => super.method(delta))(4); }
  write(value: number): number { super.value = value; return this.marker; }
}
