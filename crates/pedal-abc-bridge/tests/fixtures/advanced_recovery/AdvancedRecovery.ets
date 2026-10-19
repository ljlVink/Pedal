// 异步异常、类归属与变量作用域的编译回归探针。
// @author sky

export async function awaitSuccess(value: number, trace: string[]): Promise<number> {
  trace.push('start')
  const answer = await Promise.resolve(value + 1)
  trace.push('resolved')
  return answer * 2
}

export async function awaitFailure(trace: string[]): Promise<number> {
  try {
    await Promise.reject(new Error('rejected'))
    trace.push('unreachable')
    return 1
  } catch (error) {
    trace.push((error as Error).message)
    return 2
  }
}

export async function catchFinally(fail: boolean, trace: string[]): Promise<number> {
  try {
    trace.push('try')
    if (fail) {
      await Promise.reject(new Error('rejected'))
    }
    return await Promise.resolve(3)
  } catch (error) {
    trace.push((error as Error).message)
    return 4
  } finally {
    trace.push('finally')
  }
}

export async function finallyOverridesReturn(trace: string[]): Promise<number> {
  try {
    trace.push('try')
    return await Promise.resolve(5)
  } finally {
    trace.push('finally')
    return 6
  }
}

export async function nestedFinally(fail: boolean, trace: string[]): Promise<number> {
  try {
    try {
      trace.push('inner')
      if (fail) {
        await Promise.reject(new Error('nested'))
      }
      return await Promise.resolve(7)
    } finally {
      trace.push('inner finally')
    }
  } finally {
    trace.push('outer finally')
  }
}

export async function catchThrows(trace: string[]): Promise<number> {
  try {
    await Promise.reject(new Error('first'))
    return 8
  } catch (error) {
    trace.push((error as Error).message)
    throw new Error('second')
  } finally {
    trace.push('finally')
  }
}

export async function awaitLoop(count: number, trace: string[]): Promise<number> {
  let total = 0
  for (let index = 0; index < count; index++) {
    try {
      total += await Promise.resolve(index)
      if (index === 1) {
        continue
      }
      trace.push('body')
    } finally {
      trace.push('iteration finally')
    }
  }
  return total
}

export async function awaitInFinally(fail: boolean, trace: string[]): Promise<number> {
  try {
    if (fail) {
      throw new Error('pending')
    }
    return 9
  } finally {
    await Promise.resolve(0)
    trace.push('awaited finally')
  }
}

export class RecoveryBase {
  protected amount: number

  constructor(initial: number) {
    this.amount = initial
  }

  get value(): number { return this.amount }
  set value(next: number) { this.amount = next }
  describe(prefix: string): string { return prefix + this.amount }
  static identity(value: number): number { return value }
}

export class RecoveryChild extends RecoveryBase {
  private suffix: string

  constructor(initial: number, suffix: string) {
    super(initial)
    this.suffix = suffix
  }

  override describe(prefix: string): string { return super.describe(prefix) + this.suffix }
  get doubled(): number { return this.value * 2 }
}

export class RecoverySibling {
  describe(prefix: string): string { return prefix + 'sibling' }
  static identity(value: number): number { return value + 1 }
}

export function scopedNames(input: number): number {
  let result = input
  if (input > 0) {
    const adjustment = input + 2
    result += adjustment
  } else {
    const adjustment = input - 2
    result -= adjustment
  }
  return result
}

export function closureNames(initial: number): () => number {
  let current = initial
  return () => {
    const increment = 2
    current += increment
    return current
  }
}
