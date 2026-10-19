// 三组对象方法嵌入与词法捕获回归探针。
// @author sky
export function makeObjectMethodGroups(seed: number): any[] {
    const first = {
        alpha() { return "first-alpha"; },
        beta() { return "first-beta"; },
        gamma() { return "first-gamma"; },
    };
    const second = {
        delta() { return "second-delta"; },
        epsilon() { return "second-epsilon"; },
        zeta() { return "second-zeta"; },
    };
    const third = {
        eta() { return seed + 7; },
        theta() { return seed + 11; },
        iota() { return seed + 13; },
    };
    return [first, second, third];
}

export function makeDeepCapture(seed: number): any {
    return () => () => () => () => () => seed;
}
