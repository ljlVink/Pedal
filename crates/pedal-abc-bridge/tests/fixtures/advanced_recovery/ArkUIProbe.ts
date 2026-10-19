// hvigor 降低后的最小 ArkUI 组件回归样例。
// @author sky

if (!("finalizeConstruction" in ViewPU.prototype)) {
    Reflect.set(ViewPU.prototype, "finalizeConstruction", () => { });
}

interface ProbePage_Params {
    counter?: number;
    title?: string;
    circles?: Circle[];
}

interface Shape<R> {
    name: string;
    area(): R;
    describe(prefix: string): string;
}

export class Circle implements Shape<number> {
    static PI: number = 3.14159;
    private static count: number = 0;
    readonly id: number;
    public name: string = 'circle';
    public radius: number;
    protected label?: string;
    constructor(radius: number, label?: string) {
        Circle.count++;
        this.id = Circle.count;
        this.radius = radius;
        this.label = label ?? 'none';
    }
    static total(): number { return Circle.count; }
    get diameter(): number { return this.radius * 2; }
    set diameter(v: number) { this.radius = v / 2; }
    area(): number { return Circle.PI * this.radius ** 2; }
    describe(prefix: string): string {
        return `${prefix}${this.label}: r=${this.radius}, d=${this.diameter}`;
    }
}

export namespace MathUtil {
    export function clamp(v: number, lo: number, hi: number): number {
        return v < lo ? lo : (v > hi ? hi : v);
    }
    export const VERSION: string = '1.0';
}

class ProbePage extends ViewPU {
    constructor(parent, params, __localStorage, elmtId = -1, paramsLambda = undefined, extraInfo) {
        super(parent, __localStorage, elmtId, extraInfo);
        if (typeof paramsLambda === "function") {
            this.paramsGenerator_ = paramsLambda;
        }
        this.__counter = new ObservedPropertySimplePU(0, this, "counter");
        this.__title = new SynchedPropertySimpleOneWayPU(params.title, this, "title");
        this.circles = [new Circle(1), new Circle(2, 'big')];
        this.setInitiallyProvidedValue(params);
        this.finalizeConstruction();
    }
    setInitiallyProvidedValue(params: ProbePage_Params) {
        if (params.counter !== undefined) {
            this.counter = params.counter;
        }
        if (params.title === undefined) {
            this.__title.set('probe');
        }
        if (params.circles !== undefined) {
            this.circles = params.circles;
        }
    }
    updateStateVars(params: ProbePage_Params) {
        this.__title.reset(params.title);
    }
    purgeVariableDependenciesOnElmtId(rmElmtId) {
        this.__counter.purgeDependencyOnElmtId(rmElmtId);
        this.__title.purgeDependencyOnElmtId(rmElmtId);
    }
    aboutToBeDeleted() {
        this.__counter.aboutToBeDeleted();
        this.__title.aboutToBeDeleted();
        SubscriberManager.Get().delete(this.id__());
        this.aboutToBeDeletedInternal();
    }
    private __counter: ObservedPropertySimplePU<number>;
    get counter() {
        return this.__counter.get();
    }
    set counter(newValue: number) {
        this.__counter.set(newValue);
    }
    private __title: SynchedPropertySimpleOneWayPU<string>;
    get title() {
        return this.__title.get();
    }
    set title(newValue: string) {
        this.__title.set(newValue);
    }
    private circles: Circle[];
    initialRender() {
        this.observeComponentCreation2((elmtId, isInitialRender) => {
            Column.create();
            Column.width('100%');
            Column.height('100%');
        }, Column);
        this.observeComponentCreation2((elmtId, isInitialRender) => {
            Text.create(`${this.title} ${this.counter}`);
            Text.fontSize(20);
            Text.onClick(() => { this.counter = MathUtil.clamp(this.counter + 1, 0, 10); });
        }, Text);
        Text.pop();
        this.observeComponentCreation2((elmtId, isInitialRender) => {
            ForEach.create();
            const forEachItemGenFunction = _item => {
                const c = _item;
                this.observeComponentCreation2((elmtId, isInitialRender) => {
                    Text.create(c.describe('c-'));
                    Text.fontSize(14);
                }, Text);
                Text.pop();
            };
            this.forEachUpdateFunction(elmtId, this.circles, forEachItemGenFunction, (c: Circle) => c.id.toString(), false, false);
        }, ForEach);
        ForEach.pop();
        this.observeComponentCreation2((elmtId, isInitialRender) => {
            Button.createWithLabel('reset');
            Button.onClick(() => { this.counter = 0; });
        }, Button);
        Button.pop();
        Column.pop();
    }
    rerender() {
        this.updateDirtyElements();
    }
    static getEntryName(): string {
        return "ProbePage";
    }
}

registerNamedRoute(() => new ProbePage(undefined, {}), "", { bundleName: "com.example.arktsprobe", moduleName: "entry", pagePath: "pages/Index", pageFullPath: "entry/src/main/ets/pages/Index", integratedHsp: "false", moduleType: "followWithHap" });
