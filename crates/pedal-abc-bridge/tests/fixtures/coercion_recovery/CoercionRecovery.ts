// 被丢弃的运算仍必须执行用户转换及其异常。
// @author sky
export function discardedAdd(value: any): string {
    value + 1;
    return "done";
}
export function discardedEquality(value: any): string {
    value == 1;
    return "done";
}
export function discardedUnary(value: any): string {
    -value;
    return "done";
}
export function protectedLoop(iterable: any): any {
    try {
        for (const value of iterable) {
            value + 1;
            return "done";
        }
        return "empty";
    } catch (error) {
        return error;
    }
}
