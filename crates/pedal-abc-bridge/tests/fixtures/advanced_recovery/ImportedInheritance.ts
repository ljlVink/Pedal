// 导入父类、默认派生构造器与 super 参数求值顺序的独立回归样例。
// @author sky
import { Base as ImportedBase } from './Base';
import * as BaseModule from './Base';

export class ImportedSheet extends ImportedBase {
    isSheetShow = false;

    get visible(): boolean { return this.isSheetShow; }
    set visible(value: boolean) { this.isSheetShow = value; }
    getIsShow(): boolean { return this.isSheetShow; }
    setIsShow(value: boolean): void { this.isSheetShow = value; }
}

export class NamespaceSheet extends BaseModule.Base {
    isSheetShow = false;

    get visible(): boolean { return this.isSheetShow; }
    set visible(value: boolean) { this.isSheetShow = value; }
}

export class SpreadSheet extends ImportedBase {
    isSheetShow = false;

    constructor(...args: unknown[]) {
        super(...args);
    }
}

export class ModifiedSheet extends ImportedBase {
    isSheetShow = false;

    constructor(value: number, events: string[]) {
        events.push('before');
        value = value + 1;
        super(value, events);
        events.push('after');
    }
}
