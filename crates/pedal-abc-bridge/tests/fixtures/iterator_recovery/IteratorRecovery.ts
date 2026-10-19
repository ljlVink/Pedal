// 独立 for-of/IteratorClose 语义探针，不依赖鸿蒙宿主服务。
// @author sky
const TAG = "QueueProbe";

export class QueueProbe {
    constructor(public queues: any, public log: any[]) {}

    findQueueNameByRequestId(requestId: any): any {
        if (!requestId) return "";
        try {
            for (const pair of this.queues.entries()) {
                const name = pair[0];
                const queue = pair[1];
                if (queue?.requestId === requestId) return name;
            }
        } catch (error) {
            this.log.push([TAG, error]);
        }
        return "";
    }
}

export function breakAfterFirst(source: any, log: any[]): any {
    try {
        for (const value of source) {
            log.push(value);
            break;
        }
        return "done";
    } catch (error) {
        log.push(error);
        return "caught";
    }
}

export function continueThenReturn(source: any, log: any[]): any {
    try {
        for (const value of source) {
            if (!value) continue;
            log.push(value);
            return value;
        }
        return "empty";
    } catch (error) {
        log.push(error);
        return "caught";
    }
}
