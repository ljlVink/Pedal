// 计算属性、对象展开、深层对象方法与异步 rest 参数回归。
// @author sky

export function computedFields(key: string, value: number): Object {
  return { [key]: value, 4: value + 1, 9: value + 2 };
}

export function computedMethods(key: string, seed: number): Object {
  return {
    [key]: (delta: number): number => seed + delta,
    ["__proto__"]: (delta: number): number => seed - delta,
  };
}

export function copyFields(source: Object): Object {
  return { before: 1, ...source, after: 2 };
}

export async function asyncRest(...values: number[]): Promise<number> {
  return await Promise.resolve(values[0] + values.length);
}

export async function asyncRestNoAwait(...values: number[]): Promise<number> {
  return values.length;
}

export async function asyncRestRejected(...values: number[]): Promise<number> {
  try {
    await Promise.reject("rest rejection");
    return 0;
  } catch (error) {
    return values.length;
  }
}

export function nestedObjectMethods(seed: number): Object {
  return {
    outer() {
      return {
        inner() {
          return { leaf() { return seed + 3; } };
        },
      };
    },
  };
}

export function manyObjectMethods(seed: number): Object {
  return {
    m0() { return seed; }, m1() { return seed + 1; },
    m2() { return seed + 2; }, m3() { return seed + 3; },
    m4() { return seed + 4; }, m5() { return seed + 5; },
    m6() { return seed + 6; }, m7() { return seed + 7; },
    m8() { return seed + 8; }, m9() { return seed + 9; },
    m10() { return seed + 10; }, m11() { return seed + 11; },
    m12() { return seed + 12; }, m13() { return seed + 13; },
    m14() { return seed + 14; }, m15() { return seed + 15; },
  };
}
