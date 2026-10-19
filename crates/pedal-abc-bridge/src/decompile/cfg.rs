//! 方法级控制流图：基本块划分 + 后继 + try 覆盖
//!
//! @author ljlVink

use crate::file::TryBlock;

/// 基本块
#[derive(Debug, Clone)]
pub struct Block {
    /// 起始指令下标
    pub start: usize,
    /// 结束指令下标（不含）
    pub end: usize,
    /// 起始 pc
    pub start_pc: usize,
    /// 结束 pc（不含）
    pub end_pc: usize,
    /// 后继块下标
    pub succs: Vec<usize>,
    /// 是否以条件跳转结尾
    pub ends_cond: bool,
    /// try 块覆盖（start_pc 区间命中的块）
    pub try_of: Vec<(u32, u32, u32)>, // (start_pc, length, handler_pc)
}

/// 自然循环（支配者保证 header 支配回边源）
#[derive(Debug, Clone)]
pub struct Loop {
    /// 循环头块
    pub header: usize,
    /// 循环体块集合（含 header）
    pub body: Vec<usize>,
    /// 回边源块
    pub back_edges: Vec<usize>,
    /// 出口块（体内跳出的目标）
    pub exits: Vec<usize>,
    /// true = do-while（回边块的另一后继即出口）
    pub is_do_while: bool,
}

/// 控制流图
#[derive(Debug)]
pub struct Cfg {
    /// 基本块（按起始 pc 排序）
    pub blocks: Vec<Block>,
    /// pc → 块下标
    pub pc_to_block: std::collections::HashMap<usize, usize>,
    /// 前驱块（包含异常边，用于支配与循环分析）
    pub preds: Vec<Vec<usize>>,
    /// 支配者集合（位集，doms[b] 含 b 自身与所有支配者）
    pub doms: Vec<Vec<usize>>,
    /// 后支配者（包含编号为 blocks.len() 的虚拟出口）
    pub postdoms: Vec<Vec<usize>>,
    /// 自然循环（header 块 → 循环）
    pub loops: std::collections::HashMap<usize, Loop>,
}

/// 构建 CFG；try_blocks 用于切分与覆盖记录
pub fn build(ins: &[crate::disasm::Ins], code_len: usize, try_blocks: &[TryBlock]) -> Cfg {
    use std::collections::BTreeSet;
    // leader 集合
    let mut leaders: BTreeSet<usize> = BTreeSet::new();
    leaders.insert(0);
    let mut pc_to_idx = std::collections::HashMap::new();
    for (i, x) in ins.iter().enumerate() {
        pc_to_idx.insert(x.pc, i);
    }
    for (i, x) in ins.iter().enumerate() {
        if let Some(t) = x.jump_target() {
            if t < code_len {
                leaders.insert(t);
            }
            // 任何跳转（条件/无条件）的下一条指令都是 leader：
            // 条件跳转存在 fall-through 分支，块必须在跳转处结束
            if x.meta.jump && i + 1 < ins.len() {
                leaders.insert(ins[i + 1].pc);
            }
        }
        if matches!(x.meta.mnemonic, "return" | "returnundefined" | "throw") && i + 1 < ins.len() {
            leaders.insert(ins[i + 1].pc);
        }
    }
    for tb in try_blocks {
        leaders.insert(tb.start_pc as usize);
        leaders.insert((tb.start_pc + tb.length) as usize);
        for cb in &tb.catch_blocks {
            leaders.insert(cb.handler_pc as usize);
        }
    }
    // 分块
    let leader_list: Vec<usize> = leaders
        .into_iter()
        .filter(|pc| pc_to_idx.contains_key(pc))
        .collect();
    let mut blocks = Vec::with_capacity(leader_list.len());
    let mut pc_to_block = std::collections::HashMap::new();
    for (bi, &start_pc) in leader_list.iter().enumerate() {
        let start = pc_to_idx[&start_pc];
        let end_pc = if bi + 1 < leader_list.len() {
            leader_list[bi + 1]
        } else {
            code_len
        };
        let end = ins
            .binary_search_by(|x| x.pc.cmp(&end_pc))
            .unwrap_or_else(|e| e);
        blocks.push(Block {
            start,
            end,
            start_pc,
            end_pc,
            succs: Vec::new(),
            ends_cond: false,
            try_of: Vec::new(),
        });
        pc_to_block.insert(start_pc, bi);
    }
    // 前驱/支配者/循环分析占位（在 succs 计算完成后填充）
    let n_blocks = blocks.len();
    let mut preds = vec![Vec::new(); n_blocks];
    // 后继
    for block in &mut blocks {
        let (start_pc, end_pc, end_idx) = {
            let b = &*block;
            (b.start_pc, b.end_pc, b.end)
        };
        let last = ins.get(end_idx.saturating_sub(1));
        let mut succs = Vec::new();
        let mut ends_cond = false;
        if let Some(l) = last {
            if l.jump_target().is_some() {
                let target = l.jump_target().unwrap();
                if let Some(&t) = pc_to_block.get(&target) {
                    if !l.mnemonic().starts_with("jmp") {
                        // 条件跳转：fallthrough 也是后继
                        ends_cond = true;
                        if end_idx < ins.len()
                            && let Some(&n) = pc_to_block.get(&ins[end_idx].pc)
                        {
                            succs.push(n);
                        }
                        succs.push(t);
                    } else {
                        succs.push(t);
                    }
                }
            } else if end_idx < ins.len()
                && !matches!(l.meta.mnemonic, "return" | "returnundefined" | "throw")
            {
                // fallthrough
                if let Some(&n) = pc_to_block.get(&ins[end_idx].pc) {
                    succs.push(n);
                }
            }
        }
        succs.sort_unstable();
        succs.dedup();
        let mut try_of = Vec::new();
        for tb in try_blocks {
            let t_start = tb.start_pc as usize;
            let t_end = t_start + tb.length as usize;
            let overlap = start_pc < t_end && end_pc > t_start;
            if overlap {
                for cb in &tb.catch_blocks {
                    try_of.push((tb.start_pc, tb.length, cb.handler_pc));
                }
            }
        }
        {
            let b = block;
            b.succs = succs;
            b.ends_cond = ends_cond;
            b.try_of = try_of;
        }
    }
    // 异常入口是真正可达的控制流边；正常后继仍单独保存供 if/else 发射。
    // 若忽略这条边，共享 finally 的入口会被误判为无前驱，进而打断自然循环。
    let mut analysis_succs: Vec<_> = blocks.iter().map(|b| b.succs.clone()).collect();
    for (bi, block) in blocks.iter().enumerate() {
        for (_, _, handler_pc) in &block.try_of {
            if let Some(&handler) = pc_to_block.get(&(*handler_pc as usize))
                && !analysis_succs[bi].contains(&handler)
            {
                analysis_succs[bi].push(handler);
            }
        }
    }
    let mut reachable = vec![false; n_blocks];
    let mut pending = if n_blocks == 0 { Vec::new() } else { vec![0] };
    while let Some(b) = pending.pop() {
        if !reachable[b] {
            reachable[b] = true;
            pending.extend(analysis_succs[b].iter().copied());
        }
    }
    // return 后的编译器胶水块不可向可达块贡献虚假的支配前驱。
    for (bi, successors) in analysis_succs.iter().enumerate() {
        if !reachable[bi] {
            continue;
        }
        for &s in successors {
            if !preds[s].contains(&bi) {
                preds[s].push(bi);
            }
        }
    }
    // 支配者（迭代数据流，入口 = 块 0）
    let mut doms: Vec<Vec<usize>> = Vec::with_capacity(n_blocks);
    for (b, reachable) in reachable.iter().enumerate() {
        if b == 0 || !reachable {
            doms.push(vec![b]);
        } else {
            doms.push((0..n_blocks).collect());
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for b in 1..n_blocks {
            if !reachable[b] {
                continue;
            }
            let mut new_dom: Option<Vec<usize>> = None;
            for &p in &preds[b] {
                new_dom = Some(match new_dom {
                    None => doms[p].clone(),
                    Some(cur) => cur
                        .iter()
                        .copied()
                        .filter(|x| doms[p].contains(x))
                        .collect(),
                });
            }
            let mut nd = new_dom.unwrap_or_default();
            if !nd.contains(&b) {
                nd.push(b);
            }
            nd.sort_unstable();
            nd.dedup();
            if nd != doms[b] {
                doms[b] = nd;
                changed = true;
            }
        }
    }
    // 后支配者必须从真实后继求交；布局上较后的多前驱块不一定是汇合点。
    let mut postdoms = vec![(0..=n_blocks).collect::<Vec<_>>(); n_blocks + 1];
    postdoms[n_blocks] = vec![n_blocks];
    loop {
        let mut changed = false;
        for b in (0..n_blocks).rev() {
            let succs = if blocks[b].succs.is_empty() {
                &[n_blocks][..]
            } else {
                &blocks[b].succs
            };
            let mut next = postdoms[succs[0]].clone();
            for &s in &succs[1..] {
                next.retain(|p| postdoms[s].contains(p));
            }
            next.push(b);
            next.sort_unstable();
            next.dedup();
            if next != postdoms[b] {
                postdoms[b] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // 自然循环：s → h 且 h 支配 s
    let mut loops: std::collections::HashMap<usize, Loop> = std::collections::HashMap::new();
    for s in 0..n_blocks {
        if !reachable[s] {
            continue;
        }
        for &h in &blocks[s].succs {
            if !doms[s].contains(&h) {
                continue;
            }
            // 自然循环体：从 s 反向收集直到 h
            let mut body = vec![h, s];
            let mut stack = if h == s { Vec::new() } else { vec![s] };
            while let Some(x) = stack.pop() {
                for &p in &preds[x] {
                    if p != h && !body.contains(&p) {
                        body.push(p);
                        stack.push(p);
                    }
                }
            }
            body.sort_unstable();
            body.dedup();
            // 出口：体内块跳向体外的目标
            let mut exits = Vec::new();
            for &bb in &body {
                for &succ in &blocks[bb].succs {
                    if !body.contains(&succ) && !exits.contains(&succ) {
                        exits.push(succ);
                    }
                }
            }
            exits.sort_unstable();
            // do-while 判定（JD arkdecompiler）：回边块的另一后继即出口
            let is_do_while = blocks[s]
                .succs
                .iter()
                .any(|&x| x != h && !body.contains(&x));
            if let Some(existing) = loops.get_mut(&h) {
                existing.body.extend(body);
                existing.body.sort_unstable();
                existing.body.dedup();
                existing.back_edges.push(s);
            } else {
                loops.insert(
                    h,
                    Loop {
                        header: h,
                        body,
                        back_edges: vec![s],
                        exits,
                        is_do_while,
                    },
                );
            }
        }
    }
    for lp in loops.values_mut() {
        lp.exits = lp
            .body
            .iter()
            .flat_map(|&b| blocks[b].succs.iter().copied())
            .filter(|s| !lp.body.contains(s))
            .collect();
        lp.exits.sort_unstable();
        lp.exits.dedup();
        lp.is_do_while = !blocks[lp.header].ends_cond || lp.body.len() == 1;
    }
    Cfg {
        blocks,
        pc_to_block,
        preds,
        doms,
        postdoms,
        loops,
    }
}
