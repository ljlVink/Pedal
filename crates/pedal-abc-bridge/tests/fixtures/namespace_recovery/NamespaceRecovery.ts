// 命名空间初始化闭包、私有捕获和独立闭包实例的编译器回归。
// @author sky
export namespace shaderHandler {
    const privatePrefix = "shader";
    const privateOffset = 4;

    export async function resolve(task: (name: string) => Promise<number>): Promise<string> {
        const value = await task(privatePrefix);
        return privatePrefix + ":" + (value + privateOffset);
    }

    export function makeCounter(seed: number): () => number {
        let current = seed;
        return () => {
            current = current + privateOffset;
            return current;
        };
    }
}

export const firstCounter = shaderHandler.makeCounter(3);
export const secondCounter = shaderHandler.makeCounter(10);
