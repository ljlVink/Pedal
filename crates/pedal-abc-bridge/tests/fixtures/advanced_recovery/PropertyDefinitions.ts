// 静态属性定义必须绕过继承的 setter，并保留属性描述符。
// @author sky
export class DefinitionBase {
    static set answer(value: number) { throw new Error('static setter called'); }
}

export class DefinitionConstants extends DefinitionBase {
    static answer = 42;
    static label = 'ready';
    static enabled = true;
    constructor() { super(); }
}

export class DefinitionDynamic {
    static values = [1, 2, 3];
    static label = 'after array';
}
