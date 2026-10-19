// taskpool completion protocols emitted by the SDK's lowered @Concurrent directive.
export async function concurrentFulfilled(value: any): Promise<any> {
  "use concurrent";
  return await value;
}
export async function concurrentRejected(error: any): Promise<any> {
  "use concurrent";
  throw error;
}
export function concurrentValue(value: any): any {
  "use concurrent";
  return value;
}
export function concurrentImplicit(effect: any): void {
  "use concurrent";
  effect();
}
export function concurrentChoose(flag: boolean, left: any, right: any): any {
  "use concurrent";
  if (flag) return left();
  return right();
}
export function concurrentBranch(flag: boolean, value: any): any {
  "use concurrent";
  let result: any;
  if (flag) result = value.left();
  else result = value.right();
  return result;
}
